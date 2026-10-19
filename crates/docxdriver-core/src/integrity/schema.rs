//! ECMA-376 Transitional content-model checks for WordprocessingML parts.
//!
//! `wml-model.json` is generated from `scripts/ooxml-schemas/` by
//! `scripts/generate-wml-model.py`. For each `w:` element the checker
//! verifies which children are allowed, their order, how often they occur,
//! which are required, and the attributes and attribute values the schema
//! allows. Order ranks in the model are conservative, so an order the schema
//! allows is never reported. As ECMA-376 Part 3 prescribes, an
//! `mc:AlternateContent` is read as its `mc:Fallback`, and content in
//! ignorable or Office extension namespaces is skipped.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use serde::Deserialize;

use super::tree::{Child, Tree, XML_NS};
use super::{Findings, MC_NS, OFFICE_EXTENSION_PREFIX, W_NS};

const MODEL_JSON: &str = include_str!("wml-model.json");
const UNBOUNDED: i64 = -1;

#[derive(Deserialize)]
struct RawModel {
    namespaces: HashMap<String, String>,
    roots: HashMap<String, String>,
    types: HashMap<String, RawType>,
    simple: HashMap<String, SimpleType>,
}

#[derive(Deserialize, Default)]
struct RawType {
    #[serde(default)]
    children: HashMap<String, (Option<u32>, i64, Option<String>, u8)>,
    #[serde(default)]
    any: Vec<String>,
    #[serde(default)]
    attributes: HashMap<String, (Option<String>, u8)>,
}

#[derive(Deserialize, Default)]
struct SimpleType {
    base: Option<String>,
    #[serde(default)]
    r#enum: Vec<String>,
    #[serde(default)]
    pattern: Vec<String>,
    #[serde(default)]
    union: Vec<String>,
    list: Option<String>,
    #[serde(rename = "minInclusive")]
    min_inclusive: Option<String>,
    #[serde(rename = "maxInclusive")]
    max_inclusive: Option<String>,
    #[serde(rename = "minExclusive")]
    min_exclusive: Option<String>,
    #[serde(rename = "maxExclusive")]
    max_exclusive: Option<String>,
    length: Option<String>,
    #[serde(rename = "minLength")]
    min_length: Option<String>,
    #[serde(rename = "maxLength")]
    max_length: Option<String>,
}

type QName = (String, String);

struct ChildModel {
    rank: Option<u32>,
    max: i64,
    kind: Option<String>,
    required: bool,
}

struct TypeModel {
    /// Children in model order, for stable reporting of missing ones.
    children: BTreeMap<QName, ChildModel>,
    any: Vec<String>,
    attributes: HashMap<QName, (Option<String>, bool)>,
    any_attribute: bool,
}

struct Model {
    roots: HashMap<QName, String>,
    types: HashMap<String, TypeModel>,
    simple: HashMap<String, SimpleType>,
    prefixes: HashMap<String, String>,
    patterns: HashMap<String, regex_lite::Regex>,
}

fn qname(namespaces: &HashMap<String, String>, name: &str) -> QName {
    match name.split_once(':') {
        Some((prefix, local)) => (namespaces.get(prefix).cloned().unwrap_or_default(), local.to_string()),
        None => (String::new(), name.to_string()),
    }
}

fn model() -> &'static Model {
    static MODEL: OnceLock<Model> = OnceLock::new();
    MODEL.get_or_init(|| {
        let raw: RawModel = serde_json::from_str(MODEL_JSON).expect("wml-model.json is valid");
        let ns = &raw.namespaces;
        let types = raw
            .types
            .into_iter()
            .map(|(name, t)| {
                let children = t
                    .children
                    .iter()
                    .map(|(child, (rank, max, kind, required))| {
                        (
                            qname(ns, child),
                            ChildModel { rank: *rank, max: *max, kind: kind.clone(), required: *required != 0 },
                        )
                    })
                    .collect();
                let any_attribute = t.attributes.contains_key("*");
                let attributes = t
                    .attributes
                    .iter()
                    .filter(|(name, _)| name.as_str() != "*")
                    .map(|(name, (simple, required))| (qname(ns, name), (simple.clone(), *required != 0)))
                    .collect();
                (name, TypeModel { children, any: t.any, attributes, any_attribute })
            })
            .collect();
        let roots = raw.roots.iter().map(|(name, kind)| (qname(ns, name), kind.clone())).collect();
        let patterns = raw
            .simple
            .iter()
            .flat_map(|(_, s)| s.pattern.iter())
            .filter_map(|p| {
                regex_lite::Regex::new(&format!("^(?:{p})$")).ok().map(|re| (p.clone(), re))
            })
            .collect();
        let prefixes = raw.namespaces.iter().map(|(p, uri)| (uri.clone(), p.clone())).collect();
        Model { roots, types, simple: raw.simple, prefixes, patterns }
    })
}

