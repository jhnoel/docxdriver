//! A namespace-resolved XML tree read by quick-xml, independent of the
//! engine's own parser and serializer, so a serializer fault cannot hide
//! behind the parser that shares its assumptions.
//!
//! Parsing enforces what Word and the OPC require of every XML part: UTF-8 or
//! UTF-16 with a consistent declaration, no DTD, XML 1.0 characters only,
//! declared namespace prefixes, unique attributes, and balanced tags.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Attr {
    pub ns: String,
    pub local: String,
    pub prefix: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Child {
    Element(usize),
    Text(String),
    Comment(String),
    Pi(String),
}

#[derive(Debug, Clone)]
pub struct Node {
    pub ns: String,
    pub local: String,
    pub prefix: String,
    pub attrs: Vec<Attr>,
    /// Namespace declarations on this element: (prefix, uri); "" is the default.
    pub decls: Vec<(String, String)>,
    pub children: Vec<Child>,
    pub parent: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Tree {
    pub nodes: Vec<Node>,
    /// Comments and processing instructions outside the root element.
    pub prolog: Vec<Child>,
}

impl Tree {
    pub fn root(&self) -> usize {
        0
    }

    pub fn is(&self, node: usize, ns: &str, local: &str) -> bool {
        let n = &self.nodes[node];
        n.local == local && n.ns == ns
    }

    pub fn attr(&self, node: usize, ns: &str, local: &str) -> Option<&str> {
        self.nodes[node]
            .attrs
            .iter()
            .find(|a| a.local == local && a.ns == ns)
            .map(|a| a.value.as_str())
    }

    pub fn element_children(&self, node: usize) -> impl Iterator<Item = usize> + '_ {
        self.nodes[node].children.iter().filter_map(|c| match c {
            Child::Element(i) => Some(*i),
            _ => None,
        })
    }

    pub fn first_child(&self, node: usize, ns: &str, local: &str) -> Option<usize> {
        self.element_children(node).find(|&c| self.is(c, ns, local))
    }

    /// `node` and its descendants in document order.
    pub fn descendants(&self, node: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            out.push(current);
            let children: Vec<usize> = self.element_children(current).collect();
            stack.extend(children.into_iter().rev());
        }
        out
    }

    pub fn ancestors(&self, node: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(self.nodes[node].parent, move |&n| self.nodes[n].parent)
    }

    /// The namespace a prefix resolves to where `node` is in scope.
    pub fn lookup_prefix(&self, node: usize, prefix: &str) -> Option<&str> {
        if prefix == "xml" {
            return Some(XML_NS);
        }
        std::iter::once(node)
            .chain(self.ancestors(node))
            .find_map(|n| {
                self.nodes[n]
                    .decls
                    .iter()
                    .find(|(p, _)| p == prefix)
                    .map(|(_, uri)| uri.as_str())
            })
            .filter(|uri| !uri.is_empty())
    }

    /// Concatenated text content of `node`'s subtree.
    pub fn text(&self, node: usize) -> String {
        let mut out = String::new();
        for child in &self.nodes[node].children {
            match child {
                Child::Text(text) => out.push_str(text),
                Child::Element(i) => out.push_str(&self.text(*i)),
                _ => {}
            }
        }
        out
    }
}

/// Decode a part to text. OPC requires UTF-8 or UTF-16, and the declaration
/// must agree with the bytes.
pub fn decode(bytes: &[u8]) -> Result<String, String> {
    let (text, detected) = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (std::str::from_utf8(rest).map_err(|_| "invalid UTF-8".to_string())?.to_string(), "utf-8")
    } else if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let little = bytes[0] == 0xFF;
        let body = &bytes[2..];
        if body.len() % 2 != 0 {
            return Err("invalid UTF-16".into());
        }
        let units = body.chunks(2).map(|pair| {
            if little {
                u16::from_le_bytes([pair[0], pair[1]])
            } else {
                u16::from_be_bytes([pair[0], pair[1]])
            }
        });
        (
            char::decode_utf16(units)
                .collect::<Result<String, _>>()
                .map_err(|_| "invalid UTF-16".to_string())?,
            "utf-16",
        )
    } else if bytes.len() >= 2 && (bytes[0] == 0 || bytes[1] == 0) {
        return Err("UTF-16 without a byte order mark".into());
    } else {
        (std::str::from_utf8(bytes).map_err(|_| "invalid UTF-8".to_string())?.to_string(), "utf-8")
    };
    if let Some(declared) = declared_encoding(&text) {
        let declared = declared.to_ascii_lowercase();
        let family = match declared.as_str() {
            "utf-8" | "utf8" => "utf-8",
            "utf-16" | "utf-16le" | "utf-16be" => "utf-16",
            _ => return Err(format!("declares encoding {declared}, not UTF-8 or UTF-16")),
        };
        if family != detected {
            return Err(format!("declares encoding {declared} but is {detected}"));
        }
    }
    Ok(text)
}

