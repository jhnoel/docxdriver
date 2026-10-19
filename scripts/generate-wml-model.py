#!/usr/bin/env python3
"""Generate the WordprocessingML content model the core integrity gate checks.

Reads the bundled ECMA-376 Transitional schemas in `scripts/ooxml-schemas/`
and writes `crates/docxdriver-core/src/integrity/wml-model.json`: for every
complex type in the `w:` namespace, the children it allows with their order
rank and maximum count, the namespaces its wildcards admit, and its
attributes with their simple types; for every simple type those attributes
use, the facets a value must satisfy.

Order ranks are conservative. A sequence gives its particles increasing
ranks; everything inside a choice, or inside a repeating sequence, shares
one rank; an element that appears at two ranks gets none. A document is out
of order only when a child's rank is below one already seen, so the checker
never reports an order the schema allows. Standard library only:

    python scripts/generate-wml-model.py
"""
import json
from pathlib import Path
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
SCHEMAS = ROOT / 'scripts' / 'ooxml-schemas'
OUTPUT = ROOT / 'crates' / 'docxdriver-core' / 'src' / 'integrity' / 'wml-model.json'

XSD = 'http://www.w3.org/2001/XMLSchema'
W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'
UNBOUNDED = -1


def x(name):
    return f'{{{XSD}}}{name}'


class Schema:
    def __init__(self, path):
        self.prefixes = {}
        for event, (prefix, uri) in ET.iterparse(path, events=('start-ns',)):
            self.prefixes.setdefault(prefix, uri)
        self.root = ET.parse(path).getroot()
        self.namespace = self.root.get('targetNamespace') or ''
        self.qualified_attributes = self.root.get('attributeFormDefault') == 'qualified'

    def qname(self, value):
        prefix, _, local = value.rpartition(':')
        if prefix == 'xml':
            return '{http://www.w3.org/XML/1998/namespace}' + local
        return f'{{{self.prefixes.get(prefix, "")}}}{local}'


schemas = [Schema(path) for path in sorted(SCHEMAS.glob('*.xsd'))]
complex_types, simple_types, groups, attribute_groups, elements, attributes = {}, {}, {}, {}, {}, {}
for schema in schemas:
    for child in schema.root:
        name = child.get('name')
        if not name:
            continue
        key = f'{{{schema.namespace}}}{name}'
        table = {
            x('complexType'): complex_types, x('simpleType'): simple_types, x('group'): groups,
            x('attributeGroup'): attribute_groups, x('element'): elements, x('attribute'): attributes,
        }.get(child.tag)
        if table is not None:
            table[key] = (schema, child)


def occurs(node, attribute, default=1):
    value = node.get(attribute)
    if value is None:
        return default
    return UNBOUNDED if value == 'unbounded' else int(value)


def multiply(a, b):
    return UNBOUNDED if UNBOUNDED in (a, b) else a * b


def type_key(schema, value):
    return schema.qname(value)


def local(key):
    return key.split('}', 1)[1]


model_types, model_simple = {}, {}
anonymous = 0