fn label(model: &Model, ns: &str, local: &str) -> String {
    match model.prefixes.get(ns) {
        Some(prefix) => format!("{prefix}:{local}"),
        None if ns.is_empty() => local.to_string(),
        None => format!("{{{ns}}}{local}"),
    }
}

/// Namespaces `mc:Ignorable` declares anywhere in the part.
fn ignorable_namespaces(tree: &Tree) -> HashSet<String> {
    let mut out = HashSet::new();
    for node in 0..tree.nodes.len() {
        if let Some(list) = tree.attr(node, MC_NS, "Ignorable") {
            for prefix in list.split_whitespace() {
                if let Some(uri) = tree.lookup_prefix(node, prefix) {
                    out.insert(uri.to_string());
                }
            }
        }
    }
    out
}

/// Check a part whose root is a WordprocessingML root element.
pub fn check(tree: &Tree, part: &str, findings: &mut Findings) {
    let model = model();
    let root = tree.root();
    let key = (tree.nodes[root].ns.clone(), tree.nodes[root].local.clone());
    let Some(kind) = model.roots.get(&key) else {
        return;
    };
    let ignorable = ignorable_namespaces(tree);
    let mut checker = Checker { tree, part, model, ignorable: &ignorable, findings };
    checker.element(root, kind);
}

struct Checker<'a> {
    tree: &'a Tree,
    part: &'a str,
    model: &'static Model,
    ignorable: &'a HashSet<String>,
    findings: &'a mut Findings,
}

impl Checker<'_> {
    fn add(&mut self, message: String) {
        self.findings.add(self.part, format!("schema: {message}"));
    }

    fn skipped_namespace(&self, ns: &str) -> bool {
        ns == MC_NS || ns.starts_with(OFFICE_EXTENSION_PREFIX) || self.ignorable.contains(ns)
    }

    /// Element children with each mc:AlternateContent replaced by its Fallback.
    fn effective_children(&self, node: usize, out: &mut Vec<usize>) {
        for child in self.tree.element_children(node) {
            if self.tree.is(child, MC_NS, "AlternateContent") {
                if let Some(fallback) = self.tree.first_child(child, MC_NS, "Fallback") {
                    self.effective_children(fallback, out);
                }
            } else {
                out.push(child);
            }
        }
    }

    fn element(&mut self, node: usize, kind: &str) {
        let model = self.model;
        let Some(type_model) = model.types.get(kind) else {
            return;
        };
        let tree = self.tree;
        let n = &tree.nodes[node];
        let name = label(model, &n.ns, &n.local);

        for attr in &n.attrs {
            if attr.ns != W_NS && attr.ns != super::R_NS && attr.ns != XML_NS {
                continue;
            }
            let key = (attr.ns.clone(), attr.local.clone());
            match type_model.attributes.get(&key) {
                None if type_model.any_attribute || attr.ns == XML_NS => {}
                None => self.add(format!("{name} has unexpected attribute {}", label(model, &attr.ns, &attr.local))),
                Some((Some(simple), _)) => {
                    if !valid(model, simple, &attr.value, 0) {
                        let short = simple.split(':').next_back().unwrap_or(simple);
                        self.add(format!(
                            "{name}/@{} is not a valid {short}",
                            label(model, &attr.ns, &attr.local)
                        ));
                    }
                }
                Some((None, _)) => {}
            }
        }
        for ((ns, local), (_, required)) in &type_model.attributes {
            if *required && !n.attrs.iter().any(|a| &a.ns == ns && &a.local == local) {
                self.add(format!("{name} lacks required attribute {}", label(model, ns, local)));
            }
        }
        let has_text_content = n.children.iter().any(|c| matches!(c, Child::Text(t) if !t.trim().is_empty()));
        let mut children = Vec::new();
        self.effective_children(node, &mut children);

        let mut counts: HashMap<QName, i64> = HashMap::new();
        let mut highest: Option<u32> = None;
        let mut descend = Vec::new();
        // Word's run properties close their base sequence with the Office
        // extensions (w14 text effects); only rPrChange may follow them.
        let run_properties = matches!(kind, "CT_RPr" | "CT_ParaRPr");
        let mut extension: Option<String> = None;
        for child in children {
            let c = &tree.nodes[child];
            if run_properties && c.ns.starts_with(OFFICE_EXTENSION_PREFIX) && extension.is_none() {
                extension = Some(c.local.clone());
            }
            if self.skipped_namespace(&c.ns) {
                continue;
            }
            if c.ns == W_NS && c.local != "rPrChange" && extension.is_some() {
                self.add(format!("{} follows an Office extension element in {name}", label(model, &c.ns, &c.local)));
            }
            let key = (c.ns.clone(), c.local.clone());
            let child_name = label(model, &c.ns, &c.local);
            let Some(child_model) = type_model.children.get(&key) else {
                let admitted = type_model.any.iter().any(|any| match any.as_str() {
                    "##any" => true,
                    "##other" => c.ns != W_NS,
                    list => list.split_whitespace().any(|ns| ns == c.ns),
                });
                if !admitted && c.ns == W_NS {
                    self.add(format!("{child_name} is not allowed in {name}"));
                }
                continue;
            };
            *counts.entry(key).or_default() += 1;
            if let Some(rank) = child_model.rank {
                if highest.is_some_and(|h| rank < h) {
                    self.add(format!("{child_name} is out of order in {name}"));
                }
                highest = Some(highest.map_or(rank, |h| h.max(rank)));
            }
            if let Some(kind) = &child_model.kind {
                descend.push((child, kind.as_str()));
            }
        }
        for (key, child_model) in &type_model.children {
            let count = counts.get(key).copied().unwrap_or(0);
            if child_model.max != UNBOUNDED && count > child_model.max {
                self.add(format!("{} occurs too often in {name}", label(model, &key.0, &key.1)));
            }
            if child_model.required && count == 0 {
                self.add(format!("{name} lacks required {}", label(model, &key.0, &key.1)));
            }
        }
        if has_text_content && !type_model.children.is_empty() {
            self.add(format!("text is not allowed directly in {name}"));
        }
        for (child, kind) in descend {
            self.element(child, kind);
        }
    }
}

