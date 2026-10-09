import { createHash } from 'node:crypto';

export interface QuoteChange {
  kind: 'omit' | 'bracket';
  select: string;
  occurrence?: number;
  replacement?: string;
  marker?: '...' | '…';
}

export interface QuoteSpec {
  source: string;
  at: string;
  select: string;
  occurrence?: number;
  style?: 'double' | 'single' | 'none';
  changes?: QuoteChange[];
}

export interface ResolvedQuoteChange {
  kind: 'omit' | 'bracket';
  start: number;
  end: number;
  original: string;
  rendered: string;
}

export interface ResolvedQuote {
  quote_id: string;
  text: string;
  content: string;
  text_sha256: string;
  source: string;
  source_sha256: string;
  /** Host admission classification; absent in the pure resolver. */
  provenance_tier?: 'unregistered_local';
  at: string;
  paragraph_sha256: string;
  span: { start: number; end: number; occurrence: number };
  original: string;
  changes: ResolvedQuoteChange[];
}

export class QuoteValidationError extends Error {
  readonly code = 'quote_validation_failed';
  constructor(message: string) {
    super(message);
    this.name = 'QuoteValidationError';
  }
}

function sha256(text: string): string {
  return createHash('sha256').update(text, 'utf8').digest('hex');
}

function positiveOccurrence(value: unknown, label: string): number {
  const occurrence = value === undefined ? 1 : value;
  if (!Number.isInteger(occurrence) || Number(occurrence) < 1) {
    throw new QuoteValidationError(`${label} occurrence must be a positive integer`);
  }
  return Number(occurrence);
}

function occurrenceStart(haystack: string, needle: string, occurrence: number, label: string): number {
  if (needle.length === 0) throw new QuoteValidationError(`${label} select must not be empty`);
  let from = 0;
  for (let found = 1; found <= occurrence; found += 1) {
    const start = haystack.indexOf(needle, from);
    if (start === -1) throw new QuoteValidationError(`${label} occurrence ${occurrence} was not found exactly`);
    if (found === occurrence) return start;
    from = start + needle.length;
  }
  throw new QuoteValidationError(`${label} occurrence ${occurrence} was not found exactly`);
}

/**
 * Resolve a quote exclusively from host-supplied source text. Every authored
 * selector is checked with exact, case-sensitive matching. Adaptations address
 * the original selected span (never an already-mutated intermediate string),
 * so their coordinates and provenance are stable and order-independent.
 */
export function resolveQuote(spec: QuoteSpec, paragraphText: string, sourceSha256: string): ResolvedQuote {
  if (!spec || typeof spec !== 'object') throw new QuoteValidationError('quote must be an object');
  for (const [label, value] of [['source', spec.source], ['at', spec.at], ['select', spec.select]] as const) {
    if (typeof value !== 'string' || value.length === 0) throw new QuoteValidationError(`quote ${label} must be a non-empty string`);
  }
  const quoteOccurrence = positiveOccurrence(spec.occurrence, 'quote');
  const quoteStart = occurrenceStart(paragraphText, spec.select, quoteOccurrence, 'quote span');
  const changes: ResolvedQuoteChange[] = [];

  for (const [index, change] of (spec.changes ?? []).entries()) {
    const label = `change ${index + 1}`;
    if (!change || (change.kind !== 'omit' && change.kind !== 'bracket')) {
      throw new QuoteValidationError(`${label} kind must be omit or bracket`);
    }
    if (typeof change.select !== 'string' || change.select.length === 0) {
      throw new QuoteValidationError(`${label} select must be a non-empty string`);
    }
    const occurrence = positiveOccurrence(change.occurrence, label);
    const start = occurrenceStart(spec.select, change.select, occurrence, label);
    const end = start + change.select.length;
    let rendered: string;
    if (change.kind === 'omit') {
      const marker = change.marker ?? '…';
      if (marker !== '…' && marker !== '...') throw new QuoteValidationError(`${label} marker must be ... or …`);
      rendered = marker;
    } else {
      if (typeof change.replacement !== 'string' || change.replacement.length === 0) {
        throw new QuoteValidationError(`${label} bracket replacement must be a non-empty string`);
      }
      if (change.replacement.includes('[') || change.replacement.includes(']')) {
        throw new QuoteValidationError(`${label} replacement must not contain brackets`);
      }
      rendered = `[${change.replacement}]`;
    }
    changes.push({ kind: change.kind, start, end, original: change.select, rendered });
  }

  changes.sort((a, b) => a.start - b.start || a.end - b.end);
  for (let index = 1; index < changes.length; index += 1) {
    if (changes[index].start < changes[index - 1].end) {
      throw new QuoteValidationError(`changes overlap at offsets ${changes[index - 1].start}-${changes[index].end}`);
    }
  }

  let cursor = 0;
  let content = '';
  for (const change of changes) {
    content += spec.select.slice(cursor, change.start);
    content += change.rendered;
    cursor = change.end;
  }
  content += spec.select.slice(cursor);
  const style = spec.style ?? 'double';
  if (style !== 'double' && style !== 'single' && style !== 'none') {
    throw new QuoteValidationError('quote style must be double, single, or none');
  }
  const text = style === 'double' ? `“${content}”` : style === 'single' ? `‘${content}’` : content;

  const record = {
    text,
    content,
    style,
    source: spec.source,
    source_sha256: sourceSha256,
    at: spec.at,
    paragraph_sha256: sha256(paragraphText),
    span: { start: quoteStart, end: quoteStart + spec.select.length, occurrence: quoteOccurrence },
    original: spec.select,
    changes,
  };
  return {
    quote_id: `q1:sha256:${sha256(JSON.stringify(record))}`,
    text_sha256: sha256(text),
    ...record,
  };
}
