# Formatting support and roadmap

Baseline audited 2026-10-14 against source commit `4048452`; milestone 0 is
implemented in commit `bcce1be`. Later milestones are proposed in dependency
order, not scheduled releases or an exhaustive inventory of Word features.

## Current support

Support has three distinct meanings: displaying existing formatting, authoring
it through HTML, and changing it through typed operations. Preserving source
OOXML during an unrelated edit does not mean a feature is editable or faithfully
displayed. Recreating a document from its HTML preview is not a lossless round trip.

| Area | Available today | Missing or limited |
| --- | --- | --- |
| Character formatting | Typed selection edits for bold, italic, single underline, strike, superscript/subscript, RGB color, font size, and clearing those properties. HTML also accepts font family. | Typed font-family changes; highlight and shading; character styles; small/all caps; underline variants/color; double strike; character spacing, scaling and position; language and complex-script controls. |
| Paragraph formatting | Typed edits for an existing paragraph style, left/center/right/justified alignment, signed left/right indents and signed first-line/hanging offsets, before/after spacing, and multiple/exact/at-least line spacing. HTML accepts signed `text-indent` and paragraph margins. | Custom tab stops/leaders; keep-with-next, keep-lines, widow/orphan control, page-break-before; borders/shading; outline levels and text direction. |
| Styles and themes | Named paragraph styles are read and can be applied if declared in the document; a bounded style cascade supplies preview CSS. | Creating/updating styles, document defaults, character/table styles, theme fonts/colors, and full style-cascade fidelity. Authored stylesheets are rejected. |
| Lists | HTML creation supports nested bullets and ordered lists, ordered-list types/start values; reads retain list metadata and markers. | Typed conversion to/from lists, restart/continue numbering, level changes, custom multilevel labels/indents and marker formatting. Multi-paragraph items retain explicit continuation membership via persisted bookmarks and suppressed DOCX markers. |
| Tables | HTML creation supports tables, nested tables, and row/column spans; existing cell paragraphs remain addressable for text/paragraph edits. | Typed table/cell formatting and structural edits; configurable widths, borders, fills, padding, alignment, row heights, repeating header rows and table styles. Creation uses default table layout/borders; table/row/cell lowering forwards spans, not authored styling. |
| Pages and sections | Core exposes section-specific page margins including header/footer distance and gutter; header/footer operations support default/even/first kinds, plus an even/odd switch. HTML creation has basic section boundaries. | Page size/orientation, columns, section-break editing/type, page numbering configuration, page borders and section vertical alignment. `set_page_margins` is absent from the V1 Python capability manifest. |
| Images and drawings | HTML creation accepts inline data-URI images and dimensions; reads expose image assets and placeholders for unsupported drawings. | Typed resize/crop/replace and positioning; floating anchors, wrapping, rotation, drawing/text-box styling. |
| Equations and preview | Presentation MathML creation and dedicated equation replacement/deletion; semantic HTML/CSS preview. | Some MathML constructs/visual attributes are rejected. Unsupported constructs retain source XML/placeholders on reads. Word-equivalent pagination and complete visual fidelity are outside current preview support. |

## Milestones

### 0. Establish formatting fidelity and a capability contract — implemented

Implemented: the [capability matrix](../packages/docxdriver/schema/formatting-capabilities.json)
is checked against typed fields, Python exposure and native/WASM HTML examples.
Selection formatting now changes only requested direct properties; `false`
overrides inheritance and `clear` removes direct settings. Rich-property
regressions cover split runs, accept/reject and paragraph-mark/section
preservation. Unsupported table presentation and unmapped container styles
return diagnostics. Shared property insertion respects schema snapshot tails.

Scope completed:

- Publish a machine-checkable capability matrix for read, HTML authoring, typed
  edit, and Python exposure; distinguish unsupported from preserved-only.
- Audit the difference between explicit off, removing direct formatting, and
  inheriting a style. Define and test `clear` and `false` on styled documents.
- Add preservation regressions for rich existing run/paragraph properties.
  In particular, the former `write_run_state` normalized unrelated underlines;
  the replacement writer changes only requested properties. Double/dotted/dash
  underline variants and their metadata have preservation regressions.
- Make unsupported formatting observable. Table/row/cell HTML lowering currently
  forwards only span attributes; unsupported styling now produces diagnostics
  rather than an apparent successful formatting request.
- Add shared schema-order insertion helpers where needed, following the existing
  paragraph-property helper. Test against rich property bags, not only empty ones.

Exit: capability assertions and preservation regressions pass; unsupported
requests have explicit outcomes; inherited formatting behavior is documented.

Verification completed 2026-10-14: workspace tests, both npm package suites,
the installed Python wheel tests, and the public-content audit passed. The
release CLI conformance gate passed 1,537 inputs (1,531 LibreOffice corpus
documents plus six standard fixtures) with no new schema error kinds. Serial
and four-worker validation produced identical results on the standard fixtures.
A corpus-discovered misordered section tail has a permanent regression.

