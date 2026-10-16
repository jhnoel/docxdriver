# docxdriver

The TypeScript package transports the Rust `Request`/`RequestResult` API over
Wasm. Initialize once, then call `executeRequest` with a typed command or plan.
Plans preview by default and return bytes only when committed with their
matching preview key. `parsePlanToml` returns either a typed plan or a
core-owned structured diagnostic.

JSON Schema and TypeScript types are generated from the Rust public model.

### HTML projection

`readHtml(bytes, view)` returns renderable HTML5 with native MathML, scoped `css`,
addressable `paragraphs`, a source fingerprint, UTF-8 `source_map` regions, and
an `assets` manifest. This helper requests `read_kind: "document_ui"`, which
returns one `html` fragment; block entries contain byte ranges into that fragment.
`projection_version` is 2. Paragraph IDs and `data-docx-*`
attributes retain document identity; generated labels and equation text are
excluded from `selectable_text`.

Default `executeRequest` document reads return one complete HTML fragment in
`markup`, a per-document `css` map, and a `styles` catalog of named Word styles,
along with a source fingerprint, revision view, and equation addresses/editability.
Direct formatting is removed from the markup and replaced by `data-docx-css`
references; each CSS-map entry maps one reference to its declarations. Apply the
map when rendering the fragment. Named paragraph styles remain identified by
`data-docx-style` and are described in `styles`.
They include the entire document: headers, footers, notes, tables, fields,
revisions, images and MathML. They retain comment threads and source selectors
(`at`, `select`, `occurrence`), asset manifests linking inline image URLs to
package parts, and exact selectable text for paragraphs with revisions or
generated objects. They omit UI byte maps, the general UI stylesheet, duplicate
ordinary paragraph text and duplicate equation content. Ordinals and
`contenteditable` attributes are omitted from model HTML; paragraph IDs and
meaningful metadata remain. Direct-formatting declarations are kept in the
separate CSS map. Full-document reads are the default; compaction never selects
sections.

```ts
import { init, readHtml, renderHtmlDocument, resolveHtmlSelection, mountHtmlProjection } from "docxdriver";
await init(wasmBytes);
const projection = readHtml(docxBytes, "final");
const portableHtml = renderHtmlDocument(docxBytes, "final", "Preview");
mountHtmlProjection(container, projection);
// Supply asset payloads as the third argument, or resolve manifest URLs in the UI.
const selection = resolveHtmlSelection(container, browserRange, projection);
// selection supplies at, select, occurrence, start and end for a plan.
```

`mountHtmlProjection(container, projection, assets?)` mounts a UI projection with
unique DOM IDs and local note links for each instance, including repeated copies
of the same document. Source paragraph addresses remain in `data-docx-paragraph`;
`resolveHtmlSelection` returns those original addresses. Byte offsets in
`source_map` continue to refer to `projection.html`, rather than serialized mounted
DOM. Optional `assets` are the payload records from `read_kind: "assets"`.

`renderHtmlDocument` produces a standalone HTML document with embedded image
assets. Ordinary reads use content-addressed asset URLs without base64 payloads;
`read_kind: "assets"` retrieves the payloads. This is a semantic preview, not a
Word pagination engine. Unsupported drawings and math constructs retain source
XML and render explicit placeholders. Editing an unsupported MathML placeholder
is rejected.

Reads preserve paragraphs and tables inside nested content controls and custom
XML wrappers, including controlled table rows and cells. Section boundaries and
source addresses stay intact.

Creation accepts HTML5 paragraphs, headings, nested lists, tables (including
merged cells and nested tables), data-URI images, links, inline formatting, and a
bounded CSS subset. List items currently support one paragraph each; use `<br>`
for multiple lines. Multi-paragraph items return a diagnostic. Upper/lower MathML decorations honor `accent`/`accentunder`, including explicit
false values. Unsupported non-stretching or combined `munderover` accents are
rejected; use nested `munder`/`mover` for combined decorations. Unsupported
MathML visual attributes are rejected. Replacement content is inline HTML; paragraph/table wrappers,
revision markup and generated document objects require their dedicated operations.
HTML entities and quoted or unquoted attributes follow HTML5 rules. Scripts,
unsafe URLs and unsupported styles return diagnostics.

### Formatting contract

The [machine-readable capability matrix](schema/formatting-capabilities.json)
distinguishes HTML authoring, typed editing, preview fidelity and V1 Python
REPL exposure. `preserved_only` means source OOXML survives unrelated edits
but the formatting is not represented in the preview. `partial` means the
bounded behavior described in that entry; it does not promise Word rendering
parity. The matrix is also exported as `docxdriver/schema/formatting-capabilities.json`.

For `format_text`, omitted fields leave their direct OOXML untouched, including
underline variants/colors, themed colors and explicit off values. A boolean
`false` writes an explicit off override; superscript/subscript `false` writes
baseline alignment. `clear` removes the named direct property so the effective
style/default can supply it again. Clearing either superscript or subscript
removes the shared vertical-alignment property. If a property is both set and
cleared, clearing wins. An explicit set replaces metadata on that requested
property: `underline: true` sets single underline and a color sets literal RGB.

