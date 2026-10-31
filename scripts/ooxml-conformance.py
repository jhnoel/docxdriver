#!/usr/bin/env python3
"""Differential ECMA-376 conformance gate for docxdriver edit operations.

Every typed edit operation in the core schema is applied, through a real
preview-and-commit plan, to every input document. Each committed package is
checked by three validators:

- the bundled ECMA-376 Transitional and OPC schemas (lxml);
- package and cross-part integrity rules no schema expresses: content types
  Word accepts, relationship targets, comment/numbering/note references,
  field and bookmark pairing, table grids, w14/durable id ranges
  (scripts/ooxml_integrity.py);
- the Open XML SDK validator for the Microsoft 365 format, which also covers
  the Office extension namespaces the XSD pass strips
  (scripts/openxml-validator, built here with `dotnet`).

The gate fails when an operation introduces an error kind the source did not
already have. Pre-existing invalid content is not docxdriver's to fix, and a
run split that copies an invalid property is not a new kind of error.

The gate also fails unless every operation in the schema was applied at
least once, and unless a freshly created document validates with no errors.

Inputs are a rich synthetic document built with `create`, the repository's
DOCX fixtures, and optionally a corpus directory (scripts/fetch-corpus.sh):

    cargo build -p docxdriver-cli
    python scripts/ooxml-conformance.py [--corpus corpus/lo] [--limit N]

Requires lxml and the .NET SDK (10 or later). `--keep-outputs DIR` saves every
committed package for scripts/word-smoke.py.
"""
import argparse
import collections
from concurrent.futures import ProcessPoolExecutor
import hashlib
import html
import io
import json
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

from lxml import etree

from ooxml_integrity import integrity_errors

ROOT = Path(__file__).resolve().parents[1]
SCHEMA_DIR = ROOT / 'scripts' / 'ooxml-schemas'
OP_SCHEMA = ROOT / 'packages' / 'docxdriver' / 'schema' / 'typed-request.schema.json'
FIXTURES = [
    'crates/docxdriver-core/assets/blank.docx',
    'crates/docxdriver-core/tests/fixtures/ts-reference.docx',
    'crates/docxdriver-core/tests/fixtures/projection-parity/parity.docx',
    'crates/docxdriver-cli/tests/fixtures/sample.docx',
]
MC = 'http://schemas.openxmlformats.org/markup-compatibility/2006'
OFFICE_EXTENSIONS = 'http://schemas.microsoft.com/office/word/'
PARSER = etree.XMLParser(resolve_entities=False, no_network=True, load_dtd=False)

SYNTHETIC_HTML = (
    '<h1>Master Services Agreement</h1>'
    '<p>The <b>Supplier</b> shall deliver the <i>Services</i> within thirty days, '
    'as described at <a href="https://example.com/terms">the terms page</a>.</p>'
    '<ol><li><p>First obligation of the parties.</p><p>Continuation of the first obligation.</p><ul><li>Child obligation.</li></ul></li><li>Second obligation of the parties.</li></ol>'
    '<table><tr><td>Fee schedule</td><td>Amount due</td></tr><tr><td>Initial fee</td><td>5000 dollars</td></tr></table>'
    '<p>The area is <math display="inline"><msup><mi>r</mi><mn>2</mn></msup></math> times pi in each region.</p>'
    '<p>Either party may terminate this agreement with written notice.</p>'
    '<p>Page <page-number/> of <page-count/></p>'
)


def rich_formatting_document(source):
    """A valid property bag that exposes destructive normalization/order bugs."""
    w = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
    ppr = f'''<w:pPr xmlns:w="{w}"><w:keepNext/><w:keepLines/>
        <w:widowControl w:val="0"/><w:shd w:val="clear" w:fill="EEEEEE"/>
        <w:tabs><w:tab w:val="right" w:leader="dot" w:pos="1440"/></w:tabs>
        <w:spacing w:before="120" w:after="240" w:line="276" w:lineRule="auto"/>
        <w:ind w:left="360" w:right="180" w:hanging="180"/>
        <w:jc w:val="left"/><w:rPr><w:lang w:val="en-US"/></w:rPr>
        <w:sectPr><w:pgSz w:w="12240" w:h="15840"/></w:sectPr></w:pPr>'''
    rpr = f'''<w:rPr xmlns:w="{w}"><w:rFonts w:ascii="Arial" w:hAnsi="Arial"/>
        <w:b w:val="0"/><w:i w:val="false"/><w:smallCaps/>
        <w:color w:val="auto" w:themeColor="accent1" w:themeTint="80"/>
        <w:sz w:val="24"/><w:highlight w:val="yellow"/>
        <w:u w:val="double" w:color="FF0000" w:themeColor="accent2"/>
        <w:lang w:val="en-US"/></w:rPr>'''
    with zipfile.ZipFile(io.BytesIO(source)) as package:
        entries = {entry.filename: package.read(entry) for entry in package.infolist()}
    root = etree.fromstring(entries['word/document.xml'], PARSER)
    paragraph = root.find(f'.//{{{w}}}p')
    old = paragraph.find(f'{{{w}}}pPr')
    if old is not None:
        paragraph.remove(old)
    paragraph.insert(0, etree.fromstring(ppr.encode(), PARSER))
    for run in paragraph.findall(f'{{{w}}}r'):
        old = run.find(f'{{{w}}}rPr')
        if old is not None:
            run.remove(old)
        run.insert(0, etree.fromstring(rpr.encode(), PARSER))
    entries['word/document.xml'] = etree.tostring(root, xml_declaration=True, encoding='UTF-8')
    out = io.BytesIO()
    with zipfile.ZipFile(out, 'w', compression=zipfile.ZIP_DEFLATED) as package:
        for name, data in entries.items():
            package.writestr(name, data)
    return out.getvalue()


