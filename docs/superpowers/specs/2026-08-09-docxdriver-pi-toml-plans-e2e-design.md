# docxdriver: typed commands and plans, Pi plan workflow, pagination, e2e harness

Date: 2026-08-09. Status: clean-sheet revision; backward compatibility is not
a goal.

## Goal

Give `docxdriver-core` one typed API that executes either an individual `Command`
or an atomic `Plan`:

- commands are convenient building blocks for native callers and CLI scripts;
- plans provide source binding, atomic multi-operation execution, and a
  preview/commit gate;
- Pi mutates existing DOCX files only through plans;
- TOML and JSON are representations of the same typed plan, not independent
  execution models.

Add deterministic `docx_read` pagination and an isolated real-Pi e2e harness
that tests agent workflow and recovery behavior on representative public USPTO
office-action documents.

## Architecture

```text
                         +-------------------+
CLI individual command ->|                   |
                         | typed core API    |-> CommandResult
JSON plan ---------------| Request          |
TOML plan -> parse ------|  Command | Plan   |-> PlanResult
                         +-------------------+
                                  |
                          one parsed WorkState
```

There is one implementation of every document operation. A plan folds typed
edit operations over one in-memory `WorkState`; an individual mutating command
uses the same implementation as a one-operation execution. No typed value is
compiled back into a legacy JSON command before execution.

The JSON/Wasm boundary is a serialization adapter over the Rust types. Raw
`serde_json::Value` dispatch is not a semantic API.

## Core typed API

`docxdriver-core` owns the public request, command, plan, result, and diagnostic
types:

```rust
enum Request {
    Command(CommandRequest),
    Plan(PlanRequest),
}

struct CommandRequest {
    command: Command,
    expected_source: Option<SourceHash>,
}

enum Command {
    Create(CreateCommand),
    Read(ReadCommand),
    Find(FindCommand),
    Edit(EditCommand),
}

struct EditCommand {
    author: String,
    change_mode: ChangeMode,
    op: EditOp,
}

struct Plan {
    base: SourceHash,
    author: String,
    change_mode: ChangeMode,
    ops: NonEmptyVec<EditOp>,
}

struct PlanRequest {
    plan: Plan,
    preview_key: Option<PreviewKey>,
}

enum ChangeMode {
    Track,
    Direct,
}
```

`expected_source` gives direct CLI commands an optional compare-and-swap
precondition. Plans always require `base`.

Core functions accept typed requests and return typed results plus optional
output bytes. Host concerns such as paths, cwd containment, clocks, and atomic
filesystem replacement remain outside core.

### Commands versus plans

A command executes once and, when mutating, may immediately produce output
bytes. It is intended for explicit native use and CLI scripting. It has no
plan aliases and no preview key.

A plan:

- is bound to the exact SHA-256 of the input DOCX;
- contains one or more edit operations;
- stops on the first rejected operation;
- emits no bytes unless every operation succeeds;
- emits bytes only on a commit call with the matching preview key;
- supports plan-local aliases created by earlier insert operations.

Pi never invokes mutating commands. Its edit surface always constructs or
loads a `Plan` and uses `Request::Plan`.

## Edit operation model

The same `EditOp` enum is used by plans and individual edit commands:

```rust
enum EditOp {
    ReplaceText { /* ... */ },
    ReplaceParagraph { /* ... */ },
    FormatText { /* ... */ },
    FormatParagraph { /* ... */ },
    InsertParagraph { /* ... */ },
    DeleteParagraphs { /* ... */ },
    SetPageMargins { /* ... */ },
    SetEvenAndOddHeaders { /* ... */ },
    CommentAdd { /* ... */ },
    CommentReply { /* ... */ },
    CommentSetStatus { /* open | resolved */ },
    CommentDelete { /* ... */ },
    RevisionSettle { /* id | all, accept | reject */ },
}
```

Use sum types where fields are mutually exclusive:

- `insert_paragraph` uses `at` plus `position = before | after`, rather than
  separate `before` and `after` fields;
- `revision_settle` uses `target = <revision-id> | all` and
  `action = accept | reject`;
- `comment_set_status` uses `status = open | resolved`.

Paragraph addresses are typed as either a native paragraph ID or a plan-local
alias. Individual commands reject aliases because they have no plan binding
scope. Aliases match `[A-Za-z_][A-Za-z0-9_]*` and are case-sensitive.

