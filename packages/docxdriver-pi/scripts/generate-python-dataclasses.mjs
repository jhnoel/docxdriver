#!/usr/bin/env node
// Generates the three generated regions of the Version 1 Python surface —
// operation dataclasses, Monty type stubs, and the concise operation
// reference — from the checked-in core schema ($defs.editOp.oneOf) and
// splices them into src/python-plan-prelude.ts between the marker pairs:
//
//   # BEGIN/END GENERATED DATACLASSES
//   # BEGIN/END GENERATED TYPE STUBS
//   # BEGIN/END GENERATED OPERATION DOCS
//
// Operations are selected by scripts/plan-capability-manifest.mjs; the
// generator never redefines field shapes, types, enums, or requiredness —
// every field list, annotation, and default derives from the schema, and
// Python keyword collisions are handled generically through the keyword set
// (schema field `with` becomes constructor `with_`, mapped back to the
// schema key in to_dict). Idempotent: regenerating an already-generated file
// is byte-identical.
//
// Optional argv[1] overrides the target file (the drift test regenerates
// into a temp copy and compares). Optional DOCXDRIVER_SCHEMA_PATH overrides the
// schema location (the drift tests feed a modified copy).
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { PLAN_CAPABILITIES } from './plan-capability-manifest.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const SCHEMA_PATH = process.env.DOCXDRIVER_SCHEMA_PATH
  ? resolve(process.env.DOCXDRIVER_SCHEMA_PATH)
  : resolve(here, '../../../packages/docxdriver/schema/typed-request.schema.json');
const DEFAULT_TARGET = resolve(here, '../src/python-plan-prelude.ts');
const target = process.argv[2] ? resolve(process.argv[2]) : DEFAULT_TARGET;

/** Python keywords: a schema field colliding with one gets a trailing `_`. */
const PYTHON_KEYWORDS = new Set([
  'False', 'None', 'True', 'and', 'as', 'assert', 'async', 'await', 'break',
  'case', 'class', 'continue', 'def', 'del', 'elif', 'else', 'except',
  'finally', 'for', 'from', 'global', 'if', 'import', 'in', 'is', 'lambda',
  'match', 'nonlocal', 'not', 'or', 'pass', 'raise', 'return', 'try',
  'while', 'with', 'yield',
]);

/**
 * Schema $ref -> Python annotation for every $def reachable from the
 * selected operations. An unknown $ref fails generation (drift surfaces the
 * schema change instead of guessing a Python type).
 */
const REF_TYPES = {
  ParagraphAddress: 'str',
  InsertAnchor: 'str',
  ChromeKind: 'str',
  InsertPosition: 'str',
  ChangeMode: 'str',
  CommentStatus: 'str',
  RevisionAction: 'str',
  RevisionTarget: 'str',
  PreviewKey: 'str',
  SourceHash: 'str',
  Points: 'float',
  LineSpacing: 'dict',
  Span: 'dict',
  Diagnostic: 'dict',
};

function pythonName(field) {
  return PYTHON_KEYWORDS.has(field) ? `${field}_` : field;
}

function className(opName) {
  return opName.split('_').map((part) => part[0].toUpperCase() + part.slice(1)).join('');
}

/** True when the property accepts null (nullable type list or anyOf branch). */
function isNullable(prop) {
  if (Array.isArray(prop.type) && prop.type.includes('null')) return true;
  if (Array.isArray(prop.anyOf) && prop.anyOf.some((alt) => alt?.type === 'null')) return true;
  return false;
}

/** Base Python annotation for the non-null part of a property schema. */
function baseAnnotation(prop, opName) {
  if (Array.isArray(prop.anyOf)) {
    const alternatives = prop.anyOf.filter((alt) => alt?.type !== 'null');
    if (alternatives.length !== 1) {
      throw new Error(`editOp variant ${opName}: anyOf must have exactly one non-null alternative`);
    }
    return baseAnnotation(alternatives[0], opName);
  }
  if (typeof prop.$ref === 'string') {
    const name = prop.$ref.split('/').pop();
    const mapped = REF_TYPES[name];
    if (mapped === undefined) {
      throw new Error(`editOp variant ${opName}: $ref #/$defs/${name} has no Python mapping; extend REF_TYPES or adjust the manifest`);
    }
    return mapped;
  }
  if (prop.enum !== undefined || prop.const !== undefined) return 'str';
  const type = Array.isArray(prop.type) ? prop.type[0] : prop.type;
  switch (type) {
    case 'string': return 'str';
    case 'integer': return 'int';
    case 'boolean': return 'bool';
    case 'number': return 'float';
    case 'array': return 'list';
    default:
      throw new Error(`editOp variant ${opName}: unsupported property type ${JSON.stringify(prop.type)}`);
  }
}

