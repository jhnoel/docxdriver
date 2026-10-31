//! Typed header/footer and document-settings operations.

use serde_json::{json, Value};
use xmloxide::tree::NodeId;

use crate::api::ChromeKind;
use crate::document::DocxXml;
use crate::html::build::blocks_xml_from_html;
use crate::media::{relationship_targets, resolve_rel_target};
use crate::outcome::Outcome;
use crate::package::Package;

use super::layout::ensure_sect_pr;
use super::ExecCtx;

const CONTENT_TYPES_PART: &str = "[Content_Types].xml";
const SETTINGS_CT: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
const SETTINGS_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";
const SETTINGS_PART: &str = "word/settings.xml";
const EMPTY_SETTINGS: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
    "<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"/>",
);
const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const HEADER_CT: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const FOOTER_CT: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
const HEADER_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
const FOOTER_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";

fn parse_part(package: &Package, name: &str, default: &str) -> Result<DocxXml, Outcome> {
    let bytes = package
        .get(name)
        .map(<[u8]>::to_vec)
        .unwrap_or_else(|| default.as_bytes().to_vec());
    DocxXml::parse(&bytes).map_err(|error| Outcome::error(format!("{name}: {error}")))
}

fn ensure_content_type(
    package: &mut Package,
    part: &str,
    content_type: &str,
) -> Result<(), Outcome> {
    let mut types = parse_part(package, CONTENT_TYPES_PART, "")?;
    let root = types
        .doc
        .root_element()
        .ok_or_else(|| Outcome::error("malformed [Content_Types].xml"))?;
    let part_name = format!("/{part}");
    let exists = types.doc.children(root).any(|child| {
        types.is_local(child, "Override")
            && types.doc.attribute(child, "PartName") == Some(part_name.as_str())
    });
    if !exists {
        let node = types.doc.create_element("Override");
        types.doc.set_attribute(node, "PartName", &part_name);
        types.doc.set_attribute(node, "ContentType", content_type);
        types.doc.append_child(root, node);
        package.set(CONTENT_TYPES_PART, types.serialize());
    }
    Ok(())
}

fn ensure_document_rel(
    package: &mut Package,
    target: &str,
    rel_type: &str,
) -> Result<String, Outcome> {
    let default = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>",
    );
    let rels_part = super::document_rels_part(package);
    let mut rels = parse_part(package, &rels_part, default)?;
    let root = rels
        .doc
        .root_element()
        .ok_or_else(|| Outcome::error("malformed document.xml.rels"))?;
    // Reuse only a relationship of the same type: a header id that names a
    // footer relationship makes Word refuse the document.
    if let Some(existing) = rels.doc.children(root).find(|&child| {
        rels.is_local(child, "Relationship")
            && rels.doc.attribute(child, "Target") == Some(target)
            && rels.doc.attribute(child, "Type") == Some(rel_type)
    }) {
        return Ok(rels
            .doc
            .attribute(existing, "Id")
            .unwrap_or("rId1")
            .to_string());
    }
    let max_rid = rels
        .doc
        .children(root)
        .filter_map(|child| {
            rels.doc
                .attribute(child, "Id")
                .and_then(|id| id.strip_prefix("rId"))
                .and_then(|id| id.parse::<u64>().ok())
        })
        .max()
        .unwrap_or(0);
    let id = format!("rId{}", max_rid + 1);
    let node = rels.doc.create_element("Relationship");
    rels.doc.set_attribute(node, "Id", &id);
    rels.doc.set_attribute(node, "Type", rel_type);
    rels.doc.set_attribute(node, "Target", target);
    rels.doc.append_child(root, node);
    package.set(&rels_part, rels.serialize());
    Ok(id)
}

fn ensure_r_ns(xml: &mut DocxXml) -> Result<(), Outcome> {
    const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let Some(root) = xml.doc.root_element() else {
        return Err(Outcome::error("document has no root element"));
    };
    let has_r = xml
        .doc
        .attributes(root)
        .iter()
        .any(|a| a.name == "xmlns:r" || (a.name == "r" && a.value.contains("relationships")));
    if !has_r {
        xml.doc.set_attribute(root, "xmlns:r", R_NS);
    }
    Ok(())
}

