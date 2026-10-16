# Formatting implementation checklist

Updated 2026-10-15. These five priorities take precedence over the broader
[formatting roadmap](formatting-roadmap.md). The implementation uses `first_line_indent`, `highlight`, `set_list`,
`format_table`/`format_row`/`format_cell`, `edit_table`/`insert_table`,
`set_section_layout`, `set_section_boundary` and `set_first_page_mode`.
Bounded limitations and verification results are recorded below.

Delivery order: **indentation → highlighting → lists → tables → page/section layout**.
Ship each numbered slice with its own acceptance checks and the common delivery
checklist. Basic list editing does not require a complete style-definition API.
Table formatting should ship before the more complex grid edits.

- [x] Foundation: milestone 0, committed as `bcce1be`—selective property writes,
  clear/inherit behavior, schema-order helpers, capability matrix and regressions.

## 1. Indentation

Implemented signed left/right paragraph indentation and one signed first-line
offset; negative offsets mean hanging indentation. Other dimensions remain nonnegative.

- [x] **I1 — Contract:** extend `FormatParagraph` with first-line/hanging controls
  and clear behavior. Settle whether these use separate magnitudes or one signed
  first-line offset; expose a single unambiguous logical setting to callers.
- [x] **I2 — Signed values:** support negative left/right paragraph indents with
  paragraph-specific validation. Keep nonnegative margin, size and dimension
  constraints intact; document any Rust API type changes.
- [x] **I3 — Writer:** map the logical setting to `w:ind` first-line/hanging
  attributes. Setting one mode removes its conflicting direct mode. Zero is an
  explicit zero offset; clearing removes the direct setting and inherits.
- [x] **I4 — Preservation:** update requested indentation attributes only;
  preserve unrelated paragraph properties and character-based/logical indentation
  attributes unless the contract explicitly requires replacing a competing value.
- [x] **I5 — HTML/read:** accept negative CSS `text-indent` as hanging indentation;
  project supported left/right and first-line/hanging values consistently.
- [x] **I6 — Surfaces:** expose the fields and examples in native, WASM,
  TypeScript, the Python REPL and SDK reference.
- [x] **I7 — Acceptance:** change a styled paragraph between first-line and hanging
  modes, set zero and negative offsets, then clear. Verify effective inheritance,
  accept/reject restoration, unchanged text/IDs and untouched neighboring paragraphs.
- [ ] Complete the common delivery checklist.

## 2. Highlighting

Implemented Word palette highlighting with explicit none and clear/inherit semantics.

- [x] **H1 — Contract:** add a typed `highlight` property to `FormatText`, using
  the Word highlight palette from `ST_HighlightColor`. Include an explicit `none`
  override; `clear=["highlight"]` removes the direct setting and inherits.
- [x] **H2 — Writer:** apply `w:highlight` to selected runs without modifying
  text, fonts, shading, underline metadata or other unrequested properties.
- [x] **H3 — Inheritance:** resolve highlight from supported defaults/styles and
  direct formatting; distinguish absent, explicit none and a palette color.
- [x] **H4 — HTML:** support `<mark>` with yellow as its default and a documented
  palette attribute for other colors. Reject unknown values. Keep arbitrary RGB
  shading distinct; do not silently map it to a nearest highlight color.
- [x] **H5 — Preview:** retain the palette value in metadata and render its color
  in HTML/CSS. Distinguish document highlighting from revision/comment UI colors.
- [x] **H6 — Acceptance:** highlight a selection crossing run boundaries; change
  color, explicitly turn it off and clear it on a styled paragraph. Accept/reject
  restores prior highlighting and every unrelated run property.
- [ ] Complete the common delivery checklist.

## 3. List handling

Implemented typed membership, restart/continue, private multilevel definitions,
and persisted HTML multi-paragraph items. Typed membership changes on a list with
explicit continuations reject rather than detach continuation paragraphs.

