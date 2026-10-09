#!/usr/bin/env node
// Orchestrates the full docxdriver benchmark: rebuild wasm, generate fixtures,
// run every WASM phase, the native Rust control, and the CPU profile; writes
// machine-readable bench/results/results.json plus a human-readable
// bench/results/summary.md.
//
//   node bench/runner.mjs                    # full run
//   node bench/runner.mjs --skip-profile     # skip the --cpu-prof phase
//   node bench/runner.mjs --only warm        # just one phase
//   node bench/runner.mjs --tests            # also run cargo test + npm test
//
// Each phase is also runnable standalone (see bench/README.md).

import { spawnSync, execSync } from 'node:child_process';
import { readFile, writeFile, mkdir, readdir, stat } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { init, execute, DocxSession } from '../packages/docxdriver/dist/index.js';
import { FIXTURES_DIR, FIXTURE_SPECS, checkFixtures } from './gen-fixtures.mjs';
import { buildWork, batchLists, workJsonPath, marker, OPTIONS } from './lib/work.mjs';
import { WASM_PATH, RESULTS_DIR } from './wasm-bench.mjs';
import { summarize } from './lib/stats.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.dirname(here);
const PROFILES_DIR = path.join(here, 'profiles');
const PACKAGE_DIR = path.join(REPO, 'packages/docxdriver');

const args = process.argv.slice(2);
const only = args.includes('--only') ? args[args.indexOf('--only') + 1] : null;
const skipProfile = args.includes('--skip-profile');
const withTests = args.includes('--tests');

function run(cmd, opts = {}) {
  const { stdout, stderr, status } = spawnSync(cmd[0], cmd.slice(1), {
    cwd: opts.cwd ?? REPO,
    encoding: 'utf8',
    timeout: opts.timeout ?? 30 * 60_000,
    env: { ...process.env, ...(opts.env ?? {}) },
    maxBuffer: opts.maxBuffer ?? 64 * 1024 * 1024,
  });
  if (status !== 0) {
    console.error(`command failed (${cmd.join(' ')}):\n${(stderr || stdout).slice(0, 4000)}`);
    process.exit(1);
  }
  return stdout;
}

function sh(cmd, opts = {}) {
  const out = spawnSync(cmd, { cwd: opts.cwd ?? REPO, encoding: 'utf8', shell: true });
  return out.status === 0 ? out.stdout.trim() : '';
}

const log = (msg) => console.error(`\n== ${msg} ==`);

async function collectMeta() {
  const wasm = await readFile(WASM_PATH);
  return {
    timestamp: new Date().toISOString(),
    gitCommit: sh('git rev-parse HEAD'),
    gitDescribe: sh('git describe --tags --always --dirty'),
    os: `${os.platform()} ${os.release()}`,
    arch: os.arch(),
    cpu: os.cpus()[0]?.model ?? 'unknown',
    cpuCount: os.cpus().length,
    node: process.version,
    npm: sh('npm --version'),
    cargo: sh('cargo --version'),
    rustc: sh('rustc --version'),
    wasmPack: sh('wasm-pack --version'),
    benchmarkSubject: 'current worktree rebuilt by packages/docxdriver/scripts/build.sh',
    buildProfile:
      'workspace [profile.release]: opt-level=z, lto=true, codegen-units=1, strip=true, panic=abort; ' +
      'wasm: wasm-pack --target web --release + wasm-opt -O --enable-bulk-memory --enable-nontrapping-float-to-int; ' +
      'native references built with workspace opt-level=z and CARGO_PROFILE_RELEASE_OPT_LEVEL=3 (lto unchanged)',
    wasmFile: { bytes: wasm.length, sha256: createHash('sha256').update(wasm).digest('hex') },
    committedWasmFile: {
      bytes: Number(sh('git show HEAD:packages/docxdriver/wasm/docxdriver_bg.wasm | wc -c')),
      sha256: sh("git show HEAD:packages/docxdriver/wasm/docxdriver_bg.wasm | shasum -a 256 | awk '{print $1}'"),
    },
  };
}

// Rebuild the wasm + TS shim so the benchmark measures current core source.
async function buildPackage() {
  log('building packages/docxdriver (wasm-pack + tsc)');
  run(['npm', 'run', 'build'], { cwd: PACKAGE_DIR, timeout: 20 * 60_000 });
}

