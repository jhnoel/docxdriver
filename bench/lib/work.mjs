// The benchmark command list — the single source of truth shared by the WASM
// bench (wasm-bench.mjs), the runner, and the native Rust control
// (crates/docxdriver-core/examples/bench_native.rs reads the same work.json).
//
// Each entry: { fixture, name, command (DocxCommand JSON), options (JSON
// string, explicit now so revision dates don't drift between runs), iterations,
// warmups, verify }.
//
// Mutation commands are stateless-ABI benchmarks: every timed iteration starts
// from the pristine fixture bytes, so iterations measure equivalent inputs —
// never a progressively mutated document (the batch phase covers chains).

import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { FIXTURES_DIR } from '../gen-fixtures.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));

export const OPTIONS = JSON.stringify({ dryRun: false, now: '2025-01-01T00:00:00Z' });

// marker word for paragraph index i (0-based), as embedded by gen-fixtures.
export function marker(i) {
  return `mkr${String(i).padStart(6, '0')}`;
}

// Deterministic paragraph numbers that exist on every fixture of each size.
const PARA_NUMBERS = { small: 3, medium: 7, large: 11 };

// iteration counts per (fixture, op kind) — read-only ops are cheap enough to
// repeat more; renderHtml/editHtml on large docs are the heavy ones.
const ITERATIONS = {
  small: { read: 200, findText: 200, wordCount: 200, outline: 200, renderHtml: 100, editHtml: 100, addComment: 100, setParagraphFormat: 150, setEvenAndOddHeaders: 100 },
  medium: { read: 60, findText: 60, wordCount: 100, outline: 100, renderHtml: 30, editHtml: 30, addComment: 40, setParagraphFormat: 60, setEvenAndOddHeaders: 40, listRevisions: 60, readAll: 40, acceptAllRevisions: 40 },
    large: { read: 30, findText: 30, wordCount: 40, outline: 40, renderHtml: 30, editHtml: 20, addComment: 30, setParagraphFormat: 30, setEvenAndOddHeaders: 30 },
};
const WARMUPS = 4;

function cmd(spec, { fixture, name, command, iterations, warmups = WARMUPS, verify = null, fixtureFile = `${fixture}.docx` }) {
  spec.commands.push({ fixture: fixtureFile, name, command, options: OPTIONS, iterations, warmups, verify });
}

export function buildWork() {
  const spec = { commands: [] };

  for (const fixture of ['small', 'medium', 'large']) {
    const it = ITERATIONS[fixture];
    // Marker index 20 exists on every fixture size (markers every 20th paragraph).
    const m = marker(20);
    const para = PARA_NUMBERS[fixture];

    cmd(spec, { fixture, name: 'read', command: { type: 'read', view: 'current' }, iterations: it.read });
    cmd(spec, { fixture, name: 'findText', command: { type: 'findText', query: m, ignoreCase: true }, iterations: it.findText });
    cmd(spec, { fixture, name: 'wordCount', command: { type: 'wordCount' }, iterations: it.wordCount });
    cmd(spec, { fixture, name: 'outline', command: { type: 'outline' }, iterations: it.outline });
    cmd(spec, { fixture, name: 'renderHtml-markup', command: { type: 'renderHtml', view: 'markup' }, iterations: it.renderHtml });
    cmd(spec, {
      fixture, name: 'editHtml-tracked',
      command: { type: 'editHtml', find: m, replace: `<del>${m}</del><ins>replacement${fixture}</ins>`, author: 'bench' },
      iterations: it.editHtml,
      verify: { kind: 'editHtml', marker: m, replacement: `replacement${fixture}` },
    });
    cmd(spec, {
      fixture, name: 'addComment',
      command: { type: 'addComment', anchor: m, text: 'benchmark note', author: 'bench' },
      iterations: it.addComment,
      verify: { kind: 'addComment' },
    });
    cmd(spec, {
      fixture, name: 'setParagraphFormat',
      command: { type: 'setParagraphFormat', para, alignment: 'center', spacingAfter: 120 },
      iterations: it.setParagraphFormat,
      verify: { kind: 'setParagraphFormat', para },
    });
    cmd(spec, {
      fixture, name: 'setEvenAndOddHeaders',
      command: { type: 'setEvenAndOddHeaders', evenAndOdd: true },
      iterations: it.setEvenAndOddHeaders,
      verify: { kind: 'mutating' },
    });
  }

  // Revision-heavy fixture (medium + 60 tracked edits, see runner build).
  cmd(spec, {
    fixture: 'medium-revised', name: 'listRevisions',
    command: { type: 'listRevisions' }, iterations: ITERATIONS.medium.listRevisions,
  });
  cmd(spec, {
    fixture: 'medium-revised', name: 'read-view-all',
    command: { type: 'read', view: 'all' }, iterations: ITERATIONS.medium.readAll,
  });
  cmd(spec, {
    fixture: 'medium-revised', name: 'acceptAllRevisions',
    command: { type: 'acceptAllRevisions' }, iterations: ITERATIONS.medium.acceptAllRevisions,
    verify: { kind: 'acceptAllRevisions', revisions: 60 },
  });

  return spec;
}

export function workJsonPath() {
  return path.join(FIXTURES_DIR, 'work.json');
}

// The batch-phase command lists (sequential vs native batch equivalence).
export function batchLists() {
  const m20 = marker(20);
  const m40 = marker(40);
  return [
    {
      label: 'medium-mixed-8',
      fixture: 'medium.docx',
      commands: [
        { type: 'editHtml', find: m20, replace: `<del>${m20}</del><ins>batchA</ins>`, author: 'bench' },
        { type: 'findText', query: m20, ignoreCase: true },
        { type: 'wordCount' },
        { type: 'addComment', anchor: m40, text: 'batch comment', author: 'bench' },
        { type: 'setParagraphFormat', para: 7, alignment: 'center' },
        { type: 'renderHtml', view: 'markup' },
        { type: 'editHtml', find: m40, replace: `<del>${m40}</del><ins>batchB</ins>`, author: 'bench' },
        { type: 'read', view: 'current' },
      ],
    },
    {
      label: 'large-readonly-96',
      fixture: 'large.docx',
      commands: Array.from({ length: 24 }, (_, i) => [
        { type: 'findText', query: marker(20), ignoreCase: true },
        { type: 'wordCount' },
        { type: 'outline' },
        { type: 'read', view: 'current' },
      ]).flat(),
    },
  ];
}
