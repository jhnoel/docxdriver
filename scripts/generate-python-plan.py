#!/usr/bin/env python3
"""Generate the Python plan vocabulary from core's schema (no Node).

    core schema ($defs.editOp.oneOf) + plan-capabilities.json
        |  splice GENERATED regions
        v
    crates/docxdriver-python/python/docxdriver/plan.py   <- the one plan.py
        |   (PyPI docxdriver.plan; crates/docxdriver-monty-repl/sandbox/plan.py
        |    is a symlink to it: docxdriver_monty_repl::PLAN)
    crates/docxdriver-monty-repl/sandbox/plan.{pyi,md}    (PLAN_STUBS, PLAN_REFERENCE)
    experimental/python-repl-host/docxdriver_repl/prelude.py
        async host wrappers + reference for Python Monty hosts (unpublished)

packages/docxdriver-pi keeps its own templates (scripts/generate-python-dataclasses.mjs).

Usage: python scripts/generate-python-plan.py [--check]
--check writes nothing and exits 1 when any output is stale.
DOCXDRIVER_SCHEMA_PATH overrides the schema location.
"""
import ast
import json
import os
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
SANDBOX = ROOT / 'crates/docxdriver-monty-repl/sandbox'
HOST_SANDBOX = ROOT / 'experimental/python-repl-host/sandbox'
SCHEMA = Path(os.environ.get('DOCXDRIVER_SCHEMA_PATH', ROOT / 'packages/docxdriver/schema/typed-request.schema.json'))
CAPABILITIES = SANDBOX / 'plan-capabilities.json'
PLAN_PY = ROOT / 'crates/docxdriver-python/python/docxdriver/plan.py'
PLAN_PYI = SANDBOX / 'plan.pyi'
PLAN_MD = SANDBOX / 'plan.md'
HOST_PY = HOST_SANDBOX / 'host_wrappers.py'
HOST_MD = HOST_SANDBOX / 'host_reference.md'
REPL_PRELUDE = ROOT / 'experimental/python-repl-host/docxdriver_repl/prelude.py'
CHECK_FLAG = '--check'
# Sources are UTF-8 (e.g. `’`); the platform default differs on Windows.
ENCODING = 'utf-8'

# Python keywords: a schema field colliding with one gets a trailing `_`.
PYTHON_KEYWORDS = {
    'False', 'None', 'True', 'and', 'as', 'assert', 'async', 'await', 'break',
    'case', 'class', 'continue', 'def', 'del', 'elif', 'else', 'except',
    'finally', 'for', 'from', 'global', 'if', 'import', 'in', 'is', 'lambda',
    'match', 'nonlocal', 'not', 'or', 'pass', 'raise', 'return', 'try',
    'while', 'with', 'yield',
}

# Schema $ref -> Python annotation. An unknown $ref fails generation so a
# schema change surfaces instead of a guessed type.
REF_TYPES = {
    'ParagraphAddress': 'str', 'InsertAnchor': 'str', 'ChromeKind': 'str',
    'InsertPosition': 'str', 'ChangeMode': 'str', 'CommentStatus': 'str',
    'RevisionAction': 'str', 'RevisionTarget': 'str', 'RevisionSelector': 'str | dict', 'TextEdit': 'dict',
    'PreviewKey': 'str', 'SourceHash': 'str', 'Points': 'float',
    'LineSpacing': 'dict', 'Span': 'dict', 'Diagnostic': 'dict',
    'HighlightColor': 'str', 'SignedPoints': 'float', 'TableEdit': 'dict',
    'ParagraphContent': 'str | list[str]',
    'TableFormat': 'dict', 'RowFormat': 'dict', 'CellFormat': 'dict',
}
JSON_TYPES = {'string': 'str', 'integer': 'int', 'boolean': 'bool', 'number': 'float', 'array': 'list'}

# Host callbacks the REPL sandbox awaits.
HOST_CALLBACK_PREFIXES = ('_docx_', '_quote_', '_term_', '_inline_')
HOST_FUNCTIONS = r'\b(docx_(?:read|find|create|review|commit|help)|quote_(?:find|validate|lint)|term_render|inline_validate)\('


def python_name(field):
    return f'{field}_' if field in PYTHON_KEYWORDS else field


def class_name(op):
    return ''.join(part[0].upper() + part[1:] for part in op.split('_'))


def is_nullable(prop):
    if isinstance(prop.get('type'), list) and 'null' in prop['type']:
        return True
    return any(alt.get('type') == 'null' for alt in prop.get('anyOf', []))