async function genFixtures() {
  log('generating fixtures');
  const { writeFixtures } = await import('./gen-fixtures.mjs');
  await writeFixtures();

  // Revision-heavy fixture: medium + 60 tracked edits through DocxSession.
  log('building medium-revised.docx (60 tracked edits)');
  await init(await readFile(WASM_PATH));
  const medium = await readFile(path.join(FIXTURES_DIR, 'medium.docx'));
  const targets = [];
  for (let i = 0; i < 20; i++) targets.push(marker(i * 20)); // mkr000000 … mkr000380 (20)
  for (let i = 5; i < 400; i += 10) targets.push(`aux${String(i).padStart(6, '0')}`); // 40 more
  const buildRevised = () => {
    const session = new DocxSession(medium);
    for (const t of targets) {
      const out = session.execute({
        type: 'editHtml',
        find: t,
        replace: `<del>${t}</del><ins>rev${t.slice(0, 3)}</ins>`,
        author: 'bench',
      }, { dryRun: false, now: '2025-01-01T00:00:00Z' });
      if (out.status !== 'ok') throw new Error(`revised-fixture edit ${t} failed: ${out.summary}`);
    }
    const bytes = session.bytes;
    session.free();
    return bytes;
  };
  const revised = buildRevised();
  const revisedCheck = buildRevised();
  if (Buffer.compare(Buffer.from(revised), Buffer.from(revisedCheck)) !== 0) {
    throw new Error('medium-revised.docx is not deterministic across identical builds');
  }
  await writeFile(path.join(FIXTURES_DIR, 'medium-revised.docx'), revised);

  await checkFixtures();
  await writeFile(
    path.join(FIXTURES_DIR, 'work.json'),
    JSON.stringify(buildWork(), null, 2),
  );
}

async function phase(name, wasmPhase, extraArgs = []) {
  log(name);
  const out = run([process.execPath, '--expose-gc', path.join(here, 'wasm-bench.mjs'), wasmPhase, ...extraArgs], {
    cwd: REPO,
  });
  return JSON.parse(out);
}

async function nativeBench() {
  // Two native builds so the wasm/native gap can be attributed: the workspace
  // release profile (opt-level z — the exact flags the wasm core is built with)
  // and an opt-level 3 build (closer to "same code at full native speed").
  log('building native Rust control (bench_native example, opt-level z)');
  run(
    ['cargo', 'build', '--release', '-p', 'docxdriver-core', '--example', 'bench_native'],
    { timeout: 20 * 60_000 },
  );
  const bin = path.join(REPO, 'target/release/examples/bench_native');
  const runNative = () => JSON.parse(run([bin, FIXTURES_DIR, workJsonPath()]));
  const z = await aggregateNative(runNative());
  log('running native Rust control (opt-level z)');

  log('building native Rust control (bench_native example, opt-level 3)');
  run(
    ['cargo', 'build', '--release', '-p', 'docxdriver-core', '--example', 'bench_native'],
    { env: { CARGO_PROFILE_RELEASE_OPT_LEVEL: '3' }, timeout: 20 * 60_000 },
  );
  log('running native Rust control (opt-level 3)');
  const opt3 = await aggregateNative(runNative());
  return { optZ: z, opt3 };
}

async function aggregateNative(rows) {
  return rows.map((r) => ({
    fixture: r.fixture,
    name: r.name,
    stats: summarize(r.timesMs),
    verified: r.verified,
  }));
}

async function profilePhase() {
  log('CPU profile (--cpu-prof on large workload)');
  await mkdir(PROFILES_DIR, { recursive: true });
  run([
    process.execPath,
    '--cpu-prof',
    `--cpu-prof-dir=${PROFILES_DIR}`,
    path.join(here, 'profile-run.mjs'),
  ], { cwd: REPO, timeout: 15 * 60_000 });
  const files = (await readdir(PROFILES_DIR)).filter((f) => f.endsWith('.cpuprofile'));
  const withMtime = await Promise.all(
    files.map(async (f) => ({ f, m: (await stat(path.join(PROFILES_DIR, f))).mtimeMs })),
  );
  withMtime.sort((a, b) => b.m - a.m);
  const latest = withMtime[0]?.f;
  if (!latest) throw new Error('no cpuprofile produced');
  log(`summarizing ${latest}`);
  const summaryOut = run([process.execPath, path.join(here, 'summarize-profile.mjs'), path.join(PROFILES_DIR, latest)]);
  console.error(summaryOut);
  return JSON.parse(await readFile(path.join(RESULTS_DIR, 'profile.json'), 'utf8'));
}

async function fixtureRows() {
  const rows = [];
  const metaJson = JSON.parse(await readFile(path.join(FIXTURES_DIR, 'fixtures.json'), 'utf8'));
  for (const name of Object.keys(FIXTURE_SPECS)) {
    const meta = metaJson.fixtures[name];
    rows.push({ name, ...meta });
  }
  const revised = await readFile(path.join(FIXTURES_DIR, 'medium-revised.docx'));
  rows.push({
    name: 'medium-revised',
    file: 'medium-revised.docx',
    fileBytes: revised.length,
    sha256: createHash('sha256').update(revised).digest('hex'),
    note: 'medium + 60 tracked editHtml operations',
  });
  return rows;
}

