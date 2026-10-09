# HTML projection parity

Projection version 2 preserves the information represented by the prior projection
while using renderable HTML5 and presentation MathML. Default reads return one
complete `markup` HTML fragment plus a document-local CSS map. The explicit
`document_ui` read returns one `html` fragment with selection metadata; block
entries reference byte ranges. Equation operations now accept `mathml`.

The differential fixture freezes actual output from commit
`0e5a0f9c559b3623bf5792e187c8c846fe627dfc` in all three revision views. It is checked
in beside the source DOCX under
`crates/docxdriver-core/tests/fixtures/projection-parity/`. Tests compare semantic
inventories, rather than requiring the old serialization or appendix order.

| Prior representation | HTML / MathML representation |
| --- | --- |
| Paragraph IDs, ordinals, styles and break revisions | IDs, `data-docx-*` metadata and source paragraph spans |
| Bold, italic, underline, strike, superscript and subscript | Semantic inline elements; explicit formatting resets retained |
| Numbered paragraphs and nesting | Native lists plus original numbering and paragraph metadata |
| Sections and default, first and even header/footer slots | Sections, headers and footers with slot and section metadata |
| Merged and nested tables | Native tables with row/column spans and source anchors |
| Fields and instructions | Field spans retaining instructions, including across joined paragraphs |
| Insertion/deletion IDs and authors | `ins` / `del` with revision metadata |
| Pending formatting, including neutral resets | Formatting spans with revision metadata |
| Comment milestones and threads | Hidden marker spans, complete thread text and exact source selectors |
| Footnote/endnote references and bodies | Linked references and note asides, retaining body blocks and formatting |
| Hyperlink destinations | Native links; unsafe destinations retained in `data-docx-href` with inert `href` |
| Image relationships, dimensions and descriptions | Inline images with original package paths and mapped asset aliases; unsupported placeholders retain dimensions, descriptions and paths |
| Tabs and typed line/page/column breaks | Tab spans and native breaks with type metadata |
| Equation positions, editability and display mode | Same descriptors with native MathML content |
| Prior OMML equation cases | MathML runs, scripts, fractions, roots, limits, functions, accents, groups, delimiters, matrices, arrays, boxes and phantoms |
| Unknown equation fallback | Explicit unsupported placeholder retaining source XML |

The fixture covers 28 equation cases, Unicode text, alternate content, paragraph
break revisions, six header/footer slots and all three revision views. Permanent
inline formatting is compared per character. Browser tests render all views,
check assets and note links, and resolve a selection in a joined source paragraph.
An additional conformance check compares the complete model and UI inventories
in all three views, allowing only omission of redundant ordinal attributes.

There are deliberate improvements: notes remain available in every view, source
paragraph addresses are canonical even when the original XML contains invalid or
missing IDs, and unsupported equation XML remains recoverable. Note bodies move
to an appendix, so table and note inventories allow relocation while preserving
their contents and anchors.

This is representational parity for the prior projection's constructs, not a claim
of complete OOXML support or Word pagination fidelity. The preview is a semantic
web rendering. Creation supports one paragraph per list item; use a line break
inside that paragraph when needed. The prior projection represented lists as
numbered paragraphs and did not provide native multi-paragraph list-item input.

Run `cargo test -p docxdriver-core --test projection_parity` for the offline
differential checks, and `npm --prefix packages/docxdriver run test:e2e` for Chromium
checks. To regenerate the baseline, supply a reader built from the pinned prior
commit to `scripts/generate_projection_parity.py --baseline-reader PATH`.

Compact model reads retain all source content. Repeated direct-formatting
declarations share entries in the CSS map referenced by `data-docx-css`. HTML
parsing supplies MathML namespaces, while standalone equation metadata keeps
its namespace declaration. Comment selectors use `selection_space` (final view,
Unicode scalar offsets); exact selection-text exceptions accompany ambiguous
paragraphs. Comment markers alone do not require a duplicate paragraph string.
The browser tests compare computed formatting, MathML namespaces and image
mappings between compact and UI profiles.


The holistic-review regressions are now covered independently of the frozen prior
format: complete reads include body content controls and controlled table rows,
cells and nested tables in every view. MathML creation and tracked replacement
retain explicit upper/lower accent semantics. Note fragment IDs are scoped by the
source document; the browser mounting helper assigns instance-local DOM IDs and
links for repeated copies while retaining original paragraph source addresses.
