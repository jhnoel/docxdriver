//! Tracked-changes: listing and accept/reject resolution.
//! Offsets live in the ALL view (deleted text still occupies positions).
//! Content mutations use typed edit operations; this module settles revisions.

use serde_json::{json, Value};
use xmloxide::tree::NodeId;

use crate::document::DocxXml;
use crate::outcome::Outcome;
use crate::package::Package;
use crate::segments::{all_text, build_segments, rename_for};

use super::readonly::plural;

struct RevisionEntry {
    id: String,
    kind: &'static str,
    author: String,
    date: String,
    paragraph_index: usize,
    locator: String,
    text: String,
    /// Run-level wrapper node, or the paragraph-mark marker element.
    node: NodeId,
    is_paragraph_mark: bool,
    paragraph: NodeId,
    /// Package part when the revision lives outside document.xml (header/footer).
    part: Option<String>,
}

/// Property-change records: a snapshot of the pre-change properties, kept as a
/// child of the live property container. Accept keeps the new properties and
/// drops the record; reject restores the snapshot. (`numberingChange` is the
/// legacy odd one out with no snapshot child — stripped either way, matching
/// LibreOffice's "not implemented".)
const PROP_CHANGES: &[(&str, Option<&str>)] = &[
    ("rPrChange", Some("rPr")),
    ("pPrChange", Some("pPr")),
    ("sectPrChange", Some("sectPr")),
    ("tblPrChange", Some("tblPr")),
    ("trPrChange", Some("trPr")),
    ("tcPrChange", Some("tcPr")),
    ("tblGridChange", Some("tblGrid")),
    ("numberingChange", None),
];

const MOVE_RANGE_MARKERS: &[&str] = &[
    "moveFromRangeStart",
    "moveFromRangeEnd",
    "moveToRangeStart",
    "moveToRangeEnd",
];

/// The engine-owned static name for a revision element's kind.
fn kind_of(xml: &DocxXml, node: NodeId) -> Option<&'static str> {
    let local = xml.local_name(node)?;
    ["ins", "del", "moveFrom", "moveTo"]
        .into_iter()
        .chain(PROP_CHANGES.iter().map(|(name, _)| *name))
        .find(|known| *known == local)
}

fn ins_like(kind: &str) -> bool {
    matches!(kind, "ins" | "moveTo")
}

/// Whether `node` sits inside `ancestor` (inclusive).
fn within(xml: &DocxXml, node: NodeId, ancestor: NodeId) -> bool {
    let mut cursor = Some(node);
    while let Some(current) = cursor {
        if current == ancestor {
            return true;
        }
        cursor = xml.doc.parent(current);
    }
    false
}

/// Whether a revision element is a paragraph-mark/property marker (inside
/// pPr) rather than a content wrapper.
fn inside_ppr(xml: &DocxXml, node: NodeId, paragraph: NodeId) -> bool {
    let mut cursor = xml.doc.parent(node);
    while let Some(current) = cursor {
        if current == paragraph {
            return false;
        }
        if xml.is_w(current, "pPr") {
            return true;
        }
        cursor = xml.doc.parent(current);
    }
    false
}

fn collect_revisions(xml: &DocxXml) -> Vec<RevisionEntry> {
    let Some(body) = xml.body() else {
        return Vec::new();
    };
    collect_revisions_in(xml, body, None)
}

