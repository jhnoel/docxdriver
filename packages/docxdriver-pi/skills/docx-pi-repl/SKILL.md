---
name: docx-pi-repl
description: Author typed DOCX plans through the docxdriver-pi Python REPL. Use when the python tool is available for reading, finding, reviewing, and committing DOCX edits.
---

# DOCX Python REPL

One `python` tool. Do not unzip OOXML or mutate `.docx` with generic file tools.

## Control loop

Persistent sandbox. Host functions and operation dataclasses are already in
scope.

1. `docx_read(path)` / `docx_find(path, query)` to locate paragraphs. Address by
   the markup `id` (uppercase eight-digit hex) or a `$name` alias.
2. Build `Plan(author=..., change_mode="track", operations=[...])`.
3. `review = docx_review(path, plan)`. In an `if review is not None:` block,
   inspect `review.edits` and its commit key. Blocked reviews return `None`;
   repair the plan object and review again.
4. In a **later** Python call, `docx_commit(review.commit_key)`. Same-execution
   commit is rejected. A changed plan or source needs a new review.

Call `docx_help()` for generated signatures. Python keywords: `with_` / `as_`
map to schema keys `with` / `as`.

```python
plan = Plan(author="Pi", change_mode="track", operations=[
    ReplaceText(at="2673269E", select="thirty days", with_="sixty days"),
    SetEvenAndOddHeaders(even_and_odd=True),
    CommentAdd(at="2673269E", select="sixty days", text="please confirm"),
])
review = docx_review("contract.docx", plan)
if review is not None:
    print(review.edits)
    print(review.commit_key)
# later python call:
# if review is not None:
#     docx_commit(review.commit_key)
```

## Host functions

- `docx_create(path, html)`
- `docx_read(path, view="markup", kind="document")` → full `ReadResult`
- `docx_read(path, kind="comments" | "styles" | "revisions" | "assets")` → dict
- `docx_find(path, query, ignore_case=False)` → match records with `id` / `text`
- `docx_help(topic=None)`
- `docx_review(path, plan)` → `ReviewResult(commit_key, edits)` or `None`
- `docx_commit(commit_key)` — key owns the target file and the reviewed plan

`ReadResult` has `markup`, `equations`, `selection_space`, `selections`, `comments`,
`assets`, and `comments_error`. Return it as an expression to inspect all fields
without printing duplicate copies. `.markup` is complete renderable HTML5 with
native MathML. Return `.markup` as an expression when only the document HTML is
needed. Document results bypass generic display truncation. A printed result is
still subject to the explicit runtime print memory limit.

`re` is pre-imported. Use documented fields and `docx_help()`; the sandbox does
not expose `dir`, `getattr`, or `hasattr`. Guard `ReviewResult | None` in every
execution before accessing its fields. A snippet that fails type checking does
not execute its assignments; repair it before using those variables later.

Plans stay bound to markup-view paragraph ids even if you
read `view="final"` or `"original"` for analysis.

## Operations

Equations: inspect `docx_read(path).equations`, then use
`ReplaceEquation(at=eq["at"], equation=eq["equation"], mathml="<math><mfrac><mi>a</mi><mi>b</mi></mfrac></math>")`.
Omit `equation` for a paragraph with one equation. MathML compiles directly to Office Math; display mode is preserved. Settle existing revisions before re-editing.
Use `DeleteEquation(at=eq["at"], equation=eq["equation"])` to remove an
equation without rewriting the paragraph. In tracked mode, accept or reject
the deletion explicitly.

In HTML, use native `<math display="inline">` or `<math display="block">` with
presentation MathML operands. No LaTeX is required.

Text/paragraph: `ReplaceText`, `ReplaceParagraph`, `FormatText`,
`FormatParagraph`, `InsertParagraph`, `DeleteParagraphs`.

Header/footer: `SetHeader`, `SetFooter`, `ClearHeader`, `ClearFooter`
(`section` default 1, `kind` `default`/`even`/`first`).
`SetEvenAndOddHeaders(even_and_odd=True)` enables distinct even-page chrome so
`kind="even"` applies to even pages; `False` turns it off.

Comments:

- `CommentAdd(at, text, select=None, occurrence=None)` — whole paragraph, or pin
  one exact `select` span (`occurrence` is 1-based).
- `CommentReply(comment_id, text)`, `CommentSetStatus(comment_id, status)`,
  `CommentDelete(comment_id)` — `comment_id` is a decimal string. After add, the
  review `summary` includes the new id (`comment {id} anchored…`). Status is
  `open` or `resolved`.

`InsertParagraph` also accepts `table:N` anchors from body-level
`<table data-docx-table="N">` markup and may set `as_="$name"` for later ops.

Replacement content accepts safe inline HTML: `<strong>`, `<em>`, `<b>`, `<i>`,
`<u>`, `<s>`, `<sup>`, `<sub>`, `<a>`, `<br>`, styled `<span>`, and native `<math>`.
Do not include block wrappers, images, notes, fields, comments or revision tags.
HTML5 entities and attribute syntax are supported. Use `<br>` for explicit line
breaks. Unsupported CSS, unsafe URLs and scripts return diagnostics.

Blocked reviews, stale keys, unknown fields, and rejected ops are normal.

## Quotation audit (feature flag)

