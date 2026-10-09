#!/usr/bin/env node
// Opt-in live-agent trials for the V1 typed-plan Python REPL surface
// (python-plan-extension.ts). Each trial: fresh temp cwd, byte-identical
// fixture copy, one spawned `pi` process with ONLY the python tool, one
// prompt per task. The review protocol is the measured surface: review
// validates one typed plan, returns bounded edit context and a compact k1:
// commit key, and a later commit consumes exactly one key. Events and metrics
// are recorded under test/pi-e2e/results/<run-id>.
//
// Opt-in guards (mirroring test/pi-e2e/run.mjs):
//   PI_E2E_LIVE=1   required — never runs under `npm test`
//   PI_E2E_MODEL    required — the model id, e.g. opencode-go/deepseek-v4-flash
// Env:
//   PI_E2E_SURFACES  comma list, V1 only: "plan" (default)
//   PI_E2E_TASKS     comma list of task ids, default all manifest tasks
//   PI_E2E_REPS      repetitions per task, default 1 (full matrix: 3)
//   PI_E2E_COMMAND   pi binary override, default "pi"
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { ensureInit, runRequest } from '../../dist/engine.js';
import { createComplexFixture, createContractFixture, zipPartHashes } from '../repl-fixtures.mjs';

/**
 * Minimal plain-text listing of top-level rendered markup blocks: strips
 * tags from each top-level `<p>`/`<h1>`..`<h6>` block and decodes the basic
 * entities. This is a harness-only fingerprint helper for final-document
 * checks; the production read/find surfaces are core-owned and need no
 * projection parsing (the archived projection parser lives in
 * experimental/python-repl-surfaces-v2-v3/).
 */