SDK_PROJECT = ROOT / 'scripts' / 'openxml-validator'
SDK_BUILD = ROOT / 'target' / 'openxml-validator'


def build_sdk_validator():
    """Build scripts/openxml-validator (incremental); returns an error or None."""
    if shutil.which('dotnet') is None:
        return ('the .NET SDK is required for Open XML SDK validation: '
                'brew install dotnet (macOS) or https://dotnet.microsoft.com/download')
    result = subprocess.run(['dotnet', 'build', str(SDK_PROJECT), '-c', 'Release', '-o', str(SDK_BUILD), '--nologo', '-v', 'q'],
                            capture_output=True, text=True)
    if result.returncode != 0:
        return 'openxml-validator build failed:\n' + (result.stdout + result.stderr)[-2000:]
    return None


class SdkValidator:
    """One long-lived `dotnet openxml-validator.dll` process per gate worker."""

    def __init__(self):
        self.process = subprocess.Popen(['dotnet', str(SDK_BUILD / 'openxml-validator.dll')], stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, text=True, bufsize=1)
        self.scratch = Path(tempfile.mkdtemp(prefix='docxdriver-sdk-'))

    def errors(self, data):
        path = self.scratch / 'package.docx'
        path.write_bytes(data)
        self.process.stdin.write(str(path) + '\n')
        self.process.stdin.flush()
        result = json.loads(self.process.stdout.readline())
        found = collections.Counter()
        if 'fatal' in result:
            message = re.sub(r'\s+', ' ', result['fatal'])
            # The SDK stops at the first missing part. Rewriting relationships
            # can change which existing dangling target it encounters first;
            # integrity_errors separately checks every relationship target.
            message = re.sub(r"Part: /\S+ doesn't exist in the package", "Part: <missing> doesn't exist in the package", message)
            found[('package', 'sdk: cannot open: ' + re.sub(r'\d+', 'N', message))] += 1
        for error in result.get('errors', []):
            message = re.sub(r' List of possible elements expected:.*', '', error['description'])
            found[(error['part'].lstrip('/'), f"sdk {error['id']}: " + re.sub(r'\d+', 'N', message))] += 1
        return found


class Validator:
    def __init__(self):
        self.paths = {}
        for path in SCHEMA_DIR.glob('*.xsd'):
            namespace = etree.parse(str(path), PARSER).getroot().get('targetNamespace')
            if namespace:
                self.paths[namespace] = path
        self.compiled = {}
        self.sdk = SdkValidator()

    def errors(self, data):
        """Error kinds per part: schema, integrity, and Open XML SDK."""
        found = self.schema_errors(data)
        with zipfile.ZipFile(io.BytesIO(data)) as package:
            found.update(integrity_errors({entry.filename: package.read(entry) for entry in package.infolist()}))
        found.update(self.sdk.errors(data))
        return found

    def schema_errors(self, data):
        """Schema error kinds per part, after ECMA-376 Part 3 markup-compatibility processing."""
        found = collections.Counter()
        with zipfile.ZipFile(io.BytesIO(data)) as package:
            for entry in package.infolist():
                if not entry.filename.lower().endswith(('.xml', '.rels')):
                    continue
                try:
                    root = etree.fromstring(package.read(entry), PARSER)
                except etree.XMLSyntaxError as error:
                    found[(entry.filename, 'malformed XML: ' + str(error).splitlines()[0])] += 1
                    continue
                for alternate in list(root.iter(f'{{{MC}}}AlternateContent')):
                    parent = alternate.getparent()
                    if parent is None:
                        continue
                    fallback = alternate.find(f'{{{MC}}}Fallback')
                    index = parent.index(alternate)
                    for child in list(fallback) if fallback is not None else []:
                        parent.insert(index, child)
                        index += 1
                    parent.remove(alternate)
                # Snapshot the walk: removing children while iterating skips nodes.
                for element in list(root.iter()):
                    for name in list(element.attrib):
                        namespace = etree.QName(name).namespace or ''
                        if namespace == MC or namespace.startswith(OFFICE_EXTENSIONS):
                            del element.attrib[name]
                    for child in list(element):
                        if isinstance(child.tag, str) and (etree.QName(child).namespace or '').startswith(OFFICE_EXTENSIONS):
                            element.remove(child)
                namespace = etree.QName(root).namespace
                if namespace not in self.paths:
                    continue
                if namespace not in self.compiled:
                    self.compiled[namespace] = etree.XMLSchema(etree.parse(str(self.paths[namespace]), PARSER))
                schema = self.compiled[namespace]
                if not schema.validate(root.getroottree()):
                    for issue in schema.error_log:
                        # The expected-element list depends on the preceding
                        # siblings, so an edit near a pre-existing ordering
                        # error would otherwise read as a new kind.
                        message = re.sub(r' Expected is .*', '', issue.message)
                        found[(entry.filename, re.sub(r'\d+', 'N', message))] += 1
        return found


