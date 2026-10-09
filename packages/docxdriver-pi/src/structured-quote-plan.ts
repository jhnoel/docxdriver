import { compileInline, renderTerm, type TermSpec } from './quote-lint.js';
import type { QuoteSpec, ResolvedQuote } from './quote-provenance.js';

export interface QuoteSourceBinding {
  source: string;
  canonicalAbsPath: string;
  source_sha256: string;
  provenance_tier: 'unregistered_local';
}

export interface TrustedQuoteRendering {
  kind: 'quote' | 'term';
  text: string;
  quote_id?: string;
}

export interface ResolvedQuoteWithBinding {
  quote: ResolvedQuote;
  binding: QuoteSourceBinding;
}

export interface ResolvedStructuredPlan {
  state: unknown;
  sourceBindings: QuoteSourceBinding[];
  trustedRenderings: TrustedQuoteRendering[];
}

function plain(value: unknown): unknown {
  if (value instanceof Map) {
    const out: Record<string, unknown> = Object.create(null);
    for (const [key, item] of value) out[String(key)] = plain(item);
    return out;
  }
  if (Array.isArray(value)) return value.map(plain);
  return value;
}

function record(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined;
}

export function containsStructuredQuoteNode(value: unknown): boolean {
  const converted = plain(value);
  if (Array.isArray(converted)) return converted.some(containsStructuredQuoteNode);
  const item = record(converted);
  if (!item) return false;
  if (item.$kind === 'quote' || item.$kind === 'term' || item.$kind === 'inline') return true;
  return Object.values(item).some(containsStructuredQuoteNode);
}

/** Resolve structured `with` values before the core ever sees the plan. */
export async function resolveStructuredPlan(
  state: unknown,
  resolveQuote: (spec: QuoteSpec) => Promise<ResolvedQuoteWithBinding>,
): Promise<ResolvedStructuredPlan> {
  const converted = plain(state);
  const envelope = record(converted);
  const plan = record(envelope?.plan);
  if (!envelope || !plan || !Array.isArray(plan.ops)) throw new Error('state must contain a plan dictionary with an ops list');

  const sourceBindings = new Map<string, QuoteSourceBinding>();
  const trustedRenderings: TrustedQuoteRendering[] = [];
  const resolveNode = async (value: unknown): Promise<unknown> => {
    const node = record(value);
    if (!node || typeof node.$kind !== 'string') return value;
    if (node.$kind === 'term') {
      const term = renderTerm(node as unknown as TermSpec);
      trustedRenderings.push({ kind: 'term', text: term.text });
      return term.text;
    }
    if (node.$kind === 'quote') {
      const resolved = await resolveQuote(node as unknown as QuoteSpec);
      sourceBindings.set(`${resolved.binding.canonicalAbsPath}\0${resolved.binding.source_sha256}`, resolved.binding);
      trustedRenderings.push({ kind: 'quote', text: resolved.quote.text, quote_id: resolved.quote.quote_id });
      return resolved.quote.text;
    }
    if (node.$kind !== 'inline') throw new Error(`unknown structured inline kind ${node.$kind}`);
    if (!Array.isArray(node.parts)) throw new Error('Inline.parts must be a list');
    const capturedQuotes: ResolvedQuoteWithBinding[] = [];
    const compiled = await compileInline(node.parts, 'error', async (spec) => {
      const resolved = await resolveQuote(spec as unknown as QuoteSpec);
      capturedQuotes.push(resolved);
      return resolved.quote;
    });
    for (const resolved of capturedQuotes) {
      sourceBindings.set(`${resolved.binding.canonicalAbsPath}\0${resolved.binding.source_sha256}`, resolved.binding);
      trustedRenderings.push({ kind: 'quote', text: resolved.quote.text, quote_id: resolved.quote.quote_id });
    }
    for (const part of node.parts) {
      const term = record(part);
      if (term?.$kind === 'term') trustedRenderings.push({ kind: 'term', text: renderTerm(term as unknown as TermSpec).text });
    }
    return compiled.text;
  };

  const ops: unknown[] = [];
  for (const value of plan.ops) {
    const op = record(value);
    if (!op) {
      ops.push(value);
      continue;
    }
    const next = { ...op };
    if ('with' in next) next.with = await resolveNode(next.with);
    ops.push(next);
  }
  return {
    state: { ...envelope, plan: { ...plan, ops } },
    sourceBindings: [...sourceBindings.values()],
    trustedRenderings,
  };
}