fn max_chrome_part_n(package: &Package, prefix: &str) -> u32 {
    let mut max = 0u32;
    let mut n = 1u32;
    loop {
        let part = format!("word/{prefix}{n}.xml");
        if package.get(&part).is_none() {
            break;
        }
        max = n;
        n += 1;
    }
    max
}

fn chrome_ref_tag(scope: &str) -> &'static str {
    if scope == "hdr" {
        "headerReference"
    } else {
        "footerReference"
    }
}

fn chrome_root_tag(scope: &str) -> &'static str {
    if scope == "hdr" {
        "hdr"
    } else {
        "ftr"
    }
}

fn find_explicit_chrome_ref(
    xml: &DocxXml,
    sect: NodeId,
    scope: &str,
    kind: &str,
) -> Option<NodeId> {
    let tag = chrome_ref_tag(scope);
    xml.doc
        .children(sect)
        .find(|&child| xml.is_w(child, tag) && xml.attr(child, "type").unwrap_or("default") == kind)
}

fn resolve_chrome_part(package: &Package, _xml: &DocxXml, rid: &str) -> Option<String> {
    let rels = relationship_targets(package);
    let document_part = package.document_part_name();
    rels.get(rid)
        .map(|target| resolve_rel_target(&document_part, target))
        .filter(|part| package.get(part).is_some())
}

fn inner_xml_from_with(with: &str) -> Result<String, Outcome> {
    let inner = blocks_xml_from_html(with).map_err(Outcome::blocked)?;
    if inner.is_empty() {
        Ok("<w:p/>".into())
    } else {
        Ok(inner)
    }
}

fn chrome_part_xml(scope: &str, inner: &str) -> String {
    let root = chrome_root_tag(scope);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:{root} xmlns:w=\"{W_NS}\">{inner}</w:{root}>"
    )
}

fn ensure_title_pg(xml: &mut DocxXml, sect: NodeId) {
    let node = super::layout::ensure_sect_pr_child(xml, sect, "titlePg");
    super::layout::set_attr(xml, node, "w:val", "1");
}

