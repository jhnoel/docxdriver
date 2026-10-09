/**
 * Version 3 deriver (structured Python document model surface): turns the
 * serialized model state the prelude sends (per-paragraph original markup +
 * recorded change sets + insertions) into a typed core plan by REPLAYING the
 * change set against each paragraph's running text. The replay validates
 * every selection against the text at that point (stale selections block
 * with a diagnostic naming the paragraph), emits ops with occurrence anchors
 * computed from the running text, and builds the expected post-commit
 * projection for verification. The digest is sha256 of the full
 * canonicalized serialization, so any mutation after preview invalidates the
 * stored key. `verifyCommit` mirrors the V2 walk (final view, insert-id
 * masking, tracked-delete remnant/merge tolerance, deleted ids from the
 * gate-pinned canonical plan).
 */
import { createHash } from 'node:crypto';
import { parseProjection, paragraphMarkup } from './projection.js';
import type { FormatFlags, ParagraphTag, ProjectionDoc, ProjectionItem, ProjectionParagraph } from './projection.js';
import { allocateInsertId } from './projection-diff.js';
import type { CorePlan, DeriveResult, PreviewRecord, ReadSource, SurfaceDeriver, VerifyEvidence } from './python-host.js';

const ID_RE = /^[0-9A-F]{8}$/;

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

function blankFmt(): FormatFlags {
  return { bold: false, italic: false, underline: false, strike: false, superscript: false, subscript: false };
}

function fmtEq(a: FormatFlags, b: FormatFlags): boolean {
  return (
    a.bold === b.bold &&
    a.italic === b.italic &&
    a.underline === b.underline &&
    a.strike === b.strike &&
    a.superscript === b.superscript &&
    a.subscript === b.subscript
  );
}