`format_paragraph` follows the same omission and clear/inherit rules for its
supported properties. Clearing a style removes the explicit paragraph-style
reference and allows the default paragraph style. It preserves independent
paragraph-mark and section properties when changes are rejected. Text and
paragraph formatting always record property revisions when changed, even in
a direct-mode plan; accept/reject them with `revision_settle`.

Table, row and cell styling attributes (including `style`, `class`, dimensions,
borders, fills and alignment) are rejected during HTML authoring. Apply supported
paragraph/inline formatting inside a cell instead. Inline `style` is also rejected
on containers without a formatting mapping, such as lists, links, images and
section wrappers. Put supported text formatting on the contained paragraph/span.
These rejections return diagnostics and no document output.

### Editing equations

Canonical equations are presentation MathML:
`<math display="inline"><msup><mi>x</mi><mn>2</mn></msup></math>`.
Set `display="block"` for display math. No LaTeX is needed. Legacy
`<equation>` input remains accepted for compatibility, but output always uses
MathML and typed operations use the `mathml` field (replacing `latex`).

Default equation records contain `at`, `equation` (1-based), `display`, and
`editable`; MathML appears once in the HTML. The UI profile also includes standalone
`mathml`. Use a dedicated operation rather than selecting reconstructed equation
text:

```toml
[[ops]]
op = "replace_equation"
at = "11111111"
equation = 2
mathml = '<math><mfrac><mrow><mi>a</mi><mo>+</mo><mi>b</mi></mrow><mi>c</mi></mfrac></math>'
```

Omit `equation` only when the paragraph has one equation. Omit `display` to
preserve layout. Preview and commit the source-bound plan as usual. Tracked
replacement and deletion retain the original OMML for rejection. Settle existing
revisions before re-editing their equations. Python exposes the same `mathml`
argument in `ReplaceEquation`; `DeleteEquation` remains unchanged.

### Verification

From a source checkout, install Rust with rustup and `wasm-pack`, then run
`npm ci` and `npm run build`. Generated WASM and JavaScript outputs are ignored
by Git. Tests rebuild the runtime from source.

Run `npm run test:e2e` after installing Playwright Chromium.
The test compares native and WASM projections, renders them in Chromium, resolves
actual DOM selections, previews and commits tracked text/math edits, checks
original/final views, and rejects a stale source. Artifacts are saved under
`target/html-e2e`.

The [parity matrix](../../docs/html-projection-parity.md) covers the frozen prior
projection, all three revision views and 28 equation cases.

Model reads share repeated direct-formatting declarations through the CSS map,
while preserving their values and associations through `data-docx-css` attributes.
When combining fragments from different model reads, scope each fragment's CSS
map to its own container because reference keys are document-local.
Image `src` values use short document-local aliases; each asset entry maps `url`
to its canonical `asset_url` and original package `parts`. Asset payload reads
retain canonical URLs. Comment threads keep one ID and exact source selectors;
`selection_space` declares their revision view and character units once.

Paragraph indentation accepts signed point values for `indent_left`, `indent_right`, and `first_line_indent`. Negative first-line offsets mean hanging indentation; zero is an explicit zero; `clear: ["first_line_indent"]` removes the direct setting and inherits. These writes are selective and tracked like other paragraph formatting. Rust callers use `SignedPoints` for these three fields (rather than `Points`, which remains nonnegative for dimensions). For example: `{ "op": "format_paragraph", "at": "12345678", "indent_left": -6, "first_line_indent": -18 }`.

Setting or clearing `first_line_indent` also removes competing `firstLineChars`/`hangingChars` direct overrides, so the logical offset has one unambiguous mode. Other character/logical indentation attributes remain untouched.

List membership can be changed with `{"op":"set_list","at":["12345678"],"format":"bullet","level":0}`. Omit `num_id` to allocate a private list (decimal by default); supply an existing positive `num_id` to continue it. `start` is a positive restart value and clones an existing instance, preserving neighboring lists. Levels are 0–8. `remove:true` writes a zero numbering override, suppressing inherited membership while preserving paragraph indents. All membership edits carry paragraph-property revisions, including direct plans; settle pending paragraph revisions before editing again. Reject restores previous references; unused definitions deliberately remain so other revisions keep valid references. Paragraph reads expose `list` identity, level, format, start, value and continuation status; markers are outside selectable text.

A new `set_list` may supply `levels`, an array of one to nine definitions. Each definition supplies `format` and `label`; `%1` through the current level reference counters. Optional positive `start`, zero-based ancestor `restart_after`, signed `indent_left`/`indent_right`/`first_line_indent`, and `marker_bold`/`marker_italic`/`marker_font`/six-digit `marker_color` control that level. `levels` cannot restyle an existing `num_id`; creating a new instance keeps neighboring lists isolated. HTML `<li>` accepts multiple paragraphs and nested lists. Subsequent paragraphs explicitly belong to the same item, persist with an empty `_docxdriver_cont_…` bookmark and `numId=0` to suppress their Word marker, and read as `continuation_paragraph: true`. Ordinary paragraphs are never inferred as continuations from their indentation.

