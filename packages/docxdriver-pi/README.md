# docxdriver-pi

Pi extension for safe, plan-based DOCX workflows. Its default Pi entry point
is the experimental Python-plan REPL: it exposes exactly one `python` tool,
with validating review, bounded edit neighborhoods, and a compact-key commit.
The five-tool
surface remains available as an explicit alternate entry point.

## Five-tool behavior

`docx_edit` accepts either an inline plan (`operations`, optional `author` and
`change_mode`) or `{ file }` naming an authored TOML plan. Without
`preview_key` it evaluates the complete plan and returns a deterministic
preview report. Passing the returned key commits only if the source has not
changed. Expected plan/source/validation refusals are structured normal tool
results; filesystem and engine failures remain tool errors.

`docx_read` is the one read surface. `kind` is `document` (default), `styles`,
`comments`, `revisions`, or `assets`. Document reads return the complete HTML
projection with semantic metadata. Full-document context is the default. The
five-tool response includes the source SHA-256; Python plans bind it in the host.

The host resolves every path below Pi's cwd, rejects symlink escapes, re-reads
the source immediately before commit, uses an exclusive temporary file and
`fsync`, then atomically replaces the target. Creation is exclusive and never
overwrites.

## Portable package contents

`docxdriver-pi` vendors the generated core runtime in its own `wasm/` directory
(`docxdriver.js` and `docxdriver_bg.wasm`) and the typed operation schema in
`schema/`. It has no `file:../docxdriver` dependency: an installed tarball runs
without a sibling monorepo checkout or a separately installed `docxdriver`
package. Pi, Monty, and TypeBox remain ordinary npm dependencies.

From a source checkout, install Rust with rustup and `wasm-pack`, then run
`npm ci` and `npm run build` in this package. The build generates the WASM
runtime and JavaScript outputs; these artifacts are ignored by Git.

## Python plan REPL (default, experimental, V1)

One alternate entry point exposes the same persistent, sandboxed Python REPL
over a single authoring model: the Python-authored typed plan. It is
experimental but is the default entry point for this package. It registers
exactly one `python` tool; it does not load the five-tool surface alongside it.

```bash
pi --no-builtin-tools --tools python \
  -e ./packages/docxdriver-pi/dist/python-plan-extension.js \
  --skill ./packages/docxdriver-pi/skills/docx-pi-repl
```

Enable the conservative protected-comment quotation audit for this plan REPL
with `DOCXDRIVER_QUOTE_AUDIT_COMMENTS=1`. It is off by default. The host comments
every detected quotation in addressable final-view body text, protects those
comments from model-authored mutation, and blocks when exact comment anchoring
is impossible. The same REPL exposes `Quote`, `Term`, `Inline`, `quote_find`,
and `quote_validate`: structured objects can be passed directly to operation
`with_` fields, are resolved during review, and bind source hashes to the
commit key. The `docx-pi-repl` skill documents exact selectors, bracket and
ellipsis syntax, markup constraints, and current fail-closed limitations.
`DOCXDRIVER_QUOTE_PROVENANCE_POLICY` defaults to `permissive`, which accepts local
sources but makes no origin or authority claim. The reserved `controlled` and
`authoritative` values currently fail closed at review; they are interface
stubs for future host-issued source receipts.

## Loading the five-tool plan API

The original plan API is isolated from the REPL and can be loaded explicitly:

```sh
pi --no-builtin-tools --tools docx_create,docx_read,docx_find,docx_edit,docx_help \
  -e ./packages/docxdriver-pi/dist/index.js \
  --skill ./packages/docxdriver-pi/skills/docx-pi
```

It exposes `docx_create`, `docx_read`, `docx_find`, `docx_edit`, and
`docx_help`; all mutations use the core typed Plan API. `docx_edit` evaluates
a plan first and commits only with the returned deterministic preview key.

The V1 surface gives the agent `Plan(operations=[ReplaceText(...), ...])`
dataclasses that serialize to the exact core typed `Plan`; the agent authors
typed operations directly. Version 2 (raw projection string) and Version 3
(structured document model) are deprecated and archived under
`experimental/python-repl-surfaces-v2-v3/` as historical research only — not
built, not published, not imported by production code.

