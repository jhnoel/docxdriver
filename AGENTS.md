# Agent notes

## Changing edit operations

Any change to how an edit operation writes OOXML must keep the ECMA-376 schema
gate green. CI runs it on Linux; run it locally before pushing (requires
`lxml`):

```sh
cargo build -p docxdriver-cli
python scripts/ooxml-conformance.py
```

The gate fails when an operation introduces a schema error kind its source
did not have, when a created document is invalid, or when an operation in the
schema is never applied. A new operation needs a scenario in
`scripts/ooxml-conformance.py`.

For engine changes that create or reorder structure (sections, settings,
revisions, paragraph properties), also run the corpus once:

```sh
sh scripts/fetch-corpus.sh   # first time only; gitignored
cargo build --release -p docxdriver-cli
python scripts/ooxml-conformance.py --cli target/release/docxdriver --corpus corpus/lo
```

When creating an element inside a schema sequence (`w:sectPr`, `w:pPr`,
`w:rPr`, `w:settings`, …), insert it at its sequence position. Never append
by default. `scripts/ooxml-schemas/wml.xsd` is the reference.

## Generated files

After changing `EditOp` or its fields in `crates/docxdriver-core/src/api.rs`:

1. Regenerate the JSON schemas and TypeScript types:
   `sh packages/docxdriver/scripts/generate-schema.sh`
   (also update the type template in
   `crates/docxdriver-core/src/bin/generate_typed_schema.rs`).
2. If the operation is exposed in the Python REPL, list it in
   `packages/docxdriver-pi/scripts/plan-capability-manifest.mjs`, then run
   `node packages/docxdriver-pi/scripts/generate-python-dataclasses.mjs`.
   Hand-written helpers and reference prose live outside the generated
   regions of `packages/docxdriver-pi/src/python-plan-prelude.ts`.
3. Regenerate the Python SDK prelude:
   `python scripts/generate-docxdriver-repl-prelude.py`.
4. Rebuild the vendored WASM runtime before running npm tests:
   `npm run build` in `packages/docxdriver-pi`.

Drift tests fail if any of these steps is skipped.

Update the skills in `packages/docxdriver-pi/skills/` when agent-facing
behavior changes.

## Tests

```sh
cargo test --workspace
python scripts/ooxml-conformance.py
(cd packages/docxdriver && npm ci && npm test)
(cd packages/docxdriver-pi && npm ci && npm test)
python -m unittest discover -s crates/docxdriver-python/tests   # after installing the wheel; see its README
python scripts/audit-public-content.py
```

`cargo run --release -p docxdriver-core --example plan_scaling -- PARAS OPS`
times plan review. Review cost should stay roughly linear in operations.
