// plan-capability-manifest.mjs
/**
 * Declarative capability manifest for the Version 1 Python plan surface.
 *
 * Selects which core operations ($defs.editOp.oneOf in the checked-in
 * Rust-generated request schema) the initial REPL exposes. It selects
 * operations only: field shapes, types, enums, required/optional membership,
 * defaults, and validation all come from the schema and the core typed
 * boundary — never from this file. `docs` holds the concise per-operation
 * prose used by the generated operation reference; the schema itself carries
 * no per-field descriptions, so the prose lives here.
 *
 * Keep this list in sync with the ops the Python REPL should expose;
 * expanding it requires regenerating the prelude regions
 * (scripts/generate-python-dataclasses.mjs) and re-running the drift tests.
 */
export const PLAN_CAPABILITIES = {
  ops: [
    'replace_text',
    'replace_paragraph',
    'replace_equation',
    'delete_equation',
    'format_text',
    'format_paragraph',
    'insert_paragraph',
    'delete_paragraphs',
    'set_header',
    'set_footer',
    'clear_header',
    'clear_footer',
    'set_even_and_odd_headers',
    'comment_add',
    'comment_reply',
    'comment_set_status',
    'comment_delete',
  ],
  docs: {
    replace_text:
      'Replace an exact occurrence of `select` in the paragraph at `at` with `with`; `occurrence` picks the nth match (1-based).',
    replace_equation: 'Replace a current equation using plain LaTeX. Read docx_read(path).equations for at and equation (1-based). Omit equation only when the paragraph has one. Display mode is preserved by default. Settle existing revisions before editing their equations.',
    delete_equation:
      'Delete a current equation while preserving the paragraph. Read docx_read(path).equations for at and equation (1-based). Omit equation only when the paragraph has one. Tracked mode keeps the equation recoverable until the deletion is accepted.',
    replace_paragraph: 'Replace the entire paragraph at `at` with `with`.',
    format_text:
      'Apply character formatting to an exact occurrence of `select` in the paragraph at `at`: boolean attributes (`bold`, `italic`, `underline`, `strike`, `superscript`, `subscript`), `color` (#RRGGBB), `font_size` (points), `occurrence`, or `clear`.',
    format_paragraph:
      'Apply paragraph formatting to the paragraph at `at`: `style`, `alignment` (left/center/right/justify), `indent_left`/`indent_right`, `space_before`/`space_after`, `line_spacing` (dict with `mode` multiple/exact/at_least and `value`), or `clear`.',
    insert_paragraph:
      'Insert a new paragraph before or after an anchor: paragraph id/alias (`at`) or body-level table number (`table:N` from markup); `with` is its text, `style` its style, and `as` names the new paragraph id (a `$name` alias).',
    delete_paragraphs:
      'Delete the paragraphs listed in `at` (tracked deletion by default).',
    set_header:
      'Set section header chrome from inner HTML (`with`); optional `section` (default 1) and `kind` (default/even/first).',
    set_footer:
      'Set section footer chrome from inner HTML (`with`); optional `section` (default 1) and `kind` (default/even/first).',
    clear_header:
      'Remove the explicit header reference for a section/kind (defaults: section 1, kind default).',
    clear_footer:
      'Remove the explicit footer reference for a section/kind (defaults: section 1, kind default).',
    set_even_and_odd_headers:
      'Enable or disable distinct even-page headers/footers (`even_and_odd`); when true, `kind="even"` chrome applies to even pages.',
    comment_add:
      'Anchor a comment with `text` to the paragraph at `at`; omit `select` to comment the whole paragraph, or pass `select`/`occurrence` (1-based) to pin one exact span.',
    comment_reply:
      'Reply to an existing comment; `comment_id` is the decimal id from a prior `comment_add` or a comments-kind read.',
    comment_set_status:
      'Set a comment\'s status to `open` or `resolved` by decimal `comment_id`.',
    comment_delete:
      'Delete a comment by decimal `comment_id`.',
  },
};
