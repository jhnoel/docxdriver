# Three Python REPL Surfaces over One Host and Commit Protocol — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build three experimental persistent-Python REPL surfaces (`python-plan-extension.ts`, `python-string-extension.ts`, `python-model-extension.ts`) that share one Monty runtime, one docxdriver host adapter, and one cross-execution commit gate, and all compile to the same canonical core typed `Plan` so authoring models can be compared.

**Architecture:** A shared extension factory (`python-extension-factory.ts`) owns the lazy persistent `PythonRepl` (@pydantic/monty), the preview store, truncation, and `session_shutdown`. A shared host adapter (`python-host.ts`) exposes five private `_docx_*` callbacks and enforces the commit gate: a preview stores `{path, source hash, canonical plan, core preview key, surface-state hash, REPL execution number}`; a commit is allowed only in a later Python execution, only when the current file source hash and the recomputed surface-state hash (plan/draft/model) still match, and only through core's own `p1:sha256:` preview-key check inside `runPlan`. Each entry point installs a different Python prelude plus a surface deriver that turns Python state into typed core `EditOp`s: Version 1 authors `Plan` dataclasses directly; Version 2 diffs original vs. proposed projection strings through a shared projection parser and reconciler; Version 3 replays mutations of a Python-native document model. Post-commit render verification is per-surface but driven by one shared evidence flow.

**Tech Stack:** TypeScript ESM (NodeNext), Pi ExtensionAPI, TypeBox, `@pydantic/monty` 0.0.21, `docxdriver` WASM (`runPlan`/`executeRequest`), Node test runner, SHA-256, atomic filesystem rename, node:zlib for fixture ZIP checks.

## Global Constraints

- Work only in a new worktree from `main`: `.worktrees/python-repl-surfaces` on branch `feat/python-repl-surfaces` (create via superpowers:using-git-worktrees before Task 1). `main` already carries the typed core `Plan` API (`89e654f`); the old `docxdriver-repl` branch's legacy `DocxCommand` engine and `mvp1:` keys are **not** ported.
- Use test-driven development: add each behavior test, run it and observe the expected failure, then write production code. Commit at the end of every task.
- Every implementation and review subagent uses model `opencode-go/deepseek-v4-flash` explicitly (repo convention from the MVP plan).
- Pin runtime dependency `@pydantic/monty` to exact version `0.0.21`; require Node `>=20`. Add to `packages/docxdriver-pi` dependencies (not devDependencies).
- The experiment operation subset is exactly these six core `EditOp` variants:
  `replace_text {at, select, with, occurrence?}`, `replace_paragraph {at, with}`,
  `format_text {at, select, occurrence?, bold?, italic?, underline?, strike?, superscript?, subscript?}`,
  `format_paragraph {at, style?}`, `insert_paragraph {at, position: before|after, with?, style?, as?}`,
  `delete_paragraphs {at: string[]}`. `color`, `font_size`, `clear`, alignment, and margins are **out of subset** because the markup projection cannot represent them (verified: no color/font-size in `html/parse.rs`/`render.rs`). Deviation note: spec Task 4's "recolor" is executed as bold+underline in the corpus because color is not projection-expressible; the shared subset uses the dialect-representable marks.