class Cli:
    def __init__(self, binary, work, keep=None, stem='source'):
        self.binary, self.work, self.keep, self.stem = binary, work, keep, stem

    def run(self, *args):
        result = subprocess.run([self.binary, *args, '--json'], cwd=self.work, capture_output=True, text=True)
        try:
            return json.loads(result.stdout)
        except json.JSONDecodeError:
            return {'outcome': 'error', 'stderr': result.stderr[:300]}

    def read(self, name, kind=None):
        args = ['read', name] + (['--kind', kind] if kind else [])
        result = self.run(*args)
        return result.get('result') if result.get('outcome') == 'completed' else None

    def commit(self, name, ops, out, mode='track'):
        """Preview then commit `ops` as one plan; returns the outcome or a reason."""
        source = (self.work / name).read_bytes()
        plan = toml_plan('sha256:' + hashlib.sha256(source).hexdigest(), mode, ops)
        (self.work / 'plan.toml').write_text(plan)
        preview = self.run('plan', name, 'plan.toml')
        if preview.get('outcome') != 'previewed':
            return (preview.get('diagnostic') or {}).get('message', preview.get('stderr', 'rejected'))
        (self.work / out).unlink(missing_ok=True)
        committed = self.run('plan', name, 'plan.toml', '--commit', preview['preview_key'], '--output', out)
        if committed.get('outcome') != 'committed':
            return (committed.get('diagnostic') or {}).get('message', 'commit failed')
        if self.keep:
            digest = hashlib.sha256((self.work / out).read_bytes()).hexdigest()[:12]
            shutil.copyfile(self.work / out, Path(self.keep) / f'{self.stem}--{ops[0]["op"]}-{mode}-{digest}.docx')
        return None


def toml_value(value):
    if isinstance(value, bool):
        return 'true' if value else 'false'
    if isinstance(value, (int, float)):
        return repr(value)
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, list):
        return '[' + ', '.join(toml_value(item) for item in value) + ']'
    if isinstance(value, dict):
        return '{ ' + ', '.join(f'{key} = {toml_value(item)}' for key, item in value.items()) + ' }'
    raise TypeError(value)


def toml_plan(base, mode, ops):
    lines = [f'base = "{base}"', 'author = "conformance"', f'change_mode = "{mode}"']
    for op in ops:
        lines.append('\n[[ops]]')
        lines.extend(f'{key} = {toml_value(value)}' for key, value in op.items())
    return '\n'.join(lines) + '\n'


def schema_ops():
    schema = json.loads(OP_SCHEMA.read_text())
    return sorted(variant['properties']['op']['enum'][0] for variant in schema['$defs']['editOp']['oneOf'])


def text_paragraphs(markup):
    found = []
    for match in re.finditer(r'<p id="([0-9A-F]{8})"[^>]*>(.*?)</p>', markup, re.S):
        text = html.unescape(re.sub(r'<[^>]+>', '', match.group(2)))
        words = [word for word in text.split() if word.isalpha() and len(word) > 2]
        if len(words) >= 2:
            found.append((match.group(1), words))
    return found


