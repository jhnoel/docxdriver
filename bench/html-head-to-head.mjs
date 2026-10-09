#!/usr/bin/env node
// Paired engine and live-agent comparison. Live calls require --live and --model.
import { readFile, writeFile, mkdir, copyFile, mkdtemp } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { summarize } from './lib/stats.mjs';
import { CONTRACT_HTML } from '../packages/docxdriver-pi/test/repl-fixtures.mjs';

const repo = resolve(import.meta.dirname, '..');
const argv = process.argv.slice(2);
const option = (name, fallback) => argv.includes(name) ? argv[argv.indexOf(name) + 1] : fallback;
const baseline = option('--baseline', '/tmp/docxdriver-parity-baseline-0e5a0f9');
const model = option('--model', null);
const reps = Number(option('--reps', '3'));
const out = resolve(option('--out', join(repo, 'target', 'html-head-to-head', new Date().toISOString().replaceAll(':', '-'))));
const live = argv.includes('--live');
const pythonProtocol = argv.includes('--python');
const compactBaseline = argv.includes('--compact-baseline');
const includeSkillDocs = argv.includes('--skill-docs');
const skillPaths = ['packages/docxdriver-pi/skills/docx-pi-repl/SKILL.md', 'packages/docxdriver-pi/skills/docx-pi-repl/references/markup-dialect.md'];
const skillBundle = includeSkillDocs ? (await Promise.all(skillPaths.map(async path => `# Instructions from ${path}\n\n${await readFile(join(repo,path),'utf8')}`))).join('\n\n') : '';
if (!Number.isInteger(reps) || reps < 1) throw new Error('Invalid --reps');
if (live && !model) throw new Error('--live requires explicit --model');
const sha = bytes => createHash('sha256').update(bytes).digest('hex');
const engineDirs = { old: compactBaseline ? join(repo, 'target/html-head-to-head/compact-before-astra') : join(baseline, 'packages/docxdriver'), new: join(repo, 'packages/docxdriver') };
const pythonExtensions = { old: join(repo, 'target/html-head-to-head/python-before-astra/dist/python-plan-extension.js'), new: join(repo, 'packages/docxdriver-pi/dist/python-plan-extension.js') };
const engines = {};
const initStats = {};
await mkdir(out, { recursive: true });
if (includeSkillDocs) await writeFile(join(out, 'skill-bundle.md'), skillBundle);
for (const variant of ['old', 'new']) {
  const dir = engineDirs[variant];
  const wasm = await readFile(join(dir, 'wasm/docxdriver_bg.wasm'));
  const engine = await import(pathToFileURL(join(dir, 'dist/index.js')).href);
  const start = performance.now(); await engine.init(wasm);
  initStats[variant] = { init_ms: performance.now() - start, wasm_bytes: wasm.length, wasm_sha256: sha(wasm) };
  engines[variant] = engine;
}
const read = (variant, bytes, view = 'final') => {
  const result = engines[variant].executeRequest(bytes, { Command: { command: { kind: 'read', view } } });
  if (result.outcome !== 'completed') throw new Error(JSON.stringify(result));
  return result.result;
};
const created = engines.new.executeRequest(undefined, { Command: { command: { kind: 'create', html: CONTRACT_HTML } } });
if (!created.bytes) throw new Error('Fixture creation failed');
const fixtureDir = join(out, 'fixtures'); await mkdir(fixtureDir, { recursive: true });
await writeFile(join(fixtureDir, 'contract.docx'), created.bytes);
await copyFile(join(repo, 'target/html-e2e/source.docx'), join(fixtureDir, 'equation.docx'));
await copyFile(join(repo, 'test-docs/ctnf-18690238-data-stream.docx'), join(fixtureDir, 'complex.docx'));
await copyFile(join(repo, 'crates/docxdriver-core/tests/fixtures/projection-parity/parity.docx'), join(fixtureDir, 'parity.docx'));
const large = engines.new.executeRequest(undefined, { Command: { command: { kind: 'create', html: Array.from({ length: 500 }, (_, i) => `<p>Paragraph ${i}: The notice period is <b>thirty days</b>. Payment is due within 45 days of invoice receipt. Unicode 😀 α β.</p>`).join('\n') } } });
await writeFile(join(fixtureDir, 'large.docx'), large.bytes);

