/** Browser helpers for the core-owned HTML projection. */
import { executeRequest } from './index.js';

export interface HtmlParagraph {
  id: string;
  index: number;
  text: string;
  selectable_text: string;
}
export interface HtmlAsset { url: string; mime: string; size: number; data_url?: string }
export interface HtmlProjection {
  projection_version: 2;
  content_type: 'text/html';
  html: string;
  css: string;
  view: 'markup' | 'final' | 'original';
  paragraphs: HtmlParagraph[];
  assets: HtmlAsset[];
  equations?: { at: string; equation: number; mathml: string; display: boolean; editable: boolean }[];
  blocks: { html_start: number; html_end: number; paragraphs: string[] }[];
  source: string;
  source_map: {
    html_offset_unit: 'utf8_byte';
    text_offset_unit: 'unicode_scalar';
    regions: { html_start: number; html_end: number; part: string; editable: boolean; paragraphs: string[] }[];
  };
}

export function readHtml(input: Uint8Array, view: HtmlProjection['view'] = 'markup'): HtmlProjection {
  const output = executeRequest(input, { Command: { command: { kind: 'read', read_kind: 'document_ui', view } } });
  if (!('outcome' in output) || output.outcome !== 'completed') {
    throw new Error('diagnostic' in output ? output.diagnostic.message : 'document read failed');
  }
  return output.result as HtmlProjection;
}

/** A portable HTML document. Asset payloads are fetched from the same input
 * bytes, keeping ordinary model-facing reads free of base64 image strings. */
export function renderHtmlDocument(input: Uint8Array, view: HtmlProjection['view'] = 'final', title = 'Document'): string {
  const projection = readHtml(input, view);
  const output = executeRequest(input, { Command: { command: { kind: 'read', read_kind: 'assets' } } });
  if (!('outcome' in output) || output.outcome !== 'completed') throw new Error('asset read failed');
  const assets = (output.result as { assets: HtmlAsset[] }).assets;
  let html = projection.html;
  for (const asset of assets) {
    if (asset.data_url) html = html.replaceAll(`src="${asset.url}"`, `src="${asset.data_url}"`);
  }
  const escapedTitle = title.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>${escapedTitle}</title><style>${projection.css}</style></head><body><main class="docxdriver">${html}</main></body></html>`;
}

const mountCounters = new WeakMap<Document, number>();

/** Mount a projection with instance-local DOM IDs and note links. Source
 * addresses stay in data-docx-paragraph; source_map offsets still refer to the
 * original projection.html. Optional asset payloads come from read_kind: assets.
 * Use this when multiple documents or multiple copies share a browser page. */
export function mountHtmlProjection(root: Element, projection: HtmlProjection, assets: readonly HtmlAsset[] = []): void {
  const doc = root.ownerDocument;
  let counter = mountCounters.get(doc) ?? 0;
  let scope: string;
  do { scope = `docxdriver-mount-${++counter}`; }
  while (doc.querySelector(`[data-docx-instance="${scope}"]`));
  mountCounters.set(doc, counter);
  const template = doc.createElement('template');
  template.innerHTML = projection.html;
  const targets = new Map<string, string>();
  let index = 0;
  for (const node of Array.from(template.content.querySelectorAll('[id]'))) {
    const sourceId = node.id;
    const domId = `${scope}-${++index}`;
    if (node.matches('p, h1, h2, h3, h4, h5, h6') && !node.hasAttribute('data-docx-paragraph')) {
      node.setAttribute('data-docx-paragraph', sourceId);
    }
    targets.set(sourceId, domId);
    node.id = domId;
  }
  for (const link of Array.from(template.content.querySelectorAll('a[href^="#"]'))) {
    const target = targets.get(link.getAttribute('href')!.slice(1));
    if (target) link.setAttribute('href', `#${target}`);
  }
  const payloads = new Map(assets.filter(asset => asset.data_url).map(asset => [asset.url, asset.data_url!]));
  for (const image of Array.from(template.content.querySelectorAll('img[src]'))) {
    const payload = payloads.get(image.getAttribute('src')!);
    if (payload) image.setAttribute('src', payload);
  }
  const style = doc.createElement('style');
  style.textContent = projection.css;
  root.classList.add('docxdriver');
  root.setAttribute('data-docx-instance', scope);
  root.replaceChildren(style, template.content);
}

export interface HtmlTextSelection { at: string; select: string; occurrence: number; start: number; end: number }

/** Resolve a browser Range against source text, excluding rendered objects,
 * hidden markers and deletions. Offsets use Unicode scalars, as the core does.
 * This helper creates a selector; the source-bound core still validates edits. */
export function resolveHtmlSelection(root: Element, range: Range, projection: HtmlProjection): HtmlTextSelection {
  if (projection.view === 'original') throw new Error('original view is not an editable projection');
  const owner = (node: Node): Element | null => {
    const element = node.nodeType === 1 ? node as Element : node.parentElement;
    return element?.closest('[data-docx-paragraph], p[id], h1[id], h2[id], h3[id], h4[id], h5[id], h6[id]') ?? null;
  };
  const paragraph = owner(range.startContainer);
  if (!paragraph || paragraph !== owner(range.endContainer) || !root.contains(paragraph)) {
    throw new Error('selection must stay within one source paragraph');
  }
  if (paragraph.closest('header, footer, aside')) throw new Error('use a dedicated operation for document chrome and notes');
  const id = paragraph.getAttribute('data-docx-paragraph') ?? paragraph.id;
  const source = projection.paragraphs.find(p => p.id === id);
  if (!source) throw new Error('selection has no source paragraph');
  const doc = root.ownerDocument;
  const walker = doc.createTreeWalker(paragraph, 4); // SHOW_TEXT, independent of global window.
  let current = '';
  let start: number | undefined;
  let end: number | undefined;
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    if (node.parentElement?.closest('math, [data-docx-note], [data-docx-image-unavailable], [hidden], [data-docx-tab], del')) continue;
    const text = node.textContent ?? '';
    const scalarLength = (value: string) => [...value].length;
    const base = [...current].length;
    if (node === range.startContainer) start = base + scalarLength(text.slice(0, range.startOffset));
    if (node === range.endContainer) end = base + scalarLength(text.slice(0, range.endOffset));
    current += text;
  }
  if (current !== source.selectable_text) throw new Error('rendered paragraph does not match source text; refresh the projection');
  // Element-boundary ranges are common when selecting an entire paragraph.
  const offsetAtBoundary = (container: Node, offset: number): number => {
    const prefix = doc.createRange(); prefix.selectNodeContents(paragraph); prefix.setEnd(container, offset);
    const fragment = prefix.cloneContents();
    for (const el of Array.from(fragment.querySelectorAll('math, [data-docx-note], [data-docx-image-unavailable], [hidden], [data-docx-tab], del'))) el.remove();
    return [...(fragment.textContent ?? '')].length;
  };
  start ??= offsetAtBoundary(range.startContainer, range.startOffset);
  end ??= offsetAtBoundary(range.endContainer, range.endOffset);
  const select = [...current].slice(start, end).join('');
  if (!select) throw new Error('selection contains no editable text');
  let occurrence = 0;
  let from = 0;
  const targetByte = [...current].slice(0, start).join('').length; // JS UTF-16 position for indexOf.
  while (from <= targetByte) {
    const found = current.indexOf(select, from);
    if (found < 0 || found > targetByte) break;
    occurrence++;
    if (found === targetByte) return { at: id, select, occurrence, start, end };
    from = found + select.length;
  }
  throw new Error('selection is not an exact source span');
}
