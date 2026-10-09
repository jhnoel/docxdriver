import { afterEach, test } from 'node:test';
import assert from 'node:assert/strict';
import { Monty } from '@pydantic/monty';
import { MontyUnrecoverableError, PythonRepl } from '../dist/python-repl.js';

/**
 * Tiny task-local prelude/stubs: Task 1 owns only the runtime, so the tests
 * exercise the vocabulary they define here (`Op` dataclass + `host_fn`
 * wrapper around the private `_host_fn` callback) instead of the full
 * per-surface preludes that land in later tasks. Every behavior below
 * mirrors the MVP runtime tests: persistent state, dataclass attribute
 * repair, plain-value host callbacks, error resilience, crash recovery,
 * async callback awaiting, and idempotent close.
 *
 * Monty's type checker resolves user-code names ONLY against
 * `typeCheckStubs` (never the prelude) and cannot resolve underscore-
 * prefixed names, so the prelude exposes a real `host_fn` wrapper around the
 * private `_host_fn` callback (the MVP prelude pattern), and both `Op` and
 * `Op2` are stub-declared so the per-instance-prelude test type-checks on
 * either instance while the runtime failure still proves prelude isolation.
 */
const PRELUDE = [
  'from dataclasses import dataclass',
  '',
  '@dataclass',
  'class Op:',
  '    find: str = ""',
  '    def to_dict(self):',
  '        return {"op": "x", "find": self.find}',
  '',
  'def host_fn(path: str, data=None):',
  '    return _host_fn(path, data)',
  '',
].join('\n');

const STUBS = [
  'class Op:',
  '    def __init__(self, find: str = ""): ...',
  '    find: str',
  '    def to_dict(self) -> dict: ...',
  '',
  'class Op2:',
  '    def __init__(self, find: str = ""): ...',
  '    find: str',
  '    def to_dict(self) -> dict: ...',
  '',
  'def host_fn(path: str, data: dict | None = None) -> None: ...',
  'def _host_fn(path: str, data: dict | None = None) -> None: ...',
  '',
].join('\n');

const open = new Set();
afterEach(async () => {
  await Promise.all([...open].map((repl) => repl.close()));
  open.clear();
});

async function repl() {
  const value = await PythonRepl.create(PRELUDE, STUBS);
  open.add(value);
  return value;
}

test('state and dataclass attributes persist across feeds', async () => {
  const runtime = await repl();
  const first = await runtime.execute('find = "old"\nop = Op(find=find)', {});
  assert.equal(first.ok, true, first.error);
  const second = await runtime.execute('op.find = "corrected"\nprint(op.find)', {});
  assert.equal(second.ok, true, second.error);
  assert.equal(second.stdout, 'corrected\n');
});

test('host callbacks receive plain values and their None return reaches Python', async () => {
  const runtime = await repl();
  const toPlain = (value) => value instanceof Map
    ? Object.fromEntries([...value].map(([key, item]) => [key, toPlain(item)]))
    : Array.isArray(value) ? value.map(toPlain) : value;
  let received;
  const externalLookup = {
    _host_fn: (...args) => { received = args.map(toPlain); return null; },
  };
  const result = await runtime.execute(
    [
      'op = Op(find="old")',
      'returned = host_fn("demo.docx", op.to_dict())',
      'print(returned is None)',
    ].join('\n'),
    externalLookup,
  );
  assert.equal(result.ok, true, result.error);
  assert.equal(result.stdout, 'True\n');
  assert.deepEqual(received, ['demo.docx', { op: 'x', find: 'old' }]);
});

test('runtime errors do not erase healthy session state', async () => {
  const runtime = await repl();
  assert.equal((await runtime.execute('x = 41', {})).ok, true);
  const failed = await runtime.execute('1 / 0', {});
  assert.equal(failed.ok, false);
  assert.match(failed.error, /ZeroDivisionError/);
  const recovered = await runtime.execute('x + 1', {});
  assert.equal(recovered.ok, true, recovered.error);
  assert.equal(recovered.value, 42);
});