- All three surfaces execute through core `runPlan` with plan `base = sha256:<hex>` of the exact current bytes; preview keys are core `p1:sha256:<64 lowercase hex>` (from `compute_preview_key`); the core validates the key against source + canonical plan at commit. Never emit `mvp1:` or `v4:`.
- Preview keys **are** Python values in this design (`preview = docx_preview(...)` returns an object with `.key`; the spec's examples require it). The cross-execution gate is the preview store's REPL execution number: commit is rejected when the current execution number is not strictly greater than the preview's, and when the current source hash or recomputed surface-state hash differs from the stored record. This supersedes the MVP's "key never enters Python" rule.
- Python public API is per-surface and exactly as specified in Tasks 3, 6, 7; host callbacks are the private `_docx_create`, `_docx_read`, `_docx_help`, `_docx_preview`, `_docx_commit`. `EditHtml` is removed; unknown op names, unknown fields, invented paragraph ids, and non-string content are rejected before the engine runs.
- Defaults: plan `author` `"docxdriver"`, `change_mode` `"track"`. Version 1 accepts plan-level `author` and `change_mode` from the `Plan` dataclass; Versions 2 and 3 always emit `track`.
- Keep `packages/docxdriver-pi/src/index.ts` and `package.json` `pi.extensions` unchanged; the three surfaces load through alternate entries with `pi -e`. Do not modify `tools.ts`, `engine.ts`, or `io.ts` signatures already used by the default extension (append-only where needed).
- Keep all paths cwd-contained with the existing `resolveUnderCwd` lexical+symlink checks; wrap the read/preview/commit/write window in `withFileMutationQueue`; write via `writeDocxBytes(cwd, abs, bytes, expectedSource)` (atomic sibling temp, fsync, immediate pre-write source recheck).
- Preview store caps at 64 records (FIFO eviction) and is cleared when the Monty session resets (a reset loses all Python state, so stored records are unusable).
- Offline tests must run without a model and without network: real Monty worker, real checked-in WASM, temp dirs. Live trials are opt-in via `PI_E2E_LIVE=1` and `PI_E2E_MODEL`.
- Identical preview/report formatting across surfaces: one shared notice renderer in `python-host.ts`; the core `PlanResult.report` is the only report source.
- Post-commit render verification: after the atomic write, re-read the bytes, render the markup, and run the surface deriver's `verifyCommit`; failures are reported as prominent `kind: 'verify'` notices (the write has already happened).

## File Structure

All paths relative to the worktree root. Files that change together live together in `packages/docxdriver-pi/src`:

| File | Responsibility |
| --- | --- |
| `src/python-repl.ts` | Port of the MVP Monty adapter: persistent session, serialized feeds, crash recovery; now takes the prelude/stubs as parameters and counts executions. |
| `src/python-host.ts` | Shared host: `PreviewStore`, `SurfaceDeriver` interface, `createPythonDocxHost`, five `_docx_*` callbacks, commit gate, notices, shared report renderer, post-commit verification driver. |
| `src/python-extension-factory.ts` | Shared Pi factory: lazy single-flight runtime, per-call host, truncation, `session_shutdown`, reset store clearing. |
| `src/projection.ts` | Shared canonical-markup parser + serializer (`parseProjection`, `serializeProjection`) and `ProjectionDoc` types. |
| `src/projection-diff.ts` | Version 2 reconciler: `diffProjections(original, proposed)` → typed ops or rejection reason. |
| `src/python-plan-deriver.ts` | Version 1: normalize `Plan` dicts to core plans, digest, verify. |
| `src/python-plan-prelude.ts` | Version 1 prelude: generated dataclasses + `Plan` + `docx_*` wrappers + stubs + API reference. |
| `src/python-string-deriver.ts` | Version 2: digest(proposed), derive via reconciler, verify via projection equality. |
| `src/python-string-prelude.ts` | Version 2 prelude: `docx_read`/`docx_preview`/`docx_commit` string surface. |
| `src/python-model-deriver.ts` | Version 3: digest(model serialization), derive via change-set replay, verify via expected projection. |
| `src/python-model-prelude.ts` | Version 3 prelude: `Document`/`Paragraph`/`Selection`/`Match` classes. |
| `src/python-plan-extension.ts` | Version 1 entry: `createPythonExtension(planSurfaceConfig())`. |
| `src/python-string-extension.ts` | Version 2 entry. |
| `src/python-model-extension.ts` | Version 3 entry. |
| `scripts/generate-python-dataclasses.mjs` | Reads `packages/docxdriver/schema/typed-request.schema.json` `$defs.editOp` and emits the V1 dataclass section into `src/python-plan-prelude.ts`. |
| `test/python-repl.test.mjs` | Runtime persistence, counter, crash recovery (ported + extended). |
| `test/python-host.test.mjs` | Shared gate: preview store, execution gate, source race, mutation-after-preview, path safety, atomic writes, key format. |
| `test/projection.test.mjs` | Parser/serializer round-trips and malformed-input rejection. |
| `test/projection-diff.test.mjs` | Reconciler mapping table and ambiguity rejections. |
| `test/python-plan-extension.test.mjs` | V1 offline e2e. |
| `test/python-string-extension.test.mjs` | V2 offline e2e. |
| `test/python-model-extension.test.mjs` | V3 offline e2e. |
| `test/repl-fixtures.mjs` | Deterministic corpus: `createContractFixture(cwd)`, `createComplexFixture(cwd)` (copy of `test-docs/ctnf-18690238-data-stream.docx`), tiny ZIP part-hash reader. |
| `test/repl-conformance.test.mjs` | The 10-task corpus × 3 surfaces, scripted offline, metrics JSON + canonical-plan equivalence asserts. |
| `test/pi-e2e/repl-tasks.json` | Live-trial task prompts + verifiers (10 tasks). |
| `test/pi-e2e/repl-live.mjs` | Opt-in live-agent harness (surfaces × tasks × reps). |
| `README.md` | Three-surface section with load commands. |

Port sources (read-only, from `.worktrees/python-native-repl-mvp` on branch `docxdriver-repl`): `src/python-repl.ts` (adapted), `test/python-repl.test.mjs`, `test/python-host.test.mjs` (adapted), and the extension/lifecycle patterns in `src/python-extension.ts`.

---

### Task 1: Persistent Monty Runtime with Execution Counter

**Files:**
- Modify: `packages/docxdriver-pi/package.json`
- Modify: `packages/docxdriver-pi/package-lock.json`
- Create: `packages/docxdriver-pi/src/python-repl.ts`
- Create: `packages/docxdriver-pi/test/python-repl.test.mjs`

**Interfaces:**
- Consumes: `@pydantic/monty` `Monty.create`, `MontySession.feedStart`/`feedRun`, `CollectString`, `MontyComplete`/`FunctionSnapshot`/`NameLookupSnapshot`/`FutureSnapshot`, `MontyCrashedError`/`MontyRuntimeError`/`MontySyntaxError`/`MontyTypingError`/`MontyError`/`ProtocolError`, `CheckoutOptions`.
- Produces: `PythonRepl.create(prelude: string, stubs: string): Promise<PythonRepl>`, `PythonRepl.execute(code: string, externalLookup: Record<string, unknown>): Promise<PythonExecution>`, `PythonRepl.executionNumber: number`, `PythonRepl.close(): Promise<void>` (idempotent). `PythonExecution = { ok: boolean; stdout: string; value?: unknown; error?: string; reset: boolean }`.

- [ ] **Step 1: Pin Monty and declare the Node floor**

Run:

```bash
cd packages/docxdriver-pi
npm install --save-exact @pydantic/monty@0.0.21
```

Add to `package.json`: `"engines": { "node": ">=20" }`. Confirm `@pydantic/monty` sits under `dependencies`.

- [ ] **Step 2: Write the failing runtime tests**

Port `test/python-repl.test.mjs` from `.worktrees/python-native-repl-mvp/packages/docxdriver-pi/test/python-repl.test.mjs` with these changes: `PythonRepl.create(PRELUDE, STUBS)` takes explicit prelude/stubs (use a tiny test prelude: `from dataclasses import dataclass\n@dataclass\nclass Op:\n    find: str = ""\n    def to_dict(self):\n        return {"op": "x", "find": self.find}` plus stubs declaring `Op` and `_host_fn`), and add the two new tests below. Keep the existing tests: state persists across feeds, dataclass attribute repair (`op.find = "corrected"`), runtime errors keep session state, `close()` twice is safe, crash recovery resets the session (SIGKILL the worker via `workerPid`), and host callbacks receive plain values.

```js
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
  assert.equal((await a.execute('hasattr(Op, "to_dict")', {})).value, true);
  const missing = await b.execute('Op', {});
  assert.equal(missing.ok, false);
  assert.equal((await b.execute('Op2', {})).ok, true);
});
```

- [ ] **Step 3: Run the test and observe RED**

Run: `npm run build && node --test test/python-repl.test.mjs`
Expected: failure — `dist/python-repl.js` does not exist.

- [ ] **Step 4: Port the runtime, parameterized**

Copy `.worktrees/python-native-repl-mvp/packages/docxdriver-pi/src/python-repl.ts` and change the constructor and prelude feed:

```ts
export class PythonRepl {
  private readonly pool: Monty;
  private session: MontySession;
  private tail: Promise<void> = Promise.resolve();
  private closed = false;
  private counter = 0;

  static async create(prelude: string, stubs: string): Promise<PythonRepl> {
    const pool = await Monty.create({ minProcesses: 1, maxProcesses: 1, requestTimeout: 15 });
    try {
      const session = await pool.checkout({
        scriptName: 'docxdriver_repl.py',
        typeCheck: true,
        typeCheckStubs: stubs,
        typeCheckFormat: 'concise',
        limits: { maxDurationSecs: 10, maxMemory: 256 * 1024 * 1024, maxRecursionDepth: 500 },
      });
      const repl = new PythonRepl(pool, session);
      await session.feedRun(prelude, { skipTypeCheck: true });
      return repl;
    } catch (error) {
      await pool.close().catch(() => undefined);
      throw error;
    }
  }

  /** Monotonic feed counter; a preview's number gates later commits. */
  get executionNumber(): number { return this.counter; }

  private async executeNow(code, externalLookup): Promise<PythonExecution> {
    this.counter += 1;
    // ... rest unchanged: CollectString(1 MiB), drive() snapshot loop,
    // formatMontyError, recoverAfterCrash with SESSION_OPTIONS hoisted to a
    // module-level constant taking prelude/stubs.
  }
}
```

Keep the `drive()` snapshot loop exactly as ported (await each external callback before `resume`, decline `os` callbacks, error on `isMethodCall`), the 1 MiB print cap, and the crash path that returns `reset: true` without silently retrying. Hoist `SESSION_OPTIONS`/`feedPrelude` to close over the constructor parameters.

- [ ] **Step 5: Run focused and package tests**

Run: `npm run build && node --test test/python-repl.test.mjs && npm test`
Expected: new tests pass; the existing 9+ extension tests still pass.

- [ ] **Step 6: Commit**

```bash
git add packages/docxdriver-pi/package.json packages/docxdriver-pi/package-lock.json \
  packages/docxdriver-pi/src/python-repl.ts packages/docxdriver-pi/test/python-repl.test.mjs
git commit -m "feat(pi): port persistent typed Monty runtime with execution counter"
```

---

### Task 2: Shared Host, Preview Store, and Cross-Execution Commit Gate

**Files:**
- Create: `packages/docxdriver-pi/src/python-host.ts`
- Create: `packages/docxdriver-pi/test/python-host.test.mjs`

**Interfaces:**
- Consumes: `ensureInit`, `runPlan`, `runCreate`, `runRead` from `./engine.js`; `resolveUnderCwd`, `sha256`, `writeDocxBytes` from `./io.js`; `withFileMutationQueue` from `@earendil-works/pi-coding-agent`; Task 1's `PythonRepl`.
- Produces (later tasks rely on exactly these):

```ts
export type ReadSource = { abs: string; bytes: Uint8Array; sha256: string; markup: string };
export type CorePlan = { base: string; author: string; change_mode: 'track' | 'direct'; ops: unknown[] };
export type DeriveResult = { ok: true; plan: CorePlan } | { ok: false; message: string };
export type VerifyEvidence = { preMarkup: string; postMarkup: string; affectedIds: string[] };

export interface SurfaceDeriver {
  readonly name: 'plan' | 'string' | 'model';
  /** Deterministic hash of the Python-supplied state; null means invalid state. */
  digest(state: unknown): string | null;
  /** Turn Python state into a typed core plan. Must stamp base = `sha256:${source.sha256}`. */
  derive(state: unknown, source: ReadSource): DeriveResult;
  /** Post-commit render verification. Return a problem description or null. */
  verifyCommit(record: PreviewRecord, evidence: VerifyEvidence): Promise<string | null>;
}

export interface PreviewRecord {
  path: string;            // as authored
  sourceHash: string;      // sha256 of file bytes at preview
  canonicalPlan: string;   // deterministic JSON of the derived plan
  surfaceStateHash: string;// deriver.digest(state)
  key: string;             // core p1:sha256:<64 hex>
  executionNumber: number; // repl.executionNumber at preview
  deriverData?: unknown;   // host-internal verification state
}

export class PreviewStore {
  put(record: PreviewRecord): void;              // FIFO cap 64
  get(key: string): PreviewRecord | undefined;
  clear(): void;
}

export type PythonHostNotice = {
  kind: 'create' | 'preview' | 'commit' | 'blocked' | 'verify';
  path?: string;
  phase?: 'create' | 'validation' | 'preview' | 'commit' | 'verify';
  status: 'ok' | 'blocked' | 'problem';
  summary?: string;
  applied?: number;
  failed?: number;
  key?: string;
  ops?: Array<{ op: string; outcome: string; summary: string }>;
};

export interface PythonDocxHost { externalLookup: Record<string, unknown>; }

export function createPythonDocxHost(
  cwd: string,
  notices: PythonHostNotice[],
  deriver: SurfaceDeriver,
  store: PreviewStore,
  getExecutionNumber: () => number,
): PythonDocxHost;

export const CORE_KEY_RE: RegExp;              // /^p1:sha256:[0-9a-f]{64}$/
export function canonicalJson(value: unknown): string;  // sorted keys, arrays in order
export function renderNotices(notices: PythonHostNotice[]): string; // shared report formatting
```

- [ ] **Step 1: Write failing host-gate tests**

Create `test/python-host.test.mjs` using a stub deriver (registered in the test file) and a real `PythonRepl` created with the Task 1 test prelude. Cover, in order:

1. `_docx_preview` with a stub deriver that always derives `{op:'replace_text', at:'ABCD0001', select:'old', with:'new'}`: reading a real fixture DOCX, preview returns a dict whose `key` matches `/^p1:sha256:[0-9a-f]{64}$/`; the notice has `kind:'preview'` and carries the same key.
2. The same preview called twice stores two records; `store.get(key)` returns all six documented fields with `executionNumber === repl.executionNumber`.
3. Commit in the **same** execution (preview and commit inside one `execute` call) is blocked with a notice containing `later python execution`, and no write happens.
4. Commit in a **later** execution with the correct key and unchanged state commits; bytes change; a `commit` notice reports applied counts; `writeDocxBytes` was reached with the recorded source hash.
5. **Source race (spec Task 8):** after preview, overwrite the file with different bytes; commit is blocked (`source hash` notice) and the newer bytes are preserved.
6. **Mutation after preview (spec Task 9):** after preview, commit with a state whose `deriver.digest()` differs (stub deriver digests `state.version`); blocked with `state changed` notice; file unchanged.
7. Unknown key and malformed key (`p1:sha256:nothex`) are blocked before any engine call.
8. `_docx_read` returns the rendered markup; `_docx_create` refuses overwrite; `../escape.docx` and a symlink escape are rejected without leaking the absolute path.
9. A successful commit leaves no `.tmp` sibling behind and the post-write bytes hash equals the committed bytes.
10. `renderNotices` output is identical regardless of deriver name (shared formatting).

The stub deriver: `digest: (s) => typeof s === 'object' && s !== null ? sha256(JSON.stringify(s)) : null`, `derive: (s) => s.valid === false ? { ok: false, message: 'stub: invalid state' } : { ok: true, plan: {...} }`, `verifyCommit: () => null`.

- [ ] **Step 2: Run the test and observe RED**

Run: `npm run build && node --test test/python-host.test.mjs`
Expected: failure — `dist/python-host.js` does not exist.

- [ ] **Step 3: Implement the preview store and canonicalization**

In `src/python-host.ts`:

```ts
export class PreviewStore {
  private records = new Map<string, PreviewRecord>();
  put(record: PreviewRecord): void {
    this.records.set(record.key, record);
    while (this.records.size > 64) {
      const oldest = this.records.keys().next().value as string;
      this.records.delete(oldest);
    }
  }
  get(key: string): PreviewRecord | undefined { return this.records.get(key); }
  clear(): void { this.records.clear(); }
}

export function canonicalJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`;
  if (value !== null && typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>)
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
    return `{${entries.map(([k, v]) => `${JSON.stringify(k)}:${canonicalJson(v)}`).join(',')}}`;
  }
  return JSON.stringify(value);
}
```

Note: the store's `canonicalPlan` is a host-side deterministic digest only; the authoritative key check is core's `execute_plan` at commit. Keep the two independent.

- [ ] **Step 4: Implement the shared host with the commit gate**

`createPythonDocxHost(cwd, notices, deriver, store, getExecutionNumber)`:

- `_docx_create(path, html)`: as the MVP port — refuse overwrite, `runCreate`, `writeDocxBytes` without expected source, `kind:'create'` notice.
- `_docx_read(path, view='markup')`: `runRead(bytes, { kind: 'document', view })`, return `result.markup` (a string), validate `view ∈ {markup, final, original}`.
- `_docx_help(topic)`: `runHelp(topic)` summary text (per-surface API reference lives in the extension description instead).
- `_docx_preview(path, state)`:

```ts
const stateHash = deriver.digest(state);
if (stateHash === null) { blockedNotice('validation', 'state is not a valid ...'); return null; }
const abs = await resolveUnderCwd(cwd, String(path));
return withFileMutationQueue(abs, async () => {
  const { bytes, sha256: sourceSha } = await readSource(abs);
  const markup = await renderMarkup(abs);            // runRead view=markup
  const derived = deriver.derive(state, { abs, bytes, sha256: sourceSha, markup });
  if (!derived.ok) { blockedNotice('validation', derived.message); return null; }
  const output = runPlan(bytes, derived.plan);        // no preview key → preview mode
  if (output.outcome !== 'previewed') { blockedNotice('preview', output.diagnostic?.message ?? 'preview rejected', output.report); return null; }
  const record: PreviewRecord = {
    path: String(path), sourceHash: sourceSha,
    canonicalPlan: canonicalJson(derived.plan), surfaceStateHash: stateHash,
    key: output.preview_key, executionNumber: getExecutionNumber(),
  };
  store.put(record);
  notice({ kind: 'preview', path: String(path), phase: 'preview', status: 'ok', key: output.preview_key,
           applied: output.report.completed, failed: 0, summary: `preview ok: ${output.report.completed} ops`, ... });
  return { key: output.preview_key };
});
```

Returning `{ key }` to Python: if the Monty binding does not convert plain JS objects to Python dicts, return `new Map([['key', output.preview_key]])` instead — the public shape stays `preview.key`. The offline test asserts `preview.key` resolves in Python.

- `_docx_commit(path, key, state)` — the gate, in this exact order:

```ts
if (typeof key !== 'string' || !CORE_KEY_RE.test(key)) { blockedNotice('commit', 'malformed preview key'); return null; }
const record = store.get(key);
if (!record) { blockedNotice('commit', 'unknown preview key — preview first'); return null; }
const now = getExecutionNumber();
if (now <= record.executionNumber) { blockedNotice('commit', `commit requires a later python execution (preview was execution ${record.executionNumber}, this is ${now})`); return null; }
const stateHash = deriver.digest(state);
if (stateHash === null || stateHash !== record.surfaceStateHash) { blockedNotice('commit', 'state changed after preview — preview again'); return null; }
const abs = await resolveUnderCwd(cwd, String(path));
await withFileMutationQueue(abs, async () => {
  const source = await readSource(abs);
  if (source.sha256 !== record.sourceHash) { blockedNotice('commit', 'source changed after preview — re-read and preview again'); return; }
  const derived = deriver.derive(state, source);
  if (!derived.ok || canonicalJson(derived.plan) !== record.canonicalPlan) { blockedNotice('commit', 'plan changed after preview — preview again'); return; }
  const output = runPlan(source.bytes, derived.plan, key);   // core validates the key
  if (output.outcome !== 'committed' || !output.bytes) { blockedNotice('commit', output.diagnostic?.message ?? 'commit rejected', output.report); return; }
  await writeDocxBytes(cwd, abs, output.bytes, record.sourceHash);   // atomic + immediate pre-write recheck
  const post = await readSource(abs);
  const affectedIds = (output.report?.ops ?? []).flatMap((op) => (op.affected ?? []).map((a: any) => String(a.para_id)));
  const problem = await deriver.verifyCommit(record, { preMarkup: source.markup, postMarkup: post.markup, affectedIds });
  notice(problem === null
    ? { kind: 'commit', path: String(path), phase: 'commit', status: 'ok', applied: output.report.completed, failed: 0, summary: output.summary ?? 'committed' }
    : { kind: 'verify', path: String(path), phase: 'verify', status: 'problem', summary: problem });
});
return null;
```

`readSource(abs)` returns `{ bytes, sha256, markup }` where `markup` is rendered via `runRead` (cached per call). `renderMarkup` must be shared by preview and commit so `preMarkup` is the true pre-write render. `boundedNoticer` from the MVP port caps notices at 32 and summaries at 4096 chars.

- [ ] **Step 5: Shared notice formatting**

`renderNotices(notices)` produces one text block, identical for every surface:

```
── preview ok ── path: contract.docx
source sha256:087d99ad…
preview key: p1:sha256:…(hex)
3 ops applied
  1. replace_text @5E27EE16 applied: replaced "thirty days" with "sixty days"
