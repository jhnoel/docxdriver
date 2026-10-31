## Host functions

- `docx_create(path: str, html: str)`
- `docx_read(path: str, view: str = "markup", kind: str = "document") -> ReadResult | dict` — complete HTML
  markup and a current equations-address listing (at, equation, display, editable) by default (`kind="document"`); `kind="comments"`, `"styles"`, `"revisions"`, `"bookmarks"`, or `"assets"` returns the
  core listing object (and a second positional of `comments`/`styles`/`revisions`/`bookmarks` is treated as `kind`).
- `docx_find(path: str, query: str, ignore_case: bool = False) -> list` — core find match
  records verbatim (`{index, id?, text, style?, number?}` per matching paragraph; `id` is
  the canonical uppercase paragraph address)
- `docx_help(topic: str | None = None) -> str`
- `docx_review(path: str, plan: Plan) -> ReviewResult | None` — validates the complete plan and returns `commit_key` plus bounded per-operation `edits` with affected markup context; returns `None` when validation is blocked
- `docx_commit(commit_key: str)` — commits the immutable reviewed plan; the key owns the
  target file and plan
Review does not write the target. Review and commit must happen in separate later
Python calls; changing the plan requires a new review.

## Discovering paragraphs

`docx_find` plus `docx_read` are the supported discovery workflow:
find returns core match records to locate paragraphs, and reads return
structural markup. Operations stay addressed
by the markup-view paragraph `id`:

    matches = docx_find("contract.docx", "Payment is due within")
    for m in matches:
        print(m["id"], m["text"])
    markup = docx_read("contract.docx").markup

Reading `view="final"` or `view="original"` is allowed for analysis, but
review plans remain source-bound to the exact bytes and paragraph ids of the
current `markup` view.

## Computing operations with regex

`re` is pre-imported and `ReadResult.markup` is an ordinary string, so you can
inspect it and build operations in Python loops. Return the full ReadResult or
print its full markup and semantic metadata when reading; prefer complete document
context. Named DOCX style IDs appear in data-docx-style and map one-to-one to
ReadResult.styles (resolved CSS keyed by style ID); a paragraph without
data-docx-style uses ReadResult.default_style. Direct formatting is inline
style; a paragraph's style lists only what differs from its named style. The
style catalog is omitted from the default display; inspect result.styles to
read its definitions. Markup copied from a read is valid fragment input. Image aliases map to canonical assets in ReadResult.assets.
ReadResult.selection_space describes comment selectors; ReadResult.selections
provides exact text for paragraphs where generated content or revisions make
HTML text ambiguous:


    original = docx_read("contract.docx").markup
    ops = []
    for m in re.finditer(r'<p id="([0-9A-F]{8})"[^>]*>([^<]*?)</p>', original):
        if "Payment is due within" in m.group(2):
            ops.append(ReplaceText(at=m.group(1), select=m.group(2),
                                   with_=re.sub(r'within \d+ days', 'within 45 days', m.group(2))))
    plan = Plan(author="Pi", ops=ops)
    review = docx_review("contract.docx", plan)
    if review is not None:
        for edit in review.edits:
            print(edit["index"], edit["context"])
        # ...later python call...
        docx_commit(review.commit_key)

`select` must match the paragraph's current visible text exactly; on a document
with pending tracked changes, grep the `view="final"` projection instead.

## Example

    original = docx_read("contract.docx").markup
    plan = Plan(author="Pi", change_mode="track", ops=[
        ReplaceText(at="2673269E", select="thirty days", with_="sixty days"),
        FormatText(at="2673269E", select="sixty days", bold=True),
    ])
    review = docx_review("contract.docx", plan)
    # Inspect review.edits before committing in a later Python call.
    if review is not None:
        print(review.commit_key)

Use quote_find(source, query) to obtain the paragraph id and exact source text.
Create Quote(source, at, select, occurrence=1, style="double"), where select is one contiguous,
verbatim source span. Adapt it with .omit(exact_source_text) and
.bracket(exact_source_text, replacement). Adaptation selectors always refer to
the original span; overlaps fail. Finish with quote_validate(q). Only the host
constructs QuoteResult.text, including curly double quotation marks by default
(style="single" or style="none" are also available). Its provenance binds the source SHA-256, paragraph
SHA-256, paragraph id, offsets, original span, and every adaptation.

Raw quotation marks in ordinary authored text are lint errors. For a
non-quotational mention such as the word quotation, use
term_render(Term("quotation")); its quote marks are host-created and its
provenance is classified as kind="term" rather than a source quotation.
For authored document content, prefer Inline([plain_text, Term(...), Quote(...)]).
inline_validate() automatically lints every plain-text part before the host
renders structured nodes. The default policy is error; comment must be chosen
explicitly and returns warning spans that require protected comment bubbles.
In the plan REPL's flagged quotation mode, pass Quote, Term, or Inline directly
to an operation with_ field. docx_review resolves the node and binds its source;
copying QuoteResult.text into a plain string loses that provenance boundary.

Example:
q = Quote("source.docx", at="89ABCDEF", select="The tenant must promptly pay the charge.")
q.omit(" promptly")
q.bracket("tenant", "Tenant")
validated = quote_validate(q)
print(validated.text)
print(validated.provenance)
