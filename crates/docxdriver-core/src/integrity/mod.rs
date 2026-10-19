//! Package integrity: what makes Word refuse or "repair" a `.docx`.
//!
//! [`check`] reads a package with its own zip and XML readers (quick-xml, not
//! the engine's parser) and reports error kinds: `(part, message)` pairs with
//! no positions, so the same fault in two versions of a package is one kind.
//! It covers
//!
//! - the zip container: readable, stored or deflated, unencrypted entries
//!   with valid, case-insensitively unique part names;
//! - every XML part: UTF-8 or UTF-16, no DTD, XML 1.0 characters, declared
//!   prefixes, well-formed;
//! - the OPC layer: content types, relationship ids, modes and targets, the
//!   content type Word writes for each relationship type;
//! - references between parts: `r:` ids, comments, notes, numbering, style
//!   definitions;
//! - markup compatibility: `mc:Ignorable`, `mc:MustUnderstand`, `mc:Choice`;
//! - story structure: field, bookmark, comment and move range pairing,
//!   deleted-text markup, significant whitespace, table grids, paragraph,
//!   drawing and revision ids;
//! - the ECMA-376 Transitional content model of every WordprocessingML part
//!   (see `schema.rs`).
//!
//! [`gate`] is the commit rule: an edit may not introduce an error kind its
//! input did not already have. Pre-existing faults are not the engine's to
//! fix, and they never block an edit.

mod schema;
pub mod tree;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{Cursor, Read};

use tree::{Tree, XML_NS};

pub const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
pub const OFFICE_EXTENSION_PREFIX: &str = "http://schemas.microsoft.com/office/";
const W14_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
const W15_NS: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
const W16CID_NS: &str = "http://schemas.microsoft.com/office/word/2016/wordml/cid";
const W16CEX_NS: &str = "http://schemas.microsoft.com/office/word/2018/wordml/cex";
const WP_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PKG_RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const PKG_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

const OFFICE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const MS_REL: &str = "http://schemas.microsoft.com/office/";
const STRICT_OFFICE_DOCUMENT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument";
const WML_TYPE: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.";
const CONTENT_TYPES: &str = "[Content_Types].xml";

/// Relationship type suffix -> the content types Word writes for its target.
/// Taken from Word-authored packages, never from documentation: 0.3.0
/// registered comment parts as `application/vnd.ms-word.*` and Word refused
/// them. Mirrors `EXPECTED_CONTENT_TYPES` in `scripts/ooxml_integrity.py`.
fn expected_content_types(rel_type: &str) -> Option<Vec<String>> {
    let wml = |suffix: &str| format!("{WML_TYPE}{suffix}");
    let office = |suffix: &str| format!("application/vnd.openxmlformats-officedocument.{suffix}");
    let types = if let Some(kind) = rel_type.strip_prefix(OFFICE_REL) {
        match kind {
            "officeDocument" => vec![
                wml("document.main+xml"),
                wml("template.main+xml"),
                "application/vnd.ms-word.document.macroEnabled.main+xml".into(),
                "application/vnd.ms-word.template.macroEnabledTemplate.main+xml".into(),
            ],
            "styles" => vec![wml("styles+xml")],
            "settings" => vec![wml("settings+xml")],
            "webSettings" => vec![wml("webSettings+xml")],
            "fontTable" => vec![wml("fontTable+xml")],
            "numbering" => vec![wml("numbering+xml")],
            "header" => vec![wml("header+xml")],
            "footer" => vec![wml("footer+xml")],
            "footnotes" => vec![wml("footnotes+xml")],
            "endnotes" => vec![wml("endnotes+xml")],
            "comments" => vec![wml("comments+xml")],
            "glossaryDocument" => vec![wml("document.glossary+xml")],
            "theme" => vec![office("theme+xml")],
            "extended-properties" => vec![office("extended-properties+xml")],
            "custom-properties" => vec![office("custom-properties+xml")],
            "customXmlProps" => vec![office("customXmlProperties+xml")],
            "chart" => vec![office("drawingml.chart+xml")],
            _ => return None,
        }
    } else if rel_type == "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" {
        vec!["application/vnd.openxmlformats-package.core-properties+xml".into()]
    } else if let Some(kind) = rel_type.strip_prefix(MS_REL) {
        match kind {
            "2011/relationships/commentsExtended" => vec![wml("commentsExtended+xml")],
            "2016/09/relationships/commentsIds" => vec![wml("commentsIds+xml")],
            "2018/08/relationships/commentsExtensible" => vec![wml("commentsExtensible+xml")],
            "2011/relationships/people" => vec![wml("people+xml")],
            "2007/relationships/stylesWithEffects" => vec!["application/vnd.ms-word.stylesWithEffects+xml".into()],
            _ => return None,
        }
    } else {
        return None;
    };
    Some(types)
}

/// Namespaces Word reads without `mc:Ignorable`: ECMA-376 Transitional plus
/// the Office extension namespaces Word declares on every part it writes.
const UNDERSTOOD_PREFIXES: &[&str] = &[
    "http://schemas.openxmlformats.org/",
    "http://purl.oclc.org/ooxml/",
    "http://schemas.microsoft.com/office/",
    "urn:schemas-microsoft-com:",
    "http://www.w3.org/",
    "http://purl.org/dc/",
];

const REVISION_ELEMENTS: &[&str] = &[
    "ins", "del", "moveFrom", "moveTo", "pPrChange", "rPrChange", "sectPrChange", "tblPrChange",
    "trPrChange", "tcPrChange", "tblGridChange", "tblPrExChange", "numberingChange", "cellIns",
    "cellDel", "cellMerge",
];

/// Error kinds of one package, with how often each occurs.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Findings(BTreeMap<(String, String), usize>);

impl Findings {
    pub fn add(&mut self, part: &str, message: impl Into<String>) {
        *self.0.entry((part.to_string(), message.into())).or_default() += 1;
    }

    pub fn kinds(&self) -> BTreeSet<(String, String)> {
        self.0.keys().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&(String, String), &usize)> {
        self.0.iter()
    }
}

struct Entry {
    name: String,
    data: Vec<u8>,
}

/// Read every zip entry; container faults are findings, not errors.
fn read_entries(bytes: &[u8], findings: &mut Findings) -> Option<Vec<Entry>> {
    let mut archive = match zip::ZipArchive::new(Cursor::new(bytes)) {
        Ok(archive) => archive,
        Err(_) => {
            findings.add("", "package: not a readable zip archive");
            return None;
        }
    };
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let name = archive.name_for_index(index).unwrap_or_default().to_string();
        let mut file = match archive.by_index(index) {
            Ok(file) => file,
            Err(error) => {
                let message = if error.to_string().to_lowercase().contains("password") {
                    "package: entry is encrypted"
                } else {
                    "package: entry cannot be read"
                };
                findings.add(&name, message);
                continue;
            }
        };
        if !matches!(file.compression(), zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated) {
            findings.add(&name, "package: entry is neither stored nor deflated");
        }
        if file.is_dir() {
            entries.push(Entry { name, data: Vec::new() });
            continue;
        }
        let mut data = Vec::with_capacity(file.size() as usize);
        if file.read_to_end(&mut data).is_err() {
            findings.add(&name, "package: entry cannot be read");
            continue;
        }
        entries.push(Entry { name, data });
    }
    Some(entries)
}