#[allow(clippy::too_many_arguments)]
fn set_chrome_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    scope: &str,
    section: usize,
    kind: ChromeKind,
    with: &str,
    track: Option<(&str, &ExecCtx)>,
) -> Result<(String, Value), Outcome> {
    if section == 0 {
        return Err(Outcome::error("section must be >= 1"));
    }
    let op = if scope == "hdr" { "set_header" } else { "set_footer" };
    let kind_str = kind.as_type();
    let inner = inner_xml_from_with(with)?;
    let sect = ensure_sect_pr(xml, section)?;
    if let Some((author, ctx)) = track {
        if kind == ChromeKind::Even && !even_and_odd_headers(package) {
            return Err(Outcome::blocked(format!(
                "{op} kind=even cannot be tracked while even/odd headers are off: turning them on changes a document setting (w:evenAndOddHeaders) that Word records no revision for; run set_even_and_odd_headers in a change_mode=direct plan first"
            )));
        }
        // Turning on the first-page header is a tracked section property
        // change, as in set_first_page_mode.
        if kind == ChromeKind::First && !title_page(xml, sect) {
            super::layout::snapshot_section_properties(xml, sect, author, ctx)?;
        }
    }
    if kind == ChromeKind::First {
        ensure_title_pg(xml, sect);
    }
    if kind == ChromeKind::Even {
        set_even_and_odd_headers_typed(package, xml, true)?;
    }

    let (ct, rel_type, prefix) = if scope == "hdr" {
        (HEADER_CT, HEADER_REL, "header")
    } else {
        (FOOTER_CT, FOOTER_REL, "footer")
    };

    if let Some(ref_node) = find_explicit_chrome_ref(xml, sect, scope, kind_str) {
        let rid = xml
            .attr(ref_node, "id")
            .ok_or_else(|| Outcome::error("chrome reference is missing r:id"))?;
        let part = resolve_chrome_part(package, xml, rid).ok_or_else(|| {
            Outcome::blocked(format!(
                "section {section} {kind_str} {scope} reference does not resolve to a package part"
            ))
        })?;
        let bytes = match track {
            Some((author, ctx)) => tracked_story(xml, package, Some(&part), scope, Some(&inner), author, ctx, op)?,
            None => chrome_part_xml(scope, &inner).into_bytes(),
        };
        package.set(&part, bytes);
        return Ok((
            format!("section {section} {kind_str} {scope} updated"),
            json!({ "section": section, "kind": kind_str, "scope": scope, "part": part, "tracked": track.is_some() }),
        ));
    }

    let n = max_chrome_part_n(package, prefix) + 1;
    let part_name = format!("word/{prefix}{n}.xml");
    let target = format!("{prefix}{n}.xml");
    ensure_content_type(package, &part_name, ct)?;
    let rid = ensure_document_rel(package, &target, rel_type)?;
    ensure_r_ns(xml)?;
    // A new story's content is a tracked insertion; the story itself and
    // its reference cannot be tracked (CT_SectPrBase has no references),
    // so rejecting leaves it empty.
    let bytes = match track {
        Some((author, ctx)) => tracked_story(xml, package, None, scope, Some(&inner), author, ctx, op)?,
        None => chrome_part_xml(scope, &inner).into_bytes(),
    };
    package.set(&part_name, bytes);
    let ref_tag = chrome_ref_tag(scope);
    let ref_node = xml.doc.create_element(&format!("w:{ref_tag}"));
    xml.doc.set_attribute(ref_node, "w:type", kind_str);
    xml.doc.set_attribute(ref_node, "r:id", &rid);
    // Header and footer references lead CT_SectPr: insert after the last one.
    let successor = xml.doc.children(sect).find(|&child| {
        !xml.is_w(child, "headerReference") && !xml.is_w(child, "footerReference")
    });
    match successor {
        Some(next) => xml.doc.insert_before(next, ref_node),
        None => xml.doc.append_child(sect, ref_node),
    }

    Ok((
        format!("section {section} {kind_str} {scope} set"),
        json!({ "section": section, "kind": kind_str, "scope": scope, "part": part_name, "tracked": track.is_some() }),
    ))
}

fn title_page(xml: &DocxXml, sect: NodeId) -> bool {
    xml.child_w(sect, "titlePg")
        .is_some_and(|node| !matches!(xml.attr(node, "val"), Some("0" | "false" | "off")))
}

fn even_and_odd_headers(package: &Package) -> bool {
    let Some(settings) = package.get(SETTINGS_PART).and_then(|bytes| DocxXml::parse(bytes).ok()) else {
        return false;
    };
    let Some(root) = settings.doc.root_element() else {
        return false;
    };
    settings
        .doc
        .children(root)
        .find(|&child| settings.is_w(child, "evenAndOddHeaders"))
        .is_some_and(|node| !matches!(settings.attr(node, "val"), Some("0" | "false" | "off")))
}

/// The highest revision id in any part but the main document (the body's
/// allocator covers it) and `skip`, the story being rewritten.
fn other_parts_revision_floor(package: &Package, skip: Option<&str>) -> u64 {
    let main = package.document_part_name();
    package
        .parts()
        .filter(|(name, _)| {
            *name != main && Some(*name) != skip && name.starts_with("word/") && name.ends_with(".xml")
        })
        .filter_map(|(_, bytes)| DocxXml::parse(bytes).ok()?.max_revision_id())
        .max()
        .unwrap_or(0)
}

