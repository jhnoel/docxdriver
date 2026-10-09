/**
 * Projection reconciler (Version 2 core).
 *
 * Compiles an original-vs-proposed canonical projection diff into the narrowest
 * safe typed core operations (`runPlan` op shapes), or rejects malformed,
 * ambiguous, or unsupported changes with a model-readable reason. It never
 * rebuilds the DOCX from the string — it only emits typed ops for the engine.
 *
 * Span model: the two plain texts are tokenized into word runs
 * (`[A-Za-z0-9]+`) and separator runs, then split with a token-level
 * longest-common-subsequence LCS. Because tokens are atomic, word-internal
 * differences (e.g. `thirty`→`sixty`) stay one span while repeated
 * space-separated edits (`30 30 30`→`45 45 45`) decompose into one op per
 * token. Empty-side pairs (pure insert/delete) merge into the previous changed
 * pair (extending across the intervening equal text) or extend backward into
 * the preceding equal block (forward when at the very start). A single
 * ambiguous select is phrase-anchored by expanding it left by one word
 * (test contract: `the terms` with `occurrence: 3`, never the ambiguous bare
 * `terms`).
 */
import { createHash } from 'node:crypto';
import { parseProjection, parseProjectionLoose, ProjectionError } from './projection.js';
import type { FormatFlags, ProjectionItem, ProjectionParagraph } from './projection.js';

export type CoreOp = { op: string; [key: string]: unknown };
export type DiffResult = { ok: true; ops: CoreOp[] } | { ok: false; reason: string };

const FMT_KEYS: Array<keyof FormatFlags> = ['bold', 'italic', 'underline', 'strike', 'superscript', 'subscript'];
const FMT_TAGS: Record<keyof FormatFlags, string> = { bold: 'b', italic: 'i', underline: 'u', strike: 's', superscript: 'sup', subscript: 'sub' };
const PROTECTED_KINDS = new Set(['link', 'field', 'image', 'equation', 'table', 'protected', 'revision']);
const PARA_ATTRS = ['ord', 'num', 'break-ins', 'break-del'] as const;
const WORD_TEST = /^[A-Za-z0-9]+$/;

function blankFmt(): FormatFlags {
  return { bold: false, italic: false, underline: false, strike: false, superscript: false, subscript: false };
}

function fmtTags(fmt: FormatFlags): string[] {
  return FMT_KEYS.filter((key) => fmt[key]).map((key) => FMT_TAGS[key]);
}

function fmtEq(a: FormatFlags, b: FormatFlags): boolean {
  return FMT_KEYS.every((key) => a[key] === b[key]);
}

/** Mirror projection.ts's emitter: keep the longest common fmt prefix, close after it, reopen. */
function transitionFmt(open: FormatFlags, target: FormatFlags): string {
  const openTags = fmtTags(open);
  const targetTags = fmtTags(target);
  let keep = 0;
  while (keep < openTags.length && keep < targetTags.length && openTags[keep] === targetTags[keep]) keep += 1;
  let out = '';
  for (let i = openTags.length - 1; i >= keep; i -= 1) out += `</${openTags[i]}>`;
  for (let i = keep; i < targetTags.length; i += 1) out += `<${targetTags[i]}>`;
  return out;
}

function closeFmtTags(fmt: FormatFlags): string {
  return fmtTags(fmt).reverse().map((tag) => `</${tag}>`).join('');
}

function escapeText(text: string): string {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

function decodeEntities(text: string): string {
  return text.replace(/&(amp|lt|gt|quot);/g, (_, name: string) =>
    name === 'amp' ? '&' : name === 'lt' ? '<' : name === 'gt' ? '>' : '"',
  );
}

/** Strip tags from a verbatim inner slice and decode entities → plain text. */
function innerPlain(raw: string): string {
  return decodeEntities(raw.replace(/<[^>]*>/g, ''));
}

/** Plain-text contribution of one item (mirrors projection.ts::itemPlain). */
function plainLen(item: ProjectionItem): number {
  switch (item.kind) {
    case 'text':
      return item.text.length;
    case 'link':
    case 'field':
    case 'revision':
    case 'equation':
    case 'image':
    case 'table':
    case 'protected':
      return innerPlain(item.text).length;
  }
}

/** Per-character format flags over a paragraph's plain text. */
function fmtPerChar(p: ProjectionParagraph): FormatFlags[] {
  const out: FormatFlags[] = [];
  for (const item of p.items) {
    const f = item.kind === 'text' ? item.fmt : blankFmt();
    for (let i = 0; i < plainLen(item); i += 1) out.push(f);
  }
  return out;
}

type Token = { text: string; start: number; end: number };

/** Word runs + separator runs, each with its char span. */
function tokenize(text: string): Token[] {
  const out: Token[] = [];
  let pos = 0;
  const re = /[A-Za-z0-9]+/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(text)) !== null) {
    if (match.index > pos) out.push({ text: text.slice(pos, match.index), start: pos, end: match.index });
    out.push({ text: match[0], start: match.index, end: match.index + match[0].length });
    pos = match.index + match[0].length;
  }
  if (pos < text.length) out.push({ text: text.slice(pos), start: pos, end: text.length });
  return out;
}