fn collect_revisions_in(
    xml: &DocxXml,
    container: NodeId,
    part: Option<&str>,
) -> Vec<RevisionEntry> {
    let mut entries = Vec::new();
    for paragraph in xml.paragraphs_in(container) {
        let segments = build_segments(xml, paragraph.node);
        // Content wrappers by tree walk (NOT via segment wrappers, which see
        // only the innermost): a del nested inside a foreign ins is its own
        // revision and both must list and resolve.
        let wrappers: Vec<NodeId> = xml
            .doc
            .descendants(paragraph.node)
            .filter(|&n| {
                xml.doc.is_element(n)
                    && (xml.is_rev_ins(n) || xml.is_rev_del(n))
                    && !inside_ppr(xml, n, paragraph.node)
            })
            .collect();
        for wrapper in wrappers {
            let group: Vec<_> = segments
                .iter()
                .filter(|s| within(xml, s.text_element, wrapper))
                .collect();
            let start = group.iter().map(|s| s.start).min().unwrap_or(0);
            let end = group.iter().map(|s| s.end).max().unwrap_or(start);
            let text: String = group.iter().map(|s| s.text.as_str()).collect();
            let fallback = if xml.is_rev_ins(wrapper) {
                "ins"
            } else {
                "del"
            };
            entries.push(RevisionEntry {
                id: xml.attr(wrapper, "id").unwrap_or("").to_string(),
                kind: kind_of(xml, wrapper).unwrap_or(fallback),
                author: xml.attr(wrapper, "author").unwrap_or("").to_string(),
                date: xml.attr(wrapper, "date").unwrap_or("").to_string(),
                paragraph_index: paragraph.index,
                locator: format!("p{}:{start}-{end}", paragraph.index),
                text,
                node: wrapper,
                is_paragraph_mark: false,
                paragraph: paragraph.node,
                part: part.map(str::to_string),
            });
        }
        let marks = [
            xml.paragraph_mark_ins(paragraph.node),
            xml.paragraph_mark_del(paragraph.node),
        ];
        for marker in marks {
            let Some(marker) = marker else { continue };
            let kind = kind_of(xml, marker).unwrap_or("ins");
            let len = all_text(&segments).chars().count();
            entries.push(RevisionEntry {
                id: xml.attr(marker, "id").unwrap_or("").to_string(),
                kind,
                author: xml.attr(marker, "author").unwrap_or("").to_string(),
                date: xml.attr(marker, "date").unwrap_or("").to_string(),
                paragraph_index: paragraph.index,
                locator: format!("p{}:{len}-{len}", paragraph.index),
                text: String::new(),
                node: marker,
                is_paragraph_mark: true,
                paragraph: paragraph.node,
                part: part.map(str::to_string),
            });
        }
        // Formatting revisions inside the paragraph (rPrChange on runs or the
        // paragraph mark, pPrChange on the paragraph): zero-width entries so
        // they list and resolve by id. Table/section-level records are not
        // paragraph-anchored; accept/rejectAll sweeps them separately.
        let prop_changes: Vec<NodeId> = xml
            .doc
            .descendants(paragraph.node)
            .filter(|&n| {
                xml.doc.is_element(n)
                    && kind_of(xml, n)
                        .is_some_and(|k| !matches!(k, "ins" | "del" | "moveFrom" | "moveTo"))
            })
            .collect();
        for node in prop_changes {
            entries.push(RevisionEntry {
                id: xml.attr(node, "id").unwrap_or("").to_string(),
                kind: kind_of(xml, node).expect("filtered"),
                author: xml.attr(node, "author").unwrap_or("").to_string(),
                date: xml.attr(node, "date").unwrap_or("").to_string(),
                paragraph_index: paragraph.index,
                locator: format!("p{}:0-0", paragraph.index),
                text: String::new(),
                node,
                is_paragraph_mark: false,
                paragraph: paragraph.node,
                part: part.map(str::to_string),
            });
        }
    }
    entries
}

/// Document body revisions plus every header/footer part the document references.
fn collect_package_revisions(
    package: &Package,
    xml: &DocxXml,
) -> Vec<(Option<String>, RevisionEntry)> {
    let mut out: Vec<(Option<String>, RevisionEntry)> = collect_revisions(xml)
        .into_iter()
        .map(|e| (None, e))
        .collect();
    for part_name in crate::html::chrome::chrome_part_names(package, xml) {
        let Some(bytes) = package.get(&part_name) else {
            continue;
        };
        let Ok(part_xml) = DocxXml::parse(bytes) else {
            continue;
        };
        let Some(root) = part_xml.doc.root_element() else {
            continue;
        };
        for entry in collect_revisions_in(&part_xml, root, Some(&part_name)) {
            out.push((Some(part_name.clone()), entry));
        }
    }
    out
}

