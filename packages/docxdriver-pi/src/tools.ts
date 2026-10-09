import { Type } from 'typebox';
import type { ExtensionAPI } from '@earendil-works/pi-coding-agent';
import { ensureInit, runCreate, runFind, runHelp, runPlan, runPlanToml, runRead } from './engine.js';
import { createDocxBytes, readDocxBytes, resolveUnderCwd, SourceChangedError, writeDocxBytes } from './io.js';
import { fail, fromEngineResult, toolResult, type ToolTextResult } from './format.js';
import { readFile } from 'node:fs/promises';
import { generatedOperationSchema } from './schema.js';

const Path = Type.String({ description: 'Path to a .docx file, relative to cwd or absolute under cwd' });
const Kind = Type.Union([Type.Literal('document'), Type.Literal('styles'), Type.Literal('comments'), Type.Literal('revisions'), Type.Literal('assets')]);
const View = Type.Union([Type.Literal('markup'), Type.Literal('final'), Type.Literal('original')]);
const ChangeMode = Type.Union([Type.Literal('track'), Type.Literal('direct')]);
const PlanInline = Type.Object({ operations: Type.Array(generatedOperationSchema(), { minItems: 1 }), author: Type.Optional(Type.String()), change_mode: Type.Optional(ChangeMode) }, { additionalProperties: false });
const PlanFile = Type.Object({ file: Type.String() }, { additionalProperties: false });
const Plan = Type.Union([PlanInline, PlanFile]);
type Ctx = { cwd: string };

async function withFile(ctx: Ctx, path: string, fn: (x: { abs: string; bytes: Uint8Array; sha256: string }) => Promise<ToolTextResult> | ToolTextResult): Promise<ToolTextResult> {
  return fn(await readDocxBytes(ctx.cwd, path));
}

/** Register exactly the five plan/read Pi tools. */
export async function registerDocxTools(pi: ExtensionAPI): Promise<void> {
  await ensureInit();
  pi.registerTool({
    name: 'docx_create', label: 'DOCX create',
    description: 'Create a new DOCX. Refuses to overwrite an existing path.', promptSnippet: 'Create a DOCX safely',
    parameters: Type.Object({ path: Path, paragraphs: Type.Optional(Type.Array(Type.String())), html: Type.Optional(Type.String()) }, { additionalProperties: false }),
    async execute(_id, params, _signal, _update, ctx) {
      const p = params as any; if ((p.html === undefined) === (p.paragraphs === undefined)) fail('pass exactly one of paragraphs or html');
      const abs = await resolveUnderCwd(ctx.cwd, p.path); const result = runCreate(undefined, p.paragraphs, p.html);
      if (result.bytes === undefined) return fromEngineResult(p.path, result);
      await createDocxBytes(ctx.cwd, abs, result.bytes); return fromEngineResult(p.path, result);
    },
  });
  pi.registerTool({
    name: 'docx_read', label: 'DOCX read',
    description: 'Read a document, styles, comments, or revisions.', promptSnippet: 'Read a DOCX or typed listing',
    parameters: Type.Object({ path: Path, kind: Type.Optional(Kind), view: Type.Optional(View) }, { additionalProperties: false }),
    async execute(_id, params, _signal, _update, ctx) {
      const p = params as any; const kind = p.kind ?? 'document';
      const readArgs = kind === 'document'
        ? { kind, view: p.view ?? 'markup' }
        : { kind };
      return withFile(ctx, p.path, ({ bytes, sha256: source }) => fromEngineResult(p.path, runRead(bytes, readArgs), { source }));
    },
  });
  pi.registerTool({
    name: 'docx_find', label: 'DOCX find', description: 'Find text and return paragraph IDs with bounded context.', promptSnippet: 'Find text in a DOCX',
    parameters: Type.Object({ path: Path, query: Type.String(), ignore_case: Type.Optional(Type.Boolean()) }, { additionalProperties: false }),
    async execute(_id, params, _signal, _update, ctx) { const p = params as any; return withFile(ctx, p.path, ({ bytes }) => fromEngineResult(p.path, runFind(bytes, { query: p.query, ignore_case: p.ignore_case ?? false }))); },
  });
  pi.registerTool({
    name: 'docx_edit', label: 'DOCX edit', description: 'Preview or commit a typed Plan. Mutations are plan-only; preview_key is required to write.', promptSnippet: 'Preview or commit a DOCX plan',
    parameters: Type.Object({ path: Path, plan: Plan, preview_key: Type.Optional(Type.String()) }, { additionalProperties: false }),
    async execute(_id, params, _signal, _update, ctx) {
      const p = params as any;
      return withFile(ctx, p.path, async ({ abs, bytes, sha256: source }) => {
        const output = p.plan.file !== undefined
          ? runPlanToml(bytes, await readFile(await resolveUnderCwd(ctx.cwd, p.plan.file), 'utf8'), p.preview_key)
          : runPlan(bytes, { base: `sha256:${source}`, author: p.plan.author ?? 'docxdriver', change_mode: p.plan.change_mode ?? 'track', ops: p.plan.operations }, p.preview_key);
        if (output.outcome === 'committed' && output.bytes) {
          try { await writeDocxBytes(ctx.cwd, abs, output.bytes, source); } catch (e) { if (e instanceof SourceChangedError) return toolResult({ path: p.path, outcome: 'rejected', diagnostic: { code: e.code, message: e.message } }); throw e; }
        }
        return fromEngineResult(p.path, output, { source });
      });
    },
  });
  pi.registerTool({
    name: 'docx_help', label: 'DOCX help', description: 'Return the generated plan and read-surface reference.', promptSnippet: 'Show DOCX plan and read help',
    parameters: Type.Object({ topic: Type.Optional(Type.String()) }, { additionalProperties: false }),
    async execute(_id, params) { return fromEngineResult('', runHelp((params as any).topic)); },
  });
}
