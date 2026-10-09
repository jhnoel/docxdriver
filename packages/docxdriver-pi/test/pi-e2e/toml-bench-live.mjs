#!/usr/bin/env node
// Opt-in live-agent trials for the OLD docxdriver extension surface: the default
// five-tool extension (docx_create/docx_read/docx_find/docx_edit/docx_help)
// plus the builtin write/edit tools for authoring a TOML plan file, on the
// same 10-task benchmark as the Python REPL surfaces. The core preview/commit
// gate (p1:sha256: keys) is identical, so this measures authoring lift only.
//
// Opt-in guards (mirroring repl-live.mjs):
//   PI_E2E_LIVE=1   required — never runs under `npm test`
//   PI_E2E_MODEL    required — the model id
// Env:
//   PI_E2E_TASKS  comma list of task ids, default all manifest tasks
//   PI_E2E_REPS   repetitions per task, default 3 (the comparison matrix)
//   PI_E2E_COMMAND pi binary override, default "pi"
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { ensureInit, runRequest } from '../../dist/engine.js';
import { createComplexFixture, createContractFixture, zipPartHashes } from '../repl-fixtures.mjs';
import { checkFinalDocument, pairedToolCalls, parseLine, plainParagraphTexts, resultText, validateManifest } from './repl-live.mjs';

const here = import.meta.dirname;
const ROOT = join(here, '..', '..', '..', '..');

/** The default five-tool extension surface + builtin file tools for TOML plans. */
export const TOML_TOOLS = 'docx_create,docx_read,docx_find,docx_edit,docx_help,write,edit';

export const TOML_WORKFLOW_HINT =
  ' Work only through the docx tools and the write/edit tools: docx_read the document first ' +
  '(its result carries the source sha256: hash — that is the plan base), locate paragraph ids with ' +
  'docx_read/docx_find, author your plan as a TOML file named plan.toml using the write tool ' +
  '(shape: base = "sha256:...", author = "...", change_mode = "track", then [[ops]] tables with ' +
  'op = "replace_text" etc. and at/select/with/occurrence/position/style fields), preview with ' +
  'docx_edit(path, plan={file: "plan.toml"}) — the p1:sha256: preview key is in the result — then ' +
  'commit in a separate later docx_edit call passing that preview_key. Repair a rejected preview by ' +
  'editing plan.toml and previewing again. Stop when the requested outcome is achieved and verified.';

// ---------------------------------------------------------------------------
// Pure, offline-testable helpers
// ---------------------------------------------------------------------------

/**
 * Fold the old-surface event stream into the comparison metrics. A preview is
 * a docx_edit call without a preview_key whose outcome is "previewed"; a
 * commit is a docx_edit call with a preview_key whose outcome is "committed";
 * blocked counts rejected previews + rejected commits + write/edit failures;
 * invalidWriteAttempts counts rejected COMMIT attempts (the blocked-commit
 * guard). charsAuthored sums write content, edit oldText/newText, and the
 * docx_edit plan argument.
 */
