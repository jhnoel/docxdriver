/**
 * Version 1 DOCX host adapter for the typed-plan Python REPL.
 *
 * `_docx_review(path, state)` validates a typed plan without writing, returns
 * bounded per-edit context, and stores an immutable reviewed plan behind a
 * compact random `k1:` commit key. `_docx_commit(key)` performs the later
 * execution gate, source/path revalidation, core candidate verification, and
 * atomic write. The deterministic core preview key remains private to the
 * commit-key record.
 *
 * Every path is cwd-contained and every mutation runs inside Pi's per-file
 * queue. Host limits bound source bytes, plan size, per-edit context, the
 * aggregate review result, retained key records, notices, output, and callback
 * duration. Cancellation is cooperative; pre-write failures retain a reviewed
 * key for retry, while post-rename failures consume it and report truthfully.
 *
 * There is exactly one plan derivation path: `derivePlan` in
 * `python-plan-deriver.ts`. The V2 string reconciler and V3 model replay are
 * archived experiments and are never imported here.
 */
import { randomBytes } from 'node:crypto';
import { realpath } from 'node:fs/promises';
import { resolve } from 'node:path';
import { withFileMutationQueue } from '@earendil-works/pi-coding-agent';
import { canonicalPlanJson, ensureInit, planReportComplete, renderDocxBytes, runCreate, runFind, runHelp, runPlan, runRead } from './engine.js';
import { DocumentTooLargeError, PathChangedError, PostRenameError, SourceChangedError, createDocxBytes, formatBytes, readBackVerify, readCappedBytes, resolveUnderCwd, sha256, writeDocxBytes } from './io.js';
import { derivePlan, planDigest, type PlanDigestResult } from './python-plan-deriver.js';
import { throwIfAborted } from './python-repl.js';
import { applyQuoteAuditComments, quoteAuditFeatureFromEnv, rejectedProtectedQuoteCommentMutations, unavailableQuoteProvenancePolicy, type QuoteAuditFeatureOptions } from './quote-audit-pipeline.js';
import { compileInline, lintUnverifiedQuotes, renderTerm, type QuoteLintSeverity, type TermSpec } from './quote-lint.js';
import { QuoteValidationError, resolveQuote, type QuoteSpec } from './quote-provenance.js';
import { containsStructuredQuoteNode, resolveStructuredPlan, type QuoteSourceBinding, type TrustedQuoteRendering } from './structured-quote-plan.js';

/** Core preview keys: p1:sha256:<64 lowercase hex> (from compute_preview_key). */
export const CORE_KEY_RE = /^p1:sha256:[0-9a-f]{64}$/;

/** Commit keys: 128 random bits, `k1:` + 32 lowercase hex characters. */
export const COMMIT_KEY_RE = /^k1:[0-9a-f]{32}$/;

/** A compact random capability handle; the reviewed plan remains server-side. */
export function newCommitKey(): CommitKey {
  return `k1:${randomBytes(16).toString('hex')}`;
}

export type CommitKey = `k1:${string}`;
export type CommitKeyState = 'reviewed' | 'committing' | 'consumed';

export interface CommitKeyRecord {
  id: CommitKey;
  state: CommitKeyState;
  runtimeGeneration: number;
  /** Review-call canonical cwd root: the key's containment environment. */
  cwdRoot: string;
  canonicalAbsPath: string;
  displayPath: string;
  sourceHash: string;
  canonicalPlan: string;
  corePreviewKey: string;
  issuedExecution: number;
  quoteSourceBindings?: QuoteSourceBinding[];
  trustedQuoteRenderings?: TrustedQuoteRendering[];
}

/** Discriminated store lookup: every terminal diagnostic is stable. */
export type CommitKeyLookup =
  | { kind: 'ok'; record: CommitKeyRecord }
  | { kind: 'expired' }
  | { kind: 'consumed' }
  | { kind: 'unknown' };

export type ReadSource = { abs: string; bytes: Uint8Array; sha256: string };
export type CorePlan = { base: string; author: string; change_mode: 'track' | 'direct'; ops: unknown[] };
export type DeriveResult = { ok: true; plan: CorePlan } | { ok: false; message: string };

function plainQuoteValue(value: unknown): unknown {
  if (value instanceof Map) {
    const out: Record<string, unknown> = Object.create(null);
    for (const [key, item] of value) out[String(key)] = plainQuoteValue(item);
    return out;
  }
  if (Array.isArray(value)) return value.map(plainQuoteValue);
  return value;
}

/**
 * Core-backed candidate verification result. `candidateSha256` is the exact
 * hash the post-write read-back must match.
 */
export type CandidateVerification =
  | { ok: true; candidateSha256: string }
  | { ok: false; message: string };

const MAX_NOTICES = 32;
/** Essential (commit key/outcome-bearing) notices cap: newest-kept. */
export const MAX_ESSENTIAL_NOTICES = 32;
/** UTF-8 byte cap for a notice summary or review operation summary. */
const MAX_SUMMARY_BYTES = 4096;
const MAX_REPORTED_OPS = 32;
/**
 * Per-line byte bound of the compact protected protocol channel (a commit key
 * id is 35 bytes; labels and the summary cap leave ample headroom).
 * Combined with `MAX_ESSENTIAL_NOTICES`, a feed's protocol block is at most
 * `MAX_ESSENTIAL_NOTICES × MAX_PROTOCOL_LINE_BYTES` bytes — the proved
 * bound that keeps the channel never-truncated for host-produced notices.
 */
export const MAX_PROTOCOL_LINE_BYTES = 256;
/** Commit-key record FIFO cap, and tombstone/consumed-set caps. */
const STORE_CAP = 64;
const STORE_MAX_BYTES = 8 * 1024 * 1024;
const TOMBSTONE_CAP = 256;
const CONSUMED_CAP = 256;

/**
 * UTF-8 byte-aware truncation: never splits a multi-byte character. The
 * returned string is at most `maxBytes` bytes (plus nothing — the caller
 * appends its own truncation marker).
 */
export function truncateUtf8(text: string, maxBytes: number): string {
  if (maxBytes < 0) return '';
  const buf = Buffer.from(text, 'utf8');
  if (buf.length <= maxBytes) return text;
  let end = maxBytes;
  // Back up to a UTF-8 character boundary (skip continuation bytes).
  while (end > 0 && (buf[end] & 0xc0) === 0x80) end -= 1;
  return buf.subarray(0, end).toString('utf8');
}

/**
 * Named, documented host limits. Every default is exported so the surface
 * contract is testable and documented; partial overrides are merged per
 * host/extension via `Partial<HostLimits>`.
 */
export interface HostLimits {
  /** DOCX input bytes: read/find/preview/commit sources and create html. */
  maxDocxBytes: number;
  /** `docx_find` result-count cap. */
  maxFindResults: number;
  /** Plan operation count cap, enforced before canonicalization/engine. */
  maxPlanOps: number;
  /** Core-canonical plan byte cap, enforced before the engine. */
  maxCanonicalPlanBytes: number;
  /** Maximum serialized context bytes retained for one edit. */
  maxEditContextBytes: number;
  /** Maximum serialized bytes returned by one review result. */
  maxReviewResultBytes: number;
  /** Per-key retained byte cap: rejected outright, never evicted around. */
  commitKeyMaxBytes: number;
  /** Commit-key store total retained byte cap (deterministic FIFO eviction). */
  storeMaxBytes: number;
  /** Commit-key store record cap (deterministic FIFO eviction). */
  storeMaxRecords: number;
  /** Python `print()` collection cap in bytes. */
  maxPrintBytes: number;
  /** Code input cap in bytes (extension, before any runtime work). */
  maxCodeBytes: number;
  /** Final-expression value render cap (plus a truncation marker). */
  maxValueRenderBytes: number;
  /** Reserved control-output byte budget; control is rendered first. */
  controlBudgetBytes: number;
  /** Control-output line cap (tail-truncated: newest lines survive). */
  controlMaxLines: number;
  /**
   * Compact protected protocol channel budget. The essential line count
   * (`MAX_ESSENTIAL_NOTICES`) times the per-line byte bound
   * (`MAX_PROTOCOL_LINE_BYTES`) fits exactly, so the block is never
   * truncated for host-produced notices (a proved bound); the cap is
   * defensive for hand-built arrays.
   */
  protocolBudgetBytes: number;
  /** Host callback duration deadline; commit is exempt after markCommitting. */
  maxHostCallbackMs: number;
}

