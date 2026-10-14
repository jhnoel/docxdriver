# scripts

| Script | What it does |
| --- | --- |
| `fetch-corpus.sh` | Pulls the coverage corpus — LibreOffice's DOCX regression fixtures (`sw/qa/extras/ooxmlexport` + `ooxmlimport`, ~1,500 small targeted documents) — into the gitignored `corpus/lo/` via a blob-less sparse clone. `tests/coverage.rs` picks them up from there. |
| `build-wasm.sh` | Builds the WASM runtime from Rust, remaps build paths, and generates ignored runtime files for both npm packages. |
| `audit-public-content.py` | Checks publishable files, including DOCX members, for personal paths, source-system metadata, and accidentally tracked compiled outputs. Run alongside a credential scanner before publication. |
| `ooxml-conformance.py` | Applies every typed edit operation through a real plan to a synthetic document, the repository fixtures, and optionally `--corpus corpus/lo`, then validates each result against the ECMA-376 Transitional and OPC schemas in `ooxml-schemas/` (requires `lxml`). Fails when an operation introduces a schema error kind its source did not have, when a created document is invalid, or when any schema operation is never applied. Runs in CI. |
