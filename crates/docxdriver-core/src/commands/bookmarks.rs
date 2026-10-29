//! Bookmarks: `w:bookmarkStart`/`w:bookmarkEnd` pairs matched by `w:id`.
//!
//! Reading covers every story (body, headers, footers, footnotes, endnotes,
//! comments). Editing is body-only (`word/document.xml`): `bookmark_add`,
//! `bookmark_rename`, `bookmark_move` and `bookmark_delete` refuse a bookmark
//! that lives in another part. Word does not track bookmark changes, so every
//! operation applies directly in both change modes.
//!
//! Markers are placed as siblings of runs, so a bookmark never splits a run:
//! the run at each `select` boundary is split first, as comment ranges are.
//! A marker climbs out of wrappers (`w:ins`, `w:hyperlink`, …) whenever that
//! does not change the bookmarked text, so most markers sit directly in the
//! `w:p`.

use std::collections::HashSet;

use serde_json::{json, Value};
use xmloxide::tree::NodeId;

use crate::document::DocxXml;
use crate::outcome::Outcome;
use crate::package::Package;
use crate::segments::{build_segments, split_boundary};

use super::typed::{blocked_at, resolve_paragraph_by_para_id, select_in_paragraph};

/// Word's limit on bookmark name length.
const MAX_NAME_CHARS: usize = 40;
/// Characters of bookmarked text a read reports before truncating.
const TEXT_LIMIT: usize = 200;

const STORY_RELATIONSHIPS: &[&str] = &["header", "footer", "footnotes", "endnotes", "comments"];

/// Zero-width markers a bookmark marker may climb past without changing
/// the text it bookmarks.
const ZERO_WIDTH: &[&str] = &[
    "bookmarkStart",
    "bookmarkEnd",
    "commentRangeStart",
    "commentRangeEnd",
    "moveFromRangeStart",
    "moveFromRangeEnd",
    "moveToRangeStart",
    "moveToRangeEnd",
    "permStart",
    "permEnd",
    "proofErr",
];

const NAME_RULE: &str = "a bookmark name starts with a letter, continues with letters, digits or underscores, and has at most 40 characters (for example \"Clause_4_2\")";

/// Name format as Word enforces it, for a name an operation creates.
/// A leading underscore marks a hidden or reserved name (_GoBack, _Toc…,
/// _docxdriver_cont…), which only Word and docxdriver itself create.
pub(crate) fn validate_new_name(name: &str, field: &str) -> Result<(), String> {
    if name.starts_with('_') {
        return Err(format!(
            "{field} {name:?} starts with \"_\", which marks a hidden or reserved bookmark (_GoBack, _Toc…); choose a name that starts with a letter"
        ));
    }
    let mut chars = name.chars();
    let valid = chars.next().is_some_and(char::is_alphabetic)
        && chars.all(|c| c.is_alphanumeric() || c == '_');
    if !valid {
        let hint = if name.is_empty() {
            "it is empty".to_string()
        } else if name.contains(' ') {
            format!(
                "use underscores instead of spaces, e.g. {:?}",
                name.replace(' ', "_")
            )
        } else if name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            "it starts with a digit".to_string()
        } else {
            "it contains a character other than a letter, digit or underscore".to_string()
        };
        return Err(format!(
            "{field} {name:?} is not a valid bookmark name: {hint}; {NAME_RULE}"
        ));
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!(
            "{field} {name:?} has {} characters; {NAME_RULE}",
            name.chars().count()
        ));
    }
    Ok(())
}

