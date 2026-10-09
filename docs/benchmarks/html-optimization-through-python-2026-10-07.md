# Full HTML optimization through Python — 2026-10-07

The projection remains complete, renderable HTML with native MathML. Optimizations remove duplicate output, share repeated styles using ordinary CSS classes, shorten image URLs with a manifest mapping to canonical asset URLs and package parts, and preserve source selection exceptions and comment anchors. Full document reads remain the default.

## Controlled comparison

Both arms use the current Python authoring host and identical prompts, with the full shipped `packages/docxdriver-pi/skills/docx-pi-repl/SKILL.md` and `references/markup-dialect.md` injected in the prompt. The exact skill bundle and hash are retained with the benchmark artifacts. API documentation and examples are embedded in the Python tool description. Automatic skill loading is disabled because the benchmark exposes only the Python tool.

The before arm uses saved pre-optimization WASM and restores generic output display caps. This isolates the representation and document delivery changes; it is not an exact historical Python checkout. Both arms use HTML and MathML. The separate legacy-versus-HTML benchmark measures the original migration.

Model: `opencode-go/deepseek-v4-flash`, medium thinking. Seven tasks, three repetitions, alternating order, fresh documents and sessions: 42 trials. DOCX XML and unrelated package parts are independently verified. Provider usage includes cached context. Wall time includes provider latency and process startup.

| Metric | Before | After | Change |
| --- | ---: | ---: | ---: |
| Verified trials | 19/21 | 20/21 | +1 |
| Verified edit trials | 18/18 | 18/18 | unchanged |
| Mean total tokens | 77,434 | 59,314 | −23.4% |
| Mean turns | 6.29 | 5.14 | −18.2% |
| Tool calls | 111 | 87 | −21.6% |
| Unexpected rejections | 11 | 3 | −8 |
| Mean wall seconds | 14.72 | 13.23 | −10.1% |

All three failures were inspection answers containing extra prose around otherwise correct JSON. There were no provider failures, timeouts, or turn caps. Rejections include Python errors and unsuccessful selectors, excluding the deliberate repair-task rejection.

Paired bootstrap repeat intervals for the after/before ratio: total tokens 0.686–0.880, turns 0.743–0.909, calls 0.702–0.892, wall time 0.719–1.125. These describe variability on this fixed suite, not generalization to other documents or models. The smaller prior run without the skill showed no aggregate token benefit; these runs do not isolate the causal effect of documentation.

## Rendering correction after the live run

The measured implementation already used a `.docxdriver` wrapper to preserve CSS precedence. Astra subsequently found that locally numbered shared classes collided when two document fragments were mounted together. The final implementation scopes rules and wrappers by a deterministic 96-bit digest of the style map. Identical maps can safely share a scope; different maps remain isolated. Declaration contents are preserved without string substitutions.

This correction was made after the 42 live trials. Their provider metrics must not be presented as measurements of the final corrected build. A separate engine-only rerun measures that build, and a Chromium test mounts red, blue, and repeated red fragments together. The additional Astra review was requested but could not execute because of a usage limit; the corrected build has not received a completed repeat Astra review.

## Corrected-build read sizes

Complete engine responses, reference tokens (`o200k_base`):

| Fixture / view | Before | Corrected build | Reduction |
| --- | ---: | ---: | ---: |
| Contract / final | 773 | 723 | 6.5% |
| Equation / final | 1,201 | 1,131 | 5.8% |
| Complex / final | 8,926 | 7,160 | 19.8% |
| Parity / markup | 5,018 | 4,484 | 10.6% |
| Large / final | 48,986 | 39,586 | 19.2% |

Warm read medians increased modestly: contract 1.74→1.80 ms, equation 7.71→7.98 ms, complex 20.69→21.56 ms, parity markup 4.72→4.84 ms, large 24.97→27.87 ms. There is a small engine CPU cost for reducing the model response size.

## Validation and artifacts

The corrected build passes 96 Rust workspace tests, 118 Pi tests, and six Chromium end-to-end tests, including full-document semantic parity, comment and source selections, image occurrence associations, computed formatting, native MathML, and isolation across document fragments. Existing Python tests verify complete returned, printed, and saved reads beyond generic output caps and suppress identical printed/returned output.

Artifacts under `target/html-head-to-head/`:

- `python-html-final-with-skill-3x/`: full skill bundle, metadata and WASM hashes, 42 retained trial outputs, audited verification, provider metrics, report, and paired analysis.
- `python-html-scoped-engine/`: corrected-build complete-read sizes and warm timings.
- `python-compact-3x/`: earlier comparison without the standalone skill.

Reference projection tokens use `o200k_base`, not the live model's tokenizer. Complete response sizes include semantic metadata. No selective section reads or opaque replacement syntax were introduced.


Later holistic-review fixes add content-control traversal, accurate MathML accent
writes, and scoped note/mount identities. The measurements above belong to the
recorded earlier builds; they were not rerun after those fixes. See
[the resolved review](../reviews/html-overhaul-holistic-2026-10-07.md) for validation.