V1 uses a host-owned compact commit-key protocol instead of exposing the
deterministic core preview key. `docx_review(path, plan)` validates the complete
plan, returns one result containing `commit_key` (`k1:` plus 32 lowercase hex
characters) and bounded per-operation `edits` with affected markup and edit
neighborhoods in context. Review never writes the target. In a separate later
Python execution, `docx_commit(review.commit_key)` commits the immutable plan
and target bound by that key; it takes neither a path nor a duplicate plan.

A changed plan requires a new review. Source/path/candidate mismatches, reset,
shutdown, and key eviction invalidate the key; replay reports stable commit-key
diagnostics. Infrastructure failure before the write keeps the key retryable,
while a completed write consumes it. Review context and the retained key record
are bounded, and the key never exposes the deterministic core preview key. All
mutations go through the selected op subset (`replace_text`,
`replace_paragraph`, `format_text`, `format_paragraph`, `insert_paragraph`,
`delete_paragraphs`, `replace_equation`, `delete_equation`, header/footer chrome, `set_even_and_odd_headers`, and
`comment_*`; tracked mode by default), and every write is atomic with an
immediate pre-write source recheck.

## Resource limits

Canonical equations use presentation MathML, for example
`<math display="inline"><msup><mi>x</mi><mn>2</mn></msup></math>`.
Display equations use `display="block"`. `ReplaceEquation` accepts `mathml`;
`DeleteEquation` remains unchanged. Legacy `<equation>` input is supported for
compatibility, while reads always return native HTML and MathML. Equation
address records avoid duplicating MathML already present in the complete HTML.
The UI package requests `document_ui` for browser source maps and CSS; see the
[HTML package](../docxdriver/README.md) and [parity matrix](../../docs/html-projection-parity.md).

Host allocations and callbacks are bounded by named limits
(`HostLimits` / `DEFAULT_HOST_LIMITS` in `src/python-host.ts`), enforced
before expensive canonicalization/render/engine calls where possible:

| Limit | Default | Enforced at |
| --- | --- | --- |
| DOCX input bytes (`maxDocxBytes`) | 16 MiB | size-checked read in read/find/review/commit; create html |
| Find results (`maxFindResults`) | 1000 | `docx_find` result records |
| Plan operations (`maxPlanOps`) | 2000 | plan digest/derivation before canonicalization |
| Canonical plan bytes (`maxCanonicalPlanBytes`) | 512 KiB | review/commit before the engine |
| Per-edit context bytes (`maxEditContextBytes`) | 8 KiB | review result context entries |
| Aggregate review result bytes (`maxReviewResultBytes`) | 256 KiB | review result |
| Per-key retained bytes (`commitKeyMaxBytes`) | 4 MiB | review: rejected outright, never evicts the store |
| Store retained bytes (`storeMaxBytes`) | 8 MiB | `CommitKeyStore` deterministic FIFO byte eviction |
| Store records (`storeMaxRecords`) | 64 | FIFO count eviction |
| Python `print()` bytes (`maxPrintBytes`) | 1 MiB | `CollectString` |
| Code input (`maxCodeBytes`) | 256 KiB | extension, before any runtime work |
| Generic value render (`maxValueRenderBytes`) | 8 KiB | final-expression rendering (+ marker); complete document output bypasses it |
| Control budget (`controlBudgetBytes`) | 16 KiB | `renderExecution`: control output first, tail-truncated |
| Control lines (`controlMaxLines`) | 500 | `renderExecution` |
| Host callback duration (`maxHostCallbackMs`) | 30 s | per-callback deadline; commit exempt after `markCommitting` |

Control output (reset notice, cancellation marker, host notices with compact
commit keys and commit outcomes) is rendered FIRST under the reserved control
budget, so Python stdout — even a single line larger than the whole output
budget — can never hide review/commit output (production invariant 9). The
commit-key store is byte-accounted (`retainedBytes`), evicts deterministically
FIFO with stable `commit key expired` tombstones, and rejects a single
oversized key record rather than evicting everything.