fn number(value: &str) -> Option<f64> {
    value.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Whether `value` is in the lexical space of simple type `name`.
fn valid(model: &Model, name: &str, value: &str, depth: usize) -> bool {
    if depth > 16 {
        return true;
    }
    if let Some(builtin) = name.strip_prefix("xsd:") {
        return builtin_valid(builtin, value);
    }
    let Some(simple) = model.simple.get(name) else {
        return true;
    };
    if !simple.union.is_empty() {
        return simple.union.iter().any(|member| valid(model, member, value, depth + 1));
    }
    if let Some(item) = &simple.list {
        return value.split_whitespace().all(|v| valid(model, item, v, depth + 1));
    }
    let base = simple.base.as_deref().unwrap_or("xsd:anySimpleType");
    let string_like = is_string_based(model, base, 0);
    let value = if string_like { value } else { value.trim() };
    if !valid(model, base, value, depth + 1) {
        return false;
    }
    if !simple.r#enum.is_empty() && !simple.r#enum.iter().any(|e| e == value) {
        return false;
    }
    if !simple.pattern.is_empty()
        && !simple.pattern.iter().any(|p| model.patterns.get(p).is_none_or(|re| re.is_match(value)))
    {
        return false;
    }
    let length = if base_builtin(model, base, 0) == Some("hexBinary") {
        value.len() / 2
    } else {
        value.chars().count()
    };
    let limit = |facet: &Option<String>| facet.as_deref().and_then(|v| v.parse::<usize>().ok());
    if limit(&simple.length).is_some_and(|l| length != l)
        || limit(&simple.min_length).is_some_and(|l| length < l)
        || limit(&simple.max_length).is_some_and(|l| length > l)
    {
        return false;
    }
    let bound = |facet: &Option<String>| facet.as_deref().and_then(number);
    if let Some(v) = number(value) {
        if bound(&simple.min_inclusive).is_some_and(|b| v < b)
            || bound(&simple.max_inclusive).is_some_and(|b| v > b)
            || bound(&simple.min_exclusive).is_some_and(|b| v <= b)
            || bound(&simple.max_exclusive).is_some_and(|b| v >= b)
        {
            return false;
        }
    }
    true
}

