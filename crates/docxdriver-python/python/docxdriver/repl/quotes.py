"""Native equivalents of the Pi quotation resolver, inline lint, and audit pipeline."""
from collections import Counter
import hashlib
import html
import json
import re

from docxdriver import run_command, run_plan

AUTHOR = 'docxdriver quotation audit'
PREFIX = 'qw1:sha256:'


def compact(value):
    return json.dumps(value, ensure_ascii=False, allow_nan=False, separators=(',', ':'))


def digest(value):
    return hashlib.sha256(value.encode()).hexdigest()


def occurrence_start(text, select, occurrence=1):
    if type(occurrence) is not int or occurrence < 1:
        raise ValueError('occurrence must be a positive integer')
    if not isinstance(select, str) or not select:
        raise ValueError('select must be a non-empty string')
    start = -len(select)
    for _ in range(occurrence):
        start = text.find(select, start + len(select))
        if start < 0:
            raise ValueError(f'occurrence {occurrence} was not found exactly')
    return start


def utf16_offset(text, offset):
    return len(text[:offset].encode('utf-16-le')) // 2


def resolve_quote(spec, paragraph, source_hash):
    for name in ('source', 'at', 'select'):
        if not isinstance(spec.get(name), str) or not spec[name]:
            raise ValueError(f'quote {name} must be a non-empty string')
    occurrence = spec.get('occurrence', 1)
    start = occurrence_start(paragraph, spec['select'], occurrence)
    changes = []
    for change in spec.get('changes', []):
        if change.get('kind') not in ('omit', 'bracket'):
            raise ValueError('change kind must be omit or bracket')
        offset = occurrence_start(spec['select'], change.get('select'), change.get('occurrence', 1))
        end = offset + len(change['select'])
        if change['kind'] == 'omit':
            rendered = change.get('marker', '…')
            if rendered not in ('…', '...'):
                raise ValueError('omission marker must be ... or …')
        else:
            replacement = change.get('replacement')
            if not isinstance(replacement, str) or not replacement or '[' in replacement or ']' in replacement:
                raise ValueError('bracket replacement must be non-empty and contain no brackets')
            rendered = f'[{replacement}]'
        changes.append({'kind': change['kind'], 'start': offset, 'end': end, 'original': change['select'], 'rendered': rendered})
    changes.sort(key=lambda item: (item['start'], item['end']))
    if any(b['start'] < a['end'] for a, b in zip(changes, changes[1:])):
        raise ValueError('changes overlap')
    content, cursor = '', 0
    for change in changes:
        content += spec['select'][cursor:change['start']] + change['rendered']
        cursor = change['end']
    content += spec['select'][cursor:]
    style = spec.get('style', 'double')
    if style not in ('double', 'single', 'none'):
        raise ValueError('quote style must be double, single, or none')
    text = f'“{content}”' if style == 'double' else f'‘{content}’' if style == 'single' else content
    for change in changes:
        change['start'], change['end'] = utf16_offset(spec['select'], change['start']), utf16_offset(spec['select'], change['end'])
    record = {'text': text, 'content': content, 'style': style, 'source': spec['source'], 'source_sha256': source_hash,
              'at': spec['at'], 'paragraph_sha256': digest(paragraph),
              'span': {'start': utf16_offset(paragraph, start), 'end': utf16_offset(paragraph, start + len(spec['select'])), 'occurrence': occurrence},
              'original': spec['select'], 'changes': changes}
    return {'quote_id': 'q1:sha256:' + digest(compact(record)), 'text_sha256': digest(text), **record}


