#!/usr/bin/env python3
"""Package and cross-part integrity checks that XSD validation cannot express.

Word reports "unreadable content" for packages whose every part is
schema-valid: a part registered under a content type Word does not accept, a
relationship whose target is missing, a comment anchor with no comment, an
unbalanced complex field. These checks cover the package (OPC) layer,
references between parts, and structural rules Word enforces beyond the
ECMA-376 schemas.

`integrity_errors(entries)` returns a Counter of (part, message) error kinds,
the same shape `ooxml-conformance.py` uses for schema errors, so the gate's
"no new error kind" rule applies to both. Standalone:

    python scripts/ooxml_integrity.py file.docx [...]

Requires lxml.
"""
import collections
import posixpath
import re
import sys
import zipfile

from lxml import etree

PARSER = etree.XMLParser(resolve_entities=False, no_network=True, load_dtd=False)

W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
W14 = 'http://schemas.microsoft.com/office/word/2010/wordml'
W15 = 'http://schemas.microsoft.com/office/word/2012/wordml'
W16CID = 'http://schemas.microsoft.com/office/word/2016/wordml/cid'
W16CEX = 'http://schemas.microsoft.com/office/word/2018/wordml/cex'
MC = 'http://schemas.openxmlformats.org/markup-compatibility/2006'
R = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships'
PKG_RELS = 'http://schemas.openxmlformats.org/package/2006/relationships'
PKG_TYPES = 'http://schemas.openxmlformats.org/package/2006/content-types'

OFFICE_REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/'
MS_REL = 'http://schemas.microsoft.com/office/'
STRICT_OFFICE_DOCUMENT = 'http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument'
WML_TYPE = 'application/vnd.openxmlformats-officedocument.wordprocessingml.'

# Relationship type -> the content types Word writes for its target. Taken
# from Word-authored packages (docProps/app.xml "Microsoft Office Word") in
# the LibreOffice corpus, not from documentation: docxdriver 0.3.0 followed a
# plausible-looking but wrong vnd.ms-word.comments* type and Word rejected it.
EXPECTED_CONTENT_TYPES = {
    OFFICE_REL + 'officeDocument': {WML_TYPE + 'document.main+xml', WML_TYPE + 'template.main+xml',
                                    'application/vnd.ms-word.document.macroEnabled.main+xml',
                                    'application/vnd.ms-word.template.macroEnabledTemplate.main+xml'},
    OFFICE_REL + 'styles': {WML_TYPE + 'styles+xml'},
    OFFICE_REL + 'settings': {WML_TYPE + 'settings+xml'},
    OFFICE_REL + 'webSettings': {WML_TYPE + 'webSettings+xml'},
    OFFICE_REL + 'fontTable': {WML_TYPE + 'fontTable+xml'},
    OFFICE_REL + 'numbering': {WML_TYPE + 'numbering+xml'},
    OFFICE_REL + 'header': {WML_TYPE + 'header+xml'},
    OFFICE_REL + 'footer': {WML_TYPE + 'footer+xml'},
    OFFICE_REL + 'footnotes': {WML_TYPE + 'footnotes+xml'},
    OFFICE_REL + 'endnotes': {WML_TYPE + 'endnotes+xml'},
    OFFICE_REL + 'comments': {WML_TYPE + 'comments+xml'},
    OFFICE_REL + 'glossaryDocument': {WML_TYPE + 'document.glossary+xml'},
    OFFICE_REL + 'theme': {'application/vnd.openxmlformats-officedocument.theme+xml'},
    OFFICE_REL + 'extended-properties': {'application/vnd.openxmlformats-officedocument.extended-properties+xml'},
    OFFICE_REL + 'custom-properties': {'application/vnd.openxmlformats-officedocument.custom-properties+xml'},
    OFFICE_REL + 'customXmlProps': {'application/vnd.openxmlformats-officedocument.customXmlProperties+xml'},
    OFFICE_REL + 'chart': {'application/vnd.openxmlformats-officedocument.drawingml.chart+xml'},
    'http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties':
        {'application/vnd.openxmlformats-package.core-properties+xml'},
    MS_REL + '2011/relationships/commentsExtended': {WML_TYPE + 'commentsExtended+xml'},
    MS_REL + '2016/09/relationships/commentsIds': {WML_TYPE + 'commentsIds+xml'},
    MS_REL + '2018/08/relationships/commentsExtensible': {WML_TYPE + 'commentsExtensible+xml'},
    MS_REL + '2011/relationships/people': {WML_TYPE + 'people+xml'},
    MS_REL + '2007/relationships/stylesWithEffects': {'application/vnd.ms-word.stylesWithEffects+xml'},
}
IMAGE_REL = OFFICE_REL + 'image'

