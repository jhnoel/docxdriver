# Typed-plan Python REPL — regression matrix (hardening Task 7)

Evidence artifact for the final integration of the V1 typed-plan Python REPL
surface and the cleanup of the deprecated V2/V3 archive.

- Date: 2026-08-12 (Task 7 implementation)
- Branch: `feat/python-repl-surfaces` (worktree `python-repl-surfaces`)
- Pre-Task-7 HEAD: `0e0c7c6` (Task 6) with follow-up `7a50903`; Task 7 adds one
  commit on top: `test(pi): validate isolated typed-plan Python REPL`.

## (a) Regression matrix — requirement area → concrete test

| Area | Coverage (test file: case) |
| --- | --- |
| Cross-path authorization | `python-plan-extension.test.mjs`: "receipt owns the canonical target path"; `python-host.test.mjs`: "different cwd revalidates against the preview-bound root" |
| Missing / partial / same-execution review | `python-host.test.mjs` + `python-plan-extension.test.mjs`: commit-without-review, partial-paginated-review, review-and-commit-one-execution, review-same-execution; `repl-live.test.mjs`: "a commit without any review is not explainable", "partial review delivery leaves reviewComplete false" |
| Receipt replay / expiry / reset / generation | `python-host.test.mjs`: replay, reset/shutdown/generation, eviction tombstone |
| Plan mutation after preview | `python-host.test.mjs` + `python-plan-extension.test.mjs`: mutation-after-preview |
| Source race / path race | `python-host.test.mjs`: source-race, symlink-alias swap; live trials task 8 (harness-injected tamper on `receipt: r1:`), task 9 (plan-state mutation) |
| Candidate verification before write | `python-host.test.mjs`: candidate-validation-writes-nothing, validateCandidate; atomic-write/no-temp tests |
| Output truncation / control budget | `python-plan-extension.test.mjs`: 60 KiB stdout test; `python-repl.test.mjs`: renderExecution control-budget test; `python-host.test.mjs`: notice-cap test |
| Large review pagination | `python-host.test.mjs`: buildReviewPages + oversized-report rejection |
| All supported typed operations | `python-host.test.mjs`: round-trip; `python-plan-extension.test.mjs`: generated-dataclass round-trip; live trials tasks 1–7, 10 |
| Precise repeated-text occurrence | live trial task 3 + offline `checkFinalDocument` task-3 test in `repl-live.test.mjs` (archived conformance suite is historical only) |
| Package-part preservation | live trial task 10 `zipPartHashes` + offline task-10 test in `repl-live.test.mjs` |
| Cancellation / crash recovery | `python-plan-extension.test.mjs`: entry-abort, read/preview/pre-write/post-write cancellation, recovery-failure; `python-host.test.mjs`: abort/deadline tests |
| Archive isolation + symbol absence | `archive-isolation.test.mjs` (4 tests incl. the new symbol guard) |
| Strict harness success (fail closed) | `repl-live.test.mjs`: "trialSuccess fails closed: turn-cap or timeout termination is never success"; live harness commit-task gates (exactly one consumed receipt, `reviewComplete`, preview → review → commit ordering, exact final edit) and `main()` exit 1 on any failed cell |

## (b) Refined architecture grep evidence

The hardening plan's raw grep matches 3 generated `Paragraph(at:` doc-string
lines in `src/python-plan-prelude.ts` (schema-derived; regeneration recreates
them). The refined commands exclude only that documented generated form:

```sh
$ rg -n "lcsTokens|diffProjections|parseProjection|SurfaceDeriver|applyTextChange|applyFmtChange|Selection\(|Document\(" packages/docxdriver-pi/src
(no output — exit 1)
$ rg -n "Paragraph\(" packages/docxdriver-pi/src | rg -v "Paragraph\(at:"
(no output — exit 1)
```

