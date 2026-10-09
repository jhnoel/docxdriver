import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** The checked-in artifact generated from docxdriver-core::api::EditOp. */
export function generatedOperationSchema(): any {
  const here = dirname(fileURLToPath(import.meta.url));
  const candidates = process.env.DOCXDRIVER_SCHEMA_PATH
    ? [resolve(process.env.DOCXDRIVER_SCHEMA_PATH)]
    : [join(here, '../schema/typed-request.schema.json')];
  let schema: any;
  let lastError: unknown;
  let path = candidates[0];
  for (const candidate of candidates) {
    path = candidate;
    try { schema = JSON.parse(readFileSync(candidate, 'utf8')); break; }
    catch (error) { lastError = error; }
  }
  if (!schema) throw new Error(`generated docxdriver schema is unavailable at ${path}: ${(lastError as Error)?.message ?? 'unknown error'}`);
  const op = schema?.$defs?.editOp;
  if (!op || !Array.isArray(op.oneOf) || op.oneOf.length === 0 || op.oneOf.some((variant: any) =>
    variant?.type !== 'object' || variant.additionalProperties !== false || !variant.properties?.op || !Array.isArray(variant.required) || !variant.required.includes('op') ||
    (!Array.isArray(variant.properties.op.enum) && typeof variant.properties.op.const !== 'string')
  )) {
    throw new Error(`generated docxdriver schema at ${path} has no stable strict $defs.editOp oneOf union`);
  }
  return op;
}

export function generatedSchemaPath(): string {
  const here = dirname(fileURLToPath(import.meta.url));
  if (process.env.DOCXDRIVER_SCHEMA_PATH) return resolve(process.env.DOCXDRIVER_SCHEMA_PATH);
  return join(here, '../schema/typed-request.schema.json');
}
