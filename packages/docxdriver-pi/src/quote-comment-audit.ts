import { createHash } from 'node:crypto';

export const QUOTE_AUDIT_AUTHOR = 'docxdriver quotation audit';
export const QUOTE_AUDIT_INITIALS = 'DQA';
export const QUOTE_WARNING_PREFIX = 'qw1:sha256:';

export interface UnverifiedQuoteSpan {
  paragraph_id: string;
  select: string;
  occurrence: number;
}

export interface QuoteWarningSpec extends UnverifiedQuoteSpan {
  warning_id: string;
  author: typeof QUOTE_AUDIT_AUTHOR;
  initials: typeof QUOTE_AUDIT_INITIALS;
  status: 'open';
  text: string;
}

export interface ExistingQuoteWarning extends QuoteWarningSpec {
  comment_id: string;
}

export interface QuoteWarningCoverage {
  ok: boolean;
  missing: QuoteWarningSpec[];
  tampered: ExistingQuoteWarning[];
  duplicates: ExistingQuoteWarning[];
  stale: ExistingQuoteWarning[];
}

function sha256(text: string): string {
  return createHash('sha256').update(text, 'utf8').digest('hex');
}

function validateSpan(span: UnverifiedQuoteSpan): void {
  if (!span || typeof span.paragraph_id !== 'string' || span.paragraph_id.length === 0) throw new Error('warning paragraph_id must be non-empty');
  if (typeof span.select !== 'string' || span.select.length === 0) throw new Error('warning select must be non-empty');
  if (!Number.isInteger(span.occurrence) || span.occurrence < 1) throw new Error('warning occurrence must be a positive integer');
}

/** Stable identity for one exact unverified quotation anchor. */
export function quoteWarningId(span: UnverifiedQuoteSpan): string {
  validateSpan(span);
  return `${QUOTE_WARNING_PREFIX}${sha256(JSON.stringify([span.paragraph_id, span.select, span.occurrence]))}`;
}

/** Host-owned comment payload. The marker is auditable, not an authorization secret. */
export function buildQuoteWarning(span: UnverifiedQuoteSpan): QuoteWarningSpec {
  const warning_id = quoteWarningId(span);
  return {
    ...span,
    warning_id,
    author: QUOTE_AUDIT_AUTHOR,
    initials: QUOTE_AUDIT_INITIALS,
    status: 'open',
    text: `UNVERIFIED QUOTATION — source provenance could not be confirmed. Review before relying on this text. [${warning_id}]`,
  };
}

function exactWarning(comment: ExistingQuoteWarning, expected: QuoteWarningSpec): boolean {
  return (
    comment.warning_id === expected.warning_id &&
    comment.paragraph_id === expected.paragraph_id &&
    comment.select === expected.select &&
    comment.occurrence === expected.occurrence &&
    comment.author === expected.author &&
    comment.initials === expected.initials &&
    comment.status === 'open' &&
    comment.text === expected.text
  );
}

/**
 * Final-candidate invariant: every unverified span has exactly one pristine,
 * open audit bubble, and no protected warning remains without a span.
 */
export function auditQuoteWarningCoverage(spans: UnverifiedQuoteSpan[], comments: ExistingQuoteWarning[]): QuoteWarningCoverage {
  const expected = spans.map(buildQuoteWarning);
  const expectedById = new Map(expected.map((warning) => [warning.warning_id, warning]));
  const commentsById = new Map<string, ExistingQuoteWarning[]>();
  for (const comment of comments) {
    const list = commentsById.get(comment.warning_id) ?? [];
    list.push(comment);
    commentsById.set(comment.warning_id, list);
  }
  const missing: QuoteWarningSpec[] = [];
  const tampered: ExistingQuoteWarning[] = [];
  const duplicates: ExistingQuoteWarning[] = [];
  for (const warning of expected) {
    const candidates = commentsById.get(warning.warning_id) ?? [];
    const exact = candidates.filter((comment) => exactWarning(comment, warning));
    if (exact.length === 0) missing.push(warning);
    for (const comment of candidates) if (!exactWarning(comment, warning)) tampered.push(comment);
    if (exact.length > 1) duplicates.push(...exact.slice(1));
  }
  const stale = comments.filter((comment) => !expectedById.has(comment.warning_id));
  return {
    ok: missing.length === 0 && tampered.length === 0 && duplicates.length === 0 && stale.length === 0,
    missing,
    tampered,
    duplicates,
    stale,
  };
}

/** Protected audit comments cannot be changed by model-authored comment ops. */
export function protectedCommentMutationIds(operations: unknown[], protectedCommentIds: Set<string>): string[] {
  const rejected = new Set<string>();
  for (const value of operations) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) continue;
    const operation = value as Record<string, unknown>;
    if (!['comment_delete', 'comment_reply', 'comment_set_status'].includes(String(operation.op))) continue;
    const id = String(operation.comment_id ?? '');
    if (protectedCommentIds.has(id)) rejected.add(id);
  }
  return [...rejected];
}