# Elements whose r:id must name a relationship of this type.
REFERENCE_TYPES = {
    f'{{{W}}}headerReference': OFFICE_REL + 'header',
    f'{{{W}}}footerReference': OFFICE_REL + 'footer',
    f'{{{W}}}hyperlink': OFFICE_REL + 'hyperlink',
    f'{{{W}}}altChunk': OFFICE_REL + 'aFChunk',
}
BLIP = '{http://schemas.openxmlformats.org/drawingml/2006/main}blip'

# Namespaces Word reads without mc:Ignorable: ECMA-376 Transitional plus the
# Office extension namespaces Word itself declares on every part it writes.
UNDERSTOOD_PREFIXES = (
    'http://schemas.openxmlformats.org/',
    'http://purl.oclc.org/ooxml/',
    'http://schemas.microsoft.com/office/',
    'urn:schemas-microsoft-com:',
    'http://www.w3.org/',
    'http://purl.org/dc/',
)

REVISION_ELEMENTS = {f'{{{W}}}{name}' for name in (
    'ins', 'del', 'moveFrom', 'moveTo', 'pPrChange', 'rPrChange', 'sectPrChange', 'tblPrChange',
    'trPrChange', 'tcPrChange', 'tblGridChange', 'tblPrExChange', 'numberingChange',
    'cellIns', 'cellDel', 'cellMerge')}
STORY_CONTAINERS = {f'{{{W}}}footnote', f'{{{W}}}endnote', f'{{{W}}}comment'}


def w(name):
    return f'{{{W}}}{name}'


def wattr(element, name):
    return element.get(w(name))


def resolve(source, target):
    """Package part name a relationship of `source` points at."""
    if target.startswith('/'):
        return target.lstrip('/')
    return posixpath.normpath(posixpath.join(posixpath.dirname(source), target))


def rels_name(part):
    head, tail = posixpath.split(part)
    return posixpath.join(head, '_rels', tail + '.rels')


class Package:
    def __init__(self, entries):
        self.entries = entries
        self.lower = {name.lower(): name for name in entries}
        self.trees = {}
        self.errors = collections.Counter()
        self.types_default, self.types_override = {}, {}
        self.rels = {}

    def add(self, part, message):
        self.errors[(part, 'integrity: ' + message)] += 1

    def tree(self, part):
        if part not in self.trees:
            data = self.entries.get(part)
            try:
                root = etree.fromstring(data, PARSER) if data is not None else None
            except etree.XMLSyntaxError:
                root = None
            # Word reads one branch of mc:AlternateContent; textbox content
            # repeated in mc:Fallback legitimately duplicates ids and anchors.
            for fallback in list(root.iter(f'{{{MC}}}Fallback')) if root is not None else []:
                fallback.getparent().remove(fallback)
            self.trees[part] = root
        return self.trees[part]

    def content_type(self, part):
        override = self.types_override.get('/' + part.lower())
        if override is not None:
            return override
        extension = part.rsplit('.', 1)[-1].lower() if '.' in posixpath.basename(part) else ''
        return self.types_default.get(extension)

    def relationships(self, source):
        """{rId: (type, resolved target or None when external, mode)} of a part."""
        if source in self.rels:
            return self.rels[source]
        name = rels_name(source) if source else '_rels/.rels'
        found = {}
        root = self.tree(name)
        if root is not None:
            for rel in root.iter(f'{{{PKG_RELS}}}Relationship'):
                external = rel.get('TargetMode') == 'External'
                target = rel.get('Target') or ''
                found.setdefault(rel.get('Id'), (rel.get('Type'), None if external else resolve(source, target), external))
        self.rels[source] = found
        return found

    def targets(self, source, rel_type):
        return [target for kind, target, external in self.relationships(source).values()
                if kind == rel_type and not external and target]