/** Python annotation for a property, e.g. `str | None` for optional fields. */
function annotation(prop, optional, opName) {
  const base = baseAnnotation(prop, opName);
  return optional ? `${base} | None` : base;
}

/** `with` also accepts host-resolved structured quotation nodes in quote mode. */
function fieldAnnotation(name, prop, optional, opName) {
  const base = name === 'with' ? 'str | Inline | Quote | Term' : baseAnnotation(prop, opName);
  return optional ? `${base} | None` : base;
}

/** Assert the variant is a strict, schema-generated edit operation. */
function assertStrictVariant(variant, opName) {
  if (variant?.type !== 'object') throw new Error(`editOp variant ${opName}: type must be object`);
  if (variant.additionalProperties !== false) {
    throw new Error(`editOp variant ${opName}: additionalProperties must be false (strict schema required)`);
  }
  if (!variant.properties || !variant.properties.op) {
    throw new Error(`editOp variant ${opName}: missing op property`);
  }
  if (!Array.isArray(variant.required) || !variant.required.includes('op')) {
    throw new Error(`editOp variant ${opName}: op must be required`);
  }
}

/**
 * Resolve the manifest against the schema union. Asserts strictness on every
 * variant (not just the selected ones) so a schema that loses strictness
 * fails generation immediately, and sorts deterministically by op name.
 */
function selectedVariants(schema) {
  const oneOf = schema?.$defs?.editOp?.oneOf;
  if (!Array.isArray(oneOf) || oneOf.length === 0) {
    throw new Error(`schema at ${SCHEMA_PATH} has no $defs.editOp.oneOf union`);
  }
  const selected = [];
  for (const variant of oneOf) {
    const opName = variant?.properties?.op?.enum?.[0] ?? variant?.properties?.op?.const;
    if (typeof opName !== 'string') {
      throw new Error('editOp variant without a single string op const/enum');
    }
    assertStrictVariant(variant, opName);
    if (!PLAN_CAPABILITIES.ops.includes(opName)) continue;
    if (typeof PLAN_CAPABILITIES.docs[opName] !== 'string') {
      throw new Error(`manifest selects op ${opName} without a doc string`);
    }
    selected.push({ opName, variant });
  }
  const missing = PLAN_CAPABILITIES.ops.filter((op) => !selected.some((entry) => entry.opName === op));
  if (missing.length > 0) {
    throw new Error(`manifest selects ops missing from the schema: ${missing.join(', ')}`);
  }
  selected.sort((a, b) => (a.opName < b.opName ? -1 : a.opName > b.opName ? 1 : 0));
  return selected;
}

/**
 * Field list for a variant: required (minus `op`) first, then optional, both
 * in schema property order — exactly the constructor order Python dataclasses
 * need (required fields carry no default).
 */
function variantFields(variant, opName) {
  const props = variant.properties ?? {};
  const required = new Set((variant.required ?? []).filter((field) => field !== 'op'));
  const fields = [];
  for (const [name, prop] of Object.entries(props)) {
    if (name === 'op') continue;
    // A field is optional when the schema does not require it OR the schema
    // makes it nullable: both get a `| None` annotation and a None default.
    fields.push({ name, prop, optional: !required.has(name) || isNullable(prop) });
  }
  return [...fields.filter((field) => !field.optional), ...fields.filter((field) => field.optional)];
}

function signatureParams(fields, opName) {
  return fields
    .map(({ name, prop, optional }) => `${pythonName(name)}: ${fieldAnnotation(name, prop, optional, opName)}${optional ? ' = None' : ''}`)
    .join(', ');
}