/// A header or footer story rewritten as tracked changes: its old content
/// becomes a tracked deletion and `inner` (none to clear) a tracked
/// insertion, laid out so that accept-all leaves exactly `inner` and
/// reject-all exactly the old story:
///
/// ```text
/// O1..Om-1  old paragraphs, content and mark deleted
/// N1..Nn-1  new paragraphs, content and mark inserted
/// X         Om's mark: Om's content deleted, then Nn's inserted;
///           Nn's paragraph properties, with Om's in a pPrChange
/// ```
///
/// Old or new content other than paragraphs (a table, a content control)
/// is refused. Revision ids continue the body's allocator past every id in
/// the story and in the package's other parts, so they stay unique across
/// the package.
fn tracked_story(
    body: &DocxXml,
    package: &Package,
    part: Option<&str>,
    scope: &str,
    inner: Option<&str>,
    author: &str,
    ctx: &ExecCtx,
    op: &str,
) -> Result<Vec<u8>, Outcome> {
    let empty = chrome_part_xml(scope, "");
    let old = part.and_then(|name| package.get(name));
    let mut story = DocxXml::parse(old.unwrap_or(empty.as_bytes()))
        .map_err(|error| Outcome::error(format!("{op}: {error}")))?;
    let root = story.doc.root_element().ok_or_else(|| Outcome::error(format!("{op}: empty story part")))?;
    let refuse = |what: &str| {
        Outcome::blocked(format!(
            "{op} cannot be tracked here: the {what} holds a table or content control, and only paragraphs are tracked in a header or footer; use a change_mode=direct plan"
        ))
    };
    let blocks = |xml: &DocxXml, root: NodeId| -> Option<Vec<NodeId>> {
        let mut out = Vec::new();
        for child in xml.doc.children(root).filter(|&c| xml.doc.is_element(c)) {
            if xml.is_w(child, "p") {
                out.push(child);
            } else if !crate::document::BODY_MILESTONES.iter().any(|m| xml.is_w(child, m)) {
                return None;
            }
        }
        Some(out)
    };
    let old_paragraphs = blocks(&story, root).ok_or_else(|| refuse("current story"))?;
    let source = match inner {
        Some(inner) => Some(
            DocxXml::parse(chrome_part_xml(scope, inner).as_bytes())
                .map_err(|error| Outcome::error(format!("{op}: {error}")))?,
        ),
        None => None,
    };
    let new_paragraphs = match &source {
        Some(source) => {
            let source_root = source.doc.root_element().ok_or_else(|| Outcome::error(format!("{op}: empty content")))?;
            blocks(source, source_root).ok_or_else(|| refuse("new content"))?
        }
        None => Vec::new(),
    };
    let checkpoint = story.revision_checkpoint();
    let floor = story.max_revision_id().unwrap_or(0).max(other_parts_revision_floor(package, part));
    let next_id = || loop {
        let id = body.next_revision_id();
        if id > floor {
            break id;
        }
    };
    let date = ctx.now.as_str();

    for &paragraph in &old_paragraphs {
        expand_simple_fields(&mut story, paragraph);
        if !trackable_content(&story, paragraph) {
            return Err(Outcome::blocked(format!(
                "{op} cannot be tracked here: a paragraph of the current story holds content (a drawing, object or content control outside a run) that a tracked deletion cannot hold; use a change_mode=direct plan"
            )));
        }
    }
    // Old content: deleted; every old mark but the last joins on accept.
    for (index, &paragraph) in old_paragraphs.iter().enumerate() {
        super::typed::wrap_live_content_in_del(&mut story, paragraph, next_id(), author, date)
            .map_err(|message| Outcome::blocked(format!("{op}: {message}")))?;
        if index + 1 < old_paragraphs.len() && story.paragraph_mark_del(paragraph).is_none() {
            story.add_paragraph_mark_marker(paragraph, "del", next_id(), author, date);
        }
    }
    let Some(source) = &source else {
        story.renumber_allocated_revision_ids(checkpoint);
        return Ok(story.serialize());
    };
    let last_old = old_paragraphs.last().copied();
    for (index, &paragraph) in new_paragraphs.iter().enumerate() {
        let imported = crate::segments::import_element(&mut story, source, paragraph);
        expand_simple_fields(&mut story, imported);
        let last_new = index + 1 == new_paragraphs.len();
        wrap_content_in_ins(&mut story, imported, next_id(), author, date);
        match (last_new, last_old) {
            (true, Some(x)) => {
                // Nn's content moves after Om's deleted content, in Om.
                for child in story.doc.children(imported).collect::<Vec<_>>() {
                    if story.is_w(child, "pPr") {
                        continue;
                    }
                    story.doc.detach(child);
                    story.doc.append_child(x, child);
                }
                super::layout::adopt_paragraph_properties(&mut story, x, imported, next_id(), author, date);
            }
            (true, None) => story.doc.append_child(root, imported),
            (false, _) => {
                story.add_paragraph_mark_marker(imported, "ins", next_id(), author, date);
                match last_old {
                    Some(x) => story.doc.insert_before(x, imported),
                    None => story.doc.append_child(root, imported),
                }
            }
        }
    }
    // One id per revision element, as Word writes them.
    story.renumber_allocated_revision_ids(checkpoint);
    Ok(story.serialize())
}

