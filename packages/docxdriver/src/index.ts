// Typed Request/Plan boundary over the generated wasm artifact.
import wasmInit, {
  executeRequest as wasmExecuteRequest,
  parsePlanToml as wasmParsePlanToml,
  canonicalPlanJson as wasmCanonicalPlanJson,
  type TypedOutput,
  type InitInput,
} from '../wasm/docxdriver.js';
import type { PlanParseResult, Request, RequestResult, Plan } from './typed.js';

export * from './typed.js';
export * from './html.js';
export type { InitInput } from '../wasm/docxdriver.js';

let instantiation: Promise<void> | undefined;
let ready = false;

export function init(input?: InitInput | Promise<InitInput>): Promise<void> {
  instantiation ??= wasmInit(input === undefined ? undefined : { module_or_path: input }).then(() => {
    ready = true;
  });
  return instantiation;
}

function assertReady(): void {
  if (!ready) throw new Error('docxdriver: await init() before calling into the engine');
}

export function executeRequest(input: Uint8Array | undefined, request: Request): RequestResult {
  assertReady();
  const output: TypedOutput = wasmExecuteRequest(input, JSON.stringify(request));
  try {
    const envelope = JSON.parse(output.resultJson) as Record<string, unknown>;
    const bytes = output.bytes;
    return (bytes === undefined ? envelope : { ...envelope, bytes }) as RequestResult;
  } finally {
    output.free();
  }
}

/** Parse TOML with the core-owned parser while retaining structured diagnostics. */
export function parsePlanToml(text: string): PlanParseResult {
  assertReady();
  return JSON.parse(wasmParsePlanToml(text)) as PlanParseResult;
}

export type CanonicalPlanResult = { ok: true; canonical: string } | { ok: false; error: string };

/**
 * Core-normalized canonical plan JSON: core `validate_plan`, then null
 * stripping and sorted keys — the exact normalized plan the host stores on
 * receipts and compares at commit. This is the future producer for
 * `ReceiptRecord.canonicalPlan`; the wasm binding returns an `{"error": ...}`
 * envelope for parse/validation failures, which is surfaced as a core
 * diagnostic rather than being reworded.
 */
export function canonicalPlanJson(plan: Plan): CanonicalPlanResult {
  assertReady();
  const text = wasmCanonicalPlanJson(JSON.stringify(plan));
  try {
    const envelope = JSON.parse(text) as Record<string, unknown>;
    if (
      envelope !== null &&
      typeof envelope === 'object' &&
      !Array.isArray(envelope) &&
      Object.keys(envelope).length === 1 &&
      typeof envelope.error === 'string'
    ) {
      return { ok: false, error: envelope.error };
    }
    return { ok: true, canonical: text };
  } catch {
    // Defensive: the canonical output is always JSON; a malformed envelope
    // fails closed rather than handing an unvalidated string to the host.
    return { ok: false, error: 'canonical plan output is not valid JSON' };
  }
}
