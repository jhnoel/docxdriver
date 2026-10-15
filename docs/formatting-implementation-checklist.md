# Formatting implementation checklist

Updated 2026-10-14. These five priorities take precedence over the broader
[formatting roadmap](formatting-roadmap.md). Operation and field names below
are proposals to settle before implementation, not available API names.

Delivery order: **indentation → highlighting → lists → tables → page/section layout**.
Ship each numbered slice with its own acceptance checks and the common delivery
checklist. Basic list editing does not require a complete style-definition API.
Table formatting should ship before the more complex grid edits.

- [x] Foundation: milestone 0, committed as `bcce1be`—selective property writes,
  clear/inherit behavior, schema-order helpers, capability matrix and regressions.

## 1. Indentation

Existing typed edits cover nonnegative left/right indents. HTML creation accepts
positive first-line indentation; reads can display existing hanging indentation.

- [ ] **I1 — Contract:** extend `FormatParagraph` with first-line/hanging controls
  and clear behavior. Settle whether these use separate magnitudes or one signed
  first-line offset; expose a single unambiguous logical setting to callers.
- [ ] **I2 — Signed values:** support negative left/right paragraph indents with
  paragraph-specific validation. Keep nonnegative margin, size and dimension
  constraints intact; document any Rust API type changes.
- [ ] **I3 — Writer:** map the logical setting to `w:ind` first-line/hanging
  attributes. Setting one mode removes its conflicting direct mode. Zero is an
  explicit zero offset; clearing removes the direct setting and inherits.
- [ ] **I4 — Preservation:** update requested indentation attributes only;
  preserve unrelated paragraph properties and character-based/logical indentation
  attributes unless the contract explicitly requires replacing a competing value.
- [ ] **I5 — HTML/read:** accept negative CSS `text-indent` as hanging indentation;
  project supported left/right and first-line/hanging values consistently.
- [ ] **I6 — Surfaces:** expose the fields and examples in native, WASM,
  TypeScript, the Python REPL and SDK reference.
- [ ] **I7 — Acceptance:** change a styled paragraph between first-line and hanging
  modes, set zero and negative offsets, then clear. Verify effective inheritance,
  accept/reject restoration, unchanged text/IDs and untouched neighboring paragraphs.
- [ ] Complete the common delivery checklist.

## 2. Highlighting

Highlighting is currently preserved in source OOXML but cannot be authored or
changed through the supported formatting contract.

- [ ] **H1 — Contract:** add a typed `highlight` property to `FormatText`, using
  the Word highlight palette from `ST_HighlightColor`. Include an explicit `none`
  override; `clear=["highlight"]` removes the direct setting and inherits.
- [ ] **H2 — Writer:** apply `w:highlight` to selected runs without modifying
  text, fonts, shading, underline metadata or other unrequested properties.
- [ ] **H3 — Inheritance:** resolve highlight from supported defaults/styles and
  direct formatting; distinguish absent, explicit none and a palette color.
- [ ] **H4 — HTML:** support `<mark>` with yellow as its default and a documented
  palette attribute for other colors. Reject unknown values. Keep arbitrary RGB
  shading distinct; do not silently map it to a nearest highlight color.
- [ ] **H5 — Preview:** retain the palette value in metadata and render its color
  in HTML/CSS. Distinguish document highlighting from revision/comment UI colors.
- [ ] **H6 — Acceptance:** highlight a selection crossing run boundaries; change
  color, explicitly turn it off and clear it on a styled paragraph. Accept/reject
  restores prior highlighting and every unrelated run property.
- [ ] Complete the common delivery checklist.

## 3. List handling

Creation already supports nested bullets/numbering, list types and start values.
Typed membership, continuation and definition edits are missing.

### 3a. Basic list editing

- [ ] **L1 — Read contract:** expose list instance identity, level, format, start
  and continuation information alongside paragraph addresses. Generated markers
  remain outside selectable text.
- [ ] **L2 — Operations:** add typed assignment to a new/existing list, removal
  from a list and level changes for explicitly selected paragraphs.
- [ ] **L3 — Numbering:** add restart-at-value and continue-existing-list controls;
  validate levels and starts and preserve separate neighboring lists.