def scenarios(read):
    """(label, change_mode, ops) for one document; labels name the op they exercise."""
    markup = read['markup']
    paragraphs = text_paragraphs(markup)
    out = [
        ('set_page_margins', 'track', [{'op': 'set_page_margins', 'top': 72.0, 'left': 90.0}]),
        ('set_section_layout', 'track', [{'op': 'set_section_layout', 'page_size': 'a4', 'orientation': 'landscape', 'columns': 2, 'column_spacing': 18, 'column_separator': True, 'break_type': 'continuous', 'page_number_format': 'lowerRoman', 'page_number_start': 3}]),
        ('set_section_layout', 'track', [{'op': 'set_section_layout', 'column_widths': [144, 216], 'column_spacing': 12}, {'op': 'revision_settle', 'target': 'all', 'action': 'reject'}]),
        ('set_first_page_mode', 'track', [{'op': 'set_first_page_mode', 'enabled': True}]),
        ('set_first_page_mode', 'track', [{'op': 'set_first_page_mode', 'enabled': True}, {'op': 'revision_settle', 'target': 'all', 'action': 'reject'}]),
        ('set_first_page_mode', 'direct', [{'op': 'set_first_page_mode', 'enabled': True}, {'op': 'set_first_page_mode', 'enabled': False}]),
        ('set_even_and_odd_headers', 'direct', [{'op': 'set_even_and_odd_headers', 'even_and_odd': True}]),
        ('set_header', 'direct', [{'op': 'set_header', 'kind': 'even', 'with': '<p>Even <b>header</b></p>'}]),
        ('set_header', 'track', [{'op': 'set_header', 'with': '<p>Tracked <b>header</b></p><p>Second line</p>'}]),
        ('set_header', 'track', [{'op': 'set_header', 'kind': 'first', 'with': '<p>First page</p>'}, {'op': 'revision_settle', 'target': 'all', 'action': 'reject'}]),
        ('set_footer', 'track', [{'op': 'set_footer', 'with': '<p>Page footer</p>'}]),
        ('set_footer', 'track', [{'op': 'set_footer', 'with': '<p>Page <page-number/> of <page-count/></p>'}, {'op': 'revision_settle', 'target': 'all', 'action': 'accept'}]),
        ('set_page_margins', 'direct', [{'op': 'set_page_margins', 'top': 72.0, 'left': 90.0}]),
        ('set_page_margins', 'track', [{'op': 'set_page_margins', 'bottom': 54.0}, {'op': 'revision_settle', 'target': 'all', 'action': 'reject'}]),
        ('clear_header', 'track', [{'op': 'set_header', 'kind': 'first', 'with': '<p>First</p>'}, {'op': 'clear_header', 'kind': 'first'}]),
        ('clear_footer', 'track', [{'op': 'clear_footer'}]),
        ('revision_settle', 'track', [{'op': 'revision_settle', 'target': 'all', 'action': 'accept'}]),
        ('revision_settle', 'track', [{'op': 'revision_settle', 'target': 'all', 'action': 'reject'}]),
    ]
    if '<table' in markup:
        out.append(('insert_paragraph', 'track', [{'op': 'insert_paragraph', 'at': 'table:1', 'position': 'after', 'with': 'After the table'}]))
    for equation in (read.get('equations') or [])[:1]:
        at, number = equation['at'], equation.get('equation', 1)
        mathml = '<math display="inline"><mfrac><mi>a</mi><mi>b</mi></mfrac></math>'
        for mode in ('track', 'direct'):
            out.append(('replace_equation', mode, [{'op': 'replace_equation', 'at': at, 'equation': number, 'mathml': mathml}]))
            out.append(('delete_equation', mode, [{'op': 'delete_equation', 'at': at, 'equation': number}]))
    if not paragraphs:
        return out
    at, words = paragraphs[0]
    out.append(('edit_table', 'direct', [
        {'op': 'insert_table', 'at': at, 'position': 'before', 'rows': 2, 'columns': 2, 'as': 'alias_grid'},
        {'op': 'edit_table', 'at': '$alias_grid', 'action': {'kind': 'insert_rows', 'row': 1, 'position': 'before', 'count': 2, 'as': 'alias_rows'}},
        {'op': 'edit_table', 'at': '$alias_grid', 'action': {'kind': 'insert_columns', 'column': 1, 'position': 'after', 'count': 2, 'as': 'alias_cols'}},
        {'op': 'format_cell', 'at': '$alias_rows/row:1/cell:2', 'format': {'fill': '#FF0000'}},
        {'op': 'format_cell', 'at': '$alias_cols/row:1/cell:2', 'format': {'fill': '#0000FF'}},
    ]))
    # Tracked tables: an inserted table, inserted and deleted rows, a deleted
    # table, each also settled.
    tracked_grid = {'op': 'insert_table', 'at': at, 'position': 'before', 'rows': 2, 'columns': 2, 'as': 'tgrid', 'cells': [['a', 'b'], ['c', 'd']]}
    for action in ('accept', 'reject', None):
        settle = [{'op': 'revision_settle', 'target': 'all', 'action': action}] if action else []
        out.append(('insert_table', 'track', [tracked_grid] + settle))
        out.append(('edit_table', 'track', [tracked_grid, {'op': 'edit_table', 'at': '$tgrid', 'action': {'kind': 'insert_rows', 'row': 1, 'position': 'after', 'count': 1}}] + settle))
    if '<table' in markup:
        for action in ({'kind': 'insert_rows', 'row': 1, 'position': 'before', 'count': 2}, {'kind': 'delete_rows', 'row': 1, 'count': 1}, {'kind': 'delete'}):
            out.append(('edit_table', 'track', [{'op': 'edit_table', 'at': 'table:1', 'action': action}]))
            out.append(('edit_table', 'track', [{'op': 'edit_table', 'at': 'table:1', 'action': action}, {'op': 'revision_settle', 'target': 'all', 'action': 'reject'}]))
    out.append(('set_section_boundary', 'direct', [{'op': 'set_section_boundary', 'at': at, 'break_type': 'continuous'}]))
    out.append(('set_section_boundary', 'direct', [{'op': 'set_section_boundary', 'at': at}, {'op': 'set_section_layout', 'section_handle': f'section:{at}', 'columns': 2}, {'op': 'set_section_boundary', 'at': at, 'remove': True}]))
    out.append(('insert_table', 'direct', [{'op': 'insert_table', 'at': at, 'position': 'before', 'rows': 2, 'columns': 2, 'as': 'grid'}, {'op': 'edit_table', 'at': '$grid', 'action': {'kind': 'insert_rows', 'row': 1, 'position': 'after', 'count': 1}}]))
    out.append(('insert_table', 'direct', [
        {'op': 'insert_table', 'at': at, 'position': 'after', 'rows': 3, 'columns': 2, 'as': 'filled',
         'cells': [['<b>Name</b>', '<i>Role</i>'], ['<a href="https://example.com/ada">Ada Lovelace</a>', 'line<br>break'], ['Grace']]},
        {'op': 'replace_paragraph', 'at': '$filled_r3c2', 'with': 'Admiral <u>Hopper</u>'},
        {'op': 'insert_paragraph', 'at': '$filled_r1c1', 'position': 'after', 'with': 'second line'},
    ]))
    for action in [
        {'kind': 'insert_columns', 'column': 1, 'position': 'after', 'count': 1},
        {'kind': 'delete_rows', 'row': 1, 'count': 1},
        {'kind': 'delete_columns', 'column': 1, 'count': 1},
        {'kind': 'merge_cells', 'row': 1, 'column': 1, 'row_span': 2, 'column_span': 2},
        {'kind': 'delete'},
    ]:
        out.append(('edit_table', 'direct', [{'op': 'insert_table', 'at': at, 'position': 'before', 'rows': 2, 'columns': 2, 'as': 'grid'}, {'op': 'edit_table', 'at': '$grid', 'action': action}]))
    out.append(('edit_table', 'direct', [{'op': 'insert_table', 'at': at, 'position': 'before', 'rows': 2, 'columns': 2, 'as': 'grid'}, {'op': 'edit_table', 'at': '$grid', 'action': {'kind': 'merge_cells', 'row': 1, 'column': 1, 'row_span': 2, 'column_span': 2}}, {'op': 'edit_table', 'at': '$grid', 'action': {'kind': 'split_cell', 'row': 1, 'column': 1}}]))
    grid = [{'op': 'insert_table', 'at': at, 'position': 'before', 'rows': 2, 'columns': 2, 'as': 'grid'}]
    out.append(('format_table', 'direct', grid + [{'op': 'format_table', 'at': '$grid', 'format': {
        'width': {'unit': 'percent', 'value': 80}, 'column_widths': [120, 180], 'layout': 'fixed', 'alignment': 'center',
        'padding': {'top': 3}, 'borders': {'top': {'style': 'double', 'color': '#123456', 'width': 1}}}}]))
    out.append(('format_row', 'direct', grid + [{'op': 'format_row', 'at': '$grid/row:1', 'format': {
        'height': 24, 'height_rule': 'at_least', 'repeating_header': True, 'allow_split': False}}]))
    out.append(('format_cell', 'direct', grid + [{'op': 'format_cell', 'at': '$grid/row:1/cell:1', 'format': {
        'fill': '#ABCDEF', 'vertical_alignment': 'center', 'padding': {'left': 6}, 'borders': {'bottom': {'style': 'single', 'width': 0.5}}}}]))
    if '<table' in markup:
        for mode in ('track', 'direct'):
            out.append(('format_table', mode, [{'op': 'format_table', 'at': 'table:1', 'format': {'alignment': 'center', 'layout': 'fixed'}}]))
            out.append(('format_row', mode, [{'op': 'format_row', 'at': 'table:1/row:1', 'format': {'repeating_header': True}}]))
            out.append(('format_cell', mode, [{'op': 'format_cell', 'at': 'table:1/row:1/cell:1', 'format': {'fill': '#ABCDEF'}}]))
    other = paragraphs[min(1, len(paragraphs) - 1)][0]
    first, second = words[0], words[1]
    out += [
        ('replace_text', 'track', [{'op': 'replace_text', 'at': at, 'select': first, 'with': first + ' changed', 'occurrence': 1}]),
        ('revise_text', 'track', [{'op': 'revise_text', 'at': at, 'edits': [{'select': first, 'with': 'changed ' + first, 'occurrence': 1}]}]),
        ('replace_paragraph', 'track', [{'op': 'replace_paragraph', 'at': other, 'with': 'Replaced plain text.'}]),
    ]
    for mode in ('track', 'direct'):
        out += [
            ('replace_text', mode, [{'op': 'replace_text', 'at': at, 'select': first, 'with': first + ' <i>new</i>', 'occurrence': 1}]),
            ('revise_text', mode, [{'op': 'revise_text', 'at': at, 'edits': [
                {'select': first, 'with': 'X', 'occurrence': 1}, {'select': second, 'with': 'Y <b>Z</b>', 'occurrence': 1}]}]),
            ('revise_text', mode, [{'op': 'revise_text', 'edits': [{'select': second, 'with': second.upper(), 'all': True}]}]),
            ('replace_paragraph', mode, [{'op': 'replace_paragraph', 'at': other, 'with': 'Replaced <u>text</u>'}]),
            ('format_text', mode, [{'op': 'format_text', 'at': at, 'select': second, 'occurrence': 1, 'bold': True, 'color': '#AA0000'}]),
            ('format_text', mode, [{'op': 'format_text', 'at': at, 'select': second, 'occurrence': 1, 'font_size': 15.0}]),
            ('format_text', mode, [{'op': 'format_text', 'at': at, 'select': second, 'occurrence': 1,
                                   'bold': False, 'italic': False, 'underline': False, 'strike': False, 'superscript': False}]),
            ('format_text', mode, [{'op': 'format_text', 'at': at, 'select': second, 'occurrence': 1,
                                   'clear': ['bold', 'italic', 'underline', 'strike', 'color', 'font_size', 'subscript']}]),
            ('set_list', mode, [{'op': 'set_list', 'at': [at], 'levels': [{'format': 'decimal', 'label': '%1.', 'first_line_indent': -9, 'indent_left': 18, 'marker_bold': True}, {'format': 'lowerLetter', 'label': '%1.%2)', 'restart_after': 0, 'marker_color': '123456'}]}]),
            ('format_paragraph', mode, [{'op': 'format_paragraph', 'at': at, 'indent_left': -12, 'indent_right': -6, 'first_line_indent': -18}]),
            ('format_paragraph', mode, [{'op': 'format_paragraph', 'at': at, 'first_line_indent': 0}]),
            ('format_paragraph', mode, [{'op': 'format_paragraph', 'at': at, 'clear': ['first_line_indent', 'indent_left', 'indent_right']}]),
            ('format_paragraph', mode, [{'op': 'format_paragraph', 'at': at, 'alignment': 'center', 'space_after': 6.0,
                                         'line_spacing': {'mode': 'multiple', 'value': 1.5}}]),
            ('insert_paragraph', mode, [{'op': 'insert_paragraph', 'at': at, 'position': 'after', 'with': 'Inserted <b>text</b>', 'as': '$new'},
                                        {'op': 'replace_text', 'at': '$new', 'select': 'Inserted', 'with': 'Added'}]),
            ('insert_paragraph', mode, [{'op': 'insert_paragraph', 'at': at, 'position': 'after', 'with': ['One', '<i>Two</i>', 'Three'], 'as': 'many'},
                                        {'op': 'insert_paragraph', 'at': at, 'position': 'before', 'with': ['Before one', 'Before two']},
                                        {'op': 'replace_text', 'at': '$many_2', 'select': 'Two', 'with': 'Second'},
                                        {'op': 'insert_paragraph', 'at': '$many', 'position': 'after', 'with': 'After the group'}]),
            ('set_list', mode, [{'op': 'insert_paragraph', 'at': at, 'position': 'after', 'with': ['Item one', 'Item two'], 'as': 'items'},
                                {'op': 'set_list', 'at': ['$items'], 'format': 'bullet'}]),
            ('set_list', mode, [{'op': 'set_list', 'at': [at], 'format': 'bullet', 'level': 1}]),
            ('set_list', mode, [{'op': 'set_list', 'at': [at], 'format': 'decimal', 'start': 4}]),
            ('set_list', mode, [{'op': 'set_list', 'at': [at], 'remove': True}]),
            ('delete_paragraphs', mode, [{'op': 'delete_paragraphs', 'at': [other]}]),
            ('comment_add', mode, [{'op': 'comment_add', 'at': at, 'select': second, 'occurrence': 1, 'text': 'Check this'}]),
        ]
        # Bookmarks apply directly in both modes; later edits must keep them paired.
        mark = {'op': 'bookmark_add', 'at': at, 'name': 'DocxdriverConformance', 'select': second, 'occurrence': 1}
        whole = {'op': 'bookmark_add', 'at': other, 'name': 'DocxdriverWhole'}
        out += [
            ('bookmark_add', mode, [mark]),
            ('bookmark_add', mode, [whole]),
            ('bookmark_rename', mode, [mark, {'op': 'bookmark_rename', 'name': 'DocxdriverConformance', 'new_name': 'DocxdriverRenamed'}]),
            ('bookmark_move', mode, [mark, {'op': 'bookmark_move', 'name': 'DocxdriverConformance', 'at': other}]),
            ('bookmark_move', mode, [whole, {'op': 'bookmark_move', 'name': 'DocxdriverWhole', 'at': at, 'select': first, 'occurrence': 1}]),
            ('bookmark_delete', mode, [mark, {'op': 'bookmark_delete', 'name': 'DocxdriverConformance'}]),
            ('replace_text', mode, [mark, {'op': 'replace_text', 'at': at, 'select': second, 'with': 'Bookmarked', 'occurrence': 1}]),
            ('revise_text', mode, [mark, {'op': 'revise_text', 'at': at, 'edits': [{'select': second, 'with': 'Y', 'occurrence': 1}]}]),
            ('replace_paragraph', mode, [mark, {'op': 'replace_paragraph', 'at': at, 'with': 'Replaced <b>bookmarked</b>'}]),
            ('delete_paragraphs', mode, [whole, {'op': 'delete_paragraphs', 'at': [other]}]),
        ]
    # Rewriting, removing and settling the plan author's own pending changes.
    out += [
        ('revise_text', 'track', [
            {'op': 'replace_text', 'at': at, 'select': first, 'with': first + ' Zqx', 'occurrence': 1},
            {'op': 'revise_text', 'at': at, 'edits': [{'select': 'Zqx', 'with': 'Zqx Zqy'}]},
            {'op': 'revise_text', 'at': at, 'edits': [{'select': 'x Zq', 'with': 'xZ'}]},
            {'op': 'revise_text', 'at': at, 'edits': [{'select': ' ZqxZy', 'with': ''}]}]),
        ('delete_paragraphs', 'track', [
            {'op': 'insert_paragraph', 'at': at, 'position': 'after', 'with': 'Own pending paragraph', 'as': 'mine'},
            {'op': 'delete_paragraphs', 'at': ['$mine']}]),
        ('revision_settle', 'track', [
            {'op': 'insert_paragraph', 'at': at, 'position': 'after', 'with': 'Rejected paragraph'},
            {'op': 'revision_settle', 'target': 'all', 'action': 'reject'}]),
        ('revision_settle', 'track', [
            {'op': 'delete_paragraphs', 'at': [at]},
            {'op': 'revision_settle', 'target': 'all', 'action': 'accept'}]),
    ]
    # Tracked edits converted to amendment formatting (underline / strike).
    out.append(('convert_revisions_to_formatting', 'track', [
        {'op': 'replace_text', 'at': at, 'select': first, 'with': first + ' amended', 'occurrence': 1},
        {'op': 'insert_paragraph', 'at': at, 'position': 'after', 'with': 'Added paragraph'},
        {'op': 'delete_paragraphs', 'at': [other]},
        {'op': 'convert_revisions_to_formatting', 'target': 'all'}]))
    out.append(('convert_revisions_to_formatting', 'direct', [{'op': 'convert_revisions_to_formatting', 'target': 'all'}]))
    out.append(('revision_settle', 'track', [
        {'op': 'replace_text', 'at': at, 'select': first, 'with': 'Tracked', 'occurrence': 1},
        {'op': 'revision_settle', 'target': 'all', 'action': 'reject'}]))
    for action in ('accept', 'reject'):
        for formatting in (
            {'op': 'format_text', 'at': at, 'select': second, 'occurrence': 1, 'color': '#123456'},
            {'op': 'format_paragraph', 'at': at, 'alignment': 'center'},
        ):
            out.append(('revision_settle', 'track', [formatting,
                {'op': 'revision_settle', 'target': 'all', 'action': action}]))
    return out


