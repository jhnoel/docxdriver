/**
 * Version 1 experimental surface: Python-authored typed plans.
 *
 * Registers exactly one `python` tool backed by one lazily created,
 * persistent `PythonRepl` running the V1 prelude (typed operation dataclasses
 * + docx_* wrappers). The commit-key store lives at extension scope (survives
 * tool calls; keys are invalidated on session reset or shutdown). A
 * per-call host adapter binds the private `_docx_*` callbacks to that call's
 * cwd. Tool details carry only { status, reset, noticeCount, cancelled } —
 * never host notice payloads, receipt ids, core preview keys, Python state,
 * or document contents.
 *
 * Bounded host behavior and cancellation (Task 6):
 * - Named limits (`HostLimits`) bound code input, DOCX input, plan/report
 *   sizes, commit keys, Python output, and host callback duration; the control
 *   budget is reserved so Python stdout can never hide review/commit
 *   output (production invariant 9).
 * - The Pi `AbortSignal` is honored at entry (no runtime lookup, no feed),
 *   after runtime creation (only this call is discarded), and mid-feed the
 *   repl unwinds cooperatively. An abort that fires while a call is queued
 *   behind an in-flight feed returns the same rendered cancelled result
 *   (never a bare rejection). A cancelled call returns a rendered result
 *   with a `[cancelled]` marker and `details.status: 'cancelled'` so commit
 *   keys and commit outcomes stay visible (never a bare throw).
 * - If worker recovery fails (`MontyUnrecoverableError`), the broken runtime
 *   is discarded, the generation is bumped, and every commit key is
 *   invalidated; the next call creates fresh state.
 *
 *   pi --no-builtin-tools --tools python \
 *     -e ./packages/docxdriver-pi/src/python-plan-extension.ts
 */
import { DEFAULT_MAX_BYTES, DEFAULT_MAX_LINES, truncateHead, truncateTail } from '@earendil-works/pi-coding-agent';
import { Type } from 'typebox';
import type { ExtensionAPI } from '@earendil-works/pi-coding-agent';
import { MontyUnrecoverableError, PythonRepl } from './python-repl.js';
import type { PythonExecution } from './python-repl.js';
import { CommitKeyStore, DEFAULT_HOST_LIMITS, MAX_ESSENTIAL_NOTICES, createPlanPythonHost, protocolLinesFor, renderNotices, truncateUtf8 } from './python-host.js';
import type { HostLimits, PythonDocxHostOptions, PythonHostNotice } from './python-host.js';
import { PYTHON_API_REFERENCE, PYTHON_PRELUDE, PYTHON_TYPE_STUBS } from './python-plan-prelude.js';
import { QUOTE_API_REFERENCE, QUOTE_PYTHON_PRELUDE, QUOTE_PYTHON_TYPE_STUBS } from './python-quote-prelude.js';
import { formatBytes } from './io.js';

const TOOL_DESCRIPTION = 'Persistent sandboxed Python REPL that authors typed DOCX plans.';
const PROMPT_SNIPPET = 'Run stateful sandboxed Python authoring typed DOCX plans';
const PROMPT_GUIDELINES = [
  'Do all DOCX work through the python tool; never use other file tools on .docx files.',
  'Read first with docx_read(path) and address paragraphs by their id attribute; locate paragraphs with docx_find(path, query) (core match records with id/text).',
  'Author operations explicitly as dataclasses inside a Plan(author=..., change_mode="track"): ReplaceEquation/DeleteEquation/ReplaceText/FormatText/ReplaceParagraph/InsertParagraph/DeleteParagraphs, SetHeader/SetFooter/ClearHeader/ClearFooter/SetEvenAndOddHeaders, and CommentAdd/CommentReply/CommentSetStatus/CommentDelete.',
  'Compute operations from the docx_read output when useful: re is pre-imported, so build ops in Python loops over regex matches (see the API reference example).',
  'Review with docx_review(path, plan), inspect review.edits for the changed paragraphs in context, then commit only in a separate later python call with docx_commit(review.commit_key).',
  'Repair a blocked review by changing one attribute of an existing operation object, then review again.',
  'When quotation audit mode is enabled, put Quote/Term/Inline objects directly in operation with_ fields. Never copy QuoteResult.text into a plan string; review resolves the structured node and binds its source hash.',
];