export function collectTomlMetrics(events) {
  const { calls } = pairedToolCalls(events);
  let previewAttempts = 0;
  let commits = 0;
  let blockedCount = 0;
  let invalidWriteAttempts = 0;
  let charsAuthored = 0;
  let firstPreviewIndex = -1;
  let firstCommitIndex = -1;
  const texts = calls.map((call) => resultText(call.result));
  for (let i = 0; i < calls.length; i += 1) {
    const call = calls[i];
    const args = call.args ?? {};
    const text = texts[i];
    const committed = /"outcome"\s*:\s*"committed"/.test(text);
    const rejected = /"outcome"\s*:\s*"rejected"/.test(text);
    if (call.name === 'write' && typeof args.content === 'string') {
      charsAuthored += args.content.length;
      if (call.isError) blockedCount += 1;
    } else if (call.name === 'edit') {
      if (typeof args.oldText === 'string') charsAuthored += args.oldText.length;
      if (typeof args.newText === 'string') charsAuthored += args.newText.length;
      if (call.isError) blockedCount += 1;
    } else if (call.name === 'docx_edit') {
      if (args.plan !== undefined) charsAuthored += JSON.stringify(args.plan).length;
      const hasKey = typeof args.preview_key === 'string' && args.preview_key !== '';
      if (hasKey) {
        if (committed) {
          commits += 1;
          if (firstCommitIndex === -1) firstCommitIndex = i;
        } else if (rejected || call.isError) {
          blockedCount += 1;
          invalidWriteAttempts += 1;
        }
      } else if (committed) {
        // A commit without a key cannot happen through the old surface (the
        // engine requires the key); treat defensively as an anomaly.
        commits += 1;
        if (firstCommitIndex === -1) firstCommitIndex = i;
      } else if (rejected || call.isError) {
        // A rejected preview is a blocked attempt, not a preview — matches
        // the python-surface metric semantics.
        blockedCount += 1;
      } else {
        previewAttempts += 1;
        if (firstPreviewIndex === -1) firstPreviewIndex = i;
      }
    }
  }
  return {
    toolCalls: calls.length,
    previewAttempts,
    commits,
    blockedCount,
    invalidWriteAttempts,
    charsAuthored,
    explainableCommit: firstPreviewIndex !== -1 && firstCommitIndex !== -1 && firstPreviewIndex < firstCommitIndex,
  };
}

/** A trial succeeds when its final-state verification passed (same rule as
 * repl-live: a harness turn-cap SIGTERM after a verified outcome is recorded
 * but does not fail the trial). */
export function trialSuccess(record) {
  return record.verification?.outcome === 'ok';
}

// ---------------------------------------------------------------------------
// Trial execution
// ---------------------------------------------------------------------------

async function ensureCore() {
  await ensureInit();
}

async function renderFinalMarkup(abs) {
  await ensureCore();
  const bytes = new Uint8Array(await readFile(abs));
  const output = runRequest(bytes, { Command: { command: { kind: 'read', view: 'final' } } });
  if (output.outcome !== 'completed' || !output.result || typeof output.result.markup !== 'string') {
    throw new Error(`final render failed: ${output.diagnostic?.message ?? output.summary ?? 'unknown error'}`);
  }
  return output.result.markup;
}

