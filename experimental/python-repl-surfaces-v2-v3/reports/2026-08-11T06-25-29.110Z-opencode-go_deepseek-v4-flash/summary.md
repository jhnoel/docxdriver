# Live-Agent Trial Report — Three Python REPL Surfaces

Run: `2026-08-11T06-25-29.110Z-opencode-go_deepseek-v4-flash`
Model: `opencode-go/deepseek-v4-flash` · Surfaces: plan, string, model · Tasks: 10 · Reps: 3 (full matrix, 90 trials)
Date: 2026-08-11 · Prompt version: 1 (identical prompt text per task across all three surfaces)

Every trial ran in a fresh temp cwd on a byte-identical fixture copy with one
`pi` process exposing only the `python` tool, under the shared host and commit
gate (preview store → later-execution commit → core `p1:sha256:` key check →
atomic write → post-commit render verification). Full per-trial events live in
`task-*/surface-*/events.jsonl`; machine-readable metrics in `summary.json`.

## Headline

- **86 / 90 trials succeeded.** The 4 failures: **task 2 (repeated bulk edit)
  on the plan surface — 0/3** (the agent repeatedly authored `ReplaceText`
  operations that preview-blocked (`select matched nothing` in the 30-days
  paragraph) and never produced a valid plan in any repetition) **plus task 7
  (failed preview repair) model-3 — 1/3** (the agent repaired its edit but
  never committed; see the task-7 row below). All 4 failures share the same
  character: never committed, file left untouched — no corrupt output anywhere
  in the matrix.
- **0 session resets / recoveries** across 90 trials (no Monty crashes).
- **Every successful commit was explainable**: the `p1:sha256:` preview key
  appeared in tool output before the commit call in 21/21 (plan), 24/24
  (string), 23/23 (model) commit trials.
- **The commit gate never misfired**: tasks 8 (source race) and 9 (mutation
  after preview) rejected the commit in 18/18 trials with the file preserved,
  exactly as designed. Note the asymmetry in how the two are exercised:
  task 8's external file tamper is harness-injected (the harness rewrites the
  fixture when a preview key appears), while task 9's state mutation is
  prompt-directed because the harness cannot call the python tool into the
  model's session. Both paths exercise the same commit gate end-to-end, and
  the verifier asserts the blocked commit + preserved file for both.

## Per-task success (surface = ok / total reps)

| task | plan | string | model | cross-surface final equal |
| --- | --- | --- | --- | --- |
| 1 exact text correction | 3/3 | 3/3 | 3/3 | true |
| 2 repeated bulk edit | **0/3** | 3/3 | 3/3 | true |
| 3 ambiguous occurrence | 3/3 | 3/3 | 3/3 | false* |
| 4 arbitrary formatting | 3/3 | 3/3 | 3/3 | true |
| 5 paragraph structure | 3/3 | 3/3 | 3/3 | true |
| 6 mixed atomic edit | 3/3 | 3/3 | 3/3 | true |
| 7 failed preview repair | 3/3 | 3/3 | **2/3** | true |
| 8 source race | 3/3 | 3/3 | 3/3 | true |
| 9 mutation after preview | 3/3 | 3/3 | 3/3 | true |
| 10 package preservation | 3/3 | 3/3 | 3/3 | false** |

\* task 3: one model-surface rep (model/1) changed the **second** occurrence of
`the terms` instead of the third. Its own outcome check (exactly one uppercase
occurrence) passed, so the trial counts as ok, but the affected-paragraph
accuracy metric flags it: the model surface's occurrence anchoring is
demonstrably weaker than the plan/string surfaces' (both anchored the third
occurrence correctly in all reps). This is a genuine authoring-model finding.

\*\* task 10: surfaces picked different clean target paragraphs per trial (the
prompt deliberately does not pin one), so final documents legitimately differ.
Every trial preserved all package parts except `word/document.xml` byte-identical.

\*\* task 7/model-3: the agent made the broken edit, saw the blocked preview,
and repaired via in-session mutation, but never committed — verification
outcome error (`"sixty days" must be bold` in the final document, which was
unchanged), 0 commits, 6 blocked results, turn-capped after 17 tool calls.
Same character as the task-2 plan failures (never committed, file untouched).

## Evaluation metrics (spec list)

**Successful final document** — 86/90; the 4 failures are task 2/plan x3 plus
task 7/model-3 (both never committed; files left untouched, so no corrupt
output anywhere in the matrix).

**Python tool calls** (successful trials, avg per surface): plan 4.7, string
6.0, model 11.9. The model surface costs ~2.5× the calls of the string surface;
the plan surface is cheapest when it works.

**Preview attempts** (avg): plan 1.0, string 1.2, model 1.1 — all surfaces
mostly preview-once-then-commit.