type Opcode = {
  type: 'equal' | 'replace' | 'insert' | 'delete';
  aStart: number; // char span in the original plain text
  aEnd: number;
  bStart: number; // char span in the proposed plain text
  bEnd: number;
};

/** Token-level LCS split with difflib-style get_opcodes semantics. */
function lcsTokens(a: Token[], b: Token[]): Opcode[] {
  const n = a.length;
  const m = b.length;
  const rows: number[][] = [new Array<number>(m + 1).fill(0)];
  for (let i = 1; i <= n; i += 1) {
    const row = new Array<number>(m + 1).fill(0);
    const prev = rows[i - 1];
    for (let j = 1; j <= m; j += 1) {
      row[j] = a[i - 1].text === b[j - 1].text ? prev[j - 1] + 1 : Math.max(prev[j], row[j - 1]);
    }
    rows.push(row);
  }
  const ops: Opcode[] = [];
  let i = n;
  let j = m;
  // Empty-side opcodes carry the insertion/deletion POINT (the position where
  // the other text is empty), so the later min/max merge yields correct spans.
  const insertPoint = (): number => (i > 0 ? a[i - 1].end : 0);
  const deletePoint = (): number => (j > 0 ? b[j - 1].end : 0);
  while (i > 0 && j > 0) {
    if (a[i - 1].text === b[j - 1].text) {
      ops.push({ type: 'equal', aStart: a[i - 1].start, aEnd: a[i - 1].end, bStart: b[j - 1].start, bEnd: b[j - 1].end });
      i -= 1;
      j -= 1;
    } else if (rows[i - 1][j] >= rows[i][j - 1]) {
      const point = deletePoint();
      ops.push({ type: 'delete', aStart: a[i - 1].start, aEnd: a[i - 1].end, bStart: point, bEnd: point });
      i -= 1;
    } else {
      const point = insertPoint();
      ops.push({ type: 'insert', aStart: point, aEnd: point, bStart: b[j - 1].start, bEnd: b[j - 1].end });
      j -= 1;
    }
  }
  while (i > 0) {
    const point = deletePoint();
    ops.push({ type: 'delete', aStart: a[i - 1].start, aEnd: a[i - 1].end, bStart: point, bEnd: point });
    i -= 1;
  }
  while (j > 0) {
    const point = insertPoint();
    ops.push({ type: 'insert', aStart: point, aEnd: point, bStart: b[j - 1].start, bEnd: b[j - 1].end });
    j -= 1;
  }
  ops.reverse();
  // Merge adjacent insert+delete opcodes into replace pairs, and collapse
  // consecutive insert (or delete) runs into one opcode so the clean-check and
  // empty-side handling see a single structural op.
  const merged: Opcode[] = [];
  for (const op of ops) {
    const last = merged[merged.length - 1];
    if (
      last !== undefined &&
      last.type === op.type &&
      (op.type === 'insert' || op.type === 'delete')
    ) {
      merged[merged.length - 1] = {
        type: op.type,
        aStart: Math.min(last.aStart, op.aStart),
        aEnd: Math.max(last.aEnd, op.aEnd),
        bStart: Math.min(last.bStart, op.bStart),
        bEnd: Math.max(last.bEnd, op.bEnd),
      };
    } else if (
      last !== undefined &&
      ((last.type === 'insert' && op.type === 'delete') || (last.type === 'delete' && op.type === 'insert'))
    ) {
      merged[merged.length - 1] = {
        type: 'replace',
        aStart: Math.min(last.aStart, op.aStart),
        aEnd: Math.max(last.aEnd, op.aEnd),
        bStart: Math.min(last.bStart, op.bStart),
        bEnd: Math.max(last.bEnd, op.bEnd),
      };
    } else {
      merged.push(op);
    }
  }
  return merged;
}

type Pair = {
  select: string;
  with: string;
  origStart: number;
  propStart: number;
  propEnd: number;
  extended: boolean;
};

function isWordChar(ch: string): boolean {
  return ch !== '' && /[A-Za-z0-9]/.test(ch);
}

function commonPrefix(a: string, b: string): number {
  let i = 0;
  while (i < a.length && i < b.length && a[i] === b[i]) i += 1;
  return i;
}

function commonSuffix(a: string, b: string): number {
  let i = 0;
  while (i < a.length && i < b.length && a[a.length - 1 - i] === b[b.length - 1 - i]) i += 1;
  return i;
}

/**
 * Character-level common prefix/suffix, then adjust both to word boundaries so
 * a replace never splits a word (test contract: `thirty`→`sixty` stays whole,
 * never `thir`→`six`). Pure insertions/deletions keep the raw trim — they are
 * anchored by their surrounding equal text instead.
 */
