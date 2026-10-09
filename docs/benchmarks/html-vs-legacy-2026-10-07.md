# HTML versus prior projection — 2026-10-07

Model: opencode-go/deepseek-v4-flash; thinking: medium; 21 matched old/new trials across 7 tasks. Baseline 0e5a0f9c559b3623bf5792e187c8c846fe627dfc.

| Metric | Old | HTML / MathML | Change |
| --- | ---: | ---: | ---: |
| Verified success | 21/21 | 21/21 | |
| Unexpected rejected calls | 1 | 1 | |
| total_tokens / trial, mean | 20204.7 | 27038.1 | 33.8% |
| Uncached input tokens / trial, mean | 5623.1 | 8688.1 | 54.5% |
| output_tokens / trial, mean | 928.2 | 789.6 | -14.9% |
| turns / trial, mean | 5.0 | 4.8 | -3.8% |
| tool_calls / trial, mean | 4.0 | 3.9 | -4.7% |
| Wall seconds / trial, mean | 9.7 | 11.0 | 13.7% |

## Engine costs

| Operation | Old median ms | New median ms | Ratio |
| --- | ---: | ---: | ---: |
| text / preview | 0.59 | 1.84 | 3.14× |
| text / commit | 1.40 | 2.67 | 1.91× |
| mixed / preview | 1.30 | 6.31 | 4.85× |
| mixed / commit | 2.05 | 7.08 | 3.46× |
| equation / preview | 3.06 | 7.86 | 2.57× |
| equation / commit | 4.01 | 8.83 | 2.20× |

## Paired repeat variability

The intervals below resample repetitions within each task. They describe this fixed suite, not unseen documents or models.

| Metric | New/old mean ratio | 95% repeat interval |
| --- | ---: | ---: |
| total_tokens | 1.34× | 1.20–1.52× |
| turns | 0.96× | 0.93–1.00× |
| tool_calls | 0.95× | 0.90–1.01× |
| wall_ms | 1.14× | 0.88–1.54× |

## Interpretation

- The complete raw table and every failed outcome are in [raw results](../../target/html-head-to-head/deepseek-v4-flash-3x/report.md). Raw events, usage, diagnostics and final DOCX files are retained per trial.
- Model token counts come from provider usage, including repeated conversation context and reported cache accounting. Local projection counts use o200k_base as a reference, not the model tokenizer.
- Pi input_tokens excludes separately reported cache reads/writes; total_tokens includes them. The usage.cost values are Pi estimates using configured prices, not billing records. Provider caching remains enabled in both arms.
- The controlled read tool provides one markup copy in both arms. The full new API response is larger still because it includes html and markup, blocks, paragraph/source metadata, CSS and asset manifests.
- Edits use identical common operations; the equation task uses LaTeX for old and MathML for new. A common preview/commit wrapper isolates representation and equation authoring rather than measuring the full Python REPL product.
- Package and XML verification are independent of the projection. A deliberately failed preview on the repair task is expected and excluded from unexpected rejection totals.
- Wall time includes provider/network noise. One model, seven tasks and three repetitions per task cannot establish a universal quality difference.
- No implementation optimization was made during the measured run.

## Per-task model effort

| Task | Old success | New success | Old mean turns | New mean turns | Old mean tokens | New mean tokens |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| text | 3/3 | 3/3 | 5.00 | 5.33 | 14,205 | 19,257 |
| occurrence | 3/3 | 3/3 | 5.00 | 5.00 | 14,529 | 17,054 |
| mixed | 3/3 | 3/3 | 6.00 | 5.33 | 23,931 | 22,226 |
| repair | 3/3 | 3/3 | 6.00 | 6.00 | 19,292 | 22,468 |
| equation | 3/3 | 3/3 | 5.00 | 5.00 | 14,375 | 19,596 |
| complex | 3/3 | 3/3 | 5.00 | 5.00 | 40,851 | 79,666 |
| inspect | 3/3 | 3/3 | 3.00 | 2.00 | 14,249 | 8,999 |

## Projection size and read runtime

Final-view reads; medians from 50 alternating warm samples. Token counts here use the reference tokenizer.

| Fixture | Old read ms | New read ms | Markup token ratio | Full result token ratio |
| --- | ---: | ---: | ---: | ---: |
| contract | 0.58 | 3.03 | 2.31× | 4.38× |
| equation | 4.27 | 11.76 | 3.38× | 5.19× |
| complex | 11.34 | 28.61 | 1.85× | 3.60× |
| parity | 2.93 | 5.55 | 2.39× | 4.29× |
| large | 9.73 | 30.82 | 2.11× | 3.94× |

## Verification audit and provenance

The first plain-paragraph checker treated every Word run-property element as visible formatting. It falsely rejected the six package-preservation trials because they contained ordinary font/color metadata. The final checker evaluates bold/italic/underline, inherited styles, and special content. Every retained output was rechecked, and all six passed. Initial judgements remain preserved in the raw artifacts; no model actions were rerun. Eight negative-control/verifier tests passed.

The old WASM was rebuilt from the pinned commit and checked against the frozen native results in every view before live calls. Identical DOCX fingerprints are recorded in both arms. WASM hashes, Pi/Node versions, tool traces, usage, preview diagnostics, and final DOCX files are retained under `target/html-head-to-head/deepseek-v4-flash-3x/`.

The benchmark and reproduction instructions are in [bench/README.md](../../bench/README.md). This run supports equal observed correctness with higher token and engine costs. The small turn reduction comes mainly from metadata inspection and mixed editing; it does not establish a general efficiency improvement. Wall-time medians were 9.37 s old and 9.25 s new; the mean increase is sensitive to provider outliers.
