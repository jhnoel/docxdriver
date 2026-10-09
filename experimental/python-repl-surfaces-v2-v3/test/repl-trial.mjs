/**
 * Metrics-recording trial harness over one persistent PythonRepl.
 *
 * Each `makeTrial(surface, cwd)` owns one Monty session for one surface
 * (plan | string | model), one preview store, and one host bound to `cwd`.
 * Every `trial.execute(code)` records tool calls, authored Python characters,
 * wall time, preview/commit attempts and blocks (from the host-notice delta),
 * and crash resets (which also clear the store). `trial.planFor(taskId)`
 * captures the canonical plan JSON of the first successful preview of a task
 * via the host preview notice's `plan` field, so the conformance suite can
 * compare canonical core plans across surfaces.
 */
import { PythonRepl } from '../dist/python-repl.js';
import { PreviewStore, createPythonDocxHost } from '../dist/python-host.js';
import { PYTHON_PRELUDE as PLAN_PRELUDE, PYTHON_TYPE_STUBS as PLAN_STUBS } from '../dist/python-plan-prelude.js';
import { planSurfaceDeriver } from '../dist/python-plan-deriver.js';
import { PYTHON_PRELUDE as STRING_PRELUDE, PYTHON_TYPE_STUBS as STRING_STUBS } from '../dist/python-string-prelude.js';
import { stringSurfaceDeriver } from '../dist/python-string-deriver.js';
import { PYTHON_PRELUDE as MODEL_PRELUDE, PYTHON_TYPE_STUBS as MODEL_STUBS } from '../dist/python-model-prelude.js';
import { modelSurfaceDeriver } from '../dist/python-model-deriver.js';

const SURFACE_CONFIGS = {
  plan: { prelude: PLAN_PRELUDE, stubs: PLAN_STUBS, deriver: planSurfaceDeriver() },
  string: { prelude: STRING_PRELUDE, stubs: STRING_STUBS, deriver: stringSurfaceDeriver() },
  model: { prelude: MODEL_PRELUDE, stubs: MODEL_STUBS, deriver: modelSurfaceDeriver() },
};

/** The three experimental surfaces the conformance suite runs. */
export const REPL_SURFACES = Object.keys(SURFACE_CONFIGS);

/**
 * Create a trial for `surface` bound to `cwd`. Returns `{ trial, host }`
 * where `host` is the shared `createPythonDocxHost` result the trial drives
 * and callers may also drive directly.
 */
export async function makeTrial(surface, cwd) {
  const config = SURFACE_CONFIGS[surface];
  if (!config) throw new Error(`unknown surface ${surface} (expected one of ${REPL_SURFACES.join(', ')})`);
  const repl = await PythonRepl.create(config.prelude, config.stubs);
  const notices = [];
  const store = new PreviewStore();
  const host = createPythonDocxHost(cwd, notices, config.deriver, store, () => repl.executionNumber);

  let seen = 0;
  const countNotices = () => {
    for (const notice of notices.slice(seen)) {
      if (notice.kind === 'preview') trial.previewAttempts += 1;
      if (notice.kind === 'commit') trial.commitAttempts += 1;
      if (notice.kind === 'verify') trial.commitAttempts += 1; // post-commit verification problem
      if (notice.kind === 'blocked') {
        trial.blockedCount += 1;
        if (notice.phase === 'preview') trial.previewAttempts += 1;
        if (notice.phase === 'commit') trial.commitAttempts += 1;
      }
    }
    seen = notices.length;
  };

  const trial = {
    surface,
    /** Task id for the current scripted interaction (set by the caller). */
    task: null,
    toolCalls: 0,
    previewAttempts: 0,
    commitAttempts: 0,
    blockedCount: 0,
    recoveries: 0,
    pythonChars: 0,
    wallMs: 0,
    /** taskId → canonical plan JSON of the first successful preview. */
    plans: new Map(),
    notices,
    store,
    host,

    /** OS pid of the backing Monty worker (for crash-recovery tests). */
    get workerPid() {
      return repl.workerPid;
    },

    /** Run one scripted Python feed and fold its notices into the counters. */
    async execute(code) {
      const started = performance.now();
      const execution = await repl.execute(code, host.externalLookup);
      trial.wallMs += performance.now() - started;
      trial.toolCalls += 1;
      trial.pythonChars += code.length;
      if (execution.reset) {
        trial.recoveries += 1;
        store.clear();
      }
      countNotices();
      return execution;
    },

    /** The canonical plan JSON of the first successful preview, or null. */
    planFor(taskId) {
      if (!trial.plans.has(taskId)) {
        const first = notices.find((n) => n.kind === 'preview' && n.plan !== undefined);
        if (first !== undefined) trial.plans.set(taskId, first.plan);
      }
      return trial.plans.get(taskId) ?? null;
    },

    /** The preview key of the most recent successful preview, or null. */
    lastPreviewKey() {
      let key = null;
      for (const notice of notices) {
        if (notice.kind === 'preview' && notice.key !== undefined) key = notice.key;
      }
      return key;
    },

    /** The metric cell recorded for this trial (Task 9 output shape). */
    metrics() {
      return {
        surface: trial.surface,
        task: trial.task,
        toolCalls: trial.toolCalls,
        previewAttempts: trial.previewAttempts,
        commitAttempts: trial.commitAttempts,
        blockedCount: trial.blockedCount,
        recoveries: trial.recoveries,
        pythonChars: trial.pythonChars,
        wallMs: Math.round(trial.wallMs),
        plans: [...trial.plans.values()],
      };
    },

    async close() {
      await repl.close();
    },
  };

  return { trial, host };
}