function alignedTrim(o: string, p: string): { prefixLen: number; suffixLen: number } {
  let pl = commonPrefix(o, p);
  const to0 = o.slice(pl);
  const tp0 = p.slice(pl);
  let sl = commonSuffix(to0, tp0);
  const middleIsReplace = pl < o.length - sl && pl < p.length - sl;
  if (middleIsReplace) {
    while (pl > 0 && isWordChar(o[pl - 1]) && isWordChar(o[pl] ?? '')) pl -= 1;
    const to = o.slice(pl);
    const tp = p.slice(pl);
    sl = commonSuffix(to, tp);
    while (sl > 0 && isWordChar(to[to.length - sl]) && isWordChar(to[to.length - sl - 1] ?? '')) sl -= 1;
  }
  return { prefixLen: pl, suffixLen: sl };
}

/**
 * Collapse the opcode stream into final text pairs. Equal blocks stay pending;
 * an empty-side pair merges into the previous changed pair (extending across
 * the pending equal text) or, with no previous pair, extends backward into the
 * preceding equal block (forward when at the very start).
 */
function buildPairs(
  opcodes: Opcode[],
  origPlain: string,
  propPlain: string,
  suffixLen: number,
): Pair[] {
  const pairs: Pair[] = [];
  let pendingEqual = '';
  let consumed = 0;
  const n = opcodes.length;
  const suffix = origPlain.slice(origPlain.length - suffixLen);
  while (consumed < n) {
    const op = opcodes[consumed];
    if (op.type === 'equal') {
      pendingEqual += origPlain.slice(op.aStart, op.aEnd);
      consumed += 1;
      continue;
    }
    if (op.type === 'replace') {
      const select = origPlain.slice(op.aStart, op.aEnd);
      const with_ = propPlain.slice(op.bStart, op.bEnd);
      const last = pairs[pairs.length - 1];
      // Merge adjacent replace pairs across a SHORT intervening equal block
      // (a bare separator) when their selects differ. This prevents the LCS
      // from splitting one logical change ("the terms" -> "THE TERMS") into two
      // overlapping ops on a repeated subsequence. Identical selects are NOT
      // merged — the occurrence=rank-(k-1) chain handles those correctly.
      if (
        last !== undefined &&
        !last.extended &&
        pendingEqual.length > 0 &&
        pendingEqual.length <= 1 &&
        last.select !== select
      ) {
        last.select += pendingEqual + select;
        last.with += pendingEqual + with_;
        last.propEnd = op.bEnd;
        last.extended = false;
        pendingEqual = '';
        consumed += 1;
        continue;
      }
      pairs.push({
        select,
        with: with_,
        origStart: op.aStart,
        propStart: op.bStart,
        propEnd: op.bEnd,
        extended: false,
      });
      // The equals before this pair are consumed by it: reset so a later
      // split-pair merge or empty-side extension never double-counts them
      // (they must not inflate the merge threshold across occurrences).
      pendingEqual = '';
      consumed += 1;
      continue;
    }
    // Empty-side pair (insert or delete).
    if (pairs.length > 0) {
      const last = pairs[pairs.length - 1];
      last.select += pendingEqual + origPlain.slice(op.aStart, op.aEnd);
      last.with += pendingEqual + propPlain.slice(op.bStart, op.bEnd);
      last.propEnd = op.bEnd;
      last.extended = true;
      pendingEqual = '';
      consumed += 1;
      continue;
    }
    if (op.aStart > 0) {
      // Lone empty-side pair: extend backward into all preceding equal text
      // (the trimmed prefix plus any middle-internal equals).
      const back = origPlain.slice(0, op.aStart);
      pairs.push({
        select: back + origPlain.slice(op.aStart, op.aEnd),
        with: back + propPlain.slice(op.bStart, op.bEnd),
        origStart: 0,
        propStart: op.bStart - back.length,
        propEnd: op.bEnd,
        extended: true,
      });
      pendingEqual = '';
      consumed += 1;
      continue;
    }
    // Truly at the very start: extend forward into the following equal text
    // (middle-internal equals plus the trimmed suffix).
    let fwd = '';
    let end = consumed + 1;
    while (end < n && opcodes[end].type === 'equal') {
      fwd += origPlain.slice(opcodes[end].aStart, opcodes[end].aEnd);
      end += 1;
    }
    fwd += suffix;
    pairs.push({
      select: origPlain.slice(op.aStart, op.aEnd) + fwd,
      with: propPlain.slice(op.bStart, op.bEnd) + fwd,
      origStart: 0,
      propStart: op.bStart,
      propEnd: op.bEnd + fwd.length,
      extended: true,
    });
    consumed = end;
  }
  return pairs;
}

