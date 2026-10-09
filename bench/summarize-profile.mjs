#!/usr/bin/env node
// Summarize a Node --cpu-prof .cpuprofile into actionable top functions.
//
//   node bench/summarize-profile.mjs bench/profiles/isolate-*.cpuprofile
//
// Prints a text table (top frames by self time, plus wasm/JS-glue totals) and
// writes bench/results/profile.json with the machine-readable breakdown.
//
// Wasm frames appear as wasm-function[N] (the name section is stripped by
// wasm-opt). This script decodes the wasm export table and labels exported
// functions by their real name; unexported hot functions are grouped under
// "wasm:internal" with their index kept, and the export map is written to
// bench/results/wasm-exports.json so the indices can be resolved against the
// engine source.

import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const file = process.argv[2];
if (!file) {
  console.error('usage: node bench/summarize-profile.mjs <profile.cpuprofile>');
  process.exit(2);
}

const WASM_PATH = path.join(here, '../packages/docxdriver/wasm/docxdriver_bg.wasm');

// name → function index for every exported function (indexes match the
// wasm-function[N] labels in V8 profiles). Node's WebAssembly.Module.exports
// no longer exposes indices, so parse the binary export section (id 7).
function readWasmExports() {
  try {
    const bytes = readFileSync(WASM_PATH);
    const map = new Map();
    let p = 8; // skip magic + version
    const readU32 = () => {
      let result = 0;
      let shift = 0;
      while (true) {
        const b = bytes[p++];
        result |= (b & 0x7f) << shift;
        if ((b & 0x80) === 0) break;
        shift += 7;
      }
      return result >>> 0;
    };
    const readName = () => {
      const len = readU32();
      const s = bytes.subarray(p, p + len).toString('utf8');
      p += len;
      return s;
    };
    while (p < bytes.length) {
      const id = bytes[p++];
      const size = readU32();
      const end = p + size;
      if (id === 7) {
        const count = readU32();
        for (let i = 0; i < count; i++) {
          const name = readName();
          const kind = bytes[p++];
          const index = readU32();
          if (kind === 0) map.set(index, name); // kind 0 = function
        }
      }
      p = end;
    }
    return map;
  } catch {
    return new Map();
  }
}

const require = createRequire(import.meta.url);

function readFileSync(p) {
  // tiny sync read to keep module load simple
  return require('node:fs').readFileSync(p);
}

const prof = JSON.parse(await readFile(file, 'utf8'));
const nodes = new Map(prof.nodes.map((n) => [n.id, n]));
const totalUs = prof.timeDeltas.reduce((a, b) => a + b, 0);
const totalMs = totalUs / 1000;
const durationMs = (prof.endTime - prof.startTime) / 1000;
const exportsByIndex = readWasmExports();

// self time per node id (µs)
const selfUs = new Map();
prof.samples.forEach((id, i) => {
  selfUs.set(id, (selfUs.get(id) ?? 0) + prof.timeDeltas[i]);
});

// inclusive time via iterative DFS over the node tree
const inclusiveUs = new Map();
function computeInclusive(rootId) {
  const stack = [{ id: rootId, state: 0 }];
  while (stack.length) {
    const top = stack[stack.length - 1];
    const node = nodes.get(top.id);
    if (!node) {
      stack.pop();
      continue;
    }
    if (top.state === 0) {
      top.state = 1;
      for (const c of node.children ?? []) stack.push({ id: c, state: 0 });
    } else {
      let sum = selfUs.get(top.id) ?? 0;
      for (const c of node.children ?? []) sum += inclusiveUs.get(c) ?? 0;
      inclusiveUs.set(top.id, sum);
      stack.pop();
    }
  }
}
for (const n of prof.nodes) if (!n.parent) computeInclusive(n.id);

// Aggregate self time by canonical label + bucket (dedupes the same function
// reached from different call sites, which V8 records as separate node ids).
function classify(node) {
  const cf = node.callFrame ?? {};
  const fn = cf.functionName || '(anonymous)';
  const url = cf.url ?? '';
  if (url.startsWith('wasm://')) {
    const m = /wasm-function\[(\d+)\]/.exec(fn);
    const idx = m ? Number(m[1]) : null;
    const name = idx != null ? exportsByIndex.get(idx) : undefined;
    const label = name ? `wasm:${name}` : fn ? `${fn}` : '(wasm)';
    return { label, bucket: 'wasm', url, idx };
  }
  if (url.includes('/docxdriver.js') || url.includes('/dist/')) return { label: fn, bucket: 'js-glue', url, idx: null };
  if (!url) return { label: fn, bucket: 'v8', url, idx: null };
  return { label: fn, bucket: 'other', url, idx: null };
}

// For each node, the nearest ancestor (or self) that is an exported wasm
// function — tells the reader which engine entry point a hot internal frame
// belongs to (e.g. execute, docxsession_execute, renderHtml internals…).
const ownerByNode = new Map();

