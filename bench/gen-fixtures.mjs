#!/usr/bin/env node
// Deterministic DOCX fixture generation for the docxdriver benchmark.
//
// Writes bench/fixtures/{small,medium,large}.docx plus a revised variant, and
// bench/fixtures/fixtures.json describing each (byte size, paragraphs, chars,
// words, marker targets, sha256). Fixture generation is deliberately engine-
// independent (a tiny ZIP writer + deterministic PRNG) so nothing being
// measured leaks into fixture creation; validity is cross-checked by the
// engine in the `validate` phase (see wasm-bench.mjs).
//
//   node bench/gen-fixtures.mjs            # generate (or regenerate)
//   node bench/gen-fixtures.mjs --check    # byte-compare against disk, fail on drift
//
// Text policy: varied dictionary words per paragraph (compressible to roughly
// 40-55% of raw, never pathologically tiny), unique mkrNNNNNN marker words on
// every 20th paragraph (exactly-once search/edit/comment targets), Heading1
// paragraphs on every 25th (outline targets). A minimal w:sectPr keeps the
// package a valid Word document.

import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { buildZip } from './lib/zip.mjs';
import { mulberry32 } from './lib/prng.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
export const FIXTURES_DIR = path.join(here, 'fixtures');
export const WORK_JSON = path.join(FIXTURES_DIR, 'work.json');

const W_NS = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main';
const XML_DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>';

const WORDS = (
  'analysis benchmark boundary canvas delimit element fragment granularity hypothesis iterate junction kernel ' +
  'leverage mechanism narrative operator polynomial quadratic rationale segment threshold underlying vector ' +
  'wavelength axis baseline cascade derivative emission flux gradient heuristic index jitter keyframe lattice ' +
  'marginal nodal offset pivot quantile radius spectrum tensor uniform variance wavelet yield zero-sum ' +
  'abstract bracket catalyst domain entropy framework geometry horizon invariant jurisdiction kinetic logarithmic ' +
  'manifold nominal orthant partition quotient stochastic ternary umbrella valence workstream'
).split(' ');

// Fixture sizes: paragraphs, min/max words per paragraph, marker/heading cadence.
export const FIXTURE_SPECS = {
  small: { paragraphs: 40, minWords: 8, maxWords: 13 },
  medium: { paragraphs: 400, minWords: 14, maxWords: 20 },
  large: { paragraphs: 3000, minWords: 22, maxWords: 32 },
};
const MARKER_EVERY = 20; // every 20th paragraph (0-based) carries mkrNNNNNN
const AUX_EVERY = 10; // every 10th paragraph starting at 5 carries auxNNNNNN (revision-fixture edit targets)
const HEADING_EVERY = 25; // every 25th paragraph is Heading1