def lint_quotes(text, severity='error'):
    if not isinstance(text, str) or severity not in ('warning', 'error'):
        raise ValueError('quote_lint requires text and warning/error severity')
    diagnostics = []
    def diagnostic(start, end, code='unverified_quote_mark', message='quotation mark appears in unstructured text; use a verified Quote or a structured Term'):
        diagnostics.append({'code': code, 'severity': severity, 'message': message,
            'start': utf16_offset(text, start), 'end': utf16_offset(text, end),
            'excerpt': text[max(0, start - 24):end + 24]})
    index, tag_start = 0, None
    entities = ('&quot;', '&#34;', '&#x22;', '&ldquo;', '&rdquo;', '&lsquo;', '&rsquo;')
    while index < len(text):
        char = text[index]
        if tag_start is not None:
            if char == '>':
                tag_start = None
        elif char == '<':
            tag_start = index
        else:
            entity = next((entity for entity in entities if text.startswith(entity, index)), None)
            if entity:
                diagnostic(index, index + len(entity))
                index += len(entity) - 1
            elif char in '"“”„‟«»‘‹':
                diagnostic(index, index + 1)
        index += 1
    if tag_start is not None:
        diagnostic(tag_start, len(text), 'unterminated_inline_tag', 'unterminated inline tag prevents reliable quotation linting')
    for match in re.finditer(r"(^|[\s(\[])'([^'\n]+)'(?=$|[\s.,;:!?\)\]])", text):
        start = match.start() + len(match[1])
        prefix = text[:start]
        if prefix.rfind('<') <= prefix.rfind('>'):
            diagnostic(start, match.end())
    return sorted(diagnostics, key=lambda item: (item['start'], item['end']))


def render_term(spec):
    text = spec.get('text')
    if not isinstance(text, str) or not text:
        raise ValueError('term text must be a non-empty string')
    if lint_quotes(text):
        raise ValueError('term text must not contain quotation marks')
    style = spec.get('style', 'double')
    if style not in ('double', 'single'):
        raise ValueError('term style must be double or single')
    return {'text': f'“{text}”' if style == 'double' else f'‘{text}’', 'kind': 'term', 'style': style}


def compile_inline(parts, policy, resolver):
    if not isinstance(parts, list) or policy not in ('error', 'comment'):
        raise ValueError('Inline.parts must be a list and policy must be error or comment')
    rendered, diagnostics, quote_ids = [], [], []
    terms, previous_terminal = 0, False
    for index, value in enumerate(parts):
        if isinstance(value, str):
            severity = 'error' if policy == 'error' else 'warning'
            diagnostics.extend({**item, 'part': index} for item in lint_quotes(value, severity))
            if previous_terminal and re.match(r'[.!?]', value):
                diagnostics.append({'code': 'duplicate_punctuation_after_quote', 'severity': severity,
                    'message': 'structured quote already contains terminal punctuation; remove the following punctuation',
                    'start': 0, 'end': 1, 'excerpt': value[:25], 'part': index})
            if value:
                previous_terminal = False
            rendered.append(value)
        elif isinstance(value, dict) and value.get('$kind') == 'term':
            rendered.append(render_term(value)['text'])
            terms += 1
            previous_terminal = False
        elif isinstance(value, dict) and value.get('$kind') == 'quote':
            quote = resolver(value)
            rendered.append(quote['text'])
            quote_ids.append(quote['quote_id'])
            previous_terminal = bool(re.search(r'[.!?][”’]$', quote['text']))
        else:
            raise ValueError(f'Inline part {index} must be text, Quote, or Term')
    if policy == 'error' and diagnostics:
        item = diagnostics[0]
        raise ValueError(f"{item['message']} (Inline part {item['part']}, offsets {item['start']}-{item['end']})")
    return {'text': ''.join(rendered), 'audit': {'status': 'warnings' if diagnostics else 'verified', 'policy': policy,
        'diagnostics': diagnostics, 'verified_quotes': len(quote_ids), 'terms': terms, 'quote_ids': quote_ids}}


def contains_structured(value):
    if isinstance(value, list):
        return any(contains_structured(item) for item in value)
    if isinstance(value, dict):
        return value.get('$kind') in ('quote', 'term', 'inline') or any(contains_structured(item) for item in value.values())
    return False


