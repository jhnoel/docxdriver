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
    'format_table',
    'format_row',
    'format_cell',
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
    'set_section_boundary',
    'set_first_page_mode',
    'set_page_margins',
    'set_even_and_odd_headers',
    'comment_add',
    'comment_reply',
    'comment_set_status',
    'comment_delete',
    'bookmark_add',
    'bookmark_rename',
    'bookmark_move',
    'bookmark_delete',
    'revision_settle',
  ],
  docs: {
    format_table: 'Format the table at a source-bound table:N handle or inserted $alias. format accepts width (unit auto, points/value, percent/value), column_widths (positive points, exactly matching the grid), layout (fixed/autofit), alignment, borders, cell_spacing, padding and an existing table style ID. Omitted values preserve source properties; clear removes supported direct settings and wins over set. Grid dimensions cannot be cleared. Tracked plans snapshot table and grid properties; settle a pending property change before editing that bag again.',
    format_row: 'Format a source-bound table:N/row:N handle. format accepts height in points, height_rule (auto/at_least/exact), repeating_header, allow_split and clear. False explicitly disables header repetition or splitting; clear inherits. Tracked plans preserve the complete prior row property bag for rejection.',
    format_cell: 'Format a source-bound table:N/row:N/cell:N handle using width, borders, fill (#RRGGBB), padding, vertical_alignment (top/center/bottom), and clear. Cell indices are physical; a vertically merged logical cell is addressed through its restart cell and applies to all backing continuation cells. Continuation targets and legacy hMerge targets are rejected. Tracked plans preserve prior cell property bags. Text, merge structure and nested content remain unchanged.',

    edit_table: 'Structural table edits using a source-bound table handle. action.kind is insert_rows/delete_rows/insert_columns/delete_columns/merge_cells/split_cell/delete; coordinates and counts are one-based. Insert/delete grid operations and merge require an unmerged rectangular grid. Split keeps content in the first resulting cell; merge concatenates content in row-major order. Insert actions optionally accept as for a group alias: inserted rows use $name/row:N[/cell:N]; inserted columns use $name/row:N/cell:N within the inserted strip. Track mode records insert_rows/delete_rows/delete as tracked row insertions/deletions and refuses insert_columns/delete_columns/merge_cells/split_cell (Word does not track them); direct mode applies every action with no revision. Rejects tables containing unresolved revisions of other authors, bookmarks, comments, fields or content controls. Delete the table instead of its final row or column.',
    insert_table: "Insert a rectangular table before/after paragraph at, with rows and columns in 1..100, and optionally fill it in the same plan. cells is a list of rows, each a list of cell strings in insert_paragraph's inline content (<b>, <i>, <a href>, ...); missing trailing rows and cells stay empty, extra rows or cells are an error. Track mode inserts it as a tracked table (inserted rows and content; refused as the last block of a table cell). Optional as creates a plan-local table alias for later formatting and structural edits, and binds each cell's paragraph as $<as>_rNcM (row N, column M, from 1) for later ops in the same plan. Every cell paragraph id is reported in allocated_ids, row by row.",

    set_list: 'Assign explicitly selected paragraphs to a new list (format defaults to decimal), continue an existing num_id, or remove membership with remove=True. level is 0..8; IDs/starts are 1..2147483647. start creates an isolated restarted instance, and on new lists applies only to the selected level (explicit per-level start wins). Optional levels defines 1..9 per-level format/label/start/restart_after, signed indent_left/indent_right/first_line_indent and marker_bold/marker_italic/marker_font/marker_color for a new list. Label placeholders reference this or ancestor levels (%1..%9). Existing definitions are never restyled. Track mode records membership changes (pPrChange), direct mode applies them with no revision; settle pending paragraph changes first. Definitions remain after rejection for safe reference restoration. Typed membership changes on lists containing explicit continuation paragraphs are rejected, including numbered parents; HTML multi-paragraph creation and reconstruction are supported.',
    replace_text:
      'Replace an exact occurrence of `select` in the paragraph at `at` with `with`; `occurrence` picks the nth match (1-based).',
    replace_equation: 'Replace a current equation using presentation MathML. Read docx_read(path).equations for at and equation (1-based). Omit equation only when the paragraph has one. Display mode is preserved by default. Settle existing revisions before editing their equations.',
    delete_equation:
      'Delete a current equation while preserving the paragraph. Read docx_read(path).equations for at and equation (1-based). Omit equation only when the paragraph has one. Tracked mode keeps the equation recoverable until the deletion is accepted.',
    revise_text:
      'Several exact replacements inside one bounded scope; prefer the chainable `Revise(within, at=None, occurrence=None).replace(select, with_, occurrence=None, all=False)` builder. Scope: `within` (a phrase unique in paragraph `at`, or in the whole document when `at` is omitted; `occurrence` picks a repeated one), else the paragraph `at`, else the whole document. Each edit must match exactly once in the scope unless it sets `occurrence` or `all`. All edits resolve against the scope as it stands when the operation starts, must not overlap, and become separate minimal tracked changes.',
    replace_paragraph: 'Replace the entire paragraph at `at` with `with`.',
    format_text:
      'Apply character formatting to an exact occurrence of `select` in the paragraph at `at`: boolean attributes (`bold`, `italic`, `underline`, `strike`, `superscript`, `subscript`), `color` (#RRGGBB), `highlight` (Word palette including none), `font_size` (points), `occurrence`, or `clear`. Omitted properties preserve source OOXML; false explicitly overrides inherited formatting (baseline for superscript/subscript). Clear removes the direct property and inherits styles/defaults, winning over set; superscript/subscript share one property. Track mode records the change (rPrChange/pPrChange); direct mode applies it with no revision.',
    format_paragraph:
      'Apply paragraph formatting to the paragraph at `at`: `style`, `alignment` (left/center/right/justify), signed `indent_left`/`indent_right`, signed `first_line_indent` (negative for hanging, zero for an explicit zero offset), `space_before`/`space_after`, `line_spacing` (dict with `mode` multiple/exact/at_least and `value`), or `clear`. Omitted properties are untouched; clear removes direct settings and inherits styles/defaults, winning over set. Track mode records the change (rPrChange/pPrChange); direct mode applies it with no revision.',
    insert_paragraph:
      'Insert a new paragraph before or after an anchor: paragraph id/alias (`at`) or body-level table number (`table:N` from markup); `with` is its text, or a list of texts inserted as consecutive paragraphs in list order; `style` is the style of every new paragraph (omitted, each new paragraph takes the paragraph properties of the anchor as the Enter key in Word does, so it continues a list; after a heading it takes the next style the heading style names), and `as` names the new paragraph id (a `$name` alias), or with a list names the whole group `$name` and item N `$name_N`; a group expands to all its paragraphs in `set_list`/`delete_paragraphs`, anchors an insert after its last or before its first paragraph, and is rejected where one paragraph is expected. Several inserts at one anchor and position also keep plan order.',
    delete_paragraphs:
      'Delete the paragraphs listed in `at`. Track mode: a tracked deletion; accepting it leaves the id, style and list membership of the following paragraph intact. Direct mode: removed with no revision.',
    set_header:
      'Set section header chrome from inner HTML (`with`); optional `section` (default 1) and `kind` (default/even/first). Track mode: old content becomes a tracked deletion and the new a tracked insertion (a new header part stays, empty, after reject-all); kind=first tracks titlePg; kind=even is refused while even/odd headers are off.',
    set_footer:
      'Set section footer chrome from inner HTML (`with`); optional `section` (default 1) and `kind` (default/even/first). Tracked as set_header.',
    clear_header:
      'Remove the explicit header reference for a section/kind (defaults: section 1, kind default). Track mode deletes the content of the header as a tracked deletion instead (section 1 only; the empty header stays referenced).',
    clear_footer:
      'Remove the explicit footer reference for a section/kind (defaults: section 1, kind default). Tracked as clear_header.',
    set_section_boundary: 'Insert a section boundary after body paragraph at; remove=True merges its section into the following section using the following layout while preserving effective following story chains. break_type selects nextPage/continuous/oddPage/evenPage/nextColumn. Direct mode only; tracked structural boundaries and pending property revisions are rejected. The final body section is preserved. Inventory handles section:<paragraph ID> remain stable; section:$alias references an inserted paragraph boundary.',
    set_section_layout:
      'Selectively set section layout (section defaults to 1): page_size letter/legal/a4/a3/a5 or paired width/height in points; orientation portrait/landscape normalizes dimensions. Columns 1..45 with column_spacing, column_widths and column_separator; break_type nextPage/continuous/oddPage/evenPage/nextColumn; page_number_format, page_number_start or page_number_continue=true. Omission preserves properties; tracking snapshots section properties while preserving story relationships. Pending property changes must be settled first.',
    set_first_page_mode:
      'Enable/disable first-page stories in a section (default 1) without removing stored stories. Tracked mode records a section-property snapshot; settle pending section changes before editing again.',
    set_page_margins:
      'Set selected nonnegative page margins in points for a section (default 1): top/right/bottom/left/header/footer/gutter. Omitted values are preserved. Track mode records the old margins in a section-property change; direct mode applies them with no revision.',
    set_even_and_odd_headers:
      'Enable or disable distinct even-page headers/footers (`even_and_odd`); when true, `kind="even"` chrome applies to even pages. Direct mode only: Word cannot track this document setting, so track mode refuses it.',
    comment_add:
      'Anchor a comment with `text` to the paragraph at `at`; omit `select` to comment the whole paragraph, or pass `select`/`occurrence` (1-based) to pin one exact span. Applies directly in both change modes (Word does not track comments).',
    comment_reply:
      'Reply to an existing comment; `comment_id` is the decimal id from a prior `comment_add` or a comments-kind read.',
    comment_set_status:
      'Set a comment\'s status to `open` or `resolved` by decimal `comment_id`.',
    comment_delete:
      'Delete a comment by decimal `comment_id`.',
    bookmark_add:
      "Bookmark the paragraph at `at` with `name`: omit `select` to wrap the whole paragraph, or pass `select`/`occurrence` (1-based) to wrap one exact span. Names start with a letter, then letters, digits or underscores, at most 40 characters, unique ignoring case; a leading `_` is reserved for hidden bookmarks. Applies directly in both change modes (Word does not track bookmarks). Body paragraphs only.",
    bookmark_rename:
      "Rename the body bookmark `name` to `new_name` (same rules as bookmark_add); hidden bookmarks may be renamed from but not to.",
    bookmark_move:
      "Re-anchor the body bookmark `name` to the paragraph at `at` (whole paragraph, or `select`/`occurrence`), keeping its name and id.",
    bookmark_delete:
      "Remove the body bookmark `name`: its start and end markers only; the text is untouched.",
    revision_settle:
      'Accept or reject tracked changes: `target` is a decimal revision id from `docx_read(path, kind="revisions")` or `all`; `action` is `accept` or `reject`. Prefer the `AcceptRevision(target)` / `RejectRevision(target)` shorthands. Settling `all` also settles tracked changes made by earlier operations in the same plan, and later selectors address the settled text.',
  },
};
