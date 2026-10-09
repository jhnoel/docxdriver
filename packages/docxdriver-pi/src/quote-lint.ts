export type QuoteLintSeverity = 'warning' | 'error';
export type InlineQuotePolicy = 'error' | 'comment';

export interface QuoteLintDiagnostic {
  code: 'unverified_quote_mark' | 'unterminated_inline_tag' | 'duplicate_punctuation_after_quote';
  severity: QuoteLintSeverity;
  message: string;
  start: number;
  end: number;
  excerpt: string;
}

export interface TermSpec {
  text: string;
  style?: 'double' | 'single';
}

export interface InlineAudit {
  status: 'verified' | 'warnings';
  policy: InlineQuotePolicy;
  diagnostics: Array<QuoteLintDiagnostic & { part: number }>;
  verified_quotes: number;
  terms: number;
  quote_ids: string[];
}

export interface CompiledInline {
  text: string;
  audit: InlineAudit;
}

const DOUBLE_MARKS = new Set(['"', '“', '”', '„', '‟', '«', '»']);
const OPEN_SINGLE = new Set(['‘', '‹']);
const CLOSE_SINGLE = new Set(['’', '›']);
const QUOTE_ENTITIES = ['&quot;', '&#34;', '&#x22;', '&ldquo;', '&rdquo;', '&lsquo;', '&rsquo;'];

function excerptAt(text: string, start: number, end: number): string {
  return text.slice(Math.max(0, start - 24), Math.min(text.length, end + 24));
}

function diagnostic(text: string, start: number, end: number, severity: QuoteLintSeverity): QuoteLintDiagnostic {
  return {
    code: 'unverified_quote_mark',
    severity,
    message: 'quotation mark appears in unstructured text; use a verified Quote or a structured Term',
    start,
    end,
    excerpt: excerptAt(text, start, end),
  };
}

/**
 * Lint only text-node content. Quote-like characters inside supported inline
 * tag syntax (for example href attributes) are ignored; malformed tags fail
 * closed. Apostrophes are not quotation marks, while a balanced straight
 * single-quoted phrase is detected conservatively.
 */
export function lintUnverifiedQuotes(text: string, severity: QuoteLintSeverity = 'error'): QuoteLintDiagnostic[] {
  const diagnostics: QuoteLintDiagnostic[] = [];
  let inTag = false;
  let tagStart = -1;
  let unicodeSingleOpen = -1;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    if (inTag) {
      if (char === '>') inTag = false;
      continue;
    }
    if (char === '<') {
      inTag = true;
      tagStart = index;
      continue;
    }
    const entity = QUOTE_ENTITIES.find((candidate) => text.startsWith(candidate, index));
    if (entity) {
      diagnostics.push(diagnostic(text, index, index + entity.length, severity));
      index += entity.length - 1;
      continue;
    }
    if (DOUBLE_MARKS.has(char)) {
      diagnostics.push(diagnostic(text, index, index + 1, severity));
    } else if (OPEN_SINGLE.has(char)) {
      unicodeSingleOpen = index;
      diagnostics.push(diagnostic(text, index, index + 1, severity));
    } else if (CLOSE_SINGLE.has(char) && unicodeSingleOpen !== -1) {
      unicodeSingleOpen = -1;
    }
  }
  if (inTag) {
    diagnostics.push({
      code: 'unterminated_inline_tag',
      severity,
      message: 'unterminated inline tag prevents reliable quotation linting',
      start: tagStart,
      end: text.length,
      excerpt: excerptAt(text, tagStart, text.length),
    });
  }

  // Straight apostrophes are common, so flag only a delimited 'phrase' pair.
  const straightSingle = /(^|[\s([])'([^'\n]+)'(?=$|[\s.,;:!?\)\]])/g;
  for (const match of text.matchAll(straightSingle)) {
    const start = (match.index ?? 0) + match[1].length;
    // Do not double-report attribute-like text inside a tag.
    const prefix = text.slice(0, start);
    if (prefix.lastIndexOf('<') > prefix.lastIndexOf('>')) continue;
    diagnostics.push(diagnostic(text, start, start + match[0].length - match[1].length, severity));
  }
  return diagnostics.sort((a, b) => a.start - b.start || a.end - b.end);
}