/** Mirror crates/docxdriver-core/src/html/mod.rs::escape_text (& < > only). */
function escapeText(text: string): string {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function quote(text: string): string {
  return `'${text.replace(/'/g, "\\'")}'`;
}

/** 1-based occurrence lookup: start offset of the n-th match, or -1. */
function nthMatch(text: string, needle: string, n: number): number {
  let idx = -1;
  for (let i = 0; i < n; i += 1) {
    idx = text.indexOf(needle, idx + 1);
    if (idx === -1) return -1;
  }
  return idx;
}

function countMatches(text: string, needle: string): number {
  let count = 0;
  let idx = -1;
  for (;;) {
    idx = text.indexOf(needle, idx + 1);
    if (idx === -1) return count;
    count += 1;
  }
}

// ---- segment model for expected-content reconstruction ----
// A paragraph is a list of segments: text (chars with one fmt) or opaque
// (protected items re-emitted verbatim). Text changes and format changes are
// replayed against the segment list exactly as the engine applies ops to the
// document, so the reconstructed items match the committed final render.

type Segment =
  | { kind: 'text'; text: string; fmt: FormatFlags }
  | { kind: 'opaque'; item: ProjectionItem; dropped: boolean };

function segmentsFrom(items: ProjectionItem[]): Segment[] {
  return items.map((item) => (item.kind === 'text' ? { kind: 'text' as const, text: item.text, fmt: item.fmt } : { kind: 'opaque' as const, item, dropped: false }));
}

function flatText(segs: Segment[]): string {
  return segs.map((s) => (s.kind === 'text' ? s.text : s.item.text)).join('');
}

/** Replace [start, end) of the flat text with `replacement`. The replacement
 * segment carries the given fmt: for the model surface the `with` is always
 * plain text, and the engine builds the replacement runs from parse_with(with)
 * pieces carrying THEIR OWN fmt — so a plain replacement inserts PLAIN runs
 * even over a formatted span (the deleted span keeps its runs inside w:del;
 * the final view shows the plain replacement). Returns true when the change
 * crossed an opaque segment (protected content). */
function applyTextChange(segs: Segment[], start: number, end: number, replacement: string, fmt: FormatFlags): boolean {
  let crossed = false;
  const out: Segment[] = [];
  let pos = 0;
  for (const s of segs) {
    const len = s.kind === 'text' ? s.text.length : s.item.text.length;
    const sStart = pos;
    const sEnd = pos + len;
    pos = sEnd;
    if (sEnd <= start || sStart >= end) {
      out.push(s);
      continue;
    }
    if (s.kind === 'opaque') {
      s.dropped = true;
      crossed = true;
      continue;
    }
    const relStart = Math.max(0, start - sStart);
    const relEnd = Math.min(len, end - sStart);
    if (relStart > 0) out.push({ kind: 'text', text: s.text.slice(0, relStart), fmt: s.fmt });
    if (sStart + relStart === start) out.push({ kind: 'text', text: replacement, fmt });
    if (relEnd < len) out.push({ kind: 'text', text: s.text.slice(relEnd), fmt: s.fmt });
  }
  segs.length = 0;
  segs.push(...out);
  return crossed;
}

/** Apply a format change to [start, end) of the flat text. */
function applyFmtChange(segs: Segment[], start: number, end: number, flags: Record<string, boolean>): boolean {
  let crossed = false;
  const out: Segment[] = [];
  let pos = 0;
  for (const s of segs) {
    const len = s.kind === 'text' ? s.text.length : s.item.text.length;
    const sStart = pos;
    const sEnd = pos + len;
    pos = sEnd;
    if (sEnd <= start || sStart >= end) {
      out.push(s);
      continue;
    }
    if (s.kind === 'opaque') {
      s.dropped = true;
      crossed = true;
      continue;
    }
    const relStart = Math.max(0, start - sStart);
    const relEnd = Math.min(len, end - sStart);
    if (relStart > 0) out.push({ kind: 'text', text: s.text.slice(0, relStart), fmt: s.fmt });
    const midFmt = { ...s.fmt };
    for (const [key, value] of Object.entries(flags)) {
      if (key in midFmt) midFmt[key as keyof FormatFlags] = value;
    }
    out.push({ kind: 'text', text: s.text.slice(relStart, relEnd), fmt: midFmt });
    if (relEnd < len) out.push({ kind: 'text', text: s.text.slice(relEnd), fmt: s.fmt });
  }
  segs.length = 0;
  segs.push(...out);
  return crossed;
}

/** Merge adjacent text segments with equal fmt; drop crossed opaque segments. */
function finalizeSegments(segs: Segment[]): ProjectionItem[] {
  const items: ProjectionItem[] = [];
  for (const s of segs) {
    if (s.kind === 'opaque') {
      if (!s.dropped) items.push(s.item);
      continue;
    }
    const last = items[items.length - 1];
    if (last !== undefined && last.kind === 'text' && fmtEq(last.fmt, s.fmt)) {
      last.text += s.text;
    } else {
      items.push({ kind: 'text', text: s.text, fmt: { ...s.fmt } });
    }
  }
  return items;
}

// ---- per-paragraph state (after Monty Map conversion) ----

type TextChange = { expected: string; new: string; occurrence: number | null };
type FmtChange = { expected: string; occurrence: number | null; fmt: Record<string, boolean> };
type ModelParagraphState = {
  id: string;
  original_markup: string;
  original_text: string;
  text: string;
  style: string | null;
  deleted: boolean;
  text_changes: TextChange[];
  fmt_changes: FmtChange[];
  style_changes: string[];
};
type ModelInsertion = { anchor: string; position: 'before' | 'after'; text: string; style: string | null };
type ModelState = {
  path: string;
  source_hash: string;
  paragraphs: ModelParagraphState[];
  insertions: ModelInsertion[];
};

type ReplayOk = {
  ok: true;
  ops: Array<Record<string, unknown>>;
  items: ProjectionItem[];
  plainText: string;
  tag: ParagraphTag;
  styleClass: string | null;
  attrs: Record<string, string>;
};
type ReplayResult = ReplayOk | { ok: false; message: string };

/** Map a paragraph style name to the projection's tag/class pair, mirroring
 * crates/docxdriver-core/src/html/render.rs::paragraph_tag. */
function styleToTag(style: string | null): { tag: ParagraphTag; styleClass: string | null } {
  if (style !== null) {
    const match = /^Heading([1-6])$/.exec(style);
    if (match) return { tag: `h${match[1]}` as ParagraphTag, styleClass: null };
  }
  return { tag: 'p', styleClass: style };
}

function parseOriginal(originalMarkup: string): ProjectionParagraph | null {
  try {
    const doc = parseProjection(originalMarkup);
    if (doc.paragraphs.length !== 1) return null;
    return doc.paragraphs[0];
  } catch {
    return null;
  }
}

/**
 * Replay one paragraph's change set against its parsed original, emitting the
 * typed ops (replace_text / replace_paragraph / format_text /
 * format_paragraph) and reconstructing the expected post-edit items.
 */
function replayParagraph(original: ProjectionParagraph, id: string, p: ModelParagraphState): ReplayResult {
  const segs = segmentsFrom(original.items);
  const ops: Array<Record<string, unknown>> = [];
  let running = original.plainText;

  const whole = p.text_changes.length === 1 && p.text_changes[0].occurrence === null;
  if (whole) {
    if (p.fmt_changes.length > 0) {
      return { ok: false, message: `paragraph ${id} mixes a whole-paragraph replacement with format changes` };
    }
    const newText = p.text_changes[0].new;
    if (typeof newText !== 'string') {
      return { ok: false, message: `paragraph ${id} has a non-string whole-paragraph replacement` };
    }
    ops.push({ op: 'replace_paragraph', at: id, with: escapeText(newText) });
    running = newText;
    segs.length = 0;
    segs.push({ kind: 'text', text: newText, fmt: blankFmt() });
  } else {
    for (const tc of p.text_changes) {
      if (!isRecord(tc) || typeof tc.expected !== 'string' || typeof tc.new !== 'string') {
        return { ok: false, message: `paragraph ${id} has a malformed text change` };
      }
      const n = tc.occurrence === null || tc.occurrence === undefined ? 1 : tc.occurrence;
      if (!Number.isInteger(n) || n < 1) {
        return { ok: false, message: `paragraph ${id} has an invalid text-change occurrence` };
      }
      if (tc.expected === '') {
        return { ok: false, message: `paragraph ${id} has an empty replace selector` };
      }
      const start = nthMatch(running, tc.expected, n);
      if (start === -1) {
        const truncated = running.length > 200 ? `${running.slice(0, 200)}…` : running;
        return {
          ok: false,
          message: `stale selection in paragraph ${id}: expected ${quote(tc.expected)} at occurrence ${n}, current text is ${quote(truncated)}`,
        };
      }
      const count = countMatches(running, tc.expected);
      applyTextChange(segs, start, start + tc.expected.length, tc.new, blankFmt());
      running = flatText(segs);
      ops.push({
        op: 'replace_text',
        at: id,
        select: tc.expected,
        with: escapeText(tc.new),
        ...(count > 1 ? { occurrence: n } : {}),
      });
    }
    for (const fc of p.fmt_changes) {
      if (!isRecord(fc) || typeof fc.expected !== 'string' || !isRecord(fc.fmt)) {
        return { ok: false, message: `paragraph ${id} has a malformed format change` };
      }
      const n = fc.occurrence === null || fc.occurrence === undefined ? 1 : fc.occurrence;
      if (!Number.isInteger(n) || n < 1) {
        return { ok: false, message: `paragraph ${id} has an invalid format-change occurrence` };
      }
      const flags: Record<string, boolean> = {};
      for (const [key, value] of Object.entries(fc.fmt)) {
        if (typeof value !== 'boolean') {
          return { ok: false, message: `paragraph ${id} format change ${key} must be a boolean` };
        }
        flags[key] = value;
      }
      if (Object.keys(flags).length === 0) continue;
      const start = nthMatch(running, fc.expected, n);
      if (start === -1) {
        const truncated = running.length > 200 ? `${running.slice(0, 200)}…` : running;
        return {
          ok: false,
          message: `stale selection in paragraph ${id}: expected ${quote(fc.expected)} at occurrence ${n}, current text is ${quote(truncated)}`,
        };
      }
      const count = countMatches(running, fc.expected);
      applyFmtChange(segs, start, start + fc.expected.length, flags);
      ops.push({
        op: 'format_text',
        at: id,
        select: fc.expected,
        ...(count > 1 ? { occurrence: n } : {}),
        ...flags,
      });
    }
  }

  let tag = original.tag;
  let styleClass = original.styleClass;
  if (Array.isArray(p.style_changes) && p.style_changes.length > 0) {
    const last = p.style_changes[p.style_changes.length - 1];
    if (typeof last !== 'string' || last === '') {
      return { ok: false, message: `paragraph ${id} has an invalid style change` };
    }
    ops.push({ op: 'format_paragraph', at: id, style: last });
    const mapped = styleToTag(last);
    tag = mapped.tag;
    styleClass = mapped.styleClass;
  }

  return {
    ok: true,
    ops,
    items: finalizeSegments(segs),
    plainText: running,
    tag,
    styleClass,
    attrs: original.attrs,
  };
}

/** Normalize and validate the Monty-transported state into ModelState. */
function normalizeState(state: unknown): ModelState | { message: string } {
  const plain = toPlain(state);
  if (
    !isRecord(plain) ||
    !Array.isArray(plain.paragraphs) ||
    !Array.isArray(plain.insertions) ||
    typeof plain.source_hash !== 'string' ||
    typeof plain.path !== 'string'
  ) {
    return { message: 'state must be a serialized document model (paragraphs and insertions)' };
  }
  const paragraphs: ModelParagraphState[] = [];
  for (const raw of plain.paragraphs) {
    if (!isRecord(raw)) return { message: 'state has a malformed paragraph' };
    const id = typeof raw.id === 'string' ? raw.id.toUpperCase() : '';
    if (!ID_RE.test(id)) return { message: `state has a malformed paragraph id (expected eight uppercase hex digits)` };
    if (typeof raw.original_markup !== 'string') return { message: `paragraph ${id} is missing its original markup` };
    if (!Array.isArray(raw.text_changes) || !Array.isArray(raw.fmt_changes) || !Array.isArray(raw.style_changes)) {
      return { message: `paragraph ${id} has malformed change sets` };
    }
    paragraphs.push({
      id,
      original_markup: raw.original_markup,
      original_text: typeof raw.original_text === 'string' ? raw.original_text : '',
      text: typeof raw.text === 'string' ? raw.text : '',
      style: raw.style === null || raw.style === undefined ? null : typeof raw.style === 'string' ? raw.style : null,
      deleted: raw.deleted === true,
      text_changes: raw.text_changes as TextChange[],
      fmt_changes: raw.fmt_changes as FmtChange[],
      style_changes: raw.style_changes as string[],
    });
  }
  const insertions: ModelInsertion[] = [];
  for (const raw of plain.insertions) {
    if (!isRecord(raw) || typeof raw.anchor !== 'string' || typeof raw.text !== 'string') {
      return { message: 'state has a malformed insertion' };
    }
    const position = raw.position === 'before' ? 'before' : raw.position === 'after' ? 'after' : null;
    if (position === null) return { message: `insertion after ${raw.anchor} has an invalid position` };
    insertions.push({
      anchor: raw.anchor.toUpperCase(),
      position,
      text: raw.text,
      style: raw.style === null || raw.style === undefined ? null : typeof raw.style === 'string' ? raw.style : null,
    });
  }
  return { paragraphs, insertions, path: plain.path, source_hash: plain.source_hash };
}

function isModelState(value: ModelState | { message: string }): value is ModelState {
  return Array.isArray((value as ModelState).paragraphs);
}

/** The expected post-commit projection: survivors replayed, insertions
 * interleaved at their anchors (id-less), deleted paragraphs removed. */
function buildExpectedDoc(state: ModelState): ProjectionDoc {
  const byAnchor = new Map<string, ModelInsertion[]>();
  for (const ins of state.insertions) {
    const list = byAnchor.get(ins.anchor) ?? [];
    list.push(ins);
    byAnchor.set(ins.anchor, list);
  }
  const paragraphs: ProjectionParagraph[] = [];
  for (const p of state.paragraphs) {
    if (p.deleted) continue;
    const original = parseOriginal(p.original_markup);
    if (original === null) continue; // derive already blocked; best effort
    const replayed = replayParagraph(original, p.id, p);
    if (!replayed.ok) continue; // derive already blocked; best effort
    const expected: ProjectionParagraph = {
      id: p.id,
      tag: replayed.tag,
      styleClass: replayed.styleClass,
      attrs: replayed.attrs,
      items: replayed.items,
      plainText: replayed.plainText,
      markup: '',
    };
    expected.markup = paragraphMarkup(expected);
    const before = (byAnchor.get(p.id) ?? []).filter((ins) => ins.position === 'before');
    const after = (byAnchor.get(p.id) ?? []).filter((ins) => ins.position === 'after');
    for (const ins of before) paragraphs.push(insertedParagraph(ins));
    paragraphs.push(expected);
    for (const ins of after) paragraphs.push(insertedParagraph(ins));
  }
  return { paragraphs, blocks: paragraphs.map((paragraph) => ({ kind: 'paragraph', paragraph })) };
}

function insertedParagraph(ins: ModelInsertion): ProjectionParagraph {
  const mapped = styleToTag(ins.style);
  const paragraph: ProjectionParagraph = {
    id: null,
    tag: mapped.tag,
    styleClass: mapped.styleClass,
    attrs: {},
    items: [{ kind: 'text', text: ins.text, fmt: blankFmt() }],
    plainText: ins.text,
    markup: '',
  };
  paragraph.markup = paragraphMarkup(paragraph);
  return paragraph;
}

// ---- post-commit verification (mirrors the V2 walk) ----

/** Content equality: tag, styleClass, attrs (ord excluded), items. Ids are
 * checked separately so tracked-delete restructuring can be tolerated. */
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

function paragraphIdCompatible(
  expected: ProjectionParagraph,
  actual: ProjectionParagraph,
  isDeleteHostId: (id: string | null) => boolean,
): boolean {
  if (expected.id === null) return true;
  if (expected.id === actual.id) return true;
  return isDeleteHostId(actual.id);
}

/** The Version 3 deriver. */
export function modelSurfaceDeriver(): SurfaceDeriver {
  return {
    name: 'model',

    digest(state: unknown): string | null {
      try {
        const plain = toPlain(state);
        if (!isRecord(plain) || !Array.isArray(plain.paragraphs) || !Array.isArray(plain.insertions)) return null;
        return sha256Hex(JSON.stringify(canonicalSorted(plain)));
      } catch {
        return null;
      }
    },

    derive(state: unknown, source: ReadSource): DeriveResult {
      const normalized = normalizeState(state);
      if (!isModelState(normalized)) return { ok: false, message: normalized.message };
      if (normalized.source_hash !== `sha256:${source.sha256}`) {
        return { ok: false, message: 'document changed since open — reopen with docx_open' };
      }

      const ops: Array<Record<string, unknown>> = [];
      let deleteEmitted = false;
      const byAnchor = new Map<string, ModelInsertion[]>();
      for (const ins of normalized.insertions) {
        const list = byAnchor.get(ins.anchor) ?? [];
        list.push(ins);
        byAnchor.set(ins.anchor, list);
      }
      const deleted = new Set(normalized.paragraphs.filter((p) => p.deleted).map((p) => p.id));
      for (const ins of normalized.insertions) {
        if (deleted.has(ins.anchor)) {
          return { ok: false, message: `cannot insert ${ins.position} paragraph ${quote(ins.text)} — anchor paragraph ${ins.anchor} is deleted` };
        }
      }
      const allocatedAliases = new Set<string>();
      let chainCounter = 0;

      for (const p of normalized.paragraphs) {
        if (p.deleted) {
          if (p.text_changes.length > 0 || p.fmt_changes.length > 0 || p.style_changes.length > 0) {
            return { ok: false, message: `deleted paragraph ${p.id} has pending changes` };
          }
          if (!deleteEmitted) {
            ops.push({ op: 'delete_paragraphs', at: normalized.paragraphs.filter((q) => q.deleted).map((q) => q.id) });
            deleteEmitted = true;
          }
          continue;
        }
        const original = parseOriginal(p.original_markup);
        if (original === null) {
          return { ok: false, message: `paragraph ${p.id} original markup failed to parse` };
        }
        const replayed = replayParagraph(original, p.id, p);
        if (!replayed.ok) return { ok: false, message: replayed.message };

        // Insertions anchored at this paragraph. Chains are per-position: a run
        // of same-position inserts preserves order by aliasing — the first
        // insert anchors at the real paragraph with its computed position and
        // as=alias1; each subsequent insert anchors at the previous alias
        // ($-prefixed, position after). Different positions at one anchor are
        // independent (before-inserts precede the paragraph, after-inserts
        // follow it), so they are emitted as separate single-op chains.
        const run = byAnchor.get(p.id) ?? [];
        const emitChain = (chain: ModelInsertion[]): void => {
          let prevAs: string | null = null;
          for (let k = 0; k < chain.length; k += 1) {
            const ins = chain[k];
            const op: Record<string, unknown> = {
              op: 'insert_paragraph',
              at: prevAs === null ? p.id : `$${prevAs}`,
              position: prevAs === null ? ins.position : 'after',
              with: escapeText(ins.text),
            };
            if (ins.style !== null) op.style = ins.style;
            if (k + 1 < chain.length) {
              const allocated = allocateInsertId(source.sha256, chainCounter, allocatedAliases);
              op.as = allocated;
              allocatedAliases.add(allocated);
              prevAs = allocated;
              chainCounter += 1;
            } else {
              prevAs = null;
            }
            ops.push(op);
          }
        };
        emitChain(run.filter((ins) => ins.position === 'before'));
        ops.push(...replayed.ops);
        emitChain(run.filter((ins) => ins.position === 'after'));
      }

      const plan: CorePlan = {
        base: `sha256:${source.sha256}`,
        author: 'docxdriver',
        change_mode: 'track',
        ops: ops as CorePlan['ops'],
      };
      return { ok: true, plan };
    },

    /** Capture the expected post-commit projection for verification. */
    capture(state: unknown): unknown {
      try {
        const normalized = normalizeState(state);
        if (!isModelState(normalized)) return undefined;
        return { expected: buildExpectedDoc(normalized) };
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
      // Deleted ids come from the plan itself (gate-pinned canonical JSON):
      // this engine's delete_paragraphs report carries no `affected` entries.
      const deletedIds = new Set<string>();
      try {
        const plan = JSON.parse(record.canonicalPlan) as { ops?: Array<{ op?: string; at?: unknown }> };
        for (const op of plan.ops ?? []) {
          if (op.op === 'delete_paragraphs' && Array.isArray(op.at)) {
            for (const id of op.at) if (typeof id === 'string') deletedIds.add(id.toUpperCase());
          }
        }
      } catch {
        // Fall back to the affectedIds-based rule only.
      }
      const isDeleteHostId = (id: string | null): boolean => {
        if (id === null) return false;
        const upper = id.toUpperCase();
        return deletedIds.has(upper) || (affected.has(upper) && !expectedIds.has(upper));
      };
      const isEmpty = (p: ProjectionParagraph): boolean => p.items.length === 0 && p.plainText === '';

      let i = 0; // post index
      let j = 0; // expected index
      while (i < post.paragraphs.length && j < expected.paragraphs.length) {
        const actual = post.paragraphs[i];
        const wanted = expected.paragraphs[j];
        if (isEmpty(actual) && isDeleteHostId(actual.id) && !(wanted.id === null && isEmpty(wanted))) {
          i += 1;
          continue;
        }
        if (!paragraphContentEqual(wanted, actual)) {
          return `post-commit render verification: paragraph ${j + 1} differs from the expected model state`;
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

/**
 * Deterministic canonical JSON (sorted object keys, array order preserved).
 * Local copy kept out of python-host's exports: the digest only needs
 * stability, and V2/V3 digests must not depend on unrelated host churn.
 */
function canonicalSorted(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalSorted);
  if (value !== null && typeof value === 'object') {
    const out: Record<string, unknown> = Object.create(null);
    for (const key of Object.keys(value as Record<string, unknown>).sort()) {
      out[key] = canonicalSorted((value as Record<string, unknown>)[key]);
    }
    return out;
  }
  return value;
}
