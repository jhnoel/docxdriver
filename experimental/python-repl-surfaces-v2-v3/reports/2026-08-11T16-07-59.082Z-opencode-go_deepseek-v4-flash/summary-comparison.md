# TOML (old extension) vs Python REPL surfaces — live comparison

- TOML run: `2026-08-11T16-07-59.082Z-opencode-go_deepseek-v4-flash` (this directory) — default five-tool extension + write/edit TOML plans, model `opencode-go/deepseek-v4-flash`, 3 reps
- Baseline run: `2026-08-11T12-52-53.277Z-opencode-go_deepseek-v4-flash` — plan + string Python REPL surfaces, same model, 3 reps
- Prompt version: 1 per surface; task intents identical; the TOML prompts adapt the tooling wording (docx_edit + write/edit instead of the python tool).

## Per-task success (ok / reps)

| task | toml | plan (baseline) | string (baseline) |
| --- | --- | --- | --- |
| exact-text-correction | 3/3 | 3/3 | 3/3 |
| repeated-bulk-edit | 3/3 | 3/3 | 3/3 |
| ambiguous-occurrence | 3/3 | 3/3 | 3/3 |
| arbitrary-formatting-selection | 3/3 | 3/3 | 3/3 |
| paragraph-structure | 3/3 | 3/3 | 3/3 |
| mixed-atomic-edit | 3/3 | 3/3 | 3/3 |
| failed-preview-repair | 3/3 | 3/3 | 2/3 |
| source-race | 3/3 | 3/3 | 3/3 |
| mutation-after-preview | 3/3 | 3/3 | 3/3 |
| package-preservation | 3/3 | 3/3 | 3/3 |

## Aggregate effort (averages over all trials per surface)

| metric | toml | plan | string |
| --- | --- | --- | --- |
| tool calls / trial | 9.1 | 5.0 | 5.7 |
| chars authored / trial | 466.0 | 1078.4 | 1864.9 |
| wall clock / trial | 95.4s | 55.8s | 72.9s |
| blocked previews (total) | 24 | 9 | 14 |
| preview attempts (total) | 38 | 30 | 35 |
| invalid write attempts | 9 | 6 | 6 |
| success | 30/30 | 30/30 | 29/30 |

## Notes

- chars authored: TOML counts write content + edit old/new text + the docx_edit plan argument; the python surfaces count the python `code` argument. Both approximate authored tokens, not full session tokens (reads and tool outputs are excluded).
- The TOML surface has no regex/loop primitive: task 2's "rule-based bulk edit" becomes three enumerated operations in one plan (the prompt says one atomic plan).
- The TOML flow has no persistent in-session state: task 7 repairs by editing plan.toml; task 9 mutates by editing plan.toml after preview.