def check_package(pkg):
    names = list(pkg.entries)
    duplicates = collections.Counter(name.lower() for name in names)
    for name, count in duplicates.items():
        if count > 1:
            pkg.add(name, 'duplicate part name (part names are case-insensitive)')

    types = pkg.tree('[Content_Types].xml')
    if types is None:
        pkg.add('[Content_Types].xml', 'missing or malformed content types part')
        return False
    for default in types.iter(f'{{{PKG_TYPES}}}Default'):
        pkg.types_default[(default.get('Extension') or '').lower()] = default.get('ContentType')
    for override in types.iter(f'{{{PKG_TYPES}}}Override'):
        part = override.get('PartName') or ''
        pkg.types_override[part.lower()] = override.get('ContentType')

    for name in names:
        # Word parks deleted parts under [trash]/ without registering them.
        if name.endswith('/') or name == '[Content_Types].xml' or name.startswith('[trash]/'):
            continue
        if pkg.content_type(name) is None:
            pkg.add(name, 'part has no content type')

    sources = [''] + [name for name in names if not name.endswith('.rels')]
    for source in sources:
        rels_part = rels_name(source) if source else '_rels/.rels'
        root = pkg.tree(rels_part)
        if root is None:
            continue
        ids = collections.Counter(rel.get('Id') for rel in root.iter(f'{{{PKG_RELS}}}Relationship'))
        for rid, count in ids.items():
            if count > 1:
                pkg.add(rels_part, 'duplicate relationship Id')
        for rel_type, target, external in pkg.relationships(source).values():
            if external:
                if rel_type in EXPECTED_CONTENT_TYPES:
                    pkg.add(rels_part, f'relationship {rel_type.rsplit("/", 1)[-1]} must target a package part')
                continue
            actual = pkg.lower.get(target.lower())
            if actual is None:
                pkg.add(rels_part, f'relationship {rel_type.rsplit("/", 1)[-1]} targets a missing part')
                continue
            expected = EXPECTED_CONTENT_TYPES.get(rel_type)
            content_type = pkg.content_type(actual)
            if expected and content_type not in expected:
                pkg.add(actual, f'content type {content_type} is not what Word writes for '
                                f'{rel_type.rsplit("/", 1)[-1]} parts ({" or ".join(sorted(expected))})')
            if rel_type == IMAGE_REL and content_type and not content_type.startswith('image/'):
                pkg.add(actual, f'image relationship targets content type {content_type}')

    if not pkg.targets('', OFFICE_REL + 'officeDocument') and not pkg.targets('', STRICT_OFFICE_DOCUMENT):
        pkg.add('_rels/.rels', 'no officeDocument relationship')
    return True


def check_references(pkg, part, root):
    rels = pkg.relationships(part)
    for element in root.iter():
        if not isinstance(element.tag, str):
            continue
        for name, value in element.attrib.items():
            if not name.startswith(f'{{{R}}}') or not value:
                continue
            rel = rels.get(value)
            if rel is None:
                pkg.add(part, f'{etree.QName(element).localname}/@r:{etree.QName(name).localname} '
                              'names no relationship of this part')
                continue
            expected = REFERENCE_TYPES.get(element.tag)
            if element.tag == BLIP and etree.QName(name).localname == 'embed':
                expected = IMAGE_REL
            if expected and rel[0] != expected:
                pkg.add(part, f'{etree.QName(element).localname} r:id names a '
                              f'{(rel[0] or "").rsplit("/", 1)[-1]} relationship, not {expected.rsplit("/", 1)[-1]}')
            if element.tag == w('hyperlink') and not rel[2]:
                pkg.add(part, 'hyperlink relationship is not TargetMode="External"')


def check_markup_compatibility(pkg, part, root):
    for element in root.iter():
        if not isinstance(element.tag, str):
            continue
        ignorable = element.get(f'{{{MC}}}Ignorable')
        if ignorable is not None:
            for prefix in ignorable.split():
                if prefix not in element.nsmap:
                    pkg.add(part, f'mc:Ignorable names undeclared prefix {prefix}')
    # Content in a namespace Word does not understand must be ignorable or
    # inside mc:AlternateContent; otherwise Word refuses the part. Custom XML
    # data parts carry arbitrary namespaces and are not MC-processed.
    if etree.QName(root).namespace != W:
        return
    ignorable_ns = set()
    for element in root.iter():
        if isinstance(element.tag, str):
            for prefix in (element.get(f'{{{MC}}}Ignorable') or '').split():
                if prefix in element.nsmap:
                    ignorable_ns.add(element.nsmap[prefix])
    for element in root.iter():
        if not isinstance(element.tag, str):
            continue
        if any(etree.QName(a).namespace == MC and etree.QName(a).localname == 'Choice'
               for a in [ancestor.tag for ancestor in element.iterancestors() if isinstance(ancestor.tag, str)]):
            continue
        names = [element.tag] + list(element.attrib)
        for name in names:
            namespace = etree.QName(name).namespace
            if not namespace or namespace == MC or namespace in ignorable_ns:
                continue
            if not namespace.startswith(UNDERSTOOD_PREFIXES):
                pkg.add(part, f'non-ignorable unknown namespace {namespace}')