const referenceTokens = text => {
  const python = option('--tokenizer-python', null);
  if (!python) return null;
  const child = spawnSync(python, ['-c', 'import sys,tiktoken;print(len(tiktoken.get_encoding("o200k_base").encode(sys.stdin.read())))'], { input: text, encoding: 'utf8' });
  if (child.status !== 0) throw new Error(child.stderr);
  return Number(child.stdout.trim());
};
const micro = [];
for (const fixture of ['contract', 'equation', 'complex', 'parity', 'large']) {
  const bytes = await readFile(join(fixtureDir, fixture + '.docx'));
  for (const view of ['final', 'markup', 'original']) {
    const rows = {};
    const times = { old: [], new: [] };
    for (const variant of ['old', 'new']) {
      const result = read(variant, bytes, view);
      await writeFile(join(fixtureDir, `${fixture}-${view}-${variant}.json`), JSON.stringify(result));
      rows[variant] = {
        markup_bytes: Buffer.byteLength(result.markup), response_bytes: Buffer.byteLength(JSON.stringify(result)),
        markup_reference_tokens: referenceTokens(result.markup), response_reference_tokens: referenceTokens(JSON.stringify(result)),
        paragraph_count: result.paragraphs?.length ?? result.blocks?.flatMap(b => b.paragraphs ?? []).length ?? null,
      };
      for (let warm = 0; warm < 8; warm++) read(variant, bytes, view);
    }
    for (let iteration = 0; iteration < 50; iteration++) {
      for (const variant of iteration % 2 ? ['new', 'old'] : ['old', 'new']) {
        const start = performance.now(); read(variant, bytes, view); times[variant].push(performance.now() - start);
      }
    }
    for (const variant of ['old', 'new']) rows[variant].read_ms = summarize(times[variant]);
    micro.push({ fixture, view, fixture_sha256: sha(bytes), ...rows });
  }
}
// Verify the baseline build against the frozen native baseline, avoiding stale WASM artifacts.
const frozen = JSON.parse(await readFile(join(repo, 'crates/docxdriver-core/tests/fixtures/projection-parity/prior-projection.json'), 'utf8'));
const parity = await readFile(join(fixtureDir, 'parity.docx'));
for (const view of compactBaseline ? [] : ['final', 'markup', 'original']) {
  if (JSON.stringify(read('old', parity, view)) !== JSON.stringify(frozen[view])) {
    // JSON object ordering is not semantic; compare via canonical key ordering.
    const canon = value => Array.isArray(value) ? value.map(canon) : value && typeof value === 'object' ? Object.fromEntries(Object.keys(value).sort().map(k => [k, canon(value[k])])) : value;
    if (JSON.stringify(canon(read('old', parity, view))) !== JSON.stringify(canon(frozen[view]))) throw new Error(`Baseline mismatch: ${view}`);
  }
}

