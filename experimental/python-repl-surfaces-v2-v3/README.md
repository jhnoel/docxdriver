# Deprecated Python REPL surface experiments (V2 and V3)

**Status: DEPRECATED — research evidence only. Not supported. Not built. Not
published. Not imported by production code.**

This archive preserves Version 2 (raw projection string) and Version 3
(structured Python document model) of the experimental Python REPL surfaces,
plus the comparative trial harness and historical live-agent reports that
measured them against Version 1.

Version 1 — the Python-authored typed-plan surface — is the only REPL surface
on the production path (`packages/docxdriver-pi`). It must not be hardened,
extended, loaded by default, imported by production code, or used to justify
production correctness claims.

## Contents

- `src/` — V2/V3-only TypeScript source:
  - `python-string-extension.ts`, `python-string-prelude.ts`,
    `python-string-deriver.ts` (Version 2: raw projection string surface)
  - `python-model-extension.ts`, `python-model-prelude.ts`,
    `python-model-deriver.ts` (Version 3: structured Python document model)
  - `projection-diff.ts` (Version 2 projection reconciler: token-level LCS
    diff and insert-id allocation)
  - `projection.ts` — the canonical projection parser/serializer that backed
    the V2/V3 `docx_structure`/`docx_find` helpers (moved out of production
    in Task 5; production V1 read/find delegate to core and parse no markup)
- `test/` — V2/V3-only tests and the comparative trial harness:
  - `python-string-extension.test.mjs`, `python-model-extension.test.mjs`
  - `projection-diff.test.mjs`, `projection.test.mjs` (the parser's tests,
    moved with it)
  - `repl-conformance.test.mjs` + `repl-trial.mjs` — the 10-task offline
    conformance suite that ran every task against all three surfaces and
    asserted cross-surface equivalence. The suite is historical evidence of
    the three-surface comparison; it is not runnable here (the archive is not
    built, and production does not build or import archived modules).
- `reports/` — tracked live-agent trial reports: aggregate summaries only
  (`summary.json`, `summary.md`, `summary-comparison.md`, `task10-part-hashes.json`)
  for the three recorded runs. Raw per-trial `events.jsonl`/`run.json`/
  `stdout.log`/`stderr.log` are not tracked: the live harness writes them
  under `packages/docxdriver-pi/test/pi-e2e/results/` (gitignored). Hardening
  Task 7 removed the tracked raw logs (~139 MB) from this archive. Results
  are historical **ergonomics** evidence, not safety evidence.
- `BASELINE-2026-08-12.md` — baseline commit, test results, and untracked
  files recorded at the time of the archive move.

## Exclusions

These modules and tests are excluded from:

- package exports (`package.json` `exports` and the `files` allowlist);
- default extensions (only `src/index.ts` is registered; the V1 entry point is
  `src/python-plan-extension.ts`);
- builds (`tsc -p tsconfig.json` compiles only `src/` under
  `packages/docxdriver-pi`, which no longer contains this archive; the build
  cleans `dist/` first so stale emitted files cannot survive);
- normal test scripts (`npm test` runs `test/*.test.mjs` under
  `packages/docxdriver-pi` only).

`packages/docxdriver-pi/test/archive-isolation.test.mjs` fails any production
build or test run whose emitted code resolves into this archive, and fails if
deprecated emitted files reappear in `packages/docxdriver-pi/dist`.

## Known correctness gaps (historical implementation)

These are the reasons the experiments are deprecated. None of the gaps is
fixed here; the archive preserves the code as it was.

- **Ignored structure**: rendered markup that does not map onto the
  paragraph/token model (e.g. unsupported or nested constructs) can be
  skipped or flattened by the string reconciler instead of rejecting the
  edit, so a proposed projection may silently drop content.
- **Protected-content crossings**: edits spanning or touching protected
  blocks (tables, fields, links, notes, equations, revision spans) were not
  reliably refused, allowing the agent's string edits to cross boundaries
  the typed core operations cannot express.
- **Post-write verification**: the historical implementation verified the
  document *after* the write; a failed verification left the target already
  replaced and could only report a problem retroactively.

Production V1 replaces all three concerns with typed core operations,
pre-write candidate verification, and source/path rechecks before atomic
replacement (see `docs/superpowers/plans/2026-08-12-python-plan-repl-hardening.md`).

## Import graph at archiving (2026-08-12, commit 5b83899)

Recorded before the move so the production boundary is explicit. Production
modules are under `packages/docxdriver-pi/src`; arrows point at what the
deprecated modules imported.

```text
python-string-extension.ts
  -> python-extension-factory.ts        (production, shared factory — removed in Task 1)
  -> python-string-prelude.ts           (archived)
  -> python-string-deriver.ts           (archived)
python-string-deriver.ts
  -> projection.ts                      (production canonical projection reader)
  -> projection-diff.ts                 (archived)
  -> python-host.ts                     (production host types)
python-model-extension.ts
  -> python-extension-factory.ts        (production, shared factory — removed in Task 1)
  -> python-model-prelude.ts            (archived)
  -> python-model-deriver.ts            (archived)
python-model-deriver.ts
  -> projection.ts                      (production canonical projection reader)
  -> projection-diff.ts                 (archived: allocateInsertId)
  -> python-host.ts                     (production host types)
projection-diff.ts
  -> projection.ts                      (production canonical projection reader)
```

`projection.ts` — the canonical projection reader — is **shared** code:
production (the shared host's `_docx_structure`/`_docx_open` helpers) and
deprecated code both import it. It therefore stays in
`packages/docxdriver-pi/src`; a module is not archived solely because deprecated
code imports it. Its production fate is decided by the hardening plan (Task 5
removes the last production use).

`python-plan-deriver.ts` is the sole retained derivation path (V1 plan
normalization); it never imported the projection parser or reconciler.

## Reusing this archive

Do not. If a future effort needs V2/V3 behavior, it must be re-derived from
the typed core `Plan` API, not resurrected from these files. The files are
kept for provenance, historical measurement, and the record of why the
three-surface design was abandoned.
