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
- `docx_read(path, kind="comments" | "styles" | "revisions" | "bookmarks" | "assets")` → dict
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

Text/paragraph: `Revise`, `ReplaceText`, `ReplaceParagraph`, `FormatText`,
`FormatParagraph`, `InsertParagraph`, `DeleteParagraphs`.

For formatting, omitted properties preserve source settings. Boolean `False`
explicitly turns the property off; `clear` removes its direct setting and
inherits styles/defaults. Clear wins when also set. Superscript/subscript share
one vertical-alignment property. Formatting is tracked even in direct plans.
HTML table/row/cell styling returns a diagnostic; put supported paragraph/span
styles inside cells. Containers without a style mapping also reject `style`.

Precise wording changes: prefer `Revise`. Change only the words that change,
and bound short selectors with a scope instead of counting occurrences or
rewriting whole sentences:

```python
Revise("repair the roof within thirty days").replace("thirty", "sixty").replace("roof", "roof and gutters")
Revise(at="2673269E").replace("Tenant", "Lessee", all=True)
Revise("Section 4(b)", at="2673269E", occurrence=2).replace("4(b)", "4(c)")
Revise().replace("Acme Corp.", "Acme, Inc.", all=True)   # whole document
```

- Scope: `within` must be unique in paragraph `at`, or in the whole document
  when `at` is omitted (no id lookup needed); `occurrence` picks a repeated
  one. Without `within`, the scope is paragraph `at`, else the document.
- Each `.replace(select, with_)` must match exactly once in the scope unless
  it passes `occurrence=` or `all=True`. Selections may not overlap.
- Every selector in one `Revise` sees the scope as it was before that
  `Revise`; put all changes to one passage in one `Revise`.

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

Bookmarks:

- `BookmarkAdd(at, name, select=None, occurrence=None)` — wrap the whole
  paragraph, or one exact `select` span.
- `BookmarkRename(name, new_name)`, `BookmarkMove(name, at, select=None,
  occurrence=None)` (keeps name and id), `BookmarkDelete(name)` (markers only;
  text untouched).
- Names: a letter, then letters, digits or underscores; at most 40; unique
  ignoring case. New names must not start with `_` (hidden: `_GoBack`,
  `_Toc…`), though hidden bookmarks can be moved, renamed from or deleted.
- Applied directly even in tracked plans; body bookmarks only. An unknown
  name's error lists close matches.
- `docx_read(path, kind="bookmarks")["bookmarks"]` lists `{name, id, hidden,
  start, end, text, part}` for every story; markup does not show bookmarks.

Tracked changes:

- `AcceptRevision(target)` / `RejectRevision(target)` — `target` is a revision
  id from `docx_read(path, kind="revisions")` or `"all"`. They build
  `RevisionSettle(action, target)`.
- Operations apply in order, and every selector matches the visible text
  *as left by the preceding operations*. Settling changes that text:
  rejecting restores deleted text and removes inserted text. Prefer a
  settle-only plan: review, commit, re-read, then author edits against the
  fresh read. A stale selector's error names the earlier operations that
  changed its paragraph. `"all"` also settles tracked edits made earlier in the
  same plan, so never put it after your own edits.

`InsertParagraph` also accepts `table:N` anchors from body-level
`<table data-docx-table="N">` markup and may set `as_="$name"` for later ops.
For several paragraphs pass a list, `with_=["One", "<b>Two</b>"]`: they land
in list order. With `as_="name"`, `$name` is the group and item N is
`$name_N`: `SetList(at=["$name"], format="bullet")` lists them all, an insert
at `$name` goes after the last (or before the first), and single-paragraph ops
need `$name_N`. Separate inserts at
one anchor and position also keep plan order.

Create and fill a table in one plan: `InsertTable(at, position="after",
rows=2, columns=2, as_="t", cells=[["<b>Name</b>", "<b>Role</b>"], ["Ada"]])`.
`cells` is rows of inline-content strings; missing trailing rows/cells stay
empty. With `as_`, cell row N column M is the paragraph alias `$t_rNcM` for
later ops in the same plan, e.g. `ReplaceParagraph(at="$t_r2c2", with_="Analyst")`.
Do not commit an empty table and re-read it for cell ids.

Replacement content accepts safe inline HTML: `<strong>`, `<em>`, `<b>`, `<i>`,
`<u>`, `<s>`, `<sup>`, `<sub>`, `<a>`, `<br>`, styled `<span>`, and native `<math>`.
Do not include block wrappers, images, notes, fields, comments or revision tags.
HTML5 entities and attribute syntax are supported. Use `<br>` for explicit line
breaks. Unsupported CSS, unsafe URLs and scripts return diagnostics.

Blocked reviews, stale keys, unknown fields, and rejected ops are normal.
A review blocked with `integrity_regression` means the plan would have damaged
the document (its message names each fault); nothing was written. Faults
already in the source never cause it. Do not retry the same plan; drop or
rephrase the operation, or report that the edit cannot be made safely.

A document saved as Strict Open XML is rejected on read and edit; ask the user
to save it as a standard Word Document (.docx) in Word and try again.

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

Direct formatting is inline `style`; a paragraph's `style` lists only what
differs from its `data-docx-style` (or `result.default_style`). Markup copied
from a read can go straight into an edit's `with`. In `replace_paragraph`, bare text keeps the formatting all the
paragraph's runs share; write `font-weight:normal` (or similar) to drop it.
`create` cannot resolve references; write inline style there.
Images remain inline with dimensions, descriptions and source package paths.
`assets` maps short `url` aliases to canonical `asset_url` values and package
`parts`. Read `kind="assets"` only when binary payloads are needed. UI coordinates
and the general UI stylesheet are supplied by the explicit UI profile.

### Word-level redlines

Tracked plain-text replacements automatically compute word-level redlines in
`replace_text`, `revise_text`, and `replace_paragraph`. This is an internal
engine policy; no additional API parameter is needed. Unchanged words retain
their runs and formatting. Adjacent changed words form one deletion followed by
one insertion, including matching spaces or punctuation between them. Unchanged
words separate changes. Identical text creates no revisions.

Rich replacements (formatting, links, breaks, or equations), selections with
pending revisions, and comparisons exceeding the internal size limit retain
the existing whole-selection tracking path and its safety checks.

Example: replacing `Payment is due within thirty days.` with
`Payment is due within sixty days.` tracks only `thirty` → `sixty`.
