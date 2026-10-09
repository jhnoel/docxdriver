# scripts

| Script | What it does |
| --- | --- |
| `fetch-corpus.sh` | Pulls the coverage corpus — LibreOffice's DOCX regression fixtures (`sw/qa/extras/ooxmlexport` + `ooxmlimport`, ~1,500 small targeted documents) — into the gitignored `corpus/lo/` via a blob-less sparse clone. `tests/coverage.rs` picks them up from there. |
| `build-wasm.sh` | Builds the WASM runtime from Rust, remaps build paths, and generates ignored runtime files for both npm packages. |
| `audit-public-content.py` | Checks publishable files, including DOCX members, for personal paths, source-system metadata, and accidentally tracked compiled outputs. Run alongside a credential scanner before publication. |