- [ ] **L4 — Definitions:** allocate/reuse `numbering.xml` definitions and
  relationships safely. Clone shared definitions when an edit targets one list;
  do not restyle other lists implicitly.
- [ ] **L5 — Revision behavior:** track paragraph numbering-property changes and
  preserve prior references for rejection. Define the lifecycle of new definitions
  and reject unsupported shared-definition changes in tracked mode explicitly.
- [ ] **L6 — Acceptance:** convert prose to bullets/numbering, indent/outdent,
  restart, continue and remove membership. Verify markers, text/IDs, unaffected
  lists, plan atomicity and accept/reject behavior.
- [ ] Complete the common delivery checklist.

### 3b. Multilevel and multi-paragraph items

- [ ] **L7 — Multilevel definitions:** support bounded per-level number formats,
  label templates, start/restart rules, marker formatting and indentation. Reuse
  the indentation contract from section 1.
- [ ] **L8 — Continuation model:** define explicit membership for an unnumbered
  continuation paragraph inside one list item, including its DOCX representation
  and persistence. Do not infer membership from matching indentation alone.
- [ ] **L9 — HTML/read:** support and reconstruct multi-paragraph `<li>` content
  and nested lists under that model; retain clear diagnostics for ambiguous cases.
- [ ] **L10 — Acceptance:** round-trip a three-level outline with restarted
  numbering, a multi-paragraph item and nested child lists. Check that ordinary
  prose between lists remains ordinary prose.
- [ ] Complete the common delivery checklist.

## 4. Tables

Creation supports merged/nested tables and editing their paragraph content.
Table/row/cell formatting and structural operations need their own addresses.

### 4a. Addresses and formatting

- [ ] **T1 — Addresses:** emit source-bound table/row/cell handles, including nested
  tables. Keep them stable inside a plan when positions change; provide aliases
  for inserted objects. Preserve existing body-level `table:N` insertion anchors.
- [ ] **T2 — Cell model:** distinguish logical merged cells from physical merge
  continuation cells and define how addressing a merged cell affects its backing
  XML. Reject ambiguous/out-of-range targets with contextual diagnostics.
- [ ] **T3 — Table formatting:** add width (auto/points/percent), column widths,
  autofit/fixed layout, alignment, borders and cell-spacing/default-padding controls.
- [ ] **T4 — Cell formatting:** add width, borders, fill/shading, padding and
  vertical alignment; reuse paragraph operations for text alignment and indentation.
- [ ] **T5 — Row formatting:** add height and height rule, repeating header rows
  and row splitting across pages. Support assignment of existing table styles
  without requiring a new style-definition subsystem.
- [ ] **T6 — Writer/revisions:** preserve unspecified properties and relationships;
  support the relevant table/row/cell property snapshots and accept/reject behavior.
- [ ] **T7 — HTML/read:** extend the bounded table/row/cell styling contract only
  for implemented properties. Expose dimensions, grid and merge metadata and render
  supported styling; keep diagnostics for unsupported presentation.
- [ ] **T8 — Acceptance:** format a report table with a header row, merged cells,
  nested table, custom borders/fills and widths. Verify raw properties, preview,
  text preservation and accept/reject across all exposed surfaces.
- [ ] Complete the common delivery checklist.

### 4b. Structural editing

- [ ] **T9 — Operations:** add insert/delete rows and columns, merge/split cells
  and table insertion/deletion, with aliases where later plan operations need them.
- [ ] **T10 — Grid invariants:** update grid columns, cell spans and vertical merge
  chains consistently; preserve required cell paragraphs and nested content.
- [ ] **T11 — Content rules:** specify merge content ordering, split content
  placement and treatment of comments, bookmarks, fields and pending revisions.
  Diagnose unsupported overlaps before emitting a candidate document.
- [ ] **T12 — Tracking:** document and implement tracking per structural operation.
  Reject unsupported tracked edits rather than silently changing them to direct edits.
- [ ] **T13 — Acceptance:** combine insertion, addressing an inserted object,
  merging/splitting and deletion in one plan. Verify grid consistency, preserved
  content/relationships, atomic failure and nested-table isolation.
- [ ] Complete the common delivery checklist.