export const DEFAULT_HOST_LIMITS: HostLimits = {
  maxDocxBytes: 16 * 1024 * 1024,
  maxFindResults: 1000,
  maxPlanOps: 2000,
  maxCanonicalPlanBytes: 512 * 1024,
  maxEditContextBytes: 8 * 1024,
  maxReviewResultBytes: 256 * 1024,
  commitKeyMaxBytes: 4 * 1024 * 1024,
  storeMaxBytes: STORE_MAX_BYTES,
  storeMaxRecords: STORE_CAP,
  maxPrintBytes: 1024 * 1024,
  maxCodeBytes: 256 * 1024,
  maxValueRenderBytes: 8 * 1024,
  controlBudgetBytes: 16 * 1024,
  controlMaxLines: 500,
  protocolBudgetBytes: MAX_ESSENTIAL_NOTICES * MAX_PROTOCOL_LINE_BYTES,
  maxHostCallbackMs: 30_000,
};

/** The host callback exceeded its duration deadline. */
export class HostCallbackTimeoutError extends Error {
  readonly code = 'host_callback_timeout';
  constructor(maxMs: number) {
    super(`host callback timed out (${formatDuration(maxMs)})`);
    this.name = 'HostCallbackTimeoutError';
  }
}

function formatDuration(ms: number): string {
  if (ms >= 1000 && ms % 1000 === 0) return `${ms / 1000} s`;
  return `${ms} ms`;
}

/** A single commit-key record exceeds the store's entire byte budget. */
export class CommitKeyTooLargeError extends Error {
  readonly code = 'commit_key_too_large';
  constructor(readonly maxBytes: number, readonly actualBytes: number) {
    super(`commit key exceeds store byte budget (${formatBytes(maxBytes)})`);
    this.name = 'CommitKeyTooLargeError';
  }
}

/** Byte estimate of one retained commit-key record. */
export function estimateRecordBytes(record: CommitKeyRecord): number {
  let bytes = 192;
  bytes += Buffer.byteLength(record.id, 'utf8');
  bytes += Buffer.byteLength(record.cwdRoot, 'utf8');
  bytes += Buffer.byteLength(record.canonicalAbsPath, 'utf8');
  bytes += Buffer.byteLength(record.displayPath, 'utf8');
  bytes += Buffer.byteLength(record.sourceHash, 'utf8');
  bytes += Buffer.byteLength(record.canonicalPlan, 'utf8');
  bytes += Buffer.byteLength(record.corePreviewKey, 'utf8');
  bytes += Buffer.byteLength(JSON.stringify(record.quoteSourceBindings ?? []), 'utf8');
  bytes += Buffer.byteLength(JSON.stringify(record.trustedQuoteRenderings ?? []), 'utf8');
  return bytes;
}

/**
 * Injectable host/I/O seam for validation, cancellation, and read-back
 * tests. Production uses the core-backed defaults; the seams only narrow
 * the same checks.
 */
export interface PythonDocxHostOptions {
  /** Pi abort signal: every callback refuses host work once it fires. */
  signal?: AbortSignal;
  /** Host limits; partial overrides merge over `DEFAULT_HOST_LIMITS`. */
  limits?: Partial<HostLimits>;
  /** I/O seam: size-checked reads and the atomic write/create helpers. */
  io?: Partial<HostIo>;
  /** Override candidate verification; default = core-backed `validateCandidate`. */
  validateCandidate?: (candidate: Uint8Array, report: unknown, opCount: number) => Promise<CandidateVerification>;
  /** Override the post-write read; default = fs/promises readFile. */
  readBack?: (abs: string) => Promise<Uint8Array>;
  /** Opt-in protected quotation-comment pass; defaults to the environment feature flag. */
  quoteAudit?: QuoteAuditFeatureOptions;
}

/** Injectable I/O surface; production defaults to the io.ts helpers. */
export interface HostIo {
  /**
   * Stat-first, size-capped, cooperatively abortable read of the exact
   * bytes (`maxBytes` = `maxDocxBytes`). Seam contract: the read must
   * reject (typically `Error('aborted')`) when `signal` fires, so a
   * timed-out or caller-aborted callback body unwinds and releases the
   * per-file mutation queue. The default implementation honors the signal;
   * an injected implementation that ignores AbortSignal cannot be forcibly
   * cancelled — the deadline still fires (the race rejects) but that file's
   * queue stays wedged until the underlying I/O settles (documented
   * residual limitation).
   */
  readFile(abs: string, maxBytes?: number, signal?: AbortSignal): Promise<Uint8Array>;
  writeDocxBytes: typeof writeDocxBytes;
  createDocxBytes: typeof createDocxBytes;
}

const DEFAULT_IO: HostIo = {
  readFile: (abs, maxBytes, signal) => readCappedBytes(abs, maxBytes, signal),
  writeDocxBytes,
  createDocxBytes,
};

/** Compose the caller's abort signal and the callback deadline's signal. */
function readSignal(...signals: Array<AbortSignal | undefined>): AbortSignal | undefined {
  const present = signals.filter((s): s is AbortSignal => s !== undefined);
  if (present.length === 0) return undefined;
  if (present.length === 1) return present[0];
  if (typeof AbortSignal.any === 'function') return AbortSignal.any(present);
  // Fallback for Node < 20.3 (no AbortSignal.any): compose manually. The
  // composite dies with the callback, so its listeners are bounded too.
  const controller = new AbortController();
  for (const source of present) {
    if (source.aborted) {
      controller.abort();
      break;
    }
    source.addEventListener('abort', () => controller.abort(), { once: true });
  }
  return controller.signal;
}

/**
 * Cooperative per-callback duration deadline. No mid-flight preemption
 * exists: when the deadline fires, the drive loop's race rejects and unwinds
 * the feed, while the abandoned body keeps running in the background — its
 * `check()` checkpoints throw before any side effect, and its late
 * settlement is consumed (never an unhandled rejection). The deadline's own
 * `AbortSignal` is threaded through the default read seam so abort-aware I/O
 * unwinds the body and releases the per-file mutation queue when the
 * deadline fires; the commit disarms the deadline at `markCommitting` so the
 * irreversible window always runs to completion (its signal never fires
 * afterwards).
 */
class CallbackDeadline {
  private readonly maxMs: number;
  private armed = true;
  private fired = false;
  private timer: NodeJS.Timeout | undefined;
  private readonly controller = new AbortController();
  readonly expired: Promise<never>;
  /** Aborts when the deadline fires (while armed): abort-aware I/O unwinds. */
  readonly signal: AbortSignal;

  constructor(maxMs: number) {
    this.maxMs = maxMs;
    this.signal = this.controller.signal;
    let rejectDeadline!: (error: HostCallbackTimeoutError) => void;
    this.expired = new Promise((_, reject) => {
      rejectDeadline = reject;
    });
    const fire = (): void => {
      if (this.armed) {
        this.fired = true;
        // Abort-aware I/O (the default read seam) observes this signal and
        // unwinds the callback body, releasing the per-file mutation queue.
        this.controller.abort();
        rejectDeadline(new HostCallbackTimeoutError(maxMs));
      }
    };
    if (maxMs > 0 && Number.isFinite(maxMs)) {
      // Deliberately ref'd, never unref'd: a pending host callback with a
      // deadline is meaningful work and must keep the event loop alive. In
      // a quiet loop (headless/batch pi, direct embedding, a bare test
      // script) the Monty worker's IPC does not hold the parent loop open,
      // so an unref'd timer would let the process exit mid-operation with
      // the bound never firing.
      this.timer = setTimeout(fire, maxMs);
    } else {
      // A non-positive deadline means the budget is already exhausted at
      // entry: reject on the next microtask (deterministic test seam).
      queueMicrotask(fire);
    }
  }