def stories(part_kind, root):
    """Units whose annotation markup must pair up: the body or part root, or
    each footnote/endnote/comment."""
    if part_kind in ('footnotes', 'endnotes', 'comments'):
        return [child for child in root if child.tag in STORY_CONTAINERS]
    return [root]


def check_story(pkg, part, story, comment_ids):
    """Pairing of ranges, fields, bookmarks within one story."""
    open_bookmarks, ends = collections.Counter(), collections.Counter()
    range_starts, range_ends = set(), set()
    depth, separated = 0, []
    for element in story.iter():
        tag = element.tag
        if tag == w('bookmarkStart'):
            open_bookmarks[wattr(element, 'id')] += 1
        elif tag == w('bookmarkEnd'):
            ends[wattr(element, 'id')] += 1
        elif tag == w('commentRangeStart'):
            range_starts.add(wattr(element, 'id'))
        elif tag == w('commentRangeEnd'):
            range_ends.add(wattr(element, 'id'))
        elif tag == w('fldChar'):
            kind = wattr(element, 'fldCharType')
            if kind == 'begin':
                depth += 1
                separated.append(False)
            elif kind == 'separate':
                if depth == 0:
                    pkg.add(part, 'fldChar separate outside a field')
                elif separated[-1]:
                    pkg.add(part, 'field has more than one fldChar separate')
                else:
                    separated[-1] = True
            elif kind == 'end':
                if depth == 0:
                    pkg.add(part, 'fldChar end without a begin')
                else:
                    depth -= 1
                    separated.pop()
    if depth:
        pkg.add(part, 'fldChar begin without an end')
    for bookmark, count in open_bookmarks.items():
        if count > 1:
            pkg.add(part, 'duplicate bookmarkStart id')
        if not ends[bookmark]:
            pkg.add(part, 'bookmarkStart without a bookmarkEnd')
    for bookmark in ends:
        if not open_bookmarks[bookmark]:
            pkg.add(part, 'bookmarkEnd without a bookmarkStart')
    for comment in range_starts - range_ends:
        pkg.add(part, 'commentRangeStart without a commentRangeEnd')
    for comment in range_ends - range_starts:
        pkg.add(part, 'commentRangeEnd without a commentRangeStart')
    for comment in range_starts | range_ends:
        if comment not in comment_ids:
            pkg.add(part, 'comment range names no w:comment')


WRAPPERS = {f'{{{W}}}sdt', f'{{{W}}}sdtContent', f'{{{W}}}customXml'}


def wrapped(parent, tag):
    """Children named `tag`, looking through content-control and custom XML wrappers."""
    for child in parent:
        if child.tag == tag:
            yield child
        elif child.tag in WRAPPERS:
            yield from wrapped(child, tag)


def check_tables(pkg, part, root):
    for table in root.iter(w('tbl')):
        grid = table.find(w('tblGrid'))
        columns = len(grid.findall(w('gridCol'))) if grid is not None else 0
        above = {}
        for row in wrapped(table, w('tr')):
            position, here = 0, {}
            trpr = row.find(w('trPr'))
            before = trpr.find(w('gridBefore')) if trpr is not None else None
            if before is not None:
                position += int(wattr(before, 'val') or 0)
            for cell in wrapped(row, w('tc')):
                blocks = [child for child in cell if child.tag in (w('p'), w('tbl'), w('sdt'), w('customXml'))]
                if not blocks:
                    pkg.add(part, 'table cell has no paragraph')
                elif blocks[-1].tag == w('tbl'):
                    pkg.add(part, 'table cell does not end with a paragraph')
                tcpr = cell.find(w('tcPr'))
                span_node = tcpr.find(w('gridSpan')) if tcpr is not None else None
                span = int(wattr(span_node, 'val') or 1) if span_node is not None else 1
                vmerge = tcpr.find(w('vMerge')) if tcpr is not None else None
                state = None
                if vmerge is not None:
                    state = 'restart' if wattr(vmerge, 'val') == 'restart' else 'continue'
                    if state == 'continue' and above.get(position) not in ('restart', 'continue'):
                        pkg.add(part, 'vMerge continue cell has no merged cell above it')
                here[position] = state
                position += span
            after = trpr.find(w('gridAfter')) if trpr is not None else None
            if after is not None:
                position += int(wattr(after, 'val') or 0)
            if columns and position != columns:
                pkg.add(part, 'row spans a different number of grid columns than tblGrid declares')
            above = here


