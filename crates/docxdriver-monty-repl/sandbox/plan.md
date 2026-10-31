# docxdriver Python REPL API — Version 1 (Python-authored plan)

# BEGIN GENERATED OPERATION DOCS
## Operations

Selected from the core schema (`$defs.editOp.oneOf`) through the
capability manifest; field shapes, types, and enums are exactly the
checked-in schema’s. Every op dataclass has `to_dict()`, omitting
optional fields whose value is `None`; constructors use `with_`/`as_`
for Python keyword collisions and `to_dict()` maps back to the schema
field names. Operations are addressed by the paragraph `id` attribute
from `doc.markup()` or `doc.find()` (uppercase eight-digit hex) or a `$name` alias.

### BookmarkAdd

`BookmarkAdd(at: str, name: str, occurrence: int | None = None, select: str | None = None)`

Bookmark the paragraph at `at` with `name`: omit `select` to wrap the whole paragraph, or pass `select`/`occurrence` (1-based) to wrap one exact span. Names start with a letter, then letters, digits or underscores, at most 40 characters, unique ignoring case; a leading `_` is reserved for hidden bookmarks. Applies directly in both change modes (Word does not track bookmarks). Body paragraphs only.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `name` | `str` | yes |
| `occurrence` | `int | None` | no |
| `select` | `str | None` | no |

### BookmarkDelete

`BookmarkDelete(name: str)`

Remove the body bookmark `name`: its start and end markers only; the text is untouched.

| Field | Type | Required |
| --- | --- | --- |
| `name` | `str` | yes |

### BookmarkMove

`BookmarkMove(at: str, name: str, occurrence: int | None = None, select: str | None = None)`

Re-anchor the body bookmark `name` to the paragraph at `at` (whole paragraph, or `select`/`occurrence`), keeping its name and id.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `name` | `str` | yes |
| `occurrence` | `int | None` | no |
| `select` | `str | None` | no |

### BookmarkRename

`BookmarkRename(name: str, new_name: str)`

Rename the body bookmark `name` to `new_name` (same rules as bookmark_add); hidden bookmarks may be renamed from but not to.

| Field | Type | Required |
| --- | --- | --- |
| `name` | `str` | yes |
| `new_name` | `str` | yes |

### ClearFooter

`ClearFooter(kind: str | None = None, section: int | None = None)`

Remove the explicit footer reference for a section/kind (defaults: section 1, kind default). Tracked as clear_header.

| Field | Type | Required |
| --- | --- | --- |
| `kind` | `str | None` | no |
| `section` | `int | None` | no |

### ClearHeader

`ClearHeader(kind: str | None = None, section: int | None = None)`

Remove the explicit header reference for a section/kind (defaults: section 1, kind default). Track mode deletes the content of the header as a tracked deletion instead (section 1 only; the empty header stays referenced).

| Field | Type | Required |
| --- | --- | --- |
| `kind` | `str | None` | no |
| `section` | `int | None` | no |

### CommentAdd

`CommentAdd(at: str, text: str, occurrence: int | None = None, select: str | None = None)`

Anchor a comment with `text` to the paragraph at `at`; omit `select` to comment the whole paragraph, or pass `select`/`occurrence` (1-based) to pin one exact span. Applies directly in both change modes (Word does not track comments).

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `text` | `str` | yes |
| `occurrence` | `int | None` | no |
| `select` | `str | None` | no |

### CommentDelete

`CommentDelete(comment_id: str)`

Delete a comment by decimal `comment_id`.

| Field | Type | Required |
| --- | --- | --- |
| `comment_id` | `str` | yes |

### CommentReply

`CommentReply(comment_id: str, text: str)`

Reply to an existing comment; `comment_id` is the decimal id from a prior `comment_add` or a comments-kind read.

| Field | Type | Required |
| --- | --- | --- |
| `comment_id` | `str` | yes |
| `text` | `str` | yes |

### CommentSetStatus

`CommentSetStatus(comment_id: str, status: str)`

Set a comment's status to `open` or `resolved` by decimal `comment_id`.

| Field | Type | Required |
| --- | --- | --- |
| `comment_id` | `str` | yes |
| `status` | `str` | yes |

### DeleteEquation

`DeleteEquation(at: str, equation: int | None = None)`