## 5. Page and section layout

Margins and default/even/first header/footer setters already exist. Setting
first-kind chrome enables first-page mode; explicit mode toggling is missing.

- [ ] **P1 — Section inventory:** expose addresses, boundaries, page dimensions,
  orientation, columns, numbering, first-page mode and explicit/inherited story
  references. Define stable handles/aliases for structural plans and compatibility
  with existing numeric section selectors.
- [ ] **P2 — Shared writer:** use schema-position helpers for section properties;
  preserve unrelated settings and header/footer relationships. Define tracked
  section-property snapshots separately from structural boundary revisions.
- [ ] **P3 — Page size/orientation:** add named sizes and explicit dimensions,
  portrait/landscape changes, consistent width/height normalization and diagnostics
  for conflicting or invalid inputs. Preserve unspecified margins and section data.
- [ ] **P4 — Columns:** add count, spacing, separator and explicit unequal widths;
  validate the available text area and preserve unspecified column settings.
- [ ] **P5 — Break types:** configure next-page, continuous, odd-page, even-page
  and next-column boundaries using schema-defined values.
- [ ] **P6 — Boundary editing:** add insertion/removal of section boundaries at
  paragraph anchors. Respect section properties applying to the preceding content,
  preserve the final body section and define inheritance and merge behavior.
- [ ] **P7 — Page numbering:** add format and start/restart/continue settings.
  Provide safe PAGE/NUMPAGES field authoring helpers for visible numbering;
  changing section settings must not implicitly replace footer content.
- [ ] **P8 — First-page configuration:** add an explicit per-section enable/disable
  switch. Read/write boolean values correctly, including an existing `titlePg`
  with a false value. Disabling preserves stored first-page stories; re-enabling
  restores their use. Keep existing first-kind setter behavior compatible.
- [ ] **P9 — Surface parity:** expose page margins in the Python REPL together
  with these layout operations, and publish semantic layout metadata in previews.
- [ ] **P10 — Acceptance:** create a document with portrait and landscape sections,
  one/two/unequal columns, restarted numbering and distinct first/even/default
  stories. Toggle first-page mode without losing content and test inherited
  stories across inserted/removed boundaries and supported accept/reject paths.
- [ ] **P11 — Rendering verification:** check representative output in Word or
  LibreOffice; the HTML preview communicates layout intent and does not establish
  Word pagination fidelity.
- [ ] Complete the common delivery checklist.

## Common delivery checklist — required for every slice

- [ ] Preserve existing JSON/TOML inputs and source-bound preview/commit semantics;
  explicitly document new field validation and any public Rust type changes.
- [ ] Define omitted/set/explicit-off/clear semantics, inheritance, precedence
  and revision behavior before writing properties. Preserve unrelated source OOXML.
- [ ] Insert elements at schema positions and normalize qualified attributes;
  test existing rich property bags and imported ordering, not only fresh documents.
- [ ] Add realistic regressions for source preservation, stale addresses, atomic
  rejection and original/final/markup views. Exercise native/WASM/Python agreement.
- [ ] Update the capability matrix, HTML contract, operation reference and
  agent-facing skills as each capability becomes available.
- [ ] Update `generate_typed_schema.rs`'s type template and regenerate JSON schemas
  and TypeScript types with `sh packages/docxdriver/scripts/generate-schema.sh`.
- [ ] Add new Python operations/prose to the capability manifest; regenerate
  dataclasses and the SDK prelude with the scripts specified in [AGENTS.md](../AGENTS.md).
- [ ] Rebuild vendored WASM before npm tests; build/install the current Python
  wheel before the SDK tests.
- [ ] Add scenarios for every new operation and property combination; run the
  local ECMA-376 schema gate. Run the release corpus gate for structural/property
  changes; `--jobs 4` checks independent documents without reducing coverage.
- [ ] Run workspace, both package and applicable Python tests, the public-content
  audit and a plan-scaling check when new addressing or definition scans could
  affect review cost. Verify operation count remains approximately linear.
- [ ] Record verification results and check off the completed slice in this file.

First implementation slice: **I1–I7, indentation**, followed by **H1–H6,
highlighting**. Each slice includes the common delivery checklist.