function generateDataclass({ opName, variant }) {
  const fields = variantFields(variant, opName);
  return [
    `@dataclass`,
    `class ${className(opName)}:`,
    ...fields.map(({ name, prop, optional }) => `    ${pythonName(name)}: ${fieldAnnotation(name, prop, optional, opName)}${optional ? ' = None' : ''}`),
    `    def to_dict(self):`,
    `        value = {"op": "${opName}"}`,
    ...fields.flatMap(({ name }) => [
      `        if self.${pythonName(name)} is not None:`,
      name === 'with'
        ? `            value["${name}"] = self.${pythonName(name)}.to_dict() if hasattr(self.${pythonName(name)}, "to_dict") else self.${pythonName(name)}`
        : `            value["${name}"] = self.${pythonName(name)}`,
    ]),
    `        return value`,
  ].join('\n');
}

function generateDataclassesBlock(selected) {
  return selected.map(generateDataclass).join('\n\n') + '\n';
}

function generateTypeStub({ opName, variant }) {
  const fields = variantFields(variant, opName);
  return [
    `class ${className(opName)}:`,
    `    def __init__(self, ${signatureParams(fields, opName)}): ...`,
    ...fields.map(({ name, prop, optional }) => `    ${pythonName(name)}: ${fieldAnnotation(name, prop, optional, opName)}`),
    `    def to_dict(self) -> dict: ...`,
  ].join('\n');
}

function generateTypeStubsBlock(selected) {
  return selected.map(generateTypeStub).join('\n\n') + '\n';
}

function generateDocsBlock(selected) {
  const lines = [
    '## Operations',
    '',
    'Selected from the core schema (`$defs.editOp.oneOf`) through the',
    'capability manifest; field shapes, types, and enums are exactly the',
    'checked-in schema\u2019s. Every op dataclass has `to_dict()`, omitting',
    'optional fields whose value is `None`; constructors use `with_`/`as_`',
    'for Python keyword collisions and `to_dict()` maps back to the schema',
    'field names. Operations are addressed by the paragraph `id` attribute',
    'from `docx_read` (uppercase eight-digit hex) or a `$name` alias.',
    '',
  ];
  for (const { opName, variant } of selected) {
    const fields = variantFields(variant, opName);
    lines.push(`### ${className(opName)}`);
    lines.push('');
    lines.push(`\`${className(opName)}(${signatureParams(fields, opName)})\``);
    lines.push('');
    lines.push(PLAN_CAPABILITIES.docs[opName]);
    lines.push('');
    lines.push('| Field | Type | Required |');
    lines.push('| --- | --- | --- |');
    for (const { name, prop, optional } of fields) {
      const py = pythonName(name);
      const shown = py !== name ? `\`${py}\` (schema key \`${name}\`)` : `\`${py}\``;
      lines.push(`| ${shown} | \`${fieldAnnotation(name, prop, optional, opName)}\` | ${optional ? 'no' : 'yes'} |`);
    }
    lines.push('');
  }
  return lines.join('\n');
}

const REGIONS = [
  { start: '# BEGIN GENERATED DATACLASSES', end: '# END GENERATED DATACLASSES', blockFor: generateDataclassesBlock, embedTs: false },
  { start: '# BEGIN GENERATED TYPE STUBS', end: '# END GENERATED TYPE STUBS', blockFor: generateTypeStubsBlock, embedTs: false },
  { start: '# BEGIN GENERATED OPERATION DOCS', end: '# END GENERATED OPERATION DOCS', blockFor: generateDocsBlock, embedTs: true },
];

/** Escape a generated block for embedding in a TypeScript template literal. */
function embedTs(text) {
  return text.replace(/`/g, '\\`').replace(/\$\{/g, '\\${');
}

const schema = JSON.parse(await readFile(SCHEMA_PATH, 'utf8'));
const selected = selectedVariants(schema);
const source = await readFile(target, 'utf8');
let next = source;
for (const region of REGIONS) {
  const start = next.indexOf(region.start);
  const end = next.indexOf(region.end);
  if (start === -1 || end === -1) {
    throw new Error(`target ${target} lacks ${region.start} / ${region.end} markers`);
  }
  const block = region.embedTs ? embedTs(region.blockFor(selected)) : region.blockFor(selected);
  next = next.slice(0, start + region.start.length) + '\n' + block + '\n' + next.slice(end);
}
await writeFile(target, next);