test('crashed worker is replaced with reset: true and the prelude reloaded', async () => {
  const runtime = await repl();
  // Pre-crash session state that a `reset` must wipe.
  assert.equal((await runtime.execute('x = 41', {})).ok, true);

  const pid = runtime.workerPid;
  assert.ok(Number.isInteger(pid) && pid > 0, 'workerPid must be visible between feeds');
  process.kill(pid, 'SIGKILL');

  // The crashed worker surfaces as a failed execute with reset: true...
  const crashed = await runtime.execute('1 + 1', {});
  assert.equal(crashed.ok, false);
  assert.equal(crashed.reset, true);
  assert.match(crashed.error, /crashed/);

  // ...the runtime replaces the worker and reloads the prelude, so prelude
  // names resolve again while pre-crash state is gone.
  const reloaded = await runtime.execute('print(Op(find="old").find)', {});
  assert.equal(reloaded.ok, true, reloaded.error);
  assert.equal(reloaded.stdout, 'old\n');
  const stateGone = await runtime.execute('print(x)', {});
  assert.equal(stateGone.ok, false);
  // Type checking (session has typeCheck: true) flags the stale name as
  // unresolved before execution — proof the pre-crash global was wiped.
  assert.match(stateGone.error, /unresolved-reference/);
  assert.match(stateGone.error, /`x`/);
});

test('async external callbacks are fully awaited before execute resolves', async () => {
  const runtime = await repl();
  let settled = false;
  const externalLookup = {
    _host_fn: async () => {
      await new Promise((resolve) => setTimeout(resolve, 25));
      settled = true;
      return '<p>Hello async</p>';
    },
  };
  const result = await runtime.execute('print(host_fn("demo.docx"))', externalLookup);
  assert.equal(result.ok, true, result.error);
  assert.equal(settled, true, 'the async callback must settle before execute resolves');
  assert.equal(result.stdout, '<p>Hello async</p>\n');
});

test('async external callback rejection becomes a Python error and the session survives', async () => {
  const runtime = await repl();
  const externalLookup = {
    _host_fn: async () => {
      throw new Error('path escapes cwd: ../outside.docx');
    },
  };
  const result = await runtime.execute('print(host_fn("demo.docx"))', externalLookup);
  assert.equal(result.ok, false, 'a rejected host callback must fail the feed');
  assert.equal(result.reset, false);
  assert.match(result.error, /path escapes cwd: \.\.\/outside\.docx/);
  const after = await runtime.execute('print(1 + 1)', {});
  assert.equal(after.ok, true, after.error);
  assert.equal(after.stdout, '2\n');
});

test('close() may be called twice without throwing', async () => {
  const runtime = await repl();
  await runtime.close();
  await runtime.close();
});

test('executionNumber counts every feed including failed ones', async () => {
  const runtime = await repl();
  assert.equal(runtime.executionNumber, 0);
  await runtime.execute('x = 1', {});
  await runtime.execute('1 / 0', {});          // failure still counts
  await runtime.execute('x + 1', {});
  assert.equal(runtime.executionNumber, 3);
});

test('each runtime instance loads its own prelude', async () => {
  const a = await PythonRepl.create(PRELUDE, STUBS);
  const b = await PythonRepl.create(PRELUDE.replace('class Op', 'class Op2'), STUBS);
  open.add(a); open.add(b);
  // Monty's checker resolves a limited builtin set (hasattr is not one of
  // them), so prove a's prelude loaded a working `Op` through its own
  // method instead.
  const probe = await a.execute('print(Op().to_dict()["op"])', {});
  assert.equal(probe.ok, true, probe.error);
  assert.equal(probe.stdout, 'x\n');
  // `Op` type-checks (it is stub-declared) but is undefined in b's prelude,
  // so the failure is a runtime NameError — proof b loaded its own prelude.
  const missing = await b.execute('Op', {});
  assert.equal(missing.ok, false);
  assert.match(missing.error, /not defined/);
  assert.equal((await b.execute('Op2', {})).ok, true);
});