/// OPC part-name rules (ECMA-376 Part 2, 6.2.2) on the zip item name.
fn part_name_fault(name: &str) -> Option<&'static str> {
    if name.is_empty() || name.starts_with('/') || name.contains('\\') {
        return Some("package: invalid part name");
    }
    for segment in name.trim_end_matches('/').split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.ends_with('.') {
            return Some("package: invalid part name");
        }
    }
    None
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| value.to_string())
}

/// Package part name a relationship of `source` points at.
fn resolve_target(source: &str, target: &str) -> String {
    let target = target.split('#').next().unwrap_or("");
    let target = percent_decode(target);
    let joined = if let Some(absolute) = target.strip_prefix('/') {
        absolute.to_string()
    } else {
        let directory = source.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        if directory.is_empty() {
            target
        } else {
            format!("{directory}/{target}")
        }
    };
    let mut parts: Vec<&str> = Vec::new();
    for segment in joined.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn rels_name(part: &str) -> String {
    if part.is_empty() {
        return "_rels/.rels".into();
    }
    match part.rsplit_once('/') {
        Some((head, tail)) => format!("{head}/_rels/{tail}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// The source part of a relationships part, or None when the name is not one.
fn rels_source(name: &str) -> Option<String> {
    if name == "_rels/.rels" {
        return Some(String::new());
    }
    let (head, file) = name.rsplit_once('/')?;
    let source = file.strip_suffix(".rels")?;
    let directory = head.strip_suffix("_rels")?;
    Some(format!("{directory}{source}"))
}

#[derive(Clone)]
struct Relationship {
    rel_type: String,
    /// Resolved part name; None when external.
    target: Option<String>,
    external: bool,
}

struct Package {
    names: Vec<String>,
    data: HashMap<String, Vec<u8>>,
    lower: HashMap<String, String>,
    trees: HashMap<String, Option<Tree>>,
    defaults: HashMap<String, String>,
    overrides: HashMap<String, String>,
    rels: HashMap<String, Vec<(String, Relationship)>>,
    findings: Findings,
}

fn is_xml_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".xml") || lower.ends_with(".rels")
}

impl Package {
    fn add(&mut self, part: &str, message: impl AsRef<str>) {
        self.findings.add(part, format!("integrity: {}", message.as_ref()));
    }

    fn tree(&self, part: &str) -> Option<&Tree> {
        self.trees.get(part).and_then(Option::as_ref)
    }

    fn content_type(&self, part: &str) -> Option<String> {
        if let Some(found) = self.overrides.get(&format!("/{}", part.to_lowercase())) {
            return Some(found.clone());
        }
        let file = part.rsplit('/').next().unwrap_or(part);
        let extension = file.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
        self.defaults.get(&extension).cloned()
    }

    /// Relationships of a part in document order; the first of duplicate ids wins.
    fn relationships(&mut self, source: &str) -> Vec<(String, Relationship)> {
        if let Some(found) = self.rels.get(source) {
            return found.clone();
        }
        let mut found: Vec<(String, Relationship)> = Vec::new();
        if let Some(tree) = self.tree(&rels_name(source)) {
            for node in tree.descendants(tree.root()) {
                if !tree.is(node, PKG_RELS_NS, "Relationship") {
                    continue;
                }
                let id = tree.attr(node, "", "Id").unwrap_or("").to_string();
                if found.iter().any(|(existing, _)| existing == &id) {
                    continue;
                }
                let external = tree.attr(node, "", "TargetMode") == Some("External");
                let target = tree.attr(node, "", "Target").unwrap_or("");
                found.push((
                    id,
                    Relationship {
                        rel_type: tree.attr(node, "", "Type").unwrap_or("").to_string(),
                        target: (!external).then(|| resolve_target(source, target)),
                        external,
                    },
                ));
            }
        }
        self.rels.insert(source.to_string(), found.clone());
        found
    }

    fn targets(&mut self, source: &str, rel_type: &str) -> Vec<String> {
        self.relationships(source)
            .into_iter()
            .filter(|(_, rel)| rel.rel_type == rel_type && !rel.external)
            .filter_map(|(_, rel)| rel.target)
            .filter(|target| !target.is_empty())
            .collect()
    }

    /// The stored name for a resolved target (part names are case-insensitive).
    fn actual(&self, target: &str) -> Option<String> {
        self.lower.get(&target.to_lowercase()).cloned()
    }
}

/// Descendants of `node` (inclusive) in document order, without
/// `mc:Fallback` subtrees: Word reads one branch of `mc:AlternateContent`, and
/// textbox content repeated in the Fallback legitimately duplicates ids.
fn visible(tree: &Tree, node: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if tree.is(current, MC_NS, "Fallback") {
            continue;
        }
        out.push(current);
        let children: Vec<usize> = tree.element_children(current).collect();
        stack.extend(children.into_iter().rev());
    }
    out
}

fn w_attr<'t>(tree: &'t Tree, node: usize, name: &str) -> Option<&'t str> {
    tree.attr(node, W_NS, name)
}

fn is_w(tree: &Tree, node: usize, name: &str) -> bool {
    tree.is(node, W_NS, name)
}

/// Error kinds of a `.docx` package.
pub fn check(bytes: &[u8]) -> Findings {
    let mut findings = Findings::default();
    let Some(entries) = read_entries(bytes, &mut findings) else {
        return findings;
    };
    let mut package = Package {
        names: Vec::new(),
        data: HashMap::new(),
        lower: HashMap::new(),
        trees: HashMap::new(),
        defaults: HashMap::new(),
        overrides: HashMap::new(),
        rels: HashMap::new(),
        findings,
    };
    for entry in entries {
        package.lower.entry(entry.name.to_lowercase()).or_insert_with(|| entry.name.clone());
        package.names.push(entry.name.clone());
        package.data.insert(entry.name, entry.data);
    }
    check_container(&mut package);
    if !check_content_types(&mut package) {
        return package.findings;
    }
    parse_xml_parts(&mut package);
    check_relationships(&mut package);

    let mains = package.targets("", &format!("{OFFICE_REL}officeDocument"));
    let Some(main) = mains.first().and_then(|m| package.actual(m)) else {
        return package.findings;
    };
    if package.tree(&main).is_none() {
        return package.findings;
    }
    let mut story_parts = vec![(main.clone(), "document")];
    for kind in ["header", "footer", "footnotes", "endnotes"] {
        for target in package.targets(&main, &format!("{OFFICE_REL}{kind}")) {
            if let Some(actual) = package.actual(&target) {
                story_parts.push((actual, kind));
            }
        }
    }
    if let Some(comments) = adopted_part(&mut package, &main, &format!("{OFFICE_REL}comments"), "comments.xml") {
        story_parts.push((comments, "comments"));
    }

    let xml_parts: Vec<String> = package.names.iter().filter(|n| package.tree(n).is_some()).cloned().collect();
    for name in &xml_parts {
        if !name.ends_with(".rels") {
            check_references(&mut package, name);
        }
        check_markup_compatibility(&mut package, name);
        let tree = package.trees.remove(name).flatten();
        if let Some(tree) = &tree {
            schema::check(tree, name, &mut package.findings);
        }
        package.trees.insert(name.clone(), tree);
    }

    let comment_ids = check_comments(&mut package, &main, &story_parts);
    check_styles(&mut package, &main);
    for (part, kind) in &story_parts {
        let Some(tree) = package.trees.remove(part).flatten() else {
            continue;
        };
        let stories: Vec<usize> = if matches!(*kind, "footnotes" | "endnotes" | "comments") {
            tree.element_children(tree.root())
                .filter(|&c| is_w(&tree, c, "footnote") || is_w(&tree, c, "endnote") || is_w(&tree, c, "comment"))
                .collect()
        } else {
            vec![tree.root()]
        };
        for story in stories {
            check_story(&mut package, part, &tree, story, &comment_ids);
        }
        check_story_content(&mut package, part, *kind, &tree);
        check_tables(&mut package, part, &tree);
        check_ids(&mut package, part, &tree);
        package.trees.insert(part.clone(), Some(tree));
    }
    check_paragraph_ids_across_parts(&mut package, &story_parts);
    check_sections(&mut package, &main);
    let mut numbered: Vec<String> = story_parts.iter().map(|(p, _)| p.clone()).collect();
    numbered.extend(package.targets(&main, &format!("{OFFICE_REL}styles")).into_iter().filter_map(|t| package.actual(&t)));
    check_numbering(&mut package, &main, &numbered);
    check_notes(&mut package, &main, &story_parts);
    package.findings
}

