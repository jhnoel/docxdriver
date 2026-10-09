/**
 * Version 2 deriver (raw projection string surface): turns the state the
 * prelude sends — `{"original", "proposed"}` at preview, `{"proposed"}` at
 * commit — into a typed core plan via the projection reconciler
 * (`diffProjections`). The agent's `original` is validated against the current
 * file projection at preview; the state digest is sha256 of the proposed
 * string, so any draft mutation after preview invalidates the stored key.
 * `verifyCommit` structurally compares the committed document (final view —
 * the host renders post-write markup in the final view so tracked del/ins
 * wrappers do not leak into the comparison) against the proposed projection
 * captured at preview, masking the engine-allocated ids of new paragraphs.
 */
import { createHash } from 'node:crypto';
import { parseProjection, parseProjectionLoose, serializeProjection } from './projection.js';
import type { ProjectionDoc, ProjectionParagraph } from './projection.js';
import { diffProjections } from './projection-diff.js';
import type { CorePlan, DeriveResult, PreviewRecord, ReadSource, SurfaceDeriver, VerifyEvidence } from './python-host.js';

function sha256Hex(text: string): string {
  return createHash('sha256').update(text).digest('hex');
}

/** Recursively convert Monty-transported values (Maps for dicts) to plain JS. */
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

/** Recursive deep equality; object key order is insignificant. */
function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((item, index) => deepEqual(item, b[index]));
  }
  if (isRecord(a) && isRecord(b)) {
    const aKeys = Object.keys(a).sort();
    const bKeys = Object.keys(b).sort();
    return (
      aKeys.length === bKeys.length &&
      aKeys.every((key, index) => key === bKeys[index] && deepEqual(a[key], b[key]))
    );
  }
  return false;
}

/**
 * Content equality for post-commit verification: tag, styleClass, attrs
 * (ord excluded — the engine renumbers on insert/delete), and items. Ids are
 * deliberately NOT compared here; id attribution is checked separately so
 * tracked-delete restructuring can be tolerated.
 */
function paragraphContentEqual(expected: ProjectionParagraph, actual: ProjectionParagraph): boolean {
  const attrsWithoutOrd = (p: ProjectionParagraph): Record<string, string> => {
    const attrs: Record<string, string> = {};
    for (const [key, value] of Object.entries(p.attrs)) if (key !== 'ord') attrs[key] = value;
    return attrs;
  };
  return (
    expected.tag === actual.tag &&
    expected.styleClass === actual.styleClass &&
    deepEqual(attrsWithoutOrd(expected), attrsWithoutOrd(actual)) &&
    deepEqual(expected.items, actual.items)
  );
}

/**
 * Id attribution for one paragraph pair. A new paragraph in the proposed
 * projection (null id) masks any engine-allocated id. Otherwise the ids must
 * match exactly — unless the actual id is a delete-host id: when a paragraph
 * is tracked-deleted, its element survives in the final view and hosts the
 * content of the following paragraph (the deleted paragraph's id replaces the
 * following paragraph's id), so the actual id may be a deleted id that is in
 * the commit report (affectedIds) but absent from the proposed projection.
 */
function paragraphIdCompatible(
  expected: ProjectionParagraph,
  actual: ProjectionParagraph,
  isDeleteHostId: (id: string | null) => boolean,
): boolean {
  if (expected.id === null) return true;
  if (expected.id === actual.id) return true;
  return isDeleteHostId(actual.id);
}