/// An existing bookmark may carry any name, hidden ones included.
pub(crate) fn validate_existing_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err(
            "name must name an existing bookmark; read kind=\"bookmarks\" lists them".into(),
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Stories
// ---------------------------------------------------------------------------

/// The other story parts the main document relates to, in relationship
/// order: headers, footers, footnotes, endnotes and comments.
fn story_parts(package: &Package) -> Vec<String> {
    let main = package.document_part_name();
    let dir = main.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let Some(rels) = package
        .get(&super::document_rels_part(package))
        .and_then(|bytes| DocxXml::parse(bytes).ok())
    else {
        return Vec::new();
    };
    let Some(root) = rels.doc.root_element() else {
        return Vec::new();
    };
    let mut parts = Vec::new();
    for kind in STORY_RELATIONSHIPS {
        for rel in rels.doc.children(root) {
            if !rels.is_local(rel, "Relationship")
                || rels.doc.attribute(rel, "TargetMode") == Some("External")
            {
                continue;
            }
            let Some(rel_type) = rels.doc.attribute(rel, "Type") else {
                continue;
            };
            if rel_type.rsplit('/').next() != Some(kind) {
                continue;
            }
            let Some(target) = rels.doc.attribute(rel, "Target") else {
                continue;
            };
            let part = match target.strip_prefix('/') {
                Some(absolute) => absolute.to_string(),
                None if dir.is_empty() => target.to_string(),
                None => format!("{dir}/{target}"),
            };
            if package.get(&part).is_some() && !parts.contains(&part) {
                parts.push(part);
            }
        }
    }
    parts
}

/// One bookmark start in some story: its name, `w:id` and part.
struct Known {
    name: String,
    id: String,
    part: String,
}

fn starts_in(xml: &DocxXml, part: &str, out: &mut Vec<Known>) {
    let Some(root) = xml.doc.root_element() else {
        return;
    };
    for node in xml.doc.descendants(root) {
        if xml.is_w(node, "bookmarkStart") {
            out.push(Known {
                name: xml.attr(node, "name").unwrap_or("").to_string(),
                id: xml.attr(node, "id").unwrap_or("").to_string(),
                part: part.to_string(),
            });
        }
    }
}

/// Every bookmark start in the document: the in-memory body first, then
/// the other stories as the package holds them.
fn all_known(package: &Package, body: &DocxXml) -> Vec<Known> {
    let mut known = Vec::new();
    starts_in(body, &package.document_part_name(), &mut known);
    for part in story_parts(package) {
        if let Some(xml) = package
            .get(&part)
            .and_then(|bytes| DocxXml::parse(bytes).ok())
        {
            starts_in(&xml, &part, &mut known);
        }
    }
    known
}

/// A fresh `w:id`, unique among the bookmark ids of every story.
fn next_bookmark_id(known: &[Known]) -> String {
    let used: HashSet<i64> = known.iter().filter_map(|k| k.id.parse().ok()).collect();
    let next = used.iter().copied().max().map_or(0, |max| max + 1);
    if next <= i64::from(i32::MAX) {
        return next.to_string();
    }
    (0..).find(|id| !used.contains(id)).unwrap_or(0).to_string()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let substitution = previous + usize::from(ca != *cb);
            previous = row[j + 1];
            row[j + 1] = substitution.min(row[j] + 1).min(row[j + 1] + 1);
        }
    }
    row[b.len()]
}

/// Up to five names close to `wanted`, closest first. Hidden names are
/// offered only for a hidden query.
fn close_matches(known: &[Known], wanted: &str) -> Vec<String> {
    let wanted_lower = wanted.to_lowercase();
    let limit = (wanted.chars().count() / 3).max(2);
    let mut scored: Vec<(usize, String)> = known
        .iter()
        .filter(|k| wanted.starts_with('_') || !k.name.starts_with('_'))
        .filter_map(|k| {
            let lower = k.name.to_lowercase();
            let distance = levenshtein(&wanted_lower, &lower);
            let related = lower.contains(&wanted_lower) || wanted_lower.contains(&lower);
            (distance <= limit || (related && !lower.is_empty()))
                .then(|| (distance, k.name.clone()))
        })
        .collect();
    scored.sort();
    scored.dedup_by(|a, b| a.1 == b.1);
    scored.into_iter().take(5).map(|(_, name)| name).collect()
}

