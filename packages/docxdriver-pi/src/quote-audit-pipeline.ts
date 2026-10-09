import { planReportComplete, runPlan, runRead } from './engine.js';
import { sha256 } from './io.js';
import { buildQuoteWarning, protectedCommentMutationIds, QUOTE_AUDIT_AUTHOR, QUOTE_WARNING_PREFIX, type UnverifiedQuoteSpan } from './quote-comment-audit.js';
import type { TrustedQuoteRendering } from './structured-quote-plan.js';

export const QUOTE_AUDIT_FEATURE_ENV = 'DOCXDRIVER_QUOTE_AUDIT_COMMENTS';
export const QUOTE_PROVENANCE_POLICY_ENV = 'DOCXDRIVER_QUOTE_PROVENANCE_POLICY';

export type QuoteProvenancePolicy = 'permissive' | 'controlled' | 'authoritative';

export interface QuoteAuditFeatureOptions {
  enabled: boolean;
  /** Defaults to permissive. The stricter policies are reserved fail-closed stubs. */
  provenancePolicy?: QuoteProvenancePolicy;
}

export interface AppliedQuoteAudit {
  bytes: Uint8Array;
  warnings: number;
  protectedCommentIds: string[];
}

export interface QuoteAuditApplyOptions {
  trustedRenderings?: TrustedQuoteRendering[];
}

export function quoteAuditFeatureFromEnv(env: NodeJS.ProcessEnv = process.env): QuoteAuditFeatureOptions {
  const value = String(env[QUOTE_AUDIT_FEATURE_ENV] ?? '').trim().toLowerCase();
  const enabled = value === '1' || value === 'true' || value === 'yes' || value === 'on';
  if (!enabled) return { enabled: false, provenancePolicy: 'permissive' };
  const provenancePolicy = String(env[QUOTE_PROVENANCE_POLICY_ENV] ?? 'permissive').trim().toLowerCase();
  if (provenancePolicy !== 'permissive' && provenancePolicy !== 'controlled' && provenancePolicy !== 'authoritative') {
    throw new Error(`${QUOTE_PROVENANCE_POLICY_ENV} must be permissive, controlled, or authoritative`);
  }
  return { enabled: true, provenancePolicy };
}

export function unavailableQuoteProvenancePolicy(options: QuoteAuditFeatureOptions): string | undefined {
  const policy = options.provenancePolicy ?? 'permissive';
  if (!options.enabled || policy === 'permissive') return undefined;
  return `quotation provenance policy ${policy} is reserved but not implemented; use permissive or disable ${QUOTE_AUDIT_FEATURE_ENV}`;
}

type ParagraphRecord = { index: number; id: string; text: string };
type CommentThread = {
  id?: unknown;
  status?: unknown;
  anchor?: { ranges?: Array<{ locator?: unknown }> };
  root?: { author?: unknown; text?: unknown };
};

function inspect(bytes: Uint8Array): { paragraphs: ParagraphRecord[]; comments: CommentThread[]; markup: string } {
  const document = runRead(bytes, { kind: 'document_ui', view: 'final' });
  if (document.outcome !== 'completed' || !document.result || typeof document.result.html !== 'string') {
    throw new Error(document.diagnostic?.message ?? 'quote audit document read failed');
  }
  const commentsRead = runRead(bytes, { kind: 'comments' });
  if (commentsRead.outcome !== 'completed' || !commentsRead.result) throw new Error(commentsRead.diagnostic?.message ?? 'quote audit comments read failed');
  const paragraphs: ParagraphRecord[] = (document.result.paragraphs ?? []).map((p: ParagraphRecord) => ({ index: p.index, id: p.id, text: p.text }));
  const comments = Array.isArray(commentsRead.result.comments) ? (commentsRead.result.comments as CommentThread[]) : [];
  return { paragraphs, comments, markup: document.result.html };
}

function unaddressableText(markup: string): string {
  const withoutComments = markup.replace(/<comments\b[^>]*>[\s\S]*?<\/comments>/gi, '');
  const withoutAddressableParagraphs = withoutComments.replace(/<(p|h[1-6])\b([^>]*)\bid="[0-9A-Fa-f]{8}"[^>]*>[\s\S]*?<\/\1>/gi, '');
  return decodeText(withoutAddressableParagraphs).trim();
}

function assertNoUnanchorableQuotes(text: string): void {
  if (text !== '' && unverifiedQuoteSpans({ index: 0, id: 'UNADDRESSABLE', text }).length > 0) {
    throw new Error('quotation audit found quotation marks in content that cannot carry a comment anchor');
  }
}