/// Resolve one property-change record: accept keeps the current (new)
/// properties and drops the snapshot; reject swaps the snapshot back into the
/// live property container.
fn resolve_property_change(xml: &mut DocxXml, node: NodeId, accept: bool) {
    let inner_name = kind_of(xml, node)
        .and_then(|kind| PROP_CHANGES.iter().find(|(name, _)| *name == kind))
        .and_then(|(_, inner)| *inner);
    if !accept {
        if let (Some(inner_name), Some(parent)) = (inner_name, xml.doc.parent(node)) {
            if let Some(snapshot) = xml.child_w(node, inner_name) {
                // Replace the live container's content with the snapshot's.
                let old: Vec<NodeId> = xml.doc.children(snapshot).collect();
                let current: Vec<NodeId> = xml.doc.children(parent).collect();
                for child in old {
                    xml.doc.detach(child);
                    xml.doc.append_child(parent, child);
                }
                for child in current {
                    xml.doc.remove_node(child);
                }
                return;
            }
        }
    }
    xml.doc.remove_node(node);
}

/// Remove move-range milestone pairs; `id` limits the sweep to one revision
/// (per-id resolution of a move can leave the partner's markers behind — an
/// accept/rejectAll cleans up).
fn strip_move_range_markers(xml: &mut DocxXml, id: Option<&str>) {
    let Some(body) = xml.body() else { return };
    strip_move_range_markers_in(xml, body, id);
}

fn strip_move_range_markers_in(xml: &mut DocxXml, container: NodeId, id: Option<&str>) {
    let markers: Vec<NodeId> = xml
        .doc
        .descendants(container)
        .filter(|&n| {
            xml.doc.is_element(n)
                && MOVE_RANGE_MARKERS.iter().any(|name| xml.is_w(n, name))
                && id.is_none_or(|id| xml.attr(n, "id") == Some(id))
        })
        .collect();
    for marker in markers {
        xml.doc.remove_node(marker);
    }
}

pub fn list_revisions(package: &Package, xml: &DocxXml) -> Outcome {
    let revisions: Vec<Value> = collect_package_revisions(package, xml)
        .into_iter()
        .map(|(part, entry)| {
            let mut obj = serde_json::Map::new();
            obj.insert("id".into(), json!(entry.id));
            obj.insert("kind".into(), json!(entry.kind));
            obj.insert("author".into(), json!(entry.author));
            obj.insert("date".into(), json!(entry.date));
            obj.insert("paragraphIndex".into(), json!(entry.paragraph_index));
            obj.insert("locator".into(), json!(entry.locator));
            obj.insert("text".into(), json!(entry.text));
            if let Some(part) = part.or(entry.part) {
                obj.insert("part".into(), json!(part));
            }
            Value::Object(obj)
        })
        .collect();
    Outcome::ok(
        format!("revisions {}", revisions.len()),
        json!({ "revisions": revisions }),
    )
}