def base_annotation(prop, op):
    """Python annotation for the non-null part of a property schema."""
    if 'anyOf' in prop:
        alternatives = [alt for alt in prop['anyOf'] if alt.get('type') != 'null']
        if len(alternatives) != 1:
            raise ValueError(f'editOp variant {op}: anyOf must have exactly one non-null alternative')
        return base_annotation(alternatives[0], op)

    # A defaulted reference renders as a one-element allOf.
    if len(prop.get('allOf', [])) == 1:
        return base_annotation(prop['allOf'][0], op)

    if '$ref' in prop:
        name = prop['$ref'].split('/')[-1]
        if name not in REF_TYPES:
            raise ValueError(f'editOp variant {op}: $ref #/$defs/{name} has no Python mapping; extend REF_TYPES')
        return REF_TYPES[name]

    if 'enum' in prop or 'const' in prop:
        return 'str'

    kind = prop['type'][0] if isinstance(prop.get('type'), list) else prop.get('type')
    if kind not in JSON_TYPES:
        raise ValueError(f'editOp variant {op}: unsupported property type {json.dumps(prop.get("type"))}')
    return JSON_TYPES[kind]


def field_annotation(field):
    name, prop, optional, op = field
    base = base_annotation(prop, op)
    return f'{base} | None' if optional else base


def assert_strict(variant, op):
    if variant.get('type') != 'object':
        raise ValueError(f'editOp variant {op}: type must be object')
    if variant.get('additionalProperties') is not False:
        raise ValueError(f'editOp variant {op}: additionalProperties must be false (strict schema required)')
    if 'op' not in variant.get('properties', {}) or 'op' not in variant.get('required', []):
        raise ValueError(f'editOp variant {op}: op must be a required property')


def selected_variants(schema, capabilities):
    """Manifest-selected variants sorted by op; every variant must be strict."""
    variants = schema.get('$defs', {}).get('editOp', {}).get('oneOf')
    if not variants:
        raise ValueError(f'schema at {SCHEMA} has no $defs.editOp.oneOf union')

    selected = []
    for variant in variants:
        op_schema = variant.get('properties', {}).get('op', {})
        op = (op_schema.get('enum') or [op_schema.get('const')])[0]
        if not isinstance(op, str):
            raise ValueError('editOp variant without a single string op const/enum')
        assert_strict(variant, op)
        if op not in capabilities['ops']:
            continue
        if not isinstance(capabilities['docs'].get(op), str):
            raise ValueError(f'manifest selects op {op} without a doc string')
        selected.append((op, variant))

    missing = [op for op in capabilities['ops'] if op not in {name for name, _ in selected}]
    if missing:
        raise ValueError(f'manifest selects ops missing from the schema: {", ".join(missing)}')
    return sorted(selected, key=lambda entry: entry[0])


def variant_fields(op, variant):
    """Required fields first, then optional, each in schema order (dataclass order)."""
    required = set(variant.get('required', [])) - {'op'}
    fields = []
    for name, prop in variant.get('properties', {}).items():
        if name == 'op':
            continue
        # Optional when not required OR nullable: both get `| None = None`.
        fields.append((name, prop, name not in required or is_nullable(prop), op))
    return [f for f in fields if not f[2]] + [f for f in fields if f[2]]


def signature(fields):
    return ', '.join(f'{python_name(f[0])}: {field_annotation(f)}{" = None" if f[2] else ""}' for f in fields)


def dataclass_source(op, variant):
    fields = variant_fields(op, variant)
    lines = ['@dataclass', f'class {class_name(op)}:']
    lines += [f'    {python_name(f[0])}: {field_annotation(f)}{" = None" if f[2] else ""}' for f in fields]
    lines += ['    def to_dict(self):', f'        value = {{"op": "{op}"}}']
    for name, *_ in fields:
        attr = f'self.{python_name(name)}'
        lines.append(f'        if {attr} is not None:')
        lines.append(f'            value["{name}"] = {attr}')
    lines.append('        return value')
    return '\n'.join(lines)


def stub_source(op, variant):
    fields = variant_fields(op, variant)
    lines = [f'class {class_name(op)}:', f'    def __init__(self, {signature(fields)}): ...']
    lines += [f'    {python_name(f[0])}: {field_annotation(f)}' for f in fields]
    lines.append('    def to_dict(self) -> dict: ...')
    return '\n'.join(lines)