/** The Version 2 deriver. */
export function stringSurfaceDeriver(): SurfaceDeriver {
  return {
    name: 'string',

    digest(state: unknown): string | null {
      try {
        const plain = toPlain(state);
        if (!isRecord(plain) || typeof plain.proposed !== 'string') return null;
        return sha256Hex(plain.proposed);
      } catch {
        return null;
      }
    },

    derive(state: unknown, source: ReadSource): DeriveResult {
      const plain = toPlain(state);
      if (!isRecord(plain) || typeof plain.proposed !== 'string') {
        return { ok: false, message: 'state must contain a "proposed" string' };
      }
      // Canonicalize the original through parse+serialize on both the preview
      // and commit paths so the reconciler sees byte-identical input and
      // derives the identical canonical plan (the commit gate compares them).
      const canonical = (markup: string): string => serializeProjection(parseProjection(markup));
      let originalMarkup: string;
      if (typeof plain.original === 'string') {
        let canonicalOriginal: string;
        let canonicalCurrent: string;
        try {
          canonicalOriginal = canonical(plain.original);
          canonicalCurrent = canonical(source.markup);
        } catch (error) {
          return { ok: false, message: `markup failed to parse: ${error instanceof Error ? error.message : String(error)}` };
        }
        if (canonicalOriginal !== canonicalCurrent) {
          return { ok: false, message: 'original does not match the current document — re-read with docx_read' };
        }
        originalMarkup = canonicalOriginal;
      } else {
        // Commit path: the prelude sends only `proposed`; the gate already
        // verified the source hash, so the current render is the original.
        try {
          originalMarkup = canonical(source.markup);
        } catch (error) {
          return { ok: false, message: `current document markup failed to parse: ${error instanceof Error ? error.message : String(error)}` };
        }
      }
      const diff = diffProjections(originalMarkup, plain.proposed, { sourceSha: source.sha256 });
      if (!diff.ok) return { ok: false, message: diff.reason };
      const plan: CorePlan = {
        base: `sha256:${source.sha256}`,
        author: 'docxdriver',
        change_mode: 'track',
        ops: diff.ops,
      };
      return { ok: true, plan };
    },

    /** Capture the parsed proposed projection for post-commit verification. */
    capture(state: unknown): unknown {
      try {
        const plain = toPlain(state);
        if (!isRecord(plain) || typeof plain.proposed !== 'string') return undefined;
        return { expected: parseProjectionLoose(plain.proposed) };
      } catch {
        return undefined;
      }
    },

    async verifyCommit(record: PreviewRecord, evidence: VerifyEvidence): Promise<string | null> {
      const captured = record.deriverData as { expected?: ProjectionDoc } | undefined;
      const expected = captured?.expected;
      if (!expected || !Array.isArray(expected.paragraphs)) {
        return 'post-commit render verification: no expected projection was stored at preview';
      }
      let post: ProjectionDoc;
      try {
        post = parseProjection(evidence.postMarkup);
      } catch (error) {
        return `post-commit render verification: committed document markup failed to parse: ${error instanceof Error ? error.message : String(error)}`;
      }

      const expectedIds = new Set<string>();
      for (const p of expected.paragraphs) if (p.id !== null) expectedIds.add(p.id);
      const affected = new Set(evidence.affectedIds);
      // The deleted ids come from the plan itself: this engine's
      // delete_paragraphs report carries no `affected` entries, so
      // evidence.affectedIds is empty for a pure delete. The plan's
      // canonical JSON (stored on the preview record) carries the
      // delete_paragraphs at-ids.
      const deletedIds = new Set<string>();
      try {
        const plan = JSON.parse(record.canonicalPlan) as { ops?: Array<{ op?: string; at?: unknown }> };
        for (const op of plan.ops ?? []) {
          if (op.op === 'delete_paragraphs' && Array.isArray(op.at)) {
            for (const id of op.at) if (typeof id === 'string') deletedIds.add(id.toUpperCase());
          }
        }
      } catch {
        // canonicalPlan is host-produced JSON; a parse failure is unexpected.
        // Fall back to the affectedIds-based rule only.
      }
      // A delete-host id is a deleted paragraph's id: it appears in the plan
      // (or the commit report) but never in the proposed projection (the
      // paragraph was removed from the draft).
      const isDeleteHostId = (id: string | null): boolean => {
        if (id === null) return false;
        const upper = id.toUpperCase();
        return deletedIds.has(upper) || (affected.has(upper) && !expectedIds.has(upper));
      };
      const isEmpty = (p: ProjectionParagraph): boolean => p.items.length === 0 && p.plainText === '';

      // Walk both lists in document order. Tracked paragraph deletion leaves
      // the deleted element in the final view: an empty remnant when there is
      // no mergeable following block, or the following paragraph's content
      // merged under the deleted paragraph's id. Both shapes are legitimate;
      // the walk tolerates them while still failing on wrong content, missing
      // paragraphs, or truly extra non-empty paragraphs.
      let i = 0; // post index
      let j = 0; // expected index
      while (i < post.paragraphs.length && j < expected.paragraphs.length) {
        const actual = post.paragraphs[i];
        const wanted = expected.paragraphs[j];
        // Skip a delete remnant: an empty paragraph whose id was deleted.
        // Guard: never skip a paragraph that corresponds to an id-less empty
        // paragraph the agent explicitly proposed (an inserted empty
        // paragraph is legitimately empty and id-less on the expected side).
        if (isEmpty(actual) && isDeleteHostId(actual.id) && !(wanted.id === null && isEmpty(wanted))) {
          i += 1;
          continue;
        }
        if (!paragraphContentEqual(wanted, actual)) {
          return `post-commit render verification: paragraph ${j + 1} differs from the proposed projection`;
        }
        if (!paragraphIdCompatible(wanted, actual, isDeleteHostId)) {
          return `post-commit render verification: paragraph ${j + 1} has an unexpected paragraph id`;
        }
        i += 1;
        j += 1;
      }
      if (j < expected.paragraphs.length) {
        return `post-commit render verification: paragraph ${j + 1} is missing from the committed document`;
      }
      for (; i < post.paragraphs.length; i += 1) {
        const leftover = post.paragraphs[i];
        if (!(isEmpty(leftover) && isDeleteHostId(leftover.id))) {
          return `post-commit render verification: paragraph ${i + 1} is extra in the committed document`;
        }
      }
      return null;
    },
  };
}
