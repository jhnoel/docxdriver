/**
 * Shared canonical DOCX projection parser and serializer.
 *
 * Parses the engine's rendered document markup (read kind=document, view=markup)
 * into a structural `ProjectionDoc` and serializes it back to the exact
 * canonical dialect. The markup is the agent-facing editing surface, so this
 * module is the contract between the agent's string edits and the typed
 * operations: every construct the renderer can emit must round-trip losslessly.
 *
 * Dialect (verified against crates/docxdriver-core/src/html/render.rs):
 * - blocks: `<p id ord class num break-ins break-del>…</p>`, `<h1>…</h1>..<h6>…</h6>`,
 *   and verbatim `<table>…</table>`; blocks are joined with `\n`.
 * - inline: text (escaped `&amp; &lt; &gt;`), `<b> <i> <u> <s> <sup> <sub>`,
 *   `<a href>`, `<field instr>`, `<del id author>`, `<ins id author>`,
 *   `<format pending id author>`, self-closing `<br/>`/`<br type="…"/>`,
 *   `<image w h alt/>`, `<comment-start id/>`/`<comment-end id/>`,
 *   `<footnote>…</footnote>`/`<footnote/>` (same for endnote),
 *   `<equation display math>…</equation>`.
 * - Formatting tags close before link/field/del/ins boundaries and reopen after.
 *   For br, image, comment milestones, notes, and equations the renderer's
 *   transition is a no-op, so fmt stays OPEN across them (`<b>line1<br/>line2</b>`);
 *   the serializer reproduces exactly that, item by item.
 * - `del`/`ins`/`format` wrappers always carry `id` and `author` in rendered
 *   markup; fmt tags may carry `pending id author` for tracked format
 *   revisions (treated as opaque protected content).
 *
 * Protected constructs (link/field/del/ins/format/table/image/equation/notes/
 * comment milestones) are captured verbatim so round-tripping never re-authors
 * them; consumers treat any change inside them as unsupported.
 */

export type FormatFlags = {
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strike: boolean;
  superscript: boolean;
  subscript: boolean;
};

export type ProjectionItem =
  | { kind: 'text'; text: string; fmt: FormatFlags }
  | { kind: 'link'; text: string; href: string; fmt: FormatFlags }
  | { kind: 'field'; text: string; instr: string }
  | { kind: 'image' | 'equation' | 'table' | 'protected'; text: string }
  | { kind: 'revision'; tag: 'del' | 'ins' | 'format'; text: string; fmt: FormatFlags; id?: string; author?: string };

export type ParagraphTag = 'p' | 'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6';

export type ProjectionParagraph = {
  id: string | null; // null only for id-less (new) paragraphs
  tag: ParagraphTag;
  styleClass: string | null; // class attribute when tag is p
  attrs: Record<string, string>; // ord, num, break-ins, break-del if present
  items: ProjectionItem[];
  plainText: string; // concatenated item text
  markup: string; // re-serialized paragraph markup
};

export type ProjectionBlock =
  | { kind: 'paragraph'; paragraph: ProjectionParagraph }
  | { kind: 'protected'; text: string }; // verbatim table blocks

export type ProjectionDoc = { paragraphs: ProjectionParagraph[]; blocks: ProjectionBlock[] };

export class ProjectionError extends Error {}