fn check_container(package: &mut Package) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for name in package.names.clone() {
        *seen.entry(name.to_lowercase()).or_default() += 1;
        if let Some(fault) = part_name_fault(&name) {
            package.findings.add(&name, fault);
        }
    }
    let mut duplicates: Vec<String> = seen.into_iter().filter(|(_, count)| *count > 1).map(|(n, _)| n).collect();
    duplicates.sort();
    for name in duplicates {
        package.add(&name, "duplicate part name (part names are case-insensitive)");
    }
}

fn check_content_types(package: &mut Package) -> bool {
    let tree = package
        .data
        .get(CONTENT_TYPES)
        .map(|bytes| tree::parse(bytes));
    let tree = match tree {
        Some(Ok(tree)) => tree,
        Some(Err(error)) => {
            package.findings.add(CONTENT_TYPES, format!("xml: {error}"));
            package.add(CONTENT_TYPES, "missing or malformed content types part");
            return false;
        }
        None => {
            package.add(CONTENT_TYPES, "missing or malformed content types part");
            return false;
        }
    };
    let mut default_seen = HashSet::new();
    let mut override_seen = HashSet::new();
    for node in tree.descendants(tree.root()) {
        if tree.is(node, PKG_TYPES_NS, "Default") {
            let extension = tree.attr(node, "", "Extension").unwrap_or("").to_lowercase();
            if !default_seen.insert(extension.clone()) {
                package.add(CONTENT_TYPES, "duplicate content type Default extension");
            }
            package.defaults.insert(extension, tree.attr(node, "", "ContentType").unwrap_or("").to_string());
        } else if tree.is(node, PKG_TYPES_NS, "Override") {
            let part = tree.attr(node, "", "PartName").unwrap_or("").to_lowercase();
            if !override_seen.insert(part.clone()) {
                package.add(CONTENT_TYPES, "duplicate content type Override part name");
            }
            if !package.lower.contains_key(part.trim_start_matches('/')) {
                package.add(CONTENT_TYPES, "content type Override names a missing part");
            }
            package.overrides.insert(part, tree.attr(node, "", "ContentType").unwrap_or("").to_string());
        }
    }
    package.trees.insert(CONTENT_TYPES.into(), Some(tree));
    for name in package.names.clone() {
        // Word parks deleted parts under [trash]/ without registering them.
        if name.ends_with('/') || name == CONTENT_TYPES || name.starts_with("[trash]/") {
            continue;
        }
        if package.content_type(&name).is_none() {
            package.add(&name, "part has no content type");
        }
    }
    true
}

fn is_xml_content_type(content_type: &str) -> bool {
    let base = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    base.ends_with("+xml") || base == "application/xml" || base == "text/xml"
}

fn parse_xml_parts(package: &mut Package) {
    for name in package.names.clone() {
        if name.ends_with('/') || name == CONTENT_TYPES || name.starts_with("[trash]/") {
            continue;
        }
        let xml = is_xml_name(&name) || package.content_type(&name).is_some_and(|t| is_xml_content_type(&t));
        if !xml {
            continue;
        }
        let parsed = tree::parse(&package.data[&name]);
        if let Err(error) = &parsed {
            package.findings.add(&name, format!("xml: {error}"));
        }
        package.trees.insert(name, parsed.ok());
    }
}

fn is_ncname(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '\u{B7}'))
}

/// Relationship types a source part may hold at most once (ECMA-376 Part 1
/// part summaries), keyed by their last segment so the Strict and
/// Transitional spellings of one type count together.
fn single_instance(rel_type: &str) -> Option<&str> {
    const SOURCES: &[&str] = &[
        OFFICE_REL,
        "http://purl.oclc.org/ooxml/officeDocument/relationships/",
        "http://schemas.openxmlformats.org/package/2006/relationships/metadata/",
        "http://schemas.microsoft.com/office/2007/relationships/",
        "http://schemas.microsoft.com/office/2011/relationships/",
        "http://schemas.microsoft.com/office/2016/09/relationships/",
        "http://schemas.microsoft.com/office/2018/08/relationships/",
    ];
    const SINGLE: &[&str] = &[
        "officeDocument", "core-properties", "extended-properties", "settings", "webSettings",
        "styles", "stylesWithEffects", "numbering", "fontTable", "theme", "footnotes", "endnotes",
        "comments", "glossaryDocument", "commentsExtended", "commentsIds", "commentsExtensible",
        "people",
    ];
    let short = SOURCES.iter().find_map(|prefix| rel_type.strip_prefix(prefix))?;
    SINGLE.contains(&short).then_some(short)
}