function sha256Hex(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

async function runTrial(task, rep, resultDir, model) {
  const cwd = await mkdtemp(join(tmpdir(), `docxdriver-toml-live-${task.id}-`));
  const fixturePath = task.fixture === 'complex' ? await createComplexFixture(cwd) : await createContractFixture(cwd);
  const fixtureSha = sha256Hex(await readFile(fixturePath));
  const beforeParts = task.id === 'task10-package-preservation' ? await zipPartHashes(fixturePath) : null;
  const extPath = join(ROOT, 'packages/docxdriver-pi/src/index.ts');
  const prompt = `${task.prompt}${TOML_WORKFLOW_HINT}`;
  const args = [
    '--mode', 'json', '-p', '--no-session', '-na', '--model', model,
    '--no-extensions', '-e', extPath, '--no-skills', '--no-context-files',
    '--no-prompt-templates', '--no-builtin-tools', '--tools', TOML_TOOLS, prompt,
  ];
  const child = spawn(process.env.PI_E2E_COMMAND ?? 'pi', args, { cwd, env: { ...process.env }, stdio: ['ignore', 'pipe', 'pipe'] });
  const lines = [];
  const stdout = [];
  const stderr = [];
  let turns = 0;
  let timedOut = false;
  let turnCapped = false;
  let buffer = '';
  let tampered = false;
  let tamperSettled = Promise.resolve();
  const tamperFile = async () => {
    if (tampered) return;
    tampered = true;
    const bytes = await readFile(fixturePath);
    const tamperedBytes = Buffer.concat([bytes.subarray(0, 40), Buffer.from('XX'), bytes.subarray(40)]);
    await writeFile(fixturePath, tamperedBytes);
  };
  child.stdout.on('data', (chunk) => {
    stdout.push(chunk);
    buffer += chunk.toString();
    const rows = buffer.split('\n');
    buffer = rows.pop() ?? '';
    for (const row of rows) {
      if (!row) continue;
      lines.push(parseLine(row));
      const event = lines.at(-1);
      if (event?.type === 'message_end' && event?.message?.role === 'assistant') turns += 1;
      if (turns >= 20 && !turnCapped) {
        turnCapped = true;
        child.kill('SIGTERM');
      }
      // Task 8: harness-injected external source race — tamper the file the
      // moment a successful preview key appears in a tool result.
      if (task.id === 'task8-source-race' && event?.type === 'tool_execution_end' && resultText(event.result).includes('p1:sha256:')) {
        tamperSettled = tamperFile();
      }
    }
  });
  child.stderr.on('data', (chunk) => stderr.push(chunk));
  const started = Date.now();
  const timeout = setTimeout(() => {
    timedOut = true;
    child.kill('SIGTERM');
  }, 5 * 60 * 1000);
  const status = await new Promise((resolve) => child.on('close', (code, signal) => resolve({ code, signal })));
  clearTimeout(timeout);
  await tamperSettled;
  if (buffer) lines.push(parseLine(buffer));

  const metrics = collectTomlMetrics(lines);
  const record = {
    surface: 'toml',
    task: task.id,
    rep,
    model,
    prompt_version: 1,
    status: { code: status.code, signal: status.signal },
    timed_out: timedOut,
    turn_capped: turnCapped,
    turns,
    wall_ms: Date.now() - started,
    metrics,
    fixtureSha,
  };
  if (task.id === 'task10-package-preservation') {
    record.partHashes = { before: Object.fromEntries(beforeParts ?? new Map()), after: null };
  }

  try {
    await ensureCore();
    if (task.verify === 'commit') {
      const finalMarkup = await renderFinalMarkup(fixturePath);
      const texts = plainParagraphTexts(finalMarkup);
      let afterParts;
      if (task.id === 'task10-package-preservation') {
        afterParts = await zipPartHashes(fixturePath);
        record.partHashes.after = Object.fromEntries(afterParts);
      }
      checkFinalDocument(task.id, finalMarkup, texts, {
        verifyParts: () => ({ before: beforeParts ?? new Map(), after: afterParts ?? new Map() }),
      });
      if (metrics.commits < 1) throw new Error(`task ${task.id}: no successful commit observed`);
      if (!metrics.explainableCommit) throw new Error(`task ${task.id}: no preview key seen before the first commit`);
      record.verification = { outcome: 'ok', finalMarkup };
    } else {
      const finalBytes = await readFile(fixturePath);
      const finalSha = sha256Hex(finalBytes);
      if (task.id === 'task8-source-race') {
        if (!metrics.invalidWriteAttempts) throw new Error('task8: no blocked commit observed');
        if (!tampered) throw new Error('task8: harness tamper did not run');
        if (metrics.commits > 0) throw new Error('task8: a commit succeeded despite the source race');
        if (finalSha === fixtureSha) throw new Error('task8: file bytes were not preserved as tampered');
      } else {
        throw new Error(`task ${task.id}: unsupported blocked-task id`);
      }
      record.verification = { outcome: 'ok', finalSha };
    }
  } catch (error) {
    record.verification = { outcome: 'error', message: error instanceof Error ? error.message : String(error) };
    record.infrastructure_error = record.verification.message;
  }
  if (status.code !== 0 || status.signal) {
    record.infrastructure_error ??= `pi exited unsuccessfully: code=${status.code}, signal=${status.signal}`;
  }

  const taskDir = join(resultDir, task.id, `toml-${rep}`);
  await mkdir(taskDir, { recursive: true });
  await writeFile(join(taskDir, 'events.jsonl'), lines.map((event) => JSON.stringify(event)).join('\n') + '\n');
  await writeFile(join(taskDir, 'stdout.log'), Buffer.concat(stdout));
  await writeFile(join(taskDir, 'stderr.log'), Buffer.concat(stderr));
  await writeFile(join(taskDir, 'run.json'), JSON.stringify(record, null, 2));
  return record;
}

async function main() {
  if (process.env.PI_E2E_LIVE !== '1') throw new Error('live TOML trials require PI_E2E_LIVE=1');
  const model = process.env.PI_E2E_MODEL;
  if (!model) throw new Error('live TOML trials require explicit PI_E2E_MODEL');
  const manifest = validateManifest(JSON.parse(await readFile(join(here, 'toml-tasks.json'), 'utf8')));
  const taskFilter = (process.env.PI_E2E_TASKS ?? '').split(',').map((s) => s.trim()).filter(Boolean);
  const reps = Number.parseInt(process.env.PI_E2E_REPS ?? '3', 10);
  if (!Number.isInteger(reps) || reps < 1) throw new Error('PI_E2E_REPS must be a positive integer');
  const tasks = manifest.tasks.filter((task) => taskFilter.length === 0 || taskFilter.includes(task.id));
  if (tasks.length === 0) throw new Error('PI_E2E_TASKS matched no manifest tasks');
  const runId = `${new Date().toISOString().replaceAll(':', '-')}-${model.replaceAll('/', '_')}`;
  const resultDir = join(here, 'results', runId);
  await mkdir(resultDir, { recursive: true });
  console.log(`toml live trials: model=${model} tasks=${tasks.map((t) => t.id).join(',')} reps=${reps} -> ${resultDir}`);

  const cells = [];
  for (const task of tasks) {
    for (let rep = 1; rep <= reps; rep += 1) {
      const record = await runTrial(task, rep, resultDir, model);
      cells.push(record);
      const ok = trialSuccess(record);
      console.log(`${task.id}/toml/${rep}: ${ok ? 'ok' : 'FAIL'} calls=${record.metrics.toolCalls} previews=${record.metrics.previewAttempts} blocked=${record.metrics.blockedCount} commits=${record.metrics.commits} chars=${record.metrics.charsAuthored} wallMs=${record.wall_ms}${record.infrastructure_error ? ` -- ${record.infrastructure_error}` : ''}`);
    }
  }

  const summary = {
    generated: new Date().toISOString(),
    runId,
    model,
    surface: 'toml',
    reps,
    promptVersion: 1,
    baseline: null,
    cells: cells.map((c) => ({
      surface: c.surface,
      task: c.task,
      rep: c.rep,
      success: trialSuccess(c),
      toolCalls: c.metrics.toolCalls,
      previewAttempts: c.metrics.previewAttempts,
      commits: c.metrics.commits,
      blockedCount: c.metrics.blockedCount,
      invalidWriteAttempts: c.metrics.invalidWriteAttempts,
      charsAuthored: c.metrics.charsAuthored,
      wallMs: c.wall_ms,
      explainableCommit: c.metrics.explainableCommit,
      error: c.infrastructure_error ?? null,
    })),
  };

  // Comparison vs the Python REPL surfaces (the plan/string re-measurement).
  const baselineDir = join(here, 'results', '2026-08-11T12-52-53.277Z-opencode-go_deepseek-v4-flash');
  let baseline = null;
  try {
    baseline = JSON.parse(await readFile(join(baselineDir, 'summary.json'), 'utf8'));
  } catch {
    console.log('baseline summary.json not found; writing summary.json only');
  }
  if (baseline) {
    summary.baseline = { runId: baseline.runId, model: baseline.model };
    const flat = cells.map((c) => ({
      surface: c.surface,
      task: c.task,
      success: trialSuccess(c),
      toolCalls: c.metrics.toolCalls,
      previewAttempts: c.metrics.previewAttempts,
      commits: c.metrics.commits,
      blockedCount: c.metrics.blockedCount,
      invalidWriteAttempts: c.metrics.invalidWriteAttempts,
      charsAuthored: c.metrics.charsAuthored,
      wallMs: c.wall_ms,
    }));
    const md = [];
    md.push('# TOML (old extension) vs Python REPL surfaces — live comparison');
    md.push('');
    md.push(`- TOML run: \`${runId}\` (this directory) — default five-tool extension + write/edit TOML plans, model \`${model}\`, ${reps} reps`);
    md.push(`- Baseline run: \`${baseline.runId}\` — plan + string Python REPL surfaces, same model, 3 reps`);
    md.push('- Prompt version: 1 per surface; task intents identical; the TOML prompts adapt the tooling wording (docx_edit + write/edit instead of the python tool).');
    md.push('');
    const byTask = new Map();
    for (const c of flat) {
      if (!byTask.has(c.task)) byTask.set(c.task, []);
      byTask.get(c.task).push(c);
    }
    const ok = (c) => c.success;
    const avg = (arr, f) => (arr.length ? (arr.reduce((s, c) => s + f(c), 0) / arr.length).toFixed(1) : 'n/a');
    md.push('## Per-task success (ok / reps)');
    md.push('');
    md.push('| task | toml | plan (baseline) | string (baseline) |');
    md.push('| --- | --- | --- | --- |');
    for (const task of tasks) {
      const t = byTask.get(task.id) ?? [];
      const p = baseline.cells.filter((c) => c.surface === 'plan' && c.task === task.id);
      const s = baseline.cells.filter((c) => c.surface === 'string' && c.task === task.id);
      const fmt = (arr) => `${arr.filter(ok).length}/${arr.length}`;
      md.push(`| ${task.id.replace(/^task\d+-/, '')} | ${fmt(t)} | ${fmt(p)} | ${fmt(s)} |`);
    }
    md.push('');
    md.push('## Aggregate effort (averages over all trials per surface)');
    md.push('');
    md.push('| metric | toml | plan | string |');
    md.push('| --- | --- | --- | --- |');
    const all = flat;
    const allP = baseline.cells.filter((c) => c.surface === 'plan');
    const allS = baseline.cells.filter((c) => c.surface === 'string');
    md.push(`| tool calls / trial | ${avg(all, (c) => c.toolCalls)} | ${avg(allP, (c) => c.toolCalls)} | ${avg(allS, (c) => c.toolCalls)} |`);
    md.push(`| chars authored / trial | ${avg(all, (c) => c.charsAuthored)} | ${avg(allP, (c) => c.pythonChars)} | ${avg(allS, (c) => c.pythonChars)} |`);
    md.push(`| wall clock / trial | ${avg(all, (c) => c.wallMs / 1000)}s | ${avg(allP, (c) => c.wallMs / 1000)}s | ${avg(allS, (c) => c.wallMs / 1000)}s |`);
    const sum = (arr, f) => arr.reduce((s, c) => s + f(c), 0);
    md.push(`| blocked previews (total) | ${sum(all, (c) => c.blockedCount)} | ${sum(allP, (c) => c.blockedCount)} | ${sum(allS, (c) => c.blockedCount)} |`);
    md.push(`| preview attempts (total) | ${sum(all, (c) => c.previewAttempts)} | ${sum(allP, (c) => c.previewAttempts)} | ${sum(allS, (c) => c.previewAttempts)} |`);
    md.push(`| invalid write attempts | ${sum(all, (c) => c.invalidWriteAttempts)} | ${sum(allP, (c) => c.invalidWriteAttempts)} | ${sum(allS, (c) => c.invalidWriteAttempts)} |`);
    md.push(`| success | ${all.filter(ok).length}/${all.length} | ${allP.filter(ok).length}/${allP.length} | ${allS.filter(ok).length}/${allS.length} |`);
    md.push('');
    md.push('## Notes');
    md.push('');
    md.push('- chars authored: TOML counts write content + edit old/new text + the docx_edit plan argument; the python surfaces count the python `code` argument. Both approximate authored tokens, not full session tokens (reads and tool outputs are excluded).');
    md.push('- The TOML surface has no regex/loop primitive: task 2\'s "rule-based bulk edit" becomes three enumerated operations in one plan (the prompt says one atomic plan).');
    md.push('- The TOML flow has no persistent in-session state: task 7 repairs by editing plan.toml; task 9 mutates by editing plan.toml after preview.');
    md.push('');
    await writeFile(join(resultDir, 'summary-comparison.md'), md.join('\n'));
  }

  await writeFile(join(resultDir, 'summary.json'), JSON.stringify(summary, null, 2));
  console.log(`wrote ${join(resultDir, 'summary.json')}`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.stack ?? error.message : String(error));
    process.exitCode = 1;
  });
}