The same check is a permanent test: `archive-isolation.test.mjs` "production
src contains no deprecated projection/structured-model symbols" (allows
`(Format|Insert|Replace)Paragraph(at:` only). Production-harness grep:

```sh
$ rg -n "preview key|draft string|document model|python-string|python-model" packages/docxdriver-pi/test/pi-e2e/repl-tasks.json
(no output)
```

(Remaining `preview key` / `p1:sha256:` mentions live only in the separate
five-tool TOML baseline harness — `toml-tasks.json`, `toml-bench-live.mjs`,
`run.mjs` — which legitimately uses the preview-key contract, and in the V1
host's own "never the deterministic core preview key" comments.)

## (c) Package / build isolation evidence

- `packages/docxdriver-pi/package.json`: `files` = `["dist","src","skills","README.md"]`;
  `exports` exposes only `.`; `pi.extensions` lists only `./src/index.ts` —
  no `experimental/` path anywhere (checked by `archive-isolation.test.mjs`).
- `npm pack --dry-run`: tarball contains `dist/*.js` (incl.
  `dist/python-plan-extension.js`, `dist/python-host.js`), `src/*.ts`, skills,
  README — **no** `experimental/` entry, no `python-string*`/`python-model*`/
  `projection*` modules.
- Build: `tsc -p tsconfig.json` cleans `dist/` first; emitted module graph is
  walked and must never resolve into `experimental/python-repl-surfaces-v2-v3`
  (`archive-isolation.test.mjs`: "no production module imports from the
  experimental archive").
- Archive: `git ls-files experimental/python-repl-surfaces-v2-v3/reports` =
  7 aggregate summary files (~108 KB) after Task 7 removed the 120 tracked raw
  per-trial files (~139 MB, 167,855 lines) from the 2026-08-11T16-07-59 run;
  raw per-trial logs now live only in the gitignored
  `packages/docxdriver-pi/test/pi-e2e/results/`.

## (d) Final gate results (run 2026-08-12)

| Gate | Result | Evidence |
| --- | --- | --- |
| `cargo fmt --all --check` | PASS | exit 0, no diffs |
| `cargo clippy --workspace --all-targets -- -D warnings` | FAIL (pre-existing, unchanged) | 28 errors, all pedantic style lints in `docxdriver-core` (`needless_borrow`, `too_many_arguments`, `while_let_loop`, …); reproduced identically at Task-7 HEAD `7a50903` and at merge-base `ee990c0` — no Rust file is touched by this change (JS/MD/JSON/YAML only) |
| `cargo test --workspace` | PASS | 35 passed, 0 failed (all suites) |
| `npm test` in `packages/docxdriver` | PASS | 1 passed, 0 failed (schema drift) |
| `npm test` in `packages/docxdriver-pi` | PASS | 148 passed, 0 failed: archive-isolation 4, e2e 13, python-host 61, python-plan-extension 36, python-repl 15, repl-live 13, toml-bench-live 6 |
| `node scripts/generate-python-dataclasses.mjs` + tracked-diff check | PASS | exit 0; `git diff --exit-code -- src/python-plan-prelude.ts` clean (idempotent, no generated drift) |
| `npm pack --dry-run` (docxdriver-pi) | PASS | no `experimental/`, no archived module names; V1 `python-plan-extension` dist artifacts present |
| Refined architecture greps | PASS | 0 matches (see (b)) |
| `git diff --check` | PASS | clean (working tree and staged) |
| Live harness fast-fail (opt-in guards) | PASS | no env → "live REPL trials require PI_E2E_LIVE=1" exit 1; `PI_E2E_SURFACES=string` → "unknown surface string (V1 only: plan)" exit 1; `PI_E2E_SURFACES=plan` passes surface validation |

Baseline note: the archived `BASELINE-2026-08-12.md` records a 152-test count
for `docxdriver-pi`; the current green count is 148 (the archived conformance
suite moved out of the package with the V2/V3 archive).