const tasks = [
  { id: 'text', fixture: 'contract', prompt: "Replace 'thirty days' with bold 'sixty days' in the notice period sentence. Preserve all other text and formatting." },
  { id: 'occurrence', fixture: 'contract', prompt: "In the paragraph beginning 'The parties agree', change ONLY the third occurrence of 'the terms' to 'THE TERMS'. Preserve the other three occurrences and everything else." },
  { id: 'mixed', fixture: 'contract', prompt: "In ONE atomic plan: replace 'thirty days' with bold 'sixty days'; make 'warranty period' bold and underlined; insert 'Inserted paragraph.' immediately after 'This paragraph anchors an insertion.'; delete 'This paragraph will be deleted.'. Preserve everything else." },
  { id: 'repair', fixture: 'contract', prompt: "First deliberately preview a replace_text selecting 'missing phrase' in the notice-period paragraph. It must be rejected. Then repair the selector to 'thirty days' and replace with bold 'sixty days', preview, commit, and verify. Preserve everything else." },
  { id: 'equation', fixture: 'equation', prompt: "Replace the equation in the paragraph beginning 'Equation ' with the fraction (a+b)/c. Preserve all text, other equations, formatting, headers, notes and images." },
  { id: 'complex', fixture: 'complex', prompt: "Choose ONE plain-text paragraph with no bold, italic, underline, links, fields, math or special content, and append ' [edited]' to its text. Preserve all other paragraph XML and all other package parts. Preview, commit and verify." },
  { id: 'inspect', fixture: 'parity', prompt: 'Read the document in markup view without editing it. Return ONLY a JSON object with section_count, equation_count, available_image_width (the PNG), opaque_image_width (the unsupported image), and formatting_author (the author of the pending formatting change). Count source equations once and exclude header/footer copies.' },
];
const chosenTasks = option('--tasks', '').split(',').filter(Boolean);
const selected = tasks.filter(t => !chosenTasks.length || chosenTasks.includes(t.id));
if (!selected.length) throw new Error('No tasks selected');

