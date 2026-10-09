/**
 * Version 1 plan derivation (typed-plan surface).
 *
 * This module carries no operation vocabulary or mutation semantics. Monty
 * values are converted to plain data, the {"plan": {...}} envelope is
 * required, and the exact source base is stamped. The Rust typed boundary
 * validates operation names, fields, types, enums, addresses, and semantics.
 *
 * Resource bounds live at the derivation gate, before any canonicalization
 * or engine call: `maxOps` caps the operation count and `maxCanonicalBytes`
 * caps the canonical plan size (both are passed by the host from its
 * `HostLimits`). An oversized plan is rejected with a stable reason instead
 * of being canonicalized, rendered, or retained.
 */
import { createHash } from 'node:crypto';
import type { CorePlan, DeriveResult, ReadSource } from './python-host.js';
import { canonicalJson } from './python-host.js';

/** Recursively convert Monty-transported Maps and arrays to plain JS data. */
function toPlain(value: unknown): unknown {
  if (value instanceof Map) {
    const out: Record<string, unknown> = Object.create(null);
    for (const [key, item] of value) out[String(key)] = toPlain(item);
    return out;
  }
  if (Array.isArray(value)) return value.map(toPlain);
  return value;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

/** Deterministic string digest: sha256 hex of the UTF-8 bytes. */
export function sha256Text(text: string): string {
  return createHash('sha256').update(text).digest('hex');
}

export type PlanDigestResult =
  | { ok: true; digest: string }
  | { ok: false; reason: 'invalid' | 'too_many_ops' | 'too_large' };

/**
 * Digest the raw authored plan so any Python-side mutation invalidates it.
 * The plan is canonicalized host-side (sorted keys) purely for the digest;
 * the receipt stores the core-canonical plan. When `maxOps` / `maxBytes` are
 * given, the plan is rejected with a stable reason BEFORE canonicalization
 * cost grows further and before any engine call.
 */
export function planDigest(state: unknown, maxOps?: number, maxCanonicalBytes?: number): PlanDigestResult {
  try {
    const plain = toPlain(state);
    if (!isRecord(plain) || !('plan' in plain)) return { ok: false, reason: 'invalid' };
    const authored = plain.plan;
    if (!isRecord(authored)) return { ok: false, reason: 'invalid' };
    if (maxOps !== undefined && Array.isArray(authored.ops) && authored.ops.length > maxOps) {
      return { ok: false, reason: 'too_many_ops' };
    }
    const canonical = canonicalJson(authored);
    if (maxCanonicalBytes !== undefined && Buffer.byteLength(canonical, 'utf8') > maxCanonicalBytes) {
      return { ok: false, reason: 'too_large' };
    }
    return { ok: true, digest: sha256Text(canonical) };
  } catch {
    return { ok: false, reason: 'invalid' };
  }
}

/**
 * Convert the authored plan to plain data and bind it to the current source.
 * All vocabulary and validation remain core-owned. `maxOps` caps the
 * operation count before canonicalization or any engine call.
 */
export function derivePlan(state: unknown, source: ReadSource, maxOps?: number): DeriveResult {
  const plain = toPlain(state);
  if (!isRecord(plain) || !('plan' in plain)) {
    return { ok: false, message: 'state must contain a "plan" dictionary' };
  }
  const authored = plain.plan;
  if (!isRecord(authored)) {
    return { ok: false, message: 'plan must be a dictionary' };
  }
  if (maxOps !== undefined && Array.isArray(authored.ops) && authored.ops.length > maxOps) {
    return { ok: false, message: `plan exceeds ${maxOps} operations` };
  }
  return { ok: true, plan: { ...authored, base: `sha256:${source.sha256}` } as CorePlan };
}
