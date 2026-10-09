---
name: docx-pi
description: Read, find, preview, and commit safe typed DOCX plans with the five docxdriver-pi tools (docx_create, docx_read, docx_find, docx_edit, docx_help).
---

# DOCX with docxdriver-pi

Use only the five `docxdriver-pi` tools. Do not unzip OOXML or use legacy mutation
tools. Read before editing, preview the complete plan, then commit with the
returned preview key.

## Workflow

1. Call `docx_read({path})`. Use `kind: "document"`, `view: "markup"`, and a
   `page` when the response is paginated. Use `docx_find` to locate text in a
   large document. Both include the full source SHA-256.
2. Call `docx_edit({path, plan: {operations: [...]}})` to obtain a preview.
   Inline plans default to author `docxdriver` and `change_mode: "track"`.
3. Call the same plan with `preview_key` to commit. A successful preview is
   required before a commit; a changed source or semantic plan requires a new
   preview.

For durable plans, author a TOML file with the normal file tool and call
`docx_edit({path, plan: {file: "plan.toml"}})`. TOML comments and whitespace do
not change the normalized plan identity. Do not hand-edit the DOCX path in a
plan or use a stale key.

## Plan operations

Operations use the generated schema and serialized Rust names (`op`, snake_case
fields). Consult `docx_help` for the generated reference. Typical operations
are `replace_text`, `replace_paragraph`, `format_text`, `format_paragraph`,
`insert_paragraph`, `delete_paragraphs`, `set_header`, `set_footer`,
`clear_header`, `clear_footer`, `set_even_and_odd_headers`, `comment_add`,
`comment_reply`, `comment_set_status`, `comment_delete`, `set_page_margins`,
and `revision_settle`.

Equation operations are `replace_equation` and `delete_equation`. First read
`docx_read(path).equations`; each entry gives `at`, a paragraph address, and a
1-based `equation` number. Omit the number only when that paragraph has one
equation. `replace_equation` takes presentation MathML in `mathml` and preserves display mode by
default. `delete_equation` removes the selected equation while preserving the
surrounding paragraph. In tracked mode a deletion remains recoverable until
the revision is accepted; reject it to restore the equation.

In HTML, use native `<math display="inline">` or `<math display="block">` with
presentation MathML operands. No LaTeX is required.

`set_even_and_odd_headers` takes `even_and_odd` (boolean); when true,
`kind="even"` headers/footers apply to even pages. Comment ops: add with `at`
plus optional `select`/`occurrence` (1-based); reply, set status
(`open`/`resolved`), and delete by decimal `comment_id`. Read
`kind: "comments"` to list ids.

Paragraph addresses are uppercase eight-digit IDs from `docx_read`;
`insert_paragraph` also accepts `table:N` anchors from body-level
`<table data-docx-table="N">` markup. Insertions may define a plan-local alias (`as`) for
later operations. Measurements are points, colors are uppercase `#RRGGBB`, and
alignment is `justify`.

Replacement content accepts safe inline HTML: `<strong>`, `<em>`, `<b>`, `<i>`,
`<u>`, `<s>`, `<sup>`, `<sub>`, `<a>`, `<br>`, styled `<span>`, and native `<math>`.
Do not include block wrappers, images, notes, fields, comments or revision tags.
HTML5 entities and attribute syntax are supported. Use `<br>` for explicit line
breaks. Unsupported CSS, unsafe URLs and scripts return diagnostics.

Expected invalid plans, stale sources, stale keys, unknown fields, and rejected
operations are normal structured results. Inspect their `diagnostic.code`,
`diagnostic.path`, and optional TOML span, repair the plan, and preview again.

## Pagination

Pages are positive integers. Pagination packs complete top-level rendered blocks
under 32 KiB of UTF-8. Oversized blocks are returned intact on one page; they
are never split, so paragraph anchors remain stable.

See `references/pi-surface.md` for tool shapes.

Read the full document before editing unless the task explicitly calls for a narrower view. Default reads return one complete HTML projection and equation addresses. Preview results retain affected context; commit responses omit repeated context, echoed plans and binary DOCX data.