fn base_builtin<'m>(model: &'m Model, name: &'m str, depth: usize) -> Option<&'m str> {
    if let Some(builtin) = name.strip_prefix("xsd:") {
        return Some(builtin);
    }
    if depth > 16 {
        return None;
    }
    let simple = model.simple.get(name)?;
    base_builtin(model, simple.base.as_deref()?, depth + 1)
}

fn is_string_based(model: &Model, name: &str, depth: usize) -> bool {
    matches!(base_builtin(model, name, depth), Some("string" | "anySimpleType") | None)
}

fn integer_in(value: &str, min: i128, max: i128) -> bool {
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    match value.parse::<i128>() {
        Ok(v) => (min..=max).contains(&v),
        // Longer than i128: valid only for the unbounded integer type.
        Err(_) => min == i128::MIN && max == i128::MAX,
    }
}

fn builtin_valid(builtin: &str, raw: &str) -> bool {
    if matches!(builtin, "string" | "anySimpleType" | "normalizedString" | "anyURI") {
        return true;
    }
    let value = raw.trim();
    match builtin {
        "token" => true,
        "boolean" => matches!(value, "true" | "false" | "1" | "0"),
        "integer" => integer_in(value, i128::MIN, i128::MAX),
        "nonNegativeInteger" => integer_in(value, 0, i128::MAX),
        "positiveInteger" => integer_in(value, 1, i128::MAX),
        "long" => integer_in(value, i64::MIN as i128, i64::MAX as i128),
        "int" => integer_in(value, i32::MIN as i128, i32::MAX as i128),
        "short" => integer_in(value, i16::MIN as i128, i16::MAX as i128),
        "byte" => integer_in(value, i8::MIN as i128, i8::MAX as i128),
        "unsignedLong" => integer_in(value, 0, u64::MAX as i128),
        "unsignedInt" => integer_in(value, 0, u32::MAX as i128),
        "unsignedShort" => integer_in(value, 0, u16::MAX as i128),
        "unsignedByte" => integer_in(value, 0, u8::MAX as i128),
        "decimal" => {
            let body = value.strip_prefix(['+', '-']).unwrap_or(value);
            !body.is_empty()
                && body != "."
                && body.bytes().filter(|&b| b == b'.').count() <= 1
                && body.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        }
        "double" | "float" => matches!(value, "INF" | "-INF" | "NaN") || value.parse::<f64>().is_ok(),
        "hexBinary" => value.len() % 2 == 0 && value.bytes().all(|b| b.is_ascii_hexdigit()),
        "base64Binary" => value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b' ' | b'\n' | b'\r' | b'\t')),
        "NCName" | "ID" | "IDREF" | "NMTOKEN" => {
            let mut chars = value.chars();
            builtin == "NMTOKEN"
                || matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
                    && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '\u{B7}'))
        }
        "dateTime" => is_date_time(value),
        _ => true,
    }
}