function countMatches(haystack: string, needle: string, before?: number): number {
  let count = 0;
  let at = haystack.indexOf(needle);
  while (at !== -1) {
    if (before !== undefined && at > before) break;
    count += 1;
    at = haystack.indexOf(needle, at + 1);
  }
  return count;
}

/**
 * Phrase-anchor a single ambiguous select by expanding it left by one word
 * (right when at the very start). Returns the expanded span in both texts.
 */
function expandSelect(
  pair: Pair,
  origPlain: string,
  origTokens: Token[],
): { select: string; origStart: number; propStart: number } {
  const s = pair.origStart;
  const e = pair.origStart + pair.select.length;
  let word: Token | null = null;
  for (const token of origTokens) {
    if (token.end <= s && WORD_TEST.test(token.text)) word = token;
    if (token.start >= e) break;
  }
  if (word !== null) {
    const delta = s - word.start;
    return {
      select: origPlain.slice(word.start, e),
      origStart: word.start,
      propStart: pair.propStart - delta,
    };
  }
  for (const token of origTokens) {
    if (token.start >= e && WORD_TEST.test(token.text)) {
      return { select: origPlain.slice(s, token.end), origStart: s, propStart: pair.propStart };
    }
  }
  return { select: pair.select, origStart: pair.origStart, propStart: pair.propStart };
}

/** Slice a paragraph's items to the plain-text span [start, end). */
function sliceSpanItems(paragraph: ProjectionParagraph, start: number, end: number): ProjectionItem[] {
  const items: ProjectionItem[] = [];
  let pos = 0;
  for (const item of paragraph.items) {
    const len = plainLen(item);
    const itemStart = pos;
    const itemEnd = pos + len;
    pos = itemEnd;
    if (itemEnd <= start || itemStart >= end) continue;
    if (itemStart < start || itemEnd > end) {
      if (item.kind !== 'text') throw new ProjectionError('internal: span crosses a protected item');
      const s = Math.max(start, itemStart) - itemStart;
      const e = Math.min(end, itemEnd) - itemStart;
      items.push({ kind: 'text', text: item.text.slice(s, e), fmt: item.fmt });
    } else {
      items.push(item);
    }
  }
  return items;
}

/**
 * Dialect-serialize the proposed span for a text op's `with`, emitting the
 * proposed items' fmt state directly (additions AND removals). A plain span
 * carries no tags so the engine authors a non-formatted run (removing bold);
 * a formatted span carries its tags explicitly (preserving or adding bold).
 * The fmt pass only skips a fmt change fully covered by a text-change region
 * because this serializer always carries the proposed state.
 */
function spanContent(propPara: ProjectionParagraph, propStart: number, propEnd: number): string {
  const items = sliceSpanItems(propPara, propStart, propEnd);
  let out = '';
  let open = blankFmt();
  for (const item of items) {
    if (item.kind === 'text') {
      out += transitionFmt(open, item.fmt);
      out += escapeText(item.text);
      open = item.fmt;
    } else {
      out += item.text;
    }
  }
  out += closeFmtTags(open);
  return out;
}

/** Content of a paragraph in dialect form (no wrapper), via its serialized markup. */
function paragraphContent(p: ProjectionParagraph): string {
  const open = p.markup.indexOf('>');
  const close = p.markup.lastIndexOf(`</${p.tag}>`);
  return open === -1 || close === -1 ? '' : p.markup.slice(open + 1, close);
}

function styleOf(p: ProjectionParagraph): string | null {
  return p.tag === 'p' ? p.styleClass : `Heading${p.tag.slice(1)}`;
}

/** Structural equality of two protected items (kind, verbatim text, attrs, fmt). */
function sameProtected(a: ProjectionItem, b: ProjectionItem): boolean {
  if (a.kind !== b.kind) return false;
  if (a.text !== b.text) return false;
  if (a.kind === 'link') {
    const bb = b as typeof a;
    return a.href === bb.href && fmtEq(a.fmt, bb.fmt);
  }
  if (a.kind === 'field') return a.instr === (b as typeof a).instr;
  if (a.kind === 'revision') {
    const bb = b as typeof a;
    return a.tag === bb.tag && a.id === bb.id && a.author === bb.author && fmtEq(a.fmt, bb.fmt);
  }
  return true;
}

/**
 * Deterministically allocate a valid insert_paragraph alias from a
 * source-hash seed, skipping already-used aliases.
 *
 * The engine's valid_alias rule ([A-Za-z_][A-Za-z0-9_]*) rejects
 * digit-leading ids; a "P" + 8 uppercase hex digits alias satisfies it, never
 * collides with 8-hex paragraph ids, and is deterministic across preview and
 * commit (same state + source hash -> same alias). Aliases are referenced by
 * later ops as "$" + alias.
 */