const COMBINED_PRELUDE = `${QUOTE_PYTHON_PRELUDE}\n${PYTHON_PRELUDE}`;
const COMBINED_TYPE_STUBS = `${QUOTE_PYTHON_TYPE_STUBS}\n${PYTHON_TYPE_STUBS}`;
const COMBINED_API_REFERENCE = `${PYTHON_API_REFERENCE}\n${QUOTE_API_REFERENCE}`;

/** Cancellation marker line: reserved control output, never truncated away. */
const CANCELLED_MARKER = '[cancelled] python execution was aborted';

/**
 * Injectable extension seams for the Task 6 regressions; production callers
 * omit them. `limits` and `hostOptions` are merged once at registration and
 * threaded into every per-call host; `replFactory` replaces runtime
 * creation (e.g. a stub whose `execute` throws `MontyUnrecoverableError`).
 */
export interface PlanExtensionOptions {
  replFactory?: (prelude: string, stubs: string, options: { printCapBytes: number }) => Promise<PythonRepl>;
  limits?: Partial<HostLimits>;
  hostOptions?: Omit<PythonDocxHostOptions, 'signal' | 'limits'>;
}

/**
 * Pi registration and output policy for the V1 typed-plan REPL: registers
 * exactly the `python` tool plus an idempotent `session_shutdown` handler
 * that closes the runtime and invalidates every commit key.
 */