def resolve_plan(state, resolver):
    bindings, trusted = {}, []
    def quote(spec):
        resolved, binding = resolver(spec)
        bindings[(binding['path'], binding['hash'])] = binding
        trusted.append({'kind': 'quote', 'text': resolved['text'], 'quote_id': resolved['quote_id']})
        return resolved
    ops = []
    for authored in state['plan']['ops']:
        operation = dict(authored) if isinstance(authored, dict) else authored
        node = operation.get('with') if isinstance(operation, dict) else None
        if isinstance(node, dict) and '$kind' in node:
            kind = node['$kind']
            if kind == 'quote':
                operation['with'] = quote(node)['text']
            elif kind == 'term':
                text = render_term(node)['text']
                trusted.append({'kind': 'term', 'text': text})
                operation['with'] = text
            elif kind == 'inline':
                operation['with'] = compile_inline(node.get('parts'), 'error', quote)['text']
                trusted.extend({'kind': 'term', 'text': render_term(part)['text']} for part in node['parts']
                               if isinstance(part, dict) and part.get('$kind') == 'term')
            else:
                raise ValueError(f'unknown structured inline kind {kind}')
        ops.append(operation)
    return {**state, 'plan': {**state['plan'], 'ops': ops}}, list(bindings.values()), trusted


def completed(report, count):
    return isinstance(report, dict) and report.get('completed') == count and report.get('stopped_at') is None \
        and isinstance(report.get('ops'), list) and len(report['ops']) == count \
        and all(isinstance(op, dict) and op.get('outcome') == 'applied' for op in report['ops'])


def result_of(output):
    if output.get('outcome') != 'completed' or not isinstance(output.get('result'), dict):
        raise ValueError(output.get('diagnostic', {}).get('message', 'document read failed'))
    return output['result']


def decode_text(markup):
    return html.unescape(re.sub(r'<[^>]+>', '', re.sub(r'<br\s*/?>', '\n', markup, flags=re.I)))


def inspect_doc(data):
    markup = result_of(run_command(data, 'read', view='final'))['markup']
    comments = result_of(run_command(data, 'read', read_kind='comments')).get('comments', [])
    paragraphs = []
    for match in re.finditer(r'<(p|h[1-6])\b([^>]*)>(.*?)</\1>', markup, re.I | re.S):
        address = re.search(r'\bid="([0-9A-Fa-f]{8})"', match[2])
        if address:
            paragraphs.append({'index': len(paragraphs) + 1, 'id': address[1].upper(), 'text': decode_text(match[3])})
    return paragraphs, comments, markup


def protected(comments):
    return [thread for thread in comments if thread.get('root', {}).get('author') == AUTHOR
            or f'[{PREFIX}' in str(thread.get('root', {}).get('text', ''))]


def protected_mutations(data, operations):
    ids = {str(thread.get('id', '')) for thread in protected(inspect_doc(data)[1])}
    return sorted({str(op.get('comment_id', '')) for op in operations if isinstance(op, dict)
                   and op.get('op') in ('comment_delete', 'comment_reply', 'comment_set_status')
                   and str(op.get('comment_id', '')) in ids})


def occurrences(text, select):
    if not select:
        return []
    found, start = [], 0
    while start <= len(text) - len(select):
        position = text.find(select, start)
        if position < 0:
            break
        found.append(position)
        start = position + len(select)
    return found


def unverified_spans(paragraph):
    text, ranges = paragraph['text'], []
    patterns = (r'“[^”\n]+”', r'"[^"\n]+"', r'«[^»\n]+»', r'‘[^’\n]+’', r"(^|[\s(\[])'[^'\n]+'(?=$|[\s.,;:!?\)\]])")
    for index, pattern in enumerate(patterns):
        for match in re.finditer(pattern, text):
            start = match.start() + (len(match[1]) if index == 4 else 0)
            end = match.end()
            if not any(start < b and end > a for a, b in ranges):
                ranges.append((start, end))
    for index, char in enumerate(text):
        if char in '"“”«»‘’' and not any(a <= index < b for a, b in ranges):
            ranges.append((index, index + 1))
    spans = []
    for start, end in sorted(ranges):
        select = text[start:end]
        occurrence = occurrences(text, select).index(start) + 1
        spans.append({'paragraph_id': paragraph['id'], 'select': select, 'occurrence': occurrence,
                      'locator': f"p{paragraph['index']}:{start}-{end}"})
    return spans


