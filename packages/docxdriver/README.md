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