### 3a. Basic list editing

- [x] **L1 — Read contract:** expose list instance identity, level, format, start
  and continuation information alongside paragraph addresses. Generated markers
  remain outside selectable text.
- [x] **L2 — Operations:** add typed assignment to a new/existing list, removal
  from a list and level changes for explicitly selected paragraphs.
- [x] **L3 — Numbering:** add restart-at-value and continue-existing-list controls;
  validate levels and starts and preserve separate neighboring lists.
- [x] **L4 — Definitions:** allocate/reuse `numbering.xml` definitions and
  relationships safely. Clone shared definitions when an edit targets one list;
  do not restyle other lists implicitly.
- [x] **L5 — Revision behavior:** track paragraph numbering-property changes and
  preserve prior references for rejection. Define the lifecycle of new definitions
  and reject unsupported shared-definition changes in tracked mode explicitly.
- [x] **L6 — Acceptance:** convert prose to bullets/numbering, indent/outdent,
  restart, continue and remove membership. Verify markers, text/IDs, unaffected
  lists, plan atomicity and accept/reject behavior.
- [ ] Complete the common delivery checklist.

### 3b. Multilevel and multi-paragraph items

- [x] **L7 — Multilevel definitions:** support bounded per-level number formats,
  label templates, start/restart rules, marker formatting and indentation. Reuse
  the indentation contract from section 1.
- [x] **L8 — Continuation model:** define explicit membership for an unnumbered
  continuation paragraph inside one list item, including its DOCX representation
  and persistence. Do not infer membership from matching indentation alone.
- [x] **L9 — HTML/read:** support and reconstruct multi-paragraph `<li>` content
  and nested lists under that model; retain clear diagnostics for ambiguous cases.
- [x] **L10 — Acceptance:** round-trip a three-level outline with restarted
  numbering, a multi-paragraph item and nested child lists. Check that ordinary
  prose between lists remains ordinary prose.
- [ ] Complete the common delivery checklist.

## 4. Tables

Implemented source-bound table/row/cell handles, selective formatting with
property revisions, and bounded direct structural operations with aliases.

### 4a. Addresses and formatting

- [x] **T1 — Addresses:** emit source-bound table/row/cell handles, including nested
  tables. Keep them stable inside a plan when positions change; provide aliases
  for inserted objects. Preserve existing body-level `table:N` insertion anchors.
- [x] **T2 — Cell model:** distinguish logical merged cells from physical merge
  continuation cells and define how addressing a merged cell affects its backing
  XML. Reject ambiguous/out-of-range targets with contextual diagnostics.
- [x] **T3 — Table formatting:** add width (auto/points/percent), column widths,
  autofit/fixed layout, alignment, borders and cell-spacing/default-padding controls.
- [x] **T4 — Cell formatting:** add width, borders, fill/shading, padding and
  vertical alignment; reuse paragraph operations for text alignment and indentation.
- [x] **T5 — Row formatting:** add height and height rule, repeating header rows
  and row splitting across pages. Support assignment of existing table styles
  without requiring a new style-definition subsystem.
- [x] **T6 — Writer/revisions:** preserve unspecified properties and relationships;
  support the relevant table/row/cell property snapshots and accept/reject behavior.
- [x] **T7 — HTML/read:** extend the bounded table/row/cell styling contract only
  for implemented properties. Expose dimensions, grid and merge metadata and render
  supported styling; keep diagnostics for unsupported presentation.
- [x] **T8 — Acceptance:** format a report table with a header row, merged cells,
  nested table, custom borders/fills and widths. Verify raw properties, preview,
  text preservation and accept/reject across all exposed surfaces.
- [ ] Complete the common delivery checklist.

### 4b. Structural editing

- [x] **T9 — Operations:** add insert/delete rows and columns, merge/split cells
  and table insertion/deletion, with aliases where later plan operations need them.