### 1. Complete everyday text and paragraph editing

- Add typed font family, first-line and hanging indents; expose existing page
  margins in the Python REPL.
- Add highlight, run/paragraph shading, small/all caps and richer underline
  settings. Keep Word highlight and arbitrary RGB shading as separate concepts.
- Add paragraph pagination controls and custom tab stops with alignment/leaders.
- Extend HTML input and read CSS/metadata alongside typed support, including
  negative `text-indent` mapped to hanging indentation.

Exit: these properties can be authored, inspected, changed and cleared without
replacing text; tracked changes can be accepted/rejected with prior properties
restored; native, WASM and Python agree on supported values.

### 2. Add reusable styles and editable numbering

- Create/update paragraph and character styles with explicit IDs, inheritance,
  defaults and cycle validation; apply character styles to selected text.
- Add typed list assignment/removal, level changes, restart/continue behavior
  and multilevel definitions with marker formatting and indents.
- Support multi-paragraph list items with an explicit continuation model.
- Preserve shared style/numbering definitions and specify the scope of updates
  so changing one list/style does not unexpectedly affect unrelated content.

Exit: a heading/body style set and a multilevel numbered document can be built
and restyled through plans; list continuity and style inheritance have fixtures.

### 3. Make tables format-editable

- Introduce stable table/row/cell addresses; paragraph addresses and body-level
  `table:N` insertion anchors are insufficient for cell-property operations.
- Add table/cell borders, shading, widths, padding and alignment; row heights,
  repeating headers and row pagination controls; table styles after milestone 2.
- Add insert/delete rows/columns and merge/split cells as a separate structural
  slice, preserving nested tables and validating grid/span consistency.
- Carry supported table styling through HTML input and preview metadata/CSS.

Exit: format a report table in place, including merged cells and nested tables;
unaffected cell contents and properties survive. Document the revision behavior
for each operation and reject unsupported tracked structural edits explicitly.

### 4. Expand section layout and image control

- Add page size/orientation, columns, section-break types, page-number settings
  and explicit first-page header/footer configuration.
- Add image addresses and typed resize/replace/crop, followed by floating
  positioning and wrapping with relationship/asset validation.
- Define revision behavior for section/drawing changes and report layout intent
  even where the semantic preview cannot reproduce Word pagination.

Exit: configure a mixed portrait/landscape document and edit image placement
without rebuilding its body or disturbing section/header relationships.

### 5. Advanced typography and visual coverage

- Theme fonts/colors, complex-script and East Asian fonts, language/RTL behavior,
  character spacing/scaling/position and advanced paragraph direction controls.
- Additional drawing/text-box and MathML coverage driven by real fixtures.
- Expand preview fidelity with explicit degradation metadata; decide separately
  whether exact pagination merits an external rendering integration.

Exit: each added feature has a bounded contract, representative fixtures and
clear read/write limitations. Full Word rendering parity is a separate project.

## Delivery gates

Every implementation slice must follow [the repository agent notes](../AGENTS.md):

- Add conformance scenarios for new operations; run the local ECMA-376 gate
  (`cargo build -p docxdriver-cli`, then `python scripts/ooxml-conformance.py`).
- Run the release CLI against the LibreOffice corpus for structural/property
  changes. Insert OOXML children at their schema sequence positions.
- Regenerate JSON schemas, TypeScript types/template, applicable Python manifest
  and dataclasses, and the SDK prelude; rebuild vendored WASM before npm tests.
- Update agent-facing skills and public contracts. Run workspace/package tests,
  the installed Python wheel tests where applicable, and the public-content audit.
- Verify preview/commit, source binding, unaffected-property preservation and
  original/final/all revision views where applicable. Keep plan review roughly
  linear as new selectors and property operations are introduced.

The [implementation checklist](formatting-implementation-checklist.md) sets the
current priority order: indentation, highlighting, lists, tables, then page and
section layout. It takes precedence over this broader roadmap's suggested order.

## Audit references

- [Typed operation model](../crates/docxdriver-core/src/api.rs)
- [Character and paragraph property writers](../crates/docxdriver-core/src/commands/typed.rs)
- [HTML/CSS property mapping and style resolution](../crates/docxdriver-core/src/html/styles.rs)
- [HTML input lowering](../crates/docxdriver-core/src/html/input.rs)
- [Document/table creation](../crates/docxdriver-core/src/html/build.rs)
- [Page margins and ordered paragraph properties](../crates/docxdriver-core/src/commands/layout.rs)
- [Python capability manifest](../packages/docxdriver-pi/scripts/plan-capability-manifest.mjs)
- [Public HTML contract](../packages/docxdriver/README.md)