fn declared_encoding(text: &str) -> Option<&str> {
    let declaration = text.strip_prefix("<?xml")?;
    let declaration = &declaration[..declaration.find("?>")?];
    let start = declaration.find("encoding")? + "encoding".len();
    let rest = declaration[start..].trim_start().strip_prefix('=')?.trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &rest[1..];
    Some(&rest[..rest.find(quote)?])
}

/// XML 1.0 `Char`.
pub fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

fn normalize_eol(text: &str) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_string()
    }
}

/// Parse a part. The error is a short category without positions, so the
/// same fault in two versions of a part is one error kind.
pub fn parse(bytes: &[u8]) -> Result<Tree, String> {
    let text = decode(bytes)?;
    if let Some(c) = text.chars().find(|&c| !is_xml_char(c) && c != '\u{FEFF}') {
        return Err(format!("character U+{:04X} is not allowed in XML", c as u32));
    }
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(&text);
    let mut reader = Reader::from_str(text);
    {
        let config = reader.config_mut();
        config.check_end_names = true;
        config.check_comments = true;
        config.expand_empty_elements = false;
        config.trim_text(false);
    }
    let mut tree = Tree { nodes: Vec::new(), prolog: Vec::new() };
    let mut stack: Vec<usize> = Vec::new();
    let mut root_closed = false;
    loop {
        let event = reader.read_event().map_err(|error| format!("not well-formed: {}", error_kind(&error.to_string())))?;
        match event {
            Event::Decl(_) => {}
            Event::DocType(_) => return Err("contains a DTD".into()),
            Event::Start(start) | Event::Empty(start) if root_closed => {
                let _ = start;
                return Err("not well-formed: more than one root element".into());
            }
            Event::Start(start) => {
                let index = open(&mut tree, &stack, &start)?;
                stack.push(index);
            }
            Event::Empty(start) => {
                let index = open(&mut tree, &stack, &start)?;
                if stack.is_empty() {
                    root_closed = true;
                }
                let _ = index;
            }
            Event::End(_) => {
                stack.pop().ok_or("not well-formed: unmatched end tag")?;
                if stack.is_empty() {
                    root_closed = true;
                }
            }
            Event::Text(t) => {
                let raw: &str = &t;
                if raw.contains(']') && raw.contains("]]>") {
                    return Err("not well-formed: ']]>' in text".into());
                }
                push_text(&mut tree, &stack, &normalize_eol(raw))?;
            }
            Event::CData(t) => {
                let raw: &str = &t;
                push_text(&mut tree, &stack, &normalize_eol(raw))?;
            }
            Event::GeneralRef(r) => {
                let c = if r.is_char_ref() {
                    r.resolve_char_ref()
                        .map_err(|_| "not well-formed: invalid character reference".to_string())?
                        .ok_or("not well-formed: invalid character reference")?
                } else {
                    match &*r {
                        "lt" => '<',
                        "gt" => '>',
                        "amp" => '&',
                        "apos" => '\'',
                        "quot" => '"',
                        _ => return Err("not well-formed: undefined entity".into()),
                    }
                };
                if !is_xml_char(c) {
                    return Err(format!("character U+{:04X} is not allowed in XML", c as u32));
                }
                push_text(&mut tree, &stack, &c.to_string())?;
            }
            Event::Comment(t) => {
                let content = normalize_eol(&t);
                match stack.last() {
                    Some(&parent) => tree.nodes[parent].children.push(Child::Comment(content)),
                    None => tree.prolog.push(Child::Comment(content)),
                }
            }
            Event::PI(t) => {
                // Whitespace before `?>` is not significant to any consumer.
                let content = normalize_eol(t.trim_end());
                match stack.last() {
                    Some(&parent) => tree.nodes[parent].children.push(Child::Pi(content)),
                    None => tree.prolog.push(Child::Pi(content)),
                }
            }
            Event::Eof => break,
        }
    }
    if !stack.is_empty() {
        return Err("not well-formed: unclosed element".into());
    }
    if tree.nodes.is_empty() {
        return Err("not well-formed: no root element".into());
    }
    Ok(tree)
}