class Flattener:
    """Children of one complex type with conservative order ranks."""

    def __init__(self):
        self.children = {}
        self.wildcards = set()
        self.rank = 0

    def add(self, name, rank, maximum, kind, required):
        if name in self.children:
            old_rank, old_max, old_kind, _ = self.children[name]
            rank = old_rank if old_rank == rank else None
            maximum = UNBOUNDED if UNBOUNDED in (old_max, maximum) else old_max + maximum
            kind = old_kind if old_kind == kind else None
            required = False
        self.children[name] = [rank, maximum, kind, int(required)]

    def particle(self, schema, node, multiplier, collapsed, needed=True):
        """Walk one particle. `collapsed` is the shared rank, or None in a plain
        sequence; `needed` is whether every enclosing particle is mandatory."""
        tag = node.tag
        repeat = multiply(multiplier, occurs(node, 'maxOccurs'))
        needed = needed and collapsed is None and occurs(node, 'minOccurs') >= 1
        if tag == x('element'):
            rank = collapsed if collapsed is not None else self.rank
            if node.get('ref'):
                name = schema.qname(node.get('ref'))
                declared = elements.get(name)
                kind = element_type(*declared) if declared else None
            else:
                name = f'{{{schema.namespace}}}{node.get("name")}'
                kind = element_type(schema, node)
            self.add(name, rank, repeat, kind, needed)
            if collapsed is None:
                self.rank += 1
        elif tag == x('any'):
            self.wildcards.add(node.get('namespace') or '##any')
            if collapsed is None:
                self.rank += 1
        elif tag == x('group'):
            group_schema, group = groups[schema.qname(node.get('ref'))]
            for child in group:
                if child.tag in (x('sequence'), x('choice'), x('all')):
                    self.particle_group(group_schema, child, repeat, collapsed, needed)
        elif tag in (x('sequence'), x('choice'), x('all')):
            self.particle_group(schema, node, multiplier, collapsed, needed)

    def particle_group(self, schema, node, repeat, collapsed, needed):
        own = multiply(repeat, occurs(node, 'maxOccurs'))
        needed = needed and occurs(node, 'minOccurs') >= 1
        if collapsed is None and (node.tag != x('sequence') or own not in (0, 1)):
            collapsed = self.rank
            for child in node:
                self.particle(schema, child, own, collapsed, False)
            self.rank += 1
            return
        for child in node:
            self.particle(schema, child, own, collapsed, needed)


def element_type(schema, node):
    global anonymous
    if node.get('type'):
        key = type_key(schema, node.get('type'))
        if key in complex_types and key.startswith(f'{{{W}}}'):
            return local(key)
        return None
    inline = node.find(x('complexType'))
    if inline is not None and schema.namespace == W:
        anonymous += 1
        name = f'anonymous{anonymous}:{node.get("name")}'
        build_complex(name, schema, inline)
        return name
    return None


def attributes_of(schema, node, into):
    for child in node:
        if child.tag == x('attribute'):
            if child.get('ref'):
                name = schema.qname(child.get('ref'))
                declared_schema, declared = attributes.get(name, (schema, child))
                simple = declared.get('type')
                simple = declared_schema.qname(simple) if simple else inline_simple(declared_schema, declared)
            else:
                name = (f'{{{schema.namespace}}}' if schema.qualified_attributes else '{}') + child.get('name')
                simple = schema.qname(child.get('type')) if child.get('type') else inline_simple(schema, child)
            if child.get('use') == 'prohibited':
                into.pop(name, None)
                continue
            into[name] = [simple, int(child.get('use') == 'required')]
            if simple:
                use_simple(simple)
        elif child.tag == x('attributeGroup'):
            group_schema, group = attribute_groups[schema.qname(child.get('ref'))]
            attributes_of(group_schema, group, into)
        elif child.tag == x('anyAttribute'):
            into['*'] = [None, 0]


def inline_simple(schema, node):
    global anonymous
    inline = node.find(x('simpleType'))
    if inline is None:
        return None
    anonymous += 1
    name = f'{{{schema.namespace}}}anonymous{anonymous}'
    simple_types[name] = (schema, inline)
    return name


def use_simple(key):
    if key in model_simple or key.startswith(f'{{{XSD}}}'):
        return
    if key not in simple_types:
        model_simple[key] = {'base': '{%s}anySimpleType' % XSD}
        return
    schema, node = simple_types[key]
    description = {}
    model_simple[key] = description
    for child in node:
        if child.tag == x('restriction'):
            base = schema.qname(child.get('base')) if child.get('base') else inline_simple(schema, child)
            description['base'] = base
            use_simple(base)
            for facet in child:
                name, value = facet.tag.split('}', 1)[1], facet.get('value')
                if name == 'enumeration':
                    description.setdefault('enum', []).append(value)
                elif name == 'pattern':
                    description.setdefault('pattern', []).append(value)
                elif name in ('minInclusive', 'maxInclusive', 'minExclusive', 'maxExclusive',
                              'length', 'minLength', 'maxLength'):
                    description[name] = value
        elif child.tag == x('union'):
            members = [schema.qname(member) for member in (child.get('memberTypes') or '').split()]
            members += [inline_simple_node(schema, inline) for inline in child.findall(x('simpleType'))]
            description['union'] = members
            for member in members:
                use_simple(member)
        elif child.tag == x('list'):
            item = schema.qname(child.get('itemType')) if child.get('itemType') else None
            description['list'] = item
            if item:
                use_simple(item)