export function createPlanExtension(pi: ExtensionAPI, options?: PlanExtensionOptions): Promise<void> {
  const limits: HostLimits = { ...DEFAULT_HOST_LIMITS, ...options?.limits };
  const hostOptions = options?.hostOptions ?? {};
  const factory = options?.replFactory ?? ((prelude: string, stubs: string, opts: { printCapBytes: number }) => PythonRepl.create(prelude, stubs, opts));
  // One lazily created runtime per extension instance; cleared by shutdown.
  let repl: PythonRepl | undefined;
  // In-flight creation promise: concurrent first executions share it so only
  // one runtime is ever created (single-flight). Cleared on failure so a
  // later call retries instead of retaining a rejected promise.
  let pending: Promise<PythonRepl> | undefined;
  // Bumped by shutdown and by recovery discard: an in-flight creation that
  // resolves afterwards is discarded (closed, never kept) instead of leaking
  // a Monty worker, and keys stamped under an older generation expire.
  let generation = 0;
  let store = new CommitKeyStore({ maxRecords: limits.storeMaxRecords, maxBytes: limits.storeMaxBytes });

  const getRepl = (): Promise<PythonRepl> => {
    if (repl !== undefined) return Promise.resolve(repl);
    if (pending === undefined) {
      const gen = generation;
      const chain = factory(COMBINED_PRELUDE, COMBINED_TYPE_STUBS, { printCapBytes: limits.maxPrintBytes })
        .then(async (created) => {
          pending = undefined;
          if (gen !== generation) {
            // Shutdown raced this creation: close it and let the next call
            // start a fresh runtime.
            await created.close();
            throw new Error('PythonRepl creation cancelled by session shutdown');
          }
          repl = created;
          return created;
        })
        .catch((error) => {
          // Clear only this chain's own slot. While this chain awaited
          // close() during a shutdown race, a newer call may have installed
          // its own chain in `pending`; clearing unconditionally would let a
          // third chain start and split state across two runtimes.
          if (pending === chain) pending = undefined;
          throw error;
        });
      pending = chain;
    }
    return pending;
  };

  const shutdown = async (): Promise<void> => {
    generation += 1;
    const current = repl;
    repl = undefined;
    if (current !== undefined) await current.close();
    store.invalidateAll();
  };

  pi.on('session_shutdown', () => shutdown());

  pi.registerTool({
    name: 'python',
    label: 'Python DOCX REPL',
    description: [TOOL_DESCRIPTION, '', COMBINED_API_REFERENCE.trim()].join('\n'),
    promptSnippet: PROMPT_SNIPPET,
    promptGuidelines: PROMPT_GUIDELINES,
    parameters: Type.Object(
      { code: Type.String({ description: 'Python source for the persistent sandboxed DOCX REPL.' }) },
      { additionalProperties: false },
    ),
    async execute(_id, params, signal, _onUpdate, ctx) {
      const code = params.code;
      if (signal?.aborted) {
        // Prevent new host work: no runtime lookup, no feed, no execution
        // number consumed.
        return {
          content: [{ type: 'text', text: CANCELLED_MARKER }],
          details: { status: 'cancelled', cancelled: true, reset: false, noticeCount: 0 },
        };
      }
      if (Buffer.byteLength(code, 'utf8') > limits.maxCodeBytes) {
        return {
          content: [{ type: 'text', text: `code exceeds ${formatBytes(limits.maxCodeBytes)}` }],
          details: { status: 'error', reset: false, noticeCount: 0 },
        };
      }
      // Per-call, caller-owned notices: commit keys appear only as
      // model-visible rendered text (never in `details`); deterministic core
      // preview keys never appear anywhere.
      const notices: PythonHostNotice[] = [];
      const runtime = await getRepl();
      if (signal?.aborted) {
        // Runtime creation is shared/cached: discard only this call.
        return {
          content: [{ type: 'text', text: CANCELLED_MARKER }],
          details: { status: 'cancelled', cancelled: true, reset: false, noticeCount: 0 },
        };
      }
      const host = createPlanPythonHost(ctx.cwd, notices, store, () => runtime.executionNumber, () => generation, COMBINED_API_REFERENCE, {
        ...hostOptions,
        signal,
        limits,
      });
      let execution: PythonExecution;
      try {
        execution = await runtime.execute(code, host.externalLookup, signal);
      } catch (error) {
        if (error instanceof MontyUnrecoverableError) {
          // Worker replacement or prelude reload failed: the runtime is
          // broken. Discard it, bump the generation, and invalidate every
          // keys so the next call creates fresh state and old keys
          // report `commit key expired` — never retain a half-initialized
          // runtime or a commit key that could authorize against stale state.
          repl = undefined;
          generation += 1;
          store.invalidateAll();
          await runtime.close().catch(() => undefined);
          return {
            content: [{ type: 'text', text: 'python runtime failed and was discarded — session state lost; commit keys expired' }],
            details: { status: 'error', reset: true, noticeCount: notices.length },
          };
        }
        throw error;
      }
      if (execution.reset) {
        // The session crashed and was replaced: Python state (and therefore
        // every stored key's reviewed state) is gone. Invalidate the store
        // in place (the per-call host already holds this instance) so every
        // key reports `commit key expired` instead of `unknown key`.
        store.invalidateAll();
      }
      const cancelled = signal?.aborted === true;
      return {
        content: [{ type: 'text', text: renderExecution(execution, notices, { cancelled }) }],
        details: {
          status: cancelled ? 'cancelled' : execution.ok ? 'ok' : 'error',
          reset: execution.reset,
          noticeCount: notices.length,
          cancelled: cancelled ? true : undefined,
        },
      };
    },
  });

  return Promise.resolve();
}

export interface RenderExecutionOptions {
  /** The Pi abort signal fired: a `[cancelled]` marker is reserved control output. */
  cancelled?: boolean;
  /** Render caps; partial overrides merge over `DEFAULT_HOST_LIMITS`. */
  limits?: Partial<HostLimits>;
}