/// xsd:dateTime: `-?YYYY-MM-DDThh:mm:ss(.s+)?(Z|(+|-)hh:mm)?`.
fn is_date_time(value: &str) -> bool {
    static RE: OnceLock<regex_lite::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex_lite::Regex::new(
            r"^-?\d{4,}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])T([01]\d|2[0-3]):[0-5]\d:[0-5]\d(\.\d+)?(Z|[+-]([01]\d|2[0-3]):[0-5]\d)?$",
        )
        .unwrap()
    });
    re.is_match(value) || value.ends_with("T24:00:00") && re.is_match(&value.replace("T24:", "T00:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn findings(xml: &str) -> Vec<String> {
        let tree = super::super::tree::parse(xml.as_bytes()).unwrap();
        let mut findings = Findings::default();
        check(&tree, "word/document.xml", &mut findings);
        findings.kinds().into_iter().map(|(_, message)| message).collect()
    }

    const OPEN: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" mc:Ignorable="w14"><w:body>"#;
    const CLOSE: &str = "</w:body></w:document>";

    fn body(inner: &str) -> Vec<String> {
        findings(&format!("{OPEN}{inner}{CLOSE}"))
    }

    #[test]
    fn model_loads_every_root() {
        assert!(model().roots.len() >= 14);
        assert!(model().patterns.len() == model().simple.values().map(|s| s.pattern.len()).sum::<usize>());
    }

    #[test]
    fn valid_paragraphs_pass() {
        assert_eq!(
            body(r#"<w:p w14:paraId="1A2B3C4D"><w:pPr><w:pStyle w:val="H"/><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:i/><w:b/></w:rPr><w:t xml:space="preserve"> a </w:t></w:r></w:p><w:sectPr/>"#),
            Vec::<String>::new()
        );
    }

    #[test]
    fn reports_order_count_placement_and_values() {
        assert_eq!(body(r#"<w:p><w:pPr><w:jc w:val="center"/><w:pStyle w:val="H"/></w:pPr></w:p>"#), ["schema: w:pStyle is out of order in w:pPr"]);
        assert_eq!(body(r#"<w:p><w:pPr><w:jc w:val="middle"/></w:pPr></w:p>"#), ["schema: w:jc/@w:val is not a valid ST_Jc"]);
        assert_eq!(body(r#"<w:p><w:pPr><w:jc/></w:pPr></w:p>"#), ["schema: w:jc lacks required attribute w:val"]);
        assert_eq!(body(r#"<w:p><w:t>x</w:t></w:p>"#), ["schema: w:t is not allowed in w:p"]);
        assert_eq!(body(r#"<w:p><w:r><w:t>a</w:t><w:rPr/></w:r></w:p>"#), ["schema: w:rPr is out of order in w:r"]);
        assert_eq!(body(r#"<w:sectPr/><w:p/>"#), ["schema: w:p is out of order in w:body"]);
        assert_eq!(body(r#"<w:p><w:pPr><w:jc w:val="left"/><w:jc w:val="left"/></w:pPr></w:p>"#), ["schema: w:jc occurs too often in w:pPr"]);
        assert_eq!(body(r#"<w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>"#), ["schema: w:tbl lacks required w:tblGrid", "schema: w:tbl lacks required w:tblPr"]);
        assert_eq!(body(r#"<w:p><w:ins w:id="1"><w:r/></w:ins></w:p>"#), ["schema: w:ins lacks required attribute w:author"]);
        assert_eq!(body(r#"<w:p><w:ins w:id="1" w:author="a" w:date="yesterday"><w:r/></w:ins></w:p>"#), ["schema: w:ins/@w:date is not a valid ST_DateTime"]);
        assert_eq!(body(r#"<w:p><w:ins w:id="x" w:author="a" w:date="2024-01-02T03:04:05Z"><w:r/></w:ins></w:p>"#), ["schema: w:ins/@w:id is not a valid ST_DecimalNumber"]);
        assert_eq!(body(r#"<w:p><w:pPr><w:bogus/></w:pPr></w:p>"#), ["schema: w:bogus is not allowed in w:pPr"]);
        assert_eq!(body(r#"<w:p w:bogus="1"/>"#), ["schema: w:p has unexpected attribute w:bogus"]);
        assert_eq!(body(r#"<w:p>stray</w:p>"#), ["schema: text is not allowed directly in w:p"]);
    }

    #[test]
    fn extension_content_and_fallbacks() {
        // w14 content is ignorable; AlternateContent is read as its Fallback.
        assert_eq!(body(r#"<w:p><w:pPr><w14:foo/></w:pPr><w:r><mc:AlternateContent><mc:Choice Requires="w14"><w14:x/></mc:Choice><mc:Fallback><w:t>a</w:t></mc:Fallback></mc:AlternateContent></w:r></w:p>"#), Vec::<String>::new());
        assert_eq!(body(r#"<w:p><mc:AlternateContent><mc:Choice Requires="w14"/><mc:Fallback><w:t>a</w:t></mc:Fallback></mc:AlternateContent></w:p>"#), ["schema: w:t is not allowed in w:p"]);
    }

    #[test]
    fn simple_types() {
        let m = model();
        assert!(valid(m, "s:ST_OnOff", "true", 0) && valid(m, "s:ST_OnOff", "on", 0) && !valid(m, "s:ST_OnOff", "yes", 0));
        assert!(valid(m, "s:ST_TwipsMeasure", "1440", 0) && valid(m, "s:ST_TwipsMeasure", "1in", 0) && !valid(m, "s:ST_TwipsMeasure", "-1", 0));
        assert!(valid(m, "w:ST_LongHexNumber", "00A1B2C3", 0) && !valid(m, "w:ST_LongHexNumber", "A1B2", 0));
        assert!(valid(m, "s:ST_HexColorRGB", "FF00AA", 0) && !valid(m, "s:ST_HexColorRGB", "red", 0));
        assert!(is_date_time("2026-10-19T12:00:00Z") && is_date_time("2026-10-19T12:00:00") && !is_date_time("2026-13-01T00:00:00Z"));
    }
}