export function plainParagraphTexts(markup) {
  const texts = [];
  for (const block of String(markup).split('\n')) {
    const match = block.match(/^<(p|h[1-6])(?:\s[^>]*)?>([\s\S]*)<\/\1>$/);
    if (!match) continue;
    texts.push(
      match[2]
        .replace(/<[^>]+>/g, '')
        .replace(/&lt;/g, '<')
        .replace(/&gt;/g, '>')
        .replace(/&quot;/g, '"')
        .replace(/&#39;/g, "'")
        .replace(/&amp;/g, '&'),
    );
  }
  return texts;
}

const here = import.meta.dirname;
const ROOT = join(here, '..', '..', '..', '..');

/** The V1 typed-plan surface is the only experimental REPL surface. */
export const V1_EXTENSION = 'python-plan-extension.ts';
export const SUPPORTED_SURFACES = ['plan'];

export const WORKFLOW_HINT =
  ' Work only through the python tool: read the document first, author a typed Plan, ' +
  'then call docx_review(path, plan) to validate it and inspect review.edits, whose ' +
  'bounded edit neighborhoods show the changes in context and include a compact k1: ' +
  'commit key. In a separate later python call, commit with ' +
  'docx_commit(review.commit_key) — no path or plan. Stop when the requested outcome ' +
  'is achieved and verified.';

// ---------------------------------------------------------------------------
// Pure, offline-testable helpers
// ---------------------------------------------------------------------------

export function parseLine(line) {
  try {
    return JSON.parse(line);
  } catch {
    return { type: 'text', text: line };
  }
}

/** Validate the manifest shape; throws with a concrete message on violation. */
export function validateManifest(manifest) {
  if (!manifest || !Array.isArray(manifest.tasks) || manifest.tasks.length === 0) {
    throw new Error('manifest requires a non-empty tasks array');
  }
  const seen = new Set();
  for (const task of manifest.tasks) {
    for (const field of ['id', 'prompt', 'fixture', 'verify']) {
      if (typeof task[field] !== 'string' || task[field].trim() === '') {
        throw new Error(`task ${task.id ?? '<missing id>'} is missing string field ${field}`);
      }
    }
    if (!/^task\d+-[a-z-]+$/.test(task.id)) throw new Error(`task ${task.id} has an invalid id`);
    if (seen.has(task.id)) throw new Error(`duplicate task id ${task.id}`);
    seen.add(task.id);
    if (!['contract', 'complex'].includes(task.fixture)) throw new Error(`task ${task.id} fixture must be contract|complex`);
    if (!['commit', 'blocked'].includes(task.verify)) throw new Error(`task ${task.id} verify must be commit|blocked`);
  }
  const expected = ['task1-exact-text-correction', 'task2-repeated-bulk-edit', 'task3-ambiguous-occurrence', 'task4-arbitrary-formatting-selection', 'task5-paragraph-structure', 'task6-mixed-atomic-edit', 'task7-failed-preview-repair', 'task8-source-race', 'task9-mutation-after-review', 'task10-package-preservation'];
  for (const id of expected) {
    if (!seen.has(id)) throw new Error(`manifest is missing required task ${id}`);
  }
  return manifest;
}

export function resultText(result) {
  if (result === null || result === undefined) return '';
  if (typeof result === 'string') return result;
  if (typeof result.text === 'string') return result.text;
  if (Array.isArray(result.content)) {
    return result.content.map((c) => (typeof c === 'string' ? c : c && typeof c.text === 'string' ? c.text : '')).join('\n');
  }
  return JSON.stringify(result);
}

export function pairedToolCalls(events) {
  const open = new Map();
  const calls = [];
  for (const event of events) {
    if (event?.type === 'tool_execution_start') {
      open.set(event.toolCallId, { id: event.toolCallId, name: event.toolName, args: event.args });
    } else if (event?.type === 'tool_execution_end') {
      const call = open.get(event.toolCallId);
      if (call) calls.push({ ...call, result: event.result, isError: event.isError === true });
      open.delete(event.toolCallId);
    }
  }
  return { calls, unpaired: [...open.values()] };
}

// Host protocol markers — exact strings emitted by src/python-host.ts and
// src/python-plan-extension.ts (never the deterministic core preview key;
// the k1: commit key is the only commit credential under V1).
const REVIEW_OK = '── review ok ──';
const COMMIT_OK = '── commit ok ──';
const BLOCKED = '── blocked ──';
const COMMIT_KEY_RE = /k1:[0-9a-f]{32}/;
const REJECTED_COMMIT_RE =
  /source changed after review|path changed after review|commit key expired|commit key already consumed|unknown commit key|malformed commit key|commit requires a later python execution|candidate verification failed|commit infrastructure failure|commit cancelled|commit already in progress/;

/**
 * Fold the raw event stream into the combined review/commit metrics.
 * Pure and deterministic over a synthetic event list, so it is
 * unit-testable offline. `tamperApplied` records whether the task-8 harness
 * injection ran.
 */
export function collectMetrics(events, tamperApplied = false) {
  const { calls } = pairedToolCalls(events);
  let reviewAttempts = 0;
  let commits = 0;
  let blockedCount = 0;
  let invalidWriteAttempts = 0;
  let recoveries = 0;
  let pythonChars = 0;
  let firstReviewIndex = -1;
  let firstReviewPayloadIndex = -1;
  let firstCommitIndex = -1;
  let hasReviewPayload = false;
  const texts = calls.map((call) => resultText(call.result));
  for (let i = 0; i < calls.length; i += 1) {
    const text = texts[i];
    const args = calls[i].args;
    if (args && typeof args === 'object' && typeof args.code === 'string') pythonChars += args.code.length;
    const reviewPayload = text.includes(REVIEW_OK) && COMMIT_KEY_RE.test(text) && /(?:edits?|context|markup)/i.test(text);
    if (text.includes(REVIEW_OK)) {
      reviewAttempts += 1;
      if (firstReviewIndex === -1) firstReviewIndex = i;
    }
    if (reviewPayload) {
      hasReviewPayload = true;
      if (firstReviewPayloadIndex === -1) firstReviewPayloadIndex = i;
    }
    if (text.includes(COMMIT_OK)) {
      commits += 1;
      if (firstCommitIndex === -1) firstCommitIndex = i;
    }
    if (text.includes(BLOCKED)) blockedCount += 1;
    if (REJECTED_COMMIT_RE.test(text)) invalidWriteAttempts += 1;
    if (text.includes('session was reset')) recoveries += 1;
  }
  return {
    toolCalls: calls.length,
    reviewAttempts,
    commits,
    blockedCount,
    invalidWriteAttempts,
    recoveries,
    pythonChars,
    tamperApplied,
    explainableCommit: firstReviewPayloadIndex !== -1 && firstCommitIndex !== -1 && firstReviewPayloadIndex < firstCommitIndex,
    hasCompactCommitKey: hasReviewPayload,
    hasReviewPayload,
  };
}

function countOccurrences(haystack, needle) {
  return haystack.split(needle).length - 1;
}

/** Structural checks on the final rendered markup for verify=commit tasks. */
export function checkFinalDocument(taskId, markup, texts, extra) {
  const fail = (reason) => {
    throw new Error(`task ${taskId}: final document check failed: ${reason}`);
  };
  if (taskId === 'task1-exact-text-correction' || taskId === 'task7-failed-preview-repair' || taskId === 'task9-mutation-after-review') {
    if (!markup.includes('<b>sixty days')) fail('"sixty days" must be bold');
    if (markup.includes('thirty days')) fail('"thirty days" must be gone');
  } else if (taskId === 'task2-repeated-bulk-edit') {
    if (markup.includes('within 30 days')) fail('30-day term must be gone');
    if (markup.includes('within 60 days')) fail('60-day term must be gone');
    if (countOccurrences(markup, 'Payment is due within 45 days') < 3) fail('three 45-day paragraphs required');
  } else if (taskId === 'task3-ambiguous-occurrence') {
    if (countOccurrences(markup, 'THE TERMS') !== 1) fail('exactly one uppercased occurrence required');
    if (countOccurrences(markup, 'the terms') !== 3) fail('three lowercase occurrences must remain');
  } else if (taskId === 'task4-arbitrary-formatting-selection') {
    if (!markup.includes('<b><u>warranty period</u></b>')) fail('warranty period must be bold+underlined');
  } else if (taskId === 'task5-paragraph-structure') {
    if (!markup.includes('Replacement paragraph.')) fail('replacement paragraph text missing');
    if (!markup.includes('Inserted paragraph.')) fail('inserted paragraph missing');
    if (markup.includes('will be deleted')) fail('deleted paragraph text must be gone');
  } else if (taskId === 'task6-mixed-atomic-edit') {
    if (!markup.includes('<b>sixty')) fail('sixty days must be bold');
    if (!markup.includes('<b><u>warranty period</u></b>')) fail('warranty period must be bold+underlined');
    if (!markup.includes('Inserted paragraph.')) fail('inserted paragraph missing');
    if (markup.includes('will be deleted')) fail('deleted paragraph text must be gone');
  } else if (taskId === 'task10-package-preservation') {
    if (!texts.some((t) => t.endsWith(' [edited]'))) fail('some paragraph must end with " [edited]"');
    if (!extra || typeof extra.verifyParts !== 'function') fail('package-part verification is required');
    const parts = extra.verifyParts();
    for (const [name, hash] of parts.before) {
      if (name === 'word/document.xml') continue;
      if (parts.after.get(name) !== hash) {
        throw new Error(`task task10-package-preservation: part ${name} must be byte-identical`);
      }
    }
  }
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

async function runTrial(surface, task, rep, resultDir, model) {
  const cwd = await mkdtemp(join(tmpdir(), `docxdriver-repl-live-${task.id}-${surface}-`));
  const fixturePath = task.fixture === 'complex' ? await createComplexFixture(cwd) : await createContractFixture(cwd);
  const fixtureSha = sha256Hex(await readFile(fixturePath));
  // Task 10 needs the untouched package's part hashes BEFORE the edit runs.
  const beforeParts = task.id === 'task10-package-preservation' ? await zipPartHashes(fixturePath) : null;
  const extPath = join(ROOT, 'packages/docxdriver-pi/src', V1_EXTENSION);
  const prompt = `${task.prompt}${WORKFLOW_HINT}`;
  const args = [
    '--mode', 'json', '-p', '--no-session', '-na', '--model', model,
    '--no-extensions', '-e', extPath, '--no-skills', '--no-context-files',
    '--no-prompt-templates', '--no-builtin-tools', '--tools', 'python', prompt,
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
      // moment the review commit key appears, before the model's commit turn.
      // Realistic lead time (a full model turn); the verifier still catches
      // a lost race.
      if (task.id === 'task8-source-race' && event?.type === 'tool_execution_end') {
        const result = resultText(event.result);
        if (result.includes(REVIEW_OK) && COMMIT_KEY_RE.test(result)) tamperSettled = tamperFile();
      }
    }
  });
  child.stderr.on('data', (chunk) => stderr.push(chunk));
  const started = Date.now();
  const timeout = setTimeout(() => {
    timedOut = true;
    child.kill('SIGTERM');
  }, 10 * 60 * 1000);
  const status = await new Promise((resolve) => child.on('close', (code, signal) => resolve({ code, signal })));
  clearTimeout(timeout);
  await tamperSettled;
  if (buffer) lines.push(parseLine(buffer));

  const metrics = collectMetrics(lines, task.id === 'task8-source-race' ? tampered : false);
  const record = {
    surface,
    task: task.id,
    rep,
    model,
    prompt_version: 2,
    status: { code: status.code, signal: status.signal },
    timed_out: timedOut,
    turn_capped: turnCapped,
    turns,
    wall_ms: Date.now() - started,
    metrics,
    fixtureSha,
  };
  // Task 10: persist the package part-hash evidence (pre-edit vs post-commit)
  // so the parts-unchanged claim is reproducible from the recorded run alone.
  if (task.id === 'task10-package-preservation') {
    record.partHashes = {
      before: Object.fromEntries(beforeParts ?? new Map()),
      after: null, // filled after verification
    };
  }

  // Verify the final state. Success fails closed: commit tasks require the
  // exact final edit verified, exactly one compact key consumed, and review →
  // later commit ordering; blocked tasks require a rejected commit and
  // preserved (or tamper-preserved) bytes.
  // Turn-cap/timeout termination is never success (see trialSuccess).
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
      if (metrics.commits !== 1) throw new Error(`task ${task.id}: expected exactly one consumed commit key, saw ${metrics.commits}`);
      if (metrics.reviewAttempts !== 1) throw new Error(`task ${task.id}: expected exactly one successful review, saw ${metrics.reviewAttempts}`);
      if (!metrics.hasCompactCommitKey) throw new Error(`task ${task.id}: no compact k1: commit key observed in a successful review`);
      if (!metrics.hasReviewPayload) throw new Error(`task ${task.id}: successful review did not expose edit/context evidence`);
      if (!metrics.explainableCommit) throw new Error(`task ${task.id}: no review payload → later commit ordering observed`);
      record.verification = { outcome: 'ok', finalMarkup };
    } else {
      const finalBytes = await readFile(fixturePath);
      const finalSha = sha256Hex(finalBytes);
      if (task.id === 'task8-source-race') {
        if (!metrics.invalidWriteAttempts) throw new Error('task8: no blocked commit observed');
        if (!record.metrics.tamperApplied) throw new Error('task8: harness tamper did not run');
        if (metrics.commits > 0) throw new Error('task8: a commit succeeded despite the source race');
        if (finalSha === fixtureSha) throw new Error('task8: file bytes were not preserved as tampered');
      } else {
        throw new Error(`task ${task.id}: unsupported blocked-task id`);
      }
      record.verification = { outcome: 'ok', finalSha };
    }
    record.verification.ok = true;
  } catch (error) {
    record.verification = { outcome: 'error', message: error instanceof Error ? error.message : String(error) };
    record.infrastructure_error = record.verification.message;
  }
  if (status.code !== 0 || status.signal) {
    record.infrastructure_error ??= `pi exited unsuccessfully: code=${status.code}, signal=${status.signal}`;
  }

  const taskDir = join(resultDir, task.id, `${surface}-${rep}`);
  await mkdir(taskDir, { recursive: true });
  await writeFile(join(taskDir, 'events.jsonl'), lines.map((event) => JSON.stringify(event)).join('\n') + '\n');
  await writeFile(join(taskDir, 'stdout.log'), Buffer.concat(stdout));
  await writeFile(join(taskDir, 'stderr.log'), Buffer.concat(stderr));
  await writeFile(join(taskDir, 'run.json'), JSON.stringify(record, null, 2));
  return record;
}

