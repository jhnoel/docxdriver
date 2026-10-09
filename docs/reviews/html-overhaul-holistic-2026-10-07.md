# Holistic HTML overhaul review — 2026-10-07

Disposition: all three findings below were fixed after this review. The original findings and evidence are retained as the historical review record; the resolution is recorded below.

This is a fresh top-down review by the primary agent, not a completed Astra review. Astra's requested repeat review was unable to execute because of a usage limit. No implementation files were modified during this review.

## Findings

### P1 — Complete reads silently omit body content controls

Location: `crates/docxdriver-core/src/html/surface.rs:251`, `collect_effective_section_children`.

The section collector includes direct paragraphs and tables, plus the effective AlternateContent branch, but does not descend through `w:sdt/w:sdtContent`. A valid DOCX with a visible paragraph and a second paragraph containing `Critical content inside a content control.` returns `outcome: completed` and HTML containing only the first paragraph. The paragraph iterator used by metadata and edits does enumerate that second source paragraph, so the source model and visible document disagree. The omitted text is not supplied elsewhere in the default document response.

This omission also reproduces in the pinned prior engine. It is an inherited limitation, not a new serialization regression. It nevertheless violates this overhaul's explicit full-document-context goal and the current documentation's complete-read promise. A generic parity check against the old format cannot detect it because both versions omit the same content.

Recommendation: traverse supported body content-control wrappers consistently with the source paragraph collector, preserving order and section boundaries. For unsupported wrappers, return an explicit diagnostic or visible placeholder rather than silently hiding their contents. Add an independent expected-content fixture covering content controls and paragraphs/tables inside them.

Evidence: `target/html-holistic-review/content-control.json`; the mutated DOCX is `target/html-holistic-review/content-control.docx`. Reproduction wraps the second created body paragraph in `<w:sdt><w:sdtContent>…</w:sdtContent></w:sdt>` and reads final view. The same request was run against the pinned prior WASM.

### P2 — Accepted MathML accent attributes change silently during writes

Locations: `crates/docxdriver-core/src/html/mathml_omml.rs:429` and `:449`.

The validator explicitly accepts `accent`, `accentunder`, and `stretchy`. Conversion does not consistently honor them:

- `<munder accentunder="true"><mi>x</mi><mo stretchy="true">¯</mo></munder>` successfully creates a DOCX, but reads back as a plain lower-limit structure with neither `accentunder` nor stretching.
- `<mover accent="false"><mi>x</mi><mo>ˆ</mo></mover>` successfully creates a DOCX, but reads back as `accent="true"` with a stretching accent.

The upper converter chooses an accent by character alone; the lower converter always emits a limit for non-nary bases. This affects accepted public HTML/MathML creation and equation replacement. It also means projected underbars cannot reliably be reused as replacement MathML. The current claim that unsupported visual MathML attributes are rejected does not cover these accepted-but-changed cases.

Recommendation: respect the MathML accent semantics when mapping to OMML bars, accents and group characters; reject supported syntax whose requested rendering cannot be represented. Add write/read semantic tests for every projected equation family, including explicit false attributes and lower accents, rather than only verifying that existing OMML can be read.

Evidence: `target/html-holistic-review/math-attributes.json`, generated through shipped WASM and inspected in Chromium. Both operations returned `completed`.

### P2 — Note links collide across independently mounted documents

Locations: `crates/docxdriver-core/src/html/render.rs:894` and `crates/docxdriver-core/src/html/surface.rs:181`.

Note reference URLs and appendix element IDs use only the note kind and local OOXML number, such as `#docx-footnote-7`. When two document projections are mounted in the same page, they emit duplicate IDs. Chromium resolves the second document's note link to the first document's note body. This remains after the shared-CSS scope fix; the existing multiple-document test checks color isolation only.

Recommendation: namespace DOM note IDs and matching fragment URLs by a document/render instance scope, while retaining the original note number in `data-docx-note-id`. Support multiple copies of the same document as well as different documents. Add a multi-document note-navigation test that asserts the second reference resolves within the second document.

Evidence: `target/html-holistic-review/note-links.json`. Mounting two final-view projections of the parity fixture yielded two `docx-footnote-7` elements; the second document's target was inside the first document.

## Top-down assessment

- **Public contract:** native HTML and presentation MathML are actually used; the private atom grammar remains internal. Default reads retain a single complete projection rather than selecting sections. The content-control finding limits the word “complete.”
- **Semantic metadata:** equation addresses/editability, final-view Unicode scalar selection exceptions, comment quote/occurrence anchors, full thread text/replies/status, and inline image-to-package-part mappings remain available. The earlier generic CSS collision is corrected with deterministic style-map scopes.
- **Mutation boundary:** the typed source hash, preview/commit key, direct/tracked operation paths and preservation of unrelated package parts remain in place. The MathML finding exposes a validation-versus-conversion mismatch at this boundary.
- **Browser integration:** native MathML, style resets, image payload resolution, source selection mapping, and styles across documents are exercised. Note navigation needs document-level DOM identity, in addition to stylesheet isolation.
- **Python delivery:** complete read output bypasses generic display caps, while explicit runtime limits remain. Saved document output and identical printed/returned output are covered. This review found no additional concrete output-delivery regression.
- **Documentation and measurements:** the shipped SKILL.md and markup reference are supplied identically to both arms of the documented skill-inclusive comparison. The report distinguishes that run from the earlier no-skill run and from the later CSS-corrected build. The controlled baseline is explicitly described as saved WASM plus restored display caps, not a historical host checkout. Provider wall-time uncertainty and reference-tokenizer differences are disclosed.
- **Test strategy:** frozen old/new inventories provide useful representational regression protection, but inherited omissions and accepted-write transformations require independent expected behavior. The three reproductions above expose gaps despite passing existing suites.

## Validation scope

Fresh projection parity tests and the existing six Chromium end-to-end tests were rerun. Additional probes used the shipped current WASM, an actual DOCX modified at the XML layer, the pinned prior WASM, and Chromium. Evidence is retained under `target/html-holistic-review/`. The existing passing suites do not cover the new findings. This is not a claim of exhaustive OOXML or browser security coverage.


## Resolution after review

All three findings are fixed in the worktree:

1. One shared effective-story traversal preserves nested content-control/custom
   XML content across sections, chrome/notes, table rows/cells and table anchors.
   A source-DOCX fixture independently checks complete content and order in all
   views, section boundaries, edits inside controls and insertion after a
   controlled body table without losing wrappers or unrelated package parts.
2. The MathML compiler honors explicit true/false accent settings, maps upper
   and lower decorations to OMML accents/bars/group characters, and emits explicit
   false settings for generic limits. Non-representable non-stretching accents
   and combined accented `munderover` return diagnostics instead of changing
   their rendering silently. Sixteen combinations of direction, boolean and
   decoration are tested through both creation and tracked equation replacement,
   alongside negative cases and Chromium checks.
3. Canonical note targets and URLs include a deterministic source-document scope.
   The exported `mountHtmlProjection` helper gives all DOM IDs and internal links
   an instance-local identity when multiple projections or repeated copies share
   a page. Original paragraph addresses remain in `data-docx-paragraph`, and
   selection resolution still supplies the source address. Browser tests verify
   unique IDs, local note targets, actual link navigation and source selection
   mapping. Projection byte offsets continue to describe the original HTML.

Validation: 98 Rust workspace tests, 118 Pi tests, nine Chromium end-to-end tests
and the schema drift check pass. This resolution is verified by the primary
agent; it is not a completed additional Astra review. Benchmark artifacts retain
their measured build identity and were not rerun against these later fixes.