- [x] **T10 — Grid invariants:** update grid columns, cell spans and vertical merge
  chains consistently; preserve required cell paragraphs and nested content.
- [x] **T11 — Content rules:** specify merge content ordering, split content
  placement and treatment of comments, bookmarks, fields and pending revisions.
  Diagnose unsupported overlaps before emitting a candidate document.
- [x] **T12 — Tracking:** document and implement tracking per structural operation.
  Reject unsupported tracked edits rather than silently changing them to direct edits.
- [x] **T13 — Acceptance:** combine insertion, addressing an inserted object,
  merging/splitting and deletion in one plan. Verify grid consistency, preserved
  content/relationships, atomic failure and nested-table isolation.
- [ ] Complete the common delivery checklist.

## 5. Page and section layout

Implemented section inventory, layout, direct boundary editing, safe numbering
fields and explicit first-page mode, preserving stored first-page stories.

- [x] **P1 — Section inventory:** expose addresses, boundaries, page dimensions,
  orientation, columns, numbering, first-page mode and explicit/inherited story
  references. Define stable handles/aliases for structural plans and compatibility
  with existing numeric section selectors.
- [x] **P2 — Shared writer:** use schema-position helpers for section properties;
  preserve unrelated settings and header/footer relationships. Define tracked
  section-property snapshots separately from structural boundary revisions.
- [x] **P3 — Page size/orientation:** add named sizes and explicit dimensions,
  portrait/landscape changes, consistent width/height normalization and diagnostics
  for conflicting or invalid inputs. Preserve unspecified margins and section data.
- [x] **P4 — Columns:** add count, spacing, separator and explicit unequal widths;
  validate the available text area and preserve unspecified column settings.
- [x] **P5 — Break types:** configure next-page, continuous, odd-page, even-page
  and next-column boundaries using schema-defined values.
- [x] **P6 — Boundary editing:** add insertion/removal of section boundaries at
  paragraph anchors. Respect section properties applying to the preceding content,
  preserve the final body section and define inheritance and merge behavior.
- [x] **P7 — Page numbering:** add format and start/restart/continue settings.
  Provide safe PAGE/NUMPAGES field authoring helpers for visible numbering;
  changing section settings must not implicitly replace footer content.
- [x] **P8 — First-page configuration:** add an explicit per-section enable/disable
  switch. Read/write boolean values correctly, including an existing `titlePg`
  with a false value. Disabling preserves stored first-page stories; re-enabling
  restores their use. Keep existing first-kind setter behavior compatible.
- [x] **P9 — Surface parity:** expose page margins in the Python REPL together
  with these layout operations, and publish semantic layout metadata in previews.
- [x] **P10 — Acceptance:** create a document with portrait and landscape sections,
  one/two/unequal columns, restarted numbering and distinct first/even/default
  stories. Toggle first-page mode without losing content and test inherited
  stories across inserted/removed boundaries and supported accept/reject paths.
- [x] **P11 — Rendering verification:** check representative output in Word or
  LibreOffice; the HTML preview communicates layout intent and does not establish
  Word pagination fidelity.
- [ ] Complete the common delivery checklist.

## Common delivery checklist — required for every slice

- [x] Preserve existing JSON/TOML inputs and source-bound preview/commit semantics;
  explicitly document new field validation and any public Rust type changes.
- [x] Define omitted/set/explicit-off/clear semantics, inheritance, precedence
  and revision behavior before writing properties. Preserve unrelated source OOXML.
- [x] Insert elements at schema positions and normalize qualified attributes;
  test existing rich property bags and imported ordering, not only fresh documents.
- [x] Add realistic regressions for source preservation, stale addresses, atomic
  rejection and original/final/markup views. Exercise native/WASM/Python agreement.
- [x] Update the capability matrix, HTML contract, operation reference and
  agent-facing skills as each capability becomes available.