  /** The commit disarms the deadline once the commit key is committing. */
  disarm(): void {
    this.armed = false;
    if (this.timer !== undefined) clearTimeout(this.timer);
  }

  /** Body checkpoint: throw when the deadline fired while still armed. */
  check(): void {
    if (this.fired && this.armed) throw new HostCallbackTimeoutError(this.maxMs);
  }

  /** The deadline fired and rejected this callback (timeout already reported). */
  isExpired(): boolean {
    return this.fired;
  }

  dispose(): void {
    if (this.timer !== undefined) clearTimeout(this.timer);
  }
}

/** Race one callback body against its deadline; consume the loser's late settlement. */
function withDeadline<T>(deadline: CallbackDeadline, fn: () => Promise<T>): Promise<T> {
  const body = Promise.resolve().then(fn);
  body.catch(() => undefined);
  return Promise.race([body, deadline.expired]).finally(() => deadline.dispose());
}

/** What `_docx_review` returns to Python: the key plus bounded edit context. */
export interface ReviewEdit {
  index: number;
  op: string;
  outcome: string;
  summary: string;
  context: Array<Record<string, unknown>>;
  context_truncated?: boolean;
}

export interface ReviewResult {
  commit_key: CommitKey;
  edits: ReviewEdit[];
  truncated: boolean;
}

/**
 * Compact protected protocol channel: one bounded line per essential
 * notice, rebuilt from structured fields (never from the truncated verbose
 * block). Combined with the newest-kept essential cap in `boundedNoticer`,
 * a feed's protocol block is at most `MAX_ESSENTIAL_NOTICES ×
 * MAX_PROTOCOL_LINE_BYTES` bytes — the proved bound that makes the channel
 * never-truncated for host-produced notices.
 */
export function protocolLinesFor(notices: PythonHostNotice[]): string[] {
  const lines: string[] = [];
  for (const notice of notices) {
    const essential =
      notice.kind === 'review' ||
      notice.kind === 'commit' ||
      (notice.kind === 'blocked' && notice.phase === 'commit');
    if (!essential) continue;
    if (notice.commitKey !== undefined) {
      const summary = notice.summary !== undefined ? truncateUtf8(notice.summary, 160) : '';
      lines.push(`${notice.kind}: ${notice.commitKey}${summary ? ` \u2014 ${summary}` : ''}`);
    } else {
      // Blocked commit outcome: the commit attempt's fate without a commit key.
      lines.push(`commit blocked: ${truncateUtf8(notice.summary ?? '', 192)}`);
    }
  }
  return lines;
}

export type PythonHostNotice = {
  kind: 'create' | 'review' | 'commit' | 'blocked' | 'document_read';
  path?: string;
  phase?: 'create' | 'validation' | 'review' | 'commit';
  status: 'ok' | 'blocked';
  summary?: string;
  applied?: number;
  failed?: number;
  /** Compact commit key (never the deterministic core preview key). */
  commitKey?: CommitKey;
  bytes?: number;
  ops?: Array<{ op: string; outcome: string; summary: string }>;
};

export interface PlanPythonHost {
  externalLookup: Record<string, unknown>;
}

/**
 * Bounded, caller-owned notice collector. Nothing from it crosses into
 * Python.
 *
 * Essential notices — review/commit outcomes and blocked-commit outcomes —
 * carry commit key ids and protocol outcomes, so they are never
 * dropped by the non-essential cap (32 blocked notices cannot hide a later
 * delivery). They have their own newest-kept cap (`MAX_ESSENTIAL_NOTICES`)
 * so the caller-owned array stays memory-bounded even for a degenerate
 * feed, and every payload is UTF-8 byte-truncated BEFORE the notice is
 * retained. The compact protocol channel (`protocolLinesFor`) re-derives
 * one bounded line per essential notice, so the newest 32 commit keys/outcomes
 * are always rendered in full.
 */
function boundedNoticer(notices: PythonHostNotice[]): (notice: PythonHostNotice) => void {
  let capped = 0;
  const essential: PythonHostNotice[] = [];
  return (notice: PythonHostNotice): void => {
    const essentialKind =
      notice.kind === 'review' ||
      notice.kind === 'commit' ||
      (notice.kind === 'blocked' && notice.phase === 'commit');
    if (essentialKind) {
      if (essential.length >= MAX_ESSENTIAL_NOTICES) {
        // Newest-kept: the most recent commit key/outcome is the actionable
        // one, and Python already saw every commit key as a return value.
        const oldest = essential.shift() as PythonHostNotice;
        const at = notices.indexOf(oldest);
        if (at !== -1) notices.splice(at, 1);
      }
      essential.push(notice);
    } else {
      if (capped >= MAX_NOTICES) return;
      capped += 1;
    }
    // UTF-8 byte-aware truncation: the caps are byte bounds, never split
    // multi-byte characters, and hold before the notice is retained.
    if (notice.summary !== undefined && Buffer.byteLength(notice.summary, 'utf8') > MAX_SUMMARY_BYTES) {
      notice.summary = `${truncateUtf8(notice.summary, MAX_SUMMARY_BYTES)}…(truncated)`;
    }
    if (notice.ops !== undefined) {
      notice.ops = notice.ops.slice(0, MAX_REPORTED_OPS).map((op) => ({
        op: op.op,
        outcome: op.outcome,
        summary: Buffer.byteLength(op.summary, 'utf8') > MAX_SUMMARY_BYTES ? `${truncateUtf8(op.summary, MAX_SUMMARY_BYTES)}…` : op.summary,
      }));
    }
    notices.push(notice);
  };
}

/** Map core operation reports into bounded notice summaries. */
function reportOps(report: { ops: Array<{ op: string; outcome: string; summary: string }> }) {
  return report.ops.map((op) => ({ op: op.op, outcome: op.outcome, summary: op.summary }));
}

/** Preserve core-owned affected markup in a bounded review result. */
function jsonBytes(value: unknown): number {
  return Buffer.byteLength(JSON.stringify(value), 'utf8');
}

function boundedContextEntry(value: Record<string, unknown>, maxBytes: number): { entry: Record<string, unknown>; truncated: boolean } {
  if (jsonBytes(value) <= maxBytes) return { entry: value, truncated: false };
  const paraId = typeof value.para_id === 'string' ? value.para_id : undefined;
  const markup = typeof value.markup === 'string' ? value.markup : '';
  // Keep the paragraph address and as much rendered context as the per-edit
  // budget allows. The loop is bounded by maxBytes and never emits an
  // over-budget entry.
  for (let size = Math.max(0, maxBytes); size >= 0; size -= Math.max(1, Math.floor(maxBytes / 32))) {
    const candidate: Record<string, unknown> = {
      ...(paraId === undefined ? {} : { para_id: paraId }),
      markup: `${truncateUtf8(markup, size)}…`,
      context_truncated: true,
    };
    if (jsonBytes(candidate) <= maxBytes) return { entry: candidate, truncated: true };
  }
  const minimal: Record<string, unknown> = {
    ...(paraId === undefined ? {} : { para_id: paraId }),
    context_truncated: true,
  };
  return { entry: minimal, truncated: true };
}