### Public vocabulary and normalization

The public JSON and TOML representations use the serialized Rust field names:

- discriminator `op` and snake_case fields throughout;
- `justify`, not the OOXML value `both`;
- uppercase eight-digit paragraph IDs;
- uppercase `#RRGGBB` colors;
- decimal comment and revision IDs;
- one documented public unit for linear measurements: points.

Core normalizes point measurements to integer twentieths of a point. Inputs
must be multiples of `0.05`; canonical projections use the shortest decimal
point spelling. OOXML twips and camelCase engine names never appear in the
public plan model.

Line spacing is a tagged value rather than an ambiguous scalar plus line rule:

```text
{ mode = "multiple", value = 1.15 }
{ mode = "exact", value = 12 }
{ mode = "at_least", value = 12 }
```

Content-bearing strings—including selections, replacements, comments,
authors, and style IDs—are preserved exactly. Unknown fields and variants are
rejected. The adapters do not accept additional aliases such as `both`,
`single`, `double`, `36pt`, or `0.5in`.

## JSON and TOML plan adapters

JSON operations and TOML deserialize into the same `Plan` types and call the
same validation and normalization code.

For Pi's inline JSON path, the host supplies `operations`, optional `author`,
and optional `change_mode`; core constructs the plan using the actual input
SHA-256 as `base`. Defaults are owned by core:

```text
author = "docxdriver"
change_mode = "track"
```

A TOML plan contains explicit plan metadata and ordered `[[ops]]` tables:

```toml
base = "sha256:<64 lowercase hex>"
author = "docxdriver"
change_mode = "track"

[[ops]]
op = "replace_text"
at = "1234ABCD"
select = "old"
with = "new"
```

Canonical JSON is used for plan identity. Canonical TOML is a deterministic
human-editable projection. Both projections include resolved defaults and use
stable field ordering. These invariants must hold:

```text
Plan -> canonical JSON -> Plan == Plan
Plan -> canonical TOML -> Plan == Plan
```

TOML parsing retains source spans so a syntax or semantic diagnostic can point
to the relevant field. There is no separate TOML plan model or TOML-to-command
compiler.

## Preview and commit

Evaluating a valid plan always runs the complete atomic fold in memory.

- Without `preview_key`, a successful fold returns a preview report and key
  but no DOCX bytes.
- With `preview_key`, core first verifies the key against the current source,
  canonical plan, and edit protocol version. A match runs the same fold and
  returns output bytes. A mismatch rejects before executing operations.
- An operation rejection stops the fold and returns no bytes in either mode.

The key is an intent fingerprint, not an authentication token:

```text
p1:sha256(
  "docxdriver-plan-preview" || 0x00 ||
  EDIT_PROTOCOL_VERSION || 0x00 ||
  base_source_hash || 0x00 ||
  canonical_plan_json
)
```

The plan already contains the source hash, but the domain-separated layout is
kept explicit for review and golden-vector testing. Repairs, allocated IDs,
alias bindings, revision counts, timestamps, and output bytes are deterministic
results or execution metadata and are not separately hashed.

A preview key proves that the same source and semantic plan are being
submitted. Proving that a preview call actually occurred would require stored
state or a secret-authenticated token and is outside this local safety model.

Revision and comment timestamps are commit metadata. They may differ from the
preview simulation and do not affect semantic plan identity.

## Result and diagnostic contract

Command and plan execution have separate result enums because a direct command
is neither previewed nor committed:

```text
CommandResult:
  outcome: completed | rejected
  source?
  result?
  diagnostic?          # rejected only
```

Plan results use mutually exclusive outcomes rather than combinable status
flags:

```text
outcome: previewed | committed | rejected
source
plan?                  # normalized structured Plan when construction succeeded
canonical_toml?        # host-facing; Pi need not include it in visible text
preview_key?           # previewed only
report?:
  completed            # number of successful operations
  stopped_at?          # zero-based rejected operation index
  ops[]                # attempted operations only
    index
    op
    outcome: applied | rejected
    summary
    affected?          # bounded [{ para_id, markup }]
    context_truncated?
  repaired_ids[]
  allocated_ids[]
  aliases
diagnostic?:            # rejected only
  code
  message
  path?                 # e.g. ops[2].color
  span?:
    line
    column
    end_line?
    end_column?
```