fn error_kind(message: &str) -> String {
    // quick-xml messages carry positions and names; keep the leading clause.
    let head = message.split([':', '`', '\'', '"']).next().unwrap_or(message).trim();
    head.chars().filter(|c| !c.is_ascii_digit()).collect()
}

fn push_text(tree: &mut Tree, stack: &[usize], text: &str) -> Result<(), String> {
    match stack.last() {
        Some(&parent) => {
            let children = &mut tree.nodes[parent].children;
            if let Some(Child::Text(existing)) = children.last_mut() {
                existing.push_str(text);
            } else {
                children.push(Child::Text(text.to_string()));
            }
            Ok(())
        }
        None if text.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r')) => Ok(()),
        None => Err("not well-formed: text outside the root element".into()),
    }
}

fn split_name(name: &str) -> (&str, &str) {
    match name.split_once(':') {
        Some((prefix, local)) => (prefix, local),
        None => ("", name),
    }
}

fn is_ncname(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '\u{B7}'))
}

fn open(tree: &mut Tree, stack: &[usize], start: &BytesStart) -> Result<usize, String> {
    let name = start.name().as_ref().to_string();
    let mut decls = Vec::new();
    let mut raw_attrs = Vec::new();
    for attribute in start.attributes().with_checks(true) {
        let attribute = attribute.map_err(|error| format!("not well-formed: {}", error_kind(&error.to_string())))?;
        let key = attribute.key.as_ref().to_string();
        let value = attribute
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|_| "not well-formed: invalid reference in attribute value".to_string())?
            .into_owned();
        if let Some(c) = value.chars().find(|&c| !is_xml_char(c)) {
            return Err(format!("character U+{:04X} is not allowed in XML", c as u32));
        }
        if key == "xmlns" {
            decls.push((String::new(), value));
        } else if let Some(prefix) = key.strip_prefix("xmlns:") {
            if value.is_empty() {
                return Err("namespace prefix bound to the empty name".into());
            }
            if prefix == "xmlns" || (prefix == "xml" && value != XML_NS) {
                return Err("reserved namespace prefix redeclared".into());
            }
            decls.push((prefix.to_string(), value));
        } else {
            raw_attrs.push((key, value));
        }
    }
    let index = tree.nodes.len();
    let parent = stack.last().copied();
    tree.nodes.push(Node {
        ns: String::new(),
        local: String::new(),
        prefix: String::new(),
        attrs: Vec::new(),
        decls,
        children: Vec::new(),
        parent,
    });
    if let Some(parent) = parent {
        tree.nodes[parent].children.push(Child::Element(index));
    }
    let (prefix, local) = split_name(&name);
    if !is_ncname(local) || (!prefix.is_empty() && !is_ncname(prefix)) {
        return Err("not well-formed: invalid element name".into());
    }
    let ns = if prefix.is_empty() {
        tree.lookup_prefix(index, "").unwrap_or("").to_string()
    } else {
        tree.lookup_prefix(index, prefix)
            .ok_or_else(|| format!("undeclared namespace prefix {prefix}"))?
            .to_string()
    };
    let mut attrs = Vec::with_capacity(raw_attrs.len());
    for (key, value) in raw_attrs {
        let (attr_prefix, attr_local) = split_name(&key);
        if !is_ncname(attr_local) || (!attr_prefix.is_empty() && !is_ncname(attr_prefix)) {
            return Err("not well-formed: invalid attribute name".into());
        }
        let attr_ns = if attr_prefix.is_empty() {
            String::new()
        } else {
            tree.lookup_prefix(index, attr_prefix)
                .ok_or_else(|| format!("undeclared namespace prefix {attr_prefix}"))?
                .to_string()
        };
        if attr_ns == XMLNS_NS {
            return Err("reserved namespace used for an attribute".into());
        }
        if attrs.iter().any(|a: &Attr| a.ns == attr_ns && a.local == attr_local) {
            return Err("not well-formed: duplicate attribute".into());
        }
        attrs.push(Attr {
            ns: attr_ns,
            local: attr_local.to_string(),
            prefix: attr_prefix.to_string(),
            value,
        });
    }
    let node = &mut tree.nodes[index];
    node.ns = ns;
    node.local = local.to_string();
    node.prefix = prefix.to_string();
    node.attrs = attrs;
    Ok(index)
}