async function trial(task, rep, variant) {
  const dir = join(out, 'trials', `${task.id}-${rep}-${variant}`); await mkdir(dir, { recursive: true });
  const cwd = await mkdtemp(join(tmpdir(), `docxdriver-h2h-${variant}-`));
  const original = join(fixtureDir, task.fixture + '.docx');
  await copyFile(original, join(cwd, 'document.docx'));
  const prompt = skillBundle + (skillBundle ? '\n\n# Benchmark task\n' : '') + (pythonProtocol ? 'Use only the Python DOCX tool on document.docx. Read the COMPLETE document before editing, including ReadResult.equations, selection_space, selections, comments and assets (already included). Do not read selected sections or fetch binary asset payloads unless needed. Text replacements use inline HTML in with_; preserve existing formatting, and do not apply an extra formatting operation to an already bold replacement. Return docx_read("document.docx", view="final") as the expression to inspect it, or print its complete markup and metadata. For edits use Plan(author="Benchmark", change_mode="direct", operations=[...]), docx_review, inspect the report, and docx_commit in a separate later call. After commit, read the full final document to verify. Stop when verified. ' + task.prompt : 'Work only with the provided DOCX tools on document.docx. Read before editing. For edits, preview the plan, inspect the report, commit the returned key in a separate later call, then read final view to verify. Paragraph id attributes are addresses; source is the plan base. Stop when verified. ' + task.prompt);
  const args = ['--mode', 'json', '-p', '--no-session', '-na', '--model', model, '--thinking', option('--thinking', 'medium'), '--no-extensions', '-e', pythonProtocol ? pythonExtensions[variant] : join(repo, 'bench/html-head-to-head-extension.ts'), '--no-skills', '--no-context-files', '--no-prompt-templates', '--no-builtin-tools', '--tools', pythonProtocol ? 'python' : 'docx_read,docx_preview,docx_commit', prompt];
  const child = spawn(process.env.PI_E2E_COMMAND ?? 'pi', args, { cwd, env: { ...process.env, DOCXDRIVER_BENCH_VARIANT: variant, DOCXDRIVER_BENCH_ENGINE: engineDirs[variant] }, stdio: ['ignore', 'pipe', 'pipe'] });
  const started = performance.now();
  const events = []; const stdout = []; const stderr = []; let buffer = ''; let turns = 0; let capped = false; let timedOut = false;
  const parse = row => { try { return JSON.parse(row); } catch { return { type: 'text', text: row }; } };
  child.stdout.on('data', chunk => {
    stdout.push(chunk); buffer += chunk.toString(); const rows = buffer.split('\n'); buffer = rows.pop();
    for (const row of rows.filter(Boolean)) {
      const e = parse(row); events.push(e);
      if (e.type === 'message_end' && e.message?.role === 'assistant') turns++;
      if (turns >= 20 && !capped) { capped = true; child.kill('SIGTERM'); }
    }
  });
  child.stderr.on('data', chunk => stderr.push(chunk));
  const timeout = setTimeout(() => { timedOut = true; child.kill('SIGTERM'); }, 180_000);
  const status = await new Promise((res, rej) => { child.on('close', (code, signal) => res({ code, signal })); child.on('error', rej); });
  clearTimeout(timeout); if (buffer) events.push(parse(buffer));
  const messages = events.filter(e => e.type === 'message_end' && e.message?.role === 'assistant').map(e => e.message);
  const calls = events.filter(e => e.type === 'tool_execution_start');
  const results = events.filter(e => e.type === 'tool_execution_end');
  const usage = messages.filter(m => m.usage).map(m => m.usage);
  const sum = key => usage.some(u => u[key] != null) ? usage.reduce((n, u) => n + (u[key] ?? 0), 0) : null;
  const resultText = e => (e.result?.content ?? []).filter(c => c.type === 'text').map(c => c.text).join('\n');
  const parsedResults = results.map(e => { try { return { event: e, result: JSON.parse(resultText(e)) }; } catch { return { event: e, result: null }; } });
  const rejectionCount = pythonProtocol ? results.filter(e => e.isError || e.result?.details?.status === 'error' || /(?:── blocked ──|review: blocked)/.test(resultText(e))).length : parsedResults.filter(e => e.event.isError || e.result?.outcome === 'rejected').length;
  const commits = pythonProtocol ? results.reduce((n,e)=>n+(resultText(e).match(/^commit: k1:[0-9a-f]+ — committed:/gm)?.length ?? 0),0) : parsedResults.filter(e => e.result?.outcome === 'committed').length;
  const verificationProcess = spawnSync('python3', [join(repo, 'bench/html-head-to-head-verify.py'), task.id, original, join(cwd, 'document.docx')], { encoding: 'utf8' });
  if (verificationProcess.status !== 0) throw new Error(verificationProcess.stderr);
  let verification = JSON.parse(verificationProcess.stdout);
  let finalText = messages.at(-1)?.content?.filter(c => c.type === 'text').map(c => c.text).join('\n') ?? '';
  if (task.id === 'inspect') {
    try {
      const answer = JSON.parse(finalText.replace(/^```(?:json)?\s*|\s*```$/g, '').trim());
      const expected = { section_count: 2, equation_count: 28, available_image_width: 32, opaque_image_width: 48, formatting_author: 'Format Editor' };
      if (Object.keys(expected).some(k => answer[k] !== expected[k])) verification = { outcome: 'error', message: 'Incorrect inspection answer', answer, expected };
    } catch { verification = { outcome: 'error', message: 'Inspection answer is not JSON', finalText }; }
  } else if (commits !== 1) verification = { outcome: 'error', message: `Expected one commit, observed ${commits}`, document_check: verification };
  if (task.id === 'repair' && rejectionCount < 1) verification = { outcome: 'error', message: 'Missing deliberate rejected preview' };
  const protocolErrors = messages.filter(m => m.stopReason === 'error' || m.errorMessage).map(m => m.errorMessage ?? 'provider error');
  const record = {
    task: task.id, rep, variant, model, thinking: option('--thinking', 'medium'), fixture_sha256: sha(await readFile(original)),
    success: verification.outcome === 'ok', verification, status, timed_out: timedOut, turn_capped: capped,
    wall_ms: performance.now() - started, turns, tool_calls: calls.length, rejected_calls: rejectionCount,
    unexpected_rejections: Math.max(0, rejectionCount - (task.id === 'repair' ? 1 : 0)), commits,
    input_tokens: sum('input'), output_tokens: sum('output'), cache_read_tokens: sum('cacheRead'), cache_write_tokens: sum('cacheWrite'), reasoning_tokens: sum('reasoning'), total_tokens: sum('totalTokens'),
    pi_estimated_cost_usd: usage.length ? usage.reduce((n, u) => n + (u.cost?.total ?? 0), 0) : null,
    usage_messages: usage.length, protocol_errors: protocolErrors,
    authored_argument_chars: calls.reduce((n, c) => n + JSON.stringify(c.args ?? {}).length, 0),
    tool_response_chars: results.reduce((n, r) => n + resultText(r).length, 0),
    final_text: finalText, cwd,
  };
  await writeFile(join(dir, 'events.jsonl'), events.map(e => JSON.stringify(e)).join('\n') + '\n');
  await writeFile(join(dir, 'stderr.log'), Buffer.concat(stderr));
  await copyFile(join(cwd, 'document.docx'), join(dir, 'document.docx'));
  await writeFile(join(dir, 'run.json'), JSON.stringify(record, null, 2));
  return record;
}
const metadata = {
  baseline_commit: compactBaseline ? 'controlled pre-Astra compact WASM + generic Python display caps' : '0e5a0f9c559b3623bf5792e187c8c846fe627dfc',
  new_tree: spawnSync('git', ['diff', 'HEAD'], { cwd: repo, encoding: 'utf8' }).stdout ? 'uncommitted overhaul' : 'HEAD',
  model, reps, thinking: option('--thinking', 'medium'), token_reference_encoding: option('--tokenizer-python', null) ? 'o200k_base (reference, not DeepSeek tokenizer)' : null,
  runtime: { node: process.version, platform: process.platform, arch: process.arch }, init: initStats,
  task_protocol: pythonProtocol ? 'Shipped Python authoring tool, complete reads, direct edits. Controlled baseline uses saved pre-Astra WASM and restores generic display caps, with the same current Python host.' : 'Neutral tools: projection-only read, atomic preview, immutable-plan commit; direct edits; same wrapper and prompts except equation format description.',
  baseline_native_match: !compactBaseline,
  python_protocol: pythonProtocol,
  skill_docs: includeSkillDocs ? {paths:skillPaths,bundle_sha256:sha(Buffer.from(skillBundle)),injection:"Full instructions in the user prompt, identical in both arms; automatic skills/context disabled"} : null,
};
const cells = [];
const checkpoint = async () => { await writeFile(join(out, 'summary.json'), JSON.stringify({ metadata, micro, cells }, null, 2)); await writeReport(); };
async function writeReport() {
  const lines = [compactBaseline ? '# Compact full HTML before and after Astra recommendations' : '# Old projection vs HTML / MathML', '', `Baseline: ${metadata.baseline_commit}. Model: ${model ?? 'not run'}. Repetitions: ${reps}. Thinking: ${metadata.thinking}.`, '', '## Engine and projection size', '', '50 alternating warm reads per view; initialization shown separately. Reference token counts use o200k_base, not the live model tokenizer.', '', '| document / view | old read median ms | new read median ms | old markup tokens | new markup tokens | old full response tokens | new full response tokens |', '| --- | ---: | ---: | ---: | ---: | ---: | ---: |'];
  for (const c of micro) lines.push(`| ${c.fixture} / ${c.view} | ${c.old.read_ms.median.toFixed(2)} | ${c.new.read_ms.median.toFixed(2)} | ${c.old.markup_reference_tokens ?? 'n/a'} | ${c.new.markup_reference_tokens ?? 'n/a'} | ${c.old.response_reference_tokens ?? 'n/a'} | ${c.new.response_reference_tokens ?? 'n/a'} |`);
  if (cells.length) {
    lines.push('', '## Live agent trials', '', 'Provider-reported tokens sum each assistant response, including repeated context and cache accounting. Deliberate repair-task rejections are excluded from unexpected rejections. All trials are included, including failures.', '', '| task | old passed | new passed | old turns mean | new turns mean | old total tokens mean | new total tokens mean | old wall s mean | new wall s mean |', '| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |');
    const mean = (rows, key) => rows.length && rows.every(c => c[key] !== null) ? (rows.reduce((n, c) => n + c[key], 0) / rows.length).toFixed(1) : 'n/a';
    for (const task of [...selected, { id: 'ALL' }]) {
      const rows = variant => cells.filter(c => c.variant === variant && (task.id === 'ALL' || c.task === task.id));
      const old = rows('old'), next = rows('new'); const success = rows => `${rows.filter(c => c.success).length}/${rows.length}`;
      lines.push(`| ${task.id} | ${success(old)} | ${success(next)} | ${mean(old, 'turns')} | ${mean(next, 'turns')} | ${mean(old, 'total_tokens')} | ${mean(next, 'total_tokens')} | ${mean(old.map(c => ({ ...c, wall_s: c.wall_ms / 1000 })), 'wall_s')} | ${mean(next.map(c => ({ ...c, wall_s: c.wall_ms / 1000 })), 'wall_s')} |`);
    }
    lines.push('', '| metric | old | new |', '| --- | ---: | ---: |');
    for (const key of ['tool_calls', 'unexpected_rejections', 'input_tokens', 'output_tokens', 'cache_read_tokens', 'cache_write_tokens', 'reasoning_tokens', 'authored_argument_chars', 'tool_response_chars', 'pi_estimated_cost_usd']) {
      const total = variant => { const rows = cells.filter(c => c.variant === variant); return rows.every(c => c[key] !== null) ? rows.reduce((n, c) => n + c[key], 0) : 'n/a'; };
      lines.push(`| ${key} (total) | ${total('old')} | ${total('new')} |`);
    }
    lines.push('', '## Failures', '');
    for (const cell of cells.filter(c => !c.success || c.protocol_errors.length)) lines.push(`- ${cell.task}/${cell.rep}/${cell.variant}: ${JSON.stringify(cell.verification)}; provider errors: ${JSON.stringify(cell.protocol_errors)}`);
  }
  lines.push('', '## Scope and limits', '', pythonProtocol ? '- Both arms use the shipped Python tool and the same authoring host; the controlled before arm uses saved pre-Astra WASM and restores generic display caps.' : '- This isolates projection and equation authoring under a common tool wrapper; it does not compare the full shipped Python REPL host.', '- Full response sizes include all engine semantic metadata. Compact reads provide one full HTML projection. Python live trials include the actual read delivery path and host output.', '- Identical task fixture bytes and prompts; old/new order alternates by repetition and task. Temperature follows the same provider default; sessions and local task state are fresh.', '- Verifiers inspect DOCX XML and all unrelated ZIP parts independently of either projection.', '- Wall time includes provider latency and process startup; warm engine timing includes JSON decode. Initialization is one sample per variant, not a statistically stable cold-start estimate.', '- Small task suite and repetition count: interpret as directional evidence, not a universal model-quality conclusion.', '');
  await writeFile(join(out, 'report.md'), lines.join('\n'));
}
await checkpoint();
console.log(`Artifacts: ${out}`);
if (live) for (let taskIndex = 0; taskIndex < selected.length; taskIndex++) {
  const task = selected[taskIndex];
  for (let rep = 1; rep <= reps; rep++) for (const variant of (rep + taskIndex) % 2 ? ['old', 'new'] : ['new', 'old']) {
    const cell = await trial(task, rep, variant); cells.push(cell); await checkpoint();
    console.log(`${task.id}/${rep}/${variant}: ${cell.success ? 'PASS' : 'FAIL'} turns=${cell.turns} calls=${cell.tool_calls} unexpected=${cell.unexpected_rejections} tokens=${cell.total_tokens} seconds=${(cell.wall_ms / 1000).toFixed(1)}${cell.success ? '' : ' ' + cell.verification.message}`);
  }
}
console.log(`Report: ${join(out, 'report.md')}`);
