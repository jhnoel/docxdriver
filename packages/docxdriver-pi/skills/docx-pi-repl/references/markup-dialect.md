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
