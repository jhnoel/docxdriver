# Provenance-controlled quotation REPL

## Outcome

Add an opt-in Python REPL mode in which quoted content is a structured value,
not model-authored prose. The model chooses an exact source span and declares
editorial adaptations. A host resolver reads the source DOCX, checks every
selector case-sensitively, constructs the rendered text, and returns a
content-addressed provenance record.

The important integration rule is that a document compiler must consume the
structured resolved quote. Copying `QuoteResult.text` through a model message
is useful for inspection, but is not itself a provenance guarantee.

## Quotation lint policy

Quotation marks are reserved syntax in authored document content. The compiler
lints every ordinary text node before it renders structured inline nodes. It
detects straight and curly double quotes, guillemets, quote entities, Unicode
opening single quotes, and conservatively delimited straight single-quoted
phrases. Apostrophes are not flagged. Quotes inside parsed attributes of the
safe inline dialect are not text and are not flagged.

There are two structured ways to introduce quotation marks:

- `Quote(...)` means words attributed to a source and must pass source
  validation.
- `Term("quotation")` means a word or phrase is being mentioned rather than
  attributed. The host renders `“quotation”` and records `kind="term"` in the
  audit. `Term` cannot contain quotation marks or arbitrary nested markup.

An arbitrary `<style quote>` escape hatch is deliberately excluded: it would
let an agent disguise an unverified quotation as styling. Presentation can be
configured on `Term`, but the semantic classification remains explicit.

The quotation policy is host/session configuration, never a field the model
may downgrade. `quotation_policy="error"` blocks any unverified span.
`quotation_policy="comment"` allows one only when the host has attached an
open, protected Word comment bubble to that exact span. Comment mode is the
recommended interactive default because it preserves the text while making
uncertainty local and visible.

Each warning is deterministic over `(paragraph_id, selected_text, occurrence)`
and has a `qw1:sha256:…` identity. Its author, initials, open status, exact
anchor, warning text, and identity marker are all part of the invariant. A
candidate fails if a required warning is missing, duplicated, moved, edited,
resolved, re-authored, or stale. The warning marker is auditable metadata, not
an authorization secret.

Protected bubbles are not ordinary plan comments. The commit lifecycle is:

1. render the model-authored candidate without granting it ownership of audit
   comments;
2. classify every quotation-mark span as verified `Quote`, structured `Term`,
   or unverified;
3. host-side, remove/reconcile reserved audit comments and inject exactly one
   pristine open bubble for each unverified span;
4. reject any explicit model operation that deletes, resolves, or replies to a
   protected comment;
5. parse the final candidate again and require exact warning coverage before
   atomic write; and
6. perform the same invariant check on every later docxdriver commit.

The OOXML comment machinery is already implemented: the existing
`CommentAdd(at, select, occurrence, text)` operation provides the exact-span
anchor, and reply/status/delete operations already exist. Quote auditing should
reuse that implementation. The new work is orchestration around it: the host
runs a second, host-owned audit-comment pass after the model plan (so the audit
author is reserved and model-authored tracked changes keep their own author),
records the returned comment ids as protected, and verifies coverage on the
resulting candidate. This is not a new comment-format implementation.

If the existing comment anchor correctly fails closed—for example because the
span crosses a field, drawing, note reference, or existing comment milestone—
the quotation cannot satisfy comment policy and the document must be blocked.
There is no weaker document-level-warning fallback under this invariant.

Step 3 also handles indirect deletion: replacing a paragraph may destroy its
comment anchors, but the host runs after that replacement and reattaches
warnings to every surviving unverified quote. If the quotation itself is gone,
the stale warning is removed. The model therefore cannot make an unverified
quotation pass by manipulating comments.

Verified quotes and terms need persistent range records as well; otherwise a
later edit would see only flattened quotation marks and misclassify them.
Those records, warning identities, counts, and diagnostics live in the commit
audit manifest, while the package custom property is
`docxdriver:quotation-audit=verified|commented`. A mutation outside docxdriver makes
the prior package audit stale and requires a fresh review.

### Feature flag

The production Python review/commit host enables the protected-comment pass
only when:

```sh
DOCXDRIVER_QUOTE_AUDIT_COMMENTS=1 pi ...
```

The accepted truthy values are `1`, `true`, `yes`, and `on` (case-insensitive).
It is off by default, and the disabled path does not run quotation inspection
or alter candidate bytes.

When enabled, `DOCXDRIVER_QUOTE_PROVENANCE_POLICY` selects source admission:
`permissive` (the default) accepts any readable local DOCX and classifies its
origin as `unregistered_local`; `controlled` and `authoritative` are reserved
fail-closed stubs. A byte hash establishes stable quotation input, not source
authorship or authority. Future stricter policies must rely on host-issued
provenance receipts that the agent cannot mint merely by writing or copying a
file.

In the flagged integration, structured `Quote`, `Term`, and `Inline` values are
available in the same Python plan REPL and may be passed directly to operation
`with_` fields. Review resolves those nodes before the core sees the plan and
binds every quotation-source hash and trusted rendering into the compact commit
receipt. Commit re-reads every quotation source and rejects drift. The host
then scans the in-memory candidate, requires every reviewed rendering to occur
with exactly the reviewed multiplicity, and treats all remaining quotation-mark
spans conservatively as unverified. A second direct plan under author `docxdriver
quotation audit` deletes old reserved warnings and creates exactly one
`CommentAdd` for each unverified span. The twice-processed candidate is re-read
and coverage-checked before the existing atomic write. The commit notice
reports the number of warning bubbles.

