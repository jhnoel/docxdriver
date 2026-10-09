// Archive isolation guard (Task 0 of the typed-plan Python REPL hardening
// plan): production code must never import from the experimental V2/V3
// archive, and a clean build must never emit deprecated modules into dist.
//
// The guard covers the V1 REPL entry point, the shared runtime/host, the
// default Pi extension, and the package exports entry (dist/index.js). It
// walks the static import/export graph of every emitted production module
// and fails if any specifier resolves inside
// experimental/python-repl-surfaces-v2-v3/.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile, readdir, stat } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const PACKAGE_ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const REPO_ROOT = resolve(join(PACKAGE_ROOT, '..', '..'));
const ARCHIVE_ROOT = resolve(join(REPO_ROOT, 'experimental', 'python-repl-surfaces-v2-v3'));
const DIST = join(PACKAGE_ROOT, 'dist');
const SRC = join(PACKAGE_ROOT, 'src');
const WASM = join(PACKAGE_ROOT, 'wasm');
const SCHEMA = join(PACKAGE_ROOT, 'schema');

/** Deprecated V2/V3 modules that must never be emitted or imported. */
const ARCHIVED_MODULES = [
  'python-string-extension',
  'python-string-prelude',
  'python-string-deriver',
  'python-model-extension',
  'python-model-prelude',
  'python-model-deriver',
  'projection-diff',
  'projection',
];

/** Production entry points: the default extension (package exports), the V1
 * REPL entry point, and the shared runtime/host/deriver/prelude modules. */
const PRODUCTION_ENTRIES = [
  'index.js',
  'python-plan-extension.js',
  'python-repl.js',
  'python-host.js',
  'python-plan-deriver.js',
  'python-plan-prelude.js',
  'tools.js',
  'engine.js',
  'io.js',
  'schema.js',
  'format.js',
];

const SPECIFIER_RE = /(?:^|[^.\w])(?:import|export)\s*(?:[\w$*{},\s]+from\s*)?["']([^"']+)["']/g;

/** All static import/export specifiers in one module's source. */
function specifiersOf(source) {
  const out = [];
  for (const match of source.matchAll(SPECIFIER_RE)) out.push(match[1]);
  return out;
}

/** The resolved file path for a relative specifier, or null for bare/absolute
 * specifiers (node builtins, node_modules, package exports). */
function resolveRelative(fromFile, specifier) {
  if (specifier.startsWith('.') || specifier.startsWith('/')) {
    try {
      return fileURLToPath(new URL(specifier, pathToFileURL(fromFile)));
    } catch {
      return null;
    }
  }
  return null;
}

/** Walk the emitted module graph from `entry`, returning every module file
 * visited plus the resolved path of every relative specifier. */
async function walkGraph(entry) {
  const queue = [join(DIST, entry)];
  const visited = new Set();
  const edges = new Set(); // `${from} -> ${to}`
  while (queue.length > 0) {
    const file = queue.pop();
    if (visited.has(file)) continue;
    visited.add(file);
    const source = await readFile(file, 'utf8');
    for (const specifier of specifiersOf(source)) {
      const resolved = resolveRelative(file, specifier);
      if (resolved === null) continue;
      edges.add(`${file} -> ${resolved}`);
      if (resolved.endsWith('.js') && !visited.has(resolved)) queue.push(resolved);
    }
  }
  return { visited, edges };
}

function inside(root, target) {
  return target === root || target.startsWith(root + '/');
}

test('the experimental V2/V3 archive exists outside the production package', async () => {
  const archiveStat = await stat(ARCHIVE_ROOT);
  assert.ok(archiveStat.isDirectory(), `archive must exist at ${ARCHIVE_ROOT}`);
  const rel = resolve(join(PACKAGE_ROOT, '..'));
  assert.ok(
    !inside(rel, ARCHIVE_ROOT),
    'the archive must live outside packages/docxdriver-pi so builds and tests cannot reach it by accident',
  );
  // Package files allowlist must not include the archive.
  const pkg = JSON.parse(await readFile(join(PACKAGE_ROOT, 'package.json'), 'utf8'));
  assert.ok(!(pkg.files ?? []).some((entry) => entry.includes('experimental')), 'package files must not list the archive');
});

test('the package vendors its core WASM and schema without a sibling docxdriver package', async () => {
  const pkg = JSON.parse(await readFile(join(PACKAGE_ROOT, 'package.json'), 'utf8'));
  assert.ok((pkg.files ?? []).includes('wasm'), 'packed package must include its local WASM loader and binary');
  assert.ok((pkg.files ?? []).includes('schema'), 'packed package must include its generated operation schema');
  assert.equal(pkg.dependencies?.docxdriver, undefined, 'the extension must not require file:../docxdriver');
  for (const asset of ['docxdriver.js', 'docxdriver.d.ts', 'docxdriver_bg.wasm', 'docxdriver_bg.wasm.d.ts']) {
    assert.ok((await stat(join(WASM, asset))).isFile(), `missing vendored WASM asset ${asset}`);
  }
  assert.ok((await stat(join(SCHEMA, 'typed-request.schema.json'))).isFile(), 'missing vendored typed request schema');
  const engine = await readFile(join(SRC, 'engine.ts'), 'utf8');
  const schema = await readFile(join(SRC, 'schema.ts'), 'utf8');
  assert.ok(!engine.includes("from 'docxdriver'"), 'engine must use the local WASM loader');
  assert.ok(!schema.includes('docxdriver/schema/'), 'schema reader must use the local schema copy');
});