def warning(span):
    identifier = PREFIX + digest(compact([span['paragraph_id'], span['select'], span['occurrence']]))
    return {'warning_id': identifier, 'author': AUTHOR, 'text':
        f'UNVERIFIED QUOTATION — source provenance could not be confirmed. Review before relying on this text. [{identifier}]'}


def apply_audit(data, trusted=()):
    paragraphs, comments, markup = inspect_doc(data)
    chrome = re.sub(r'<comments\b[^>]*>.*?</comments>', '', markup, flags=re.I | re.S)
    chrome = decode_text(re.sub(r'<(p|h[1-6])\b[^>]*\bid="[0-9A-Fa-f]{8}"[^>]*>.*?</\1>', '', chrome, flags=re.I | re.S)).strip()
    expected = Counter(item['text'] for item in trusted if unverified_spans({'index': 0, 'id': 'TRUSTED', 'text': item['text']}))
    trusted_keys = set()
    for text, count in expected.items():
        matches = [(paragraph, index + 1) for paragraph in paragraphs for index, _ in enumerate(occurrences(paragraph['text'], text))]
        chrome_matches = occurrences(chrome, text)
        if len(matches) + len(chrome_matches) != count:
            raise ValueError(f'structured quotation rendering is missing or ambiguous in the committed candidate: {text}')
        trusted_keys.update((paragraph['id'], text, occurrence) for paragraph, occurrence in matches)
        for start in reversed(chrome_matches):
            chrome = chrome[:start] + ' ' * len(text) + chrome[start + len(text):]
    if unverified_spans({'index': 0, 'id': 'UNADDRESSABLE', 'text': chrome}):
        raise ValueError('quotation audit found quotation marks in content that cannot carry a comment anchor')
    spans = [span for paragraph in paragraphs for span in unverified_spans(paragraph)
             if (span['paragraph_id'], span['select'], span['occurrence']) not in trusted_keys]
    ops = [{'op': 'comment_delete', 'comment_id': str(thread['id'])} for thread in protected(comments) if str(thread.get('id', '')).isdigit()]
    ops.extend({'op': 'comment_add', 'at': span['paragraph_id'], 'select': span['select'], 'occurrence': span['occurrence'], 'text': warning(span)['text']} for span in spans)
    if ops:
        plan = {'base': 'sha256:' + hashlib.sha256(data).hexdigest(), 'author': AUTHOR, 'change_mode': 'direct', 'ops': ops}
        preview = run_plan(data, plan)
        if preview.get('outcome') != 'previewed' or not completed(preview.get('report'), len(ops)):
            raise ValueError('quote audit comment preview failed')
        commit = run_plan(data, plan, preview['preview_key'])
        if commit.get('outcome') != 'committed' or not completed(commit.get('report'), len(ops)):
            raise ValueError('quote audit comment commit failed')
        data = commit['bytes']
    final = protected(inspect_doc(data)[1])
    if len(final) != len(spans):
        raise ValueError('quote audit candidate contains missing, duplicate, or stale protected comments')
    for span in spans:
        expected = warning(span)
        matches = [thread for thread in final if thread.get('status') == 'open'
                   and thread.get('root', {}).get('author') == expected['author'] and thread.get('root', {}).get('text') == expected['text']
                   and any(item.get('locator') == span['locator'] for item in thread.get('anchor', {}).get('ranges', []))]
        if len(matches) != 1:
            raise ValueError(f"quote audit coverage failed for {expected['warning_id']}: expected {span['locator']}")
    return data, len(spans)