export function escapeXml(text) {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function sentence(rand, minWords, maxWords) {
  const count = minWords + Math.floor(rand() * (maxWords - minWords + 1));
  const words = [];
  for (let i = 0; i < count; i++) words.push(WORDS[Math.floor(rand() * WORDS.length)]);
  words[0] = words[0][0].toUpperCase() + words[0].slice(1);
  return words.join(' ') + '.';
}

/** Deterministic body XML for a fixture: paragraphs + minimal sectPr. */
export function bodyXml(spec, seed) {
  const rand = mulberry32(seed);
  const paragraphs = [];
  let chars = 0;
  let words = 0;
  for (let i = 0; i < spec.paragraphs; i++) {
    const text = sentence(rand, spec.minWords, spec.maxWords);
    const isHeading = i % HEADING_EVERY === 0;
    let marker = '';
    if (i % MARKER_EVERY === 0) marker = ` mkr${String(i).padStart(6, '0')}`;
    else if (i % AUX_EVERY === 5) marker = ` aux${String(i).padStart(6, '0')}`;
    const content = escapeXml(text + marker);
    chars += text.length + marker.length;
    words += (text + marker).split(/\s+/).filter(Boolean).length;
    if (isHeading) {
      paragraphs.push(
        `<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t xml:space="preserve">${content}</w:t></w:r></w:p>`,
      );
    } else {
      paragraphs.push(`<w:p><w:r><w:t xml:space="preserve">${content}</w:t></w:r></w:p>`);
    }
  }
  paragraphs.push(
    '<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr>',
  );
  return { body: paragraphs.join(''), chars, words };
}

export function packageParts(body) {
  return [
    {
      name: '[Content_Types].xml',
      data: Buffer.from(
        `${XML_DECL}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">` +
          '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>' +
          '<Default Extension="xml" ContentType="application/xml"/>' +
          '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>' +
          '</Types>',
        'utf8',
      ),
    },
    {
      name: '_rels/.rels',
      data: Buffer.from(
        `${XML_DECL}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">` +
          '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>' +
          '</Relationships>',
        'utf8',
      ),
    },
    {
      name: 'word/document.xml',
      data: Buffer.from(
        `${XML_DECL}<w:document xmlns:w="${W_NS}"><w:body>${body}</w:body></w:document>`,
        'utf8',
      ),
    },
    {
      name: 'word/_rels/document.xml.rels',
      data: Buffer.from(
        `${XML_DECL}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>`,
        'utf8',
      ),
    },
  ];
}

export function buildFixture(name) {
  const spec = FIXTURE_SPECS[name];
  if (!spec) throw new Error(`unknown fixture: ${name}`);
  const seed = `docxdriver-bench-${name}`.split('').reduce((a, c) => (a * 31 + c.charCodeAt(0)) >>> 0, 7);
  const { body, chars, words } = bodyXml(spec, seed);
  const bytes = buildZip(packageParts(body));
  const markers = [];
  for (let i = 0; i < spec.paragraphs; i += MARKER_EVERY) markers.push(i);
  const aux = [];
  for (let i = 5; i < spec.paragraphs; i += AUX_EVERY) aux.push(i);
  return {
    name,
    bytes,
    meta: {
      paragraphs: spec.paragraphs,
      chars,
      words,
      markers,
      aux,
      headings: Math.ceil(spec.paragraphs / HEADING_EVERY),
    },
  };
}

export async function writeFixtures() {
  await mkdir(FIXTURES_DIR, { recursive: true });
  const fixtures = {};
  for (const name of Object.keys(FIXTURE_SPECS)) {
    const { bytes, meta } = buildFixture(name);
    const file = path.join(FIXTURES_DIR, `${name}.docx`);
    await writeFile(file, bytes);
    fixtures[name] = {
      ...meta,
      file: `${name}.docx`,
      fileBytes: bytes.length,
      sha256: createHash('sha256').update(bytes).digest('hex'),
    };
  }
  await writeFile(
    path.join(FIXTURES_DIR, 'fixtures.json'),
    JSON.stringify({ fixtures, generatedAt: 'deterministic' }, null, 2),
  );
  return fixtures;
}

/** --check: regenerate in memory and byte-compare against disk. */
export async function checkFixtures() {
  let mismatch = null;
  for (const name of Object.keys(FIXTURE_SPECS)) {
    const { bytes } = buildFixture(name);
    const onDisk = await readFile(path.join(FIXTURES_DIR, `${name}.docx`));
    const a = createHash('sha256').update(bytes).digest('hex');
    const b = createHash('sha256').update(onDisk).digest('hex');
    if (a !== b) {
      mismatch = { name, expected: a, onDisk: b };
      break;
    }
  }
  if (mismatch) {
    console.error(`fixture drift: ${mismatch.name} regenerates to a different sha256 than on disk`);
    process.exit(1);
  }
  console.log('fixtures deterministic: regenerated bytes match disk for all sizes');
}

async function main() {
  const check = process.argv.includes('--check');
  if (check) {
    await checkFixtures();
    return;
  }
  const fixtures = await writeFixtures();
  for (const [name, meta] of Object.entries(fixtures)) {
    console.log(
      `${name}: ${meta.fileBytes} bytes, ${meta.paragraphs} paragraphs, ~${meta.chars} chars, ~${meta.words} words, ${meta.markers.length + meta.aux.length} markers`,
    );
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((err) => {
    console.error(err);
    process.exit(1);
  });
}