When the host was launched with `DOCXDRIVER_QUOTE_AUDIT_COMMENTS=1`, every
quotation-mark span in final-view addressable body text is conservatively
treated as unverified. On commit, the host attaches an exact, open Word comment
under the reserved author `docxdriver quotation audit`; the commit summary reports
`quotation audit warnings: N`. Do not delete, reply to, or resolve those
comments. The host rejects direct attempts and reconstructs the protected set
after other edits.

The accepted truthy flag values are `1`, `true`, `yes`, and `on`, ignoring
case. Any other value leaves the audit disabled and does not alter output.

Source admission is separately selected with
`DOCXDRIVER_QUOTE_PROVENANCE_POLICY`. It is read only when the audit feature is
enabled:

- `permissive` (default) accepts any readable workspace DOCX. It proves that
  the rendered quotation matches the hash-bound bytes, but does **not** prove
  who created those bytes or whether the source is authoritative. Such quote
  results report `provenance_tier="unregistered_local"`.
- `controlled` and `authoritative` are reserved, fail-closed stubs. Selecting
  either blocks review with a not-implemented diagnostic. Do not claim that
  either policy currently admits user uploads, downloads, or approved
  databases.

Renaming, copying, or hashing a file does not establish its origin. Under the
current permissive policy, an agent-authored source cannot be distinguished
reliably from another unregistered local file.

In this mode, `Quote`, `Term`, and `Inline` are part of this same REPL. Put the
structured object directly in an operation's `with_` field. Review resolves it
to engine text, binds every quotation source hash into the commit key, and
commit re-reads those sources before writing. The audit recognizes those exact
reviewed renderings and does not comment them. Never copy `QuoteResult.text`
into a plain string; that discards the structured boundary and is treated as an
unverified quotation.

An unverified quotation added inside a tracked insertion can fail closed when
Word expands the physical comment range beyond the exact quotation; use
`change_mode="direct"` when raw quotation text must be retained with a warning.
Raw quotation marks in headers, footers, or other content without an
addressable paragraph id block because an exact comment cannot be attached.
Structured renderings are accepted there only when the reviewed rendering is
unambiguous in the committed candidate.

The current commit receipt owns the structured range records. They are not yet
persisted as a package manifest, so a later independent commit sees an older
verified quotation conservatively as unverified unless that plan introduces
the rendering through a structured node again.

## Structured quotations

1. Use `quote_find(source, query)` to obtain the source paragraph `id` and
   exact text.
2. Create `Quote(source, at=id, select=exact_span, occurrence=1)`. The span is
   contiguous, exact, case-sensitive source text.
3. Optionally call `omit(exact_text, occurrence=1, marker="…")` or
   `bracket(exact_text, replacement, occurrence=1)`. Selectors refer to the
   original span; overlaps fail. Omission markers are only `…` or `...`, and
   bracket replacements are nonempty and cannot contain brackets.
4. Compose ordinary text, `Term(...)`, and `Quote(...)` inside `Inline([...])`,
   and pass that object directly as `with_`.

```python
matches = quote_find("source.docx", "The tenant must")
q = Quote("source.docx", at=matches[0]["id"], select=matches[0]["text"])
q.omit(" promptly")
q.bracket("tenant", "Tenant")

plan = Plan(author="Pi", change_mode="direct", operations=[
    ReplaceParagraph(
        at="2673269E",
        with_=Inline(["The term ", Term("quotation"), " introduces ", q]),
    ),
])
review = docx_review("output.docx", plan)
```

`Quote.style` is `double` (default), `single`, or `none`; the host supplies the
curly delimiters. `Term` is for a word or phrase being mentioned rather than
attributed and accepts `double` or `single`. It must be nonempty and cannot
contain quote marks or nested markup. Plain parts of `Inline` are linted, and
duplicate punctuation after a quote that already contains terminal punctuation
is rejected. A structured rendering that is missing or occurs ambiguously in
the committed candidate blocks rather than silently losing provenance.

## Markup syntax traps

The surface uses HTML5 with native presentation MathML. Document creation accepts
block elements; `with_` remains inline-only. Entities such as `&nbsp;` and numeric
references are supported. Unsupported structures and CSS return diagnostics.
See [references/markup-dialect.md](references/markup-dialect.md).

## Complete document context

Read the entire document before editing. There is no section paging; prefer full
context unless the task specifically calls for narrowed discovery. Ordinary
paragraph text appears once in HTML. Exact source text for ambiguous paragraphs
is in `selections.paragraphs` (`at`, `selectable_text`). Generated equation text,
list labels, note numbers, tabs and displayed revision text are not interchangeable
with that source selection space.

Comments include thread ID, body, replies, status and source anchor ranges:
`at`, `select`, `occurrence`, `start`, `end`, and `marked_text` when different from
current text. `selection_space` declares final-view Unicode scalar offsets. Use
provided selectors rather than estimating positions from HTML.

Repeated styles may use ordinary CSS classes defined once in an embedded style
block. All formatting values remain available. For authored replacement fragments,
use equivalent inline formatting instead of copying an unresolved class.
Images remain inline with dimensions, descriptions and source package paths.
`assets` maps short `url` aliases to canonical `asset_url` values and package
`parts`. Read `kind="assets"` only when binary payloads are needed. UI coordinates
and the general UI stylesheet are supplied by the explicit UI profile.