/// Wrap a new paragraph's content in one tracked insertion: runs directly,
/// a hyperlink's or field's runs inside it (a tracked insertion holds runs).
pub(crate) fn wrap_content_in_ins(xml: &mut DocxXml, paragraph: NodeId, id: u64, author: &str, date: &str) {
    for child in xml.doc.children(paragraph).collect::<Vec<_>>() {
        if !xml.doc.is_element(child) || xml.is_w(child, "pPr") {
            continue;
        }
        if xml.is_w(child, "hyperlink") || xml.is_w(child, "smartTag") {
            wrap_content_in_ins(xml, child, id, author, date);
            continue;
        }
        if !xml.is_w(child, "r") {
            continue;
        }
        let ins = xml.doc.create_element("w:ins");
        xml.doc.set_attribute(ins, "w:id", &id.to_string());
        xml.doc.set_attribute(ins, "w:author", author);
        xml.doc.set_attribute(ins, "w:date", date);
        xml.doc.insert_before(child, ins);
        xml.doc.detach(child);
        xml.doc.append_child(ins, child);
    }
}

/// Rewrite each simple field (`w:fldSimple`) of `paragraph` as the
/// equivalent complex field runs: a tracked change holds runs, so Word
/// tracks a field as begin / instruction / separate / result / end runs.
fn expand_simple_fields(xml: &mut DocxXml, paragraph: NodeId) {
    let fields: Vec<NodeId> = xml.doc.children(paragraph).filter(|&c| xml.is_w(c, "fldSimple")).collect();
    for field in fields {
        let instruction = xml.attr(field, "instr").unwrap_or_default().to_string();
        let dirty = xml.attr(field, "dirty").map(str::to_string);
        let format = xml
            .doc
            .children(field)
            .find(|&c| xml.is_w(c, "r"))
            .and_then(|run| xml.child_w(run, "rPr"));
        let run = |xml: &mut DocxXml, child: NodeId| {
            let run = xml.doc.create_element("w:r");
            if let Some(format) = format {
                let copy = xml.doc.clone_node(format, true);
                xml.doc.append_child(run, copy);
            }
            xml.doc.append_child(run, child);
            xml.doc.insert_before(field, run);
        };
        let char_run = |xml: &mut DocxXml, kind: &str| {
            let node = xml.doc.create_element("w:fldChar");
            xml.doc.set_attribute(node, "w:fldCharType", kind);
            run(xml, node);
        };
        let begin = xml.doc.create_element("w:fldChar");
        xml.doc.set_attribute(begin, "w:fldCharType", "begin");
        if let Some(dirty) = &dirty {
            xml.doc.set_attribute(begin, "w:dirty", dirty);
        }
        run(xml, begin);
        let text = xml.doc.create_element("w:instrText");
        xml.doc.set_attribute(text, "xml:space", "preserve");
        let value = xml.doc.create_text(&format!(" {} ", instruction.trim()));
        xml.doc.append_child(text, value);
        run(xml, text);
        char_run(xml, "separate");
        for child in xml.doc.children(field).collect::<Vec<_>>() {
            xml.doc.detach(child);
            xml.doc.insert_before(field, child);
        }
        char_run(xml, "end");
        xml.doc.remove_node(field);
    }
}