function fmt(stat) {
  return `${stat.median.toFixed(2)}ms (min ${stat.min.toFixed(2)}, p95 ${stat.p95.toFixed(2)}, max ${stat.max.toFixed(2)})`;
}

async function main() {
  await mkdir(RESULTS_DIR, { recursive: true });
  if (!only) {
    await buildPackage();
  }

  // Capture metadata after a build so the hash names the bytes benchmarked.
  const meta = await collectMeta();
  console.error(`benchmarking ${meta.gitCommit} on ${meta.os} / ${meta.cpu}`);

  if (!only) {
    await genFixtures();
  } else if (only === 'fixtures') {
    await genFixtures();
    console.log(JSON.stringify({ meta, fixtures: await fixtureRows() }, null, 2));
    return;
  }

  const results = { meta, fixtures: [] };
  if (only) {
    const phaseMap = {
      validate: () => phase('validate', 'validate'),
      cold: () => phase('cold', 'cold'),
      warm: () => phase('warm', 'warm'),
      batch: () => phase('batch', 'batch'),
      memory: () => phase('memory', 'memory'),
      native: nativeBench,
      profile: profilePhase,
    };
    const fn = phaseMap[only];
    if (!fn) { console.error(`unknown --only phase: ${only}`); process.exit(2); }
    results[only] = await fn();
    console.log(JSON.stringify(results, null, 2));
    return;
  }

  results.fixtures = await fixtureRows();
  results.validate = await phase('validate fixtures via engine', 'validate');
  results.cold = await phase('cold wasm init (fresh processes)', 'cold');
  if (results.cold.moduleSha !== meta.wasmFile.sha256) {
    throw new Error('cold phase benchmarked a different WASM module than metadata records');
  }
  results.warm = await phase('warm execute latency', 'warm');
  results.batch = await phase('sequential vs native batch', 'batch');
  results.memory = await phase('memory usage + high-water', 'memory');
  results.native = await nativeBench();
  results.profile = skipProfile ? null : await profilePhase();

  if (withTests) {
    log('running existing tests (cargo test + npm test)');
    const capture = (cmd, cwd) => {
      const res = spawnSync(cmd, { cwd, encoding: 'utf8', shell: true, timeout: 30 * 60_000, maxBuffer: 64 * 1024 * 1024 });
      if (res.status !== 0) {
        console.error(`tests failed (${cmd}):\n${(res.stderr || res.stdout || '').slice(0, 4000)}`);
        process.exit(1);
      }
      return (res.stdout || '').trim().split('\n').slice(-8).join('\n');
    };
    results.tests = {
      cargo: capture('cargo test --workspace 2>&1', REPO),
      npm: capture('npm test 2>&1', PACKAGE_DIR),
    };
  }

  await writeFile(path.join(RESULTS_DIR, 'results.json'), JSON.stringify(results, null, 2));

  // ---- human-readable summary ----
  const L = [];
  L.push(`# docxdriver benchmark summary`);
  L.push('');
  L.push(`- commit: ${meta.gitCommit} (${meta.gitDescribe})`);
  L.push(`- machine: ${meta.os} / ${meta.arch} / ${meta.cpu} (${meta.cpuCount} cores)`);
  L.push(`- node ${meta.node}, ${meta.cargo}, ${meta.rustc}, ${meta.wasmPack}`);
  L.push(`- wasm module: ${meta.wasmFile.bytes} bytes, sha256 ${meta.wasmFile.sha256.slice(0, 16)}…`);
  if (meta.committedWasmFile.sha256 !== meta.wasmFile.sha256) {
    L.push(`- committed wasm differs: ${meta.committedWasmFile.bytes} bytes, sha256 ${meta.committedWasmFile.sha256.slice(0, 16)}…`);
  }
  L.push(`- subject: ${meta.benchmarkSubject}`);
  L.push(`- build: ${meta.buildProfile}`);
  L.push('');
  L.push('## fixtures');
  for (const f of results.fixtures) {
    L.push(`- ${f.name}: ${f.fileBytes} bytes, ${f.paragraphs ?? '-'} paragraphs, ~${f.chars ?? '-'} chars${f.note ? ` (${f.note})` : ''}`);
  }
  L.push('');
  L.push('## cold wasm init (fresh process, per sample)');
  L.push(`- file read: ${fmt(results.cold.readMs)}`);
  L.push(`- compile+instantiate: ${fmt(results.cold.initMs)}`);
  L.push(`- first execute: ${fmt(results.cold.firstExecMs)}`);
  L.push(`- first small-DOCX read command: ${fmt(results.cold.firstDocxExecMs)}`);
  L.push(`- parent-observed child spawn-to-exit: ${fmt(results.cold.processElapsedMs)}`);
  L.push(`- node process baseline (spawn + exit): ${fmt(results.cold.nodeBaselineSpawnMs)}`);
  L.push('');
  L.push('## warm execute latency (median of samples, pristine fixture per mutation iteration)');
  L.push('');
  L.push('| fixture | command | median | min | p95 | max | samples |');
  L.push('|---|---|---|---|---|---|---|');
  for (const r of results.warm) {
    L.push(`| ${r.fixture} | ${r.name} | ${r.stats.median.toFixed(2)}ms | ${r.stats.min.toFixed(2)} | ${r.stats.p95.toFixed(2)} | ${r.stats.max.toFixed(2)} | ${r.stats.n} |`);
  }
  L.push('');
  L.push('## sequential vs native batch (equivalent command lists, validated outcomes)');
  L.push('');
  L.push('| list | ops | sequential median | batch median | speedup |');
  L.push('|---|---|---|---|---|');
  for (const r of results.batch) {
    L.push(`| ${r.label} | ${r.ops} | ${r.sequentialMs.median.toFixed(2)}ms | ${r.batchMs.median.toFixed(2)}ms | ${r.speedup.toFixed(1)}x |`);
  }
  L.push('');
  L.push('## memory (--expose-gc, bytes)');
  const mem = results.memory;
  L.push(`- init: ${mem.initMs.toFixed(1)}ms`);
  L.push(`- rss growth: init ${(mem.growth.rssInit / 1e6).toFixed(1)}MB, warm ${(mem.growth.rssWarm / 1e6).toFixed(1)}MB, high-water ${(mem.growth.rssHighWater / 1e6).toFixed(1)}MB`);
  L.push(`- external growth (wasm memory + buffers): init ${(mem.growth.externalInit / 1e6).toFixed(1)}MB, high-water ${(mem.growth.externalHighWater / 1e6).toFixed(1)}MB`);
  L.push(`- high-water peak: rss ${(mem.highWaterPeak.rss / 1e6).toFixed(1)}MB, external ${(mem.highWaterPeak.external / 1e6).toFixed(1)}MB, arrayBuffers ${(mem.highWaterPeak.arrayBuffers / 1e6).toFixed(1)}MB`);
  L.push(`- after releasing retained outputs + GC: rss ${(mem.afterRelease.rss / 1e6).toFixed(1)}MB, external ${(mem.afterRelease.external / 1e6).toFixed(1)}MB`);
  L.push('');
  L.push('## native Rust-core references (same bytes + command JSON, no wasm/glue)');
  L.push('');
  L.push('| fixture | command | wasm median | native opt-z | native opt-3 | wasm/opt-z | wasm/opt-3 |');
  L.push('|---|---|---|---|---|---|---|');
  const nativeByNameZ = new Map(results.native.optZ.map((r) => [`${r.fixture}|${r.name}`, r]));
  const nativeByName3 = new Map(results.native.opt3.map((r) => [`${r.fixture}|${r.name}`, r]));
  for (const w of results.warm) {
    const nz = nativeByNameZ.get(`${w.fixture}|${w.name}`);
    const n3 = nativeByName3.get(`${w.fixture}|${w.name}`);
    if (!nz || !n3) continue;
    const rz = w.stats.median / nz.stats.median;
    const r3 = w.stats.median / n3.stats.median;
    L.push(`| ${w.fixture} | ${w.name} | ${w.stats.median.toFixed(3)}ms | ${nz.stats.median.toFixed(3)}ms | ${n3.stats.median.toFixed(3)}ms | ${rz.toFixed(1)}x | ${r3.toFixed(1)}x |`);
  }
  L.push('');
  if (results.profile) {
    L.push('## CPU profile (large workload, renderHtml + editHtml + read loop)');
    L.push(`- ${results.profile.durationMs}ms wall, ${results.profile.totalMs}ms sampled`);
    L.push(`- wasm engine self: ${results.profile.totals.wasmEngineSelfPct}% — JS glue/shim self: ${results.profile.totals.jsGlueSelfPct}% — V8 internals self: ${results.profile.totals.v8InternalsSelfPct}%`);
    L.push('');
    L.push('exported wasm entry points by inclusive time:');
    for (const e of results.profile.exportedEntries) {
      L.push(`- ${e.name}: ${e.inclusiveMs}ms (${e.inclusivePct}%)`);
    }
    L.push('');
    L.push('top frames by self time:');
    for (const t of results.profile.top.slice(0, 12)) {
      L.push(`- ${t.label}: ${t.selfMs}ms (${t.selfPct}%)`);
    }
  }
  L.push('');
  L.push('Full machine-readable data: results.json. Methodology and commands: bench/README.md.');
  await writeFile(path.join(RESULTS_DIR, 'summary.md'), L.join('\n'));
  console.log(L.join('\n'));
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
