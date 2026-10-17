# Agent notes

## Changing edit operations

Any change to how an edit operation writes OOXML must keep the OOXML
conformance gate green. It checks every committed package three ways:

- ECMA-376 Transitional and OPC schemas (lxml);
- package and cross-part integrity no schema expresses
  (`scripts/ooxml_integrity.py`): content types, relationship targets and
  `r:id` types, comment/numbering/note references, field and bookmark
  pairing, table grids, `w14:paraId`/durable id ranges, duplicate revision
  ids, `mc:Ignorable`;
- the Open XML SDK validator for the Microsoft 365 format
  (`scripts/openxml-validator`), which also covers the w14/w15/w16
  extension namespaces the schema pass strips.

It needs `lxml` and the .NET SDK 10+ (`brew install dotnet`). The gate runs
before every commit that touches `crates/`, `Cargo.*` or the gate scripts,
via prek; install the hook once per clone and do not bypass it with
`--no-verify`:

```sh
prek install                                  # once per clone
prek run ooxml-conformance --all-files        # same check by hand
```

or directly:

```sh
cargo build -p docxdriver-cli
python scripts/ooxml-conformance.py
```

CI runs the same gate on Linux. It fails when an operation introduces an
error kind its source did not have, when a created document has any error,
or when an operation in the schema is never applied. A new operation needs a
scenario in `scripts/ooxml-conformance.py`. To check any `.docx` by hand:
`python scripts/ooxml_integrity.py FILE.docx`.

Content types and relationship types must be the ones Word itself writes.
Take them from Word-authored files (the corpus), never from memory: 0.3.0
registered the comment extension parts as `application/vnd.ms-word.*` and
Word refused every commented document. `EXPECTED_CONTENT_TYPES` in
`scripts/ooxml_integrity.py` is the table; extend it when adding a part.

Word is the only real oracle. On macOS with Word installed, run the Word
smoke test after changing how parts or relationships are registered, and
before a release:

```sh
prek run --hook-stage manual word-smoke
```

It opens one output per source/operation/mode in Word and fails on any that
Word will not open cleanly; outputs of sources Word already rejects (the
`parity` and `ts-reference` fixtures) are skipped. It force-quits Word, so it
refuses to run while Word is open; ask the user before running it.

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
2. If the operation is exposed to Python, list it in
   `crates/docxdriver-monty-repl/sandbox/plan-capabilities.json`, then run
   `python scripts/generate-python-plan.py`. It regenerates `plan.py`,
   `plan.pyi` and `plan.md` in that directory (the single source for the
   `docxdriver-monty-repl` crate and the `docxdriver.plan` module) and their
   copies. Hand-written code lives outside the GENERATED regions.
3. For the Pi REPL, also list it in
   `packages/docxdriver-pi/scripts/plan-capability-manifest.mjs`, then run
   `node packages/docxdriver-pi/scripts/generate-python-dataclasses.mjs`.
   Pi keeps its own templates in `packages/docxdriver-pi/src/python-plan-prelude.ts`.
4. Rebuild the vendored WASM runtime before running npm tests:
   `npm run build` in `packages/docxdriver-pi`.

Drift tests fail if any of these steps is skipped.

Update the skills in `packages/docxdriver-pi/skills/` when agent-facing
behavior changes.

## Tests

```sh
cargo test --workspace
python scripts/ooxml-conformance.py           # needs lxml + .NET SDK 10+; also the prek hook
(cd packages/docxdriver && npm ci && npm test)
(cd packages/docxdriver-pi && npm ci && npm test)
python -m unittest discover -s crates/docxdriver-python/tests   # after installing the wheel; see its README
PYTHONPATH=experimental/python-repl-host python -m unittest discover -s experimental/python-repl-host/tests
python scripts/audit-public-content.py
```

`cargo run --release -p docxdriver-core --example plan_scaling -- PARAS OPS`
times plan review. Review cost should stay roughly linear in operations.
