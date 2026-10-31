# HTML document surface

Document reads return HTML5, with paragraph IDs and `data-docx-*` metadata.
Use standard `p`, `h1`–`h6`, `ol`, `ul`, `li`, `table`, `tbody`, `tr`, `td`,
`img`, `header`, `footer`, and `section` elements. Equations are native
presentation MathML `<math>` elements. Notes are linked references and separate
note bodies; fields and comments use spans with metadata.

HTML entities, single/double quotes, and unquoted attributes follow HTML5 rules.
Use `<br>` for explicit line breaks; literal formatting newlines become spaces.
A bounded inline CSS subset supports fonts, sizes, colors, decoration and
paragraph alignment, spacing and indents. Unsupported CSS returns diagnostics.
Each list item supports one paragraph; use `<br>` for line breaks.
Multi-paragraph list items are rejected rather than silently renumbered.

## Inline replacement content

`with_` accepts text and safe inline HTML:

```html
<strong>bold</strong> <em>italic</em> <u>underline</u> <s>strike</s>
<sup>sup</sup> <sub>sub</sub> <a href="https://example.com">link</a> <br>
<span style="color:#0055AA;font-size:14pt">styled text</span>
<math display="inline"><mfrac><mi>a</mi><mi>b</mi></mfrac></math>
```

Do not include block wrappers, images, notes, fields, comments, or revision
markup in replacements. Use dedicated operations for document objects.
The engine owns tracked `ins` and `del` elements. Scripts, event handlers,
unsafe URLs, malformed structures, and unsupported math return diagnostics.

## Equations

Inspect `docx_read(path).equations`. Replace using
`ReplaceEquation(at=eq["at"], equation=eq["equation"], mathml="<math><mi>x</mi></math>")`.
Native MathML compiles directly to OMML; no LaTeX is required. Display mode is
preserved unless specified. Tracked operations retain original OMML for rejection.

## Source selection and quotation audit

Use paragraph IDs in HTML and exact `ReadResult.selections` exceptions for
operations. Comments provide ready-to-use source selectors in `ReadResult.comments`.
Rendered equation text, list labels and note numbers are not selectable source
text. HTML escaping does not bypass quotation audit: decoded quotation marks
are still audited. Never alter the reserved quotation audit comments.

`InsertParagraph` also accepts `table:N` anchors from body-level
`<table data-docx-table="N">` markup and may set `as_="$name"` for later ops.
A list `with_` inserts one paragraph per item in list order; `$name` is the group (for `SetList`, `DeleteParagraphs` and insert anchors) and `$name_N` each item.

Repeated styles use native CSS classes and an embedded stylesheet; the result
remains renderable HTML5. Preserve formatting when authoring replacement text;
translate any referenced class to its equivalent inline CSS. Image aliases map
once to canonical assets and original package paths in `ReadResult.assets`.


Upper and lower MathML decorations preserve explicit `accent`/`accentunder`
values. Use nested `munder`/`mover` for combined decorations; unsupported
non-stretching accents and accented `munderover` return a diagnostic.
Content controls may contain source paragraphs and tables; full document reads
include their content in order. Treat DOM note IDs as opaque and retain the
original note identity from `data-docx-note-id`.

### Word highlighting

Use `<mark>text</mark>` for yellow or `<mark data-highlight="cyan">text</mark>` for a palette color. The exact palette is black, blue, cyan, green, magenta, red, yellow, white, darkBlue, darkCyan, darkGreen, darkMagenta, darkRed, darkYellow, darkGray, lightGray, and none. Unknown values are rejected. This is Word highlighting; arbitrary RGB shading is separate. `format_text` accepts `highlight` with these values; `none` overrides inherited highlighting and `clear=["highlight"]` removes direct highlighting. Unspecified properties are preserved, and revision rejection restores the prior run properties. Preview spans retain `data-docx-highlight` and display the palette color.

Paragraph `margin-left`, `margin-right`, and `text-indent` accept signed pt/px lengths. Negative `text-indent` creates hanging indentation; zero writes an explicit first-line zero. Typed `format_paragraph` uses signed `first_line_indent` in points, with the same convention. `clear=["first_line_indent"]` removes direct first-line/hanging attributes and inherits. Left/right and first-line writes preserve unrelated character and logical indentation attributes.

