#!/usr/bin/env node
// Recheck every retained final DOCX with the final verifier; retain original judgements.
import { readFile, writeFile, copyFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
const repo = resolve(import.meta.dirname, '..');
const dir = resolve(process.argv[2]);
const data = JSON.parse(await readFile(join(dir, 'summary.json'), 'utf8'));
const taskFixture = { text: 'contract', occurrence: 'contract', mixed: 'contract', repair: 'contract', equation: 'equation', complex: 'complex', inspect: 'parity' };
const changes = [];
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
for (const c of data.cells) {
  const original = join(dir, 'fixtures', taskFixture[c.task] + '.docx');
  const trialDir = join(dir, 'trials', `${c.task}-${c.rep}-${c.variant}`);
  if (sha(await readFile(original)) !== c.fixture_sha256) throw new Error('Fixture fingerprint mismatch');
  const proc = spawnSync('python3', [join(repo, 'bench/html-head-to-head-verify.py'), c.task, original, join(trialDir, 'document.docx')], { encoding: 'utf8' });
  if (proc.status !== 0) throw new Error(proc.stderr);
  let check = JSON.parse(proc.stdout);
  if (c.task === 'inspect') {
    try {
      const answer = JSON.parse(c.final_text.replace(/^```(?:json)?\s*|\s*```$/g, '').trim());
      const expected = { section_count: 2, equation_count: 28, available_image_width: 32, opaque_image_width: 48, formatting_author: 'Format Editor' };
      if (Object.keys(expected).some(k => answer[k] !== expected[k])) check = { outcome: 'error', message: 'Incorrect inspection answer', answer, expected };
    } catch { check = { outcome: 'error', message: 'Inspection answer is not JSON' }; }
  } else if (c.commits !== 1) check = { outcome: 'error', message: `Expected one commit, observed ${c.commits}` };
  if (c.task === 'repair' && c.rejected_calls < 1) check = { outcome: 'error', message: 'Missing deliberate rejected preview' };
  if (JSON.stringify(check) !== JSON.stringify(c.verification)) {
    c.pre_audit_verification = c.verification;
    changes.push({ task: c.task, rep: c.rep, variant: c.variant, before: c.verification, after: check });
  }
  c.verification = check; c.success = check.outcome === 'ok';
  await writeFile(join(trialDir, 'run.json'), JSON.stringify(c, null, 2));
}
const source = await readFile(join(repo, 'bench/html-head-to-head-verify.py'));
const audit = { checked_trials: data.cells.length, verifier_sha256: sha(source), changed_judgements: changes, note: data.metadata.python_protocol ? 'All retained final documents were independently rechecked with the unchanged final verifier, including unrelated XML/package-part checks and the protected Python commit protocol. No model calls or measured actions were rerun.' : 'Initial plain-paragraph check rejected any run properties. Final check permits normal font/color metadata and Latin-inactive complex-script flags while rejecting visible bold/italic/underline and special content. All retained outputs were rechecked; no model calls or measured actions were rerun.' };
data.metadata.verification_audit = audit;
data.metadata.pi_version = spawnSync('pi', ['--version'], { encoding: 'utf8' }).stdout.trim();
data.metadata.benchmark_sources_sha256 = Object.fromEntries(await Promise.all(['html-head-to-head.mjs', 'html-head-to-head-extension.ts', 'html-head-to-head-verify.py'].map(async name => [name, sha(await readFile(join(repo, 'bench', name)))])));
await writeFile(join(dir, 'summary.json'), JSON.stringify(data, null, 2));
await writeFile(join(dir, 'verification-audit.json'), JSON.stringify(audit, null, 2));
await copyFile(join(dir, 'report.md'), join(dir, 'pre-audit-report.md'));
const lines = [data.metadata.python_protocol ? '# Full-context HTML optimization through Python: audited raw results' : '# Old projection vs HTML / MathML: audited raw results', '', `Model ${data.metadata.model}; ${data.metadata.reps} repetitions per task. Baseline ${data.metadata.baseline_commit}. All ${data.cells.length} outputs independently rechecked.`, '', '| Trial | Outcome | Turns | Tool calls | Unexpected rejected calls | Total tokens | Wall seconds |', '| --- | --- | ---: | ---: | ---: | ---: | ---: |'];
for (const c of data.cells) lines.push(`| ${c.task}/${c.rep}/${c.variant} | ${c.success ? 'PASS' : 'FAIL'} | ${c.turns} | ${c.tool_calls} | ${c.unexpected_rejections} | ${c.total_tokens} | ${(c.wall_ms / 1000).toFixed(1)} |`);
lines.push('', '## Projection and read costs', '', '| Document / view | Old median ms | New median ms | Old markup reference tokens | New markup reference tokens | Old full result reference tokens | New full result reference tokens |', '| --- | ---: | ---: | ---: | ---: | ---: | ---: |');
for (const c of data.micro) lines.push(`| ${c.fixture}/${c.view} | ${c.old.read_ms.median.toFixed(2)} | ${c.new.read_ms.median.toFixed(2)} | ${c.old.markup_reference_tokens} | ${c.new.markup_reference_tokens} | ${c.old.response_reference_tokens} | ${c.new.response_reference_tokens} |`);
lines.push('', '## Verification audit', '', audit.note, '', `Changed initial classifications: ${changes.length}. Original judgements remain in pre-audit-report.md and each affected run.json; full audit in verification-audit.json.`, '', '## Failures', '');
for (const c of data.cells.filter(c => !c.success || c.protocol_errors.length)) lines.push(`- ${c.task}/${c.rep}/${c.variant}: ${JSON.stringify(c.verification)}; provider errors ${JSON.stringify(c.protocol_errors)}`);
lines.push('', data.metadata.python_protocol ? 'Provider usage includes repeated context and cache accounting. Projection reference counts use o200k_base, not DeepSeek tokenization. Both arms use the same shipped Python authoring host and native HTML/MathML. The controlled before arm uses saved WASM and generic display caps. No implementation optimization was made during the run.' : 'Provider-reported tokens include repeated context and cache accounting. Projection reference tokens use o200k_base, not DeepSeek tokenization. Provider caching remains enabled in both arms. Warm read timings use 50 alternating samples. Neutral common tooling returns one projection copy; this is not a full Python REPL comparison. No implementation optimization was made during the run.', '');
await writeFile(join(dir, 'report.md'), lines.join('\n'));
console.log(JSON.stringify(audit, null, 2));
