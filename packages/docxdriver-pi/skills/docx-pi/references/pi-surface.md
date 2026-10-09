# docxdriver-pi surface

| Tool | Required shape |
|---|---|
| `docx_create` | `{path, paragraphs: string[] | html: string}` (exactly one) |
| `docx_read` | `{path, kind?, view?}` |
| `docx_find` | `{path, query, ignore_case?}` |
| `docx_edit` | `{path, plan: {operations, author?, change_mode?} | {file}, preview_key?}` |
| `docx_help` | `{topic?}` |

`docx_edit` is plan-only. Inline plans bind to the current SHA-256 and use core
defaults. File plans are TOML and own explicit `base`, `author`, and
`change_mode`. A no-key call is preview; the key returned by that call is the
only commit capability.

Read `kind` values are `document`, `styles`, `comments`, `revisions`, and `assets`.
`view` (`markup`, `final`, `original`) applies only to documents.
Document reads return the complete HTML and semantic metadata in one response.
Full-document context is the default; there is no section paging.

Plan operation vocabulary, result envelopes, and diagnostics are generated from
the core schema. Consult `docx_help` for the generated reference. Typical ops
include `replace_text`, `replace_paragraph`, `format_text`, `format_paragraph`,
`insert_paragraph`, `delete_paragraphs`, `set_header`, `set_footer`,
`clear_header`, `clear_footer`, `set_even_and_odd_headers`, `comment_add`,
`comment_reply`, `comment_set_status`, and `comment_delete`.

Equation edits use `replace_equation` and `delete_equation`. Discover the
paragraph address and 1-based equation number from `docx_read(...).equations`.
HTML embeds native presentation MathML in `<math display="inline|block">`.
`replace_equation` takes `mathml`; output contains MathML rather than LaTeX.

`set_even_and_odd_headers` takes `even_and_odd` (boolean). Comment ops: add with
`at` plus optional `select`/`occurrence`; reply, set status (`open`/`resolved`),
and delete by decimal `comment_id`. Read `kind: "comments"` to list ids.