/// Whether every child of `paragraph` is content a tracked change can
/// wrap (runs and their wrappers, revisions, range markers).
fn trackable_content(xml: &DocxXml, paragraph: NodeId) -> bool {
    const ALLOWED: &[&str] = &["pPr", "r", "hyperlink", "smartTag", "dir", "bdo", "ins", "del", "moveFrom", "moveTo"];
    xml.doc.children(paragraph).filter(|&c| xml.doc.is_element(c)).all(|child| {
        ALLOWED.iter().any(|name| xml.is_w(child, name))
            || crate::document::BODY_MILESTONES.iter().any(|name| xml.is_w(child, name))
    })
}

fn clear_chrome_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    scope: &str,
    section: usize,
    kind: ChromeKind,
    track: Option<(&str, &ExecCtx)>,
) -> Result<(String, Value), Outcome> {
    if section == 0 {
        return Err(Outcome::error("section must be >= 1"));
    }
    let op = if scope == "hdr" { "clear_header" } else { "clear_footer" };
    let kind_str = kind.as_type();
    let sect = ensure_sect_pr(xml, section)?;
    let Some(ref_node) = find_explicit_chrome_ref(xml, sect, scope, kind_str) else {
        return Err(Outcome::blocked(format!(
            "section {section} has no explicit {kind_str} {scope} to clear"
        )));
    };
    if let Some((author, ctx)) = track {
        // Word tracks the story's content, never its reference: tracked,
        // the content is deleted and the (then empty) story stays. In the
        // first section that is what removing it shows; a later section
        // without its own story would inherit the previous one instead.
        if section != 1 {
            return Err(Outcome::blocked(format!(
                "{op} cannot be tracked in section {section}: removing a later section's {kind_str} {scope} makes it inherit the previous section's, and Word cannot track a header or footer reference; use a change_mode=direct plan"
            )));
        }
        let rid = xml.attr(ref_node, "id").ok_or_else(|| Outcome::error("chrome reference is missing r:id"))?;
        let part = resolve_chrome_part(package, xml, rid).ok_or_else(|| {
            Outcome::blocked(format!("section {section} {kind_str} {scope} reference does not resolve to a package part"))
        })?;
        let bytes = tracked_story(xml, package, Some(&part), scope, None, author, ctx, op)?;
        package.set(&part, bytes);
        return Ok((
            format!("section {section} {kind_str} {scope} content marked for tracked deletion"),
            json!({ "section": section, "kind": kind_str, "scope": scope, "part": part, "tracked": true }),
        ));
    }
    xml.doc.detach(ref_node);
    Ok((
        format!("section {section} {kind_str} {scope} cleared"),
        json!({ "section": section, "kind": kind_str, "scope": scope }),
    ))
}

/// `track` is the author and context of a tracked edit, `None` for direct.
pub fn set_header_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    section: usize,
    kind: ChromeKind,
    with: &str,
    track: Option<(&str, &ExecCtx)>,
) -> Result<(String, Value), Outcome> {
    set_chrome_typed(package, xml, "hdr", section, kind, with, track)
}

pub fn set_footer_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    section: usize,
    kind: ChromeKind,
    with: &str,
    track: Option<(&str, &ExecCtx)>,
) -> Result<(String, Value), Outcome> {
    set_chrome_typed(package, xml, "ftr", section, kind, with, track)
}