Parse and validation failures may omit `plan` and `report`. Source, preview-key,
and operation refusals return `outcome = rejected`. The diagnostic code
distinguishes them; redundant fields such as `source_matched`, `failed`,
`not_attempted`, and a separate repair count are omitted.

Expected refusals are typed `rejected` results. On Pi they are normal tool
results with `isError = false`. Filesystem failures, Wasm initialization
failures, panics, and unexpected engine failures throw and become tool errors.

## Pi surface

### `docx_edit`

```text
docx_edit(
  path,
  plan:
    { operations, author?, change_mode? }
    | { file },
  preview_key?
)
```

- The `plan` union makes invalid combinations unrepresentable.
- Inline operations use the actual DOCX source and core defaults.
- A plan file owns `base`, `author`, and `change_mode`.
- No preview key means preview; a key means commit.
- The Pi extension calls only the core plan API, never a mutating command.
- Preview never writes the DOCX or a TOML sidecar.
- Inline operations do not implicitly create or replace files. When a durable
  TOML artifact is useful, the agent explicitly authors it with its normal file
  tool, then calls `docx_edit` with `{ file }`.

The normal short workflow is:

```text
docx_read(path)
docx_edit(path, plan = { operations = [...] })             # preview
docx_edit(path, plan = { operations = [...] }, preview_key) # commit
```

The editable-file workflow is:

```text
write(plan.toml, authored TOML)
docx_edit(path, plan = { file = "plan.toml" })
edit(plan.toml, correction)
docx_edit(path, plan = { file = "plan.toml" })
docx_edit(path, plan = { file = "plan.toml" }, preview_key)
```

Whitespace- and comment-only TOML edits preserve the key when they deserialize
to the same normalized plan. Any semantic change requires a new preview.

### `docx_read`

All document reading and inspection uses one tool:

```text
docx_read(
  path,
  kind?: document | styles | comments | revisions,
  view?,
  page?
)
```

- `kind` defaults to `document`.
- `view = markup | final | original` and `page` apply only to `document`.
- `styles`, `comments`, and `revisions` return typed listings.
- Every response includes `path`, `kind`, and the full source SHA-256.
- Document responses additionally include `view`, `page`, `pages`,
  `budget_bytes`, and `oversized`.

Document pagination packs complete top-level rendered blocks into a fixed
32 KiB UTF-8 content budget. A single oversize block occupies one oversize page
and is never split, preserving paragraph anchors. Page numbers are positive
integers; omission means page 1. Invalid and out-of-range pages return
structured input diagnostics.

### Remaining Pi tools

- `docx_create` creates a new document, writes atomically, and refuses to
  overwrite an existing path.
- `docx_find` searches document text and returns matching paragraph IDs and
  bounded context.
- `docx_help` returns the generated plan and read-surface reference.

The Pi surface is therefore:

```text
docx_create
docx_read
docx_find
docx_edit
docx_help
```

There are no `docx_style`, `docx_comment`, `docx_revision`,
`docx_edit_batch`, immediate mutation, or `dry_run` Pi tools.

## CLI surface

The CLI exposes both branches of the typed core API.

Plan execution provides the same preview/commit protocol as Pi:

```text
docxdriver plan document.docx plan.toml
docxdriver plan document.docx plan.toml --commit <preview-key>
```

Every `EditOp` is also available as an individual CLI operation for scripting.
Individual operations execute through `Request::Command` and may write
immediately:

```text
docxdriver replace-text document.docx --at 1234ABCD \
  --select old --with new --author docxdriver --change-mode track
```

Mutating CLI commands support:

- `--dry-run` to execute without writing;
- `--expect-source sha256:<hex>` for optional source compare-and-swap;
- `--output PATH`, defaulting to atomic replacement of the input;
- the same normalized result and diagnostic schema in JSON output mode.

CLI read, find, and create commands likewise map directly to typed `Command`
variants. The CLI owns argument parsing, path I/O, human formatting, and exit
codes; it does not own plan validation, operation compilation, preview keys, or
execution policy.

## Schema and documentation generation

Derive JSON Schema from the Rust request types and commit the generated schema
artifact consumed by the TypeScript package and Pi tool definitions. Generate
TypeScript request/result types and the operation reference from the same
schema.