Delete a current equation while preserving the paragraph. Read doc.equations() for at and equation (1-based). Omit equation only when the paragraph has one. Tracked mode keeps the equation recoverable until the deletion is accepted.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `equation` | `int | None` | no |

### DeleteParagraphs

`DeleteParagraphs(at: list)`

Delete the paragraphs listed in `at`. Track mode: a tracked deletion; accepting it leaves the id, style and list membership of the following paragraph intact. Direct mode: removed with no revision.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `list` | yes |

### EditTable

`EditTable(action: dict, at: str)`

Structural table edits using a source-bound table handle. action.kind is insert_rows/delete_rows/insert_columns/delete_columns/merge_cells/split_cell/delete; coordinates and counts are one-based. Insert/delete grid operations and merge require an unmerged rectangular grid. Split keeps content in the first resulting cell; merge concatenates content in row-major order. Insert actions optionally accept as for a group alias: inserted rows use $name/row:N[/cell:N]; inserted columns use $name/row:N/cell:N within the inserted strip. Track mode records insert_rows/delete_rows/delete as tracked row insertions/deletions and refuses insert_columns/delete_columns/merge_cells/split_cell (Word does not track them); direct mode applies every action with no revision. Rejects tables containing unresolved revisions of other authors, bookmarks, comments, fields or content controls. Delete the table instead of its final row or column.

| Field | Type | Required |
| --- | --- | --- |
| `action` | `dict` | yes |
| `at` | `str` | yes |

### FormatCell

`FormatCell(at: str, format: dict)`