export function allocateInsertId(sourceSha: string, index: number, existing: Set<string>): string {
  const h = createHash('sha256').update(`${sourceSha}\0${String(index)}`).digest('hex');
  let v = 1 + (parseInt(h.slice(0, 8), 16) % 0x7fffffff);
  for (let attempt = 0; attempt < 1_000_000; attempt += 1) {
    const id = `P${v.toString(16).toUpperCase().padStart(8, '0')}`;
    if (!existing.has(id)) return id;
    v = (v % 0x7fffffff) + 1;
  }
  throw new ProjectionError('insert id allocation exhausted');
}

/**
 * Formatting pass: maximal spans where the per-character format flags differ.
 * Spans with identical text become format_text ops; spans with differing text
 * are valid only when fully covered by a text-change region (the replace_text
 * `with` carries the agent's added tags) — a fmt change over unchanged text
 * adjacent to a text change returns a rejection reason.
 */
type FmtRegion = {
  equal: boolean;
  origStart: number;
  origEnd: number;
  propStart: number;
  propEnd: number;
};

/**
 * The region structure for the aligned fmt comparison: equal regions align
 * orig/prop positions 1:1; changed regions map the original span to the
 * proposed span (region-relative alignment). Built from the LCS opcodes plus
 * the trimmed prefix/suffix.
 */
function fmtRegions(origPlain: string, propPlain: string, opcodes: Opcode[], prefixLen: number, suffixLen: number): FmtRegion[] {
  const regions: FmtRegion[] = [];
  if (prefixLen > 0) regions.push({ equal: true, origStart: 0, origEnd: prefixLen, propStart: 0, propEnd: prefixLen });
  for (const op of opcodes) {
    if (op.type === 'equal') {
      regions.push({ equal: true, origStart: op.aStart, origEnd: op.aEnd, propStart: op.bStart, propEnd: op.bEnd });
    } else {
      regions.push({ equal: false, origStart: op.aStart, origEnd: op.aEnd, propStart: op.bStart, propEnd: op.bEnd });
    }
  }
  if (suffixLen > 0) {
    const os = origPlain.length - suffixLen;
    const ps = propPlain.length - suffixLen;
    regions.push({ equal: true, origStart: os, origEnd: origPlain.length, propStart: ps, propEnd: propPlain.length });
  }
  return regions;
}

/**
 * Formatting pass over PAIR-ALIGNED regions (finding 3): equal regions align
 * 1:1; changed regions map the original span to the proposed span, so length-
 * changing text edits no longer misalign the fmt comparison. A fmt-diff span
 * entirely over unchanged (equal-region) text becomes format_text ops; a span
 * entirely inside changed regions is carried by the replace_text `with` (which
 * serializes the proposed fmt state directly — additions and removals); a span
 * overlapping both (a fmt change partially covering unchanged text) rejects.
 */
function fmtOpsFor(orig: ProjectionParagraph, prop: ProjectionParagraph, id: string, regions: FmtRegion[]): CoreOp[] | string {
  const a = fmtPerChar(orig);
  const b = fmtPerChar(prop);
  const propPlain = prop.plainText;
  const origPlain = orig.plainText;
  const kindAt = new Array<'equal' | 'changed'>(propPlain.length);
  const diffAt = new Array<boolean>(propPlain.length);
  for (const region of regions) {
    const len = region.propEnd - region.propStart;
    const aligned = Math.min(len, region.origEnd - region.origStart);
    for (let k = 0; k < len; k += 1) {
      const ppos = region.propStart + k;
      kindAt[ppos] = region.equal ? 'equal' : 'changed';
      diffAt[ppos] = k < aligned && !fmtEq(a[region.origStart + k] ?? blankFmt(), b[ppos] ?? blankFmt());
    }
  }
  const spans: Array<{ start: number; end: number }> = [];
  let s = -1;
  for (let i = 0; i < propPlain.length; i += 1) {
    const diff = diffAt[i];
    if (diff && s === -1) s = i;
    if (!diff && s !== -1) {
      spans.push({ start: s, end: i });
      s = -1;
    }
  }
  if (s !== -1) spans.push({ start: s, end: propPlain.length });

  const allEqual = (span: { start: number; end: number }): boolean => {
    let equal = true;
    for (let i = span.start; i < span.end; i += 1) if (kindAt[i] !== 'equal') equal = false;
    return equal;
  };
  const allChanged = (span: { start: number; end: number }): boolean => {
    let changed = true;
    for (let i = span.start; i < span.end; i += 1) if (kindAt[i] !== 'changed') changed = false;
    return changed;
  };

  const selectCounts = new Map<string, number>();
  for (const span of spans) {
    if (allEqual(span)) {
      const select = propPlain.slice(span.start, span.end);
      selectCounts.set(select, (selectCounts.get(select) ?? 0) + 1);
    } else if (!allChanged(span)) {
      return `formatting change overlaps a text change in paragraph ${id} — separate the edits`;
    }
    // All-changed spans are carried by the replace_text with.
  }
  const ops: CoreOp[] = [];
  for (const span of spans) {
    if (!allEqual(span)) continue;
    const select = propPlain.slice(span.start, span.end);
    const matches = countMatches(origPlain, select);
    let occurrence: number | undefined;
    // format_text does NOT change text, so the engine's occurrence counting
    // (1-based among matches in the current text) never shifts: identical-select
    // spans keep their ORIGINAL ranks. The rank-(k-1) adjustment used by
    // replace_text would make the second op re-target the first occurrence.
    if (matches > 1) occurrence = countMatches(origPlain, select, span.start);
    const flags: Record<string, unknown> = {};
    const af = a[span.start] ?? blankFmt();
    const bf = b[span.start] ?? blankFmt();
    for (const key of FMT_KEYS) {
      if (af[key] !== bf[key]) flags[key] = bf[key];
    }
    ops.push({ op: 'format_text', at: id, select, ...flags, ...(occurrence !== undefined ? { occurrence } : {}) });
  }
  return ops;
}