def inline_simple_node(schema, node):
    global anonymous
    anonymous += 1
    name = f'{{{schema.namespace}}}anonymous{anonymous}'
    simple_types[name] = (schema, node)
    return name


def build_complex(name, schema, node):
    if name in model_types:
        return
    entry = {}
    model_types[name] = entry
    flat = Flattener()
    attrs = {}

    def content(schema, node):
        for child in node:
            if child.tag in (x('sequence'), x('choice'), x('group'), x('all')):
                flat.particle(schema, child, 1, None)
            elif child.tag in (x('complexContent'), x('simpleContent')):
                derivation = child[0]
                base = schema.qname(derivation.get('base'))
                if child.tag == x('simpleContent'):
                    entry['text'] = base
                    use_simple(base) if base not in complex_types else None
                if derivation.tag.endswith('extension') and base in complex_types:
                    base_schema, base_node = complex_types[base]
                    content(base_schema, base_node)
                content(schema, derivation)
        attributes_of(schema, node, attrs)

    content(schema, node)
    if flat.children:
        entry['children'] = flat.children
    if flat.wildcards:
        entry['any'] = sorted(flat.wildcards)
    if attrs:
        entry['attributes'] = attrs
    for _, _, kind, _ in flat.children.values():
        if kind and kind not in model_types and f'{{{W}}}{kind}' in complex_types:
            build_complex(kind, *complex_types[f'{{{W}}}{kind}'])


for key, (schema, node) in sorted(complex_types.items()):
    if key.startswith(f'{{{W}}}'):
        build_complex(local(key), schema, node)
roots = {}
for key, (schema, node) in sorted(elements.items()):
    if key.startswith(f'{{{W}}}'):
        kind = element_type(schema, node)
        if kind:
            roots[key] = kind

PREFIXES = {
    W: 'w', XSD: 'xsd', 'http://www.w3.org/XML/1998/namespace': 'xml',
    'http://schemas.openxmlformats.org/officeDocument/2006/math': 'm',
    'http://schemas.openxmlformats.org/officeDocument/2006/relationships': 'r',
    'http://schemas.openxmlformats.org/officeDocument/2006/sharedTypes': 's',
    'http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing': 'wp',
    'http://schemas.openxmlformats.org/schemaLibrary/2006/main': 'sl',
    '': '',
}


def compact(value):
    """Clark names to prefixed names, recursively; the checker maps prefixes back."""
    if isinstance(value, str) and value.startswith('{'):
        namespace, name = value[1:].split('}', 1)
        return f'{PREFIXES[namespace]}:{name}' if PREFIXES[namespace] else name
    if isinstance(value, list):
        return [compact(item) for item in value]
    if isinstance(value, dict):
        return {compact(key): compact(item) for key, item in value.items()}
    return value


namespaces = {prefix: uri for uri, prefix in PREFIXES.items() if prefix}
OUTPUT.parent.mkdir(parents=True, exist_ok=True)
OUTPUT.write_text(json.dumps(compact({'roots': roots, 'types': model_types, 'simple': model_simple}) | {'namespaces': namespaces},
                             sort_keys=True, separators=(',', ':')) + '\n', encoding='utf-8')
print(f'{OUTPUT.relative_to(ROOT)}: {len(roots)} roots, {len(model_types)} types, {len(model_simple)} simple types')