fn check_relationships(package: &mut Package) {
    let image_rel = format!("{OFFICE_REL}image");
    let rels_parts: Vec<String> = package
        .names
        .iter()
        .filter(|n| n.to_ascii_lowercase().ends_with(".rels"))
        .cloned()
        .collect();
    for rels_part in rels_parts {
        let Some(source) = rels_source(&rels_part) else {
            package.add(&rels_part, "relationships part name has no source part");
            continue;
        };
        if !source.is_empty() && package.actual(&source).is_none() {
            package.add(&rels_part, "relationships part has no source part");
        }
        let Some(tree) = package.tree(&rels_part) else {
            continue;
        };
        let mut ids: HashMap<String, usize> = HashMap::new();
        let mut faults = Vec::new();
        for node in tree.descendants(tree.root()) {
            if !tree.is(node, PKG_RELS_NS, "Relationship") {
                continue;
            }
            let id = tree.attr(node, "", "Id").unwrap_or("");
            *ids.entry(id.to_string()).or_default() += 1;
            if !is_ncname(id) {
                faults.push("relationship Id is not a valid xsd:ID");
            }
            if tree.attr(node, "", "Type").is_none_or(str::is_empty) {
                faults.push("relationship has no Type");
            }
            if tree.attr(node, "", "Target").is_none() {
                faults.push("relationship has no Target");
            }
            if let Some(mode) = tree.attr(node, "", "TargetMode") {
                if mode != "External" && mode != "Internal" {
                    faults.push("relationship TargetMode is neither External nor Internal");
                }
            }
        }
        for fault in faults {
            package.add(&rels_part, fault);
        }
        if ids.values().any(|&count| count > 1) {
            package.add(&rels_part, "duplicate relationship Id");
        }
        let mut single: HashMap<String, usize> = HashMap::new();
        for (_, rel) in package.relationships(&source) {
            if let Some(short) = single_instance(&rel.rel_type) {
                *single.entry(short.to_string()).or_default() += 1;
            }
        }
        let mut repeated: Vec<&String> = single.iter().filter(|(_, &n)| n > 1).map(|(k, _)| k).collect();
        repeated.sort();
        for short in repeated {
            package.add(&rels_part, format!("more than one {short} relationship"));
        }
        for (_, rel) in package.relationships(&source) {
            let short = rel.rel_type.rsplit('/').next().unwrap_or("").to_string();
            if rel.external {
                if expected_content_types(&rel.rel_type).is_some() {
                    package.add(&rels_part, format!("relationship {short} must target a package part"));
                }
                continue;
            }
            let target = rel.target.unwrap_or_default();
            let Some(actual) = package.actual(&target) else {
                package.add(&rels_part, format!("relationship {short} targets a missing part"));
                continue;
            };
            let content_type = package.content_type(&actual);
            if let Some(expected) = expected_content_types(&rel.rel_type) {
                if !content_type.as_ref().is_some_and(|t| expected.contains(t)) {
                    let mut sorted = expected.clone();
                    sorted.sort();
                    package.add(
                        &actual,
                        format!(
                            "content type {} is not what Word writes for {short} parts ({})",
                            content_type.as_deref().unwrap_or("None"),
                            sorted.join(" or ")
                        ),
                    );
                }
            }
            if rel.rel_type == image_rel {
                if let Some(t) = content_type.filter(|t| !t.starts_with("image/")) {
                    package.add(&actual, format!("image relationship targets content type {t}"));
                }
            }
        }
    }
    if package.targets("", &format!("{OFFICE_REL}officeDocument")).is_empty()
        && package.targets("", STRICT_OFFICE_DOCUMENT).is_empty()
    {
        package.add("_rels/.rels", "no officeDocument relationship");
    }
}

fn check_references(package: &mut Package, part: &str) {
    let rels = package.relationships(part);
    let Some(tree) = package.tree(part) else {
        return;
    };
    let reference_type = |node: usize, attr: &str| -> Option<String> {
        let n = &tree.nodes[node];
        let kind = match (n.ns.as_str(), n.local.as_str()) {
            (W_NS, "headerReference") => "header",
            (W_NS, "footerReference") => "footer",
            (W_NS, "hyperlink") => "hyperlink",
            (W_NS, "altChunk") => "aFChunk",
            (A_NS, "blip") if attr == "embed" => "image",
            _ => return None,
        };
        Some(format!("{OFFICE_REL}{kind}"))
    };
    let mut faults = Vec::new();
    for node in visible(tree, tree.root()) {
        let n = &tree.nodes[node];
        for attr in &n.attrs {
            if attr.ns != R_NS || attr.value.is_empty() {
                continue;
            }
            let Some((_, rel)) = rels.iter().find(|(id, _)| id == &attr.value) else {
                faults.push(format!("{}/@r:{} names no relationship of this part", n.local, attr.local));
                continue;
            };
            if let Some(expected) = reference_type(node, &attr.local) {
                if rel.rel_type != expected {
                    faults.push(format!(
                        "{} r:id names a {} relationship, not {}",
                        n.local,
                        rel.rel_type.rsplit('/').next().unwrap_or(""),
                        expected.rsplit('/').next().unwrap_or("")
                    ));
                }
            }
            if n.ns == W_NS && n.local == "hyperlink" && !rel.external {
                faults.push("hyperlink relationship is not TargetMode=\"External\"".into());
            }
        }
    }
    for fault in faults {
        package.add(part, fault);
    }
}

fn check_markup_compatibility(package: &mut Package, part: &str) {
    let Some(tree) = package.tree(part) else {
        return;
    };
    let mut faults = Vec::new();
    // Prefix lists must name declared prefixes, on the raw tree.
    for node in tree.descendants(tree.root()) {
        for (attr, label) in [("Ignorable", "mc:Ignorable"), ("MustUnderstand", "mc:MustUnderstand"), ("ProcessContent", "mc:ProcessContent")] {
            if let Some(list) = tree.attr(node, MC_NS, attr) {
                for entry in list.split_whitespace() {
                    let prefix = entry.split(':').next().unwrap_or(entry);
                    if tree.lookup_prefix(node, prefix).is_none() {
                        faults.push(format!("{label} names undeclared prefix {prefix}"));
                    }
                }
            }
        }
        if let Some(list) = tree.attr(node, MC_NS, "MustUnderstand") {
            for prefix in list.split_whitespace() {
                if let Some(uri) = tree.lookup_prefix(node, prefix) {
                    if !UNDERSTOOD_PREFIXES.iter().any(|p| uri.starts_with(p)) {
                        faults.push(format!("mc:MustUnderstand names namespace Word does not understand {uri}"));
                    }
                }
            }
        }
        if tree.is(node, MC_NS, "AlternateContent") && tree.first_child(node, MC_NS, "Choice").is_none() {
            faults.push("mc:AlternateContent has no mc:Choice".into());
        }
        if tree.is(node, MC_NS, "Choice") {
            for prefix in tree.attr(node, "", "Requires").unwrap_or("").split_whitespace() {
                if tree.lookup_prefix(node, prefix).is_none() {
                    faults.push(format!("mc:Choice Requires names undeclared prefix {prefix}"));
                }
            }
        }
    }
    // Content in a namespace Word does not understand must be ignorable or
    // inside mc:AlternateContent; otherwise Word refuses the part. Custom XML
    // data parts carry arbitrary namespaces and are not MC-processed.
    if tree.nodes[tree.root()].ns == W_NS {
        let mut ignorable = HashSet::new();
        for node in visible(tree, tree.root()) {
            for prefix in tree.attr(node, MC_NS, "Ignorable").unwrap_or("").split_whitespace() {
                if let Some(uri) = tree.lookup_prefix(node, prefix) {
                    ignorable.insert(uri.to_string());
                }
            }
        }
        for node in visible(tree, tree.root()) {
            if tree.ancestors(node).any(|a| tree.is(a, MC_NS, "Choice")) {
                continue;
            }
            let n = &tree.nodes[node];
            let names = std::iter::once(n.ns.as_str()).chain(n.attrs.iter().map(|a| a.ns.as_str()));
            for namespace in names {
                if namespace.is_empty() || namespace == MC_NS || ignorable.contains(namespace) {
                    continue;
                }
                if !UNDERSTOOD_PREFIXES.iter().any(|p| namespace.starts_with(p)) {
                    faults.push(format!("non-ignorable unknown namespace {namespace}"));
                }
            }
        }
    }
    for fault in faults {
        package.add(part, fault);
    }
}

#[derive(Default)]
struct Pairing {
    starts: HashMap<String, usize>,
    ends: HashMap<String, usize>,
}

impl Pairing {
    fn report(&self, faults: &mut Vec<String>, start: &str, end: &str, duplicate: bool) {
        let mut ids: Vec<&String> = self.starts.keys().chain(self.ends.keys()).collect();
        ids.sort();
        ids.dedup();
        for id in ids {
            let starts = self.starts.get(id).copied().unwrap_or(0);
            let ends = self.ends.get(id).copied().unwrap_or(0);
            if duplicate && starts > 1 {
                faults.push(format!("duplicate {start} id"));
            }
            if starts > 0 && ends == 0 {
                faults.push(format!("{start} without a {end}"));
            }
            if ends > 0 && starts == 0 {
                faults.push(format!("{end} without a {start}"));
            }
        }
    }
}