// ---------------------------------------------------------------------------
// Task 6 required regressions: cooperative cancellation and recovery hardening
// ---------------------------------------------------------------------------

test('abort at entry answers without consuming an execution number or doing host work', async () => {
  const runtime = await repl();
  assert.equal(runtime.executionNumber, 0);
  const controller = new AbortController();
  controller.abort();
  let called = false;
  const result = await runtime.execute('x = 1', { _host_fn: () => { called = true; } }, controller.signal);
  assert.equal(result.ok, false);
  assert.equal(result.error, 'aborted');
  assert.equal(result.aborted, true);
  assert.equal(result.reset, false);
  assert.equal(called, false, 'no host callback may run for an aborted-before-feed call');
  assert.equal(runtime.executionNumber, 0, 'an aborted-before-feed call must not consume an execution number');
  // The session is untouched: a normal feed works and counts as execution 1.
  const after = await runtime.execute('x = 41', {});
  assert.equal(after.ok, true, after.error);
  assert.equal(runtime.executionNumber, 1);
});

test('abort during a suspended host callback unwinds via resumeError and the session stays healthy', async () => {
  const runtime = await repl();
  const controller = new AbortController();
  let markEntered;
  const entered = new Promise((resolve) => { markEntered = resolve; });
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const externalLookup = {
    _host_fn: async () => {
      markEntered();
      await gate;             // suspended until the test fires the abort
      return 'late value';    // settles after abort: the value must be discarded
    },
  };
  const resultP = runtime.execute('print(host_fn("demo.docx"))\nprint("done")', externalLookup, controller.signal);
  // Wait until the feed is suspended on the host callback, then cancel.
  await entered;
  controller.abort();
  release();
  const result = await resultP;
  assert.equal(result.ok, false, 'the aborted feed must fail');
  assert.match(result.error, /aborted/);
  assert.equal(result.aborted, true);
  assert.equal(result.reset, false);
  assert.ok(!result.stdout.includes('done'), 'the feed unwound before the trailing print');
  // The session is healthy: prelude names resolve and later feeds work.
  const after = await runtime.execute('print(Op(find="old").find)', {});
  assert.equal(after.ok, true, after.error);
  assert.equal(after.stdout, 'old\n');
  const again = await runtime.execute('print(1 + 1)', {});
  assert.equal(again.ok, true, again.error);
  assert.equal(again.stdout, '2\n');
});

test('print overflow fails the feed with a clear cap error and the session stays healthy', async () => {
  // L4: CollectString throws when the print cap is exceeded; the feed fails
  // with a clear diagnostic and later feeds keep working (trailing output
  // is not silently dropped).
  const runtime = await PythonRepl.create(PRELUDE, STUBS, { printCapBytes: 10 * 1024 });
  open.add(runtime);
  const overflow = await runtime.execute("print('x' * 20000)\nprint('trailing')", {});
  assert.equal(overflow.ok, false, 'the overflowing feed must fail, not silently drop output');
  assert.ok(!overflow.stdout.includes('trailing'), 'output after the overflow must not appear');
  const after = await runtime.execute('print(1 + 1)', {});
  assert.equal(after.ok, true, after.error);
  assert.equal(after.stdout, '2\n');
});