const agg = new Map(); // label -> {selfUs, incUs, count, bucket, label}
for (const [id, us] of selfUs) {
  const node = nodes.get(id);
  const c = classify(node);
  const key = `${c.bucket}\u0000${c.label}`;
  const e = agg.get(key) ?? { selfUs: 0, incUs: 0, count: 0, bucket: c.bucket, label: c.label };
  e.selfUs += us;
  e.incUs += inclusiveUs.get(id) ?? us;
  e.count += 1;
  agg.set(key, e);
}

const rows = [...agg.values()].sort((a, b) => b.selfUs - a.selfUs);
const totals = { wasm: 0, 'js-glue': 0, v8: 0, other: 0 };
for (const r of rows) totals[r.bucket] += r.selfUs;

const top = rows.slice(0, 25).map((r) => ({
  label: r.label,
  bucket: r.bucket,
  callSites: r.count,
  selfMs: +(r.selfUs / 1000).toFixed(2),
  selfPct: +(100 * r.selfUs / totalUs).toFixed(2),
  inclusiveMs: +(r.incUs / 1000).toFixed(2),
}));

// Exported wasm entry points (execute, docxsession_execute, …) by inclusive
// time: V8 cpuprof does not link wasm-internal frames to their callers, so
// this is the reliable way to see which engine call path dominates.
const exportedEntries = [];
for (const n of prof.nodes) {
  const c = classify(n);
  if (c.bucket === 'wasm' && c.label.startsWith('wasm:')) {
    exportedEntries.push({
      name: c.label.slice(5),
      index: c.idx,
      selfMs: +((selfUs.get(n.id) ?? 0) / 1000).toFixed(2),
      inclusiveMs: +((inclusiveUs.get(n.id) ?? 0) / 1000).toFixed(2),
      inclusivePct: +(100 * (inclusiveUs.get(n.id) ?? 0) / totalUs).toFixed(2),
    });
  }
}
exportedEntries.sort((a, b) => b.inclusiveMs - a.inclusiveMs);

const result = {
  profile: path.basename(file),
  totalMs: +totalMs.toFixed(2),
  durationMs: +durationMs.toFixed(2),
  samples: prof.samples.length,
  top,
  exportedEntries,
  totals: {
    wasmEngineSelfMs: +(totals.wasm / 1000).toFixed(2),
    wasmEngineSelfPct: +(100 * totals.wasm / totalUs).toFixed(2),
    jsGlueSelfMs: +(totals['js-glue'] / 1000).toFixed(2),
    jsGlueSelfPct: +(100 * totals['js-glue'] / totalUs).toFixed(2),
    v8InternalsSelfMs: +(totals.v8 / 1000).toFixed(2),
    v8InternalsSelfPct: +(100 * totals.v8 / totalUs).toFixed(2),
    otherSelfMs: +(totals.other / 1000).toFixed(2),
    otherSelfPct: +(100 * totals.other / totalUs).toFixed(2),
  },
  wasmExports: [...exportsByIndex.entries()].sort((a, b) => a[0] - b[0]).map(([idx, name]) => ({ idx, name })),
};

await mkdir(path.join(here, 'results'), { recursive: true });
await writeFile(path.join(here, 'results/profile.json'), JSON.stringify(result, null, 2));
await writeFile(path.join(here, 'results/wasm-exports.json'), JSON.stringify(Object.fromEntries([...exportsByIndex.entries()].map(([i, n]) => [i, n])), null, 2));

console.log(`profile ${path.basename(file)}: ${result.durationMs}ms wall, ${result.totalMs}ms sampled (${result.samples} samples)`);
console.log('');
console.log('self time by module:');
for (const [label, key] of [['wasm engine', 'wasmEngineSelfMs'], ['JS glue/shim', 'jsGlueSelfMs'], ['V8 internals', 'v8InternalsSelfMs'], ['other', 'otherSelfMs']]) {
  console.log(`  ${label.padEnd(14)} ${result.totals[key].toFixed(1).padStart(8)}ms  ${result.totals[key.replace('Ms', 'Pct')].toFixed(1).padStart(5)}%`);
}
console.log('');
console.log('');
console.log('exported wasm entry points by inclusive time:');
for (const e of exportedEntries) {
  console.log(`  ${e.name.padEnd(28)} ${e.inclusiveMs.toFixed(1).padStart(8)}ms inc ${e.inclusivePct.toFixed(1).padStart(5)}%  (wasm-function[${e.index}])`);
}
console.log('');
console.log('top frames by self time:');
for (const r of top) {
  console.log(`  ${r.label.slice(0, 62).padEnd(62)} ${r.selfMs.toFixed(1).padStart(8)}ms self ${r.selfPct.toFixed(1).padStart(5)}%  ${r.inclusiveMs.toFixed(1).padStart(8)}ms inc`);
}