const PARA_TAGS = new Set(['p', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6']);
const FMT_TAGS = new Set(['b', 'i', 'u', 's', 'sup', 'sub']);
const VERBATIM_TAGS = new Set(['a', 'field', 'del', 'ins', 'format']);
const SELF_CLOSING_TAGS = new Set(['br', 'image', 'comment-start', 'comment-end']);
const NOTE_TAGS = new Set(['footnote', 'endnote']);
const ID_RE = /^[0-9A-F]{8}$/;

const FMT_KEY: Record<string, keyof FormatFlags> = {
  b: 'bold',
  i: 'italic',
  u: 'underline',
  s: 'strike',
  sup: 'superscript',
  sub: 'subscript',
};

function blankFmt(): FormatFlags {
  return { bold: false, italic: false, underline: false, strike: false, superscript: false, subscript: false };
}

function fmtTags(fmt: FormatFlags): string[] {
  const tags: string[] = [];
  if (fmt.bold) tags.push('b');
  if (fmt.italic) tags.push('i');
  if (fmt.underline) tags.push('u');
  if (fmt.strike) tags.push('s');
  if (fmt.superscript) tags.push('sup');
  if (fmt.subscript) tags.push('sub');
  return tags;
}

function decodeEntities(text: string): string {
  return text.replace(/&(amp|lt|gt|quot);/g, (_, name: string) =>
    name === 'amp' ? '&' : name === 'lt' ? '<' : name === 'gt' ? '>' : '"',
  );
}

/** Mirror crates/docxdriver-core/src/html/mod.rs::escape_text (& < > only). */
function escapeText(text: string): string {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

/** Strip tags from a verbatim inner slice and decode entities → plain text. */
function innerPlain(raw: string): string {
  return decodeEntities(raw.replace(/<[^>]*>/g, ''));
}

function escapeAttr(text: string): string {
  return escapeText(text).replace(/"/g, '&quot;');
}

/**
 * Transition the open fmt stack from `open` to `target`, mirroring render.rs
 * transition_fmt exactly: keep the longest common PREFIX of the two fmt stacks,
 * close every tag after it in LIFO order (`</i></b>`), then reopen the target
 * tags after the prefix. This is what makes overlapping non-nested fmt sets
 * (e.g. {b} → {b,i} → {i} emitting `</i></b><i>`) round-trip byte-for-byte.
 */
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
  return fmtTags(fmt)
    .slice()
    .reverse()
    .map((tag) => `</${tag}>`)
    .join('');
}

/**
 * Pending-fmt protected scopes (`<b pending …>`) are fmt boundaries — the
 * renderer closes all fmt before a pending run. Everything else captured
 * verbatim (br, image, comment milestones, notes, equations) leaves fmt open:
 * render.rs transition() is a no-op when the wrappers and fmt are unchanged,
 * so the engine emits `<b>line1<br/>line2</b>` with fmt spanning the element.
 */
const FMT_SCOPE_RE = /^<(b|i|u|s|sup|sub)([\s>])/;

function itemPlain(item: ProjectionItem): string {
  switch (item.kind) {
    case 'text':
      return item.text;
    case 'link':
    case 'field':
    case 'revision':
    case 'equation':
      return innerPlain(item.text);
    case 'image':
    case 'table':
    case 'protected':
      return innerPlain(item.text);
  }
}

/** Serialize a hard-boundary item — fmt is fully closed before it and reopened by the next text item. */
function boundaryMarkup(item: ProjectionItem): string {
  switch (item.kind) {
    case 'link':
      return `<a href="${escapeAttr(item.href)}">${item.text}</a>`;
    case 'field':
      return `<field instr="${escapeAttr(item.instr)}">${item.text}</field>`;
    case 'revision': {
      let out = `<${item.tag}`;
      if (item.tag === 'format') out += ' pending';
      if (item.id !== undefined) out += ` id="${escapeAttr(item.id)}"`;
      if (item.author !== undefined) out += ` author="${escapeAttr(item.author)}"`;
      return `${out}>${item.text}</${item.tag}>`;
    }
    default:
      throw new ProjectionError(`internal: ${item.kind} is not a boundary item`);
  }
}

export function paragraphMarkup(p: ProjectionParagraph): string {
  let out = `<${p.tag}`;
  if (p.id !== null) out += ` id="${escapeAttr(p.id)}"`;
  if (p.attrs.ord !== undefined) out += ` ord="${escapeAttr(p.attrs.ord)}"`;
  if (p.styleClass !== null) out += ` class="${escapeAttr(p.styleClass)}"`;
  if (p.attrs.num !== undefined) out += ` num="${escapeAttr(p.attrs.num)}"`;
  if (p.attrs['break-ins'] !== undefined) out += ` break-ins="${escapeAttr(p.attrs['break-ins'])}"`;
  if (p.attrs['break-del'] !== undefined) out += ` break-del="${escapeAttr(p.attrs['break-del'])}"`;
  out += '>';
  let open: FormatFlags = blankFmt();
  for (const item of p.items) {
    switch (item.kind) {
      case 'text':
        out += transitionFmt(open, item.fmt);
        out += escapeText(item.text);
        open = item.fmt;
        break;
      case 'link':
      case 'field':
      case 'revision':
        out += closeFmtTags(open);
        open = blankFmt();
        out += boundaryMarkup(item);
        break;
      case 'protected':
        if (FMT_SCOPE_RE.test(item.text)) {
          out += closeFmtTags(open);
          open = blankFmt();
        }
        out += item.text;
        break;
      case 'image':
      case 'equation':
      case 'table':
        out += item.text; // fmt stays open across these, exactly as the renderer emits
        break;
    }
  }
  out += closeFmtTags(open);
  out += `</${p.tag}>`;
  return out;
}

export function serializeProjection(doc: ProjectionDoc): string {
  const blocks =
    Array.isArray(doc.blocks) && doc.blocks.length > 0
      ? doc.blocks
      : doc.paragraphs.map((paragraph) => ({ kind: 'paragraph' as const, paragraph }));
  return blocks
    .map((block) => (block.kind === 'paragraph' ? paragraphMarkup(block.paragraph) : block.text))
    .join('\n');
}

type TagInfo = {
  name: string;
  attrs: Record<string, string>;
  selfClosing: boolean;
  closing: boolean;
  openStart: number; // position of '<'
};

function parseMarkup(markup: string, loose: boolean): ProjectionDoc {
  const doc: ProjectionDoc = { paragraphs: [], blocks: [] };
  const len = markup.length;
  let pos = 0;

  const error = (message: string): never => {
    throw new ProjectionError(message);
  };

  const readTag = (): TagInfo => {
    const openStart = pos;
    let i = pos + 1;
    const closing = markup[i] === '/';
    if (closing) i += 1;
    const nameStart = i;
    while (i < len && /[a-zA-Z0-9-]/.test(markup[i])) i += 1;
    const name = markup.slice(nameStart, i);
    if (name === '') error(`malformed tag near ${JSON.stringify(markup.slice(openStart, openStart + 20))}`);
    if (closing) {
      while (i < len && markup[i] === ' ') i += 1;
      if (markup[i] !== '>') error(`malformed close tag </${name}>`);
      pos = i + 1;
      return { name, attrs: {}, selfClosing: false, closing: true, openStart };
    }
    const attrs: Record<string, string> = {};
    let selfClosing = false;
    for (;;) {
      while (i < len && (markup[i] === ' ' || markup[i] === '\t' || markup[i] === '\n')) i += 1;
      if (i >= len) error(`unterminated tag <${name}>`);
      const c = markup[i];
      if (c === '>') {
        i += 1;
        break;
      }
      if (c === '/') {
        if (markup[i + 1] === '>') {
          i += 2;
          selfClosing = true;
          break;
        }
        error(`malformed self-close in <${name}>`);
      }
      const aStart = i;
      while (i < len && /[a-zA-Z0-9-]/.test(markup[i])) i += 1;
      const aName = markup.slice(aStart, i);
      if (aName === '') error(`malformed attribute in <${name}>`);
      while (i < len && markup[i] === ' ') i += 1;
      if (markup[i] === '=') {
        i += 1;
        while (i < len && markup[i] === ' ') i += 1;
        if (markup[i] !== '"') error(`attribute ${aName} in <${name}> must be double-quoted`);
        i += 1;
        const vStart = i;
        while (i < len && markup[i] !== '"') i += 1;
        if (i >= len) error(`unterminated attribute value for ${aName} in <${name}>`);
        attrs[aName] = decodeEntities(markup.slice(vStart, i));
        i += 1;
      } else {
        // Valueless flag attribute (the renderer emits `<equation display …>`).
        attrs[aName] = '';
      }
    }
    pos = i;
    return { name, attrs, selfClosing, closing: false, openStart };
  };

  /** Capture the raw slice from `from` to the matching close tag; error if absent. */
  const captureTo = (tag: string, from: number): { raw: string; end: number } => {
    const close = `</${tag}>`;
    const closeAt = markup.indexOf(close, from);
    if (closeAt === -1) error(`unclosed <${tag}>`);
    return { raw: markup.slice(from, closeAt), end: closeAt + close.length };
  };

  // Per-paragraph build state.
  let paraTag: ParagraphTag | null = null;
  let paraId: string | null = null;
  let styleClass: string | null = null;
  let attrs: Record<string, string> = {};
  let items: ProjectionItem[] = [];
  let fmt: FormatFlags = blankFmt();
  let textBuf = '';
  let stack: Array<{ tag: string }> = [];

  const paragraphContext = (): string => (paraTag === null ? 'document' : `paragraph ${paraId ?? '<no id>'}`);

  const flushText = (): void => {
    if (textBuf === '') return;
    items.push({ kind: 'text', text: textBuf, fmt: { ...fmt } });
    textBuf = '';
  };

  const finalizeParagraph = (): void => {
    const plainText = items.map(itemPlain).join('');
    const paragraph: ProjectionParagraph = {
      id: paraId,
      tag: paraTag as ParagraphTag,
      styleClass,
      attrs,
      items,
      plainText,
      markup: '',
    };
    paragraph.markup = paragraphMarkup(paragraph);
    doc.paragraphs.push(paragraph);
    doc.blocks.push({ kind: 'paragraph', paragraph });
  };

  while (pos < len) {
    const lt = markup.indexOf('<', pos);
    if (lt === -1) {
      const rest = markup.slice(pos);
      if (paraTag === null) {
        if (rest.trim() !== '') error(`text outside a paragraph element: ${JSON.stringify(rest.slice(0, 40))}`);
      } else {
        textBuf += decodeEntities(rest);
      }
      pos = len;
      break;
    }
    if (lt > pos) {
      const run = markup.slice(pos, lt);
      if (paraTag === null) {
        if (run.trim() !== '') error(`text outside a paragraph element: ${JSON.stringify(run.slice(0, 40))}`);
      } else {
        textBuf += decodeEntities(run);
      }
      pos = lt;
    }

    const tag = readTag();

    if (tag.closing) {
      if (paraTag === null) error(`unexpected close tag </${tag.name}> at ${paragraphContext()}`);
      if (FMT_TAGS.has(tag.name)) {
        const top = stack[stack.length - 1];
        if (top === undefined || top.tag !== tag.name) {
          error(`mismatched close tag </${tag.name}> in ${paragraphContext()}`);
        }
        flushText();
        stack.pop();
        fmt[FMT_KEY[tag.name]] = false;
        continue;
      }
      if (PARA_TAGS.has(tag.name)) {
        if (tag.name !== paraTag) error(`mismatched close tag </${tag.name}> for ${paragraphContext()}`);
        if (stack.length > 0) error(`unclosed <${stack[stack.length - 1].tag}> before </${paraTag}>`);
        flushText();
        finalizeParagraph();
        paraTag = null;
        paraId = null;
        styleClass = null;
        attrs = {};
        items = [];
        fmt = blankFmt();
        textBuf = '';
        stack = [];
        continue;
      }
      error(`unexpected close tag </${tag.name}> in ${paragraphContext()}`);
    }

    // Open tags.
    if (paraTag === null) {
      if (PARA_TAGS.has(tag.name)) {
        if (tag.selfClosing) error(`<${tag.name}/> is not a valid paragraph`);
        const allowed = tag.name === 'p' ? ['id', 'ord', 'class', 'num', 'break-ins', 'break-del'] : ['id', 'ord', 'num', 'break-ins', 'break-del'];
        for (const key of Object.keys(tag.attrs)) {
          if (!allowed.includes(key)) error(`unexpected attribute ${key} on <${tag.name}>`);
        }
        if (tag.attrs.id === undefined) {
          if (!loose) error(`missing id on <${tag.name}>`);
          paraId = null;
        } else {
          if (!ID_RE.test(tag.attrs.id)) {
            error(`invalid paragraph id ${JSON.stringify(tag.attrs.id)} (expected eight uppercase hex digits)`);
          }
          paraId = tag.attrs.id;
        }
        styleClass = tag.name === 'p' ? (tag.attrs.class ?? null) : null;
        attrs = {};
        for (const key of ['ord', 'num', 'break-ins', 'break-del']) {
          if (tag.attrs[key] !== undefined) attrs[key] = tag.attrs[key];
        }
        paraTag = tag.name as ParagraphTag;
        items = [];
        fmt = blankFmt();
        textBuf = '';
        stack = [];
        continue;
      }
      if (tag.name === 'table') {
        if (tag.selfClosing) {
          doc.blocks.push({ kind: 'protected', text: markup.slice(tag.openStart, pos) });
          continue;
        }
        const close = `</table>`;
        const closeAt = markup.indexOf(close, pos);
        if (closeAt === -1) error('unclosed <table>');
        doc.blocks.push({ kind: 'protected', text: markup.slice(tag.openStart, closeAt + close.length) });
        pos = closeAt + close.length;
        continue;
      }
      if (FMT_TAGS.has(tag.name) || VERBATIM_TAGS.has(tag.name) || SELF_CLOSING_TAGS.has(tag.name) || NOTE_TAGS.has(tag.name)) {
        error(`<${tag.name}> outside a paragraph`);
      }
      error(`unknown tag <${tag.name}>`);
    }

    // Inside a paragraph.
    if (FMT_TAGS.has(tag.name)) {
      if (tag.selfClosing) error(`<${tag.name}/> is not valid inside a paragraph`);
      if (tag.attrs.pending !== undefined) {
        // Tracked format revision: opaque protected scope, captured verbatim.
        for (const key of Object.keys(tag.attrs)) {
          if (key !== 'pending' && key !== 'id' && key !== 'author') {
            error(`unexpected attribute ${key} on <${tag.name} pending>`);
          }
        }
        flushText();
        const { end } = captureTo(tag.name, pos);
        const full = markup.slice(tag.openStart, end);
        pos = end;
        items.push({ kind: 'protected', text: full });
        continue;
      }
      if (Object.keys(tag.attrs).length > 0) {
        error(`unexpected attribute on <${tag.name}>`);
      }
      const key = FMT_KEY[tag.name];
      if (fmt[key]) error(`nested <${tag.name}> in ${paragraphContext()}`);
      if (tag.name === 'sup' && fmt.subscript) error('conflicting <sup> inside <sub>');
      if (tag.name === 'sub' && fmt.superscript) error('conflicting <sub> inside <sup>');
      flushText();
      fmt[key] = true;
      stack.push({ tag: tag.name });
      continue;
    }
    if (VERBATIM_TAGS.has(tag.name)) {
      if (tag.selfClosing) error(`<${tag.name}/> is not valid inside a paragraph`);
      let allowed: string[];
      if (tag.name === 'a') allowed = ['href'];
      else if (tag.name === 'field') allowed = ['instr'];
      else if (tag.name === 'del' || tag.name === 'ins') allowed = ['id', 'author'];
      else allowed = ['pending', 'id', 'author'];
      for (const key of Object.keys(tag.attrs)) {
        if (!allowed.includes(key)) error(`unexpected attribute ${key} on <${tag.name}>`);
      }
      if (tag.name === 'a' && tag.attrs.href === undefined) error('<a> requires href');
      if (tag.name === 'field' && tag.attrs.instr === undefined) error('<field> requires instr');
      flushText();
      const { raw, end } = captureTo(tag.name, pos);
      pos = end;
      const fmtAtOpen = { ...fmt };
      if (tag.name === 'a') {
        items.push({ kind: 'link', text: raw, href: tag.attrs.href, fmt: fmtAtOpen });
      } else if (tag.name === 'field') {
        items.push({ kind: 'field', text: raw, instr: tag.attrs.instr });
      } else {
        items.push({
          kind: 'revision',
          tag: tag.name as 'del' | 'ins' | 'format',
          text: raw,
          fmt: fmtAtOpen,
          ...(tag.attrs.id !== undefined ? { id: tag.attrs.id } : {}),
          ...(tag.attrs.author !== undefined ? { author: tag.attrs.author } : {}),
        });
      }
      continue;
    }
    if (SELF_CLOSING_TAGS.has(tag.name)) {
      if (!tag.selfClosing) error(`<${tag.name}> must be self-closing`);
      flushText();
      const full = markup.slice(tag.openStart, pos);
      if (tag.name === 'image') {
        items.push({ kind: 'image', text: full });
      } else {
        items.push({ kind: 'protected', text: full });
      }
      continue;
    }
    if (NOTE_TAGS.has(tag.name)) {
      flushText();
      if (tag.selfClosing) {
        items.push({ kind: 'protected', text: markup.slice(tag.openStart, pos) });
        continue;
      }
      const { end } = captureTo(tag.name, pos);
      const full = markup.slice(tag.openStart, end);
      pos = end;
      items.push({ kind: 'protected', text: full });
      continue;
    }
    if (tag.name === 'equation') {
      // `<equation display latex="…"><math>…</math></equation>` — display is a valueless
      // flag attribute; the body is reconstructed LaTeX. Opaque protected item,
      // fmt stays open across it (the renderer's transition is a no-op here).
      flushText();
      if (tag.selfClosing) {
        items.push({ kind: 'equation', text: markup.slice(tag.openStart, pos) });
        continue;
      }
      const { end } = captureTo('equation', pos);
      const full = markup.slice(tag.openStart, end);
      pos = end;
      items.push({ kind: 'equation', text: full });
      continue;
    }
    if (PARA_TAGS.has(tag.name)) {
      error(`nested <${tag.name}> inside ${paragraphContext()}`);
    }
    error(`unknown tag <${tag.name}> in ${paragraphContext()}`);
  }

  if (paraTag !== null) {
    error(`unclosed paragraph <${paraTag}> at end of projection`);
  }
  if (doc.blocks.length === 0) {
    error('empty projection: no paragraph or table blocks found');
  }
  return doc;
}

export function parseProjection(markup: string): ProjectionDoc {
  return parseMarkup(markup, false);
}

export function parseProjectionLoose(markup: string): ProjectionDoc {
  return parseMarkup(markup, true);
}