/** A trial succeeds only when its final-state verification passed AND the
 * run was not terminated by the harness turn cap or timeout: turn-cap
 * termination alone never counts as success. A capped/timeout run that also
 * failed verification is a failure either way. */
export function trialSuccess(record) {
  return record.verification?.outcome === 'ok' && !record.turn_capped && !record.timed_out;
}

/** Normalized final-document fingerprint (single-surface equality helper). */
export function finalFingerprint(finalMarkup) {
  try {
    return plainParagraphTexts(finalMarkup).join('\u0000');
  } catch {
    return null;
  }
}

async function main() {
  if (process.env.PI_E2E_LIVE !== '1') throw new Error('live REPL trials require PI_E2E_LIVE=1');
  const model = process.env.PI_E2E_MODEL;
  if (!model) throw new Error('live REPL trials require explicit PI_E2E_MODEL');
  const manifestPath = join(here, 'repl-tasks.json');
  const manifest = validateManifest(JSON.parse(await readFile(manifestPath, 'utf8')));
  const surfaces = (process.env.PI_E2E_SURFACES ?? 'plan').split(',').map((s) => s.trim()).filter(Boolean);
  for (const surface of surfaces) {
    if (!SUPPORTED_SURFACES.includes(surface)) throw new Error(`unknown surface ${surface} (V1 only: ${SUPPORTED_SURFACES.join('|')})`);
  }
  const taskFilter = (process.env.PI_E2E_TASKS ?? '').split(',').map((s) => s.trim()).filter(Boolean);
  const reps = Number.parseInt(process.env.PI_E2E_REPS ?? '1', 10);
  if (!Number.isInteger(reps) || reps < 1) throw new Error('PI_E2E_REPS must be a positive integer');
  const tasks = manifest.tasks.filter((task) => taskFilter.length === 0 || taskFilter.includes(task.id));
  if (tasks.length === 0) throw new Error('PI_E2E_TASKS matched no manifest tasks');
  const runId = `${new Date().toISOString().replaceAll(':', '-')}-${model.replaceAll('/', '_')}`;
  const resultDir = join(here, 'results', runId);
  await mkdir(resultDir, { recursive: true });
  console.log(`repl live trials: model=${model} surfaces=${surfaces.join(',')} tasks=${tasks.map((t) => t.id).join(',')} reps=${reps} -> ${resultDir}`);

  const cells = [];
  for (const task of tasks) {
    for (const surface of surfaces) {
      for (let rep = 1; rep <= reps; rep += 1) {
        const record = await runTrial(surface, task, rep, resultDir, model);
        cells.push(record);
        const ok = trialSuccess(record);
        console.log(`${task.id}/${surface}/${rep}: ${ok ? 'ok' : 'FAIL'} calls=${record.metrics.toolCalls} reviews=${record.metrics.reviewAttempts} commits=${record.metrics.commits} blocked=${record.metrics.blockedCount} wallMs=${record.wall_ms}${record.infrastructure_error ? ` -- ${record.infrastructure_error}` : ''}`);
      }
    }
  }

  // Per-task success on the V1 surface (any rep succeeded).
  const byTask = new Map();
  for (const cell of cells) {
    if (!byTask.has(cell.task)) byTask.set(cell.task, []);
    byTask.get(cell.task).push(cell);
  }
  const taskSummaries = [];
  for (const task of tasks) {
    const taskCells = byTask.get(task.id) ?? [];
    const success = taskCells.some((c) => trialSuccess(c));
    taskSummaries.push({ task: task.id, verify: task.verify, success });
    console.log(`task ${task.id}: ${success ? 'ok' : 'FAIL'}`);
  }

  const summary = {
    generated: new Date().toISOString(),
    runId,
    model,
    surfaces,
    reps,
    promptVersion: 2,
    cells: cells.map((c) => ({
      surface: c.surface,
      task: c.task,
      rep: c.rep,
      success: trialSuccess(c),
      toolCalls: c.metrics.toolCalls,
      reviewAttempts: c.metrics.reviewAttempts,
      hasCompactCommitKey: c.metrics.hasCompactCommitKey,
      commits: c.metrics.commits,
      blockedCount: c.metrics.blockedCount,
      invalidWriteAttempts: c.metrics.invalidWriteAttempts,
      recoveries: c.metrics.recoveries,
      pythonChars: c.metrics.pythonChars,
      wallMs: c.wall_ms,
      explainableCommit: c.metrics.explainableCommit,
      error: c.infrastructure_error ?? null,
    })),
    tasks: taskSummaries,
  };
  await writeFile(join(resultDir, 'summary.json'), JSON.stringify(summary, null, 2));
  console.log(`wrote ${join(resultDir, 'summary.json')}`);
  // Fail closed: any trial that did not strictly succeed fails the harness.
  if (cells.some((c) => !trialSuccess(c))) process.exitCode = 1;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.stack ?? error.message : String(error));
    process.exitCode = 1;
  });
}
