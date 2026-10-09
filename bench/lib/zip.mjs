// Minimal, deterministic ZIP writer for DOCX fixtures.
//
// Purpose: produce semantically valid .docx packages from plain parts without
// any dependency, with byte-for-byte deterministic output (fixed DOS
// timestamps, no extra fields, no data descriptors). The engine reads packages
// generically through the zip crate (see crates/docxdriver-core/src/package.rs),
// so this is the smallest shape that round-trips: [Content_Types].xml,
// _rels/.rels, word/document.xml, word/_rels/document.xml.rels — the same
// minimal shape the core test helper (tests/common/mod.rs) uses.

import { deflateRawSync } from 'node:zlib';

// CRC-32 (IEEE 802.3), table-driven.
const CRC_TABLE = new Uint32Array(256);
for (let n = 0; n < 256; n++) {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  CRC_TABLE[n] = c >>> 0;
}

export function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

const LFH = 0x04034b50;
const CDH = 0x02014b50;
const EOCD = 0x06054b50;
const METHOD_DEFLATE = 8;
// Fixed DOS timestamp (1980-01-01 00:00:00) — deterministic archives.
const DOS_TIME = 0;
const DOS_DATE = 0x21;

/**
 * Build a deterministic ZIP archive.
 * @param {Array<{name: string, data: Uint8Array}>} parts archive entries in order
 * @returns {Buffer}
 */
export function buildZip(parts) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const { name, data } of parts) {
    const nameBuf = Buffer.from(name, 'utf8');
    const crc = crc32(data);
    const comp = deflateRawSync(data, { level: 9 });
    const csize = comp.length;
    const usize = data.length;

    const lfh = Buffer.alloc(30);
    lfh.writeUInt32LE(LFH, 0);
    lfh.writeUInt16LE(20, 4); // version needed to extract
    lfh.writeUInt16LE(0, 6); // flags (sizes known → no data descriptor)
    lfh.writeUInt16LE(METHOD_DEFLATE, 8);
    lfh.writeUInt16LE(DOS_TIME, 10);
    lfh.writeUInt16LE(DOS_DATE, 12);
    lfh.writeUInt32LE(crc, 14);
    lfh.writeUInt32LE(csize, 18);
    lfh.writeUInt32LE(usize, 22);
    lfh.writeUInt16LE(nameBuf.length, 26);
    lfh.writeUInt16LE(0, 28); // extra field length
    locals.push(lfh, nameBuf, comp);

    const cdh = Buffer.alloc(46);
    cdh.writeUInt32LE(CDH, 0);
    cdh.writeUInt16LE(20, 4); // version made by
    cdh.writeUInt16LE(20, 6); // version needed
    cdh.writeUInt16LE(0, 8);
    cdh.writeUInt16LE(METHOD_DEFLATE, 10);
    cdh.writeUInt16LE(DOS_TIME, 12);
    cdh.writeUInt16LE(DOS_DATE, 14);
    cdh.writeUInt32LE(crc, 16);
    cdh.writeUInt32LE(csize, 20);
    cdh.writeUInt32LE(usize, 24);
    cdh.writeUInt16LE(nameBuf.length, 28);
    cdh.writeUInt16LE(0, 30); // extra field length
    cdh.writeUInt16LE(0, 32); // comment length
    cdh.writeUInt16LE(0, 34); // disk number start
    cdh.writeUInt16LE(0, 36); // internal attributes
    cdh.writeUInt32LE(0, 38); // external attributes
    cdh.writeUInt32LE(offset, 42); // relative offset of local header
    centrals.push(cdh, nameBuf);

    offset += 30 + nameBuf.length + csize;
  }

  const centralStart = offset;
  const centralBuf = Buffer.concat(centrals);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(EOCD, 0);
  eocd.writeUInt16LE(0, 4); // disk number
  eocd.writeUInt16LE(0, 6); // central dir disk
  eocd.writeUInt16LE(parts.length, 8);
  eocd.writeUInt16LE(parts.length, 10);
  eocd.writeUInt32LE(centralBuf.length, 12);
  eocd.writeUInt32LE(centralStart, 16);
  eocd.writeUInt16LE(0, 20); // comment length
  return Buffer.concat([...locals, centralBuf, eocd]);
}