Cancellation honors the Pi tool `AbortSignal` at entry, after runtime
creation, in the REPL drive loop (before/after host callbacks), and at commit
checkpoints. A suspended feed is unwound via `resumeError`, never abandoned.
Cancellation before rename leaves the target untouched and the commit key
retryable; cancellation observed only after rename consumes the key and reports
`committed: N ops — cancellation arrived during commit; write completed`.
Synchronous spans (WASM, rename, fsync) are cooperative and non-preemptible.
If worker replacement or prelude reload fails after a crash, the runtime is
discarded, the generation is bumped, commit keys are invalidated, and the next
call creates fresh state.

```sh
npm test                          # build + deterministic offline tests
PI_E2E_LIVE=1 PI_E2E_MODEL=opencode-go/deepseek-v4-flash \
  node test/pi-e2e/repl-live.mjs  # opt-in live-agent trials (V1 REPL)
```

The live harness runs each manifest task on fresh fixture copies
(PI_E2E_SURFACES/PI_E2E_TASKS/PI_E2E_REPS select subsets; `PI_E2E_SURFACES`
accepts only `plan` — the V1 typed-plan surface; the full matrix is 10 tasks
x 3 reps) and writes metrics under `test/pi-e2e/results/`. A trial succeeds
only when one successful review returns bounded edit context and a compact
key, exactly one key is consumed, the exact requested edit is verified in the
final document (package parts byte-identical for task 10), and the run was not
terminated by the turn cap or timeout.

Tasks 8 and 9 exercise the commit gate with interference between review and
the commit attempt. Task 8's external file tamper IS harness-injected (the
harness rewrites the fixture the moment a review key appears). Task 9 mutates
the Python plan after review; commit must still use the immutable reviewed
plan captured behind the key. Either way the commit gate is exercised
end-to-end and the verifier asserts the documented result.

## Development

```sh
cd packages/docxdriver-pi
npm install
npm test                 # build + deterministic offline tests
PI_E2E_LIVE=1 PI_E2E_MODEL=gpt-5.6-luna npm run test:e2e:live
```

The opt-in live harness copies fixtures into a fresh temporary cwd and records
JSONL events, tool calls, structured refusals, and infrastructure errors under
`test/pi-e2e/results/` (ignored by git; tracked archive reports hold aggregate
summaries only). It never runs as part of `npm test`. `npm run test:e2e:live`
runs the five-tool behavioral suite (`test/pi-e2e/run.mjs`); the V1 REPL
trials run via `node test/pi-e2e/repl-live.mjs`.

Default reads provide one full HTML projection, including headers, footers,
notes, revisions and native MathML. They retain comment bodies and exact anchors,
image placements and asset mappings, and exact source selection text where
rendered text is ambiguous. UI byte coordinates and the general stylesheet stay
in the explicit UI read. Full reads never default to selected sections.

Repeated styles are defined once in compact HTML when economical. Images use
short aliases mapped to canonical asset URLs and package parts; their original
package path stays inline to distinguish occurrences. Comments keep text,
replies, status and exact selectors without repeating thread IDs or dates.
`selection_space` declares the selectors' revision view and units once.
Comment markers alone do not duplicate paragraph text.

Complete document values and printed HTML bypass generic display caps, including
values saved in earlier Python calls. Identical printed and returned document
values are delivered once. Runtime memory and print collection limits remain
explicit errors; return `.markup` as an expression for large values. Preview
retains affected context; commit returns a concise receipt without repeating
context, authored plans or binary data.

## Inspecting document formatting

Document reads keep CSS declarations out of `ReadResult.markup`. Paragraphs
retain their exact DOCX style ID in `data-docx-style`, including headings and
paragraphs using the document's default style. `result.styles` maps style IDs
to resolved CSS (including inherited properties); `result.css` maps the short
`data-docx-css` references to effective presentation declarations, including
direct formatting. These references are local to each read.

The default REPL display shows catalog counts, so definitions are read only
when requested:

```python
result = docx_read("document.docx")
print(result)
print(result.styles["Heading1"])
print(result.css)
```

`docx_read("document.docx", kind="styles")` still returns the DOCX style
metadata listing (names, types, and inheritance). CSS is a projection of the
supported formatting properties; it is not a lossless replacement for OOXML.
