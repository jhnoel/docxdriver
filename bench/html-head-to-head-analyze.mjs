#!/usr/bin/env node
// Analyze paired results; bootstrap intervals describe repeat variability on this fixed suite.
import { readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
const dir = resolve(process.argv[2]);
const data = JSON.parse(await readFile(join(dir, 'summary.json'), 'utf8'));
const edits = JSON.parse(await readFile(join(dir, 'edit-performance.json'), 'utf8'));
const mean = values => values.reduce((a, b) => a + b, 0) / values.length;
const quantile = (values, p) => { const sorted = values.toSorted((a, b) => a - b); return sorted[Math.floor((sorted.length - 1) * p)]; };
const pairs = new Map();
for (const c of data.cells) { const key = `${c.task}/${c.rep}`; if (!pairs.has(key)) pairs.set(key, {}); pairs.get(key)[c.variant] = c; }
const paired = [...pairs.values()].filter(p => p.old && p.new);
const groups = new Map();
for (const p of paired) { if (!groups.has(p.old.task)) groups.set(p.old.task, []); groups.get(p.old.task).push(p); }
let seed = 20261007;
const random = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; };
const metrics = {};
for (const key of ['total_tokens', 'input_tokens', 'output_tokens', 'turns', 'tool_calls', 'wall_ms']) {
  if (!paired.every(p => p.old[key] !== null && p.new[key] !== null)) continue;
  const oldMean = mean(paired.map(p => p.old[key])), newMean = mean(paired.map(p => p.new[key]));
  const deltas = [], ratios = [];
  for (let b = 0; b < 10_000; b++) {
    const sample = [...groups.values()].flatMap(group => Array.from({ length: group.length }, () => group[Math.floor(random() * group.length)]));
    const a = mean(sample.map(p => p.old[key])), z = mean(sample.map(p => p.new[key]));
    deltas.push(z - a); ratios.push(z / a);
  }
  metrics[key] = { old_mean: oldMean, new_mean: newMean, new_old_ratio: newMean / oldMean, mean_paired_delta: newMean - oldMean, paired_delta_95_repeat_interval: [quantile(deltas, .025), quantile(deltas, .975)], ratio_95_repeat_interval: [quantile(ratios, .025), quantile(ratios, .975)] };
}
const counts = variant => {
  const rows = data.cells.filter(c => c.variant === variant);
  return {
    trials: rows.length, verified_successes: rows.filter(c => c.success).length,
    unexpected_rejections: rows.reduce((n, c) => n + c.unexpected_rejections, 0),
    trials_with_unexpected_rejections: rows.filter(c => c.unexpected_rejections > 0).length,
    infrastructure_or_provider_failures: rows.filter(c => c.status.code !== 0 || c.protocol_errors.length || c.usage_messages === 0).length,
    timeouts: rows.filter(c => c.timed_out).length,
    turn_caps: rows.filter(c => c.turn_capped).length,
    pi_estimated_cost_usd: rows.every(c => (c.pi_estimated_cost_usd ?? c.provider_reported_cost) != null) ? rows.reduce((n, c) => n + (c.pi_estimated_cost_usd ?? c.provider_reported_cost), 0) : null,
  };
};
const analysis = {
  pairs: paired.length, counts: { old: counts('old'), new: counts('new') }, metrics,
  success_discordance: {
    both_success: paired.filter(p => p.old.success && p.new.success).length,
    old_only_success: paired.filter(p => p.old.success && !p.new.success).length,
    new_only_success: paired.filter(p => !p.old.success && p.new.success).length,
    neither_success: paired.filter(p => !p.old.success && !p.new.success).length,
  },
  interval_scope: 'Seeded 10,000 resamples of paired repetitions within each fixed task; conditional repeat variability, not generalization to other tasks, models or documents.',
};
await writeFile(join(dir, 'analysis.json'), JSON.stringify(analysis, null, 2));
const lines = [
  '# Head-to-head measurement results', '',
  `Model: ${data.metadata.model}; thinking: ${data.metadata.thinking}; ${paired.length} matched old/new trials across ${groups.size} tasks. Baseline ${data.metadata.baseline_commit}.`, '',
  data.metadata.python_protocol ? '| Metric | Before | After | Change |' : '| Metric | Old | HTML / MathML | Change |', '| --- | ---: | ---: | ---: |',
  `| Verified success | ${analysis.counts.old.verified_successes}/${analysis.counts.old.trials} | ${analysis.counts.new.verified_successes}/${analysis.counts.new.trials} | |`,
  `| Unexpected rejected calls | ${analysis.counts.old.unexpected_rejections} | ${analysis.counts.new.unexpected_rejections} | |`,
];
for (const key of ['total_tokens', 'input_tokens', 'output_tokens', 'turns', 'tool_calls', 'wall_ms']) {
  const m = metrics[key]; if (!m) continue;
  const scale = key === 'wall_ms' ? 1000 : 1;
  const label = key === 'wall_ms' ? 'Wall seconds' : key === 'input_tokens' ? 'Uncached input tokens' : key;
  lines.push(`| ${label} / trial, mean | ${(m.old_mean / scale).toFixed(1)} | ${(m.new_mean / scale).toFixed(1)} | ${((m.new_old_ratio - 1) * 100).toFixed(1)}% |`);
}
lines.push('', '## Engine costs', '', '| Operation | Old median ms | New median ms | Ratio |', '| --- | ---: | ---: | ---: |');
for (const row of edits) for (const kind of ['preview', 'commit']) {
  const a = row.old[kind + '_ms'].median, b = row.new[kind + '_ms'].median;
  lines.push(`| ${row.task} / ${kind} | ${a.toFixed(2)} | ${b.toFixed(2)} | ${(b / a).toFixed(2)}× |`);
}
lines.push('', '## Paired repeat variability', '', 'The intervals below resample repetitions within each task. They describe this fixed suite, not unseen documents or models.', '', '| Metric | New/old mean ratio | 95% repeat interval |', '| --- | ---: | ---: |');
for (const key of ['total_tokens', 'turns', 'tool_calls', 'wall_ms']) {
  const m = metrics[key]; if (m) lines.push(`| ${key} | ${m.new_old_ratio.toFixed(2)}× | ${m.ratio_95_repeat_interval.map(x => x.toFixed(2)).join('–')}× |`);
}
lines.push('', '## Interpretation', '',
  '- The complete raw table and every failed outcome are in [report.md](report.md). Raw events, usage, diagnostics and final DOCX files are retained per trial.',
  '- Model token counts come from provider usage, including repeated conversation context and reported cache accounting. Local projection counts use o200k_base as a reference, not the model tokenizer.',
  '- Pi input_tokens excludes separately reported cache reads/writes; total_tokens includes them. The usage.cost values are Pi estimates using configured prices, not billing records. Provider caching remains enabled in both arms.',
  data.metadata.python_protocol ? '- Both arms use one complete HTML projection and the shipped Python authoring host. The controlled before arm restores generic display caps; the after arm preserves full document delivery.' : '- The controlled read tool provides one markup copy in both arms. The original overhaul API response also repeated html/markup and included block, paragraph, CSS and asset metadata.',
  data.metadata.python_protocol ? '- Both arms author native MathML and use the same Python plan/review/commit protocol. Engine timing uses identical direct operations.' : '- Edits use identical common operations; the equation task uses LaTeX for old and MathML for new. A common preview/commit wrapper isolates representation and equation authoring rather than measuring the full Python REPL product.',
  '- Package and XML verification are independent of the projection. A deliberately failed preview on the repair task is expected and excluded from unexpected rejection totals.',
  '- Wall time includes provider/network noise. One model, seven tasks and three repetitions per task cannot establish a universal quality difference.',
  '- No implementation optimization was made during the measured run.', '');
await writeFile(join(dir, 'comparison.md'), lines.join('\n'));
console.log(JSON.stringify(analysis, null, 2));
