import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import wasmInit, {
  canonicalPlanJson as wasmCanonicalPlanJson,
  executeRequest as wasmExecuteRequest,
  parsePlanToml as wasmParsePlanToml,
  type TypedOutput,
} from '../wasm/docxdriver.js';
import { generatedOperationSchema } from './schema.js';

/** The core ABI is JSON; local structural aliases keep this package portable
 * without a sibling `docxdriver` npm package. */
type Plan = Record<string, unknown>;
type Request = Record<string, unknown>;

/** Keep the host independent from generated type names while the core ABI evolves. */
export type EngineResult = {
  status?: string;
  outcome?: string;
  summary?: string;
  diagnostic?: { code?: string; message?: string; path?: string; span?: unknown };
  result?: any;
  bytes?: Uint8Array;
  [key: string]: any;
};

let ready: Promise<void> | undefined;
let initialized = false;
export function ensureInit(): Promise<void> {
  ready ??= (async () => {
    const wasmUrl = new URL('../wasm/docxdriver_bg.wasm', import.meta.url);
    await wasmInit({ module_or_path: await readFile(fileURLToPath(wasmUrl)) });
    initialized = true;
  })();
  return ready;
}

function assertReady(): void {
  if (!initialized) throw new Error('docxdriver-pi: await ensureInit() before calling into the engine');
}

/** Execute a typed public request. The JSON adapter is deliberately the only ABI
 * detail here; no legacy command is synthesized by the Pi extension. */
export function runRequest(input: Uint8Array | undefined, request: Request): EngineResult {
  assertReady();
  const output: TypedOutput = wasmExecuteRequest(input, JSON.stringify(request));
  try {
    const envelope = JSON.parse(output.resultJson) as EngineResult;
    const bytes = output.bytes;
    return bytes === undefined ? envelope : { ...envelope, bytes };
  } finally {
    output.free();
  }
}

export function runCreate(input: undefined, paragraphs?: string[], html?: string): EngineResult {
  return runRequest(input, { Command: { command: { kind: 'create', ...(html !== undefined ? { html } : { paragraphs }) } } });
}

export function runRead(input: Uint8Array, args: Record<string, unknown>): EngineResult {
  const { kind, ...readArgs } = args;
  return runRequest(input, { Command: { command: { kind: 'read', ...readArgs, ...(kind && kind !== 'document' ? { read_kind: kind } : {}) } } } as Request);
}

/** Candidate verification failed: the core could not parse/render the bytes. */
export class CandidateValidationError extends Error {
  readonly code = 'candidate_validation_failed';
  constructor(message: string) {
    super(message);
    this.name = 'CandidateValidationError';
  }
}

/**
 * Render the canonical projection of exact bytes through core read. The core
 * re-parses the zip container and document.xml and runs a full document
 * render (WorkState::parse + render_document); any rejection means the bytes
 * are not a valid, fully renderable DOCX. This is the single byte-render
 * helper: candidate verification, read-back checks, and the read surface all
 * share it.
 *
 * Throws CandidateValidationError unless the read outcome is 'completed'
 * with string markup.
 */
export function renderDocxBytes(bytes: Uint8Array, view: 'markup' | 'final' | 'original' = 'final'): string {
  const output = runRead(bytes, { kind: 'document', view });
  if (output.outcome !== 'completed' || !output.result || typeof output.result.markup !== 'string') {
    throw new CandidateValidationError(`read failed: ${output.diagnostic?.message ?? output.summary ?? 'unknown error'}`);
  }
  return output.result.markup;
}

/**
 * Report-shape completeness predicate — counting only, no plan semantics.
 * The Rust core is the sole semantic oracle; this only asserts the report
 * shows every frozen plan operation completed with nothing stopped early
 * and no op rejected.
 */
export function planReportComplete(report: unknown, opCount: number): boolean {
  if (report === null || typeof report !== 'object') return false;
  const r = report as { completed?: unknown; stopped_at?: unknown; ops?: unknown };
  if (r.completed !== opCount) return false;
  if (r.stopped_at !== undefined && r.stopped_at !== null) return false;
  if (!Array.isArray(r.ops) || r.ops.length !== opCount) return false;
  return r.ops.every((op) => op !== null && typeof op === 'object' && (op as { outcome?: unknown }).outcome === 'applied');
}

export function runFind(input: Uint8Array, args: Record<string, unknown>): EngineResult {
  return runRequest(input, { Command: { command: { kind: 'find', ...args } } } as Request);
}

export function runPlan(input: Uint8Array, plan: unknown, previewKey?: string): EngineResult {
  return runRequest(input, { Plan: { plan: plan as Plan, ...(previewKey !== undefined ? { preview_key: previewKey } : {}) } });
}

export type CanonicalPlanResult = { ok: true; canonical: string } | { ok: false; error: string };

/**
 * Core-normalized canonical plan JSON for the authored plan: core
 * `validate_plan`, then null stripping and sorted keys. This is the future
 * producer for `ReceiptRecord.canonicalPlan` (the coordinator wires the two
 * `python-host.ts` call sites after Task 3); rejection carries the core
 * diagnostic unchanged.
 */
export function canonicalPlanJson(plan: unknown): CanonicalPlanResult {
  assertReady();
  const text = wasmCanonicalPlanJson(JSON.stringify(plan as Plan));
  try {
    const envelope = JSON.parse(text) as Record<string, unknown>;
    if (
      envelope !== null &&
      typeof envelope === 'object' &&
      !Array.isArray(envelope) &&
      Object.keys(envelope).length === 1 &&
      typeof envelope.error === 'string'
    ) return { ok: false, error: envelope.error };
    return { ok: true, canonical: text };
  } catch {
    return { ok: false, error: 'canonical plan output is not valid JSON' };
  }
}

export function runPlanToml(input: Uint8Array, text: string, previewKey?: string): EngineResult {
  assertReady();
  const parsed = JSON.parse(wasmParsePlanToml(text)) as { outcome: string; diagnostic?: unknown; plan?: unknown };
  return parsed.outcome === 'rejected'
    ? { outcome: 'rejected', diagnostic: parsed.diagnostic as EngineResult['diagnostic'] }
    : runPlan(input, parsed.plan ?? {}, previewKey);
}

export function runHelp(topic?: string): EngineResult {
  const operationSchema = generatedOperationSchema();
  const operations = operationSchema.oneOf.flatMap((variant: any) => {
    const op = variant.properties?.op;
    return typeof op?.const === 'string' ? [op.const] : Array.isArray(op?.enum) ? op.enum : [];
  });
  return { outcome: 'completed', summary: topic ? `plan help: ${topic}` : 'plan and read help', result: {
    topic: topic ?? 'all',
    tools: ['docx_create', 'docx_read', 'docx_find', 'docx_edit', 'docx_help'],
    plan: { operations, schema: operationSchema, preview: 'preview_key required for commit' },
    read: { kinds: ['document', 'styles', 'comments', 'revisions', 'assets'], document_surface: 'HTML with MathML' },
  } };
}