function decodeText(markup: string): string {
  return markup
    .replace(/<br\s*\/?>/gi, '\n')
    .replace(/<[^>]+>/g, '')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&#39;|&apos;/g, "'")
    .replace(/&#(\d+);/g, (_all, digits: string) => String.fromCodePoint(Number(digits)))
    .replace(/&#x([0-9a-f]+);/gi, (_all, digits: string) => String.fromCodePoint(Number.parseInt(digits, 16)))
    .replace(/&amp;/g, '&');
}

function scalarOffset(text: string, utf16Offset: number): number {
  return [...text.slice(0, utf16Offset)].length;
}

function occurrenceAt(text: string, select: string, start: number): number {
  let occurrence = 0;
  let from = 0;
  while (from <= start) {
    const found = text.indexOf(select, from);
    if (found === -1 || found > start) break;
    occurrence += 1;
    if (found === start) return occurrence;
    from = found + select.length;
  }
  return 1;
}

/** Conservative paired quotation spans plus unmatched double/curly marks. */
export function unverifiedQuoteSpans(paragraph: ParagraphRecord): Array<UnverifiedQuoteSpan & { locator: string }> {
  const text = paragraph.text;
  const ranges: Array<{ start: number; end: number }> = [];
  const patterns = [/“[^”\n]+”/gu, /"[^"\n]+"/gu, /«[^»\n]+»/gu, /‘[^’\n]+’/gu, /(^|[\s([])'[^'\n]+'(?=$|[\s.,;:!?\)\]])/gu];
  for (const pattern of patterns) {
    for (const match of text.matchAll(pattern)) {
      let start = match.index ?? 0;
      let selected = match[0];
      if (pattern === patterns[4] && match[1]) {
        start += match[1].length;
        selected = selected.slice(match[1].length);
      }
      const end = start + selected.length;
      if (!ranges.some((range) => start < range.end && end > range.start)) ranges.push({ start, end });
    }
  }
  const covered = (index: number) => ranges.some((range) => index >= range.start && index < range.end);
  for (let index = 0; index < text.length; index += 1) {
    if ('"“”«»‘’'.includes(text[index]) && !covered(index)) ranges.push({ start: index, end: index + 1 });
  }
  return ranges
    .sort((a, b) => a.start - b.start)
    .map(({ start, end }) => {
      const select = text.slice(start, end);
      const scalarStart = scalarOffset(text, start);
      const scalarEnd = scalarOffset(text, end);
      return {
        paragraph_id: paragraph.id,
        select,
        occurrence: occurrenceAt(text, select, start),
        locator: `p${paragraph.index}:${scalarStart}-${scalarEnd}`,
      };
    });
}

function protectedThreads(comments: CommentThread[]): CommentThread[] {
  return comments.filter((thread) => {
    const author = thread.root?.author;
    const text = thread.root?.text;
    return author === QUOTE_AUDIT_AUTHOR || (typeof text === 'string' && text.includes(`[${QUOTE_WARNING_PREFIX}`));
  });
}

export function protectedQuoteCommentIds(bytes: Uint8Array): string[] {
  return protectedThreads(inspect(bytes).comments)
    .map((thread) => String(thread.id ?? ''))
    .filter((id) => /^\d+$/.test(id));
}

function occurrences(text: string, select: string): number[] {
  const found: number[] = [];
  let from = 0;
  while (from <= text.length - select.length) {
    const at = text.indexOf(select, from);
    if (at === -1) break;
    found.push(at);
    from = at + select.length;
  }
  return found;
}

/** Match reviewed structured renderings to the committed candidate exactly. */
function quotationSpans(
  paragraphs: ParagraphRecord[],
  markup: string,
  trustedRenderings: TrustedQuoteRendering[],
): Array<UnverifiedQuoteSpan & { locator: string }> {
  const expectedByText = new Map<string, number>();
  for (const rendering of trustedRenderings) {
    if (unverifiedQuoteSpans({ index: 0, id: 'TRUSTED', text: rendering.text }).length === 0) continue;
    expectedByText.set(rendering.text, (expectedByText.get(rendering.text) ?? 0) + 1);
  }
  const trustedKeys = new Set<string>();
  let chrome = unaddressableText(markup);
  for (const [text, expected] of expectedByText) {
    const paragraphMatches = paragraphs.flatMap((paragraph) =>
      occurrences(paragraph.text, text).map((_start, index) => ({ paragraph, occurrence: index + 1 })),
    );
    const chromeMatches = occurrences(chrome, text);
    if (paragraphMatches.length + chromeMatches.length !== expected) {
      throw new Error(`structured quotation rendering is missing or ambiguous in the committed candidate: ${text}`);
    }
    for (const match of paragraphMatches) {
      trustedKeys.add(JSON.stringify([match.paragraph.id, text, match.occurrence]));
    }
    for (const start of [...chromeMatches].reverse()) {
      chrome = `${chrome.slice(0, start)}${' '.repeat(text.length)}${chrome.slice(start + text.length)}`;
    }
  }
  assertNoUnanchorableQuotes(chrome);
  return paragraphs
    .flatMap(unverifiedQuoteSpans)
    .filter((span) => !trustedKeys.has(JSON.stringify([span.paragraph_id, span.select, span.occurrence])));
}

export function rejectedProtectedQuoteCommentMutations(bytes: Uint8Array, operations: unknown[]): string[] {
  return protectedCommentMutationIds(operations, new Set(protectedQuoteCommentIds(bytes)));
}

/**
 * Deterministic second pass over an in-memory committed candidate. Existing
 * reserved audit comments are removed and recreated, then the final package
 * is parsed to prove exact open-comment coverage before it can be written.
 */
export function applyQuoteAuditComments(candidate: Uint8Array, options?: QuoteAuditApplyOptions): AppliedQuoteAudit {
  const before = inspect(candidate);
  const existing = protectedThreads(before.comments);
  const spans = quotationSpans(before.paragraphs, before.markup, options?.trustedRenderings ?? []);
  const operations: Array<Record<string, unknown>> = [];
  for (const thread of existing) {
    const id = String(thread.id ?? '');
    if (/^\d+$/.test(id)) operations.push({ op: 'comment_delete', comment_id: id });
  }
  for (const span of spans) {
    const warning = buildQuoteWarning(span);
    operations.push({ op: 'comment_add', at: span.paragraph_id, select: span.select, occurrence: span.occurrence, text: warning.text });
  }
  let bytes = candidate;
  if (operations.length > 0) {
    const plan = { base: `sha256:${sha256(candidate)}`, author: QUOTE_AUDIT_AUTHOR, change_mode: 'direct', ops: operations };
    const preview = runPlan(candidate, plan);
    if (preview.outcome !== 'previewed' || typeof preview.preview_key !== 'string' || !planReportComplete(preview.report, operations.length)) {
      throw new Error(`quote audit comment preview failed: ${preview.diagnostic?.message ?? 'incomplete audit plan'}`);
    }
    const committed = runPlan(candidate, plan, preview.preview_key);
    if (committed.outcome !== 'committed' || !committed.bytes || !planReportComplete(committed.report, operations.length)) {
      throw new Error(`quote audit comment commit failed: ${committed.diagnostic?.message ?? 'incomplete audit plan'}`);
    }
    bytes = committed.bytes;
  }

  const after = inspect(bytes);
  const finalProtected = protectedThreads(after.comments);
  const byWarning = new Map<string, CommentThread[]>();
  for (const thread of finalProtected) {
    const text = String(thread.root?.text ?? '');
    const match = text.match(/\[(qw1:sha256:[0-9a-f]{64})\]/);
    if (!match) throw new Error('quote audit candidate contains malformed protected comment');
    const list = byWarning.get(match[1]) ?? [];
    list.push(thread);
    byWarning.set(match[1], list);
  }
  for (const span of spans) {
    const expected = buildQuoteWarning(span);
    const matches = (byWarning.get(expected.warning_id) ?? []).filter((thread) => {
      const ranges = thread.anchor?.ranges ?? [];
      return thread.status === 'open' && thread.root?.author === expected.author && thread.root?.text === expected.text && ranges.some((range) => range.locator === span.locator);
    });
    if (matches.length !== 1) {
      const observed = (byWarning.get(expected.warning_id) ?? []).flatMap((thread) => thread.anchor?.ranges?.map((range) => String(range.locator ?? '')) ?? []);
      throw new Error(`quote audit coverage failed for ${expected.warning_id}: expected ${span.locator}, observed ${observed.join(', ') || 'none'}`);
    }
  }
  if (finalProtected.length !== spans.length) throw new Error('quote audit candidate contains missing, duplicate, or stale protected comments');
  return {
    bytes,
    warnings: spans.length,
    protectedCommentIds: finalProtected.map((thread) => String(thread.id ?? '')).filter((id) => /^\d+$/.test(id)),
  };
}