/// Resolve one revision entry in place.
fn resolve_entry(xml: &mut DocxXml, entry: &RevisionEntry, accept: bool) -> Result<(), Outcome> {
    if !matches!(entry.kind, "ins" | "del" | "moveFrom" | "moveTo") {
        resolve_property_change(xml, entry.node, accept);
        return Ok(());
    }
    if entry.is_paragraph_mark {
        // The break goes away — the paragraphs join — on a rejected split
        // (mark ins) or an accepted merge (mark del); otherwise only the
        // marker is dropped and the break stands.
        let join = accept != ins_like(entry.kind);
        xml.doc.remove_node(entry.node);
        if join {
            // Pull the next paragraph's run content into this one and drop it.
            let next = {
                let mut sibling = xml.doc.next_sibling(entry.paragraph);
                while let Some(node) = sibling {
                    if xml.is_w(node, "p") {
                        break;
                    }
                    sibling = xml.doc.next_sibling(node);
                }
                sibling
            };
            if let Some(next) = next {
                let children: Vec<NodeId> = xml.doc.children(next).collect();
                for child in children {
                    if xml.is_w(child, "pPr") {
                        continue;
                    }
                    xml.doc.detach(child);
                    xml.doc.append_child(entry.paragraph, child);
                }
                xml.doc.remove_node(next);
            }
        }
        return Ok(());
    }

    let is_ins = ins_like(entry.kind);
    match (is_ins, accept) {
        // Accepted insert / rejected delete: the content stays — unwrap it.
        (true, true) | (false, false) => {
            let children: Vec<NodeId> = xml.doc.children(entry.node).collect();
            for child in children {
                xml.doc.detach(child);
                xml.doc.insert_before(entry.node, child);
                if !accept {
                    // Rejected delete: its w:delText leaves become live w:t again.
                    let leaves: Vec<NodeId> = xml
                        .doc
                        .descendants(child)
                        .filter(|&n| xml.is_w(n, "delText"))
                        .collect();
                    for leaf in leaves {
                        let name = rename_for(xml, leaf, false);
                        xml.doc.rename_element(leaf, name);
                    }
                }
            }
            xml.doc.remove_node(entry.node);
        }
        // Rejected insert / accepted delete: the content goes.
        (true, false) | (false, true) => {
            xml.doc.remove_node(entry.node);
        }
    }
    Ok(())
}

pub fn accept_reject_one_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    revision_id: &str,
    accept: bool,
) -> Result<(String, Value), Outcome> {
    // Prefer the body; fall through to chrome parts so header/footer edits resolve.
    if let Some(entry) = collect_revisions(xml)
        .into_iter()
        .find(|e| e.id == revision_id)
    {
        let is_move = matches!(entry.kind, "moveFrom" | "moveTo");
        resolve_entry(xml, &entry, accept)?;
        if is_move {
            strip_move_range_markers(xml, Some(revision_id));
        }
        let verb = if accept { "accepted" } else { "rejected" };
        return Ok((
            format!("{verb} revision {revision_id}"),
            json!({ "revisionId": revision_id }),
        ));
    }
    for part_name in crate::html::chrome::chrome_part_names(package, xml) {
        let Some(bytes) = package.get(&part_name) else {
            continue;
        };
        let Ok(mut part_xml) = DocxXml::parse(bytes) else {
            continue;
        };
        let Some(root) = part_xml.doc.root_element() else {
            continue;
        };
        let Some(entry) = collect_revisions_in(&part_xml, root, Some(&part_name))
            .into_iter()
            .find(|e| e.id == revision_id)
        else {
            continue;
        };
        let is_move = matches!(entry.kind, "moveFrom" | "moveTo");
        resolve_entry(&mut part_xml, &entry, accept)?;
        if is_move {
            strip_move_range_markers_in(&mut part_xml, root, Some(revision_id));
        }
        package.set(&part_name, part_xml.serialize());
        let verb = if accept { "accepted" } else { "rejected" };
        return Ok((
            format!("{verb} revision {revision_id}"),
            json!({ "revisionId": revision_id, "part": part_name }),
        ));
    }
    Err(Outcome::error(format!("revision not found: {revision_id}")))
}

pub fn accept_reject_all(
    package: &mut Package,
    xml: &mut DocxXml,
    accept: bool,
) -> Result<(String, Value), Outcome> {
    let mut count = accept_reject_all_in_xml(xml, accept)?;
    for part_name in crate::html::chrome::chrome_part_names(package, xml) {
        let Some(bytes) = package.get(&part_name) else {
            continue;
        };
        let Ok(mut part_xml) = DocxXml::parse(bytes) else {
            continue;
        };
        let n = accept_reject_all_in_xml(&mut part_xml, accept)?;
        if n > 0 {
            package.set(&part_name, part_xml.serialize());
            count += n;
        }
    }
    let verb = if accept { "accepted" } else { "rejected" };
    Ok((
        format!("{verb} {}", plural(count, "revision", "revisions")),
        json!({ "revisions": count }),
    ))
}