Setting or clearing `first_line_indent` also removes competing `firstLineChars`/`hangingChars` direct overrides, so the logical offset has one unambiguous mode. Other character/logical indentation attributes remain untouched.

`<li>` accepts multiple paragraphs and child lists, retaining one marker per item. Continuation paragraphs carry explicit persisted DOCX membership; matching indentation alone does not establish membership. Read metadata identifies them with `continuation_paragraph`. Typed `set_list` accepts bounded `levels` definitions for a new list: format/label, start/restart_after, signed paragraph indentation and marker font/color/bold/italic. Array positions are levels; label counters may reference the current level and ancestors.

### Table formatting

The document read result includes a `tables` inventory with source-bound handles, grid widths in twips, physical row/cell indices, grid spans, vertical merge metadata and direct properties. Tables are numbered in preorder, including nested tables. Existing `table:N` insertion anchors remain body-level anchors. Use the inventory handle for `format_table`, `format_row`, or `format_cell`; handles retain source identity across a plan even when structural operations shift positions. Inserted tables can use `$alias` handles.

All three operations take `at` and a typed `format` object. Widths are `{"unit":"auto"}`, `{"unit":"points","value":180}`, or `{"unit":"percent","value":75}`. Percent values must be greater than zero and at most 100. `column_widths` are positive point values matching the existing grid column count. `layout` is `fixed` or `autofit`; table alignment is `left`, `center`, or `right`. `padding` contains optional `top`, `right`, `bottom`, and `left` point values. `borders` contains optional `top`, `left`, `bottom`, `right`, `inside_horizontal`, and `inside_vertical` objects with `style`, optional `color` (`#RRGGBB`) and optional `width` (0.25–12 points in quarter-point increments). Supported border styles are nil, none, single, double, dotted, dashed, dotDash, dotDotDash, triple, wave, and doubleWave. `cell_spacing` uses points. `style` selects an existing table style ID.

Rows support `height` in points, `height_rule` (`auto`, `at_least`, `exact`), `repeating_header`, and `allow_split`. Cells support width, borders, `fill` (`#RRGGBB`), padding, and `vertical_alignment` (`top`, `center`, `bottom`). Address a vertically merged logical cell through its restart cell; formatting applies to its backing continuation cells. Addressing a continuation directly is rejected. Legacy horizontal merges require normalization before formatting.

Omitted properties preserve direct OOXML. Explicit false overrides row flags. `clear` removes supported direct settings and inherits; it wins over a value supplied in the same format. Grid column dimensions cannot be cleared. Tracked edits preserve complete property snapshots; accept keeps the new formatting and reject restores prior properties. Settle an existing pending change to a property bag before formatting it again.

HTML creation accepts the same typed format objects as JSON in `data-format` attributes on `<table>`, `<tr>`, and `<td>` (or `<th>`). For example:

```html
<table data-format='{"width":{"unit":"percent","value":75},"layout":"fixed"}'>
  <tr data-format='{"repeating_header":true}'>
    <td data-format='{"fill":"#EEEEEE","padding":{"top":4}}'><p>Heading</p></td>
  </tr>
</table>
```

Unknown attributes inside the format object and invalid values are rejected. Arbitrary table CSS remains outside the authoring contract. HTML preview carries `data-docx-handle` and renders supported direct dimensions, alignment, borders, padding, fill, row height and split intent. It communicates layout intent and does not establish Word pagination fidelity.

Use empty `<page-number/>` and `<page-count/>` helpers inside paragraphs (including header/footer HTML) to author Word PAGE/NUMPAGES fields. A decimal cached value can also be supplied with `<span data-docx-field="PAGE">1</span>`. Word recalculates these fields; the HTML preview shows cached values, not page layout fidelity.

Document `sections` metadata contains stable `section:<boundary-paragraph-ID>` / `section:final` handles. Pass `section_handle` to `set_section_layout` instead of a numeric section. `set_section_boundary(at=...)` inserts after a body paragraph; `remove=True` merges into the following section using its layout. Insertion clones containing section properties. These structural edits require direct mode; settle pending section/paragraph property changes first. The final body section is retained, and removing a boundary preserves effective following story references.

`set_list` rejects membership edits on lists containing explicit continuation paragraphs, including numbered parents and continuations independently. Use the supported HTML creation/reconstruction model; independent tracked continuation membership changes are not implemented.