/// Pairing of ranges, fields and bookmarks within one story.
fn check_story(package: &mut Package, part: &str, tree: &Tree, story: usize, comment_ids: &HashSet<String>) {
    let mut faults: Vec<String> = Vec::new();
    let mut bookmarks = Pairing::default();
    let mut comments = Pairing::default();
    let mut move_from = Pairing::default();
    let mut move_to = Pairing::default();
    // Open fields; true once the field reached its separator.
    let mut fields: Vec<bool> = Vec::new();
    for node in visible(tree, story) {
        let n = &tree.nodes[node];
        if n.ns != W_NS {
            continue;
        }
        let id = || w_attr(tree, node, "id").unwrap_or("").to_string();
        match n.local.as_str() {
            "bookmarkStart" => *bookmarks.starts.entry(id()).or_default() += 1,
            "bookmarkEnd" => *bookmarks.ends.entry(id()).or_default() += 1,
            "commentRangeStart" => *comments.starts.entry(id()).or_default() += 1,
            "commentRangeEnd" => *comments.ends.entry(id()).or_default() += 1,
            "moveFromRangeStart" => *move_from.starts.entry(id()).or_default() += 1,
            "moveFromRangeEnd" => *move_from.ends.entry(id()).or_default() += 1,
            "moveToRangeStart" => *move_to.starts.entry(id()).or_default() += 1,
            "moveToRangeEnd" => *move_to.ends.entry(id()).or_default() += 1,
            "fldChar" => match w_attr(tree, node, "fldCharType") {
                Some("begin") => fields.push(false),
                Some("separate") => match fields.last_mut() {
                    None => faults.push("fldChar separate outside a field".into()),
                    Some(true) => faults.push("field has more than one fldChar separate".into()),
                    Some(separated) => *separated = true,
                },
                Some("end") => {
                    if fields.pop().is_none() {
                        faults.push("fldChar end without a begin".into());
                    }
                }
                _ => {}
            },
            "instrText" | "delInstrText" => {
                if fields.last() != Some(&false) {
                    faults.push(format!("{} outside a field code", n.local));
                }
            }
            _ => {}
        }
    }
    if !fields.is_empty() {
        faults.push("fldChar begin without an end".into());
    }
    bookmarks.report(&mut faults, "bookmarkStart", "bookmarkEnd", true);
    comments.report(&mut faults, "commentRangeStart", "commentRangeEnd", true);
    move_from.report(&mut faults, "moveFromRangeStart", "moveFromRangeEnd", true);
    move_to.report(&mut faults, "moveToRangeStart", "moveToRangeEnd", true);
    let mut range_ids: Vec<&String> = comments.starts.keys().chain(comments.ends.keys()).collect();
    range_ids.sort();
    range_ids.dedup();
    for id in range_ids {
        if !comment_ids.contains(id) {
            faults.push("comment range names no w:comment".into());
        }
    }
    for fault in faults {
        package.add(part, fault);
    }
}

/// Text markup and story shape: deleted text, significant whitespace,
/// paragraphs Word requires, drawing ids, bookmark names, move range names.
fn check_story_content(package: &mut Package, part: &str, kind: &str, tree: &Tree) {
    let mut faults: Vec<String> = Vec::new();
    let in_deletion = |node: usize| -> Option<&'static str> {
        for ancestor in tree.ancestors(node) {
            let a = &tree.nodes[ancestor];
            if a.ns != W_NS {
                continue;
            }
            match a.local.as_str() {
                "del" => return Some("del"),
                "moveFrom" => return Some("moveFrom"),
                "txbxContent" => return None,
                _ => {}
            }
        }
        None
    };
    let preserved = |node: usize| -> bool {
        std::iter::once(node)
            .chain(tree.ancestors(node))
            .find_map(|n| tree.attr(n, XML_NS, "space"))
            == Some("preserve")
    };
    let mut docpr_ids: HashMap<String, usize> = HashMap::new();
    let mut bookmark_names: HashMap<String, usize> = HashMap::new();
    let mut move_names: (HashSet<String>, HashSet<String>) = Default::default();
    for node in visible(tree, tree.root()) {
        let n = &tree.nodes[node];
        if n.ns == WP_NS && n.local == "docPr" {
            if let Some(id) = tree.attr(node, "", "id") {
                *docpr_ids.entry(id.to_string()).or_default() += 1;
            }
            continue;
        }
        if n.ns != W_NS {
            continue;
        }
        match n.local.as_str() {
            "t" | "delText" | "instrText" | "delInstrText" => {
                let text = tree.text(node);
                let edge = |c: Option<char>| c.is_some_and(|c| matches!(c, ' ' | '\t' | '\n' | '\r'));
                if (edge(text.chars().next()) || edge(text.chars().next_back())) && !preserved(node) {
                    // Named per pair: deleting or restoring converts t and
                    // delText (instrText and delInstrText) faithfully.
                    let kind = if n.local.contains("nstr") { "field code" } else { "run text" };
                    faults.push(format!("{kind} has leading or trailing space without xml:space=\"preserve\""));
                }
                let deletion = in_deletion(node);
                match (n.local.as_str(), deletion) {
                    ("delText", None) => faults.push("w:delText outside a deletion".into()),
                    ("delInstrText", None) => faults.push("w:delInstrText outside a deletion".into()),
                    ("t", Some("del")) => faults.push("w:t inside a deletion".into()),
                    ("instrText", Some("del")) => faults.push("w:instrText inside a deletion".into()),
                    _ => {}
                }
            }
            "bookmarkStart" => {
                if let Some(name) = w_attr(tree, node, "name") {
                    *bookmark_names.entry(name.to_string()).or_default() += 1;
                }
            }
            "moveFromRangeStart" => {
                move_names.0.insert(w_attr(tree, node, "name").unwrap_or("").to_string());
            }
            "moveToRangeStart" => {
                move_names.1.insert(w_attr(tree, node, "name").unwrap_or("").to_string());
            }
            "txbxContent" => {
                if !visible(tree, node).iter().any(|&d| d != node && is_w(tree, d, "p")) {
                    faults.push("w:txbxContent has no paragraph".into());
                }
            }
            "footnote" | "endnote" | "comment" if tree.nodes[node].parent == Some(tree.root()) => {
                if !visible(tree, node).iter().any(|&d| is_w(tree, d, "p")) {
                    faults.push(format!("w:{} has no paragraph", n.local));
                }
            }
            _ => {}
        }
    }
    if matches!(kind, "header" | "footer") && !visible(tree, tree.root()).iter().any(|&d| is_w(tree, d, "p")) {
        faults.push(format!("{kind} has no paragraph"));
    }
    if docpr_ids.values().any(|&count| count > 1) {
        faults.push("duplicate wp:docPr id".into());
    }
    if bookmark_names.values().any(|&count| count > 1) {
        faults.push("duplicate bookmark name".into());
    }
    if move_names.0.symmetric_difference(&move_names.1).next().is_some() {
        faults.push("move range name has no counterpart".into());
    }
    for fault in faults {
        package.add(part, fault);
    }
}