def docs_source(selected, capabilities):
    lines = [
        '## Operations',
        '',
        'Selected from the core schema (`$defs.editOp.oneOf`) through the',
        'capability manifest; field shapes, types, and enums are exactly the',
        'checked-in schema’s. Every op dataclass has `to_dict()`, omitting',
        'optional fields whose value is `None`; constructors use `with_`/`as_`',
        'for Python keyword collisions and `to_dict()` maps back to the schema',
        'field names. Operations are addressed by the paragraph `id` attribute',
        'from `doc.markup()` or `doc.find()` (uppercase eight-digit hex) or a `$name` alias.',
        '',
    ]
    for op, variant in selected:
        fields = variant_fields(op, variant)
        lines += [f'### {class_name(op)}', '', f'`{class_name(op)}({signature(fields)})`', '',
                  capabilities['docs'][op], '', '| Field | Type | Required |', '| --- | --- | --- |']
        for field in fields:
            name, py = field[0], python_name(field[0])
            shown = f'`{py}` (schema key `{name}`)' if py != name else f'`{py}`'
            lines.append(f'| {shown} | `{field_annotation(field)}` | {"no" if field[2] else "yes"} |')
        lines.append('')
    return '\n'.join(lines)


def read(path):
    """UTF-8 text with LF newlines on every platform."""
    return path.read_text(encoding=ENCODING)


def splice(text, region, block):
    """Replace the body between `# BEGIN <region>` and `# END <region>`."""
    start, end = f'# BEGIN {region}', f'# END {region}'
    i, j = text.find(start), text.find(end)
    if i == -1 or j == -1:
        raise ValueError(f'missing {start} / {end} markers')
    return text[:i + len(start)] + '\n' + block + '\n' + text[j:]


class AsyncHostCalls(ast.NodeTransformer):
    """Await host callbacks and make their callers async (Python Monty hosts)."""

    def visit_FunctionDef(self, node):
        if not any(is_host_call(call) for call in ast.walk(node)):
            return node
        node = self.generic_visit(node)
        return ast.copy_location(ast.AsyncFunctionDef(name=node.name, args=node.args, body=node.body,
            decorator_list=node.decorator_list, returns=node.returns, type_comment=node.type_comment), node)

    def visit_Call(self, node):
        node = self.generic_visit(node)
        if not is_host_call(node):
            return node
        return ast.copy_location(ast.Await(value=node), node)


def is_host_call(node):
    return (isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
            and node.func.id.startswith(HOST_CALLBACK_PREFIXES))


def repl_prelude(host, reference):
    """docxdriver_repl.prelude: installed plan.py + async host wrappers."""
    host = ast.unparse(ast.fix_missing_locations(AsyncHostCalls().visit(ast.parse(host))))
    reference = re.sub(HOST_FUNCTIONS, r'await \1(', reference)
    reference = re.sub(r'await docx_read\(([^)\n]*)\)\.markup', r'(await docx_read(\1)).markup', reference)
    return ('# Generated by scripts/generate-python-plan.py from ../sandbox and the docxdriver plan reference.\n'
            'from importlib.resources import files\n\n'
            "PLAN = files('docxdriver').joinpath('plan.py').read_text(encoding='utf-8')\n"
            f'HOST_WRAPPERS = {host!r}\n'
            "PRELUDE = PLAN + '\\n' + HOST_WRAPPERS\n"
            f'REFERENCE = {reference!r}\n')


def generate():
    """Map every output path to its expected content."""
    schema = json.loads(read(SCHEMA))
    capabilities = json.loads(read(CAPABILITIES))
    selected = selected_variants(schema, capabilities)

    # Splice the schema-derived regions into plan.py and its stubs/reference.
    plan_py = splice(read(PLAN_PY), 'GENERATED DATACLASSES',
                     '\n\n'.join(dataclass_source(*entry) for entry in selected) + '\n')
    plan_pyi = splice(read(PLAN_PYI), 'GENERATED TYPE STUBS',
                      '\n\n'.join(stub_source(*entry) for entry in selected) + '\n')
    plan_md = splice(read(PLAN_MD), 'GENERATED OPERATION DOCS', docs_source(selected, capabilities))

    reference = plan_md + '\n' + read(HOST_MD)
    return {
        PLAN_PY: plan_py,
        PLAN_PYI: plan_pyi,
        PLAN_MD: plan_md,
        REPL_PRELUDE: repl_prelude(read(HOST_PY), reference),
    }


def main(argv):
    outputs = generate()
    stale = [path for path, text in outputs.items() if not path.exists() or read(path) != text]

    if CHECK_FLAG in argv:
        for path in stale:
            print(f'stale: {path.relative_to(ROOT)}', file=sys.stderr)
        return 1 if stale else 0

    for path in stale:
        path.write_text(outputs[path], encoding=ENCODING, newline='\n')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
