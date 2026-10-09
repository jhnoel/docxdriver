// Identical neutral tool protocol for both projection versions. No built-in file tools.
import { Type } from '../packages/docxdriver-pi/node_modules/typebox/build/index.mjs';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';

export default async function (pi: any) {
  const enginePath = process.env.DOCXDRIVER_BENCH_ENGINE!;
  const engine = await import(pathToFileURL(resolve(enginePath, 'dist/index.js')).href);
  await engine.init(await readFile(resolve(enginePath, 'wasm/docxdriver_bg.wasm')));
  const equationField = process.env.DOCXDRIVER_BENCH_VARIANT === 'old' ? 'latex' : 'mathml';
  const at = Type.String({ description: 'Paragraph id from the document projection' });
  const select = Type.String();
  const occurrence = Type.Optional(Type.Integer({ minimum: 1 }));
  const operation = Type.Union([
    Type.Object({ op: Type.Literal('replace_text'), at, select, with: Type.String(), occurrence }, { additionalProperties: false }),
    Type.Object({ op: Type.Literal('format_text'), at, select, occurrence, bold: Type.Optional(Type.Boolean()), italic: Type.Optional(Type.Boolean()), underline: Type.Optional(Type.Boolean()) }, { additionalProperties: false }),
    Type.Object({ op: Type.Literal('replace_paragraph'), at, with: Type.String() }, { additionalProperties: false }),
    Type.Object({ op: Type.Literal('insert_paragraph'), at, position: Type.Union([Type.Literal('before'), Type.Literal('after')]), with: Type.String() }, { additionalProperties: false }),
    Type.Object({ op: Type.Literal('delete_paragraphs'), at: Type.Array(at, { minItems: 1 }) }, { additionalProperties: false }),
    Type.Object({ op: Type.Literal('replace_equation'), at, equation: Type.Optional(Type.Integer({ minimum: 1 })), [equationField]: Type.String() }, { additionalProperties: false }),
  ]);
  const receipts = new Map<string, any>();
  const file = (ctx: any) => resolve(ctx.cwd, 'document.docx');
  const respond = (value: any) => ({ content: [{ type: 'text', text: JSON.stringify(value) }], details: {} });
  const run = (bytes: Uint8Array | undefined, request: any) => {
    const { bytes: _bytes, ...result } = engine.executeRequest(bytes, request);
    return result;
  };
  pi.registerTool({
    name: 'docx_read', label: 'Read DOCX',
    description: 'Read document.docx. Returns source and the complete document projection in markup. Paragraph id attributes are addresses. Views: final, original, markup. Read before editing.',
    parameters: Type.Object({ view: Type.Optional(Type.Union([Type.Literal('final'), Type.Literal('original'), Type.Literal('markup')])) }),
    async execute(_id: string, args: any, _signal: any, _update: any, ctx: any) {
      const result = run(await readFile(file(ctx)), { Command: { command: { kind: 'read', view: args.view ?? 'final' } } });
      return respond(result.outcome === 'completed'
        ? { outcome: result.outcome, source: result.result.source, markup: result.result.markup }
        : result);
    },
  });
  pi.registerTool({
    name: 'docx_preview', label: 'Preview edit',
    description: 'Preview an atomic edit of document.docx; does not write. Pass base from docx_read source, and ops. Inspect report before committing. Supported ops: replace_text {at,select,with,occurrence?}; format_text {at,select,occurrence?,bold?,italic?,underline?}; replace_paragraph {at,with}; insert_paragraph {at,position:"before"|"after",with}; delete_paragraphs {at:[ids]}; replace_equation {at,equation?:1-based,' + equationField + ':string}. Text replacements use inline HTML. Equation replacement uses ' + (equationField === 'latex' ? 'LaTeX.' : 'presentation MathML with a math root.'),
    parameters: Type.Object({ base: Type.String(), ops: Type.Array(operation, { minItems: 1 }) }, { additionalProperties: false }),
    async execute(_id: string, args: any, _signal: any, _update: any, ctx: any) {
      const bytes = await readFile(file(ctx));
      const plan = { base: args.base, author: 'Benchmark', change_mode: 'direct', ops: args.ops };
      const result = run(bytes, { Plan: { plan } });
      if (result.outcome === 'previewed') receipts.set(result.preview_key, { plan, source: createHash('sha256').update(bytes).digest('hex') });
      return respond(result);
    },
  });
  pi.registerTool({
    name: 'docx_commit', label: 'Commit reviewed edit',
    description: 'In a later tool call, commit exactly the plan stored by a successful docx_preview using its preview_key. No plan resend. Source changes reject the commit.',
    parameters: Type.Object({ preview_key: Type.String() }),
    async execute(_id: string, args: any, _signal: any, _update: any, ctx: any) {
      const receipt = receipts.get(args.preview_key);
      if (!receipt) return respond({ outcome: 'rejected', diagnostic: { code: 'unknown_preview', message: 'Unknown preview key' } });
      const bytes = await readFile(file(ctx));
      const result = engine.executeRequest(bytes, { Plan: { plan: receipt.plan, preview_key: args.preview_key } });
      if (result.outcome === 'committed') {
        const current = await readFile(file(ctx));
        if (createHash('sha256').update(current).digest('hex') !== receipt.source) return respond({ outcome: 'rejected', diagnostic: { code: 'source_changed', message: 'Source changed before write' } });
        await writeFile(file(ctx), result.bytes);
        receipts.delete(args.preview_key);
      }
      const { bytes: _bytes, ...envelope } = result;
      return respond(envelope);
    },
  });
}