def comment_chain(cli, name):
    """comment_add, then reply / status / delete against the new comment id."""
    read = cli.read(name)
    paragraphs = text_paragraphs(read['markup']) if read else []
    if not paragraphs:
        return []
    at, words = paragraphs[0]
    if cli.commit(name, [{'op': 'comment_add', 'at': at, 'select': words[0], 'occurrence': 1, 'text': 'Thread'}], 'commented.docx'):
        return []
    listing = cli.read('commented.docx', 'comments') or {}
    ids = [str(comment['id']) for comment in listing.get('comments', []) if (comment.get('root') or {}).get('text') == 'Thread']
    if not ids:
        return []
    comment_id = ids[-1]
    return [
        ('comment_reply', [{'op': 'comment_reply', 'comment_id': comment_id, 'text': 'Reply'}]),
        ('comment_set_status', [{'op': 'comment_set_status', 'comment_id': comment_id, 'status': 'resolved'}]),
        ('comment_delete', [{'op': 'comment_delete', 'comment_id': comment_id}]),
    ]


def unmasked(introduced, removed):
    """Drop schema content-model errors an edit merely uncovered.

    Validators report a content-model error and skip the rest of
    it, so repairing an invalid source element (format_table adding the
    missing tblPr of an invalid table) exposes pre-existing errors further
    down the same part. A new "not expected" schema error counts only when
    the edit did not also remove one from that part.
    """
    def content_model(message):
        if message.startswith('sdk Sch_UnexpectedElementContentExpectingComplex:'):
            return 'sdk'
        if not message.startswith(('integrity:', 'sdk')) and 'This element is not expected' in message:
            return 'schema'
        return None
    repaired = {(part, content_model(message)) for part, message in removed if content_model(message)}
    # A tracked edit keeps the old invalid properties in pPrChange. The SDK
    # still counts that snapshot's error, even when XSD reports the repair in
    # the live properties; its newly exposed ordering error has the same cause.
    repaired.update((part, 'sdk') for part, kind in list(repaired) if kind == 'schema')
    return {(part, message) for part, message in introduced if (part, content_model(message)) not in repaired}