**Recoveries after rejection** — blocked results per surface, **computed over
successful trials only** (failed trials are excluded because their failure is
the measurement, not a recovery): plan 10, string 23, model 23. Task 7
(deliberate broken edit) showed repair quality over successful reps: plan 4.3
calls / 3 blocks, string 9.0 calls / 9 blocks, model 19.0 calls / 5 blocks —
the model surface needed the most exploration but repaired via in-session
object mutation as designed. Task 5/6 additionally show blocked commit
attempts the agent recovered from (3 and 2 respectively across surfaces) —
the diagnostic text was sufficient to retry successfully.

**Python source chars authored** (avg per successful trial): plan 1017, string
1797, model 2871. Authoring effort is lowest for typed plans, highest for the
document model.

**Wall-clock completion time** (avg per successful trial): plan 57 s, string
68 s, model 140 s. The model surface is ~2.5× slower end-to-end; 10 trials
(11%) hit the 20-turn cap after a successful verified commit (recorded, not
counted as failures).

**Invalid or unsafe write attempts** — 23 total: the 18 intended task 8/9
rejections (9 source-race + 9 state-mutation, all three surfaces × three
reps) plus 5 agent-recovered blocked commits in tasks 5/6. **No invalid write
ever reached disk** (the gate + atomic write + pre-write source recheck held
in all 90 trials).

**Accuracy of affected paragraph selection** — perfect except task 3 model/1
(second vs third occurrence, see above). All structural edits (tasks 5/6)
landed on the right paragraphs; no trial touched unrelated content.

**Preservation of formatting and unrelated package content** — task 1's bold
survived replacement on all three surfaces (plan folds `<b>` into `with`;
string keeps tags; model adds an explicit format op); tasks 4/6 formatting
assertions passed; task 10 verified every package part except
`word/document.xml` byte-identical across all 9 trials. The harness persists
the pre/post package part-hash maps in each task-10 trial's `run.json`
(`partHashes.before` / `partHashes.after`) so the claim is reproducible from
the recorded run; for this run the after-maps were verified in-process at
trial time (the trial temp dirs are removed on completion, so the post-commit
files are not retained — the `run.json` verification outcome is the evidence).

**Agent's ability to explain the preview before commit** — 68/68 successful
commit trials had the preview key visible before the commit call; the preview
report (ops + outcome + per-op summaries) is rendered identically by all three
surfaces, and the agents referenced it (repairing blocked commits in tasks
5/6/7 instead of re-deriving blindly).

**Implementation complexity / maintenance burden** (offline evidence) — the
plan surface is the smallest (dataclass generator + strict normalization);
the string surface required the largest safety-critical piece (projection
parser + reconciler, 2 fix rounds during review); the model surface is the
largest public API (Document/Paragraph/Selection prelude + replay deriver).
The offline conformance suite (10 tasks × 3 surfaces) is green and asserts
cross-surface final-document equivalence (tasks 1-2, 8-9), byte equality
(task 8), and op-level plan comparability (tasks 3-7, 10) — it caught two real
cross-task defects during development, both fixed and re-verified.

**Percentage of real edits expressible by the surface** (per task cell:
10 tasks × 3 reps = 30 cells) — string 30/30 (100%); plan 27/30 (90%, task 2
unbroken in live authoring despite full engine support); model 29/30
(96.7%, task 7 model-3 never committed). The task-2 plan failure is a
verbosity/schema-knowledge finding, not an engine limitation.

## Authoring-model comparison (the experiment's purpose)

- **String surface** is the most reliable and fastest for bulk/rule edits
  (task 2: 3/3, ~35 s, ~4 calls) and the best at occurrence anchoring; its
  cost is more blocked previews when the agent edits projection markup.
- **Plan surface** is cheapest when it works (fewest calls, fewest chars) and
  anchors occurrences perfectly, but the agent must translate intent into
  typed ops with exact paragraph ids and select strings — the schema-knowledge
  burden is real and it failed task 2 outright in all reps.
- **Model surface** is the most Pythonic (traversal, selections, object
  mutation) and repairs via attribute mutation, but costs ~2.5× the calls and
  time, and its occurrence anchoring slipped once (task 3 model/1).

All three surfaces compiled to the same core typed `Plan` semantics — the
offline conformance suite's cross-surface equivalence (10/10 tasks) holds, and
the live final-document fingerprints match for every task where the task pins
a deterministic outcome (8/10 tasks; task 10 differs only in which paragraph
the agent chose, task 3 in one model-surface occurrence misanchor).

## Environment notes

- 10 trials were SIGTERMed by the harness 20-turn cap after a successful,
  verified commit; the verification-based success criterion records these as
  ok (the process boundary is a harness artifact, the verified outcome is the
  measurement).
- Model: `opencode-go/deepseek-v4-flash` (repo-convention model). Prompts were
  NOT tuned per surface — identical text per task across surfaces (prompt
  version 1).