test('vendored core artifacts match the checked-in core build', async () => {
  const core = join(REPO_ROOT, 'packages', 'docxdriver');
  for (const asset of ['docxdriver.js', 'docxdriver.d.ts', 'docxdriver_bg.wasm', 'docxdriver_bg.wasm.d.ts']) {
    assert.deepEqual(
      await readFile(join(WASM, asset)),
      await readFile(join(core, 'wasm', asset)),
      `vendored ${asset} is stale; copy the regenerated core artifact before release`,
    );
  }
  assert.deepEqual(
    await readFile(join(SCHEMA, 'typed-request.schema.json')),
    await readFile(join(core, 'schema', 'typed-request.schema.json')),
    'vendored typed request schema is stale; copy the regenerated core schema before release',
  );
});

test('a clean build emits no deprecated modules and keeps every production entry', async () => {
  let entries;
  try {
    entries = await readdir(DIST);
  } catch {
    assert.fail(`dist/ is missing — run "npm run build" before the tests (npm test does this)`);
  }
  const emitted = new Set(entries.filter((name) => name.endsWith('.js')));
  for (const archived of ARCHIVED_MODULES) {
    assert.ok(
      !emitted.has(`${archived}.js`),
      `deprecated emitted file dist/${archived}.js must not exist — the build must clean dist/ first`,
    );
  }
  for (const entry of PRODUCTION_ENTRIES) {
    assert.ok(emitted.has(entry), `production entry dist/${entry} must be emitted`);
  }
});

test('no production module imports from the experimental archive', async () => {
  // Walk every emitted production entry (covers the V1 entry point, shared
  // runtime/host, default extension, and package exports).
  const sources = new Set();
  const edges = new Set();
  for (const entry of PRODUCTION_ENTRIES) {
    const { visited, edges: graph } = await walkGraph(entry);
    for (const file of visited) sources.add(file);
    for (const edge of graph) edges.add(edge);
  }
  // Also scan the TypeScript sources directly so a future tsconfig include
  // cannot smuggle archive files past the emitted-graph walk.
  for (const name of await readdir(SRC)) {
    if (!name.endsWith('.ts')) continue;
    sources.add(join(SRC, name));
    const source = await readFile(join(SRC, name), 'utf8');
    for (const specifier of specifiersOf(source)) {
      const resolved = resolveRelative(join(SRC, name), specifier);
      if (resolved !== null) edges.add(`${join(SRC, name)} -> ${resolved}`);
    }
  }

  const violations = [...edges].filter((edge) => {
    const target = edge.split(' -> ')[1];
    return inside(ARCHIVE_ROOT, target);
  });
  assert.deepEqual(violations, [], `production imports must never resolve into the experimental archive`);

  // Belt and braces: no emitted or source file may even mention the archive
  // path, and no archived module name may be imported anywhere in production.
  for (const file of sources) {
    const text = await readFile(file, 'utf8');
    assert.ok(
      !text.includes('experimental/python-repl-surfaces-v2-v3'),
      `${file} references the experimental archive`,
    );
    for (const archived of ARCHIVED_MODULES) {
      assert.ok(
        !new RegExp(`['"].*${archived}['"]`).test(text),
        `${file} imports archived module "${archived}"`,
      );
    }
  }
});

/** Deprecated V2/V3 surface machinery that must never appear in production
 * source. `Paragraph(` is allowed only in the schema-generated operation
 * doc strings (FormatParagraph(at: / InsertParagraph(at: / ReplaceParagraph(at:). */
const FORBIDDEN_SYMBOLS = [
  'lcsTokens', 'diffProjections', 'parseProjection', 'SurfaceDeriver',
  'applyTextChange', 'applyFmtChange', 'Document(', 'Selection(',
];
const GENERATED_PARAGRAPH_DOC = /(?:Format|Insert|Replace)Paragraph\(at:/;

test('production src contains no deprecated projection/structured-model symbols', async () => {
  for (const name of await readdir(SRC)) {
    if (!name.endsWith('.ts')) continue;
    const text = await readFile(join(SRC, name), 'utf8');
    for (const symbol of FORBIDDEN_SYMBOLS) {
      assert.ok(!text.includes(symbol), `${name} contains deprecated symbol "${symbol}"`);
    }
    for (const line of text.split('\n')) {
      if (line.includes('Paragraph(') && !GENERATED_PARAGRAPH_DOC.test(line)) {
        assert.fail(`${name} contains a non-generated Paragraph( use: ${line.trim()}`);
      }
    }
  }
});