- [x] Update `generate_typed_schema.rs`'s type template and regenerate JSON schemas
  and TypeScript types with `sh packages/docxdriver/scripts/generate-schema.sh`.
- [x] Add new Python operations/prose to the capability manifest; regenerate
  dataclasses and the SDK prelude with the scripts specified in [AGENTS.md](../AGENTS.md).
- [x] Rebuild vendored WASM before npm tests; build/install the current Python
  wheel before the SDK tests.
- [ ] Add scenarios for every new operation and property combination; run the
  local ECMA-376 schema gate. Run the release corpus gate for structural/property
  changes; `--jobs 4` checks independent documents without reducing coverage.
- [ ] Run workspace, both package and applicable Python tests, the public-content
  audit and a plan-scaling check when new addressing or definition scans could
  affect review cost. Verify operation count remains approximately linear.
- [ ] Record verification results and check off the completed slice in this file.

All five implementation areas are present with the bounded limitations below.
The common delivery markers remain incomplete until every final verification
requirement has passed, including the public-content audit.

Implementation verification notes (2026-10-15): highlighting native regressions cover palette/default/style reads, explicit none and clear, rich run-property preservation, cross-run selection, and accept/reject. Table formatting native regressions cover merged-cell backing XML, nested-table isolation, geometry/borders/fill/header rows, complete property restoration, stable source handles, inserted aliases, invalid values and atomic failure. HTML table authoring uses typed JSON `data-format` attributes, and preview exposes direct supported CSS; complete table-style visual inheritance and pagination fidelity are not claimed. Full generated-surface and three-validator conformance results belong to the common delivery checks.


Bounded behavior: structural table and section-boundary edits require direct mode.
Table row/column grid edits and merging require rectangular unmerged grids; split
supports modern gridSpan/vMerge, while legacy hMerge and unsafe annotation/revision
overlaps reject. Table-style assignment is supported, but complete inherited table
style rendering is outside the preview contract. Continuation membership is explicit
and persists through HTML round trips; typed set_list edits to any member of such
an instance reject until bookmark membership can be tracked and restored safely.

Rendering verification: a representative report was opened and exported by
LibreOffice 26.2.4.2 on 2026-10-15. The PDF has three A4 pages with portrait,
landscape, portrait dimensions; the landscape column separator, nested table,
highlighted selection and page fields render. This uncovered and fixed body
section boundaries accidentally placed in nested table cell paragraphs. Word smoke
verification requires explicit permission because the prescribed smoke test
force-quits Word (AGENTS.md).


Verification (2026-10-15): Rust workspace 165 tests, Python SDK
17 tests against a freshly built wheel, and Pi package 123 tests pass.
Core package 34 tests cover WASM formatting and inserted table group aliases.
Schema/TypeScript/Python generation and drift checks pass. The required prek
hook is installed. Plan review for 1,500 paragraphs and 150/300/600 operations
now takes approximately 57/103/213 ms after replacing per-operation full-document
revision-ID scans with incremental allocation bookkeeping; source IDs and
uniqueness remain covered by regression tests. Independent new list definitions
for 75/150/300 operations take approximately 41/65/114 ms using the new
`list_scaling` example and a per-plan numbering working set.

The public-content audit passes. Manual Word experiment documents remain local
and are ignored by Git. The full corpus conformance results are recorded below.

The three-validator corpus run covered 1,537 documents. It exposed a section
boundary insertion bug: changing a cloned break type appended a duplicate
`w:val`. The writer now replaces that attribute, with a dedicated regression
test preserving the following section's original break type. All 126 failing
documents were rechecked after the fix. The remaining 35 diagnostics across
12 documents exactly match a separate run against committed version `6ee3dcd`;
there are no additional diagnostics. These existing failures involve text-effect
property ordering, imported row/paragraph property ordering, comment/field/bookmark
settlement, story relationships, and tracked table grids. The full corpus is
therefore not entirely green; the six-document required commit gate is checked
separately.