def check_ids(pkg, part, root):
    para_ids = collections.Counter()
    for element in root.iter():
        if not isinstance(element.tag, str):
            continue
        for attribute in ('paraId', 'textId'):
            value = element.get(f'{{{W14}}}{attribute}')
            if value is None:
                continue
            if not re.fullmatch(r'[0-9A-Fa-f]{8}', value) or int(value, 16) >= 0x80000000:
                pkg.add(part, f'w14:{attribute} is not eight hex digits below 80000000')
            elif attribute == 'paraId':
                para_ids[value.upper()] += 1
    for value, count in para_ids.items():
        if count > 1:
            pkg.add(part, 'duplicate w14:paraId')
    revision_ids = collections.Counter(
        wattr(element, 'id') for element in root.iter() if element.tag in REVISION_ELEMENTS and wattr(element, 'id') is not None)
    for value, count in revision_ids.items():
        if count > 1:
            pkg.add(part, 'duplicate revision w:id')


def check_comments(pkg, main, story_parts):
    comments_parts = pkg.targets(main, OFFICE_REL + 'comments')
    comment_ids, last_para = set(), {}
    comments_part = comments_parts[0] if comments_parts else None
    root = pkg.tree(comments_part) if comments_part else None
    if root is not None:
        ids = collections.Counter()
        for comment in root.findall(w('comment')):
            cid = wattr(comment, 'id')
            ids[cid] += 1
            comment_ids.add(cid)
            paragraphs = list(comment.iter(w('p')))
            if paragraphs:
                para = paragraphs[-1].get(f'{{{W14}}}paraId')
                if para:
                    last_para[para.upper()] = cid
        for cid, count in ids.items():
            if count > 1:
                pkg.add(comments_part, 'duplicate w:comment id')

    referenced = set()
    for part, kind in story_parts:
        story_root = pkg.tree(part)
        if story_root is None:
            continue
        for reference in story_root.iter(w('commentReference')):
            cid = wattr(reference, 'id')
            referenced.add(cid)
            if cid not in comment_ids:
                pkg.add(part, 'commentReference names no w:comment')
    if comments_part:
        for cid in comment_ids - referenced:
            pkg.add(comments_part, 'w:comment has no commentReference')

    extended = pkg.targets(main, MS_REL + '2011/relationships/commentsExtended')
    root = pkg.tree(extended[0]) if extended else None
    if root is not None:
        entries = root.findall(f'{{{W15}}}commentEx')
        own = collections.Counter((entry.get(f'{{{W15}}}paraId') or '').upper() for entry in entries)
        for entry in entries:
            para = (entry.get(f'{{{W15}}}paraId') or '').upper()
            if para not in last_para:
                pkg.add(extended[0], 'commentEx paraId is not the last paragraph of a comment')
            parent = entry.get(f'{{{W15}}}paraIdParent')
            if parent is not None and parent.upper() not in own:
                pkg.add(extended[0], 'commentEx paraIdParent names no commentEx')
        for para, count in own.items():
            if count > 1:
                pkg.add(extended[0], 'duplicate commentEx paraId')

    durable = set()
    ids_parts = pkg.targets(main, MS_REL + '2016/09/relationships/commentsIds')
    root = pkg.tree(ids_parts[0]) if ids_parts else None
    if root is not None:
        seen = collections.Counter()
        for entry in root.findall(f'{{{W16CID}}}commentId'):
            if (entry.get(f'{{{W16CID}}}paraId') or '').upper() not in last_para:
                pkg.add(ids_parts[0], 'commentId paraId is not the last paragraph of a comment')
            value = entry.get(f'{{{W16CID}}}durableId') or ''
            seen[value.upper()] += 1
            durable.add(value.upper())
            if not re.fullmatch(r'[0-9A-Fa-f]{8}', value) or not 0 < int(value, 16) < 0x7FFFFFFF:
                pkg.add(ids_parts[0], 'durableId is not eight hex digits in (0, 7FFFFFFF)')
        for value, count in seen.items():
            if count > 1:
                pkg.add(ids_parts[0], 'duplicate durableId')

    extensible = pkg.targets(main, MS_REL + '2018/08/relationships/commentsExtensible')
    root = pkg.tree(extensible[0]) if extensible else None
    if root is not None:
        for entry in root.findall(f'{{{W16CEX}}}commentExtensible'):
            if (entry.get(f'{{{W16CEX}}}durableId') or '').upper() not in durable:
                pkg.add(extensible[0], 'commentExtensible durableId is not in commentsIds')
    return comment_ids


