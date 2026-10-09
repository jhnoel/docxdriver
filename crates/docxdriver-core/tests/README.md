# Core integration tests

`cargo test -p docxdriver-core` runs the typed core boundary tests. The old
dispatcher-era integration targets were intentionally removed with the
compatibility API; they tested legacy command names, raw `Value` dispatch,
operation lists, and `batch`/`typed_plan` shapes that are not part of the
clean-sheet contract.

`typed_api.rs` is the conformance suite for the public
`Request`/`Command`/`Plan` API. It covers:

- `create`, `read` (document, styles, comments, revisions), `find`, and the
  expected-source compare-and-swap guard;
- all 13 `EditOp` variants through both an individual `Command::Edit` and a
  one-entry `Plan`;
- direct and tracked edits, aliases, deterministic inserted IDs, atomic
  preview/commit, stale source and preview-key rejection, and no-byte failure
  behavior;
- canonical JSON/TOML round trips, strict unknown-field rejection, point-unit
  validation, aliases, operation diagnostics, and JSON/TOML source spans;
- structural document inspection with renderable HTML and native MathML.

`projection_parity.rs` compares all three views against frozen output from the
prior implementation, including 28 equation cases. `html_e2e.rs` exercises HTML
creation, editing, assets and source-bound round trips. The fixture and baseline
are checked in, so these tests run offline.

`package_layout.rs` keeps the OPC relationship-based main-part and typed plan
write invariant. `interop.rs` reads the checked-in reference-engine DOCX
through typed read/listing and plan APIs. `spike.rs` remains the xmloxide
semantic round-trip and parser-budget adoption gate.

The small package builders in the test keep these checks offline and avoid a
large binary fixture. The historical JSON conformance fixtures remain only as
reference data; their legacy request shapes are not executed by this suite.