Format a source-bound table:N/row:N/cell:N handle using width, borders, fill (#RRGGBB), padding, vertical_alignment (top/center/bottom), and clear. Cell indices are physical; a vertically merged logical cell is addressed through its restart cell and applies to all backing continuation cells. Continuation targets and legacy hMerge targets are rejected. Tracked plans preserve prior cell property bags. Text, merge structure and nested content remain unchanged.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `format` | `dict` | yes |

### FormatParagraph

`FormatParagraph(at: str, alignment: str | None = None, clear: list | None = None, first_line_indent: float | None = None, indent_left: float | None = None, indent_right: float | None = None, line_spacing: dict | None = None, space_after: float | None = None, space_before: float | None = None, style: str | None = None)`

Apply paragraph formatting to the paragraph at `at`: `style`, `alignment` (left/center/right/justify), signed `indent_left`/`indent_right`, signed `first_line_indent` (negative for hanging, zero for an explicit zero offset), `space_before`/`space_after`, `line_spacing` (dict with `mode` multiple/exact/at_least and `value`), or `clear`. Omitted properties are untouched; clear removes direct settings and inherits styles/defaults, winning over set. Track mode records the change (rPrChange/pPrChange); direct mode applies it with no revision.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `alignment` | `str | None` | no |
| `clear` | `list | None` | no |
| `first_line_indent` | `float | None` | no |
| `indent_left` | `float | None` | no |
| `indent_right` | `float | None` | no |
| `line_spacing` | `dict | None` | no |
| `space_after` | `float | None` | no |
| `space_before` | `float | None` | no |
| `style` | `str | None` | no |

### FormatRow

`FormatRow(at: str, format: dict)`

Format a source-bound table:N/row:N handle. format accepts height in points, height_rule (auto/at_least/exact), repeating_header, allow_split and clear. False explicitly disables header repetition or splitting; clear inherits. Tracked plans preserve the complete prior row property bag for rejection.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `format` | `dict` | yes |

### FormatTable

`FormatTable(at: str, format: dict)`

Format the table at a source-bound table:N handle or inserted $alias. format accepts width (unit auto, points/value, percent/value), column_widths (positive points, exactly matching the grid), layout (fixed/autofit), alignment, borders, cell_spacing, padding and an existing table style ID. Omitted values preserve source properties; clear removes supported direct settings and wins over set. Grid dimensions cannot be cleared. Tracked plans snapshot table and grid properties; settle a pending property change before editing that bag again.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `format` | `dict` | yes |

### FormatText

`FormatText(at: str, select: str, bold: bool | None = None, clear: list | None = None, color: str | None = None, font_size: float | None = None, highlight: str | None = None, italic: bool | None = None, occurrence: int | None = None, strike: bool | None = None, subscript: bool | None = None, superscript: bool | None = None, underline: bool | None = None)`

Apply character formatting to an exact occurrence of `select` in the paragraph at `at`: boolean attributes (`bold`, `italic`, `underline`, `strike`, `superscript`, `subscript`), `color` (#RRGGBB), `highlight` (Word palette including none), `font_size` (points), `occurrence`, or `clear`. Omitted properties preserve source OOXML; false explicitly overrides inherited formatting (baseline for superscript/subscript). Clear removes the direct property and inherits styles/defaults, winning over set; superscript/subscript share one property. Track mode records the change (rPrChange/pPrChange); direct mode applies it with no revision.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `select` | `str` | yes |
| `bold` | `bool | None` | no |
| `clear` | `list | None` | no |
| `color` | `str | None` | no |
| `font_size` | `float | None` | no |
| `highlight` | `str | None` | no |
| `italic` | `bool | None` | no |
| `occurrence` | `int | None` | no |
| `strike` | `bool | None` | no |
| `subscript` | `bool | None` | no |
| `superscript` | `bool | None` | no |
| `underline` | `bool | None` | no |

### InsertParagraph

`InsertParagraph(at: str, position: str, as_: str | None = None, style: str | None = None, with_: str | list[str] | None = None)`

Insert a new paragraph before or after an anchor: paragraph id/alias (`at`) or body-level table number (`table:N` from markup); `with` is its text, or a list of texts inserted as consecutive paragraphs in list order; `style` is the style of every new paragraph (omitted, each new paragraph takes the paragraph properties of the anchor as the Enter key in Word does, so it continues a list; after a heading it takes the next style the heading style names), and `as` names the new paragraph id (a `$name` alias), or with a list names the whole group `$name` and item N `$name_N`; a group expands to all its paragraphs in `set_list`/`delete_paragraphs`, anchors an insert after its last or before its first paragraph, and is rejected where one paragraph is expected. Several inserts at one anchor and position also keep plan order.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `position` | `str` | yes |
| `as_` (schema key `as`) | `str | None` | no |
| `style` | `str | None` | no |
| `with_` (schema key `with`) | `str | list[str] | None` | no |

### InsertTable

`InsertTable(at: str, columns: int, position: str, rows: int, as_: str | None = None, cells: list | None = None)`

Insert a rectangular table before/after paragraph at, with rows and columns in 1..100, and optionally fill it in the same plan. cells is a list of rows, each a list of cell strings in insert_paragraph's inline content (<b>, <i>, <a href>, ...); missing trailing rows and cells stay empty, extra rows or cells are an error. Track mode inserts it as a tracked table (inserted rows and content; refused as the last block of a table cell). Optional as creates a plan-local table alias for later formatting and structural edits, and binds each cell's paragraph as $<as>_rNcM (row N, column M, from 1) for later ops in the same plan. Every cell paragraph id is reported in allocated_ids, row by row.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `columns` | `int` | yes |
| `position` | `str` | yes |
| `rows` | `int` | yes |
| `as_` (schema key `as`) | `str | None` | no |
| `cells` | `list | None` | no |

### ReplaceEquation

`ReplaceEquation(at: str, mathml: str, display: bool | None = None, equation: int | None = None)`

Replace a current equation using presentation MathML. Read doc.equations() for at and equation (1-based). Omit equation only when the paragraph has one. Display mode is preserved by default. Settle existing revisions before editing their equations.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `mathml` | `str` | yes |
| `display` | `bool | None` | no |
| `equation` | `int | None` | no |

### ReplaceParagraph

`ReplaceParagraph(at: str, with_: str)`

Replace the entire paragraph at `at` with `with`.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `with_` (schema key `with`) | `str` | yes |

### ReplaceText

`ReplaceText(at: str, select: str, with_: str, occurrence: int | None = None)`

Replace an exact occurrence of `select` in the paragraph at `at` with `with`; `occurrence` picks the nth match (1-based).

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `select` | `str` | yes |
| `with_` (schema key `with`) | `str` | yes |
| `occurrence` | `int | None` | no |

### ReviseText

`ReviseText(edits: list, at: str | None = None, occurrence: int | None = None, within: str | None = None)`

Several exact replacements inside one bounded scope; prefer the chainable `Revise(within, at=None, occurrence=None).replace(select, with_, occurrence=None, all=False)` builder. Scope: `within` (a phrase unique in paragraph `at`, or in the whole document when `at` is omitted; `occurrence` picks a repeated one), else the paragraph `at`, else the whole document. Each edit must match exactly once in the scope unless it sets `occurrence` or `all`. All edits resolve against the scope as it stands when the operation starts, must not overlap, and become separate minimal tracked changes.

| Field | Type | Required |
| --- | --- | --- |
| `edits` | `list` | yes |
| `at` | `str | None` | no |
| `occurrence` | `int | None` | no |
| `within` | `str | None` | no |

### RevisionSettle

`RevisionSettle(action: str, target: str)`

Accept or reject tracked changes: `target` is a decimal revision id from `doc.revisions()` or `all`; `action` is `accept` or `reject`. Prefer the `AcceptRevision(target)` / `RejectRevision(target)` shorthands. Settling `all` also settles tracked changes made by earlier operations in the same plan, and later selectors address the settled text.

| Field | Type | Required |
| --- | --- | --- |
| `action` | `str` | yes |
| `target` | `str` | yes |

### SetEvenAndOddHeaders

`SetEvenAndOddHeaders(even_and_odd: bool)`

Enable or disable distinct even-page headers/footers (`even_and_odd`); when true, `kind="even"` chrome applies to even pages. Direct mode only: Word cannot track this document setting, so track mode refuses it.

| Field | Type | Required |
| --- | --- | --- |
| `even_and_odd` | `bool` | yes |

### SetFirstPageMode

`SetFirstPageMode(enabled: bool, section: int | None = None)`

Enable/disable first-page stories in a section (default 1) without removing stored stories. Tracked mode records a section-property snapshot; settle pending section changes before editing again.

| Field | Type | Required |
| --- | --- | --- |
| `enabled` | `bool` | yes |
| `section` | `int | None` | no |

### SetFooter

`SetFooter(with_: str, kind: str | None = None, section: int | None = None)`

Set section footer chrome from inner HTML (`with`); optional `section` (default 1) and `kind` (default/even/first). Tracked as set_header.

| Field | Type | Required |
| --- | --- | --- |
| `with_` (schema key `with`) | `str` | yes |
| `kind` | `str | None` | no |
| `section` | `int | None` | no |

### SetHeader

`SetHeader(with_: str, kind: str | None = None, section: int | None = None)`

Set section header chrome from inner HTML (`with`); optional `section` (default 1) and `kind` (default/even/first). Track mode: old content becomes a tracked deletion and the new a tracked insertion (a new header part stays, empty, after reject-all); kind=first tracks titlePg; kind=even is refused while even/odd headers are off.

| Field | Type | Required |
| --- | --- | --- |
| `with_` (schema key `with`) | `str` | yes |
| `kind` | `str | None` | no |
| `section` | `int | None` | no |

### SetList

`SetList(at: list, format: str | None = None, level: int | None = None, levels: list | None = None, num_id: int | None = None, remove: bool | None = None, start: int | None = None)`

Assign explicitly selected paragraphs to a new list (format defaults to decimal), continue an existing num_id, or remove membership with remove=True. level is 0..8; IDs/starts are 1..2147483647. start creates an isolated restarted instance, and on new lists applies only to the selected level (explicit per-level start wins). Optional levels defines 1..9 per-level format/label/start/restart_after, signed indent_left/indent_right/first_line_indent and marker_bold/marker_italic/marker_font/marker_color for a new list. Label placeholders reference this or ancestor levels (%1..%9). Existing definitions are never restyled. Track mode records membership changes (pPrChange), direct mode applies them with no revision; settle pending paragraph changes first. Rejecting a tracked list change removes the definitions it added once nothing refers to them. Typed membership changes on lists containing explicit continuation paragraphs are rejected, including numbered parents; HTML multi-paragraph creation and reconstruction are supported.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `list` | yes |
| `format` | `str | None` | no |
| `level` | `int | None` | no |
| `levels` | `list | None` | no |
| `num_id` | `int | None` | no |
| `remove` | `bool | None` | no |
| `start` | `int | None` | no |

### SetPageMargins

`SetPageMargins(bottom: float | None = None, footer: float | None = None, gutter: float | None = None, header: float | None = None, left: float | None = None, right: float | None = None, section: int | None = None, top: float | None = None)`

Set selected nonnegative page margins in points for a section (default 1): top/right/bottom/left/header/footer/gutter. Omitted values are preserved. Track mode records the old margins in a section-property change; direct mode applies them with no revision.

| Field | Type | Required |
| --- | --- | --- |
| `bottom` | `float | None` | no |
| `footer` | `float | None` | no |
| `gutter` | `float | None` | no |
| `header` | `float | None` | no |
| `left` | `float | None` | no |
| `right` | `float | None` | no |
| `section` | `int | None` | no |
| `top` | `float | None` | no |

### SetSectionBoundary

`SetSectionBoundary(at: str, break_type: str | None = None, remove: bool | None = None)`

Insert a section boundary after body paragraph at; remove=True merges its section into the following section using the following layout while preserving effective following story chains. break_type selects nextPage/continuous/oddPage/evenPage/nextColumn. Direct mode only; tracked structural boundaries and pending property revisions are rejected. The final body section is preserved. Inventory handles section:<paragraph ID> remain stable; section:$alias references an inserted paragraph boundary.

| Field | Type | Required |
| --- | --- | --- |
| `at` | `str` | yes |
| `break_type` | `str | None` | no |
| `remove` | `bool | None` | no |

### SetSectionLayout

`SetSectionLayout(break_type: str | None = None, column_separator: bool | None = None, column_spacing: float | None = None, column_widths: list | None = None, columns: int | None = None, height: float | None = None, orientation: str | None = None, page_number_continue: bool | None = None, page_number_format: str | None = None, page_number_start: int | None = None, page_size: str | None = None, section: int | None = None, section_handle: str | None = None, width: float | None = None)`

Selectively set section layout (section defaults to 1): page_size letter/legal/a4/a3/a5 or paired width/height in points; orientation portrait/landscape normalizes dimensions. Columns 1..45 with column_spacing, column_widths and column_separator; break_type nextPage/continuous/oddPage/evenPage/nextColumn; page_number_format, page_number_start or page_number_continue=true. Omission preserves properties; tracking snapshots section properties while preserving story relationships. Pending property changes must be settled first.

| Field | Type | Required |
| --- | --- | --- |
| `break_type` | `str | None` | no |
| `column_separator` | `bool | None` | no |
| `column_spacing` | `float | None` | no |
| `column_widths` | `list | None` | no |
| `columns` | `int | None` | no |
| `height` | `float | None` | no |
| `orientation` | `str | None` | no |
| `page_number_continue` | `bool | None` | no |
| `page_number_format` | `str | None` | no |
| `page_number_start` | `int | None` | no |
| `page_size` | `str | None` | no |
| `section` | `int | None` | no |
| `section_handle` | `str | None` | no |
| `width` | `float | None` | no |

# END GENERATED OPERATION DOCS

## Scoped revisions

`Revise(within=None, at=None, occurrence=None)` builds one `ReviseText`
operation; chain `.replace(select, with_, occurrence=None, all=False)` for each
change. The scope bounds every `select`, so short selectors stay unambiguous
in long documents and each change becomes a minimal tracked change:

    # unique phrase, no paragraph id needed:
    Revise("the tenant shall pay rent within thirty days").replace("thirty", "sixty").replace("rent", "the rent")
    Revise(at="2673269E").replace("Tenant", "Lessee", all=True)  # whole paragraph
    Revise("Section 4(b)", at="2673269E", occurrence=2).replace("4(b)", "4(c)")
    Revise().replace("Acme Corp.", "Acme, Inc.", all=True)       # whole document

`within` must occur exactly once in its paragraph (`at`) or document unless
`occurrence` picks one. Each `select` must occur exactly once in the scope
unless it passes `occurrence` or `all=True`. All selectors resolve against the
scope as it stands when the operation starts, so later replacements never see
earlier ones; overlapping selections are rejected.

`AcceptRevision(target)` and `RejectRevision(target)` build `RevisionSettle`
operations; `target` is a revision id from `doc.revisions()` or
`"all"`. Put settle operations first: `"all"` also settles tracked changes
made earlier in the same plan.

Every operation dataclass has `to_dict()`, omitting optional fields whose
value is `None`; `Plan.to_dict()` returns `{"author": ..., "change_mode": ..., "ops": [...]}`.

Tracked plain-text replacements automatically compute word-level redlines in
ReplaceText, ReviseText, and ReplaceParagraph. Unchanged words retain their
formatting; adjacent changed words form one deletion followed by one insertion.
Rich replacements retain whole-selection tracking to apply their formatting.