def check_numbering(pkg, main, numbered_parts):
    numbering = pkg.targets(main, OFFICE_REL + 'numbering')
    root = pkg.tree(numbering[0]) if numbering else None
    nums, abstracts = set(), set()
    if root is not None:
        abstracts = {wattr(node, 'abstractNumId') for node in root.findall(w('abstractNum'))}
        for num in root.findall(w('num')):
            nums.add(wattr(num, 'numId'))
            reference = num.find(w('abstractNumId'))
            if reference is None or wattr(reference, 'val') not in abstracts:
                pkg.add(numbering[0], 'w:num names no abstractNum')
    for part in numbered_parts:
        part_root = pkg.tree(part)
        if part_root is None:
            continue
        for numpr in part_root.iter(w('numPr')):
            num_id = numpr.find(w('numId'))
            if num_id is not None and wattr(num_id, 'val') not in ('0', None) and wattr(num_id, 'val') not in nums:
                pkg.add(part, 'numPr numId names no w:num')
            level = numpr.find(w('ilvl'))
            if level is not None and wattr(level, 'val') not in {str(n) for n in range(9)}:
                pkg.add(part, 'numPr ilvl is outside 0-8')


def check_notes(pkg, main, story_parts):
    for kind in ('footnote', 'endnote'):
        parts = pkg.targets(main, OFFICE_REL + kind + 's')
        root = pkg.tree(parts[0]) if parts else None
        ids = {wattr(note, 'id') for note in root.findall(w(kind))} if root is not None else set()
        for part, _ in story_parts:
            story_root = pkg.tree(part)
            if story_root is None:
                continue
            for reference in story_root.iter(w(kind + 'Reference')):
                if wattr(reference, 'id') not in ids:
                    pkg.add(part, f'{kind}Reference names no w:{kind}')


def integrity_errors(entries):
    """Error kinds for a package given as {part name: bytes}."""
    pkg = Package(entries)
    if not check_package(pkg):
        return pkg.errors
    mains = pkg.targets('', OFFICE_REL + 'officeDocument')
    main = mains[0] if mains else None
    if main is None or pkg.tree(main) is None:
        return pkg.errors

    story_parts = [(main, 'document')]
    for kind in ('header', 'footer', 'footnotes', 'endnotes', 'comments'):
        story_parts += [(part, kind) for part in pkg.targets(main, OFFICE_REL + kind)]

    for name in entries:
        if name.lower().endswith(('.xml', '.rels')) and pkg.tree(name) is not None:
            root = pkg.tree(name)
            if not name.endswith('.rels'):
                check_references(pkg, name, root)
            check_markup_compatibility(pkg, name, root)

    comment_ids = check_comments(pkg, main, story_parts)
    for part, kind in story_parts:
        root = pkg.tree(part)
        if root is None:
            continue
        for story in stories(kind, root):
            check_story(pkg, part, story, comment_ids)
        check_tables(pkg, part, root)
        check_ids(pkg, part, root)
    styles = pkg.targets(main, OFFICE_REL + 'styles')
    check_numbering(pkg, main, [part for part, _ in story_parts] + styles)
    check_notes(pkg, main, story_parts)
    return pkg.errors


def read_entries(path):
    with zipfile.ZipFile(path) as package:
        return {entry.filename: package.read(entry) for entry in package.infolist()}


def main(paths):
    status = 0
    for path in paths:
        errors = integrity_errors(read_entries(path))
        for (part, message), count in sorted(errors.items()):
            print(f'{path}: {part}: {message} (x{count})')
        status |= bool(errors)
    return status


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