export function reviewEdits(
  report: { ops: Array<{ index?: number; op: string; outcome: string; summary: string; affected?: unknown[]; context_truncated?: boolean }> },
  maxEditBytes: number,
  maxResultBytes: number,
): { edits: ReviewEdit[]; truncated: boolean; withinLimit: boolean } {
  let truncated = false;
  const edits = report.ops.map((op, position) => {
    const context: Array<Record<string, unknown>> = [];
    let contextTruncated = Boolean(op.context_truncated);
    let contextBytes = 0;
    for (const value of Array.isArray(op.affected) ? op.affected : []) {
      if (value === null || typeof value !== 'object' || Array.isArray(value)) {
        contextTruncated = true;
        continue;
      }
      const remaining = maxEditBytes - contextBytes;
      if (remaining <= 0) {
        contextTruncated = true;
        continue;
      }
      const bounded = boundedContextEntry(value as Record<string, unknown>, remaining);
      const candidateContext = [...context, bounded.entry];
      const candidateBytes = jsonBytes(candidateContext);
      if (candidateBytes <= maxEditBytes) {
        context.push(bounded.entry);
        contextBytes = candidateBytes;
      } else {
        contextTruncated = true;
      }
      contextTruncated ||= bounded.truncated;
    }
    const summaryTruncated = Buffer.byteLength(op.summary, 'utf8') > MAX_SUMMARY_BYTES;
    if (summaryTruncated) truncated = true;
    if (contextTruncated) truncated = true;
    return {
      index: typeof op.index === 'number' ? op.index + 1 : position + 1,
      op: op.op,
      outcome: op.outcome,
      summary: summaryTruncated ? `${truncateUtf8(op.summary, MAX_SUMMARY_BYTES)}…` : op.summary,
      context,
      ...(contextTruncated ? { context_truncated: true } : {}),
    };
  });
  const envelope = () => ({ commit_key: `k1:${'0'.repeat(32)}`, edits, truncated });
  // If the aggregate result is too large, discard context from the tail and
  // mark each affected edit. If summaries alone cannot fit, the review is
  // rejected rather than returning an over-budget result.
  while (jsonBytes(envelope()) > maxResultBytes) {
    const edit = [...edits].reverse().find((candidate) => candidate.context.length > 0);
    if (edit !== undefined) {
      edit.context = [];
      edit.context_truncated = true;
      truncated = true;
      continue;
    }
    const summaryEdit = [...edits].reverse().find((candidate) => candidate.summary.length > 0);
    if (summaryEdit === undefined) break;
    summaryEdit.summary = truncateUtf8(summaryEdit.summary, Math.max(0, Math.floor(summaryEdit.summary.length / 2)));
    summaryEdit.context_truncated = true;
    truncated = true;
  }
  return { edits, truncated, withinLimit: jsonBytes(envelope()) <= maxResultBytes };
}

