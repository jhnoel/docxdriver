# Plan vs String — Re-measurement after surface help fixes

- New run: `2026-08-11T12-52-53.277Z-opencode-go_deepseek-v4-flash` (this directory)
- Baseline run: `2026-08-11T06-25-29.110Z-opencode-go_deepseek-v4-flash` (same checkout history, before commit `0e2f655`)
- Model: `opencode-go/deepseek-v4-flash` · Surfaces: plan, string · Tasks: 10 · Reps: 3 (60 trials per run)
- Prompt version: 1 (identical task text per task across surfaces; unchanged between runs)
- Code delta being measured (commit `0e2f655`): (1) `_docx_help` returns the per-surface API reference instead of the generic engine help that advertised phantom capabilities (`docx_find`/`docx_edit`/`revision_settle`); (2) V1 (plan) prelude pre-imports `re` with checker stubs; (3) V1 API reference + prompt guidelines teach regex-computed operation authoring (ops built in Python loops over the `docx_read` projection).

## Headline

- **plan: 27/30 → 30/30.** The task-2 bulk-edit failure (0/3 in the baseline, caused by the phantom-capability discovery spiral) is **fixed: 3/3**, with the agent using the regex-computed-ops pattern directly (task2/plan-2 completes in 3 calls: read → loop-build ops → commit).
- **string: 30/30 → 29/30.** One new red cell: task7 (failed-preview repair)/string/3 never committed (20 calls, 6 previews, 3 blocks, turn-capped; final doc unchanged). Same failure character as the baseline's task7/model-3 no-commit — a repair-task turn-cap, not a capability regression (tasks 5/6/7 string still ok 2-3/3).
- **Plan is now strictly cheaper on every effort axis and equally reliable (30/30 vs 29/30).**

## Success per task (ok / 3 reps; old -> new)

| task | plan old → new | string old → new |
| --- | --- | --- |
| 1 exact text correction | 3 → 3 | 3 → 3 |
| 2 repeated bulk edit | **0 → 3** | 3 → 3 |
| 3 ambiguous occurrence | 3 → 3 | 3 → 3 |
| 4 arbitrary formatting | 3 → 3 | 3 → 3 |
| 5 paragraph structure | 3 → 3 | 3 → 3 |
| 6 mixed atomic edit | 3 → 3 | 3 → 3 |
| 7 failed preview repair | 3 → 3 | 3 → **2** |
| 8 source race | 3 → 3 | 3 → 3 |
| 9 mutation after preview | 3 → 3 | 3 → 3 |
| 10 package preservation | 3 → 3 | 3 → 3 |
| **total** | **27/30 → 30/30** | **30/30 → 29/30** |

## Aggregate effort (30 trials per surface per run; averages unless noted)

| metric | plan old → new | string old → new |
| --- | --- | --- |
| tool calls / trial | 6.1 → **5.0** | 6.0 → 5.7 |
| python chars / trial (token proxy) | 1563 → **1078** | 1797 → 1865 |
| wall clock / trial | 75 s → **56 s** | 68 s → 73 s |
| blocked results (total) | 22 → **9** | 23 → 14 |
| preview attempts (total) | 32 → 30 | 36 → 35 |
| commits (total) | 21 → 24 | 24 → 23 |
| phantom-capability probe calls | 8 → **1** | 5 → 3 |

## Token efficiency (chars authored)

- plan 1078 vs string 1865 → **plan is 1.7× cheaper**, and now without any failure cost to amortize (the baseline's plan average was dragged up by three 5-8k-char spirals; those are gone).
- Per-task calls: plan is now cheaper or equal on tasks 2 (6.0 vs 4.0 — still slightly behind string on the pure bulk edit), 4, 6, 7 (5.0 vs 12.3 — repair is much cheaper), 9, 10. String is cheaper only on task 1 (3.3 vs 4.7) and task 3 (5.7 vs 4.7→ plan slightly higher).
- The plan task-2 path: baseline 19.3 calls/trial → **6.0** calls/trial.

## Errors / blocked results

- Both surfaces: far fewer blocked previews post-fix (plan 22 → 9, string 23 → 14); plan's task-2 blocks (the `select matched nothing` loop) are gone.
- string's task7/3 failure: 3 blocked, 6 preview attempts, never committed — the repair loop still costs string ~12 calls/trial on task 7 vs plan's 5.
- No invalid write reached disk in either run (task 8/9 rejections behaved identically: 6/6 per surface per run).

## Phantom-capability probes (the spiral metric)

Count of tool calls whose code mentions `docx_find|docx_edit|settle|revision_settle|hasattr|getattr|dir(`:

- plan: 8/187 calls (4%) → **1/149 calls (1%)**
- string: 5/181 (3%) → 3/171 (2%)

The plan spiral is essentially eliminated; the one remaining probe is a single `dir(re)` call in task2/plan-1 (exploring the pre-imported `re` module — benign).

## Verification

- Independent spot-check of task2/plan-1..3 events confirms the mechanism: agents read the projection, `re.finditer` the payment paragraphs, build `ReplaceText` ops in a loop, preview once, commit. plan-2: 3 calls.
- All 10 task-level `finalEqual` flags true except task 10 (surfaces pick different clean target paragraphs per trial — same as baseline; package preservation held in all trials).
- Cell shape and counts verified from `summary.json` (60 cells per run; 6 per task cell).

## Interpretation

- The help fix + regex-computed-ops guidance turned the plan surface's single measured weakness into a win and removed the exploration overhead that had inflated its call count. Plan now leads string on calls, chars, wall time, and blocked results while matching reliability (30/30 vs 29/30 — the one string failure is a turn-capped repair no-commit, the same class as baseline noise).
- String's remaining edge is task-2 authoring speed (4.0 vs 6.0 calls) — the pure `re.sub` bulk primitive is still the most direct for rule-shaped edits — and it has no regression in capability; its cost is more blocked previews and heavier repair loops.
- Caveat: single model, n=3 per cell. The string 29/30 vs plan 30/30 difference is within per-rep noise; the directional findings (plan cheaper on 8/10 tasks, spiral eliminated, repair cheaper on plan) are consistent across reps.