A drift test regenerates the artifacts and requires a clean diff. Do not
maintain separate command-name manifests, TypeBox operation unions, and prose
operation counts by hand.

## Write safety

Core never writes paths. CLI and Pi hosts implement the same write protocol:

1. resolve the target beneath the allowed cwd and reject lexical or symlink
   escapes;
2. immediately before a commit write, re-read the target and verify its SHA-256
   still matches the bytes passed to core;
3. create a unique temporary file in the target directory with exclusive
   creation;
4. write and flush the bytes;
5. atomically rename the temporary file over the target;
6. clean up the temporary file after any failure.

A pre-write source mismatch is a structured rejection. Filesystem errors are
infrastructure failures. `docx_create` uses exclusive destination creation so
it cannot overwrite an existing file.

## Core and package changes

1. Add typed request, command, plan, result, and diagnostic modules to core.
2. Change operation implementations to accept typed arguments instead of raw
   JSON values.
3. Implement one atomic plan evaluator over the typed operations.
4. Add JSON and spanned TOML plan adapters in core.
5. Add canonical JSON/TOML projection and the `p1` preview-key algorithm.
6. Replace raw command dispatch, generic operation lists, `batch`, and
   `typed_plan` with the typed `Request` boundary.
7. Make the CLI a thin adapter over both `Request::Command` and
   `Request::Plan`.
8. Generate Wasm/TypeScript schemas and types from the core model.
9. Rewrite `docxdriver-pi` around plan-only editing, unified reading, pagination,
   and safe host I/O.

Because compatibility is not a goal, remove obsolete code, types, fixtures,
documentation, and migration errors rather than retaining aliases or rejection
paths for old request shapes.

## Fixtures

Commit two representative public USPTO office-action DOCX files with a
manifest containing application number, document code, public source URL,
retrieval date, SHA-256, and expected smoke-read metadata. Fixture setup checks
the hashes.

Use small generated documents for exact operation cases, pagination boundaries,
oversize blocks, and I/O failure injection. These cases do not require more
large public binaries.

## Isolated real-Pi e2e harness

Location: `packages/docxdriver-pi/test/pi-e2e/`.

Each task copies its fixtures into a fresh temporary cwd and starts Pi with
only the extension, skill, context, and five DOCX tools enabled. The harness
records the model, prompt version, JSONL events, stdout, stderr, exit status,
turn count, wall time, tool calls, structured rejections, and infrastructure
errors.

The live suite tests agent behavior, not exhaustive engine conformance:

1. read, preview, and commit a single inline JSON edit;
2. preview and commit a multi-operation plan with an alias;
3. diagnose and repair malformed or invalid TOML;
4. recover from a stale source or preview key;
5. use pagination to find and edit content beyond page one.

Verifiers inspect the final DOCX through core and enforce only safety-critical
tool sequencing, such as requiring a successful preview before commit. They do
not reject an alternative valid workflow merely because it uses fewer calls.

Each task has a five-minute wall timeout and a 12-turn cap. Live tests require
`PI_E2E_LIVE=1` and an explicit model. Normal test commands remain offline.
Results are written under a gitignored timestamp-and-model directory.

Atomic-write failure injection, path containment, every operation variant,
every read kind, and pagination boundary behavior are deterministic offline
tests rather than live-model tasks.

## Validation

1. Core tests:
   - every `Command` and `EditOp` variant through typed and JSON boundaries;
   - every edit operation as both an individual command and a plan entry;
   - JSON/TOML round-trip equality and strict unknown-field rejection;
   - source mismatch, preview, commit, stale key, and atomic stop;
   - aliases, paragraph-ID repairs, and deterministic inserted IDs;
   - fixed `p1` golden vectors;
   - structured field paths and TOML spans.
2. CLI tests:
   - individual operation scripting, dry run, expected source, output path,
     and atomic replacement;
   - plan preview/commit and JSON result parity with core.
3. Wasm and TypeScript tests:
   - generated schema/type drift;
   - typed command and plan calls;
   - byte/result transport.
4. Offline Pi tests:
   - exact five-tool registration surface;
   - plan-only mutation enforcement;
   - unified read kinds and pagination;
   - expected rejection versus infrastructure-error handling;
   - path containment, pre-write re-read, exclusive create, and atomic replace.
5. One explicitly configured live-Pi behavioral run.