/// Rows, looking through content-control and custom XML wrappers.
fn wrapped(tree: &Tree, parent: usize, name: &str, out: &mut Vec<usize>) {
    for child in tree.element_children(parent) {
        if is_w(tree, child, name) {
            out.push(child);
        } else if is_w(tree, child, "sdt") || is_w(tree, child, "sdtContent") || is_w(tree, child, "customXml") {
            wrapped(tree, child, name, out);
        }
    }
}

fn int_attr(tree: &Tree, node: Option<usize>, default: i64) -> i64 {
    node.and_then(|n| w_attr(tree, n, "val")).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

fn check_tables(package: &mut Package, part: &str, tree: &Tree) {
    let mut faults: Vec<&str> = Vec::new();
    for table in visible(tree, tree.root()) {
        if !is_w(tree, table, "tbl") {
            continue;
        }
        let columns = tree
            .first_child(table, W_NS, "tblGrid")
            .map(|grid| tree.element_children(grid).filter(|&c| is_w(tree, c, "gridCol")).count())
            .unwrap_or(0) as i64;
        let mut rows = Vec::new();
        wrapped(tree, table, "tr", &mut rows);
        if rows.is_empty() {
            faults.push("table has no rows");
        }
        let mut above: HashMap<i64, Option<&str>> = HashMap::new();
        for row in rows {
            let mut position = 0i64;
            let mut here: HashMap<i64, Option<&str>> = HashMap::new();
            let trpr = tree.first_child(row, W_NS, "trPr");
            position += int_attr(tree, trpr.and_then(|p| tree.first_child(p, W_NS, "gridBefore")), 0);
            let mut cells = Vec::new();
            wrapped(tree, row, "tc", &mut cells);
            if cells.is_empty() {
                faults.push("table row has no cells");
            }
            for cell in cells {
                let blocks: Vec<usize> = tree
                    .element_children(cell)
                    .filter(|&c| ["p", "tbl", "sdt", "customXml"].iter().any(|n| is_w(tree, c, n)))
                    .collect();
                match blocks.last() {
                    None => faults.push("table cell has no paragraph"),
                    Some(&last) if is_w(tree, last, "tbl") => faults.push("table cell does not end with a paragraph"),
                    _ => {}
                }
                let tcpr = tree.first_child(cell, W_NS, "tcPr");
                let span = int_attr(tree, tcpr.and_then(|p| tree.first_child(p, W_NS, "gridSpan")), 1);
                let vmerge = tcpr.and_then(|p| tree.first_child(p, W_NS, "vMerge"));
                let state = vmerge.map(|v| if w_attr(tree, v, "val") == Some("restart") { "restart" } else { "continue" });
                if state == Some("continue") && !matches!(above.get(&position), Some(Some(_))) {
                    faults.push("vMerge continue cell has no merged cell above it");
                }
                here.insert(position, state);
                position += span;
            }
            position += int_attr(tree, trpr.and_then(|p| tree.first_child(p, W_NS, "gridAfter")), 0);
            if columns != 0 && position != columns {
                faults.push("row spans a different number of grid columns than tblGrid declares");
            }
            above = here;
        }
    }
    for fault in faults {
        package.add(part, fault);
    }
}

fn is_hex8_below(value: &str, limit: u64) -> bool {
    value.len() == 8 && value.bytes().all(|b| b.is_ascii_hexdigit()) && u64::from_str_radix(value, 16).is_ok_and(|v| v < limit)
}

fn check_ids(package: &mut Package, part: &str, tree: &Tree) {
    let mut faults: Vec<String> = Vec::new();
    let mut para_ids: HashMap<String, usize> = HashMap::new();
    let mut revision_ids: HashMap<String, usize> = HashMap::new();
    for node in visible(tree, tree.root()) {
        for attribute in ["paraId", "textId"] {
            let Some(value) = tree.attr(node, W14_NS, attribute) else {
                continue;
            };
            if !is_hex8_below(value, 0x8000_0000) {
                faults.push(format!("w14:{attribute} is not eight hex digits below 80000000"));
            } else if attribute == "paraId" {
                *para_ids.entry(value.to_uppercase()).or_default() += 1;
            }
        }
        let n = &tree.nodes[node];
        if n.ns == W_NS && REVISION_ELEMENTS.contains(&n.local.as_str()) {
            if let Some(id) = w_attr(tree, node, "id") {
                *revision_ids.entry(id.to_string()).or_default() += 1;
            }
        }
    }
    if para_ids.values().any(|&c| c > 1) {
        faults.push("duplicate w14:paraId".into());
    }
    if revision_ids.values().any(|&c| c > 1) {
        faults.push("duplicate revision w:id".into());
    }
    for fault in faults {
        package.add(part, fault);
    }
}

/// `w14:paraId` identifies a paragraph across the whole document; comment
/// extension parts address comment paragraphs by it.
fn check_paragraph_ids_across_parts(package: &mut Package, story_parts: &[(String, &str)]) {
    let mut owner: HashMap<String, String> = HashMap::new();
    let mut faults = Vec::new();
    for (part, _) in story_parts {
        let Some(tree) = package.tree(part) else {
            continue;
        };
        let mut own = HashSet::new();
        for node in visible(tree, tree.root()) {
            if let Some(value) = tree.attr(node, W14_NS, "paraId") {
                own.insert(value.to_uppercase());
            }
        }
        for value in own {
            match owner.get(&value) {
                Some(first) if first != part => faults.push(part.clone()),
                Some(_) => {}
                None => {
                    owner.insert(value, part.clone());
                }
            }
        }
    }
    faults.dedup();
    for part in faults {
        package.add(&part, "w14:paraId also used in another part");
    }
}

fn first_target(package: &mut Package, main: &str, rel: &str) -> Option<String> {
    package.targets(main, rel).first().and_then(|t| package.actual(t))
}

/// The part the engine edits for a relationship type: the related part, or,
/// when the main document has no such relationship, the part at the name the
/// engine adopts (`word/comments.xml`). Faults already latent in an unlinked
/// part are then pre-existing, not introduced by the edit that links it.
fn adopted_part(package: &mut Package, main: &str, rel: &str, file: &str) -> Option<String> {
    if !package.targets(main, rel).is_empty() {
        return first_target(package, main, rel);
    }
    let directory = main.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let name = if directory.is_empty() { file.to_string() } else { format!("{directory}/{file}") };
    package.actual(&name)
}

fn check_comments(package: &mut Package, main: &str, story_parts: &[(String, &str)]) -> HashSet<String> {
    let mut comment_ids = HashSet::new();
    let mut last_para: HashMap<String, String> = HashMap::new();
    let comments_part = adopted_part(package, main, &format!("{OFFICE_REL}comments"), "comments.xml");
    let mut faults: Vec<(String, String)> = Vec::new();
    if let Some(part) = &comments_part {
        if let Some(tree) = package.tree(part) {
            let mut ids: HashMap<String, usize> = HashMap::new();
            for comment in tree.element_children(tree.root()).filter(|&c| is_w(tree, c, "comment")) {
                let id = w_attr(tree, comment, "id").unwrap_or("").to_string();
                *ids.entry(id.clone()).or_default() += 1;
                comment_ids.insert(id.clone());
                let paragraphs: Vec<usize> = visible(tree, comment).into_iter().filter(|&p| is_w(tree, p, "p")).collect();
                if let Some(para) = paragraphs.last().and_then(|&p| tree.attr(p, W14_NS, "paraId")) {
                    last_para.insert(para.to_uppercase(), id);
                }
            }
            if ids.values().any(|&c| c > 1) {
                faults.push((part.clone(), "duplicate w:comment id".into()));
            }
        }
    }
    let mut referenced = HashSet::new();
    for (part, _) in story_parts {
        let Some(tree) = package.tree(part) else {
            continue;
        };
        for node in visible(tree, tree.root()) {
            if is_w(tree, node, "commentReference") {
                let id = w_attr(tree, node, "id").unwrap_or("").to_string();
                if !comment_ids.contains(&id) {
                    faults.push((part.clone(), "commentReference names no w:comment".into()));
                }
                referenced.insert(id);
            }
        }
    }
    if let Some(part) = &comments_part {
        for _ in comment_ids.difference(&referenced) {
            faults.push((part.clone(), "w:comment has no commentReference".into()));
        }
    }

    if let Some(part) = adopted_part(package, main, &format!("{MS_REL}2011/relationships/commentsExtended"), "commentsExtended.xml") {
        if let Some(tree) = package.tree(&part) {
            let entries: Vec<usize> = tree.element_children(tree.root()).filter(|&c| tree.is(c, W15_NS, "commentEx")).collect();
            let mut own: HashMap<String, usize> = HashMap::new();
            for &entry in &entries {
                *own.entry(tree.attr(entry, W15_NS, "paraId").unwrap_or("").to_uppercase()).or_default() += 1;
            }
            for &entry in &entries {
                let para = tree.attr(entry, W15_NS, "paraId").unwrap_or("").to_uppercase();
                if !last_para.contains_key(&para) {
                    faults.push((part.clone(), "commentEx paraId is not the last paragraph of a comment".into()));
                }
                if let Some(parent) = tree.attr(entry, W15_NS, "paraIdParent") {
                    if !own.contains_key(&parent.to_uppercase()) {
                        faults.push((part.clone(), "commentEx paraIdParent names no commentEx".into()));
                    }
                }
            }
            if own.values().any(|&c| c > 1) {
                faults.push((part.clone(), "duplicate commentEx paraId".into()));
            }
        }
    }

    let mut durable = HashSet::new();
    if let Some(part) = adopted_part(package, main, &format!("{MS_REL}2016/09/relationships/commentsIds"), "commentsIds.xml") {
        if let Some(tree) = package.tree(&part) {
            let mut seen: HashMap<String, usize> = HashMap::new();
            for entry in tree.element_children(tree.root()).filter(|&c| tree.is(c, W16CID_NS, "commentId")) {
                if !last_para.contains_key(&tree.attr(entry, W16CID_NS, "paraId").unwrap_or("").to_uppercase()) {
                    faults.push((part.clone(), "commentId paraId is not the last paragraph of a comment".into()));
                }
                let value = tree.attr(entry, W16CID_NS, "durableId").unwrap_or("");
                *seen.entry(value.to_uppercase()).or_default() += 1;
                durable.insert(value.to_uppercase());
                if !(is_hex8_below(value, 0x7FFF_FFFF) && u64::from_str_radix(value, 16).is_ok_and(|v| v > 0)) {
                    faults.push((part.clone(), "durableId is not eight hex digits in (0, 7FFFFFFF)".into()));
                }
            }
            if seen.values().any(|&c| c > 1) {
                faults.push((part.clone(), "duplicate durableId".into()));
            }
        }
    }
    if let Some(part) = adopted_part(package, main, &format!("{MS_REL}2018/08/relationships/commentsExtensible"), "commentsExtensible.xml") {
        if let Some(tree) = package.tree(&part) {
            for entry in tree.element_children(tree.root()).filter(|&c| tree.is(c, W16CEX_NS, "commentExtensible")) {
                if !durable.contains(&tree.attr(entry, W16CEX_NS, "durableId").unwrap_or("").to_uppercase()) {
                    faults.push((part.clone(), "commentExtensible durableId is not in commentsIds".into()));
                }
            }
        }
    }
    for (part, fault) in faults {
        package.add(&part, fault);
    }
    comment_ids
}

/// Style definitions. References from stories to undefined styles are not
/// checked: Word falls back to the default style, and the engine's comments
/// name `CommentText`/`CommentReference` without defining them.
fn check_styles(package: &mut Package, main: &str) {
    let Some(part) = first_target(package, main, &format!("{OFFICE_REL}styles")) else {
        return;
    };
    let Some(tree) = package.tree(&part) else {
        return;
    };
    let mut ids: HashMap<String, usize> = HashMap::new();
    let styles: Vec<usize> = tree.element_children(tree.root()).filter(|&c| is_w(tree, c, "style")).collect();
    for &style in &styles {
        *ids.entry(w_attr(tree, style, "styleId").unwrap_or("").to_string()).or_default() += 1;
    }
    let mut faults: Vec<String> = Vec::new();
    if ids.values().any(|&c| c > 1) {
        faults.push("duplicate styleId".into());
    }
    for &style in &styles {
        for reference in ["basedOn", "next", "link"] {
            if let Some(value) = tree.first_child(style, W_NS, reference).and_then(|r| w_attr(tree, r, "val")) {
                if !ids.contains_key(value) {
                    faults.push(format!("style {reference} names no style"));
                }
            }
        }
    }
    for fault in faults {
        package.add(&part, fault);
    }
}

fn check_sections(package: &mut Package, main: &str) {
    let Some(tree) = package.tree(main) else {
        return;
    };
    let mut faults = Vec::new();
    for section in visible(tree, tree.root()).into_iter().filter(|&n| is_w(tree, n, "sectPr")) {
        for reference in ["headerReference", "footerReference"] {
            let mut seen = HashSet::new();
            for node in tree.element_children(section).filter(|&c| is_w(tree, c, reference)) {
                if !seen.insert(w_attr(tree, node, "type").unwrap_or("default")) {
                    faults.push(format!("duplicate {reference} type in a section"));
                }
            }
        }
    }
    for fault in faults {
        package.add(main, fault);
    }
}

fn check_numbering(package: &mut Package, main: &str, numbered_parts: &[String]) {
    let mut nums = HashSet::new();
    let numbering = adopted_part(package, main, &format!("{OFFICE_REL}numbering"), "numbering.xml");
    let mut faults: Vec<(String, String)> = Vec::new();
    if let Some(part) = &numbering {
        if let Some(tree) = package.tree(part) {
            let mut abstracts: HashMap<String, usize> = HashMap::new();
            for node in tree.element_children(tree.root()).filter(|&c| is_w(tree, c, "abstractNum")) {
                *abstracts.entry(w_attr(tree, node, "abstractNumId").unwrap_or("").to_string()).or_default() += 1;
                let mut levels = HashSet::new();
                for level in tree.element_children(node).filter(|&c| is_w(tree, c, "lvl")) {
                    if !levels.insert(w_attr(tree, level, "ilvl").unwrap_or("").to_string()) {
                        faults.push((part.clone(), "duplicate lvl ilvl in an abstractNum".into()));
                    }
                }
            }
            if abstracts.values().any(|&c| c > 1) {
                faults.push((part.clone(), "duplicate abstractNumId".into()));
            }
            let mut num_counts: HashMap<String, usize> = HashMap::new();
            for num in tree.element_children(tree.root()).filter(|&c| is_w(tree, c, "num")) {
                let id = w_attr(tree, num, "numId").unwrap_or("").to_string();
                *num_counts.entry(id.clone()).or_default() += 1;
                nums.insert(id);
                let reference = tree.first_child(num, W_NS, "abstractNumId");
                if !reference.and_then(|r| w_attr(tree, r, "val")).is_some_and(|v| abstracts.contains_key(v)) {
                    faults.push((part.clone(), "w:num names no abstractNum".into()));
                }
            }
            if num_counts.values().any(|&c| c > 1) {
                faults.push((part.clone(), "duplicate w:num numId".into()));
            }
        }
    }
    for part in numbered_parts {
        let Some(tree) = package.tree(part) else {
            continue;
        };
        for numpr in visible(tree, tree.root()).into_iter().filter(|&n| is_w(tree, n, "numPr")) {
            if let Some(value) = tree.first_child(numpr, W_NS, "numId").and_then(|n| w_attr(tree, n, "val")) {
                if value != "0" && !nums.contains(value) {
                    faults.push((part.clone(), "numPr numId names no w:num".into()));
                }
            }
            if let Some(level) = tree.first_child(numpr, W_NS, "ilvl") {
                if !w_attr(tree, level, "val").is_some_and(|v| v.len() == 1 && ('0'..='8').contains(&v.chars().next().unwrap())) {
                    faults.push((part.clone(), "numPr ilvl is outside 0-8".into()));
                }
            }
        }
    }
    for (part, fault) in faults {
        package.add(&part, fault);
    }
}

fn check_notes(package: &mut Package, main: &str, story_parts: &[(String, &str)]) {
    let settings = adopted_part(package, main, &format!("{OFFICE_REL}settings"), "settings.xml");
    let mut faults: Vec<(String, String)> = Vec::new();
    for kind in ["footnote", "endnote"] {
        let notes = first_target(package, main, &format!("{OFFICE_REL}{kind}s"));
        let ids: HashSet<String> = notes
            .as_ref()
            .and_then(|p| package.tree(p))
            .map(|tree| {
                tree.element_children(tree.root())
                    .filter(|&c| is_w(tree, c, kind))
                    .map(|c| w_attr(tree, c, "id").unwrap_or("").to_string())
                    .collect()
            })
            .unwrap_or_default();
        for (part, _) in story_parts {
            let Some(tree) = package.tree(part) else {
                continue;
            };
            for node in visible(tree, tree.root()) {
                if is_w(tree, node, &format!("{kind}Reference")) && !ids.contains(w_attr(tree, node, "id").unwrap_or("")) {
                    faults.push((part.clone(), format!("{kind}Reference names no w:{kind}")));
                }
            }
        }
        // Separator notes named by the document-wide note properties.
        if let Some(tree) = settings.as_ref().and_then(|p| package.tree(p)) {
            if let Some(properties) = tree.first_child(tree.root(), W_NS, &format!("{kind}Pr")) {
                for note in tree.element_children(properties).filter(|&c| is_w(tree, c, kind)) {
                    if !ids.contains(w_attr(tree, note, "id").unwrap_or("")) {
                        faults.push((settings.clone().unwrap(), format!("settings {kind}Pr names no w:{kind}")));
                    }
                }
            }
        }
    }
    for (part, fault) in faults {
        package.add(&part, fault);
    }
}

/// Error kinds `after` has that `before` did not. A schema finding is known
/// when `before` had it in any part: the bundled schemas predate markup Word
/// writes (`w:hint="cs"`), and an edit that carries such existing formatting
/// into another part, as a new comment does, has not damaged anything.
pub fn introduced(before: &Findings, after: &Findings) -> Vec<(String, String)> {
    let known = before.kinds();
    let known_schema: HashSet<&str> = known
        .iter()
        .filter(|(_, message)| message.starts_with("schema: "))
        .map(|(_, message)| message.as_str())
        .collect();
    after
        .kinds()
        .into_iter()
        .filter(|kind| !known.contains(kind) && !known_schema.contains(kind.1.as_str()))
        .collect()
}

/// Parts whose bytes differ between two packages, present in both.
fn changed_parts(input: &[u8], output: &[u8]) -> Vec<(String, Vec<u8>)> {
    let read = |bytes: &[u8]| -> HashMap<String, Vec<u8>> {
        let mut ignored = Findings::default();
        read_entries(bytes, &mut ignored)
            .unwrap_or_default()
            .into_iter()
            .map(|e| (e.name, e.data))
            .collect()
    };
    let before = read(input);
    let after = read(output);
    let mut changed: Vec<(String, Vec<u8>)> = before
        .into_iter()
        .filter(|(name, data)| after.get(name).is_some_and(|new| new != data))
        .collect();
    changed.sort_by(|a, b| a.0.cmp(&b.0));
    changed
}

/// Whether the engine's XML reader and writer reproduce `original` exactly,
/// judged by an independent parser. A part they cannot round-trip would be
/// silently altered outside the edit, so it may not be rewritten.
pub fn round_trip_fault(original: &[u8]) -> Option<String> {
    let Ok(reference) = tree::parse(original) else {
        // The output check reports a malformed part; nothing to preserve here.
        return None;
    };
    let written = match crate::document::engine_round_trip(original) {
        Ok(written) => written,
        Err(_) => return Some("the engine's XML reader rejects it".into()),
    };
    match tree::parse(&written) {
        Ok(copy) => tree::first_difference(&reference, &copy),
        Err(error) => Some(format!("the engine writes it back malformed ({error})")),
    }
}

/// A commit-gate refusal: what the edit would have broken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateFailure {
    /// Error kinds the edit would introduce, `(part, message)`.
    pub introduced: Vec<(String, String)>,
    /// Rewritten parts the engine cannot reproduce faithfully, `(part, difference)`.
    pub lossy: Vec<(String, String)>,
}

