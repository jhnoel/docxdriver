# docxdriver benchmark harness

## HTML projection head-to-head

`html-head-to-head.mjs` compares the pinned prior projection with the HTML/MathML
overhaul on byte-identical fixtures. It records alternating warm read latency,
WASM size, projection and complete-response size, and optional reference tokenizer
counts. Live trials measure independently verified success, provider-reported
tokens, turns, tool calls, rejected operations and wall time.

Build the old `packages/docxdriver` WASM from commit
`0e5a0f9c559b3623bf5792e187c8c846fe627dfc` in an isolated checkout, and build the
current package. Generate the browser E2E source fixture with
`npm --prefix packages/docxdriver run test:e2e`. Then run:

```sh
node bench/html-head-to-head.mjs --baseline /path/to/prior-checkout \
  --live --model opencode-go/deepseek-v4-flash --thinking medium --reps 3
```

Omit `--live` for offline engine measurements. Live runs use the selected model's
configured credentials and incur its normal usage. `--tasks` accepts a comma list:
`text,occurrence,mixed,repair,equation,complex,inspect`. `--out` selects the artifact
directory. `--tokenizer-python` optionally points to Python with `tiktoken`
installed; o200k_base counts are reference counts, not DeepSeek token counts.

The identical neutral tool wrapper returns one projection copy per read and uses
an immutable reviewed plan for commits. Equation descriptions specify LaTeX in
the old arm and MathML in the new arm. This measures the projection/authoring
change under controlled tooling, not the shipped Python REPL host as a whole.
The old WASM is checked against frozen native outputs before any live calls.
Results, raw events, final DOCX files and a Markdown report go under
`target/html-head-to-head/`. The DOCX verifier checks the result and unrelated ZIP
parts independently of either markup format. Three repetitions per task provide
directional evidence; use more models, documents and repetitions for firm claims.

After the run, measure preview/commit engine latency and summarize paired results:

```sh
node bench/html-head-to-head-edits.mjs /path/to/results /path/to/prior-checkout
node bench/html-head-to-head-audit.mjs /path/to/results
node bench/html-head-to-head-analyze.mjs /path/to/results
python3 bench/html-head-to-head-verify-test.py
```

The analysis writes `comparison.md` and `analysis.json`, including conditional
repeat-variability intervals from paired resampling within each fixed task.
The audit rechecks all retained DOCX files and fixture fingerprints with the final
verifier and preserves any changed initial judgements. Run it once after live
trials complete, before analyzing the results.

Reproducible benchmark + profile harness for the docxdriver Rust-to-WASM DOCX
engine (the `docxdriver` npm package that `docxdriver-pi` uses). Everything runs
standalone or through the orchestrator:

```sh
node bench/runner.mjs              # full run: build → fixtures → all phases → results
node bench/runner.mjs --skip-profile
node bench/runner.mjs --only warm   # any phase: validate|cold|warm|batch|memory|native|profile
node bench/runner.mjs --tests       # also run cargo test --workspace + npm test
```

Results land in `bench/results/results.json` (machine-readable) and
`bench/results/summary.md` (human-readable). Generated fixtures, results, and
profiles are gitignored.

## Layout

| path | role |
|---|---|
| `bench/gen-fixtures.mjs` | deterministic DOCX fixtures (tiny ZIP writer + PRNG, no deps) |
| `bench/lib/zip.mjs` | deterministic ZIP writer (`node:zlib` deflate, fixed timestamps) |
| `bench/lib/work.mjs` | the shared command list (`work.json`) used by WASM and native |
| `bench/lib/stats.mjs` | median/p95/min/max/mean helpers |
| `bench/wasm-bench.mjs` | WASM phases: `validate`, `cold`, `warm`, `batch`, `memory` |
| `bench/profile-run.mjs` | `--cpu-prof` workload target (large-doc render/edit/read loop) |
| `bench/summarize-profile.mjs` | cpuprofile → top frames + module totals; writes `profile.json`, `wasm-exports.json` |
| `crates/docxdriver-core/examples/bench_native.rs` | native Rust-core control (`execute_json_with_options`, no wasm/glue) |
| `bench/README.md` | this file |

