/**
 * Deterministic conformance corpus and a dependency-free ZIP part-hash reader.
 *
 * `createContractFixture` renders the fixed contract HTML through the real
 * checked-in WASM (create is deterministic: same input bytes → identical
 * output bytes and identical rendered ids), so every surface sees identical
 * fresh copies. `createComplexFixture` copies the checked-in real-world
 * package (fields/media/comments) byte-identically for package-preservation
 * checks. `zipPartHashes` reads only — central directory walk, deflate via
 * node:zlib, no dependencies, no writes.
 */
import { createHash } from 'node:crypto';
import { copyFile, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { inflateRawSync } from 'node:zlib';
import { ensureInit, runCreate, runRead } from '../dist/engine.js';

/** The exact contract corpus: 1 heading + 12 paragraphs (13 blocks). */
export const CONTRACT_HTML = [
  '<h1>Service Agreement</h1>',
  '<p>The notice period is <b>thirty days</b>.</p>',
  '<p>Payment is due within 30 days of invoice receipt.</p>',
  '<p>Payment is due within 45 days of invoice receipt.</p>',
  '<p>Payment is due within 60 days of invoice receipt.</p>',
  '<p>Confidential Information means any information disclosed by either party.</p>',
  '<p>The parties agree that the terms shall govern, and the terms shall prevail, and the terms shall bind, and the terms shall endure.</p>',
  '<p>Net 30. Net 45. Net 60. Net 90.</p>',
  '<p>The warranty period is twelve months from delivery.</p>',
  '<p>This paragraph will be replaced entirely.</p>',
  '<p>This paragraph anchors an insertion.</p>',
  '<p>This paragraph will be deleted.</p>',
  '<p>Final paragraph. Signatures follow.</p>',
].join('\n');

/**
 * Create `contract.docx` under `cwd` from the fixed corpus HTML and return
 * its absolute path. Paragraph ids are read dynamically from the rendered
 * markup by callers — never hard-coded here.
 */
export async function createContractFixture(cwd) {
  await ensureInit();
  const output = runCreate(undefined, undefined, CONTRACT_HTML);
  if (output.outcome !== 'completed' || !output.bytes) {
    throw new Error(`contract fixture create failed: ${output.diagnostic?.message ?? output.summary ?? 'unknown error'}`);
  }
  const abs = join(cwd, 'contract.docx');
  await writeFile(abs, output.bytes);
  return abs;
}

/**
 * Copy the checked-in real-world package (fields, media, comments) to
 * `complex.docx` under `cwd`, byte-identical to the source, and return the
 * copy's absolute path.
 */
export async function createComplexFixture(cwd) {
  const abs = join(cwd, 'complex.docx');
  await copyFile(complexFixtureSourcePath(), abs);
  return abs;
}

/** Absolute path of the checked-in real-world package used for Task 10. */
export function complexFixtureSourcePath() {
  const here = dirname(fileURLToPath(import.meta.url));
  return join(resolve(here, '..', '..', '..'), 'test-docs', 'ctnf-18690238-data-stream.docx');
}

/** Render the canonical projection of the docx at `abs` for the given view. */
export async function renderDocxMarkup(abs, view = 'markup') {
  await ensureInit();
  const bytes = new Uint8Array(await readFile(abs));
  const output = runRead(bytes, { kind: 'document', view });
  if (output.outcome !== 'completed' || !output.result || typeof output.result.markup !== 'string') {
    throw new Error(`render failed: ${output.diagnostic?.message ?? output.summary ?? 'unknown error'}`);
  }
  return output.result.markup;
}

const EOCD_SIG = 0x06054b50;
const CD_SIG = 0x02014b50;
const LOCAL_SIG = 0x04034b50;

function u16(buf, off) {
  return buf.readUInt16LE(off);
}
function u32(buf, off) {
  return buf.readUInt32LE(off);
}

/** Locate the end-of-central-directory record (handles prepended data). */
function findEocd(buf) {
  const min = Math.max(0, buf.length - 22 - 0xffff);
  for (let i = buf.length - 22; i >= min; i -= 1) {
    if (u32(buf, i) === EOCD_SIG) return i;
  }
  throw new Error('zip: end-of-central-directory record not found');
}

/** Inflate (or copy verbatim) one entry's data from the local header. */
function readLocalData(buf, localOffset, method, compressedSize, uncompressedSize) {
  if (u32(buf, localOffset) !== LOCAL_SIG) {
    throw new Error('zip: bad local file header');
  }
  const nameLen = u16(buf, localOffset + 26);
  const extraLen = u16(buf, localOffset + 28);
  const dataOffset = localOffset + 30 + nameLen + extraLen;
  const raw = buf.subarray(dataOffset, dataOffset + compressedSize);
  if (method === 0) {
    if (raw.length !== uncompressedSize) {
      throw new Error('zip: stored entry size mismatch');
    }
    return raw;
  }
  if (method === 8) {
    const out = inflateRawSync(raw);
    if (out.length !== uncompressedSize) {
      throw new Error('zip: deflated entry size mismatch');
    }
    return out;
  }
  throw new Error(`zip: unsupported compression method ${method}`);
}

/**
 * Map entry name → sha256 of the inflated entry bytes. Read-only, no deps.
 */
export async function zipPartHashes(abs) {
  const buf = await readFile(abs);
  const eocd = findEocd(buf);
  const totalEntries = u16(buf, eocd + 10);
  let cursor = u32(buf, eocd + 16);
  const parts = new Map();
  for (let n = 0; n < totalEntries; n += 1) {
    if (u32(buf, cursor) !== CD_SIG) {
      throw new Error(`zip: bad central directory entry ${n}`);
    }
    const method = u16(buf, cursor + 10);
    const compressedSize = u32(buf, cursor + 20);
    const uncompressedSize = u32(buf, cursor + 24);
    const nameLen = u16(buf, cursor + 28);
    const extraLen = u16(buf, cursor + 30);
    const commentLen = u16(buf, cursor + 32);
    const localOffset = u32(buf, cursor + 42);
    const name = buf.toString('utf8', cursor + 46, cursor + 46 + nameLen);
    const data = readLocalData(buf, localOffset, method, compressedSize, uncompressedSize);
    parts.set(name, createHash('sha256').update(data).digest('hex'));
    cursor += 46 + nameLen + extraLen + commentLen;
  }
  return parts;
}