```

Use the core report's per-op `index`, `op`, `outcome`, and `summary`; `key` only on preview notices; blocked notices prefix `── blocked ──`; verify notices prefix `── verification problem ──`.

- [ ] **Step 6: Run focused and package tests**

Run: `npm run build && node --test test/python-host.test.mjs && npm test`
Expected: all new gate tests pass; existing tests unaffected.

- [ ] **Step 7: Commit**

```bash
git add packages/docxdriver-pi/src/python-host.ts packages/docxdriver-pi/test/python-host.test.mjs
git commit -m "feat(pi): shared Python host with cross-execution preview store and commit gate"
```

---

### Task 3: Version 1 — Python-Authored Plan Surface (Reference)

**Files:**
- Create: `packages/docxdriver-pi/scripts/generate-python-dataclasses.mjs`
- Create: `packages/docxdriver-pi/src/python-plan-prelude.ts`
- Create: `packages/docxdriver-pi/src/python-plan-deriver.ts`
- Create: `packages/docxdriver-pi/src/python-extension-factory.ts`
- Create: `packages/docxdriver-pi/src/python-plan-extension.ts`
- Create: `packages/docxdriver-pi/test/python-plan-extension.test.mjs`

**Interfaces:**
- Consumes: Task 1 `PythonRepl.create`, Task 2 `createPythonDocxHost`/`PreviewStore`/`SurfaceDeriver`/`canonicalJson`/`renderNotices`, `generatedOperationSchema` from `./schema.js` (indirectly via the generator), `truncateHead`/`DEFAULT_MAX_LINES`/`DEFAULT_MAX_BYTES` from `@earendil-works/pi-coding-agent`.
- Produces:
  - `src/python-plan-prelude.ts` exports `PYTHON_PRELUDE`, `PYTHON_TYPE_STUBS`, `PYTHON_API_REFERENCE` for the V1 surface.
  - `src/python-plan-deriver.ts` exports `planSurfaceDeriver(): SurfaceDeriver` (name `'plan'`).
  - `src/python-extension-factory.ts` exports `createPythonExtension(config: PythonSurfaceConfig): (pi: ExtensionAPI) => Promise<void>` where `PythonSurfaceConfig = { name: 'plan'|'string'|'model'; prelude: string; stubs: string; apiReference: string; deriver: SurfaceDeriver; toolName: string; toolDescription: string; promptSnippet: string; promptGuidelines: string[] }`.
  - `src/python-plan-extension.ts` default-export `createPythonExtension(planSurfaceConfig())`.

- [ ] **Step 1: Write the failing V1 surface tests**

`test/python-plan-extension.test.mjs` mirrors the MVP extension test shape (stub Pi capturing `registerTool` and `session_shutdown`) with the V1 workflow:

```js
test('v1: registers exactly the python tool', () => {
  assert.deepEqual([...tools.keys()], ['python']);
});

test('v1: plan authoring, preview key, later-execution commit', async () => {
  const cwd = await freshDir();
  const result = await call('python', { code: [
    'original = docx_read("contract.docx")',
    'plan = Plan(author="Pi", change_mode="track", operations=[',
    '    ReplaceText(at="' + firstId + '", select="thirty days", with_="sixty days"),',
    '    FormatText(at="' + firstId + '", select="sixty days", bold=True),',
    '])',
    'preview = docx_preview("contract.docx", plan)',
    'print("KEY=" + preview.key)',
  ].join('\n') }, cwd);
  assert.match(result.text, /KEY=p1:sha256:[0-9a-f]{64}/);
  const key = result.text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];
  const commit = await call('python', { code: [
    'docx_commit("contract.docx", preview.key, plan)',
    'print("committed")',
  ].join('\n') }, cwd);
  assert.match(commit.text, /committed/);
  const final = await renderMarkup(join(cwd, 'contract.docx'));
  assert.match(final, /sixty days/);
  assert.ok(!final.includes('thirty days'));
});
```

Also test: the fixture must be created first via `docx_create` in the same REPL (first tool call), a same-execution commit attempt is blocked, `Plan(operations=[])` and an unknown op name produce `── blocked ──` validation notices before any write, and `docx_edit`/`EditHtml` do not exist in the prelude (NameError).

- [ ] **Step 2: Run the test and observe RED**

Run: `npm run build && node --test test/python-plan-extension.test.mjs`
Expected: failure — `dist/python-plan-extension.js` does not exist.

- [ ] **Step 3: Write the dataclass generator and its drift test**

`scripts/generate-python-dataclasses.mjs` (Node, no deps): read `packages/docxdriver/schema/typed-request.schema.json`, take `$defs.editOp.oneOf`, filter to the six-op subset, and for each variant emit a `@dataclass` with:
- required fields as positional-or-keyword params with `: T` annotations (first the op-specific requireds, plus `at`/`select`/`with` etc.);
- optional fields as `= None`;
- Python keyword collisions renamed: `with` → `with_`, `as` → `as_`, `from` → `from_`;
- `delete_paragraphs.at` annotated `list` (its `at` is an array);
- `occurrence: int | None = None`;
- a `to_dict(self)` that emits `{"op": "<name>", ...}` omitting `None` fields and mapping `with_`→`"with"`, `as_`→`"as"`; `at` values passed through unchanged (uppercasing happens host-side);
- filter `format_text` properties to the six dialect attrs (bold, italic, underline, strike, superscript, subscript) and `format_paragraph` to `style`;
- emit `Plan`:

```python
@dataclass
class Plan:
    author: str = "docxdriver"
    change_mode: str = "track"
    operations: list = field(default_factory=list)
    def to_dict(self):
        return {
            "author": self.author,
            "change_mode": self.change_mode,
            "ops": [op.to_dict() for op in self.operations],
        }
```

The script writes the generated block into `src/python-plan-prelude.ts` between `// BEGIN GENERATED DATACLASSES` and `// END GENERATED DATACLASSES` markers (idempotent). Add a drift test inside `test/python-plan-extension.test.mjs` that runs the generator to a temp file and asserts the checked-in block matches (like `packages/docxdriver/test/schema-drift.test.mjs`).

- [ ] **Step 4: Write the V1 prelude, stubs, and API reference**

`src/python-plan-prelude.ts` exports the generated dataclasses plus exactly this wrapper layer (order matters: the prelude must define the dataclasses first, then the wrappers):

```python
from dataclasses import dataclass, field

# ...generated dataclasses...

@dataclass
class PreviewResult:
    key: str

def docx_create(path: str, html: str):
    return _docx_create(path, html)

def docx_read(path: str, view: str = "markup") -> str:
    return _docx_read(path, view)

def docx_help(topic: str | None = None) -> str:
    return _docx_help(topic)

def docx_preview(path: str, plan: Plan):
    return _docx_preview(path, {"plan": plan.to_dict()})

def docx_commit(path: str, key: str, plan: Plan):
    _docx_commit(path, key, {"plan": plan.to_dict()})
    return None
```

`PYTHON_TYPE_STUBS` declares the generated dataclasses (constructor + attribute + `to_dict`) and the six public/private functions. `PYTHON_API_REFERENCE` is a concise doc with the exact signatures and the spec's V1 example (author, change_mode, ReplaceText/FormatText, preview, commit).

- [ ] **Step 5: Implement the V1 deriver**

`src/python-plan-deriver.ts`:

- `digest(state)`: `null` unless `state` is a plain object with a `plan` key; else `sha256(canonicalJson(plan))` where `plan` is the normalized plan (below).
- `derive(state, source)`: normalize with the MVP's strict rules adapted to the subset: plan must contain exactly `author` (non-empty string), `change_mode` ∈ `{track, direct}`, `ops` (non-empty array); each op's `op` must be one of the six; unknown fields rejected; `at` (or `at[]`) uppercased; `occurrence` positive integer; `with`/`select` strings preserved untrimmed; `position` ∈ `{before, after}`; then return `{ ok: true, plan: { base: 'sha256:' + source.sha256, author, change_mode, ops } }`.
- `verifyCommit(record, evidence)`: extract paragraph id sets from `evidence.preMarkup` and `evidence.postMarkup` with a small self-contained helper `idSetFromMarkup(markup): Set<string>` defined in this file (balanced-tag scan for top-level `<p …>`/`<hN …>` elements, regex `id="([0-9A-F]{8})"` per element — ids only, no full parsing, so Task 3 has no dependency on Task 4's parser), compute the symmetric set difference, and return `null` when that set is a subset of `evidence.affectedIds`, else a message listing the unexpected ids. Task 3's tests cover it with the two markup strings captured from a real preview/commit pair.

- [ ] **Step 6: Implement the shared factory and the V1 entry point**

`src/python-extension-factory.ts` (adapted from the MVP's `python-extension.ts`: single-flight lazy `PythonRepl.create(prelude, stubs)`, generation counter for shutdown races, per-call `notices` + `store` + host, `session_shutdown` closes the runtime and clears the store, `details` = `{ status, reset, noticeCount }`, output rendered with `renderExecution` using `truncateHead` at `DEFAULT_MAX_LINES`/`DEFAULT_MAX_BYTES`). Additions:

```ts
let store = new PreviewStore();
const runtime = await getRepl();
const notices: PythonHostNotice[] = [];
const host = createPythonDocxHost(ctx.cwd, notices, config.deriver, store, () => runtime.executionNumber);
const execution = await runtime.execute(code, host.externalLookup);
if (execution.reset) store.clear();   // session state was lost; records are unusable
```

`src/python-plan-extension.ts`:

```ts
import { createPythonExtension } from './python-extension-factory.js';
import { PYTHON_PRELUDE, PYTHON_TYPE_STUBS, PYTHON_API_REFERENCE } from './python-plan-prelude.js';
import { planSurfaceDeriver } from './python-plan-deriver.js';

export default createPythonExtension({
  name: 'plan',
  prelude: PYTHON_PRELUDE, stubs: PYTHON_TYPE_STUBS, apiReference: PYTHON_API_REFERENCE,
  deriver: planSurfaceDeriver(),
  toolName: 'python',
  toolDescription: 'Persistent sandboxed Python REPL that authors typed DOCX plans (Version 1: Python-authored plan).',
  promptSnippet: 'Run stateful sandboxed Python authoring typed DOCX plans',
  promptGuidelines: [
    'Do all DOCX work through the python tool; never use other file tools on .docx files.',
    'Read first with docx_read(path) and address paragraphs by their id attribute.',
    'Author operations explicitly: ReplaceText/FormatText/ReplaceParagraph/InsertParagraph/DeleteParagraphs inside a Plan(author=..., change_mode="track").',
    'Preview with docx_preview(path, plan); commit only in a separate later python call with docx_commit(path, preview.key, plan).',
    'Repair a blocked preview by changing one attribute of an existing operation object, then preview again.',
  ],
});
```

- [ ] **Step 7: Run offline verification**

Run: `npm run build && node --test test/python-plan-extension.test.mjs && npm test`
Expected: all pass; generator drift test passes; default extension surface unchanged.

- [ ] **Step 8: Commit**

```bash
git add packages/docxdriver-pi/scripts/generate-python-dataclasses.mjs \
  packages/docxdriver-pi/src/python-plan-prelude.ts packages/docxdriver-pi/src/python-plan-deriver.ts \
  packages/docxdriver-pi/src/python-extension-factory.ts packages/docxdriver-pi/src/python-plan-extension.ts \
  packages/docxdriver-pi/test/python-plan-extension.test.mjs
git commit -m "feat(pi): Version 1 Python-authored typed plan REPL surface"
```

---

### Task 4: Shared Projection Parser and Serializer

**Files:**
- Create: `packages/docxdriver-pi/src/projection.ts`
- Create: `packages/docxdriver-pi/test/projection.test.mjs`

**Interfaces:**
- Consumes: nothing beyond TS. The markup dialect grammar (verified against `html/render.rs` and a live render): paragraphs are `<p id="…" ord="…" class="…" num="…" break-ins="…" break-del="…">…</p>` or `<h1>…</h1>`…`<h6>…</h6>`; inline items are text, `<b>`, `<i>`, `<u>`, `<s>`, `<sup>`, `<sub>`, `<a href="…">`, `<br/>`, `<field instr="…">`, `<del id author>`, `<ins id author>`, `<format pending …>`, `<image …/>`, `<equation>`, `<table>`; attributes use double quotes; text escapes `&amp; &lt; &gt; &quot;`.
- Produces (used by Tasks 5, 6, 7 and by V1's `verifyCommit`):

```ts
export type FormatFlags = { bold: boolean; italic: boolean; underline: boolean; strike: boolean; superscript: boolean; subscript: boolean };
export type ProjectionItem =
  | { kind: 'text'; text: string; fmt: FormatFlags }
  | { kind: 'link'; text: string; href: string; fmt: FormatFlags }
  | { kind: 'field'; text: string; instr: string }
  | { kind: 'image' | 'equation' | 'table' | 'protected'; text: string }
  | { kind: 'revision'; tag: 'del' | 'ins' | 'format'; text: string; fmt: FormatFlags };
export type ProjectionParagraph = {
  id: string | null;                 // null only for id-less (new) paragraphs
  tag: 'p' | 'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6';
  styleClass: string | null;         // class attribute when tag is p
  attrs: Record<string, string>;     // ord, num, break-ins, break-del if present
  items: ProjectionItem[];
  plainText: string;                 // concatenated item text
  markup: string;                    // re-serialized paragraph markup
};
export type ProjectionDoc = { paragraphs: ProjectionParagraph[] };

export function parseProjection(markup: string): ProjectionDoc;      // throws ProjectionError(message)
export class ProjectionError extends Error {}
export function serializeProjection(doc: ProjectionDoc): string;     // canonical dialect string
export function paragraphMarkup(p: ProjectionParagraph): string;     // one paragraph, dialect syntax
```

- [ ] **Step 1: Write the failing parser tests**

`test/projection.test.mjs`:

```js
test('parses the canonical render of a simple document', () => {
  const doc = parseProjection('<p id="5E27EE16" ord="1">Payment is due within <b>30 days</b>.</p>\n<p id="69D4601B" ord="2">Second <i>terms</i>.</p>');
  assert.equal(doc.paragraphs.length, 2);
  const [first, second] = doc.paragraphs;
  assert.equal(first.id, '5E27EE16');
  assert.equal(first.plainText, 'Payment is due within 30 days.');
  assert.deepEqual(first.items, [
    { kind: 'text', text: 'Payment is due within ', fmt: blankFmt },
    { kind: 'text', text: '30 days', fmt: { ...blankFmt, bold: true } },
    { kind: 'text', text: '.', fmt: blankFmt },
  ]);
  assert.equal(second.items[0].fmt.italic, true);
});

test('round-trip: serialize(parse(markup)) is stable and attribute-ordered', () => {
  const markup = '<p id="A1B2C3D4" ord="1" class="Heading1" num="1.">One <u>under</u>.</p>';
  assert.equal(serializeProjection(parseProjection(markup)), markup);
});

test('protected items: link, field, table, image, equation, del/ins/format revisions parse without loss', () => {
  const markup = '<p id="A1B2C3D4" ord="1">See <a href="https://x.example">the x</a> in <field instr="DATE">1/1/26</field><del id="r1" author="a"> old</del><ins id="r2" author="b"> new</ins><br/> done</p>';
  const doc = parseProjection(markup);
  assert.equal(doc.paragraphs[0].items.filter((i) => i.kind === 'link').length, 1);
  assert.equal(doc.paragraphs[0].items.filter((i) => i.kind === 'revision').length, 2);
  assert.equal(serializeProjection(doc), markup);
});

test('rejects malformed markup with a located message', () => {
  for (const bad of ['<p>unclosed', '<p id="x" ord="1">', '<p id="A0000001" ord="1"><b>no close</p>', '<div>x</div>', '<p id="A0000001" ord="1"><unknown>x</unknown></p>']) {
    assert.throws(() => parseProjection(bad), ProjectionError);
  }
});

test('treats whitespace between blocks as insignificant', () => {
  const a = parseProjection('<p id="A0000001" ord="1">x</p>\n<p id="B0000002" ord="2">y</p>');
  const b = parseProjection('<p id="A0000001" ord="1">x</p><p id="B0000002" ord="2">y</p>');
  assert.deepEqual(a, b);
});
```

- [ ] **Step 2: Run and observe RED**

Run: `npm run build && node --test test/projection.test.mjs`
Expected: failure — `dist/projection.js` does not exist.

- [ ] **Step 3: Implement the parser and serializer**

`parseProjection` algorithm (reference implementation; ~180 lines):

1. Strip insignificant whitespace between top-level elements: tokenize with a small scanner that consumes `<`-tag tokens and text runs, skipping whitespace-only text outside any element.
2. Maintain a stack. On open tag: `p`/`h1..h6` must be top-level (a new paragraph closes the previous one); `b`/`i`/`u`/`s`/`sup`/`sub`/`a`/`field`/`del`/`ins`/`format` open inline scopes; `br`/`image` must self-close; `table` opens a protected scope that consumes until `</table>` verbatim; `equation` until `</equation>` verbatim; unknown tags throw.
3. Emit `ProjectionItem`s as text with the current format flags; when a `del`/`ins`/`format` scope is open, the emitted item kind is `revision` (with its tag). `a` scopes emit `link` items; `field` emits one `field` item whose text is the concatenated inner text (nested inline tags inside a field are flattened into its text verbatim).
4. Paragraph close: require id matching `/^[0-9A-F]{8}$/` when present; `id` may be absent only for paragraphs the caller parses from *proposed* content — provide a second export `parseProjectionLoose` used by the reconciler's proposed side which permits a missing `id` (still rejecting invalid ids).
5. Compute `plainText` and `markup` via `paragraphMarkup` (canonical attribute order: `id`, `ord`, `class`, `num`, `break-ins`, `break-del`; inline tags in the order `b,i,u,s,sup,sub`; `del`/`ins` close before formatting and reopen after, matching `render.rs`).
6. On any structural error, throw `ProjectionError` with a message that includes the paragraph context (not a byte offset — model-readable).

`serializeProjection(doc)` joins `paragraphMarkup(p)` with `\n`. Guard rails: items with `fmt` non-default render nested tags; text is escaped with the same escapes as `html/mod.rs::escape_text`.

- [ ] **Step 4: Run focused and package tests**

Run: `npm run build && node --test test/projection.test.mjs && npm test`

- [ ] **Step 5: Commit**

```bash
git add packages/docxdriver-pi/src/projection.ts packages/docxdriver-pi/test/projection.test.mjs
git commit -m "feat(pi): shared canonical DOCX projection parser and serializer"
```

---

### Task 5: Projection Reconciler (Version 2 Core)

**Files:**
- Create: `packages/docxdriver-pi/src/projection-diff.ts`
- Create: `packages/docxdriver-pi/test/projection-diff.test.mjs`

**Interfaces:**
- Consumes: Task 4 `parseProjection`/`parseProjectionLoose`/`serializeProjection`/`ProjectionDoc`/`ProjectionItem`/`FormatFlags`.
- Produces: `diffProjections(originalMarkup: string, proposedMarkup: string, opts?: { sourceSha?: string }): DiffResult` with `DiffResult = { ok: true; ops: CoreOp[] } | { ok: false; reason: string }`, where `CoreOp` is the JSON op shape consumed by `runPlan` (`{ op: 'replace_text', at, select, with, occurrence? }` etc.), in document order. Also exports `allocateInsertId(sourceSha: string, index: number, existing: Set<string>): string` for chained inserts. `sourceSha` is required for chained (two or more consecutive id-less) inserts — the string deriver passes `source.sha256` (stable across preview/commit because the gate pins the source hash); when chaining is needed without `sourceSha`, reject with `reason: 'consecutive new paragraphs are ambiguous without a source hash'`.

- [ ] **Step 1: Write the failing reconciler tests**

`test/projection-diff.test.mjs` covers the spec's initial mapping table exactly:

```js
test('changed span maps to the narrowest replace_text', () => {
  const r = diffProjections(
    '<p id="2673269E" ord="1">The notice period is thirty days.</p>',
    '<p id="2673269E" ord="1">The notice period is sixty days.</p>');
  assert.deepEqual(r, { ok: true, ops: [{ op: 'replace_text', at: '2673269E', select: 'thirty', with: 'sixty' }] });
});

test('a single span change keeps the fmt tags out of the select and into with', () => {
  const r = diffProjections(
    '<p id="2673269E" ord="1">The <b>notice period</b> is thirty days.</p>',
    '<p id="2673269E" ord="1">The <b><u>notice period</u></b> is thirty days.</p>');
  assert.deepEqual(r, { ok: true, ops: [{ op: 'format_text', at: '2673269E', select: 'notice period', underline: true }] });
});

test('third occurrence of repeated text maps to occurrence=3', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">the terms. the terms. the terms. the terms.</p>',
    '<p id="A0000001" ord="1">the terms. the terms. the TERMS. the terms.</p>');
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: 'A0000001', select: 'the terms', with: 'the TERMS', occurrence: 3 }]);
});

test('every occurrence changed maps to sequential occurrence=1 ops', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">30 30 30</p>',
    '<p id="A0000001" ord="1">45 45 45</p>');
  assert.deepEqual(r.ops, [
    { op: 'replace_text', at: 'A0000001', select: '30', with: '45', occurrence: 1 },
    { op: 'replace_text', at: 'A0000001', select: '30', with: '45', occurrence: 1 },
    { op: 'replace_text', at: 'A0000001', select: '30', with: '45', occurrence: 1 },
  ]);
});

test('pure insertion inside a mostly-unchanged paragraph extends the select', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">Due.</p>',
    '<p id="A0000001" ord="1">Due within 30 days.</p>');
  assert.deepEqual(r.ops, [{ op: 'replace_text', at: 'A0000001', select: 'Due', with: 'Due within 30 days' }]);
});

test('paragraph changed by more than half maps to replace_paragraph', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">Old wording entirely.</p>',
    '<p id="A0000001" ord="1">Brand new wording.</p>');
  assert.deepEqual(r.ops, [{ op: 'replace_paragraph', at: 'A0000001', with: 'Brand new wording.' }]);
});



test('new id-less paragraph maps to insert_paragraph', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">one</p><p id="B0000002" ord="2">three</p>',
    '<p id="A0000001" ord="1">one</p><p>two</p><p id="B0000002" ord="2">three</p>');
  assert.deepEqual(r.ops, [{ op: 'insert_paragraph', at: 'A0000001', position: 'after', with: 'two' }]);
});

test('removed ids map to one delete_paragraphs op', () => {
  const r = diffProjections(
    '<p id="A0000001" ord="1">one</p><p id="B0000002" ord="2">two</p><p id="C0000003" ord="3">three</p>',
    '<p id="A0000001" ord="1">one</p><p id="C0000003" ord="3">three</p>');
  assert.deepEqual(r.ops, [{ op: 'delete_paragraphs', at: ['B0000002'] }]);
});

test('inline markup delta maps to format_text', () => {
  const r = diffProjections(
    '<p id="C0000003" ord="1">The <b>warranty</b> period.</p>',
    '<p id="C0000003" ord="1">The <b><i>warranty</i></b> period.</p>');
  assert.deepEqual(r.ops, [{ op: 'format_text', at: 'C0000003', select: 'warranty', italic: true }]);
});

test('class change maps to format_paragraph style', () => {
  const r = diffProjections('<p id="D0000004" ord="1" class="Normal">x</p>', '<p id="D0000004" ord="1" class="Heading1">x</p>');
  assert.deepEqual(r.ops, [{ op: 'format_paragraph', at: 'D0000004', style: 'Heading1' }]);
});

test('rejects: invented id, protected content change, crossing revision boundary, consecutive id-less paragraphs, ord/num tampering', () => {
  const cases = [
    ['invented id', '<p id="A0000001" ord="1">x</p>', '<p id="DEADBEEF" ord="1">y</p>'],
    ['link text change', '<p id="A0000001" ord="1"><a href="https://x.example">x</a></p>', '<p id="A0000001" ord="1"><a href="https://x.example">y</a></p>'],
    ['field content change', '<p id="A0000001" ord="1"><field instr="DATE">1/1/26</field></p>', '<p id="A0000001" ord="1"><field instr="DATE">2/2/26</field></p>'],
    ['revision boundary', '<p id="A0000001" ord="1"><del id="r1" author="a">old</del> mid</p>', '<p id="A0000001" ord="1"><del id="r1" author="a">old</del> mid2</p>'],
    ['two consecutive new paragraphs', '<p id="A0000001" ord="1">x</p>', '<p id="A0000001" ord="1">x</p><p>y</p><p>z</p>'],
    ['num tampering', '<p id="A0000001" ord="1" num="1.">x</p>', '<p id="A0000001" ord="1" num="2.">x</p>'],
  ];
  for (const [label, orig, prop] of cases) {
    const r = diffProjections(orig, prop);
    assert.equal(r.ok, false, label);
    assert.match(r.reason, new RegExp(label.split(' ')[0]));
  }
});

test('malformed proposed projection is rejected with a located reason', () => {
  const r = diffProjections('<p id="A0000001" ord="1">x</p>', '<p id="A0000001" ord="1"><b>x</p>');
  assert.equal(r.ok, false);
  assert.match(r.reason, /malformed|parse/i);
});
```

- [ ] **Step 2: Run and observe RED**

Run: `npm run build && node --test test/projection-diff.test.mjs`
Expected: failure — `dist/projection-diff.js` does not exist.

- [ ] **Step 3: Implement the reconciler**

`diffProjections(originalMarkup, proposedMarkup)`:

1. `parseProjection(originalMarkup)` (strict) and `parseProjectionLoose(proposedMarkup)`; any `ProjectionError` → `{ ok: false, reason: 'malformed projection: ' + message }`.
2. Reject `proposed` paragraphs whose `id` is present but not in the original id set — `reason: 'unknown paragraph id "<id>" — ids are engine-owned; write new paragraphs without an id attribute'`. Reject `original` paragraphs missing ids (cannot happen from a render; defensive).
3. **Protected content:** for every matched paragraph, compare `items` structurally by kind/href/instr/tag; any difference in `link`, `field`, `image`, `equation`, `table`, `protected`, or `revision` items → `reason: 'protected content changed in paragraph <id> (<kind>) — not expressible through the projection'`. A changed span (computed in step 5) that starts or ends inside a protected item's text range also rejects (`reason: 'change crosses protected content in paragraph <id>'`).
4. **Structure pass:** original ids missing from proposed (in original order) → one `delete_paragraphs` op at the position of the first deleted paragraph. Id-less proposed paragraphs → insertion anchors: the nearest preceding proposed paragraph with an original id; if none, `at` = first original paragraph with `position: 'before'`. Two or more consecutive id-less paragraphs → chain via `allocateInsertId(sourceSha, index, originalIds)` (first 8 hex chars of `sha256(sourceSha + '\0' + index)` mapped into `[1, 0x7FFFFFFF]`, skipping ids in the original set); `sourceSha` comes from `opts.sourceSha`; a collision → `reason: 'insert id collision'`. `with` = `paragraphMarkup`-style content string of the proposed paragraph (text + inline fmt tags; `style` extracted from `styleClass`).
5. **Content pass per matched paragraph** (skip if deleted):
   a. Split both plain texts into changed/unchanged spans with a difflib-style longest-common-substring splitter (`get_opcodes` semantics: `equal` / `replace` / `insert` / `delete` pairs, computed on paragraph-sized strings — an O(n²) DP is fine at this scale). Compute `equalChars` = total length of `equal` blocks; if `equalChars / max(origLen, propLen) < 0.5` → whole paragraph → `replace_paragraph(at, with=proposed paragraph content)` (dialect-serialized, fmt tags included) and skip to step 7.
   b. If no pairs → no text ops.
   c. Empty-side pairs (pure `insert`/`delete`): merge each into the adjacent changed pair when one exists (previous if present, else next), extending that pair's `select`/`with` across the intervening equal block; a lone empty-side pair is extended backward into the preceding equal block (forward when at the very start). If the extended `select` matches more than once in the original plainText → `replace_paragraph` fallback for the whole paragraph (safe, not narrow).
   d. Otherwise emit one `replace_text` per pair in document order. `select` = plain text of the original span; `with` = dialect-serialized content of the proposed span (item range, so any fmt tags the agent added inside the span carry into `with`). Occurrence: count matches of `select` in the **original** plainText; if the paragraph has multiple pairs with the same `select`, emit `occurrence: 1` for each (they consume sequentially); otherwise emit the 1-based rank of this span among identical matches in the original.
   e. **Containment guard (cross-op only):** if any emitted `with` contains the `select` of a *different* op in the same paragraph → `reason: 'replacement text re-introduces a selected phrase in paragraph <id> — ambiguous'`. Self-containment of a single op is allowed (the engine applies each op exactly once).
6. **Formatting pass per matched paragraph:** build per-character `FormatFlags` arrays for original and proposed plain text (using item positions; formatting from `revision` items is excluded — already rejected above). Find maximal spans where flags differ. If a span's underlying text also differs (text-change region) → the replace_text op's `with` already carries the new fmt (step 5d serializes the span's tags) — only reject when a fmt-change span strictly overlaps an UNCHANGED region adjacent to a text change: `reason: 'formatting change overlaps a text change in paragraph <id> — separate the edits'`. Pure fmt-change spans (text unchanged) → one `format_text(at, select=span text, occurrence per step 5d rules, ...changed flags only)`.
7. **Style pass per matched paragraph:** `styleClass` (or heading tag mapped: `hN` → `'Heading' + N`, `p` → `styleClass`) differs → `format_paragraph(at, style=<proposed>)`; a change to *no* style → `reason: 'cannot clear a paragraph style through the projection'`.
8. Reject changes to `attrs.ord`, `attrs.num`, `attrs['break-ins']`, `attrs['break-del']` — `reason: 'projection-protected attribute changed in paragraph <id>'`.
9. Order ops by document position (deletes at first deleted position, inserts at their anchor position, content ops at their paragraph position, formatting and style ops immediately after the paragraph's text ops). Return `{ ok: true, ops }`.

`allocateInsertId(sourceSha, index, existing)`: `h = sha256(sourceSha + '\0' + String(index))`; `v = 1 + (parseInt(h.slice(0, 8), 16) % 0x7FFFFFFF)`; format as 8 uppercase hex; if in `existing` (or `00000000`), increment mod range and retry (bounded loop, then error).

- [ ] **Step 4: Run focused and package tests**

Run: `npm run build && node --test test/projection-diff.test.mjs && npm test`

- [ ] **Step 5: Commit**

```bash
git add packages/docxdriver-pi/src/projection-diff.ts packages/docxdriver-pi/test/projection-diff.test.mjs
git commit -m "feat(pi): projection reconciler compiling string diffs to typed core ops"
```

---

### Task 6: Version 2 — Raw Projection String Surface

**Files:**
- Create: `packages/docxdriver-pi/src/python-string-prelude.ts`
- Create: `packages/docxdriver-pi/src/python-string-deriver.ts`
- Create: `packages/docxdriver-pi/src/python-string-extension.ts`
- Create: `packages/docxdriver-pi/test/python-string-extension.test.mjs`

**Interfaces:**
- Consumes: Task 2 host/deriver types, Task 4 `parseProjection`/`serializeProjection`, Task 5 `diffProjections`.
- Produces:
  - `PYTHON_PRELUDE`/`PYTHON_TYPE_STUBS`/`PYTHON_API_REFERENCE` for the string surface.
  - `stringSurfaceDeriver(): SurfaceDeriver` (name `'string'`).

- [ ] **Step 1: Write the failing V2 surface tests**

`test/python-string-extension.test.mjs` (same stub-Pi harness and fixture setup as Task 3):

```js
test('v2: string replace, preview, later commit', async () => {
  const cwd = await freshDir();
  const first = await call('python', { code: [
    'original = docx_read("contract.docx")',
    'draft = original.replace("The notice period is thirty days.", "The notice period is sixty days.")',
    'draft = re.sub(r"Payment is due within \\\\d+ days", "Payment is due within 45 days", draft)',
    'preview = docx_preview("contract.docx", original=original, proposed=draft)',
    'print("KEY=" + preview.key)',
  ].join('\n') }, cwd);
  const key = first.text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];
  const second = await call('python', { code: [
    'docx_commit("contract.docx", preview.key, proposed=draft)',
    'print("committed")',
  ].join('\n') }, cwd);
  assert.match(second.text, /committed/);
  const final = await renderMarkup(join(cwd, 'contract.docx'));
  assert.match(final, /sixty days/);
  assert.match(final, /Payment is due within 45 days/);
});
```

Also test: (a) a stale `original` (different from the current render) blocks preview with `original does not match`; (b) `draft = draft.replace("sixty days", "ninety days")` between preview and commit blocks with `state changed after preview`; (c) an invented paragraph id in `draft` blocks with the reconciler's reason and no write; (d) `docx_preview` requires both `original` and `proposed` keyword args (TypeError otherwise).

- [ ] **Step 2: Run and observe RED**

Run: `npm run build && node --test test/python-string-extension.test.mjs`
Expected: failure — `dist/python-string-extension.js` does not exist.

- [ ] **Step 3: Implement the V2 prelude**

```python
import re as _re

@dataclass
class PreviewResult:
    key: str

def docx_create(path: str, html: str):
    return _docx_create(path, html)

def docx_read(path: str, view: str = "markup") -> str:
    return _docx_read(path, view)

def docx_help(topic: str | None = None) -> str:
    return _docx_help(topic)

def docx_preview(path: str, *, original: str, proposed: str):
    return _docx_preview(path, {"original": original, "proposed": proposed})

def docx_commit(path: str, key: str, *, proposed: str):
    _docx_commit(path, key, {"proposed": proposed})
    return None
```

`PYTHON_TYPE_STUBS` declares the four public functions (keyword-only `original`/`proposed`), `PreviewResult`, `_docx_preview`/`_docx_commit` taking a `dict`, and imports `re` so user snippets type-check. `PYTHON_API_REFERENCE` shows the spec's V2 example verbatim.

- [ ] **Step 4: Implement the V2 deriver**

`stringSurfaceDeriver()`:

- `digest(state)`: `null` unless `state` is an object with a string `proposed`; else `sha256(state.proposed)`.
- `derive(state, source)`:
  1. `state.original` must be present at preview (the prelude always sends it) and must equal `source.markup` structurally: `serializeProjection(parseProjection(state.original)) === serializeProjection(parseProjection(source.markup))` — on mismatch `{ ok: false, message: 'original does not match the current document — re-read with docx_read' }`. At commit the prelude sends only `proposed`; the deriver then uses `source.markup` as the original (safe because the gate already verified the source hash).
  2. `diffProjections(originalMarkup, proposedMarkup, { sourceSha: source.sha256 })` → on failure `{ ok: false, message: reason }`.
  3. Return `{ ok: true, plan: { base: 'sha256:' + source.sha256, author: 'docxdriver', change_mode: 'track', ops } }`.
- `verifyCommit(record, evidence)`: parse `evidence.postMarkup`; structural equality against the stored expected doc: `record.deriverData` holds `{ expected: ProjectionDoc }` captured at preview (parse of `proposed` with id-masking: id-less proposed paragraphs match any engine-allocated id in post). Equality = same paragraph count, same order, per paragraph: id equal or masked, tag/styleClass/attrs (ord excluded) equal, items deep-equal. Mismatch → message listing the first differing paragraph position. Store `deriverData` inside `derive` is not possible (derive has no store access) — instead the **host** stores `deriverData` on the record: extend Task 2's `_docx_preview` to capture `deriverData: deriver.capture?.(state)` — add an optional `capture(state): unknown` to `SurfaceDeriver` (undefined for V1/V3), stored on the record. Update Task 2's interface line accordingly (append-only).

- [ ] **Step 5: Implement the V2 entry point**

`src/python-string-extension.ts` mirrors Task 3's entry with `stringSurfaceDeriver()` and V2 guidance: work from `docx_read` output only, never hand-edit `id`/`ord`/`num` attributes, prefer `re.sub`/`str.replace` with surrounding context, keep `original` from the same read, preview before commit in a later call.

- [ ] **Step 6: Run offline verification**

Run: `npm run build && node --test test/python-string-extension.test.mjs && npm test`

- [ ] **Step 7: Commit**

```bash
git add packages/docxdriver-pi/src/python-string-prelude.ts packages/docxdriver-pi/src/python-string-deriver.ts \
  packages/docxdriver-pi/src/python-string-extension.ts packages/docxdriver-pi/test/python-string-extension.test.mjs
git commit -m "feat(pi): Version 2 raw projection string REPL surface"
```

---

### Task 7: Version 3 — Structured Python Document Model Surface

**Files:**
- Create: `packages/docxdriver-pi/src/python-model-prelude.ts`
- Create: `packages/docxdriver-pi/src/python-model-deriver.ts`
- Create: `packages/docxdriver-pi/src/python-model-extension.ts`
- Create: `packages/docxdriver-pi/test/python-model-extension.test.mjs`

**Interfaces:**
- Consumes: Task 2 host/deriver types, Task 4 parser/serializer.
- Produces:
  - `PYTHON_PRELUDE`/`PYTHON_TYPE_STUBS`/`PYTHON_API_REFERENCE` for the model surface.
  - `modelSurfaceDeriver(): SurfaceDeriver` (name `'model'`).

- [ ] **Step 1: Write the failing V3 surface tests**

`test/python-model-extension.test.mjs`:

```js
test('v3: traversal, selection, preview, later commit', async () => {
  const cwd = await freshDir();
  const first = await call('python', { code: [
    'doc = docx_open("contract.docx")',
    'for para in doc.paragraphs:',
    '    if "thirty days" in para.text:',
    '        para.replace("thirty days", "sixty days")',
    'doc.select("sixty days").bold = True',
    'preview = docx_preview(doc)',
    'print("KEY=" + preview.key)',
  ].join('\n') }, cwd);
  const key = first.text.match(/KEY=(p1:sha256:[0-9a-f]{64})/)[1];
  const second = await call('python', { code: [
    'docx_commit(doc, preview.key)',
    'print("committed")',
  ].join('\n') }, cwd);
  assert.match(second.text, /committed/);
  const final = await renderMarkup(join(cwd, 'contract.docx'));
  assert.match(final, /<b>sixty days<\/b>/);
});
```

Also test: (a) `para.replace` with stale expected text raises `ValueError` mentioning the paragraph id; (b) `doc.select("missing")` raises `ValueError`; (c) mutation after preview (`para.replace(...)` again, or `doc.select("sixty days").italic = True`) blocks commit with `state changed after preview`; (d) `para.text = "..."` produces a replace_paragraph (assert via the preview report ops summary or via final markup); (e) `para.delete()` + `doc.insert_after(para, "New paragraph")` in one preview commit atomically; (f) `for match in doc.regex(r"\\b\\d+ days\\b"): match.bold = True` formats every match.

- [ ] **Step 2: Run and observe RED**

Run: `npm run build && node --test test/python-model-extension.test.mjs`
Expected: failure — `dist/python-model-extension.js` does not exist.

- [ ] **Step 3: Implement the model prelude**

The prelude defines (Python-native classes, no JS objects — the host returns only dicts/lists/strings):

```python
@dataclass
class PreviewResult:
    key: str

class Selection:
    """A targeted span inside one paragraph; attribute writes record format changes."""
    def __init__(self, paragraph, expected_text, occurrence):
        self._paragraph = paragraph
        self.expected_text = expected_text
        self.occurrence = occurrence      # 1-based within the paragraph
        self._fmt = {}
    def _set(self, name, value):
        self._fmt[name] = value
    @property
    def bold(self): return self._fmt.get("bold", False)
    @bold.setter
    def bold(self, value): self._set("bold", value)
    # italic, underline, strike, superscript, subscript: identical pattern

class Paragraph:
    def __init__(self, doc, data):
        self._doc = doc
        self.id = data["id"]
        self._original_markup = data["markup"]
        self._original_text = data["text"]
        self._text = data["text"]
        self._style = data.get("style")
        self._deleted = False
        self.text_changes = []   # {"expected": str, "new": str, "occurrence": int|None}
        self.fmt_changes = []    # {"expected": str, "occurrence": int|None, "fmt": dict}
        self.style_changes = []  # [str]
    @property
    def text(self): return self._text
    @text.setter
    def text(self, value):
        if self._deleted: raise ValueError(f"paragraph {self.id} is deleted")
        self._text = value
        self.text_changes = [{"expected": self._text, "new": value, "occurrence": None}]
        self.fmt_changes = []
    def replace(self, old, new, occurrence=None):
        if self._deleted: raise ValueError(f"paragraph {self.id} is deleted")
        if occurrence is None:
            count = self._text.count(old)
            if count == 0: raise ValueError(f"replace: {old!r} not found in paragraph {self.id}")
            if count > 1: raise ValueError(f"replace: {old!r} occurs {count} times in paragraph {self.id} — pass occurrence=1..{count}")
            occurrence = 1
        elif not (isinstance(occurrence, int) and occurrence >= 1):
            raise ValueError(f"replace: occurrence must be a positive integer")
        spans = [i for i in range(len(self._text)) if self._text.startswith(old, i)]
        if len(spans) < occurrence: raise ValueError(f"replace: occurrence {occurrence} out of range in paragraph {self.id}")
        self.text_changes.append({"expected": old, "new": new, "occurrence": occurrence})
        self._text = self._text[:spans[occurrence - 1]] + new + self._text[spans[occurrence - 1] + len(old):]
    def delete(self):
        if self._deleted: raise ValueError(f"paragraph {self.id} already deleted")
        self._deleted = True
        self.text_changes = []; self.fmt_changes = []
    @property
    def style(self): return self._style
    @style.setter
    def style(self, value):
        self._style = value
        self.style_changes = [value]

class Document:
    def __init__(self, data):
        self.path = data["path"]
        self.source_hash = data["source_hash"]
        self.paragraphs = [Paragraph(self, p) for p in data["paragraphs"]]
        self.insertions = []  # {"anchor": str, "position": "after"|"before", "text": str, "style": str|None}
    def _find(self, text, occurrence):
        found = []
        for para in self.paragraphs:
            if para._deleted: continue
            count = para._text.count(text)
            for i in range(1, count + 1):
                found.append((para, i))
        if occurrence is None and len(found) == 0: raise ValueError(f"select: {text!r} not found in any paragraph")
        if occurrence is None and len(found) > 1: raise ValueError(f"select: {text!r} is ambiguous — pass occurrence=1..{len(found)}")
        return found[0] if occurrence is None else found[occurrence - 1]
    def select(self, text, occurrence=None):
        para, occ = self._find(text, occurrence)
        return Selection(para, text, occ)
    def regex(self, pattern):
        for para in self.paragraphs:
            if para._deleted: continue
            for m in _re.finditer(pattern, para._text):
                yield Selection(para, m.group(0), 1 + para._text[:m.start()].count(m.group(0)))
    def insert_after(self, para, text, style=None): self.insertions.append({"anchor": para.id, "position": "after", "text": text, "style": style})
    def insert_before(self, para, text, style=None): self.insertions.append({"anchor": para.id, "position": "before", "text": text, "style": style})
    def serialize(self):
        return {
            "path": self.path, "source_hash": self.source_hash,
            "paragraphs": [{
                "id": p.id, "original_markup": p._original_markup, "original_text": p._original_text,
                "text": p._text, "style": p._style, "deleted": p._deleted,
                "text_changes": p.text_changes, "fmt_changes": p.fmt_changes,
            } for p in self.paragraphs],
            "insertions": self.insertions,
        }

def docx_open(path: str):
    return Document(_docx_open(path))

def docx_preview(doc: Document):
    return _docx_preview(doc.path, doc.serialize())

def docx_commit(doc: Document, key: str):
    _docx_commit(doc.path, key, doc.serialize())
    return None
```

Mutation-time validation in Python gives the repair workflow its diagnostics; the deriver re-validates authoritatively at preview. `doc.regex` uses `re.finditer` with `re.escape`-free semantics (raw pattern), occurrences counted per paragraph. `_docx_open(path)` host callback (implemented in this task's deriver host wiring — see Step 4) returns `{ path, source_hash, paragraphs: [{id, text, style, markup}], markup }` parsed from the rendered projection via `parseProjection`.

- [ ] **Step 4: Implement the model deriver**

`modelSurfaceDeriver()`:

- `digest(state)`: `null` unless `state` is an object with `paragraphs` and `insertions`; else `sha256(canonicalJson(state))` (the full serialization — any mutation changes the digest).
- `derive(state, source)`:
  1. `state.source_hash !== 'sha256:' + source.sha256` → `{ ok: false, message: 'document changed since open — reopen with docx_open' }`.
  2. Parse each paragraph's `original_markup` with `parseProjection` (paragraph-level wrap: `parseProjection(p.original_markup).paragraphs[0]`); mismatch count vs `state.paragraphs` → invalid state.
  3. **Replay per paragraph** (running text = `plainText` of the parsed items, then updated by text changes; fmt arrays rebuilt after each text change):
     - if `deleted`: no other changes allowed on it (else blocked `deleted paragraph <id> has pending changes`).
     - text_changes in recorded order: find the `occurrence`-th match of `expected` in the running text; not found → `{ ok: false, message: 'stale selection in paragraph <id>: expected {expected!r} at occurrence {n}, current text is {running!r}' }`; emit `replace_text(at=id, select=expected, with=new, occurrence=n)`; apply to running text. Multiple text changes are validated against the running text (this is the replay that makes `para.replace` chains correct).
     - if `text_changes` has a whole-paragraph entry (from the `text` setter — `text_changes` is a single-element list whose expected equals the original text): emit `replace_paragraph(at=id, with=escaped new text)` instead, and require no other text/fmt changes on the paragraph (else blocked).
     - fmt_changes in order: validate `expected` at `occurrence` in the *current* running text (post-text-changes); emit `format_text(at=id, select=expected, occurrence, ...fmt)`; formatting does not change the running text.
     - style_changes: last value → `format_paragraph(at=id, style=value)`.
  4. Insertions: one `insert_paragraph(at=anchor, position, with=escaped text, style?)` each, in recorded order, chained with `allocateInsertId` when consecutive insertions share an anchor (Task 5's helper) — engine-allocated otherwise.
  5. Deleted paragraphs (in document order) → one `delete_paragraphs(at=[...])` op at the first deleted position.
  6. Ops in document order (paragraph order, then insertion positions, delete at first deleted position). Return the plan with `author: 'docxdriver'`, `change_mode: 'track'`.
  7. Compute the expected post-commit `ProjectionDoc` (original parsed items + applied text/fmt/style changes + insertions as id-less paragraphs + deleted paragraphs removed) and return it as `{ ok: true, plan, expected }` — extend `DeriveResult` with `expected?: ProjectionDoc` (append-only).
- `capture(state)`: `{ expected }` from the last derive — the host stores it in `record.deriverData` (Task 6's `capture` hook; V3 sets it).
- `verifyCommit(record, evidence)`: parse `evidence.postMarkup`; structural equality against `record.deriverData.expected` with insert-id masking (same rule as Task 6); mismatch → message with the first differing paragraph.

- [ ] **Step 5: Implement the V3 entry point and host wiring**

`src/python-model-extension.ts` mirrors Tasks 3/6 with `modelSurfaceDeriver()`, plus guidance: open once with `docx_open`, mutate paragraph attributes/selections, preview, commit later, reopen after any external change. Add `_docx_open` to the host callback set in `python-host.ts` (Task 2 file — append-only): `_docx_open(path)` returns `{ path, source_hash, paragraphs, markup }` built from `runRead` markup + `parseProjection`; the host must also accept derivers that declare `needsOpen` — simplest: always register `_docx_open`; derivers other than model ignore it.

- [ ] **Step 6: Run offline verification**

Run: `npm run build && node --test test/python-model-extension.test.mjs && npm test`

- [ ] **Step 7: Commit**

```bash
git add packages/docxdriver-pi/src/python-model-prelude.ts packages/docxdriver-pi/src/python-model-deriver.ts \
  packages/docxdriver-pi/src/python-model-extension.ts packages/docxdriver-pi/test/python-model-extension.test.mjs
git commit -m "feat(pi): Version 3 structured Python document model REPL surface"
```

---

### Task 8: Conformance Corpus and Trial Harness

**Files:**
- Create: `packages/docxdriver-pi/test/repl-fixtures.mjs`
- Create: `packages/docxdriver-pi/test/repl-trial.mjs`

**Interfaces:**
- Consumes: `executeRequest`/`init` from `docxdriver` (for fixture creation and final-document verification), the three extension entry points, `PythonRepl`.
- Produces:
  - `createContractFixture(cwd): Promise<string>` — writes `contract.docx` (returns its path). `createComplexFixture(cwd): Promise<string>` — copies `test-docs/ctnf-18690238-data-stream.docx` (path resolved from the package root). `zipPartHashes(abs): Promise<Map<string, string>>` — tiny central-directory reader (`node:zlib` `inflateRawSync`, ~70 lines, read-only, no deps) mapping entry name → sha256 of inflated bytes.
  - `makeTrial(surface)` — a `ReplTrial` wrapper around a `PythonRepl` that records `{ surface, task, toolCalls, previewAttempts, commitAttempts, blockedCount, recoveries, pythonChars, wallMs, plans: string[] }` per scripted interaction; `trial.execute(code, host)` increments counters and catches `reset` → `recoveries` + store clear.

Contract fixture content (created via `runCreate` html — ids are read dynamically from the rendered markup, never hard-coded):

```html
<h1>Service Agreement</h1>
<p>The notice period is thirty days.</p>
<p>Payment is due within 30 days of invoice receipt.</p>
<p>Payment is due within 45 days of invoice receipt.</p>
<p>Payment is due within 60 days of invoice receipt.</p>
<p>Confidential Information means any information disclosed by either party.</p>
<p>The parties agree that the terms shall govern, and the terms shall prevail, and the terms shall bind, and the terms shall endure.</p>
<p>Net 30. Net 45. Net 60. Net 90.</p>
<p>The warranty period is twelve months from delivery.</p>
<p>This paragraph will be replaced entirely.</p>
<p>This paragraph anchors an insertion.</p>
<p>This paragraph will be deleted.</p>
<p>Final paragraph. Signatures follow.</p>
```

The fixture's `thirty days` phrase is bold (`<b>thirty days</b>`), so Task 1 exercises formatting semantics through replacement. Engine ground truth: the deleted span keeps its runs/formatting; the replacement runs carry the `with` content's own dialect fmt (plain `with` inserts plain runs). Therefore: the V2 adapter keeps the `<b>` tags in the draft (the reconciler serializes the proposed span's fmt into `with`, preserving bold); the V1 adapter authors `with_="<b>sixty days</b>"`; the V3 adapter does `para.replace("thirty days", "sixty days")` plus `doc.select("sixty days").bold = True`. All three final documents then contain `<b>sixty days</b>` and the plans are semantically equivalent.

- [ ] **Step 1: Write failing fixture tests**

`test/repl-fixtures.test.mjs` (or fold into `repl-conformance.test.mjs` from Task 9 — prefer folding): create the contract fixture, assert `parseProjection(renderMarkup)` has 13 paragraphs, ids unique and `/^[0-9A-F]{8}$/`, plain texts match the table above, and `zipPartHashes` on the complex fixture contains `word/document.xml` plus at least one media entry.

- [ ] **Step 2: Run and observe RED**

Run: `npm run build && node --test test/repl-conformance.test.mjs` — fails: files do not exist.

- [ ] **Step 3: Implement the corpus generator and ZIP reader**

As specified in the interfaces. The tiny ZIP reader walks the end-of-central-directory record (EOCD signature `0x06054b50`), reads the central directory entries (signature `0x02014b50`), and for each entry inflates the local header's data (`0x04034b50`, local header offset from the central entry, skip local header + name + extra, `inflateRawSync` when method 8, verbatim when method 0). No writes, no deps.

- [ ] **Step 4: Implement the trial harness**

`makeTrial(surface)` returns `{ trial, host }` where `host = createPythonDocxHost(cwd, notices, deriver, store, () => repl.executionNumber)`. `trial.execute(code)` records `toolCalls += 1`, `pythonChars += code.length`, `wallMs += elapsed`, and when the execution returns `reset: true` clears the store and increments `recoveries`. `trial.planFor(taskId)` stores the canonical plan JSON of the first successful preview of that task (host preview notice carries `canonicalPlan` via a `plan` field on the notice — append `plan?: string` to `PythonHostNotice`, populated by the host from `canonicalJson(derived.plan)`).

- [ ] **Step 5: Run and commit**

Run: `npm run build && node --test test/repl-conformance.test.mjs && npm test`

```bash
git add packages/docxdriver-pi/test/repl-fixtures.mjs packages/docxdriver-pi/test/repl-trial.mjs
git commit -m "test(pi): conformance corpus, ZIP part-hash reader, and trial harness"
```

---

### Task 9: Deterministic Offline Conformance Suite (10 Tasks × 3 Surfaces)

**Files:**
- Create: `packages/docxdriver-pi/test/repl-conformance.test.mjs`

**Interfaces:**
- Consumes: Task 8 fixtures/trial harness, Tasks 3/6/7 preludes + derivers + extension factories, Task 4 parser, `executeRequest` for final verification.
- Produces: `test-results/repl-conformance.json` — per task × surface: `{ surface, task, success, toolCalls, previewAttempts, commitAttempts, blockedCount, recoveries, pythonChars, wallMs, canonicalPlan }` plus a cross-surface `equivalent: boolean` per task.

- [ ] **Step 1: Write the conformance tests, one block per task**

Each task script runs against a fresh fixture copy per surface (same bytes → same ids). The scripted actions are the "agent" (they mirror what the live trials will prompt). Write all ten; representative bodies:

```js
const TASKS = {
  'task1-exact-text-correction': async (ctx) => {            // replace one unique phrase, preserve formatting
    await ctx.run([
      'doc = docx_open("contract.docx")',
      'para = next(p for p in doc.paragraphs if "notice period" in p.text)',
      'para.replace("thirty days", "sixty days")',
      'preview = docx_preview(doc)',
      'print("KEY=" + preview.key)',
    ]);
    const key = ctx.lastKey();
    await ctx.run([`docx_commit(doc, ${JSON.stringify(key)})`, 'print("committed")']);
    const final = await ctx.render();
    assert.match(final, /sixty days/);
    assert.ok(!final.includes('thirty days'));
    assert.match(final, /<b>sixty days<\/b>/);               // formatting preserved
  },

  'task2-repeated-bulk-edit': async (ctx) => {               // every date match in one paragraph set
    await ctx.run([
      'doc = docx_open("contract.docx")',
      'for para in doc.paragraphs:',
      '    if "Payment is due within" in para.text:',
      '        para.replace(para.text[para.text.index("within") + 7:].strip(), "45 days")',
      'preview = docx_preview(doc)',
      'print("KEY=" + preview.key)',
    ]);
    ...commit + assert all three paragraphs now say "within 45 days"...
  },

  'task3-ambiguous-occurrence': async (ctx) => {             // only the third occurrence
    await ctx.run([
      'doc = docx_open("contract.docx")',
      'para = next(p for p in doc.paragraphs if "the terms shall" in p.text)',
      'para.replace("the terms", "THE TERMS", occurrence=3)',
      'preview = docx_preview(doc)',
      'print("KEY=" + preview.key)',
    ]);
    ...commit + assert exactly one "THE TERMS"...
  },

  'task4-arbitrary-formatting-selection': async (ctx) => {   // bold + underline, no text change
    await ctx.run([
      'doc = docx_open("contract.docx")',
      'sel = doc.select("warranty period")',
      'sel.bold = True',
      'sel.underline = True',
      'preview = docx_preview(doc)',
      'print("KEY=" + preview.key)',
    ]);
    ...commit + assert `<b><u>warranty period</u></b>` (tag order per renderer)...;
  },

  'task5-paragraph-structure': async (ctx) => {              // insert + replace + delete
    await ctx.run([
      'doc = docx_open("contract.docx")',
      'anchor = next(p for p in doc.paragraphs if "anchors an insertion" in p.text)',
      'repl = next(p for p in doc.paragraphs if "replaced entirely" in p.text)',
      'gone = next(p for p in doc.paragraphs if "will be deleted" in p.text)',
      'doc.insert_after(anchor, "Inserted paragraph.")',
      'repl.text = "Replacement paragraph."',
      'gone.delete()',
      'preview = docx_preview(doc)',
      'print("KEY=" + preview.key)',
    ]);
    ...commit + assert order, replacement text, deletion absent...;
  },

  'task6-mixed-atomic-edit': async (ctx) => {                // text + format + insert + delete in one preview
    ...combine task 1/3/4/5 elements in one preview, assert atomic commit and report counts...;
  },

  'task7-failed-preview-repair': async (ctx) => {            // stale selector → diagnostic → repair → retry
    await ctx.run([
      'doc = docx_open("contract.docx")',
      'para = next(p for p in doc.paragraphs if "notice period" in p.text)',
      'try:',
      '    para.replace("missing phrase", "x")',
      'except ValueError as e:',
      '    print("EXPECTED-ERROR", e)',
      'para.replace("thirty days", "sixty days")',
      'preview = docx_preview(doc)',
      'print("KEY=" + preview.key)',
    ]);
    assert.match(ctx.text, /EXPECTED-ERROR/);
    ...commit + verify...;
  },

  'task8-source-race': async (ctx) => {                      // external modify between preview and commit
    await ctx.previewOnly();                                  // preview, no commit
    await ctx.tamper(bytes => Buffer.concat([bytes.subarray(0, 40), Buffer.from('XX'), bytes.subarray(40)]));
    await ctx.run([`docx_commit(doc, ${JSON.stringify(ctx.lastKey())})`, 'print("done")']);
    assert.match(ctx.text, /source changed after preview/);
    assert.equal(await ctx.rawChanged(), false);              // tampered bytes preserved
  },

  'task9-mutation-after-preview': async (ctx) => {           // change model state after preview
    await ctx.previewOnly();
    await ctx.run(['para.replace("sixty days", "ninety days")']);  // mutate
    await ctx.run([`docx_commit(doc, ${JSON.stringify(ctx.lastKey())})`, 'print("done")']);
    assert.match(ctx.text, /state changed after preview/);
  },

  'task10-package-preservation': async (ctx) => {            // unsupported structures + untouched parts
    const before = await zipPartHashes(await ctx.complexFixture());
    ... single replace_text on a paragraph that renders cleanly (no protected spans) ...
    const after = await zipPartHashes(...);
    for (const [name, hash] of before) {
      if (name === 'word/document.xml') continue;
      assert.equal(after.get(name), hash, `part ${name} must be byte-identical`);
    }
  },
};
```

Each surface has a per-task adapter that runs the same *intent* in its own idiom (V1: dataclasses with `at` resolved from `docx_read` ids; V2: `original.replace(...)`/`re.sub` strings; V3: the model code above). The adapter for V1 must look up the paragraph ids first (a `docx_read` call) and build the `Plan` from them.

- [ ] **Step 2: Run and observe RED**

Run: `npm run build && node --test test/repl-conformance.test.mjs`
Expected: failures across tasks (surfaces not wired).

- [ ] **Step 3: Implement the runner and equivalence assertion**

`repl-conformance.test.mjs` structure: for each surface, `makeTrial`, fresh fixture copy, run the task adapter, collect metrics; after all tasks, assert **equivalence per task**: for every task where all three surfaces succeeded, (a) the final rendered documents must be structurally equal after id normalization (parse + serialize with engine-allocated ids masked — same rule as the V2/V3 verifyCommit walk), and (b) where two surfaces' canonical plans are directly comparable op-for-op (same op type, select, with, at after masking), they must match. Plan deep-equality is NOT asserted for task 1 (V1 authors a wider select/with than the reconciler's narrowest span) and task 2 (occurrence forms may differ); for tasks 3-10 assert op-level equality after masking base and as/allocated ids. Write the metrics table to `test-results/repl-conformance.json` and print a summary line per task.

- [ ] **Step 4: Run the full suite and commit**

Run: `npm run build && node --test test/repl-conformance.test.mjs && npm test && cargo test --workspace`
Expected: zero failures; conformance JSON shows `success: true` for all 30 cells and `equivalent: true` for all 10 tasks.

```bash
git add packages/docxdriver-pi/test/repl-conformance.test.mjs
git commit -m "test(pi): offline conformance suite — 10 tasks across 3 REPL surfaces"
```

---

### Task 10: Live-Agent Trials, Documentation, and Evaluation

**Files:**
- Create: `packages/docxdriver-pi/test/pi-e2e/repl-tasks.json`
- Create: `packages/docxdriver-pi/test/pi-e2e/repl-live.mjs`
- Modify: `packages/docxdriver-pi/README.md`

**Interfaces:**
- Consumes: the three extension entry points, the contract fixture, `executeRequest` verification, `PI_E2E_LIVE`/`PI_E2E_MODEL`/`PI_E2E_SURFACES`/`PI_E2E_REPS` env vars.
- Produces: `test/pi-e2e/results/<run-id>/` containing per-trial JSONL (`tool_execution_start`/`tool_execution_end` events), the final verified docx, and a `summary.json` with the evaluation metrics; the prompts are identical across surfaces for each task.

- [ ] **Step 1: Write the live-task manifest**

`repl-tasks.json` — ten entries, one per conformance task, each with `{ id, prompt, fixture: 'contract' | 'complex', verify: 'commit' | 'blocked' }`. Task 7's prompt instructs the model to begin with a deliberately stale selector and repair it; tasks 8 and 9 instruct the model to preview, then the harness performs the external tamper/mutation between the preview tool call and the next call (harness-injected, not prompted), then the model is asked to attempt the commit and observe the rejection.

- [ ] **Step 2: Write the live harness**

`repl-live.mjs` (patterned on the existing `pi-e2e/run.mjs`): for each surface (configurable via `PI_E2E_SURFACES=plan,string,model`) × task × rep (default `PI_E2E_REPS=1`, full matrix `3`): create a fresh temp cwd, copy the fixture, spawn

```bash
pi --mode json -p --no-session -na --model "$PI_E2E_MODEL" \
  --no-extensions -e <abs path to the surface extension> \
  --no-skills --no-context-files --no-prompt-templates \
  --no-builtin-tools --tools python '<task prompt>'
```

collect events, count tool calls / preview attempts (tool calls whose text contains `preview key:` or `p1:sha256:`), blockedCount (text contains `── blocked ──`), wall time, verify the final docx with `executeRequest`, write `summary.json` with the spec's metric list. Guard: refuse to run without `PI_E2E_LIVE=1` and `PI_E2E_MODEL`.

- [ ] **Step 3: Document the three surfaces**

Add a `Three REPL surfaces (experimental)` section to `packages/docxdriver-pi/README.md` with the exact load pattern per surface:

```bash
pi --no-builtin-tools --tools python \
  -e ./packages/docxdriver-pi/src/python-plan-extension.ts
# python-string-extension.ts  → raw projection string surface
# python-model-extension.ts   → structured document model surface
```

Explain the shared commit protocol (preview stores path/source hash/canonical plan/core key/surface-state hash/execution number; commit only in a later execution with unchanged source and state), the subset of six ops, and the conformance + live-trial commands:

```bash
npm test
node --test test/repl-conformance.test.mjs
PI_E2E_LIVE=1 PI_E2E_MODEL=opencode-go/deepseek-v4-flash node test/pi-e2e/repl-live.mjs
```

- [ ] **Step 4: Run the smoke live trial (one surface × task 1)**

Run the harness with `PI_E2E_SURFACES=plan` and task 1 only; confirm the model previews (p1: key appears), commits in a later call, and the verifier accepts. Iterate on prompt wording only if the trial fails for harness reasons (never tune prompts per-surface).

- [ ] **Step 5: Run the full matrix and write the evaluation report**

Run the full `3 surfaces × 10 tasks × 3 reps` matrix. Produce `test/pi-e2e/results/<run-id>/summary.md` answering each spec evaluation metric (success, tool calls, preview attempts, recoveries, Python chars authored, wall time, invalid write attempts, affected-paragraph accuracy, formatting/package preservation, pre-commit explainability from the notice text, and the canonical-plan equivalence table). Treat Version 1 as the semantic oracle: any V2/V3 plan that differs from V1's for a task (after id masking) is a reconciler bug to fix in Tasks 5/7, not a per-surface variation.

- [ ] **Step 6: Full branch verification and commit**

```bash
cd "$(git rev-parse --show-toplevel)/.worktrees/python-repl-surfaces"
cargo test --workspace
cd packages/docxdriver-pi && npm test && npm run build
```

```bash
git add packages/docxdriver-pi/test/pi-e2e/repl-tasks.json \
  packages/docxdriver-pi/test/pi-e2e/repl-live.mjs packages/docxdriver-pi/README.md
git commit -m "feat(pi): live-agent trial harness and three-surface documentation"
```

---

## Self-Review

**Spec coverage:** shared foundation (persistent Monty session — T1; sandboxed execution — T1; current typed Rust Plan API — T2/T3; source-bound core preview keys — T2; cross-execution commit gate — T2; atomic writes + pre-write source verification — T2 via `writeDocxBytes`; identical preview/report formatting — T2 `renderNotices`; post-commit render verification — T2 driver + T3/T6/T7 derivers; same operation subset — Global Constraints + T3; benchmark tasks — T8/T9/T10) ✓. V1 (dataclasses generated from schema — T3; serialize to exact core Plan — T3; preview passes plan to runPlan — T2/T3; commit hashes normalized plan — T2; EditHtml removed — T1 constraints; plan-level author/change_mode — T3) ✓. V2 (canonical raw string — T4/T6; reconciler parse→diff→Plan — T5; mappings — T5 tests; rejects malformed/ambiguous — T5; never rebuilds DOCX from string — T5/T6) ✓. V3 (host returns plain data — T7; Python-native classes — T7; mutations record change set — T7; preview serializes to ops — T7; setter semantics table — T7; stale-selection rejection — T7; digest invalidation — T7/T2) ✓. Preview store six fields — T2 `PreviewRecord` ✓. Commit only in later execution + unchanged state — T2 gate ✓. Three entry points + shared factory — T3/T6/T7 ✓. Corpus tasks 1–10 — T9 (task 8/9 as rejection tests) ✓. Evaluation metrics — T9 metrics JSON + T10 summary ✓. Build order — matches T1→T10 ✓.

**Placeholder scan:** no TBD/TODO; every step carries code or an exact spec; the parser's algorithm is fully specified with reference behavior and tests; task scripts for all ten corpus tasks are specified (representative bodies written out; remaining bodies follow the same pattern with stated assertions). One deliberate, flagged deviation: Task 4 "recolor" is executed as bold+underline (color is not projection-expressible — verified in `html/parse.rs`/`render.rs`).

**Type consistency:** `PreviewRecord`/`SurfaceDeriver`/`DeriveResult`/`VerifyEvidence`/`PythonHostNotice` are defined once in Task 2 and referenced identically in Tasks 3/6/7; `capture(state)` is added in Task 6 and used by Task 7; `DeriveResult.expected` is added in Task 7; `PythonHostNotice.plan` added in Task 8 — all append-only, so earlier task files gain fields without signature churn. `diffProjections(originalMarkup, proposedMarkup, opts?: { sourceSha?: string })` is called with two args in Task 5's unit tests and with `{ sourceSha }` in Task 6; `allocateInsertId(sourceSha, index, existing)` defined in Task 5, reused in Task 7. Function names match across tasks (`createPythonDocxHost`, `parseProjection`, `serializeProjection`, `diffProjections`, `createPythonExtension`, `planSurfaceDeriver`/`stringSurfaceDeriver`/`modelSurfaceDeriver`).