## Methodology

1. **Fixtures** are generated *outside* timed regions by an engine-independent
   deterministic ZIP writer + mulberry32 PRNG (fixed seed): small (40
   paragraphs, ~2.5 KB), medium (400, ~15 KB), large (3000, ~145 KB), plus
   `medium-revised.docx` (medium + 60 tracked `editHtml` operations = 120
   revisions). Text is varied dictionary prose (compresses to ~20–35% of raw —
   not pathologically tiny), with unique `mkrNNNNNN`/`auxNNNNNN` marker words
   every 10–20th paragraph (exactly-once find/edit/comment targets) and
   Heading1 paragraphs every 25th. `gen-fixtures.mjs --check` byte-compares
   regeneration against disk to prove determinism. Fixture validity is
   cross-checked *by the engine* (`wasm-bench.mjs validate`: read/wordCount/
   outline/renderHtml assertions).

2. **Cold init** (`cold`): 8 fresh Node processes; each child reports file-read
   time, compile+instantiate time (`init(bytes)`), a document-free first call,
   a first small-DOCX read, and parent-observed spawn-to-exit time separately.
   A `node -e ''` spawn baseline is a separate process-startup reference.

3. **Warm `execute` latency** (`warm`): one process, `--expose-gc`; warmups
   (4) then timed samples per command (counts by
   cost, see `lib/work.mjs`). Mutating commands are *stateless-ABI*: every
   iteration starts from pristine fixture bytes, so iterations measure
   equivalent inputs — never a progressively mutated document. Read-back
   verification (status, output bytes, `read`/`listComments`/`listRevisions`
   projections) runs once per command *outside* the timed loop. Options always
   carry an explicit `now` so revision/comment dates are identical across
   samples and across the native control.

4. **Sequential vs native batch** (`batch`): two equivalent command lists
   (8 mixed ops on medium; 96 read-only ops on large) run as (a) a sequential
   chain of per-op `execute` calls carrying bytes between mutations and (b) one
   array-form batch call (one parse, one serialize). Both paths are warmed,
   their order alternates across 11 measured rounds, and equivalence is checked
   via per-op statuses plus final current/all reads, markup HTML, comments, and
   revisions. Read-only-only lists return no bytes — final state is the pristine
   fixture by construction.

5. **Memory** (`memory`, requires `--expose-gc`): `process.memoryUsage()`
   (rss, heapUsed/Total, external, arrayBuffers) at baseline, after init,
   after a warm workload, and after a 30-iteration high-water loop that
   repeatedly renders/edits/reads the large document while retaining outputs;
   peaks tracked per iteration, then release + gc.

6. **Native Rust-core control** (`bench_native.rs`): reads the *same* fixture
   files and `work.json` (same command JSON) and calls
   `docxdriver_core::execute_json_with_options` directly — no wasm-bindgen, no JS
   glue, no TS shim. Built twice: the workspace release profile (opt-level **z**,
   matching the Rust profile used for WASM) and `CARGO_PROFILE_RELEASE_OPT_LEVEL=3`.
   The wasm/opt-z ratio is a native Rust reference with a matching Rust profile,
   but it still includes target/runtime/allocator differences and is not a pure
   glue-cost measurement. The wasm/opt-3 ratio additionally includes the
   optimization-level difference. Neither is a claim about TypeScript
   performance; there is no equivalent TypeScript implementation in this repo.

7. **CPU profile** (`profile`): `node --cpu-prof` over a representative large
   workload (renderHtml markup + tracked editHtml + read, looped), summarized
   by `summarize-profile.mjs` into self/inclusive time
   per frame with wasm/JS-glue/V8 totals and the wasm export table decoded
   (wasm-opt strips the name section, so internal frames appear as
   `wasm-function[N]`; `bench/results/wasm-exports.json` maps exported
   indexes, and the exported entry points are listed by inclusive time).

8. **Verification**: every phase fails hard (non-zero exit) on any command
   error; mutations are checked for output bytes (`PK` zip magic) and
   read-back; fixtures are engine-validated; existing tests run via
   `cargo test --workspace` and `cd packages/docxdriver && npm test`
   (`runner.mjs --tests`).