/** Deterministic JSON: recursively sorted object keys, array order preserved. */
export function canonicalJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`;
  if (value !== null && typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
    return `{${entries.map(([key, item]) => `${JSON.stringify(key)}:${canonicalJson(item)}`).join(',')}}`;
  }
  return JSON.stringify(value);
}

export interface CommitKeyStoreOptions {
  /** Record FIFO cap; default 64. */
  maxRecords?: number;
  /** Retained byte cap; default 8 MiB. */
  maxBytes?: number;
}

/**
 * FIFO-capped commit-key records keyed by the random `k1:` key, byte-accounted:
 * `retainedBytes` sums `estimateRecordBytes` over active records, and `put`
 * evicts deterministically (Map insertion order) while the record count or
 * the retained bytes exceed their caps. Active records (reviewed / committing) live in `records`; consumed ids move to a bounded
 * `consumed` set so replay is diagnosed as consumed rather than unknown;
 * evicted and invalidated ids move to a bounded `expired` tombstone set so
 * replay is diagnosed as expired. A single record beyond the whole store
 * budget is rejected outright (`CommitKeyTooLargeError`) — it never evicts
 * everything else.
 */
export class CommitKeyStore {
  private records = new Map<CommitKey, CommitKeyRecord>();
  private expired = new Set<CommitKey>();
  private consumed = new Set<CommitKey>();
  private readonly maxRecords: number;
  private readonly maxBytes: number;

  constructor(options?: CommitKeyStoreOptions) {
    this.maxRecords = options?.maxRecords ?? STORE_CAP;
    this.maxBytes = options?.maxBytes ?? STORE_MAX_BYTES;
  }

  get size(): number {
    return this.records.size;
  }

  get expiredSize(): number {
    return this.expired.size;
  }

  get consumedSize(): number {
    return this.consumed.size;
  }

  /** Total retained bytes of active records (canonical plans). */
  get retainedBytes(): number {
    let total = 0;
    for (const record of this.records.values()) total += estimateRecordBytes(record);
    return total;
  }

  put(record: CommitKeyRecord): void {
    if (estimateRecordBytes(record) > this.maxBytes) {
      // One record beyond the entire store budget: reject it outright. The
      // host pre-checks `commitKeyMaxBytes` before put; this is the store's
      // own defense so an oversized record can never evict everything.
      throw new CommitKeyTooLargeError(this.maxBytes, estimateRecordBytes(record));
    }
    this.records.set(record.id, record);
    while (this.records.size > this.maxRecords || this.retainedBytes > this.maxBytes) {
      const oldest = this.records.keys().next().value as CommitKey;
      this.records.delete(oldest);
      this.expired.add(oldest);
    }
    while (this.expired.size > TOMBSTONE_CAP) {
      const oldest = this.expired.values().next().value as CommitKey;
      this.expired.delete(oldest);
    }
  }

  lookup(id: CommitKey, generation: number): CommitKeyLookup {
    if (this.expired.has(id)) return { kind: 'expired' };
    if (this.consumed.has(id)) return { kind: 'consumed' };
    const record = this.records.get(id);
    if (record === undefined) return { kind: 'unknown' };
    if (record.runtimeGeneration !== generation) return { kind: 'expired' };
    return { kind: 'ok', record };
  }

  /** Enter the irreversible commit window; only from `reviewed`. */
  markCommitting(id: CommitKey): void {
    const record = this.records.get(id);
    if (record !== undefined && record.state === 'reviewed') record.state = 'committing';
  }

  /** Irreversible: success or source/plan/state/semantic mismatch. */
  consume(id: CommitKey): void {
    const record = this.records.get(id);
    if (record !== undefined) {
      record.state = 'consumed';
      this.records.delete(id);
    }
    this.consumed.add(id);
    while (this.consumed.size > CONSUMED_CAP) {
      const oldest = this.consumed.values().next().value as CommitKey;
      this.consumed.delete(oldest);
    }
  }

  /** Infrastructure failure before the write: back to retryable `reviewed`. */
  revertToReviewed(id: CommitKey): void {
    const record = this.records.get(id);
    if (record !== undefined && record.state === 'committing') record.state = 'reviewed';
  }

  /** Reset/shutdown: tombstone every active id; replay reports expired. */
  invalidateAll(): void {
    for (const id of this.records.keys()) {
      this.expired.add(id);
      this.records.delete(id);
    }
    while (this.expired.size > TOMBSTONE_CAP) {
      const oldest = this.expired.values().next().value as CommitKey;
      this.expired.delete(oldest);
    }
  }
}

function headerFor(notice: PythonHostNotice): string {
  switch (notice.kind) {
    case 'document_read':
      return '';
    case 'review':
      return '── review ok ──';
    case 'commit':
      return '── commit ok ──';
    case 'create':
      return '── create ok ──';
    case 'blocked':
      return '── blocked ──';
  }
}

/** The shared report renderer: identical for every surface. */
export function renderNotices(notices: PythonHostNotice[]): string {
  const lines: string[] = [];
  for (const notice of notices) {
    if (notice.kind === 'document_read') continue;
    const header = headerFor(notice);
    lines.push(notice.path !== undefined ? `${header} path: ${notice.path}` : header);
    if (notice.summary !== undefined) lines.push(notice.summary);
    if (notice.applied !== undefined) {
      lines.push(`${notice.applied} ops applied${notice.failed ? `, ${notice.failed} failed` : ''}`);
    }
    for (const [index, op] of (notice.ops ?? []).entries()) {
      lines.push(`  ${index + 1}. ${op.op} ${op.outcome}: ${op.summary}`);
    }
    // The commit-key line is the notice's essential payload: it renders last,
    // so any tail truncation of this notice's block keeps it.
    if (notice.commitKey !== undefined) lines.push(`commit_key: ${notice.commitKey}`);
  }
  return lines.join('\n');
}

async function readSourceBytes(abs: string, maxBytes: number, io: HostIo, signal?: AbortSignal): Promise<ReadSource> {
  const bytes = new Uint8Array(await io.readFile(abs, maxBytes, signal));
  return { abs, bytes, sha256: sha256(bytes) };
}

/**
 * Core-backed candidate verification: the candidate bytes must re-parse and
 * fully render as a DOCX through core read, and the core report must show
 * every frozen plan operation completed with nothing stopped early or
 * rejected. No TypeScript interpretation of plan semantics happens here —
 * the Rust core is the sole semantic oracle; this checks parse/render
 * success and report shape only. The final-view render is produced and
 * discarded: rendering success is the verification, and the markup is never
 * retained or returned (it can be tens of MB for a near-limit document).
 */
export async function validateCandidate(candidate: Uint8Array, report: unknown, opCount: number): Promise<CandidateVerification> {
  try {
    renderDocxBytes(candidate, 'final');
  } catch (error) {
    return { ok: false, message: error instanceof Error ? error.message : String(error) };
  }
  if (!planReportComplete(report, opCount)) {
    const summary =
      report === null || typeof report !== 'object'
        ? 'core report missing'
        : `core report incomplete: ${(report as { completed?: unknown }).completed ?? '?'}/${opCount} ops completed`;
    return { ok: false, message: summary };
  }
  return { ok: true, candidateSha256: sha256(candidate) };
}

function blocked(
  push: (notice: PythonHostNotice) => void,
  path: string,
  phase: 'create' | 'validation' | 'review' | 'commit',
  summary: string,
  report?: { completed: number; ops: Array<{ op: string; outcome: string; summary: string }> },
): void {
  push({
    kind: 'blocked',
    path,
    phase,
    status: 'blocked',
    summary,
    ...(report !== undefined
      ? { applied: report.completed, failed: report.ops.length - report.completed, ops: reportOps(report) }
      : {}),
  });
}

/** Stable diagnostics for the terminal store states. */
function diagnosticForLookup(lookup: CommitKeyLookup): string {
  switch (lookup.kind) {
    case 'expired':
      return 'commit key expired — review again';
    case 'consumed':
      return 'commit key already consumed — review again';
    default:
      return 'unknown commit key — review first';
  }
}

/** Stable validation-blocked message for a bounded plan digest. */
function digestBlockedMessage(reason: Extract<PlanDigestResult, { ok: false }>['reason'], limits: HostLimits): string {
  switch (reason) {
    case 'too_many_ops':
      return `plan exceeds ${limits.maxPlanOps} operations`;
    case 'too_large':
      return `plan exceeds ${formatBytes(limits.maxCanonicalPlanBytes)} canonical size`;
    default:
      return 'state is not a valid dictionary';
  }
}

/**
 * Create the V1 host adapter bound to `cwd`. `notices` is caller-owned and
 * receives model-visible, JSON-serializable notices; nothing from it is ever
 * returned through Python. `getExecutionNumber` supplies the live REPL feed
 * counter so review/commit can be gated to later executions. `getGeneration`
 * supplies the extension's runtime generation: commit keys are stamped at
 * preview and any lookup under a different generation reports expired.
 * `helpText` is the V1 API reference returned by `docx_help`; when absent,
 * the host falls back to the generic engine help. `options` is the
 * injectable host/I/O seam (signal, limits, candidate validation, read-back,
 * and read/write overrides) used by the Task 3/6 regressions; production
 * callers omit it.
 */
export function createPlanPythonHost(
  cwd: string,
  notices: PythonHostNotice[],
  store: CommitKeyStore,
  getExecutionNumber: () => number,
  getGeneration: () => number,
  helpText?: string,
  options?: PythonDocxHostOptions,
): PlanPythonHost {
  const limits: HostLimits = { ...DEFAULT_HOST_LIMITS, ...options?.limits };
  const io: HostIo = { ...DEFAULT_IO, ...options?.io };
  const signal = options?.signal;
  const push = boundedNoticer(notices);
  const validateCandidateFor = options?.validateCandidate ?? validateCandidate;
  const quoteAudit = options?.quoteAudit ?? quoteAuditFeatureFromEnv();
  const unavailableQuotePolicy = unavailableQuoteProvenancePolicy(quoteAudit);

  const requireAvailableQuotePolicy = () => {
    if (unavailableQuotePolicy) throw new QuoteValidationError(unavailableQuotePolicy);
  };

  const resolveQuoteSpec = async (value: unknown, abort?: AbortSignal) => {
    requireAvailableQuotePolicy();
    const spec = plainQuoteValue(value) as QuoteSpec;
    if (!spec || typeof spec.source !== 'string' || typeof spec.at !== 'string' || typeof spec.select !== 'string') {
      throw new QuoteValidationError('quote requires string source, at, and select fields');
    }
    const abs = await resolveUnderCwd(cwd, spec.source);
    const bytes = new Uint8Array(await io.readFile(abs, limits.maxDocxBytes, abort));
    const sourceHash = sha256(bytes);
    const output = runFind(bytes, { query: spec.select, ignore_case: false });
    if (output.outcome !== 'completed' || !output.result) {
      throw new QuoteValidationError(output.diagnostic?.message ?? 'quote span search failed');
    }
    const candidates = (output.result.matches as Array<Record<string, unknown>>).filter(
      (match) => String(match.id ?? '') === spec.at && typeof match.text === 'string' && match.text.includes(spec.select),
    );
    if (candidates.length === 0) throw new QuoteValidationError(`quote span was not found exactly in paragraph ${spec.at}`);
    return {
      quote: { ...resolveQuote(spec, String(candidates[0].text), sourceHash), provenance_tier: 'unregistered_local' as const },
      binding: { source: spec.source, canonicalAbsPath: abs, source_sha256: sourceHash, provenance_tier: 'unregistered_local' as const },
    };
  };

  const host = {
    _docx_create: async (path: unknown, html: unknown): Promise<null> => {
      throwIfAborted(signal);
      const deadline = new CallbackDeadline(limits.maxHostCallbackMs);
      return withDeadline(deadline, async () => {
        await ensureInit();
        if (typeof html !== 'string') {
          push({ kind: 'blocked', path: String(path), phase: 'create', status: 'blocked', summary: 'html must be a string' });
          return null;
        }
        if (Buffer.byteLength(html, 'utf8') > limits.maxDocxBytes) {
          push({ kind: 'blocked', path: String(path), phase: 'create', status: 'blocked', summary: `document exceeds input limit (${formatBytes(limits.maxDocxBytes)})` });
          return null;
        }
        const abs = await resolveUnderCwd(cwd, String(path));
        const readAbort = readSignal(signal, deadline.signal);
        await withFileMutationQueue(abs, async () => {
          // Refuse overwrite: create only writes a brand-new file. The
          // exists probe is the capped default read: an existing file larger
          // than `maxDocxBytes` is refused by the same stat-first check
          // (still `create refused`, never an allocation of the file).
          try {
            await io.readFile(abs, limits.maxDocxBytes, readAbort);
            push({ kind: 'blocked', path: String(path), phase: 'create', status: 'blocked', summary: 'create refused: file already exists' });
            return;
          } catch (error) {
            if ((error as NodeJS.ErrnoException).code === 'ENOENT') {
              // Missing: creation may proceed.
            } else if (error instanceof DocumentTooLargeError) {
              // Exists, but oversized: still refuse creation.
              push({ kind: 'blocked', path: String(path), phase: 'create', status: 'blocked', summary: 'create refused: file already exists' });
              return;
            } else {
              throw error;
            }
          }
          const output = runCreate(undefined, undefined, html);
          if (output.outcome !== 'completed' || !output.bytes) {
            push({ kind: 'blocked', path: String(path), phase: 'create', status: 'blocked', summary: output.diagnostic?.message ?? output.summary ?? 'create failed' });
            return;
          }
          // Last pre-mutation checkpoint: an abandoned callback (cancelled
          // or expired deadline) must not create the file in the background.
          throwIfAborted(signal);
          deadline.check();
          // createDocxBytes is intentionally exclusive (hard-link create): it can
          // never race into an overwrite.
          await io.createDocxBytes(cwd, abs, output.bytes);
          push({
            kind: 'create',
            path: String(path),
            phase: 'create',
            status: 'ok',
            summary: output.summary ?? 'created',
            bytes: output.bytes.byteLength,
          });
        });
        return null;
      });
    },

    _docx_read: async (path: unknown, view: unknown, kindOrKwargs?: unknown): Promise<unknown> => {
      throwIfAborted(signal);
      const deadline = new CallbackDeadline(limits.maxHostCallbackMs);
      return withDeadline(deadline, async () => {
        await ensureInit();
        let readKind: string | undefined;
        if (kindOrKwargs !== undefined && kindOrKwargs !== null) {
          if (typeof kindOrKwargs === 'string') {
            readKind = kindOrKwargs;
          } else if (typeof kindOrKwargs === 'object' && !Array.isArray(kindOrKwargs)) {
            const kw = kindOrKwargs as Record<string, unknown>;
            if (kw.kind !== undefined && kw.kind !== null) readKind = String(kw.kind);
          }
        }
        const abs = await resolveUnderCwd(cwd, String(path));
        const readAbort = readSignal(signal, deadline.signal);
        if (readKind === 'comments' || readKind === 'styles' || readKind === 'revisions' || readKind === 'assets') {
          return withFileMutationQueue(abs, async () => {
            const bytes = new Uint8Array(await io.readFile(abs, limits.maxDocxBytes, readAbort));
            const output = runRead(bytes, { kind: readKind });
            if (output.outcome !== 'completed' || !output.result) {
              throw new Error(`read failed: ${output.diagnostic?.message ?? output.summary ?? 'unknown error'}`);
            }
            return output.result;
          });
        }
        const renderView = view === undefined || view === null ? 'markup' : String(view);
        if (renderView !== 'markup' && renderView !== 'final' && renderView !== 'original') {
          throw new Error('view must be one of markup, final, original');
        }
        return withFileMutationQueue(abs, async () => {
          const bytes = new Uint8Array(await io.readFile(abs, limits.maxDocxBytes, readAbort));
          const output = runRead(bytes, { kind: 'document', view: renderView });
          if (output.outcome !== 'completed' || !output.result || typeof output.result.markup !== 'string') {
            throw new Error(`read failed: ${output.diagnostic?.message ?? output.summary ?? 'unknown error'}`);
          }
          push({ kind: 'document_read', status: 'ok' });
          return {
            styles: output.result.styles ?? {}, css: output.result.css ?? {},
            markup: output.result.markup, equations: output.result.equations ?? [],
            selection_space: output.result.selection_space ?? null,
            selections: output.result.selections ?? null, comments: output.result.comments ?? [],
            assets: output.result.assets ?? [], comments_error: output.result.comments_error ?? null,
          };
        });
      });
    },

    _docx_find: async (path: unknown, query: unknown, ignore_case: unknown): Promise<Array<Record<string, unknown>>> => {
      throwIfAborted(signal);
      const deadline = new CallbackDeadline(limits.maxHostCallbackMs);
      return withDeadline(deadline, async () => {
        await ensureInit();
        if (typeof query !== 'string' || query.trim() === '') {
          throw new Error('query must be a non-empty string');
        }
        const abs = await resolveUnderCwd(cwd, String(path));
        const readAbort = readSignal(signal, deadline.signal);
        return withFileMutationQueue(abs, async () => {
          const bytes = new Uint8Array(await io.readFile(abs, limits.maxDocxBytes, readAbort));
          const output = runFind(bytes, { query, ignore_case: ignore_case === undefined || ignore_case === null ? false : Boolean(ignore_case) });
          if (output.outcome !== 'completed' || !output.result) {
            throw new Error(`find failed: ${output.diagnostic?.message ?? output.summary ?? 'unknown error'}`);
          }
          // Core find match records verbatim: {index, id?, text, style?, number?}.
          const matches = output.result.matches as Array<Record<string, unknown>>;
          if (matches.length > limits.maxFindResults) {
            throw new Error(`find produced ${matches.length} matches (limit ${limits.maxFindResults}); narrow the query`);
          }
          return matches;
        });
      });
    },

    _quote_find: async (source: unknown, query: unknown, ignoreCase: unknown): Promise<Array<Record<string, unknown>>> => {
      if (!quoteAudit.enabled) throw new QuoteValidationError('structured quotation mode is disabled; enable DOCXDRIVER_QUOTE_AUDIT_COMMENTS');
      requireAvailableQuotePolicy();
      return host._docx_find(source, query, ignoreCase);
    },

    _quote_validate: async (value: unknown) => {
      if (!quoteAudit.enabled) throw new QuoteValidationError('structured quotation mode is disabled; enable DOCXDRIVER_QUOTE_AUDIT_COMMENTS');
      throwIfAborted(signal);
      const deadline = new CallbackDeadline(limits.maxHostCallbackMs);
      return withDeadline(deadline, async () => (await resolveQuoteSpec(value, readSignal(signal, deadline.signal))).quote);
    },

    _quote_lint: async (text: unknown, severity: unknown) => {
      if (typeof text !== 'string') throw new QuoteValidationError('quote_lint text must be a string');
      if (severity !== 'warning' && severity !== 'error') throw new QuoteValidationError('severity must be warning or error');
      return lintUnverifiedQuotes(text, severity as QuoteLintSeverity);
    },

    _term_render: async (value: unknown) => renderTerm(plainQuoteValue(value) as TermSpec),

    _inline_validate: async (value: unknown, policy: unknown) => {
      if (!quoteAudit.enabled) throw new QuoteValidationError('structured quotation mode is disabled; enable DOCXDRIVER_QUOTE_AUDIT_COMMENTS');
      const inline = plainQuoteValue(value) as { parts?: unknown[] };
      const selectedPolicy = policy === 'comment' ? 'comment' : policy === 'error' ? 'error' : undefined;
      if (!selectedPolicy) throw new QuoteValidationError('inline policy must be error or comment');
      return compileInline(inline?.parts ?? [], selectedPolicy, async (spec) => (await resolveQuoteSpec(spec, signal)).quote);
    },

    _docx_help: async (topic: unknown): Promise<string> => {
      throwIfAborted(signal);
      const deadline = new CallbackDeadline(limits.maxHostCallbackMs);
      return withDeadline(deadline, async () => {
        if (helpText !== undefined) return helpText;
        await ensureInit();
        const output = runHelp(topic === null || topic === undefined ? undefined : String(topic));
        if (output.outcome !== 'completed' || !output.result) {
          throw new Error(`help failed: ${output.diagnostic?.message ?? output.summary ?? 'unknown error'}`);
        }
        return JSON.stringify(output.result, null, 2);
      });
    },

    _docx_review: async (path: unknown, state: unknown): Promise<ReviewResult | null> => {
      throwIfAborted(signal);
      const deadline = new CallbackDeadline(limits.maxHostCallbackMs);
      return withDeadline(deadline, async () => {
        await ensureInit();
        const pathText = String(path);
        const digest = planDigest(state, limits.maxPlanOps, limits.maxCanonicalPlanBytes);
        if (!digest.ok) {
          blocked(push, pathText, 'validation', digestBlockedMessage(digest.reason as 'invalid' | 'too_many_ops' | 'too_large', limits));
          return null;
        }
        const abs = await resolveUnderCwd(cwd, pathText);
        // The key binds the review-call canonical cwd root: commit revalidates
        // containment against this root, never the commit call's cwd.
        const cwdRoot = await realpath(resolve(cwd));
        const readAbort = readSignal(signal, deadline.signal);
        return withFileMutationQueue(abs, async () => {
          if (unavailableQuotePolicy) {
            blocked(push, pathText, 'validation', unavailableQuotePolicy);
            return null;
          }
          let source: ReadSource;
          try {
            source = await readSourceBytes(abs, limits.maxDocxBytes, io, readAbort);
          } catch (error) {
            if (error instanceof DocumentTooLargeError) {
              blocked(push, pathText, 'review', error.message);
              return null;
            }
            throw error;
          }
          let resolvedState = state;
          let quoteSourceBindings: QuoteSourceBinding[] = [];
          let trustedQuoteRenderings: TrustedQuoteRendering[] = [];
          if (quoteAudit.enabled) {
            try {
              const structured = await resolveStructuredPlan(state, (spec) => resolveQuoteSpec(spec, readAbort));
              resolvedState = structured.state;
              quoteSourceBindings = structured.sourceBindings;
              trustedQuoteRenderings = structured.trustedRenderings;
            } catch (error) {
              blocked(push, pathText, 'validation', `structured quotation validation failed: ${error instanceof Error ? error.message : String(error)}`);
              return null;
            }
          } else if (containsStructuredQuoteNode(state)) {
            blocked(push, pathText, 'validation', 'structured quotation mode is disabled; enable DOCXDRIVER_QUOTE_AUDIT_COMMENTS');
            return null;
          }
          const derived = derivePlan(resolvedState, source, limits.maxPlanOps);
          if (!derived.ok) {
            blocked(push, pathText, 'validation', derived.message);
            return null;
          }
          if (quoteAudit.enabled) {
            const protectedMutations = rejectedProtectedQuoteCommentMutations(source.bytes, derived.plan.ops);
            if (protectedMutations.length > 0) {
              blocked(push, pathText, 'validation', `plan attempts to mutate protected quotation audit comment${protectedMutations.length === 1 ? '' : 's'}: ${protectedMutations.join(', ')}`);
              return null;
            }
          }
          const canonical = canonicalPlanJson(derived.plan);
          if (!canonical.ok) {
            blocked(push, pathText, 'validation', canonical.error);
            return null;
          }
          if (Buffer.byteLength(canonical.canonical, 'utf8') > limits.maxCanonicalPlanBytes) {
            blocked(push, pathText, 'validation', `plan exceeds ${formatBytes(limits.maxCanonicalPlanBytes)} canonical size`);
            return null;
          }
          const output = runPlan(source.bytes, derived.plan);
          if (output.outcome !== 'previewed') {
            blocked(push, pathText, 'review', output.diagnostic?.message ?? 'review rejected', output.report);
            return null;
          }
          if (!planReportComplete(output.report, derived.plan.ops.length)) {
            blocked(push, pathText, 'review', `review rejected: core report incomplete (${output.report.completed}/${derived.plan.ops.length} ops)`, output.report);
            return null;
          }
          const coreKey = output.preview_key;
          if (typeof coreKey !== 'string' || !CORE_KEY_RE.test(coreKey)) {
            blocked(push, pathText, 'review', 'review rejected: core preview key missing');
            return null;
          }
          const review = reviewEdits(output.report, limits.maxEditContextBytes, limits.maxReviewResultBytes);
          if (!review.withinLimit) {
            blocked(push, pathText, 'review', `review result exceeds ${formatBytes(limits.maxReviewResultBytes)}`);
            return null;
          }
          const placeholderKey = `k1:${'0'.repeat(32)}` as CommitKey;
          const record: CommitKeyRecord = {
            id: placeholderKey,
            state: 'reviewed',
            runtimeGeneration: getGeneration(),
            cwdRoot,
            canonicalAbsPath: abs,
            displayPath: pathText,
            sourceHash: source.sha256,
            canonicalPlan: canonical.canonical,
            corePreviewKey: coreKey,
            issuedExecution: getExecutionNumber(),
            quoteSourceBindings,
            trustedQuoteRenderings,
          };
          if (estimateRecordBytes(record) > limits.commitKeyMaxBytes) {
            blocked(push, pathText, 'review', `commit key record too large (exceeds ${formatBytes(limits.commitKeyMaxBytes)})`);
            return null;
          }
          throwIfAborted(signal);
          deadline.check();
          // Mint only after report, context, record-size, and final callback
          // checkpoints have all passed.
          const key = newCommitKey();
          record.id = key;
          store.put(record);
          push({
            kind: 'review',
            path: pathText,
            phase: 'review',
            status: 'ok',
            commitKey: key,
            summary: `review ok: ${output.report.completed} ops`,
            applied: output.report.completed,
            failed: 0,
          });
          return { commit_key: key, edits: review.edits, truncated: review.truncated };
        });
      });
    },

    _docx_commit: async (key: unknown): Promise<null> => {
      throwIfAborted(signal);
      const deadline = new CallbackDeadline(limits.maxHostCallbackMs);
      return withDeadline(deadline, async () => {
        await ensureInit();
        const id = String(key);
        if (!COMMIT_KEY_RE.test(id)) {
          push({ kind: 'blocked', phase: 'commit', status: 'blocked', summary: 'malformed commit key' });
          return null;
        }
        const lookup = store.lookup(id as CommitKey, getGeneration());
        if (lookup.kind !== 'ok') {
          push({ kind: 'blocked', phase: 'commit', status: 'blocked', summary: diagnosticForLookup(lookup) });
          return null;
        }
        const record = lookup.record;
        const now = getExecutionNumber();
        if (now <= record.issuedExecution) {
          push({
            kind: 'blocked',
            path: record.displayPath,
            phase: 'commit',
            status: 'blocked',
            summary: `commit requires a later python execution (review was execution ${record.issuedExecution}, this is ${now})`,
          });
          return null;
        }
        if (record.state === 'committing') {
          push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'commit already in progress' });
          return null;
        }
        let plan: CorePlan;
        try {
          plan = JSON.parse(record.canonicalPlan) as CorePlan;
          if (!plan || typeof plan !== 'object' || !Array.isArray(plan.ops)) throw new Error('stored plan is invalid');
        } catch {
          store.consume(record.id);
          push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'stored plan is invalid — review again' });
          return null;
        }
        await withFileMutationQueue(record.canonicalAbsPath, async () => {
          const readAbort = readSignal(signal, deadline.signal);
          let bytes: Uint8Array;
          let source: ReadSource;
          // Set once rename(2) has happened: every later failure is
          // post-rename — the write landed, the commit key is consumed, and the
          // output must truthfully say so.
          let renamed = false;
          try {
            // Path-identity recheck: the commit key-bound canonical path must
            // still resolve to itself. An in-window replacement by a
            // symlink/alias to another file (even byte-identical) is a
            // semantic race: rename over the symlink would silently replace
            // the wrong namespace object. A vanished file stays an
            // infrastructure failure (retryable).
            let actual: string;
            try {
              actual = await realpath(record.canonicalAbsPath);
            } catch (error) {
              if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
              throw new Error('target vanished before commit');
            }
            if (actual !== record.canonicalAbsPath) {
              store.consume(record.id);
              push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'path changed after review — re-read and review again' });
              return;
            }
            // Hash the raw bytes before any rendering: a changed (or corrupt)
            // file must trip the source gate, never surface as a parse error.
            bytes = new Uint8Array(await io.readFile(record.canonicalAbsPath, limits.maxDocxBytes, readAbort));
            const sourceSha = sha256(bytes);
            if (sourceSha !== record.sourceHash) {
              store.consume(record.id);
              push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'source changed after review — re-read and review again' });
              return;
            }
            source = { abs: record.canonicalAbsPath, bytes, sha256: sourceSha };
            for (const binding of record.quoteSourceBindings ?? []) {
              let actualSource: string;
              try {
                actualSource = await realpath(binding.canonicalAbsPath);
              } catch {
                throw new SourceChangedError(binding.source_sha256, '<missing quotation source>');
              }
              if (actualSource !== binding.canonicalAbsPath) throw new SourceChangedError(binding.canonicalAbsPath, actualSource);
              const quoteBytes = new Uint8Array(await io.readFile(binding.canonicalAbsPath, limits.maxDocxBytes, readAbort));
              const actualQuoteHash = sha256(quoteBytes);
              if (actualQuoteHash !== binding.source_sha256) throw new SourceChangedError(binding.source_sha256, actualQuoteHash);
            }
          } catch (error) {
            if (error instanceof SourceChangedError) {
              store.consume(record.id);
              push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'source changed after review — re-read and review again' });
              return;
            }
            if (error instanceof DocumentTooLargeError) {
              // The source grew beyond the input limit: the bytes differ from
              // the reviewed source, so the commit key is spent.
              store.consume(record.id);
              push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'source changed after review — re-read and review again' });
              return;
            }
            if (deadline.isExpired() || error instanceof HostCallbackTimeoutError) {
              // The duration deadline expired during the commit gates (still
              // pre-write): the target is untouched and the commit key stays
              // retryable; the label says timed out, not infrastructure.
              store.revertToReviewed(record.id);
              push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'commit timed out — commit key kept, retry' });
              throw error;
            }
            if (signal?.aborted) {
              // Cancellation before any semantic decision: the commit key stays
              // retryable and the failure surfaces to Python.
              store.revertToReviewed(record.id);
              push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'commit cancelled — commit key kept, retry' });
              throw error;
            }
            // I/O failure before any semantic decision: keep the commit key
            // retryable and surface the failure to Python.
            store.revertToReviewed(record.id);
            push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'commit infrastructure failure — commit key kept, retry' });
            throw error;
          }
          let output: ReturnType<typeof runPlan>;
          try {
            // All semantic gates passed. The irreversible window starts only
            // after the pre-write checkpoints: cancellation or an expired
            // deadline before this point leaves the target untouched and the
            // commit key retryable.
            throwIfAborted(signal);
            deadline.check();
            store.markCommitting(record.id);
            // The write window is exempt from the duration deadline: the
            // commit runs to completion, so the drive loop is never told
            // "timed out" while a write may still land afterwards.
            deadline.disarm();
            output = runPlan(source.bytes, plan, record.corePreviewKey);
            if (output.outcome !== 'committed' || !output.bytes) {
              // Core semantic rejection: the plan no longer commits; the
              // commit key is spent and the model must review again.
              store.consume(record.id);
              blocked(push, record.displayPath, 'commit', output.diagnostic?.message ?? 'commit rejected', output.report);
              return;
            }
            // Source metadata consistency: the plan's base is the reviewed
            // source hash and core reports the same source it folded. Cheap
            // assertion-grade check before the candidate oracle runs.
            if (output.source !== `sha256:${record.sourceHash}`) {
              store.consume(record.id);
              push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'candidate verification failed: committed source metadata does not match the reviewed source' });
              return;
            }
            let candidateBytes = output.bytes;
            let quoteWarningCount = 0;
            if (quoteAudit.enabled) {
              try {
                const audited = applyQuoteAuditComments(candidateBytes, { trustedRenderings: record.trustedQuoteRenderings ?? [] });
                candidateBytes = audited.bytes;
                quoteWarningCount = audited.warnings;
              } catch (error) {
                store.consume(record.id);
                push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: `quotation audit failed: ${error instanceof Error ? error.message : String(error)}` });
                return;
              }
            }
            // Candidate verification happens BEFORE any filesystem mutation:
            // the candidate must re-parse and fully render as a DOCX through
            // core read, and the report must show every frozen plan operation
            // completed. A failure writes nothing and consumes the commit key.
            const verification = await validateCandidateFor(candidateBytes, output.report, plan.ops.length);
            if (!verification.ok) {
              store.consume(record.id);
              push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: `candidate verification failed: ${verification.message}` });
              return;
            }
            if (candidateBytes.byteLength > limits.maxDocxBytes) {
              // The candidate would exceed the surface's own input limit: the
              // reviewed plan no longer fits the contract (the model could
              // not even read the result back). Pre-write: the commit key is
              // spent and nothing is written.
              store.consume(record.id);
              push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: `candidate exceeds input limit (${formatBytes(limits.maxDocxBytes)}) — review again with a smaller plan` });
              return;
            }
            // Last pre-rename checkpoint: cancellation or an expired deadline
            // here still leaves the target untouched and the commit key
            // retryable.
            throwIfAborted(signal);
            deadline.check();
            // Immediate source/path recheck and durable atomic replacement:
            // exclusive sibling temp, temp fsync, repeated canonical
            // path/source checks, atomic rename, platform-gated directory
            // fsync, cleanup. The review-bound cwd root anchors containment;
            // the expected realpath rejects any in-window path-identity swap;
            // the pre-write source rechecks are capped at `maxDocxBytes`.
            await io.writeDocxBytes(record.cwdRoot, record.canonicalAbsPath, candidateBytes, record.sourceHash, {
              expectedRealPath: record.canonicalAbsPath,
              maxSourceBytes: limits.maxDocxBytes,
            });
            // The atomic replacement happened. Cancellation observed now — or
            // any failure in the durability verification below — is a
            // post-rename outcome: the commit key is consumed and the output
            // truthfully reports the completed write (never a silent success
            // or a false failure).
            renamed = true;
            const cancelledAfterWrite = signal?.aborted === true;
            // Read back and hash-check the exact verified candidate bytes
            // (stat-first, capped at the candidate's exact size). The
            // mutation has already occurred: a mismatch is an infrastructure
            // error (never a success notice) and the commit key stays consumed.
            await readBackVerify(record.canonicalAbsPath, verification.candidateSha256, candidateBytes.byteLength, options?.readBack);
            store.consume(record.id);
            push({
              kind: 'commit',
              path: record.displayPath,
              phase: 'commit',
              status: 'ok',
              summary: cancelledAfterWrite
                ? `committed: ${output.report.completed} ops${quoteAudit.enabled ? `; quotation audit warnings: ${quoteWarningCount}` : ''} — cancellation arrived during commit; write completed`
                : `committed: ${output.report.completed} ops${quoteAudit.enabled ? `; quotation audit warnings: ${quoteWarningCount}` : ''}`,
              applied: output.report.completed,
              failed: 0,
              bytes: candidateBytes.byteLength,
              commitKey: record.id,
            });
          } catch (error) {
            if (error instanceof SourceChangedError) {
              store.consume(record.id);
              push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'source changed after review — re-read and review again' });
              return;
            }
            if (error instanceof PathChangedError) {
              store.consume(record.id);
              push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'path changed after review — re-read and review again' });
              return;
            }
            if (error instanceof PostRenameError || renamed) {
              // rename(2) happened: the mutation landed. The commit key is
              // consumed and the output truthfully reports the completed
              // write; the infrastructure error still surfaces to Python
              // (durability verification failed — never a false success).
              store.consume(record.id);
              const detail = error instanceof Error ? error.message : String(error);
              push({
                kind: 'blocked',
                path: record.displayPath,
                phase: 'commit',
                status: 'blocked',
                summary: `commit infrastructure failure — write completed; commit key consumed: ${detail}`,
              });
              throw error;
            }
            if (deadline.isExpired() || error instanceof HostCallbackTimeoutError) {
              // The duration deadline expired before the write: the target is
              // untouched and the commit key stays retryable.
              store.revertToReviewed(record.id);
              push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'commit timed out — commit key kept, retry' });
              throw error;
            }
            if (signal?.aborted) {
              // Cancellation before rename: the target is untouched and the
              // commit key stays retryable.
              store.revertToReviewed(record.id);
              push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'commit cancelled — commit key kept, retry' });
              throw error;
            }
            store.revertToReviewed(record.id);
            push({ kind: 'blocked', path: record.displayPath, phase: 'commit', status: 'blocked', summary: 'commit infrastructure failure — commit key kept, retry' });
            throw error;
          }
        });
        return null;
      });
    },
  };

  return { externalLookup: host };
}