/// The first difference between two trees, comparing every name, prefix,
/// namespace declaration, attribute, text node, comment and processing
/// instruction. Attribute and declaration order are not significant in XML.
pub fn first_difference(a: &Tree, b: &Tree) -> Option<String> {
    if a.prolog != b.prolog {
        return Some("comments or processing instructions outside the root element differ".into());
    }
    let mut pending = vec![(0usize, 0usize)];
    while let Some((x, y)) = pending.pop() {
        let (left, right) = (&a.nodes[x], &b.nodes[y]);
        let label = || format!("{}:{}", left.prefix, left.local);
        if (&left.ns, &left.local, &left.prefix) != (&right.ns, &right.local, &right.prefix) {
            return Some(format!("element {} became {}:{}", label(), right.prefix, right.local));
        }
        let mut left_attrs = left.attrs.clone();
        let mut right_attrs = right.attrs.clone();
        left_attrs.sort();
        right_attrs.sort();
        if left_attrs != right_attrs {
            let changed = left_attrs
                .iter()
                .find(|a| !right_attrs.contains(a))
                .or_else(|| right_attrs.iter().find(|a| !left_attrs.contains(a)))
                .map(|a| format!("{}:{}", a.prefix, a.local))
                .unwrap_or_default();
            return Some(format!("attribute {changed} of {} differs", label()));
        }
        let mut left_decls = left.decls.clone();
        let mut right_decls = right.decls.clone();
        left_decls.sort();
        right_decls.sort();
        if left_decls != right_decls {
            return Some(format!("namespace declarations of {} differ", label()));
        }
        if left.children.len() != right.children.len() {
            return Some(format!("content of {} differs", label()));
        }
        for (l, r) in left.children.iter().zip(&right.children) {
            match (l, r) {
                (Child::Element(i), Child::Element(j)) => pending.push((*i, *j)),
                (l, r) if l == r => {}
                (Child::Text(_), Child::Text(_)) => return Some(format!("text in {} differs", label())),
                _ => return Some(format!("content of {} differs", label())),
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_what_word_rejects() {
        assert!(parse(b"<a/>").is_ok());
        assert!(parse(b"<!DOCTYPE a><a/>").unwrap_err().contains("DTD"));
        assert!(parse(b"<a>&#1;</a>").unwrap_err().contains("U+0001"));
        assert!(parse(b"<a>\x01</a>").unwrap_err().contains("U+0001"));
        assert!(parse(b"<p:a/>").unwrap_err().contains("undeclared"));
        assert!(parse(b"<a b='1' b='2'/>").is_err());
        assert!(parse(b"<a xmlns:x='u' xmlns:y='u' x:b='1' y:b='2'/>").is_err());
        assert!(parse(b"<a><b></a>").is_err());
        assert!(parse(b"<a/><b/>").is_err());
        assert!(parse(b"<a>&nbsp;</a>").is_err());
        assert!(parse(b"<?xml version='1.0' encoding='windows-1252'?><a/>").unwrap_err().contains("windows-1252"));
        let mut utf16 = vec![0xFF, 0xFE];
        for unit in "<?xml version='1.0' encoding='UTF-8'?><a/>".encode_utf16() {
            utf16.extend(unit.to_le_bytes());
        }
        assert!(parse(&utf16).unwrap_err().contains("is utf-16"));
    }

    #[test]
    fn compares_content_not_layout() {
        let a = parse(b"<w:a xmlns:w='u' w:x='1' w:y='2'>t&amp;<b/></w:a>").unwrap();
        let b = parse(b"<w:a w:y=\"2\" w:x=\"1\" xmlns:w=\"u\">t&#38;<b></b></w:a>").unwrap();
        assert_eq!(first_difference(&a, &b), None);
        let c = parse(b"<w:a xmlns:w='u' w:x='1' w:y='2'>t&amp; <b/></w:a>").unwrap();
        assert!(first_difference(&a, &c).is_some());
        let d = parse(b"<v:a xmlns:v='u' v:x='1' v:y='2'>t&amp;<b/></v:a>").unwrap();
        assert!(first_difference(&a, &d).is_some());
    }
}
