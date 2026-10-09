import type { EngineResult } from './engine.js';

export type ToolTextResult = { content: [{ type: 'text'; text: string }]; details: unknown; isError?: false };
export function toolResult(details: unknown, text?: string): ToolTextResult {
  return { content: [{ type: 'text', text: text ?? JSON.stringify(details) }], details };
}

/** Expected refusals are normal results. Only infrastructure failures throw. */
export function fromEngineResult(path: string, output: EngineResult, extras: Record<string, unknown> = {}): ToolTextResult {
  if (output.status === 'error' && !output.diagnostic && output.outcome !== 'rejected') throw new Error(output.summary ?? 'engine failure');
  // Binary output is written by the host. Plan echoes repeat the submitted
  // operations; canonical TOML is an authoring artifact, not edit context.
  const { bytes: _bytes, plan: _plan, canonical_toml: _toml, ...result } = output;
  if (result.outcome === 'committed' && result.report) {
    const report = result.report as any;
    result.report = { ...report, ops: report.ops?.map(({ affected: _affected, ...op }: any) => op) };
  }
  return toolResult({ path, ...result, ...extras });
}
export function fail(message: string): never { throw new Error(message); }