impl GateFailure {
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        for (part, difference) in &self.lossy {
            lines.push(format!("{part}: rewriting this part would alter content the edit does not touch ({difference})"));
        }
        for (part, message) in &self.introduced {
            lines.push(format!("{}: {message}", if part.is_empty() { "package" } else { part }));
        }
        const SHOWN: usize = 12;
        let more = lines.len().saturating_sub(SHOWN);
        lines.truncate(SHOWN);
        if more > 0 {
            lines.push(format!("and {more} more"));
        }
        format!(
            "the edit would damage the document, so nothing was written; the source is unchanged. {}",
            lines.join("; ")
        )
    }
}

/// The commit rule: `output` may not have an error kind `input` lacks, and
/// every part rewritten must round-trip through the engine faithfully.
pub fn gate(input: &[u8], output: &[u8]) -> Result<(), GateFailure> {
    let lossy: Vec<(String, String)> = changed_parts(input, output)
        .into_iter()
        .filter(|(name, _)| is_xml_name(name))
        .filter_map(|(name, original)| round_trip_fault(&original).map(|fault| (name, fault)))
        .collect();
    let new = introduced(&check(input), &check(output));
    if lossy.is_empty() && new.is_empty() {
        Ok(())
    } else {
        Err(GateFailure { introduced: new, lossy })
    }
}

/// A package built from nothing (create) must have no fault at all.
pub fn gate_created(output: &[u8]) -> Result<(), GateFailure> {
    let findings = check(output);
    if findings.is_empty() {
        Ok(())
    } else {
        Err(GateFailure { introduced: findings.kinds().into_iter().collect(), lossy: Vec::new() })
    }
}
