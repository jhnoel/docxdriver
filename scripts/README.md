# scripts

| Script | What it does |
| --- | --- |
| `fetch-corpus.sh` | Pulls the coverage corpus — LibreOffice's DOCX regression fixtures (`sw/qa/extras/ooxmlexport` + `ooxmlimport`, ~1,500 small targeted documents) — into the gitignored `corpus/lo/` via a blob-less sparse clone. `tests/coverage.rs` picks them up from there. |
| `build-wasm.sh` | Builds the WASM runtime from Rust, remaps build paths, and generates ignored runtime files for both npm packages. |
| `audit-public-content.py` | Checks publishable files, including DOCX members, for personal paths, source-system metadata, and accidentally tracked compiled outputs. Run alongside a credential scanner before publication. |
| `ooxml-conformance.py` | Applies every typed edit operation through a real plan to a synthetic document, the repository fixtures, and optionally `--corpus corpus/lo`, then checks each result three ways: the ECMA-376 Transitional and OPC schemas in `ooxml-schemas/`, the integrity rules in `ooxml_integrity.py`, and the Open XML SDK validator in `openxml-validator/` (requires `lxml` and the .NET SDK 10+). Fails when an operation introduces an error kind its source did not have, when a created document has any error, or when any schema operation is never applied. `--keep-outputs DIR` saves every committed package. Runs in CI and as the prek pre-commit hook. |
| `ooxml_integrity.py` | Package and cross-part checks no schema expresses: content types Word accepts, relationship targets and `r:id` types, comment/numbering/note references, field and bookmark pairing, table grids, id ranges and uniqueness, `mc:Ignorable`. Used by the gate; `python scripts/ooxml_integrity.py FILE.docx` checks any package. |
| `openxml-validator/` | .NET console app wrapping the Open XML SDK `OpenXmlValidator` (Microsoft 365 format); reads .docx paths on stdin and writes one JSON line each. Built by the gate. |
| `word-smoke.py` | macOS + Microsoft Word only: opens `--keep-outputs` packages in Word and fails on any Word will not open cleanly, skipping outputs of sources Word already rejects. Force-quits Word. `prek run --hook-stage manual word-smoke`. |

For a full corpus run, pass `--jobs 4` to check independent documents in
parallel. Each worker has its own temporary outputs and schema validator;
all operation coverage and failures are aggregated. The default remains one
worker. Parallel runs report progress every 100 documents.