pub fn clear_header_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    section: usize,
    kind: ChromeKind,
    track: Option<(&str, &ExecCtx)>,
) -> Result<(String, Value), Outcome> {
    clear_chrome_typed(package, xml, "hdr", section, kind, track)
}

pub fn clear_footer_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    section: usize,
    kind: ChromeKind,
    track: Option<(&str, &ExecCtx)>,
) -> Result<(String, Value), Outcome> {
    clear_chrome_typed(package, xml, "ftr", section, kind, track)
}

pub fn set_even_and_odd_headers_typed(
    package: &mut Package,
    _xml: &mut DocxXml,
    even_odd: bool,
) -> Result<(String, Value), Outcome> {
    if package.get(SETTINGS_PART).is_none() {
        package.set(SETTINGS_PART, EMPTY_SETTINGS.as_bytes().to_vec());
    }
    ensure_content_type(package, SETTINGS_PART, SETTINGS_CT)?;
    ensure_document_rel(package, "settings.xml", SETTINGS_REL)?;

    let mut settings = parse_part(package, SETTINGS_PART, EMPTY_SETTINGS)?;
    let root = settings
        .doc
        .root_element()
        .ok_or_else(|| Outcome::error("malformed settings.xml"))?;
    let existing = settings
        .doc
        .children(root)
        .find(|&child| settings.is_w(child, "evenAndOddHeaders"));
    match (even_odd, existing) {
        (true, None) => {
            let node = settings.doc.create_element("w:evenAndOddHeaders");
            // CT_Settings is a sequence: insert before the first later element.
            const SUCCESSORS: &[&str] = &[
                "bookFoldRevPrinting", "bookFoldPrinting", "bookFoldPrintingSheets",
                "drawingGridHorizontalSpacing", "drawingGridVerticalSpacing",
                "displayHorizontalDrawingGridEvery", "displayVerticalDrawingGridEvery",
                "doNotUseMarginsForDrawingGridOrigin", "drawingGridHorizontalOrigin",
                "drawingGridVerticalOrigin", "doNotShadeFormData", "noPunctuationKerning",
                "characterSpacingControl", "printTwoOnOne", "strictFirstAndLastChars",
                "noLineBreaksAfter", "noLineBreaksBefore", "savePreviewPicture",
                "doNotValidateAgainstSchema", "saveInvalidXml", "ignoreMixedContent",
                "alwaysShowPlaceholderText", "doNotDemarcateInvalidXml", "saveXmlDataOnly",
                "useXSLTWhenSaving", "saveThroughXslt", "showXMLTags",
                "alwaysMergeEmptyNamespace", "updateFields", "hdrShapeDefaults", "footnotePr",
                "endnotePr", "compat", "docVars", "rsids", "mathPr", "attachedSchema",
                "themeFontLang", "clrSchemeMapping", "doNotIncludeSubdocsInStats",
                "doNotAutoCompressPictures", "forceUpgrade", "captions", "readModeInkLockDown",
                "smartTagType", "schemaLibrary", "shapeDefaults", "doNotEmbedSmartTags",
                "decimalSymbol", "listSeparator",
            ];
            let successor = settings.doc.children(root).find(|&child| {
                settings
                    .local_name(child)
                    .is_some_and(|local| SUCCESSORS.contains(&local))
            });
            match successor {
                Some(next) => settings.doc.insert_before(next, node),
                None => settings.doc.append_child(root, node),
            }
        }
        (true, Some(node)) => {
            if settings
                .attr(node, "val")
                .is_some_and(|value| matches!(value, "false" | "0" | "off"))
            {
                settings.doc.remove_attribute(node, "w:val");
                settings.doc.remove_attribute(node, "val");
            }
        }
        (false, Some(node)) => settings.doc.detach(node),
        (false, None) => {}
    }
    package.set(SETTINGS_PART, settings.serialize());
    Ok((
        format!(
            "evenAndOddHeaders={}",
            if even_odd { "true" } else { "false" }
        ),
        json!({ "evenAndOdd": even_odd }),
    ))
}