export function diffProjections(
  originalMarkup: string,
  proposedMarkup: string,
  opts?: { sourceSha?: string },
): DiffResult {
  let original: ReturnType<typeof parseProjection>;
  let proposed: ReturnType<typeof parseProjection>;
  try {
    original = parseProjection(originalMarkup);
  } catch (error) {
    return { ok: false, reason: `malformed projection: ${error instanceof Error ? error.message : String(error)}` };
  }
  try {
    proposed = parseProjectionLoose(proposedMarkup);
  } catch (error) {
    return { ok: false, reason: `malformed projection: ${error instanceof Error ? error.message : String(error)}` };
  }

  for (const p of original.paragraphs) {
    if (p.id === null) return { ok: false, reason: 'malformed projection: original paragraph missing an id' };
  }

  const originalById = new Map(original.paragraphs.map((p) => [p.id as string, p]));
  const proposedById = new Map<string, ProjectionParagraph>();
  const proposedIdLess: Array<{ paragraph: ProjectionParagraph; index: number }> = [];
  for (let i = 0; i < proposed.paragraphs.length; i += 1) {
    const p = proposed.paragraphs[i];
    if (p.id === null) {
      proposedIdLess.push({ paragraph: p, index: i });
      continue;
    }
    if (!originalById.has(p.id)) {
      return {
        ok: false,
        reason: `invented paragraph id "${p.id}" — ids are engine-owned; write new paragraphs without an id attribute`,
      };
    }
    proposedById.set(p.id, p);
  }

  // ---- structure: deletions (original order) ----
  const deletedIds: string[] = [];
  for (const p of original.paragraphs) {
    if (!proposedById.has(p.id as string)) deletedIds.push(p.id as string);
  }

  // ---- structure: insertions ----
  const insertOps: Array<{ op: CoreOp; index: number }> = [];
  {
    let chainId: string | null = null;
    let chainCounter = 0;
    const allocatedAliases = new Set<string>();
    for (const { paragraph, index } of proposedIdLess) {
      let at: string;
      let position: 'before' | 'after';
      if (chainId === null) {
        let anchor: string | null = null;
        for (let i = index - 1; i >= 0; i -= 1) {
          const id = proposed.paragraphs[i].id;
          if (id !== null) {
            anchor = id;
            break;
          }
        }
        if (anchor !== null) {
          at = anchor;
          position = 'after';
        } else {
          at = original.paragraphs[0].id as string;
          position = 'before';
        }
      } else {
        // Chain: anchor at the previous insert's alias (engine address syntax
        // is "$" + alias), always after it so document order is preserved.
        at = `$${chainId}`;
        position = 'after';
      }
      const op: CoreOp = { op: 'insert_paragraph', at, position };
      const content = paragraphContent(paragraph);
      if (content !== '') op.with = content;
      const style = styleOf(paragraph);
      if (style !== null) op.style = style;
      const next = proposed.paragraphs[index + 1];
      if (next !== undefined && next.id === null) {
        if (opts?.sourceSha === undefined) {
          return { ok: false, reason: 'two or more consecutive new paragraphs are ambiguous without a source hash' };
        }
        const allocated = allocateInsertId(opts.sourceSha, chainCounter, allocatedAliases);
        op.as = allocated;
        allocatedAliases.add(allocated);
        chainId = allocated;
        chainCounter += 1;
      } else {
        chainId = null;
      }
      insertOps.push({ op, index });
    }
  }

  // ---- per-paragraph passes ----
  const paragraphOps = new Map<string, CoreOp[]>();
  for (const p of original.paragraphs) {
    const id = p.id as string;
    const q = proposedById.get(id);
    if (q === undefined) continue; // deleted

    // Protected attributes (ord/num/break-ins/break-del) must not change.
    for (const key of PARA_ATTRS) {
      if ((q.attrs[key] ?? undefined) !== (p.attrs[key] ?? undefined)) {
        return { ok: false, reason: `projection-protected attribute "${key}" changed in paragraph ${id}` };
      }
    }

    // Protected content: the ordered sequence of protected items must be equal.
    const aProtected = p.items.filter((item) => PROTECTED_KINDS.has(item.kind));
    const bProtected = q.items.filter((item) => PROTECTED_KINDS.has(item.kind));
    if (aProtected.length !== bProtected.length) {
      const kind = (bProtected[0] ?? aProtected[0])?.kind;
      return {
        ok: false,
        reason: `protected content changed in paragraph ${id} (${kind}) — not expressible through the projection`,
      };
    }
    for (let i = 0; i < aProtected.length; i += 1) {
      if (!sameProtected(aProtected[i], bProtected[i])) {
        return {
          ok: false,
          reason: `protected content changed in paragraph ${id} (${aProtected[i].kind}) — not expressible through the projection`,
        };
      }
    }

    const origPlain = p.plainText;
    const propPlain = q.plainText;
    const hasRevision = aProtected.some((item) => item.kind === 'revision');
    const textDiffers = origPlain !== propPlain;
    const fmtDiffers = !fmtEqFmtArrays(fmtPerChar(p), fmtPerChar(q));
    if (hasRevision && (textDiffers || fmtDiffers)) {
      return {
        ok: false,
        reason: `text change in paragraph ${id} crosses revision content — revision boundaries are protected`,
      };
    }

    const localOps: CoreOp[] = [];

    if (textDiffers) {
      const { prefixLen, suffixLen } = alignedTrim(origPlain, propPlain);
      const midOrig = origPlain.slice(prefixLen, origPlain.length - suffixLen);
      const midProp = propPlain.slice(prefixLen, propPlain.length - suffixLen);
      const opcodes = lcsTokens(tokenize(midOrig), tokenize(midProp)).map((op) => ({
        type: op.type,
        aStart: op.aStart + prefixLen,
        aEnd: op.aEnd + prefixLen,
        bStart: op.bStart + prefixLen,
        bEnd: op.bEnd + prefixLen,
      }));
      let equalChars = prefixLen + suffixLen;
      const nonEqual: Opcode[] = [];
      for (const op of opcodes) {
        if (op.type === 'equal') {
          equalChars += op.aEnd - op.aStart;
        } else {
          nonEqual.push(op);
        }
      }
      const ratio = equalChars / Math.max(origPlain.length, propPlain.length);
      const clean =
        nonEqual.length === 0 ||
        (nonEqual.length === 1 && nonEqual[0].type === 'insert') ||
        (nonEqual.every((op) => op.type === 'replace') &&
          nonEqual.every((op) => origPlain.slice(op.aStart, op.aEnd) === origPlain.slice(nonEqual[0].aStart, nonEqual[0].aEnd)));

      if (!clean && ratio < 0.5) {
        localOps.push({ op: 'replace_paragraph', at: id, with: paragraphContent(q) });
      } else {
        const pairs = buildPairs(opcodes, origPlain, propPlain, suffixLen);
        const usable = pairs.filter((pair) => pair.select !== '');
        if (usable.length === 0) {
          localOps.push({ op: 'replace_paragraph', at: id, with: paragraphContent(q) });
        } else {
          let pairsToUse = usable;
          for (const pair of pairsToUse) {
            if (pair.extended && countMatches(origPlain, pair.select) > 1) {
              pairsToUse = [];
              localOps.push({ op: 'replace_paragraph', at: id, with: paragraphContent(q) });
              break;
            }
          }
          if (pairsToUse.length > 0) {
            // Crossing guard: final spans must not overlap protected item text.
            const protectedRanges: Array<{ start: number; end: number }> = [];
            {
              let pos = 0;
              for (const item of p.items) {
                const len = plainLen(item);
                if (PROTECTED_KINDS.has(item.kind)) protectedRanges.push({ start: pos, end: pos + len });
                pos += len;
              }
            }
            for (const pair of pairsToUse) {
              const s = pair.origStart;
              const e = pair.origStart + pair.select.length;
              for (const range of protectedRanges) {
                if (s < range.end && e > range.start) {
                  return { ok: false, reason: `change crosses protected content in paragraph ${id}` };
                }
              }
            }
            // Occurrence + ambiguity phrase-anchoring. For multiple pairs with
            // the same select, emit the pair's original rank minus (k-1) — they
            // consume sequentially, so the k-th identical-select span in
            // document order targets rank - (k-1) in the evolving text.
            const selectCounts = new Map<string, number>();
            for (const pair of pairsToUse) selectCounts.set(pair.select, (selectCounts.get(pair.select) ?? 0) + 1);
            const selectSeen = new Map<string, number>();
            const textOps: CoreOp[] = [];
            const origTokens = tokenize(origPlain);
            const spanCrossesProtected = (start: number, end: number): boolean =>
              protectedRanges.some((range) => start < range.end && end > range.start);
            for (const pair of pairsToUse) {
              let select = pair.select;
              let origStart = pair.origStart;
              let propStart = pair.propStart;
              let occurrence: number | undefined;
              if ((selectCounts.get(select) ?? 0) > 1) {
                const k = (selectSeen.get(select) ?? 0) + 1;
                selectSeen.set(select, k);
                const rank = countMatches(origPlain, select, pair.origStart);
                occurrence = rank - (k - 1);
              } else {
                const matches = countMatches(origPlain, select);
                // Phrase-anchor only SINGLE-WORD ambiguous selects: a select
                // that already spans words is a complete phrase ("the terms"
                // stays, never over-expanding to "and the terms"). Multi-word
                // selects fall through to occurrence = original rank.
                if (matches > 1 && WORD_TEST.test(select)) {
                  const expanded = expandSelect(pair, origPlain, origTokens);
                  select = expanded.select;
                  origStart = expanded.origStart;
                  propStart = expanded.propStart;
                  const expandedMatches = countMatches(origPlain, select);
                  if (expandedMatches > 1) occurrence = countMatches(origPlain, select, origStart);
                } else if (matches > 1) {
                  occurrence = countMatches(origPlain, select, origStart);
                }
              }
              // Phrase expansion must not reach into protected content.
              if (spanCrossesProtected(origStart, origStart + select.length)) {
                return { ok: false, reason: `change crosses protected content in paragraph ${id}` };
              }
              textOps.push({
                op: 'replace_text',
                at: id,
                select,
                with: spanContent(q, propStart, pair.propEnd),
                ...(occurrence !== undefined ? { occurrence } : {}),
              });
            }
            const regions = fmtRegions(origPlain, propPlain, opcodes, prefixLen, suffixLen);
            const fmtResult = fmtOpsFor(p, q, id, regions);
            if (typeof fmtResult === 'string') return { ok: false, reason: fmtResult };
            // Containment guard (cross-op only): a `with` must not re-introduce
            // the select of a different op in the same paragraph.
            const allSelects = [...textOps, ...fmtResult].map((op) => op.select as string);
            for (let i = 0; i < textOps.length; i += 1) {
              const withText = textOps[i].with as string;
              for (let j = 0; j < allSelects.length; j += 1) {
                if (j < textOps.length && j === i) continue; // self-containment is allowed
                if (withText.includes(allSelects[j])) {
                  return {
                    ok: false,
                    reason: `replacement text re-introduces a selected phrase in paragraph ${id} — ambiguous`,
                  };
                }
              }
            }
            localOps.push(...textOps, ...fmtResult);
          }
        }
      }
    } else {
      const regions: FmtRegion[] = [
        { equal: true, origStart: 0, origEnd: origPlain.length, propStart: 0, propEnd: propPlain.length },
      ];
      const fmtResult = fmtOpsFor(p, q, id, regions);
      if (typeof fmtResult === 'string') return { ok: false, reason: fmtResult };
      localOps.push(...fmtResult);
    }

    // ---- style pass ----
    const origStyle = styleOf(p);
    const propStyle = styleOf(q);
    if (origStyle !== propStyle) {
      if (propStyle === null) {
        return { ok: false, reason: 'cannot clear a paragraph style through the projection' };
      }
      localOps.push({ op: 'format_paragraph', at: id, style: propStyle });
    }

    paragraphOps.set(id, localOps);
  }

  // ---- document-order assembly ----
  const events: Array<{ at: number; op: CoreOp }> = [];
  for (const { op, index } of insertOps) events.push({ at: index, op });
  if (deletedIds.length > 0) {
    const firstDeletedIndex = original.paragraphs.findIndex((p) => deletedIds.includes(p.id as string));
    let deleteAt = proposed.paragraphs.length;
    for (let i = 0; i < proposed.paragraphs.length; i += 1) {
      const id = proposed.paragraphs[i].id;
      if (id === null) continue;
      const origIndex = original.paragraphs.findIndex((p) => p.id === id);
      if (origIndex > firstDeletedIndex) {
        deleteAt = i;
        break;
      }
    }
    events.push({ at: deleteAt, op: { op: 'delete_paragraphs', at: deletedIds } });
  }
  for (let i = 0; i < proposed.paragraphs.length; i += 1) {
    const id = proposed.paragraphs[i].id;
    if (id === null) continue;
    const localOps = paragraphOps.get(id);
    if (localOps !== undefined) {
      for (const op of localOps) events.push({ at: i, op });
    }
  }
  events.sort((a, b) => a.at - b.at);
  return { ok: true, ops: events.map((event) => event.op) };
}

function fmtEqFmtArrays(a: FormatFlags[], b: FormatFlags[]): boolean {
  const len = Math.max(a.length, b.length);
  for (let i = 0; i < len; i += 1) {
    if (!fmtEq(a[i] ?? blankFmt(), b[i] ?? blankFmt())) return false;
  }
  return true;
}
