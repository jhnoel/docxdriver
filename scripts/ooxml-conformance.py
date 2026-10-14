#!/usr/bin/env python3
"""Differential ECMA-376 conformance gate for docxdriver edit operations.

Every typed edit operation in the core schema is applied, through a real
preview-and-commit plan, to every input document. Each committed package is
validated against the bundled ECMA-376 Transitional and OPC schemas, and the
gate fails when an operation introduces an error kind the source did not
already have. Pre-existing invalid content is not docxdriver's to fix, and a
run split that copies an invalid property is not a new kind of error.

The gate also fails unless every operation in the schema was applied at
least once, and unless a freshly created document validates with no errors.

Inputs are a rich synthetic document built with `create`, the repository's
DOCX fixtures, and optionally a corpus directory (scripts/fetch-corpus.sh):

    cargo build -p docxdriver-cli
    python scripts/ooxml-conformance.py [--corpus corpus/lo] [--limit N]

Requires lxml.
"""
import argparse
import collections
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
    '<ol><li>First obligation of the parties.</li><li>Second obligation of the parties.</li></ol>'
    '<table><tr><td>Fee schedule</td><td>Amount due</td></tr><tr><td>Initial fee</td><td>5000 dollars</td></tr></table>'
    '<p>The area is <math display="inline"><msup><mi>r</mi><mn>2</mn></msup></math> times pi in each region.</p>'
    '<p>Either party may terminate this agreement with written notice.</p>'
)


class Validator:
    def __init__(self):
        self.paths = {}
        for path in SCHEMA_DIR.glob('*.xsd'):
            namespace = etree.parse(str(path), PARSER).getroot().get('targetNamespace')
            if namespace:
                self.paths[namespace] = path
        self.compiled = {}

    def errors(self, data):
        """Error kinds per part, after ECMA-376 Part 3 markup-compatibility processing."""
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
    def __init__(self, binary, work):
        self.binary, self.work = binary, work

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
        ('set_even_and_odd_headers', 'track', [{'op': 'set_even_and_odd_headers', 'even_and_odd': True}]),
        ('set_header', 'track', [{'op': 'set_header', 'kind': 'even', 'with': '<p>Even <b>header</b></p>'}]),
        ('set_footer', 'track', [{'op': 'set_footer', 'with': '<p>Page footer</p>'}]),
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
    other = paragraphs[min(1, len(paragraphs) - 1)][0]
    first, second = words[0], words[1]
    for mode in ('track', 'direct'):
        out += [
            ('replace_text', mode, [{'op': 'replace_text', 'at': at, 'select': first, 'with': first + ' <i>new</i>', 'occurrence': 1}]),
            ('revise_text', mode, [{'op': 'revise_text', 'at': at, 'edits': [
                {'select': first, 'with': 'X', 'occurrence': 1}, {'select': second, 'with': 'Y <b>Z</b>', 'occurrence': 1}]}]),
            ('revise_text', mode, [{'op': 'revise_text', 'edits': [{'select': second, 'with': second.upper(), 'all': True}]}]),
            ('replace_paragraph', mode, [{'op': 'replace_paragraph', 'at': other, 'with': 'Replaced <u>text</u>'}]),
            ('format_text', mode, [{'op': 'format_text', 'at': at, 'select': second, 'occurrence': 1, 'bold': True, 'color': '#AA0000'}]),
            ('format_paragraph', mode, [{'op': 'format_paragraph', 'at': at, 'alignment': 'center', 'space_after': 6.0,
                                         'line_spacing': {'mode': 'multiple', 'value': 1.5}}]),
            ('insert_paragraph', mode, [{'op': 'insert_paragraph', 'at': at, 'position': 'after', 'with': 'Inserted <b>text</b>', 'as': '$new'},
                                        {'op': 'replace_text', 'at': '$new', 'select': 'Inserted', 'with': 'Added'}]),
            ('delete_paragraphs', mode, [{'op': 'delete_paragraphs', 'at': [other]}]),
            ('comment_add', mode, [{'op': 'comment_add', 'at': at, 'select': second, 'occurrence': 1, 'text': 'Check this'}]),
        ]
    out.append(('revision_settle', 'track', [
        {'op': 'replace_text', 'at': at, 'select': first, 'with': 'Tracked', 'occurrence': 1},
        {'op': 'revision_settle', 'target': 'all', 'action': 'reject'}]))
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


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--cli', default=str(ROOT / 'target' / 'debug' / 'docxdriver'))
    parser.add_argument('--corpus', help='directory searched recursively for .docx inputs')
    parser.add_argument('--limit', type=int, default=0, help='use at most N corpus documents')
    args = parser.parse_args()

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
        inputs += [ROOT / fixture for fixture in FIXTURES]
        if args.corpus:
            corpus = sorted(Path(args.corpus).rglob('*.docx'))
            inputs += corpus[: args.limit] if args.limit else corpus

        for document in inputs:
            name = 'source.docx'
            if document != work / name:
                shutil.copyfile(document, work / name)
            read = cli.read(name)
            if read is None:
                continue
            before = set(validator.errors((work / name).read_bytes()))
            cases = scenarios(read) + [(label, 'track', ops) for label, ops in comment_chain(cli, name)]
            for label, mode, ops in cases:
                source = 'commented.docx' if label in ('comment_reply', 'comment_set_status', 'comment_delete') else name
                reason = cli.commit(source, ops, 'out.docx', mode)
                if reason:
                    rejected[label] += 1
                    continue
                applied[label] += 1
                baseline = before if source == name else set(validator.errors((work / source).read_bytes()))
                for kind, message in sorted(set(validator.errors((work / 'out.docx').read_bytes())) - baseline):
                    failures.append(f'{document.name}: {label} ({mode}) introduced {kind}: {message[:240]}')
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