fn unknown_name(known: &[Known], name: &str) -> Outcome {
    let matches = close_matches(known, name);
    let hint = if matches.is_empty() {
        let visible = known.iter().filter(|k| !k.name.starts_with('_')).count();
        format!(
            "the document has {visible} visible bookmark{}",
            if visible == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "close matches: {}",
            matches
                .iter()
                .map(|m| format!("{m:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    Outcome::blocked(format!(
        "no bookmark named {name:?}; {hint} — read kind=\"bookmarks\" lists every bookmark"
    ))
}

/// Reject `name` when another bookmark (other than `except_id` in the
/// body) already uses it, ignoring case.
fn ensure_unused(
    known: &[Known],
    name: &str,
    body_part: &str,
    except_id: Option<&str>,
) -> Result<(), Outcome> {
    let lower = name.to_lowercase();
    let clash = known.iter().find(|k| {
        k.name.to_lowercase() == lower && !(k.part == body_part && Some(k.id.as_str()) == except_id)
    });
    match clash {
        Some(existing) => Err(Outcome::blocked(format!(
            "bookmark name {name:?} is already used by bookmark {:?}{} — names are unique in the document, ignoring case; choose another name, or bookmark_move / bookmark_rename / bookmark_delete the existing one",
            existing.name,
            if existing.part == body_part { String::new() } else { format!(" in {}", existing.part) }
        ))),
        None => Ok(()),
    }
}

/// The body bookmark start named `name`: an exact match first, then the
/// one matching ignoring case (Word treats names case-insensitively).
fn find_body_start(package: &Package, xml: &DocxXml, name: &str) -> Result<NodeId, Outcome> {
    let root = xml
        .doc
        .root_element()
        .ok_or_else(|| Outcome::error("document part has no root"))?;
    let starts: Vec<NodeId> = xml
        .doc
        .descendants(root)
        .filter(|&node| xml.is_w(node, "bookmarkStart"))
        .collect();
    if let Some(&node) = starts
        .iter()
        .find(|&&node| xml.attr(node, "name") == Some(name))
    {
        return Ok(node);
    }
    let lower = name.to_lowercase();
    if let Some(&node) = starts.iter().find(|&&node| {
        xml.attr(node, "name")
            .is_some_and(|n| n.to_lowercase() == lower)
    }) {
        return Ok(node);
    }
    let known = all_known(package, xml);
    if let Some(elsewhere) = known.iter().find(|k| k.name.to_lowercase() == lower) {
        return Err(Outcome::blocked(format!(
            "bookmark {:?} is in {}; bookmark editing is body-only ({}) — headers, footers and notes are read-only for bookmarks",
            elsewhere.name,
            elsewhere.part,
            package.document_part_name()
        )));
    }
    Err(unknown_name(&known, name))
}

/// Every body `bookmarkEnd` carrying `id`.
fn body_ends(xml: &DocxXml, id: &str) -> Vec<NodeId> {
    let Some(root) = xml.doc.root_element() else {
        return Vec::new();
    };
    xml.doc
        .descendants(root)
        .filter(|&node| xml.is_w(node, "bookmarkEnd") && xml.attr(node, "id") == Some(id))
        .collect()
}

fn enclosing_paragraph(xml: &DocxXml, node: NodeId) -> Option<NodeId> {
    xml.doc.ancestors(node).find(|&a| xml.is_w(a, "p"))
}

// ---------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------

fn is_mc(xml: &DocxXml, node: NodeId) -> bool {
    xml.is_local(node, "AlternateContent")
        || xml.is_local(node, "Choice")
        || xml.is_local(node, "Fallback")
}

/// Property and zero-width siblings occupy no text.
fn is_textless(xml: &DocxXml, node: NodeId) -> bool {
    if !xml.doc.is_element(node) {
        return true;
    }
    let local = xml.local_name(node).unwrap_or("");
    local.ends_with("Pr") || local == "fldData" || ZERO_WIDTH.iter().any(|m| xml.is_w(node, m))
}

/// Where a marker goes for the run at a selection edge: the run itself, or
/// the outermost wrapper the run begins (`start`) or ends (`!start`), up to
/// a direct child of the paragraph. Markup-compatibility branches are never
/// entered: there the marker goes beside the whole `mc:AlternateContent`.
fn marker_anchor(xml: &DocxXml, run: NodeId, start: bool) -> NodeId {
    let path: Vec<NodeId> = std::iter::once(run)
        .chain(xml.doc.ancestors(run).take_while(|&a| !xml.is_w(a, "p")))
        .collect();
    if path.iter().any(|&n| is_mc(xml, n)) {
        return *path.last().unwrap_or(&run);
    }
    let mut node = run;
    while let Some(parent) = xml.doc.parent(node) {
        if xml.is_w(parent, "p") {
            break;
        }
        let mut siblings = if start {
            Box::new(std::iter::successors(xml.doc.prev_sibling(node), |&n| {
                xml.doc.prev_sibling(n)
            })) as Box<dyn Iterator<Item = NodeId>>
        } else {
            Box::new(std::iter::successors(xml.doc.next_sibling(node), |&n| {
                xml.doc.next_sibling(n)
            }))
        };
        if !siblings.all(|n| is_textless(xml, n)) {
            break;
        }
        node = parent;
    }
    node
}

/// Insert `start_node`/`end_node` into `paragraph` around the whole content
/// or the `select` span. Returns (matched, whole_paragraph).
fn place(
    xml: &mut DocxXml,
    paragraph: NodeId,
    address: &str,
    select: Option<&str>,
    occurrence: Option<usize>,
    start_node: NodeId,
    end_node: NodeId,
) -> Result<usize, Outcome> {
    let Some(select) = select else {
        if occurrence.is_some() {
            return Err(blocked_at(address, "occurrence requires select"));
        }
        match xml.child_w(paragraph, "pPr") {
            Some(ppr) => xml.doc.insert_after(ppr, start_node),
            None => match xml.doc.first_child(paragraph) {
                Some(first) => xml.doc.insert_before(first, start_node),
                None => xml.doc.append_child(paragraph, start_node),
            },
        }
        xml.doc.append_child(paragraph, end_node);
        return Ok(0);
    };
    if select.contains('<') {
        return Err(blocked_at(
            address,
            "select must be plain text without HTML markup — select addresses the paragraph's visible text",
        ));
    }
    let (start, end, _current, matched) =
        select_in_paragraph(xml, paragraph, select, occurrence)
            .map_err(|outcome| blocked_at(address, outcome.summary))?;
    if start >= end {
        return Err(blocked_at(address, "the selection covers no text"));
    }
    split_boundary(xml, paragraph, end);
    split_boundary(xml, paragraph, start);
    let segments = build_segments(xml, paragraph);
    let covered: Vec<&crate::segments::Segment> = segments
        .iter()
        .filter(|s| s.start >= start && s.end <= end && s.start < s.end)
        .collect();
    let (Some(first), Some(last)) = (covered.first(), covered.last()) else {
        return Err(blocked_at(address, "the selection covers no text"));
    };
    let first_anchor = marker_anchor(xml, first.run, true);
    let last_anchor = marker_anchor(xml, last.run, false);
    xml.doc.insert_before(first_anchor, start_node);
    xml.doc.insert_after(last_anchor, end_node);
    Ok(matched)
}

fn marker(xml: &mut DocxXml, start: bool, id: &str, name: &str) -> NodeId {
    let node = xml.doc.create_element(if start {
        "w:bookmarkStart"
    } else {
        "w:bookmarkEnd"
    });
    xml.doc.set_attribute(node, "w:id", id);
    if start {
        xml.doc.set_attribute(node, "w:name", name);
    }
    node
}

fn summary_target(
    address: &str,
    select: Option<&str>,
    occurrence: Option<usize>,
    matched: usize,
) -> String {
    match (select, occurrence) {
        (Some(_), Some(n)) => format!("occurrence {n} of {matched} in paragraph {address}"),
        (Some(_), None) => format!("the 1 match in paragraph {address}"),
        (None, _) => format!("paragraph {address}"),
    }
}

pub(crate) struct Anchor<'a> {
    pub at: &'a str,
    pub select: Option<&'a str>,
    pub occurrence: Option<usize>,
}

fn resolve_body_paragraph(
    package: &Package,
    xml: &mut DocxXml,
    source: Option<[u8; 32]>,
    at: &str,
) -> Result<(NodeId, String), Outcome> {
    let address = at.to_uppercase();
    if !crate::document::is_valid_para_id(&address) {
        return Err(Outcome::error(format!(
            "at must be a valid w14:paraId (eight hexadecimal digits with 00000000 < value < 80000000): {at:?}"
        )));
    }
    let paragraph = resolve_paragraph_by_para_id(xml, package, source, &address)?;
    Ok((paragraph, address))
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

pub(crate) fn add(
    package: &mut Package,
    xml: &mut DocxXml,
    source: Option<[u8; 32]>,
    name: &str,
    anchor: &Anchor,
) -> Result<(String, Value), Outcome> {
    validate_new_name(name, "name").map_err(Outcome::error)?;
    let (paragraph, address) = resolve_body_paragraph(package, xml, source, anchor.at)?;
    let known = all_known(package, xml);
    ensure_unused(&known, name, &package.document_part_name(), None)
        .map_err(|outcome| blocked_at(&address, outcome.summary))?;
    let id = next_bookmark_id(&known);
    let start_node = marker(xml, true, &id, name);
    let end_node = marker(xml, false, &id, name);
    let matched = place(
        xml,
        paragraph,
        &address,
        anchor.select,
        anchor.occurrence,
        start_node,
        end_node,
    )?;
    let summary = format!(
        "bookmark {name:?} (id {id}) added around {}",
        summary_target(&address, anchor.select, anchor.occurrence, matched)
    );
    Ok((
        summary,
        json!({
            "paraId": address,
            "bookmark": name,
            "bookmarkId": id,
            "matched": matched,
            "occurrence": anchor.occurrence,
            "wholeParagraph": anchor.select.is_none(),
        }),
    ))
}

pub(crate) fn rename(
    package: &mut Package,
    xml: &mut DocxXml,
    name: &str,
    new_name: &str,
) -> Result<(String, Value), Outcome> {
    validate_new_name(new_name, "new_name").map_err(Outcome::error)?;
    let start = find_body_start(package, xml, name)?;
    let id = xml.attr(start, "id").unwrap_or("").to_string();
    let old = xml.attr(start, "name").unwrap_or("").to_string();
    let known = all_known(package, xml);
    ensure_unused(&known, new_name, &package.document_part_name(), Some(&id))?;
    super::layout::set_attr(xml, start, "w:name", new_name);
    let para_id = enclosing_paragraph(xml, start)
        .and_then(|p| xml.para_id(p))
        .map(str::to_owned);
    let mut result = json!({"bookmark": new_name, "previousName": old, "bookmarkId": id});
    if let Some(para_id) = para_id {
        result["paraId"] = json!(para_id);
    }
    Ok((
        format!("bookmark {old:?} (id {id}) renamed to {new_name:?}"),
        result,
    ))
}

pub(crate) fn move_to(
    package: &mut Package,
    xml: &mut DocxXml,
    source: Option<[u8; 32]>,
    name: &str,
    anchor: &Anchor,
) -> Result<(String, Value), Outcome> {
    let start = find_body_start(package, xml, name)?;
    let (paragraph, address) = resolve_body_paragraph(package, xml, source, anchor.at)?;
    let id = xml.attr(start, "id").unwrap_or("").to_string();
    let actual = xml.attr(start, "name").unwrap_or(name).to_string();
    for end in body_ends(xml, &id) {
        xml.doc.remove_node(end);
    }
    xml.doc.remove_node(start);
    let start_node = marker(xml, true, &id, &actual);
    let end_node = marker(xml, false, &id, &actual);
    let matched = place(
        xml,
        paragraph,
        &address,
        anchor.select,
        anchor.occurrence,
        start_node,
        end_node,
    )?;
    let summary = format!(
        "bookmark {actual:?} (id {id}) moved to {}",
        summary_target(&address, anchor.select, anchor.occurrence, matched)
    );
    Ok((
        summary,
        json!({
            "paraId": address,
            "bookmark": actual,
            "bookmarkId": id,
            "matched": matched,
            "occurrence": anchor.occurrence,
            "wholeParagraph": anchor.select.is_none(),
        }),
    ))
}

pub(crate) fn delete(
    package: &mut Package,
    xml: &mut DocxXml,
    name: &str,
) -> Result<(String, Value), Outcome> {
    let start = find_body_start(package, xml, name)?;
    let id = xml.attr(start, "id").unwrap_or("").to_string();
    let actual = xml.attr(start, "name").unwrap_or(name).to_string();
    let para_id = enclosing_paragraph(xml, start)
        .and_then(|p| xml.para_id(p))
        .map(str::to_owned);
    for end in body_ends(xml, &id) {
        xml.doc.remove_node(end);
    }
    xml.doc.remove_node(start);
    let mut result = json!({"bookmark": actual, "bookmarkId": id});
    if let Some(para_id) = para_id {
        result["paraId"] = json!(para_id);
    }
    Ok((
        format!("bookmark {actual:?} (id {id}) deleted; its text is unchanged"),
        result,
    ))
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

struct Entry {
    name: String,
    id: String,
    part: String,
    start: Option<String>,
    end: Option<String>,
    /// Waiting for the next paragraph: the start sat between paragraphs.
    start_pending: bool,
    open: bool,
    text: String,
    chars: usize,
    truncated: bool,
}

impl Entry {
    fn push(&mut self, text: &str) {
        for c in text.chars() {
            if self.chars >= TEXT_LIMIT {
                self.truncated = true;
                return;
            }
            self.text.push(c);
            self.chars += 1;
        }
    }
}

struct Walk<'a> {
    xml: &'a DocxXml,
    part: &'a str,
    para_id: &'a dyn Fn(NodeId) -> Option<String>,
    entries: Vec<Entry>,
    open: Vec<usize>,
    last_paragraph: Option<String>,
}

impl Walk<'_> {
    fn node(&mut self, node: NodeId, paragraph: Option<&Option<String>>) {
        for child in self.xml.doc.children(node) {
            let xml = self.xml;
            if !xml.doc.is_element(child) {
                continue;
            }
            if xml.is_w(child, "p") {
                let id = (self.para_id)(child);
                let open: Vec<usize> = self.open.clone();
                for index in open {
                    let entry = &mut self.entries[index];
                    if entry.start_pending {
                        entry.start = id.clone();
                        entry.start_pending = false;
                    } else if !entry.text.is_empty() && !entry.text.ends_with('\n') {
                        entry.push("\n");
                    }
                }
                self.node(child, Some(&id));
                self.last_paragraph = id;
                continue;
            }
            if xml.is_w(child, "bookmarkStart") {
                let index = self.entries.len();
                self.entries.push(Entry {
                    name: xml.attr(child, "name").unwrap_or("").to_string(),
                    id: xml.attr(child, "id").unwrap_or("").to_string(),
                    part: self.part.to_string(),
                    start: paragraph.cloned().flatten(),
                    end: None,
                    start_pending: paragraph.is_none(),
                    open: true,
                    text: String::new(),
                    chars: 0,
                    truncated: false,
                });
                self.open.push(index);
                continue;
            }
            if xml.is_w(child, "bookmarkEnd") {
                let id = xml.attr(child, "id").unwrap_or("");
                if let Some(position) = self.open.iter().position(|&i| self.entries[i].id == id) {
                    let index = self.open.remove(position);
                    let end = match paragraph {
                        Some(p) => p.clone(),
                        None => self.last_paragraph.clone(),
                    };
                    let entry = &mut self.entries[index];
                    entry.open = false;
                    entry.end = end;
                    if entry.start_pending {
                        entry.start = entry.end.clone();
                        entry.start_pending = false;
                    }
                }
                continue;
            }
            // Deleted text (w:delText) is not current text; markers inside
            // deletions are still walked.
            if xml.is_w(child, "pPr") || xml.is_w(child, "rPr") || xml.is_w(child, "delText") {
                continue;
            }
            let text = if xml.is_w(child, "t") {
                Some(xml.doc.text_content(child))
            } else if xml.is_w(child, "tab")
                && xml.doc.parent(child).is_some_and(|p| xml.is_w(p, "r"))
            {
                Some("\t".to_string())
            } else if (xml.is_w(child, "br") || xml.is_w(child, "cr"))
                && xml.doc.parent(child).is_some_and(|p| xml.is_w(p, "r"))
            {
                Some("\n".to_string())
            } else {
                None
            };
            if let Some(text) = text {
                for &index in &self.open {
                    self.entries[index].push(&text);
                }
                continue;
            }
            if xml.is_local(child, "AlternateContent") {
                if let Some(branch) = xml.alternate_content_branch(child) {
                    self.node(branch, paragraph);
                }
                continue;
            }
            self.node(child, paragraph);
        }
    }
}

fn walk_story(xml: &DocxXml, part: &str, para_id: &dyn Fn(NodeId) -> Option<String>) -> Vec<Entry> {
    let Some(root) = xml.doc.root_element() else {
        return Vec::new();
    };
    let mut walk = Walk {
        xml,
        part,
        para_id,
        entries: Vec::new(),
        open: Vec::new(),
        last_paragraph: None,
    };
    walk.node(root, None);
    walk.entries
}

/// Every bookmark in every story: name, w:id, hidden, start/end paragraph
/// ids, the (truncated, current-view) bookmarked text, and its part.
pub(crate) fn bookmarks(package: &Package, xml: &DocxXml, source: Option<&[u8; 32]>) -> Vec<Value> {
    let main = package.document_part_name();
    let index = xml.resolve_paragraph_ids(&main, source.unwrap_or(&[0; 32]));
    let body_id = |node: NodeId| index.ids.get(&node).map(|(id, _)| id.clone());
    let mut entries = walk_story(xml, &main, &body_id);
    for part in story_parts(package) {
        if let Some(story) = package
            .get(&part)
            .and_then(|bytes| DocxXml::parse(bytes).ok())
        {
            let raw_id = |node: NodeId| story.para_id(node).map(str::to_uppercase);
            entries.extend(walk_story(&story, &part, &raw_id));
        }
    }
    entries
        .into_iter()
        .map(|entry| {
            let mut text = entry.text.trim_end_matches('\n').to_string();
            if entry.truncated {
                text.push('…');
            }
            json!({
                "name": entry.name,
                "id": entry.id,
                "hidden": entry.name.starts_with('_'),
                "start": entry.start,
                "end": entry.end,
                "text": text,
                "part": entry.part,
            })
        })
        .collect()
}

pub(crate) fn list_bookmarks(
    package: &Package,
    xml: &DocxXml,
    source: Option<&[u8; 32]>,
) -> Outcome {
    let list = bookmarks(package, xml, source);
    let summary = format!("bookmarks {}", list.len());
    Outcome::ok(summary, json!({ "bookmarks": list }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_word_rules() {
        assert!(validate_new_name("Clause_4", "name").is_ok());
        assert!(validate_new_name("Überblick", "name").is_ok());
        assert!(validate_new_name(&"a".repeat(40), "name").is_ok());
        assert!(validate_new_name(&"a".repeat(41), "name").is_err());
        assert!(validate_new_name("4Clause", "name")
            .unwrap_err()
            .contains("digit"));
        assert!(validate_new_name("my mark", "name")
            .unwrap_err()
            .contains("my_mark"));
        assert!(validate_new_name("a-b", "name").is_err());
        assert!(validate_new_name("", "name").is_err());
        assert!(validate_new_name("_Toc1", "new_name")
            .unwrap_err()
            .starts_with("new_name "));
    }

    #[test]
    fn close_matches_rank_by_distance_and_skip_hidden() {
        let known: Vec<Known> = ["Intro", "Introduction", "Outro", "_GoBack"]
            .iter()
            .map(|n| Known {
                name: (*n).into(),
                id: "0".into(),
                part: "word/document.xml".into(),
            })
            .collect();
        assert_eq!(
            close_matches(&known, "intro"),
            vec!["Intro", "Outro", "Introduction"]
        );
        assert!(close_matches(&known, "GoBack").is_empty());
        assert_eq!(close_matches(&known, "_GoBak"), vec!["_GoBack"]);
    }

    #[test]
    fn reads_cross_paragraph_point_and_deleted_text() {
        let xml = DocxXml::parse(r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"><w:body>
<w:p w14:paraId="11111111"><w:r><w:t xml:space="preserve">one </w:t></w:r><w:bookmarkStart w:id="1" w:name="Span"/><w:r><w:t>two</w:t></w:r><w:del w:id="9" w:author="a"><w:r><w:delText>gone</w:delText></w:r></w:del></w:p>
<w:bookmarkStart w:id="2" w:name="_Between"/>
<w:p w14:paraId="22222222"><w:r><w:t>three</w:t></w:r><w:bookmarkEnd w:id="1"/><w:bookmarkStart w:id="3" w:name="Point"/><w:bookmarkEnd w:id="3"/></w:p>
<w:bookmarkEnd w:id="2"/>
</w:body></w:document>"#.as_bytes()).unwrap();
        let index = xml.resolve_paragraph_ids("word/document.xml", &[0; 32]);
        let id = |node: NodeId| index.ids.get(&node).map(|(id, _)| id.clone());
        let entries = walk_story(&xml, "word/document.xml", &id);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "Span");
        assert_eq!(entries[0].text, "two\nthree");
        assert_eq!(entries[0].start.as_deref(), Some("11111111"));
        assert_eq!(entries[0].end.as_deref(), Some("22222222"));
        assert_eq!(entries[1].start.as_deref(), Some("22222222"));
        assert_eq!(entries[1].end.as_deref(), Some("22222222"));
        assert_eq!(entries[1].text, "three");
        assert_eq!(entries[2].text, "");
    }
}
