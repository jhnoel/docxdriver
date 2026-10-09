"""Independent DOCX verification; does not inspect either projection's markup."""
import json
import sys
import zipfile
import xml.etree.ElementTree as ET

W = '{http://schemas.openxmlformats.org/wordprocessingml/2006/main}'
M = '{http://schemas.openxmlformats.org/officeDocument/2006/math}'

def parts(path):
    with zipfile.ZipFile(path) as z:
        return {name: z.read(name) for name in z.namelist()}

def paragraphs(data):
    root = ET.fromstring(data['word/document.xml'])
    # delete_paragraphs retains tracked source XML even in a direct plan.
    # A fully deleted paragraph and its deleted terminating break disappear
    # from final content. Keep the source XML available for collateral checks.
    return [p for p in root.find(W + 'body').iter(W + 'p')
            if not (p.find('./' + W + 'pPr/' + W + 'rPr/' + W + 'del') is not None
                    and not text(p))]

def text(p):
    return ''.join(n.text or '' for n in p.iter(W + 't'))

def semantic_xml(node):
    # xml:space is immaterial when a text node has no boundary whitespace.
    clone = ET.fromstring(ET.tostring(node))
    for t in clone.iter(W + 't'):
        if (t.text or '') == (t.text or '').strip():
            t.attrib.pop('{http://www.w3.org/XML/1998/namespace}space', None)
    return ET.tostring(clone)

def styled(p, phrase, props):
    letters = []
    for r in p.iter(W + 'r'):
        rp = r.find(W + 'rPr')
        active = set()
        if rp is not None:
            for prop in props:
                node = rp.find(W + prop)
                if node is not None and node.get(W + 'val', '1') not in ('0', 'false', 'off', 'none'):
                    active.add(prop)
        letters.extend((c, active) for c in text(r))
    full = ''.join(c for c, _ in letters)
    start = full.index(phrase)
    assert all(set(props) <= active for _, active in letters[start:start + len(phrase)]), 'Required formatting missing'

def plain_paragraph(p, data):
    styles = {}
    default = None
    if 'word/styles.xml' in data:
        root = ET.fromstring(data['word/styles.xml'])
        styles = {s.get(W + 'styleId'): s for s in root.findall(W + 'style')}
        default = next((s.get(W + 'styleId') for s in styles.values()
                        if s.get(W + 'type') == 'paragraph' and s.get(W + 'default') in ('1', 'true')), None)
    def apply(state, rpr):
        if rpr is not None:
            for prop in ('b', 'i', 'u'):
                n = rpr.find(W + prop)
                if n is not None: state[prop] = n.get(W + 'val', '1') not in ('0', 'false', 'off', 'none')
        return state
    def style_state(name, seen=None):
        seen = set() if seen is None else seen
        if name not in styles or name in seen: return {}
        seen.add(name); s = styles[name]; parent = s.find(W + 'basedOn')
        state = style_state(parent.get(W + 'val'), seen) if parent is not None else {}
        return apply(state, s.find(W + 'rPr'))
    pstyle = p.find('./' + W + 'pPr/' + W + 'pStyle')
    base = style_state(pstyle.get(W + 'val') if pstyle is not None else default)
    for r in p.iter(W + 'r'):
        if not text(r): continue
        state = base.copy(); rpr = r.find(W + 'rPr')
        rstyle = rpr.find(W + 'rStyle') if rpr is not None else None
        if rstyle is not None: state.update(style_state(rstyle.get(W + 'val')))
        if any(apply(state, rpr).values()): return False
    # Normal font/color metadata, Latin-inactive bCs/iCs and bookmark milestones
    # do not make visible plain text bold/italic/underlined.
    special = ('hyperlink', 'fldSimple', 'fldChar', 'instrText', 'drawing', 'pict', 'footnoteReference', 'endnoteReference', 'commentReference')
    return not any(n.tag in {W + tag for tag in special} or n.tag == M + 'oMath' for n in p.iter())