/**
 * Render Python stdout, the optional `=> value`, errors, the reset notice,
 * the optional cancellation marker, and host notices into one model-visible
 * text body.
 *
 * Control output (reset, cancellation marker, notices) is rendered FIRST
 * under a reserved, tail-truncated budget (`controlBudgetBytes`, newest
 * lines kept), so a huge Python stdout — even a single line that exceeds the
 * whole budget — can never hide the commit key, review state, commit
 * outcome, cancellation marker, or reset notice (production invariant 9).
 *
 * A compact protected protocol channel follows the verbose control block:
 * one bounded line per essential notice (review/commit and blocked-commit),
 * rebuilt from structured fields. The essential line count
 * (`MAX_ESSENTIAL_NOTICES`, newest-kept at push time) times the per-line
 * byte bound (`MAX_PROTOCOL_LINE_BYTES`) fits exactly inside
 * `protocolBudgetBytes`, so for host-produced notices the channel is never
 * truncated — commit keys, review state, and commit outcomes are
 * structurally protected from verbose-notice truncation (the truncation
 * applied here is defensive, for hand-built arrays only).
 *
 * Python stdout/value/error follows inside the remaining budget
 * (`DEFAULT_MAX_BYTES − control bytes − protocol bytes`), head-truncated
 * with Pi's exact truncation helpers. `[output truncated]` is appended once
 * when any block was truncated. Complete document output bypasses these
 * generic display caps; runtime print and memory limits still apply.
 */
export function renderExecution(execution: PythonExecution, notices: PythonHostNotice[], options?: RenderExecutionOptions): string {
  const limits: HostLimits = { ...DEFAULT_HOST_LIMITS, ...options?.limits };
  const cancelled = options?.cancelled === true;
  // Control lines are tail-truncated (newest kept), so the short, mandatory
  // signals (reset notice, cancellation marker) come LAST: they survive even
  // when a huge notice run pushes the control block over its budget.
  const controlParts: string[] = [];
  if (notices.length > 0) controlParts.push(renderNotices(notices));
  if (execution.reset) controlParts.push('session was reset after a crash: Python state was lost');
  if (cancelled) controlParts.push(CANCELLED_MARKER);
  const control = truncateTail(controlParts.join('\n'), { maxLines: limits.controlMaxLines, maxBytes: limits.controlBudgetBytes });
  // Compact protected protocol channel (see the doc comment above).
  const protocol = truncateTail(protocolLinesFor(notices).join('\n'), {
    maxLines: MAX_ESSENTIAL_NOTICES,
    maxBytes: limits.protocolBudgetBytes,
  });
  // Document output is already bounded by the runtime's collection/memory
  // limits. Preserve it through the generic display limits, including a read
  // saved in an earlier execution and later printed or returned.
  const containsProjection = (text: string): boolean => /<section data-docx-section=/.test(text);
  const valueText = execution.value === undefined ? '' : typeof execution.value === 'string' ? execution.value : JSON.stringify(execution.value);
  const documentOutput = notices.some(n => n.kind === 'document_read') || containsProjection(execution.stdout) || containsProjection(valueText.replace(/\\"/g, '"'));
  const pythonParts: string[] = [];
  if (execution.stdout !== '') pythonParts.push(execution.stdout.replace(/\n$/, ''));
  const duplicateValue = documentOutput && valueText !== '' && execution.stdout.includes(valueText);
  if (execution.value !== undefined && !duplicateValue) pythonParts.push(`=> ${renderValue(execution.value, documentOutput ? Number.MAX_SAFE_INTEGER : limits.maxValueRenderBytes)}`);
  if (!execution.ok && execution.error !== undefined) pythonParts.push(execution.error);
  const pythonText = pythonParts.join('\n');
  const pythonBudget = Math.max(0, DEFAULT_MAX_BYTES - control.outputBytes - protocol.outputBytes);
  const python = documentOutput
    ? { content: pythonText, truncated: false }
    : truncateHead(pythonText, { maxLines: DEFAULT_MAX_LINES, maxBytes: pythonBudget });
  const parts = [control.content, protocol.content, python.content].filter((part) => part !== '');
  const truncated = control.truncated || protocol.truncated || python.truncated;
  return truncated ? `${parts.join('\n')}\n[output truncated]` : parts.join('\n');
}

function renderValue(value: unknown, maxBytes: number): string {
  const text = typeof value === 'string' ? value : JSON.stringify(value);
  if (Buffer.byteLength(text, 'utf8') > maxBytes) {
    // UTF-8 byte-aware truncation: the cap is a byte bound and never splits
    // a multi-byte character.
    return `${truncateUtf8(text, maxBytes)}…(value truncated)`;
  }
  return text;
}

export default createPlanExtension;