/// Resolve every pending revision in one part (body or chrome).
fn accept_reject_all_in_xml(xml: &mut DocxXml, accept: bool) -> Result<usize, Outcome> {
    // Run-level wrappers first, paragraph-mark markers last: resolving a mark
    // can merge paragraphs, which would invalidate sibling walks mid-iteration.
    // Marks run in reverse document order so a merge that consumes the next
    // paragraph never invalidates a mark entry still waiting on it.
    // Every resolution re-collects: revisions nest (a del inside a foreign
    // ins) and resolving an outer wrapper relocates or removes inner ones, so
    // pre-collected nodes go stale. Content wrappers first; paragraph marks
    // after, last-first (a merge consumes the following paragraph).
    let mut count = 0usize;
    let is_content = |e: &RevisionEntry| {
        !e.is_paragraph_mark && matches!(e.kind, "ins" | "del" | "moveFrom" | "moveTo")
    };
    let scope = |xml: &DocxXml| xml.body().or_else(|| xml.doc.root_element());
    loop {
        let Some(root) = scope(xml) else { break };
        let Some(entry) = collect_revisions_in(xml, root, None)
            .into_iter()
            .find(is_content)
        else {
            break;
        };
        resolve_entry(xml, &entry, accept)?;
        count += 1;
    }
    loop {
        let Some(root) = scope(xml) else { break };
        let Some(entry) = collect_revisions_in(xml, root, None)
            .into_iter()
            .rfind(|e| e.is_paragraph_mark)
        else {
            break;
        };
        resolve_entry(xml, &entry, accept)?;
        count += 1;
    }
    // One record at a time, re-scanning between resolutions: rejecting a
    // record swaps subtrees around, which can relocate or remove records
    // collected earlier (e.g. an rPrChange nested in a pPrChange snapshot).
    loop {
        let record = scope(xml).and_then(|root| {
            xml.doc.descendants(root).find(|&n| {
                xml.doc.is_element(n)
                    && kind_of(xml, n)
                        .is_some_and(|k| !matches!(k, "ins" | "del" | "moveFrom" | "moveTo"))
            })
        });
        let Some(node) = record else { break };
        resolve_property_change(xml, node, accept);
        count += 1;
    }
    if let Some(root) = scope(xml) {
        strip_move_range_markers_in(xml, root, None);
    }
    Ok(count)
}

/*    split_boundary(xml, paragraph, offset);
    let segments = build_segments(xml, paragraph);

    // Paragraph-child-level anchors for everything at or past the split point.
    let mut anchors: Vec<NodeId> = Vec::new();
    for segment in segments.iter().filter(|s| s.start >= offset) {
        let anchor = segment.wrapper.unwrap_or(segment.run);
        // Walk up to the paragraph-child level.
        let mut node = anchor;
        while let Some(parent) = xml.doc.parent(node) {
            if xml.is_w(parent, "p") {
                break;
            }
            node = parent;
        }
        if !anchors.contains(&node) {
            anchors.push(node);
        }
    }

    let new_paragraph = xml.doc.create_element("w:p");
    if let Some(ppr) = xml.child_w(paragraph, "pPr") {
        // The second half inherits the original paragraph's terminating break —
        // including any mark revision on it (the clone keeps the marker); the
        // first half gets a fresh break, so its own marker is removed.
        let clone = xml.doc.clone_node(ppr, true);
        xml.doc.append_child(new_paragraph, clone);
        if let Some(rpr) = xml.child_w(ppr, "rPr") {
            for kind in ["ins", "del", "moveTo", "moveFrom"] {
                if let Some(marker) = xml.child_w(rpr, kind) {
                    xml.doc.remove_node(marker);
                }
            }
        }
    }
    xml.doc.insert_after(paragraph, new_paragraph);
    for anchor in anchors {
        xml.doc.detach(anchor);
        xml.doc.append_child(new_paragraph, anchor);
    }
    new_paragraph
}*/