def verify(task, before_path, after_path):
    before, after = parts(before_path), parts(after_path)
    bp, ap = paragraphs(before), paragraphs(after)
    bt, at = [text(p) for p in bp], [text(p) for p in ap]
    assert before.keys() == after.keys(), 'ZIP package parts changed'
    for name in before:
        if name != 'word/document.xml':
            assert before[name] == after[name], f'Unrelated part changed: {name}'
    notice = 'The notice period is thirty days.'
    if task in ('text', 'repair'):
        i = bt.index(notice)
        expected = bt.copy(); expected[i] = 'The notice period is sixty days.'
        assert at == expected, 'Unexpected text changes'
        styled(ap[i], 'sixty days', ['b'])
        if task == 'text':
            expected_xml = ET.fromstring(before['word/document.xml'])
            for node in expected_xml.iter(W + 't'):
                if node.text == 'thirty days': node.text = 'sixty days'
            assert semantic_xml(expected_xml) == semantic_xml(ET.fromstring(after['word/document.xml'])), 'Unrelated XML changed'
    elif task == 'occurrence':
        i = next(i for i, t in enumerate(bt) if t.startswith('The parties agree'))
        pieces = bt[i].split('the terms'); expected = bt.copy()
        expected[i] = 'the terms'.join(pieces[:3]) + 'THE TERMS' + 'the terms'.join(pieces[3:])
        assert at == expected, 'Wrong occurrence or other text changed'
        for j in range(len(bp)):
            if j != i: assert ET.tostring(bp[j]) == ET.tostring(ap[j]), 'Unrelated paragraph changed'
    elif task == 'mixed':
        expected = bt.copy(); expected[bt.index(notice)] = 'The notice period is sixty days.'
        expected.insert(expected.index('This paragraph anchors an insertion.') + 1, 'Inserted paragraph.')
        expected.remove('This paragraph will be deleted.')
        assert at == expected, 'Atomic edits did not produce expected text'
        styled(ap[at.index('The notice period is sixty days.')], 'sixty days', ['b'])
        styled(ap[at.index('The warranty period is twelve months from delivery.')], 'warranty period', ['b', 'u'])
        changed = {notice, 'The warranty period is twelve months from delivery.', 'This paragraph will be deleted.'}
        for p in bp:
            if text(p) not in changed:
                q = ap[at.index(text(p))]
                assert ET.tostring(p) == ET.tostring(q), 'Unrelated paragraph formatting changed'
    elif task == 'equation':
        assert at == bt, 'Text changed while editing equation'
        i = next(i for i, t in enumerate(bt) if t.startswith('Equation '))
        math = ap[i].find('.//' + M + 'oMath')
        assert math is not None
        fractions = list(math.iter(M + 'f'))
        assert len(fractions) == 1, 'Expected one fraction'
        numerator = fractions[0].find(M + 'num'); denominator = fractions[0].find(M + 'den')
        assert ''.join(n.text or '' for n in numerator.iter(M + 't')) == 'a+b', 'Wrong numerator'
        assert ''.join(n.text or '' for n in denominator.iter(M + 't')) == 'c', 'Wrong denominator'
        for j in range(len(bp)):
            if j != i: assert ET.tostring(bp[j]) == ET.tostring(ap[j]), 'Unrelated paragraph changed'
    elif task == 'complex':
        assert len(ap) == len(bp)
        changed = [i for i in range(len(bp)) if ET.tostring(bp[i]) != ET.tostring(ap[i])]
        assert len(changed) == 1, 'Must change exactly one paragraph'
        i = changed[0]
        assert at[i] == bt[i] + ' [edited]', 'Unexpected append'
        assert plain_paragraph(bp[i], before), 'Chosen paragraph was not plain'
    elif task == 'inspect':
        assert before == after, 'Inspection modified source'
    else:
        raise ValueError(task)
    return {'outcome': 'ok'}

if __name__ == '__main__':
    try:
        print(json.dumps(verify(*sys.argv[1:])))
    except Exception as exc:
        print(json.dumps({'outcome': 'error', 'message': str(exc)}))