Content without a paragraph id cannot carry the existing exact-span comment
operation. Raw quote marks there block; an unambiguous structured rendering can
pass because it needs no warning. Trusted structured ranges currently live in
the reviewed commit receipt, not a persisted package manifest. A later
independent commit therefore treats an older verified rendering as unverified
unless that plan supplies it as a structured node again. Persisting those
records in the package remains the next integration layer; until then the
enabled behavior prefers a false-positive warning over a silent quotation.

The final coverage check compares the physical comment locator to the exact
final-view quotation range. The current core may expand a selection to an
entire tracked insertion wrapper; the feature rejects that candidate instead
of accepting a broader highlight. Direct-mode edits and unchanged addressable
text support exact anchors today. Exact nested comment ranges inside tracked
revision wrappers are required before the flagged path can accept that case.

## Python surface

```python
matches = quote_find("source.docx", "The parties agree")

q = Quote(
    "source.docx",
    at="033873CC",
    select="The parties agree that the terms shall govern, and the terms shall prevail, and the terms shall bind, and the terms shall endure.",
    style="double",
)
q.omit("and the terms shall prevail, and the terms shall bind,")
q.bracket("The parties", "The Parties")
validated = quote_validate(q)

term = term_render(Term("quotation"))
# term.text is host-rendered as “quotation”; provenance.kind is "term"
```

`select` is a single contiguous, exact span. `occurrence` disambiguates
repeated spans. Adaptation selectors address the original selected span, not
an intermediate rendering, which makes coordinates stable and call order
irrelevant. Adaptations are:

- `omit(select, occurrence=1, marker="…")`: replace exact source text with
  either `…` or `...`.
- `bracket(select, replacement, occurrence=1)`: replace exact source text with
  a host-created `[replacement]`. Replacement text may not contain brackets.

Overlapping changes, missing occurrences, empty selectors, malformed markers,
and non-exact capitalization all fail closed.

`style` is `double` (the default), `single`, or `none`. The host supplies curly
quotation marks, so the agent never types quotation delimiters either.

## Resolved record

The host returns:

```text
quote_id             q1:sha256:<digest of the resolved record>
text                 host-rendered quotation, including quote marks
content              adapted inner content
text_sha256          digest of rendered UTF-8 text
source               cwd-contained source path
source_sha256        digest of the DOCX bytes
at                   stable paragraph id
paragraph_sha256     digest of paragraph plain text
span                 paragraph-relative start/end and occurrence
original             exact unadapted source span
changes[]            original offsets/text and host rendering
```

The paragraph id is not sufficient provenance by itself because ids may be
reused in a changed file. Source, paragraph, rendered-text, and complete-record
hashes make accidental drift visible and allow a consumer to re-resolve the
record.

## Compiler integration

The production Python plan REPL accepts quote values in operation `with_`
fields:

```python
InsertParagraph(at=anchor, position="after", with_=Inline([
    "The court explained: ",
    q,
]))
```

At review, the host resolves `$quote`, `$term`, and `$inline` nodes, stamps
source hashes and trusted renderings into the review receipt, and supplies
plain rendered text to the DOCX engine. It lints only ordinary text nodes, then
renders verified quotes and terms; linting the final flattened string would
incorrectly flag host-created marks. At commit, it re-reads every quotation
source and rejects the commit if any source hash changed. Exact candidate
multiplicity is also checked so a missing or ambiguous rendering fails closed.
Persisting the quote records inside the package for later audit export remains
future work.

The inline boundary also rejects duplicate punctuation immediately following a
structured quote that already owns terminal source punctuation. This was added
after a headless trial composed a valid quote followed by an extra period.

This is stronger than accepting a model-authored desired rendering and trying
to align it after the fact. Alignment around brackets and ellipses can be
ambiguous. Exact edit operations make every omission and substituted source
span explicit while keeping the Python syntax compact.

## Headless agent trial

The prototype was loaded as the only tool in a non-interactive `pi` run using
`openai-codex/gpt-5.4-mini`. The task requested one full paragraph, one middle
omission, and one bracketed capitalization change.

Observed flow:

1. `quote_find("contract.docx", "The parties agree")`
2. Construct `Quote(...)`, call `omit(...)`, `bracket(...)`, then
   `quote_validate(...)`.
3. Report the host-rendered text and source/paragraph hashes.

The agent completed the task in two tool calls with no validation repair and
correctly selected the full paragraph. The first prototype emitted only the
inner content; that trial prompted moving quote-mark style into the host-owned
object. The current rendering is:

```text
“[The Parties] agree that the terms shall govern, … and the terms shall endure.”
```

The trial confirmed that operation-based adaptations are easier to validate
than a free-form edited rendering. It also revealed that chat-level copying is
outside the trust boundary, motivating structured quote nodes and commit-time
source revalidation in the compiler integration above.

A follow-up headless trial used one automatic `Inline` boundary containing
plain text, a `Term`, and a source `Quote`. No manual lint call was requested.
The strict compiler returned:

```text
The word “quotation” differs from this verified excerpt: “The notice period is thirty days.”
status=verified, verified_quotes=1, terms=1, diagnostics=[]
```

The preceding trial had appended an extra period after the already-punctuated
source quote. A structural regression now rejects that boundary, and the
follow-up agent produced the clean rendering above.