## Environment metadata

Recorded automatically in `results.json`/`summary.md` (`meta`): git commit,
OS/release, arch, CPU model/count, Node/npm/cargo/rustc/wasm-pack versions,
wasm module size + sha256, and the build profile. To reproduce:

- Build: `cd packages/docxdriver && npm run build` (wasm-pack `--target web
  --release` + wasm-opt `-O --enable-bulk-memory
  --enable-nontrapping-float-to-int`, then `tsc`).
- Workspace `[profile.release]`: `opt-level = "z"`, `lto = true`,
  `codegen-units = 1`, `strip = true`, `panic = "abort"` (the wasm core is
  built with these exact flags).
- Native control: `cargo build --release -p docxdriver-core --example bench_native`
  (default profile) and with `CARGO_PROFILE_RELEASE_OPT_LEVEL=3`.

## Known limitations / measurement caveats

- **opt-level z vs 3**: the wasm core is size-optimized; the opt-3 native
  build is a speed reference. Its gap combines optimization level, target,
  runtime, and allocator differences; an opt-3 WASM build is needed to
  attribute those effects.
- **Cold init excludes static JS module import/evaluation**, because imports run
  before the child timer starts. It separates warm-cache file read,
  compile+instantiate, and a document-free first `help` call. The `node -e ''`
  number is a separate process-spawn reference, not a subtractable component.
- **`now` injection**: options carry a fixed `now`, so hosts that let the
  shim default to the wall clock add ~µs of `new Date()` — not measured here.
- **Zip determinism**: engine output bytes are *not* compared across
  sequential/batch because valid ZIP layout may differ. Equality checks cover
  current/all reads, markup HTML, comments, and revisions, but are not a formal
  comparison of every unexposed package part.
- **Profile frame attribution**: V8 cpuprof does not link wasm-internal frames
  to their callers (and wasm-opt strips names), so hot internals are reported
  as `wasm-function[N]` with the export table for resolution; entry-point
  inclusive times give the command-path view.
- **High-water memory** deliberately retains outputs to observe one Node host's
  stress peak; `external` includes more than WASM memory and transient in-call
  peaks may be missed. Memory after release + GC is reported separately.
- **Scope**: this harness characterizes current WASM behavior and native Rust
  references. It does not benchmark a TypeScript port or compare robustness.
- **Machine dependence**: absolute numbers are for the recorded host only
  (Apple Silicon here); compare ratios, not absolutes, across machines.

## Full-context HTML compaction through Python

`html-head-to-head.mjs --python --compact-baseline` compares saved pre-compaction
WASM to current WASM through the shipped Python authoring tool. Both arms use the
same current host and prompts; the controlled before arm restores generic output
caps. It is a controlled comparison, not an exact historical Python checkout.
Freeze the engine first, then prepare the common Python host:

```sh
node bench/html-compaction-baseline.mjs --engine PATH_TO_SAVED_DOCXDRIVER_PACKAGE
node bench/html-head-to-head.mjs --python --compact-baseline --skill-docs --live \
  --model opencode-go/deepseek-v4-flash --thinking medium --reps 3 \
  --tokenizer-python PATH_TO_PYTHON_WITH_TIKTOKEN \
  --out target/html-head-to-head/python-compact-3x
```

The same seven tasks, fresh paired fixture copies, alternating order and independent
DOCX verifier are used. Commit success is counted from the protected Python commit
protocol; failed selectors and Python errors count as rejections. Reference token
counts measure complete engine reads; live provider usage measures the full Python
conversation, including its description and repeated context. The engine timing
suite includes 500 paragraphs. Full-output delivery tests separately cover returned,
printed and previously saved documents exceeding generic display caps.

`--skill-docs` supplies the complete shipped `docx-pi-repl/SKILL.md` and markup
reference in the same prompt for both arms. The exact bundle and its SHA-256
are retained with the results. API documentation and examples are also present
in the Python tool description. Runs without this flag omit the standalone skill.