Direct structural edits use `edit_table` with a source-bound table handle and a tagged `action`: `insert_rows`, `delete_rows`, `insert_columns`, `delete_columns`, `merge_cells`, `split_cell`, or `delete`. Row/column coordinates and counts are one-based; insert actions specify `position: "before" | "after"`. Merge specifies `row`, `column`, `row_span`, and `column_span`. Split specifies its logical origin `row`/`column`. Insertion/deletion of rows or columns and merging require a rectangular grid without preexisting merges or omitted cells. Merge moves content in row-major order and preserves nested tables; split retains content in the first resulting cell and creates empty neighbors. Structural edits reject tracked mode and unresolved revisions, bookmarks, comments, fields, or content controls in the table. Remove a table instead of deleting every row or column.

`insert_table` accepts a paragraph `at`, `position`, `rows`, `columns`, and optional `as` alias. Dimensions are 1–100; this is direct-only and section-boundary anchors are rejected. Later operations can target the inserted table alias. Structural deletion preserves unused relationships; it does not collect media from elsewhere in the document.

Document reads expose `sections`: each section has a one-based compatibility index, a stable `section:<terminating-paragraph-ID>` handle (or `section:final`), paragraph boundaries, dimensions, margins, columns, numbering, first-page mode and explicit/inherited story references. `set_section_layout` accepts `section_handle` instead of `section`; handles follow their terminating paragraph when preceding sections are inserted. In a plan, `section:$alias` identifies the boundary on an inserted paragraph alias.

`set_section_boundary` inserts a boundary after body paragraph `at`, optionally with `break_type`; `remove: true` removes that boundary and merges preceding content into the following section using the following layout. Insertion clones the containing section properties. Removal preserves the following section's effective header/footer chains and the final body section. Structural boundaries require direct mode; pending paragraph/section property revisions and tracked boundary edits are rejected explicitly.

In HTML creation and header/footer setters, empty `<page-number/>` and `<page-count/>` helpers author Word PAGE and NUMPAGES fields. `<span data-docx-field="PAGE">1</span>` and NUMPAGES also accept a decimal cached result; Word recalculates dirty fields. These helpers do not calculate pagination or replace existing footer content. Other field instructions remain unsupported for authoring.

Row/column insert actions can assign an `as` group alias. For inserted rows, `$group/row:1` addresses the first inserted row and `$group/row:1/cell:2` its second cell. For inserted columns, `$group/row:2/cell:1` addresses the first inserted cell in the second row. Group coordinates refer to the inserted strip at creation and remain bound to those objects as positions shift. A bare group alias is ambiguous and is rejected.

Section layout edits use `set_section_layout`. Choose `page_size` (`letter`, `legal`,
`a4`, `a3`, `a5`) or paired positive `width`/`height` in points; `orientation`
normalizes their ordering. `columns`, `column_spacing`, `column_widths` and
`column_separator` configure equal or unequal columns within the section text
area. Setting `columns` selects equal-width mode unless `column_widths` is also
supplied; spacing/separator-only edits preserve existing unequal widths. `break_type` accepts `nextPage`, `continuous`, `oddPage`, `evenPage` or
`nextColumn`. `page_number_format` and `page_number_start` configure numbering;
`page_number_continue: true` removes a direct restart, while false is rejected.
Page-number starts are limited to 1–2,147,483,647, matching Word’s signed integer representation.
Visible numbering is authored separately with `<page-number/>` and `<page-count/>`.
Omitted fields preserve source properties. Section property changes honor the
plan's tracking mode and reject overlapping pending section-property changes.

`set_first_page_mode` sets a section's `enabled` boolean without deleting stored
first-page headers or footers. Setting first-kind chrome also enables that mode,
including when an imported `titlePg` currently holds a false value. Disabling,
re-enabling and rejecting tracked mode changes preserve the stored stories.
HTML section metadata communicates layout intent; browser previews do not
establish Word pagination fidelity.

Typed `set_list` membership changes on a list containing explicit continuation paragraphs are currently rejected, including edits to the numbered parent or a continuation independently. This preserves persisted item membership and tracked restoration. Multi-paragraph item creation and reconstruction remain available through HTML.

List IDs and start values are bounded to 1–2147483647. For new definitions, an operation-level `start` applies only to the selected level; other levels default to 1. An explicit per-level `start` takes precedence. Continuing an existing list allocates no definition and remains supported when the source numbering ID range is exhausted. Typed operations cannot join or alter a list instance containing persisted continuation paragraphs.

List definition review scaling can be checked with `cargo run --release -p docxdriver-core --example list_scaling -- PARAS OPS`. Numbering definitions are cached and indexed for a plan, then serialized at report/output boundaries.

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
