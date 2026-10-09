#!/usr/bin/env node
// Opt-in real-Pi behavioral harness. Each task gets a fresh cwd/process, only
// the extension skill and five DOCX tools, and a bounded event stream.
import { mkdir, readFile, stat, writeFile } from 'node:fs/promises';
import { mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { ensureInit, runRequest } from '../../dist/engine.js';

if (process.env.PI_E2E_LIVE !== '1') throw new Error('live Pi e2e requires PI_E2E_LIVE=1');
if (!process.env.PI_E2E_MODEL) throw new Error('live Pi e2e requires explicit PI_E2E_MODEL');
const here = import.meta.dirname;
const root = join(here, '..', '..', '..', '..');
const manifest = JSON.parse(await readFile(join(here, 'manifest.json'), 'utf8'));
const allowedTools = ['docx_create', 'docx_read', 'docx_find', 'docx_edit', 'docx_help'];
if (!Array.isArray(manifest.fixtures) || !manifest.fixtures.length) throw new Error('fixture manifest has no available fixtures');
for (const entry of manifest.fixtures) {
  for (const field of ['application_number', 'document_code', 'source_url', 'retrieved', 'asset', 'sha256', 'smoke']) if (!entry[field]) throw new Error(`fixture manifest missing ${field}`);
  if (!/^https:\/\//.test(entry.source_url) || !/^[0-9a-f]{64}$/.test(entry.sha256)) throw new Error(`fixture manifest has invalid source/hash for ${entry.id}`);
  if (entry.smoke.kind !== 'document' || !Number.isInteger(entry.smoke.min_blocks) || entry.smoke.min_blocks < 1) throw new Error(`fixture manifest has invalid smoke metadata for ${entry.id}`);
}
const fixture = manifest.fixtures[0];
const sourceAsset = join(root, fixture.asset);
const sourceBytes = await readFile(sourceAsset).catch((error) => {
  throw new Error(`fixture unavailable for ${fixture.id}: ${sourceAsset} (${error.code ?? error.message})`);
});
const actualHash = createHash('sha256').update(sourceBytes).digest('hex');
if (actualHash !== fixture.sha256) throw new Error(`fixture hash mismatch for ${fixture.id}: ${actualHash}`);
if ((await stat(sourceAsset)).size === 0) throw new Error(`fixture is empty: ${sourceAsset}`);

async function ensureCore() {
  await ensureInit();
}
async function generatedLongDocument() {
  await ensureCore();
  const paragraphs = Array.from({ length: 360 }, (_, index) => `Generated long-document paragraph ${index + 1}: ${'content '.repeat(24)}`);
  const output = runRequest(undefined, { Command: { command: { kind: 'create', paragraphs } } });
  if (!output.bytes || !(output.bytes instanceof Uint8Array)) throw new Error('core could not generate long-document fixture');
  return output.bytes;
}

const tasks = [
  { id: 'inline-edit', prompt: 'Copy the fixture to work.docx. Read it, make one inline JSON replace_text plan, preview it, then commit with the returned preview key. Verify the final file.', verify: 'commit' },
  { id: 'alias-plan', prompt: 'Copy the fixture to work.docx. Read it, preview a multi-operation plan with insert_paragraph defining an alias and a later operation using that alias, then commit and verify.', verify: 'commit' },
  { id: 'toml-repair', prompt: 'Copy the fixture to work.docx. Use only the normal write/edit tools to author and repair a TOML plan file (no shell). Start with one intentional validation error, use the structured diagnostic to repair it, then preview and commit the corrected TOML plan.', verify: 'commit', normalTools: ['write', 'edit'] },
  { id: 'stale-recovery', prompt: 'Copy the fixture to work.docx. Demonstrate recovery from a stale preview key or source mismatch: observe the structured rejection, re-read, preview the current plan, and commit only after a successful preview.', verify: 'recovery' },
  { id: 'long-document-edit', prompt: 'A generated long DOCX is already at work.docx. Use docx_read and/or docx_find to locate content near the end, then preview and commit a safe plan editing that content.', verify: 'commit' },
];

const runId = `${new Date().toISOString().replaceAll(':', '-')}-${process.env.PI_E2E_MODEL.replaceAll('/', '_')}`;
const resultDir = join(here, 'results', runId); await mkdir(resultDir, { recursive: true });

function parseLine(line) { try { return JSON.parse(line); } catch { return { type: 'text', text: line }; } }
function isAssistantMessage(event) { return event?.type?.includes('message_end') && event?.message?.role === 'assistant'; }
function pairedToolCalls(events) {
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
function resultDetails(call) { return call.result?.details ?? {}; }
function verifyTask(task, events, cwd, finalInspection) {
  const paired = pairedToolCalls(events);
  if (paired.unpaired.length) throw new Error(`task ${task.id} has unpaired tool calls: ${paired.unpaired.map((x) => x.id).join(', ')}`);
  const calls = paired.calls;
  const names = calls.map((call) => call.name);
  const expectedTools = new Set([...allowedTools, ...(task.normalTools ?? [])]);
  const unknown = names.filter((name) => !expectedTools.has(name));
  if (unknown.length) throw new Error(`task ${task.id} used disallowed tools: ${unknown.join(', ')}`);
  if (!names.includes('docx_read') || !names.includes('docx_edit')) throw new Error(`task ${task.id} did not exercise read/edit`);
  const editCalls = calls.filter((call) => call.name === 'docx_edit');
  const previews = editCalls.filter((call) => !call.args?.preview_key && resultDetails(call).outcome === 'previewed');
  const commits = editCalls.filter((call) => call.args?.preview_key && resultDetails(call).outcome === 'committed');
  for (const commit of commits) {
    if (!previews.some((preview) => resultDetails(preview).preview_key === commit.args.preview_key)) throw new Error(`task ${task.id} committed with an unmatched preview key`);
  }
  if (task.verify === 'commit' && (!previews.length || !commits.length)) throw new Error(`task ${task.id} lacked successful preview->commit sequencing`);
  const rejections = calls.filter((call) => resultDetails(call).outcome === 'rejected' || call.isError);
  if (task.verify === 'recovery' && !rejections.length) throw new Error(`task ${task.id} lacked a structured recovery rejection`);
  return {
    tool_calls: calls.length,
    previews: previews.length,
    commits: commits.length,
    rejections: rejections.map((call) => ({ name: call.name, diagnostic: resultDetails(call).diagnostic })),
    errors: calls.filter((call) => call.isError).map((call) => ({ name: call.name, result: call.result })),
    files: names.includes('docx_edit') ? ['work.docx'] : [],
    final_core_read: finalInspection,
  };
}

for (const task of tasks) {
  const cwd = await mkdtemp(join(tmpdir(), `docxdriver-pi-live-${task.id}-`));
  const taskBytes = task.id === 'long-document-edit' ? await generatedLongDocument() : sourceBytes;
  await writeFile(join(cwd, 'work.docx'), taskBytes);
  const normalTools = task.normalTools ?? [];
  const enabledTools = [...allowedTools, ...normalTools];
  const prompt = `${task.prompt}\nFixture: ${fixture.id}. The only DOCX extension tools available are ${allowedTools.join(', ')}.${normalTools.length ? ` The only normal file tools available are ${normalTools.join(', ')}; use them only for the TOML plan.` : ' No normal file tools are enabled.'} Do not use shell or any other extension. Stop after the task is verified.`;
  const args = ['--offline', '--no-session', '--no-context-files', '--no-builtin-tools', '--tools', enabledTools.join(','), '--extension', join(root, 'packages/docxdriver-pi/src/index.ts'), '--skill', join(root, 'packages/docxdriver-pi/skills/docx-pi'), '--model', process.env.PI_E2E_MODEL, '--mode', 'json', '--print', prompt];
  const child = spawn(process.env.PI_E2E_COMMAND ?? 'pi', args, { cwd, env: { ...process.env }, stdio: ['ignore', 'pipe', 'pipe'] });
  const lines = []; const stdout = []; const stderr = []; let turns = 0; let timedOut = false; let turnCapped = false; let buffer = '';
  child.stdout.on('data', (chunk) => {
    stdout.push(chunk);
    buffer += chunk.toString(); const rows = buffer.split('\n'); buffer = rows.pop() ?? '';
    for (const row of rows) { if (!row) continue; lines.push(parseLine(row)); if (isAssistantMessage(lines.at(-1))) turns++; if (turns >= 12 && !turnCapped) { turnCapped = true; child.kill('SIGTERM'); } }
  });
  child.stderr.on('data', (chunk) => stderr.push(chunk));
  const started = Date.now(); const timeout = setTimeout(() => { timedOut = true; child.kill('SIGTERM'); }, 5 * 60 * 1000);
  const status = await new Promise((resolve) => child.on('close', (code, signal) => resolve({ code, signal })));
  clearTimeout(timeout); if (buffer) lines.push(parseLine(buffer));
  const record = { task: task.id, model: process.env.PI_E2E_MODEL, prompt_version: 3, cwd, status, timed_out: timedOut, turn_capped: turnCapped, turns, wall_ms: Date.now() - started };
  let finalInspection;
  try {
    await ensureCore();
    const finalBytes = new Uint8Array(await readFile(join(cwd, 'work.docx')));
    const output = runRequest(finalBytes, { Command: { command: { kind: 'read', view: 'markup' } } });
    if (output.outcome !== 'completed') throw new Error(`final DOCX typed read failed: ${JSON.stringify(output)}`);
    finalInspection = { outcome: 'completed', result: output.result };
  } catch (error) { record.infrastructure_error = error instanceof Error ? error.message : String(error); }
  try {
    const verification = verifyTask(task, lines, cwd, finalInspection); record.verification = verification;
  } catch (error) { record.infrastructure_error = error instanceof Error ? error.message : String(error); }
  if (status.code !== 0 || status.signal) record.infrastructure_error ??= `Pi exited unsuccessfully: code=${status.code}, signal=${status.signal}`;
  const taskDir = join(resultDir, task.id); await mkdir(taskDir, { recursive: true });
  await writeFile(join(taskDir, 'events.jsonl'), lines.map((event) => JSON.stringify(event)).join('\n') + '\n');
  await writeFile(join(taskDir, 'stdout.log'), Buffer.concat(stdout));
  await writeFile(join(taskDir, 'stderr.log'), Buffer.concat(stderr)); await writeFile(join(taskDir, 'run.json'), JSON.stringify(record, null, 2));
  if (record.infrastructure_error || timedOut || turnCapped) process.exitCode = 1;
}