_process_validator = None


def check_document(job):
    """All scenarios for one source, with private package outputs/validator."""
    global _process_validator
    document, binary, keep = job
    if _process_validator is None:
        _process_validator = Validator()
    validator = _process_validator
    work = Path(tempfile.mkdtemp(prefix='docxdriver-conformance-'))
    stem = f'{document.stem}-{hashlib.sha256(str(document).encode()).hexdigest()[:6]}'
    cli = Cli(binary, work, keep, stem)
    failures, applied, rejected = [], collections.Counter(), collections.Counter()
    try:
        name = 'source.docx'
        shutil.copyfile(document, work / name)
        if keep:
            shutil.copyfile(document, Path(keep) / f'{stem}--source.docx')
        read = cli.read(name)
        if read is None:
            return failures, applied, rejected
        before = validator.errors((work / name).read_bytes())
        cases = scenarios(read) + [(label, 'track', ops) for label, ops in comment_chain(cli, name)]
        for label, mode, ops in cases:
            source = 'commented.docx' if label in ('comment_reply', 'comment_set_status', 'comment_delete') else name
            reason = cli.commit(source, ops, 'out.docx', mode)
            if reason:
                rejected[label] += 1
                continue
            applied[label] += 1
            baseline = before if source == name else validator.errors((work / source).read_bytes())
            after = validator.errors((work / 'out.docx').read_bytes())
            # Compare introduced kinds, but retain counts when detecting a
            # repair: another occurrence of the old error may still remain.
            for kind, message in sorted(unmasked(after.keys() - baseline.keys(), baseline - after)):
                failures.append(f'{document.name}: {label} ({mode}) introduced {kind}: {message[:240]}')
        return failures, applied, rejected
    finally:
        shutil.rmtree(work, ignore_errors=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--cli', default=str(ROOT / 'target' / 'debug' / 'docxdriver'))
    parser.add_argument('--corpus', help='directory searched recursively for .docx inputs')
    parser.add_argument('--limit', type=int, default=0, help='use at most N corpus documents')
    parser.add_argument('--jobs', type=int, default=1, help='independent document workers (default: 1)')
    parser.add_argument('--keep-outputs', metavar='DIR', help='save every committed package to DIR (for scripts/word-smoke.py)')
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error('--jobs must be positive')
    problem = build_sdk_validator()
    if problem:
        print('FAIL', problem)
        return 1
    keep = str(Path(args.keep_outputs).resolve()) if args.keep_outputs else None
    if keep:
        Path(keep).mkdir(parents=True, exist_ok=True)

    validator = Validator()
    work = Path(tempfile.mkdtemp(prefix='docxdriver-conformance-'))
    cli = Cli(str(Path(args.cli).resolve()), work)
    failures, applied, rejected = [], collections.Counter(), collections.Counter()
    try:
        created = cli.run('create', 'synthetic.docx', '--html', SYNTHETIC_HTML)
        if created.get('outcome') != 'completed':
            failures.append(f'create failed: {created}')
            inputs = []
        else:
            for kind, message in validator.errors((work / 'synthetic.docx').read_bytes()):
                failures.append(f'created document is invalid: {kind}: {message}')
            applied['create'] += 1
            inputs = [work / 'synthetic.docx']
            rich = rich_formatting_document((work / 'synthetic.docx').read_bytes())
            (work / 'rich-formatting.docx').write_bytes(rich)
            for kind, message in validator.errors(rich):
                failures.append(f'rich formatting fixture is invalid: {kind}: {message}')
            inputs.append(work / 'rich-formatting.docx')
        inputs += [ROOT / fixture for fixture in FIXTURES]
        if args.corpus:
            corpus = sorted(Path(args.corpus).rglob('*.docx'))
            inputs += corpus[: args.limit] if args.limit else corpus

        jobs = [(document, str(Path(args.cli).resolve()), keep) for document in inputs]
        if args.jobs == 1:
            results = map(check_document, jobs)
            for failures_here, applied_here, rejected_here in results:
                failures.extend(failures_here)
                applied.update(applied_here)
                rejected.update(rejected_here)
        else:
            with ProcessPoolExecutor(max_workers=args.jobs) as pool:
                for count, (failures_here, applied_here, rejected_here) in enumerate(pool.map(check_document, jobs), 1):
                    failures.extend(failures_here)
                    applied.update(applied_here)
                    rejected.update(rejected_here)
                    if count % 100 == 0:
                        print(f'{count}/{len(inputs)} documents checked', file=sys.stderr, flush=True)
    finally:
        shutil.rmtree(work, ignore_errors=True)

    uncovered = [op for op in schema_ops() if applied[op] == 0]
    for op in uncovered:
        failures.append(f'operation {op} was never applied; extend the scenarios')
    print(f'{len(inputs)} documents')
    for op in sorted(set(applied) | set(rejected)):
        print(f'  {op:28} applied {applied[op]:5}  rejected {rejected[op]:5}')
    for failure in failures:
        print('FAIL', failure)
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