test('abort while queued behind an in-flight feed resolves with the aborted result, never a rejection', async () => {
  const runtime = await repl();
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  let markEntered;
  const entered = new Promise((resolve) => { markEntered = resolve; });
  const externalLookup = {
    _host_fn: async () => {
      markEntered();
      await gate; // in-flight feed suspended on the host callback
      return 'ok';
    },
  };
  const first = runtime.execute('print(host_fn("demo.docx"))\nprint("first-done")', externalLookup);
  await entered;
  // The second call queues on the tail promise behind the in-flight feed.
  const controller = new AbortController();
  const queued = runtime.execute('print(1)', {}, controller.signal);
  controller.abort();
  release();
  // The queued call must RESOLVE with the normal aborted result.
  const result = await queued;
  assert.equal(result.ok, false);
  assert.equal(result.error, 'aborted');
  assert.equal(result.aborted, true);
  assert.equal(result.reset, false);
  assert.equal(runtime.executionNumber, 1, 'the queued-aborted call must not consume an execution number');
  // The in-flight feed completes normally and the session stays healthy.
  const firstResult = await first;
  assert.equal(firstResult.ok, true, firstResult.error);
  assert.match(firstResult.stdout, /first-done/);
  const after = await runtime.execute('print(Op(find="old").find)', {});
  assert.equal(after.ok, true, after.error);
  assert.equal(after.stdout, 'old\n');
});

test('recovery failure on a closed pool discards the runtime: MontyUnrecoverableError and safe close', async () => {
  const pool = await Monty.create({ minProcesses: 1, maxProcesses: 1, requestTimeout: 15 });
  const runtime = await PythonRepl.create(PRELUDE, STUBS, { pool });
  open.add(runtime);
  assert.equal((await runtime.execute('x = 1', {})).ok, true);
  // Close the pool: the checked-out session keeps working, but a crash makes
  // the replacement checkout fail — the recovery path must discard the
  // runtime instead of retaining a broken instance.
  await pool.close();
  process.kill(runtime.workerPid, 'SIGKILL');
  await assert.rejects(() => runtime.execute('1 + 1', {}), MontyUnrecoverableError);
  // Broken: every further execute rejects immediately with the same error.
  await assert.rejects(() => runtime.execute('1 + 1', {}), MontyUnrecoverableError);
  // close() on the broken instance is still safe.
  await runtime.close();
  await runtime.close();
  // A fresh instance (own pool) works normally.
  const fresh = await PythonRepl.create(PRELUDE, STUBS);
  open.add(fresh);
  const result = await fresh.execute('print(Op(find="fresh").find)', {});
  assert.equal(result.ok, true, result.error);
  assert.equal(result.stdout, 'fresh\n');
});

test('recovery failure on prelude reload discards the runtime: MontyUnrecoverableError and safe close', async () => {
  const runtime = await PythonRepl.create(PRELUDE, STUBS, {
    reloadPrelude: async () => { throw new Error('injected prelude reload failure'); },
  });
  open.add(runtime);
  assert.equal((await runtime.execute('x = 1', {})).ok, true);
  process.kill(runtime.workerPid, 'SIGKILL');
  await assert.rejects(() => runtime.execute('1 + 1', {}), MontyUnrecoverableError);
  // The half-initialized replacement session was discarded: no further use.
  await assert.rejects(() => runtime.execute('1 + 1', {}), MontyUnrecoverableError);
  await runtime.close();
});

test('document style catalogs are inspectable but omitted from ReadResult display', async () => {
  const { PYTHON_PRELUDE, PYTHON_TYPE_STUBS } = await import('../dist/python-plan-prelude.js');
  const runtime = await PythonRepl.create(PYTHON_PRELUDE, PYTHON_TYPE_STUBS);
  open.add(runtime);
  const result = await runtime.execute('r = ReadResult(markup="<p>Body</p>", styles={"Normal":"font-size:12pt"}, css={"dx1":"color:red"})\nprint(r)\nprint(r.styles["Normal"])\nprint(r.css["dx1"])', {});
  assert.equal(result.ok, true);
  const lines = result.stdout.trim().split('\n');
  assert.match(lines[0], /styles=1 definitions, css=1 rules/);
  assert.doesNotMatch(lines[0], /font-size|color:red/);
  assert.equal(lines[1], 'font-size:12pt');
  assert.equal(lines[2], 'color:red');
});
