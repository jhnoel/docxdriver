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
    'revise_text',
    'replace_paragraph',
    'replace_equation',
    'delete_equation',
    'format_text',
    'format_paragraph',
    'set_list',
    'edit_table',
    'insert_table',
    'insert_paragraph',
    'delete_paragraphs',
    'set_header',
    'set_footer',
    'clear_header',
    'clear_footer',
    'set_section_layout',
    'set_first_page_mode',
    'set_page_margins',
    'set_even_and_odd_headers',
    'comment_add',
    'comment_reply',
    'comment_set_status',
    'comment_delete',
    'revision_settle',
  ],
  docs: {
    edit_table: 'Direct structural table edits using a source-bound table handle. action.kind is insert_rows/delete_rows/insert_columns/delete_columns/merge_cells/split_cell/delete; coordinates and counts are one-based. Insert/delete grid operations and merge require an unmerged rectangular grid. Split keeps content in the first resulting cell; merge concatenates content in row-major order. Rejects tracked mode and tables containing unresolved revisions, bookmarks, comments, fields or content controls. Delete the table instead of its final row or column.',
    insert_table: 'Insert a rectangular table before/after paragraph at, with rows and columns in 1..100. Direct mode only. Optional as creates a plan-local table alias for later formatting and structural edits.',

    set_list: 'Assign explicitly selected paragraphs to a new list (format defaults to decimal), continue an existing num_id, or remove membership with remove=True. level is 0..8; start is positive and creates an isolated restarted instance. Optional levels defines 1..9 per-level format/label/start/restart_after, signed indent_left/indent_right/first_line_indent and marker_bold/marker_italic/marker_font/marker_color for a new list. Label placeholders reference this or ancestor levels (%1..%9). Existing definitions are never restyled. Membership changes are tracked; settle pending paragraph changes first. Definitions remain after rejection for safe reference restoration.',
    replace_text:
      'Replace an exact occurrence of `select` in the paragraph at `at` with `with`; `occurrence` picks the nth match (1-based).',
    replace_equation: 'Replace a current equation using presentation MathML. Read docx_read(path).equations for at and equation (1-based). Omit equation only when the paragraph has one. Display mode is preserved by default. Settle existing revisions before editing their equations.',
    delete_equation:
      'Delete a current equation while preserving the paragraph. Read docx_read(path).equations for at and equation (1-based). Omit equation only when the paragraph has one. Tracked mode keeps the equation recoverable until the deletion is accepted.',
    revise_text:
      'Several exact replacements inside one bounded scope; prefer the chainable `Revise(within, at=None, occurrence=None).replace(select, with_, occurrence=None, all=False)` builder. Scope: `within` (a phrase unique in paragraph `at`, or in the whole document when `at` is omitted; `occurrence` picks a repeated one), else the paragraph `at`, else the whole document. Each edit must match exactly once in the scope unless it sets `occurrence` or `all`. All edits resolve against the scope as it stands when the operation starts, must not overlap, and become separate minimal tracked changes.',
    replace_paragraph: 'Replace the entire paragraph at `at` with `with`.',
    format_text:
      'Apply character formatting to an exact occurrence of `select` in the paragraph at `at`: boolean attributes (`bold`, `italic`, `underline`, `strike`, `superscript`, `subscript`), `color` (#RRGGBB), `highlight` (Word palette including none), `font_size` (points), `occurrence`, or `clear`. Omitted properties preserve source OOXML; false explicitly overrides inherited formatting (baseline for superscript/subscript). Clear removes the direct property and inherits styles/defaults, winning over set; superscript/subscript share one property. Formatting changes are always tracked.',
    format_paragraph:
      'Apply paragraph formatting to the paragraph at `at`: `style`, `alignment` (left/center/right/justify), signed `indent_left`/`indent_right`, signed `first_line_indent` (negative for hanging, zero for an explicit zero offset), `space_before`/`space_after`, `line_spacing` (dict with `mode` multiple/exact/at_least and `value`), or `clear`. Omitted properties are untouched; clear removes direct settings and inherits styles/defaults, winning over set. Formatting changes are always tracked.',
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
    set_section_layout:
      'Selectively set section layout (section defaults to 1): page_size letter/legal/a4/a3/a5 or paired width/height in points; orientation portrait/landscape normalizes dimensions. Columns 1..45 with column_spacing, column_widths and column_separator; break_type nextPage/continuous/oddPage/evenPage/nextColumn; page_number_format, page_number_start or page_number_continue=true. Omission preserves properties; tracking snapshots section properties while preserving story relationships. Pending property changes must be settled first.',
    set_first_page_mode:
      'Enable/disable first-page stories in a section (default 1) without removing stored stories. Tracked mode records a section-property snapshot; settle pending section changes before editing again.',
    set_page_margins:
      'Set selected nonnegative page margins in points for a section (default 1): top/right/bottom/left/header/footer/gutter. Omitted values are preserved. Like existing header/footer setters, margin changes are direct even in tracked plans.',
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
    revision_settle:
      'Accept or reject tracked changes: `target` is a decimal revision id from `docx_read(path, kind="revisions")` or `all`; `action` is `accept` or `reject`. Prefer the `AcceptRevision(target)` / `RejectRevision(target)` shorthands. Settling `all` also settles tracked changes made by earlier operations in the same plan, and later selectors address the settled text.',
  },
};