/** Host-owned rendering for a non-quotational mention of a word or phrase. */
export function renderTerm(spec: TermSpec): { text: string; kind: 'term'; style: 'double' | 'single' } {
  if (!spec || typeof spec.text !== 'string' || spec.text.length === 0) throw new Error('term text must be a non-empty string');
  if (lintUnverifiedQuotes(spec.text).length > 0) throw new Error('term text must not contain quotation marks');
  const style = spec.style ?? 'double';
  if (style !== 'double' && style !== 'single') throw new Error('term style must be double or single');
  return { text: style === 'double' ? `“${spec.text}”` : `‘${spec.text}’`, kind: 'term', style };
}

/**
 * Compile the structured boundary used by document text fields. Only plain
 * string parts are linted; trusted host resolution owns Quote rendering and
 * renderTerm owns Term punctuation.
 */
export async function compileInline(
  parts: unknown[],
  policy: InlineQuotePolicy,
  resolveStructuredQuote: (spec: Record<string, unknown>) => Promise<{ text: string; quote_id: string }>,
): Promise<CompiledInline> {
  if (!Array.isArray(parts)) throw new Error('Inline.parts must be a list');
  if (policy !== 'error' && policy !== 'comment') throw new Error('quotation policy must be error or comment');
  const rendered: string[] = [];
  const diagnostics: Array<QuoteLintDiagnostic & { part: number }> = [];
  const quoteIds: string[] = [];
  let verifiedQuotes = 0;
  let terms = 0;
  let previousQuoteHasTerminalPunctuation = false;
  for (const [part, value] of parts.entries()) {
    if (typeof value === 'string') {
      const severity: QuoteLintSeverity = policy === 'error' ? 'error' : 'warning';
      const found = lintUnverifiedQuotes(value, severity).map((item) => ({ ...item, part }));
      diagnostics.push(...found);
      if (previousQuoteHasTerminalPunctuation && /^[.!?]/.test(value)) {
        diagnostics.push({
          code: 'duplicate_punctuation_after_quote',
          severity,
          message: 'structured quote already contains terminal punctuation; remove the following punctuation',
          start: 0,
          end: 1,
          excerpt: value.slice(0, 25),
          part,
        });
      }
      rendered.push(value);
      if (value.length > 0) previousQuoteHasTerminalPunctuation = false;
      continue;
    }
    if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error(`Inline part ${part} must be text, Quote, or Term`);
    const record = value as Record<string, unknown>;
    if (record.$kind === 'term') {
      const term = renderTerm(record as unknown as TermSpec);
      rendered.push(term.text);
      terms += 1;
      previousQuoteHasTerminalPunctuation = false;
    } else if (record.$kind === 'quote') {
      const quote = await resolveStructuredQuote(record);
      if (!quote || typeof quote.text !== 'string' || typeof quote.quote_id !== 'string') throw new Error(`Inline quote part ${part} did not resolve`);
      rendered.push(quote.text);
      quoteIds.push(quote.quote_id);
      verifiedQuotes += 1;
      previousQuoteHasTerminalPunctuation = /[.!?][”’]$/.test(quote.text);
    } else {
      throw new Error(`Inline part ${part} has unknown structured kind`);
    }
  }
  if (policy === 'error' && diagnostics.length > 0) {
    const first = diagnostics[0];
    throw new Error(`${first.message} (Inline part ${first.part}, offsets ${first.start}-${first.end})`);
  }
  return {
    text: rendered.join(''),
    audit: {
      status: diagnostics.length === 0 ? 'verified' : 'warnings',
      policy,
      diagnostics,
      verified_quotes: verifiedQuotes,
      terms,
      quote_ids: quoteIds,
    },
  };
}
