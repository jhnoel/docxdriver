//! OOXML comments: word/comments.xml holds the comment content,
//! word/commentsExtended.xml carries resolution state (w15:done) and reply
//! threading (w15:paraIdParent), word/commentsIds.xml carries durable ids,
//! word/commentsExtensible.xml carries the Word 365 durable-id/dateUtc row,
//! and document.xml anchors threads with commentRangeStart/End + a
//! commentReference run. Every part is registered in [Content_Types].xml and
//! document.xml.rels. Word for Mac / Microsoft 365 recovers the package as
//! corrupt if commentsIds exists without commentsExtensible, if a durableId
//! has its high bit set, or if the extended parts use the long
//! `vnd.openxmlformats-officedocument` content types instead of the
//! `vnd.ms-word` types Word itself writes.

use std::collections::HashSet;

use serde_json::{json, Value};
use xmloxide::tree::NodeId;

use crate::document::DocxXml;
use crate::outcome::Outcome;
use crate::package::Package;
use crate::segments::build_segments;

use super::ExecCtx;

const COMMENTS_PART: &str = "word/comments.xml";
const EXTENDED_PART: &str = "word/commentsExtended.xml";
const IDS_PART: &str = "word/commentsIds.xml";
const EXTENSIBLE_PART: &str = "word/commentsExtensible.xml";
const CONTENT_TYPES_PART: &str = "[Content_Types].xml";
const DOCUMENT_RELS_PART: &str = "word/_rels/document.xml.rels";

const W15_NS: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
const W14_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
const W16CID_NS: &str = "http://schemas.microsoft.com/office/word/2016/wordml/cid";
const W16CEX_NS: &str = "http://schemas.microsoft.com/office/word/2018/wordml/cex";

const EMPTY_COMMENTS: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
    "<w:comments xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
    "xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\"></w:comments>",
);
const EMPTY_EXTENDED: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
    "<w15:commentsEx xmlns:w15=\"http://schemas.microsoft.com/office/word/2012/wordml\"></w15:commentsEx>",
);
const EMPTY_IDS: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
    "<w16cid:commentsIds xmlns:w16cid=\"http://schemas.microsoft.com/office/word/2016/wordml/cid\"></w16cid:commentsIds>",
);
const EMPTY_EXTENSIBLE: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
    "<w16cex:commentsExtensible xmlns:w16cex=\"http://schemas.microsoft.com/office/word/2018/wordml/cex\"></w16cex:commentsExtensible>",
);

fn parse_part(package: &Package, name: &str, default: &str) -> Result<DocxXml, Outcome> {
    let bytes = package
        .get(name)
        .map(<[u8]>::to_vec)
        .unwrap_or_else(|| default.as_bytes().to_vec());
    DocxXml::parse(&bytes).map_err(|error| {
        let detail = error
            .strip_prefix("document.xml parse failed: ")
            .unwrap_or(error.as_str());
        Outcome::error(format!("{name}: XML parse failed: {detail}"))
    })
}

/// Register a part's content type + document relationship (idempotent).
fn register_part(
    package: &mut Package,
    part: &str,
    content_type: &str,
    rel_type: &str,
) -> Result<(), Outcome> {
    // [Content_Types].xml override
    let mut types = parse_part(package, CONTENT_TYPES_PART, "")?;
    let root = types
        .doc
        .root_element()
        .ok_or_else(|| Outcome::error("malformed [Content_Types].xml"))?;
    let part_name = format!("/{part}");
    let existing = types.doc.children(root).find(|&child| {
        types.is_local(child, "Override")
            && types.doc.attribute(child, "PartName") == Some(part_name.as_str())
    });
    match existing {
        Some(node) => {
            // Repair a previously written Override that used a content type
            // Word does not recognize (the long openxmlformats form for the
            // commentsExtended/Ids/Extensible parts) — leaving the wrong type
            // in place is the same recovery dialog as omitting the part.
            if types.doc.attribute(node, "ContentType") != Some(content_type) {
                types.doc.set_attribute(node, "ContentType", content_type);
                package.set(CONTENT_TYPES_PART, types.serialize());
            }
        }
        None => {
            let node = types.doc.create_element("Override");
            types.doc.set_attribute(node, "PartName", &part_name);
            types.doc.set_attribute(node, "ContentType", content_type);
            types.doc.append_child(root, node);
            package.set(CONTENT_TYPES_PART, types.serialize());
        }
    }

    // word/_rels/document.xml.rels relationship
    let mut rels = parse_part(package, DOCUMENT_RELS_PART, "")?;
    let root = rels
        .doc
        .root_element()
        .ok_or_else(|| Outcome::error("malformed document.xml.rels"))?;
    let target = part.strip_prefix("word/").unwrap_or(part);
    let exists = rels.doc.children(root).any(|child| {
        rels.is_local(child, "Relationship") && rels.doc.attribute(child, "Target") == Some(target)
    });
    if !exists {
        let mut max_rid = 0u64;
        for child in rels.doc.children(root) {
            if let Some(id) = rels
                .doc
                .attribute(child, "Id")
                .and_then(|v| v.strip_prefix("rId"))
                .and_then(|v| v.parse::<u64>().ok())
            {
                max_rid = max_rid.max(id);
            }
        }
        let node = rels.doc.create_element("Relationship");
        rels.doc
            .set_attribute(node, "Id", &format!("rId{}", max_rid + 1));
        rels.doc.set_attribute(node, "Type", rel_type);
        rels.doc.set_attribute(node, "Target", target);
        rels.doc.append_child(root, node);
        package.set(DOCUMENT_RELS_PART, rels.serialize());
    }
    Ok(())
}

fn register_comment_parts(package: &mut Package) -> Result<(), Outcome> {
    register_part(
        package,
        COMMENTS_PART,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments",
    )?;
    register_part(
        package,
        EXTENDED_PART,
        "application/vnd.ms-word.commentsExtended+xml",
        "http://schemas.microsoft.com/office/2011/relationships/commentsExtended",
    )?;
    register_part(
        package,
        IDS_PART,
        "application/vnd.ms-word.commentsIds+xml",
        "http://schemas.microsoft.com/office/2016/09/relationships/commentsIds",
    )?;
    register_part(
        package,
        EXTENSIBLE_PART,
        "application/vnd.ms-word.commentsExtensible+xml",
        "http://schemas.microsoft.com/office/2018/08/relationships/commentsExtensible",
    )
}

struct CommentRecord {
    id: String,
    author: String,
    date: String,
    text: String,
    para_id: String,
}

fn comment_records(comments: &DocxXml) -> Vec<CommentRecord> {
    let Some(root) = comments.doc.root_element() else {
        return Vec::new();
    };
    comments
        .doc
        .children(root)
        .filter(|&node| comments.is_local(node, "comment"))
        .map(|node| {
            let paragraph = comments
                .doc
                .descendants(node)
                .find(|&n| comments.is_local(n, "p"));
            let text: String = comments
                .doc
                .descendants(node)
                .filter(|&n| comments.is_local(n, "t"))
                .map(|n| comments.doc.text_content(n))
                .collect();
            CommentRecord {
                id: comments.attr(node, "id").unwrap_or("").to_string(),
                author: comments.attr(node, "author").unwrap_or("").to_string(),
                date: comments.attr(node, "date").unwrap_or("").to_string(),
                text,
                para_id: paragraph
                    .and_then(|p| comments.attr(p, "paraId"))
                    .unwrap_or("")
                    .to_string(),
            }
        })
        .collect()
}

/// paraId -> (done, paraIdParent)
fn extended_records(extended: &DocxXml) -> Vec<(String, bool, Option<String>)> {
    let Some(root) = extended.doc.root_element() else {
        return Vec::new();
    };
    extended
        .doc
        .children(root)
        .filter(|&node| extended.is_local(node, "commentEx"))
        .map(|node| {
            (
                extended.attr(node, "paraId").unwrap_or("").to_string(),
                extended.attr(node, "done") == Some("1"),
                extended.attr(node, "paraIdParent").map(str::to_string),
            )
        })
        .collect()
}

fn durable_ids(ids: &DocxXml) -> Vec<(String, String)> {
    let Some(root) = ids.doc.root_element() else {
        return Vec::new();
    };
    ids.doc
        .children(root)
        .filter(|&node| ids.is_local(node, "commentId"))
        .map(|node| {
            (
                ids.attr(node, "paraId").unwrap_or("").to_string(),
                ids.attr(node, "durableId").unwrap_or("").to_string(),
            )
        })
        .collect()
}

fn next_comment_id(records: &[CommentRecord]) -> u64 {
    records
        .iter()
        .filter_map(|r| r.id.parse::<u64>().ok())
        .max()
        .map(|max| max + 1)
        .unwrap_or(0)
}

/// The next unused comment-paragraph id: a valid `w14:paraId` is exactly
/// eight hex digits with `00000000 < value < 80000000` — the same contract
/// `is_valid_para_id` enforces for document paragraphs — and must be unique
/// across both the comment records *and* the document's occupied paragraph
/// ids. Word treats a comments.xml paraId that collides with a body
/// paragraph as corrupt. Allocation starts past the highest existing id
/// (or at 1), probes forward on collision, and wraps within the valid range.
fn next_para_id(records: &[CommentRecord], extra_occupied: &HashSet<u64>) -> u64 {
    let mut occupied: HashSet<u64> = records
        .iter()
        .filter_map(|r| u64::from_str_radix(&r.para_id, 16).ok())
        .collect();
    occupied.extend(extra_occupied.iter().copied());
    let mut candidate = occupied
        .iter()
        .copied()
        .max()
        .map(|max| max + 1)
        .unwrap_or(1);
    loop {
        if candidate == 0 || candidate >= 0x8000_0000 {
            candidate = 1;
        }
        if !occupied.contains(&candidate) {
            return candidate;
        }
        candidate += 1;
    }
}

fn document_para_ids(xml: &DocxXml) -> HashSet<u64> {
    xml.all_paragraphs()
        .into_iter()
        .filter_map(|node| {
            xml.para_id(node)
                .and_then(|value| u64::from_str_radix(value, 16).ok())
        })
        .collect()
}

/// Deterministic 8-hex durable id (FNV-1a over identifying fields), clamped
/// into Word's valid `ST_LongHexNumber` window `00000000 < id < 7FFFFFFF`.
/// A raw u32 FNV lands with the high bit set about half the time; Word for
/// Mac recovers the package as unreadable content when that happens.
fn durable_id(para_id: &str, author: &str, text: &str, occupied: &HashSet<u32>) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in para_id.bytes().chain(author.bytes()).chain(text.bytes()) {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let mut candidate = (hash % 0x7FFF_FFFE) + 1;
    for _ in 0..0x7FFF_FFFE {
        if !occupied.contains(&candidate) {
            return format!("{candidate:08X}");
        }
        candidate += 1;
        if candidate >= 0x7FFF_FFFF {
            candidate = 1;
        }
    }
    format!("{candidate:08X}")
}

fn initials_from_author(author: &str) -> String {
    author
        .split_whitespace()
        .filter_map(|word| {
            word.chars()
                .find(|ch| ch.is_ascii_alphabetic())
                .map(|ch| ch.to_ascii_uppercase())
        })
        .take(4)
        .collect()
}

/// Append a w:comment to comments.xml + its extended/ids/extensible records.
#[allow(clippy::too_many_arguments)]
fn append_comment_record(
    package: &mut Package,
    id: u64,
    para_id: &str,
    parent_para_id: Option<&str>,
    author: &str,
    date: &str,
    text: &str,
) -> Result<(), Outcome> {
    let mut comments = parse_part(package, COMMENTS_PART, EMPTY_COMMENTS)?;
    let root = comments
        .doc
        .root_element()
        .ok_or_else(|| Outcome::error("malformed comments.xml"))?;
    let comment = comments.doc.create_element("w:comment");
    comments.doc.set_attribute(comment, "w:id", &id.to_string());
    comments.doc.set_attribute(comment, "w:author", author);
    comments.doc.set_attribute(comment, "w:date", date);
    comments
        .doc
        .set_attribute(comment, "w:initials", &initials_from_author(author));
    let paragraph = comments.doc.create_element("w:p");
    comments.doc.set_attribute(paragraph, "w14:paraId", para_id);
    let ppr = comments.doc.create_element("w:pPr");
    let pstyle = comments.doc.create_element("w:pStyle");
    comments.doc.set_attribute(pstyle, "w:val", "CommentText");
    comments.doc.append_child(ppr, pstyle);
    comments.doc.append_child(paragraph, ppr);
    let annotation_run = comments.doc.create_element("w:r");
    let annotation_rpr = comments.doc.create_element("w:rPr");
    let annotation_style = comments.doc.create_element("w:rStyle");
    comments
        .doc
        .set_attribute(annotation_style, "w:val", "CommentReference");
    comments.doc.append_child(annotation_rpr, annotation_style);
    comments.doc.append_child(annotation_run, annotation_rpr);
    let annotation_ref = comments.doc.create_element("w:annotationRef");
    comments.doc.append_child(annotation_run, annotation_ref);
    comments.doc.append_child(paragraph, annotation_run);
    let run = comments.create_run(text, None);
    comments.doc.append_child(paragraph, run);
    comments.doc.append_child(comment, paragraph);
    comments.doc.append_child(root, comment);
    // The appended paragraph carries `w14:paraId`; a pre-existing comments
    // part from another author may not declare the `w14` prefix at all.
    // Attribute names are stored verbatim, so an undeclared prefix would
    // serialize into a part strict XML consumers reject as corrupt — declare
    // the namespace on the part root before writing.
    comments.ensure_prefix_declared("w14", W14_NS);
    package.set(COMMENTS_PART, comments.serialize());

    let mut extended = parse_part(package, EXTENDED_PART, EMPTY_EXTENDED)?;
    let root = extended
        .doc
        .root_element()
        .ok_or_else(|| Outcome::error("malformed commentsExtended.xml"))?;
    let entry = extended.doc.create_element("w15:commentEx");
    extended.doc.set_attribute(entry, "w15:paraId", para_id);
    if let Some(parent) = parent_para_id {
        extended
            .doc
            .set_attribute(entry, "w15:paraIdParent", parent);
    }
    extended.doc.set_attribute(entry, "w15:done", "0");
    extended.doc.append_child(root, entry);
    extended.ensure_prefix_declared("w15", W15_NS);
    package.set(EXTENDED_PART, extended.serialize());

    let mut ids = parse_part(package, IDS_PART, EMPTY_IDS)?;
    let occupied: HashSet<u32> = durable_ids(&ids)
        .into_iter()
        .filter_map(|(_, durable)| u32::from_str_radix(&durable, 16).ok())
        .collect();
    let durable = durable_id(para_id, author, text, &occupied);
    let root = ids
        .doc
        .root_element()
        .ok_or_else(|| Outcome::error("malformed commentsIds.xml"))?;
    let entry = ids.doc.create_element("w16cid:commentId");
    ids.doc.set_attribute(entry, "w16cid:paraId", para_id);
    ids.doc.set_attribute(entry, "w16cid:durableId", &durable);
    ids.doc.append_child(root, entry);
    ids.ensure_prefix_declared("w16cid", W16CID_NS);
    package.set(IDS_PART, ids.serialize());

    let mut extensible = parse_part(package, EXTENSIBLE_PART, EMPTY_EXTENSIBLE)?;
    let root = extensible
        .doc
        .root_element()
        .ok_or_else(|| Outcome::error("malformed commentsExtensible.xml"))?;
    let entry = extensible.doc.create_element("w16cex:commentExtensible");
    extensible
        .doc
        .set_attribute(entry, "w16cex:durableId", &durable);
    extensible.doc.set_attribute(entry, "w16cex:dateUtc", date);
    extensible.doc.append_child(root, entry);
    extensible.ensure_prefix_declared("w16cex", W16CEX_NS);
    package.set(EXTENSIBLE_PART, extensible.serialize());

    register_comment_parts(package)
}

/// Anchor a comment to an in-memory paragraph char range and append its
/// comments/commentsExtended/commentsIds records — the shared tail behind both
/// the one-shot `addComment` and the typed `comment_add` op (which resolves
/// the paragraph by `w14:paraId` before calling this). Returns the new comment
/// id as a decimal string.
#[allow(clippy::too_many_arguments)]
pub(crate) fn anchor_comment_at(
    package: &mut Package,
    xml: &mut DocxXml,
    paragraph: NodeId,
    start: usize,
    end: usize,
    author: &str,
    date: &str,
    text: &str,
) -> Result<String, Outcome> {
    let comments = parse_part(package, COMMENTS_PART, EMPTY_COMMENTS)?;
    let records = comment_records(&comments);
    let id = next_comment_id(&records);
    let para_id = format!("{:08X}", next_para_id(&records, &document_para_ids(xml)));

    insert_anchor_markers(xml, paragraph, start, end, id)?;
    append_comment_record(package, id, &para_id, None, author, date, text)?;
    Ok(id.to_string())
}

/// Place commentRangeStart/End (+ the reference run) around [start, end).
fn insert_anchor_markers(
    xml: &mut DocxXml,
    paragraph: NodeId,
    start: usize,
    end: usize,
    id: u64,
) -> Result<(), Outcome> {
    crate::segments::split_boundary(xml, paragraph, start);
    crate::segments::split_boundary(xml, paragraph, end);
    let segments = build_segments(xml, paragraph);
    let id_string = id.to_string();

    let range_start = xml.doc.create_element("w:commentRangeStart");
    xml.doc.set_attribute(range_start, "w:id", &id_string);
    let range_end = xml.doc.create_element("w:commentRangeEnd");
    xml.doc.set_attribute(range_end, "w:id", &id_string);
    let reference_run = xml.doc.create_element("w:r");
    let reference_rpr = xml.doc.create_element("w:rPr");
    let reference_style = xml.doc.create_element("w:rStyle");
    xml.doc
        .set_attribute(reference_style, "w:val", "CommentReference");
    xml.doc.append_child(reference_rpr, reference_style);
    xml.doc.append_child(reference_run, reference_rpr);
    let reference = xml.doc.create_element("w:commentReference");
    xml.doc.set_attribute(reference, "w:id", &id_string);
    xml.doc.append_child(reference_run, reference);

    let first_anchor = segments
        .iter()
        .find(|s| s.start >= start)
        .map(|s| paragraph_child_anchor(xml, s.wrapper.unwrap_or(s.run)));
    let last_anchor = segments
        .iter()
        .rev()
        .find(|s| s.end <= end && s.start >= start)
        .map(|s| paragraph_child_anchor(xml, s.wrapper.unwrap_or(s.run)));

    match first_anchor {
        Some(anchor) => xml.doc.insert_before(anchor, range_start),
        None => xml.doc.append_child(paragraph, range_start),
    }
    // Word writes commentReference in a run after commentRangeEnd (the
    // MiniMax comments_guide / native Word layout).
    match last_anchor {
        Some(anchor) => {
            xml.doc.insert_after(anchor, range_end);
            xml.doc.insert_after(range_end, reference_run);
        }
        None => {
            xml.doc.append_child(paragraph, range_end);
            xml.doc.append_child(paragraph, reference_run);
        }
    }
    Ok(())
}

fn paragraph_child_anchor(xml: &DocxXml, node: NodeId) -> NodeId {
    let mut current = node;
    while let Some(parent) = xml.doc.parent(current) {
        if xml.is_w(parent, "p") {
            return current;
        }
        current = parent;
    }
    current
}

/// Anchor ranges per comment id: walk each paragraph tracking the all-view text
/// position at which each commentRangeStart/End marker sits.
fn anchor_ranges(xml: &DocxXml) -> Vec<(String, String)> {
    let mut ranges: Vec<(String, String)> = Vec::new();
    for paragraph in xml.paragraphs() {
        let mut offset = 0usize;
        let mut starts: Vec<(String, usize)> = Vec::new();
        walk_markers(
            xml,
            paragraph.node,
            &mut offset,
            &mut starts,
            &mut ranges,
            paragraph.index,
        );
    }
    ranges
}

fn walk_markers(
    xml: &DocxXml,
    node: NodeId,
    offset: &mut usize,
    starts: &mut Vec<(String, usize)>,
    ranges: &mut Vec<(String, String)>,
    paragraph_index: usize,
) {
    for child in xml.doc.children(node) {
        if !xml.doc.is_element(child) {
            continue;
        }
        if xml.is_w(child, "pPr") {
            continue;
        }
        if xml.is_w(child, "commentRangeStart") {
            if let Some(id) = xml.attr(child, "id") {
                starts.push((id.to_string(), *offset));
            }
            continue;
        }
        if xml.is_w(child, "commentRangeEnd") {
            if let Some(id) = xml.attr(child, "id") {
                if let Some(position) = starts.iter().position(|(sid, _)| sid == id) {
                    let (_, start) = starts.remove(position);
                    ranges.push((
                        id.to_string(),
                        format!("p{paragraph_index}:{start}-{offset}"),
                    ));
                }
            }
            continue;
        }
        if xml.is_w(child, "t") || xml.is_w(child, "delText") {
            *offset += xml.doc.text_content(child).chars().count();
            continue;
        }
        walk_markers(xml, child, offset, starts, ranges, paragraph_index);
    }
}

/// Comment threads (anchors, resolution status, replies) — the shared
/// collection logic behind both `listComments` and `read` (whose `comments`
/// field must match it exactly; see `readonly::read`).
pub(crate) fn comment_threads(package: &Package) -> Result<Vec<Value>, Outcome> {
    if package.get(COMMENTS_PART).is_none() {
        return Ok(Vec::new());
    }
    let comments = parse_part(package, COMMENTS_PART, EMPTY_COMMENTS)?;
    let extended = parse_part(package, EXTENDED_PART, EMPTY_EXTENDED).ok();
    let ids = parse_part(package, IDS_PART, EMPTY_IDS).ok();
    let document = package
        .document_xml()
        .ok()
        .and_then(|bytes| DocxXml::parse(bytes).ok());

    let records = comment_records(&comments);
    let extended_map = extended.as_ref().map(extended_records).unwrap_or_default();
    let durable_map = ids.as_ref().map(durable_ids).unwrap_or_default();
    let ranges = document.as_ref().map(anchor_ranges).unwrap_or_default();

    let done_of = |para_id: &str| {
        extended_map
            .iter()
            .find(|(p, _, _)| p == para_id)
            .map(|(_, done, _)| *done)
            .unwrap_or(false)
    };
    let parent_of = |para_id: &str| {
        extended_map
            .iter()
            .find(|(p, _, _)| p == para_id)
            .and_then(|(_, _, parent)| parent.clone())
    };
    let durable_of = |para_id: &str| {
        durable_map
            .iter()
            .find(|(p, _)| p == para_id)
            .map(|(_, d)| d.clone())
            .unwrap_or_default()
    };
    let comment_json = |record: &CommentRecord, thread_id: &str, parent_id: Option<&str>| {
        json!({
            "id": record.id,
            "threadId": thread_id,
            "parentId": parent_id,
            "ooxmlCommentId": record.id,
            "paraId": record.para_id,
            "durableId": durable_of(&record.para_id),
            "author": record.author,
            "date": record.date,
            "text": record.text,
        })
    };

    let mut threads: Vec<Value> = Vec::new();
    for root in records.iter().filter(|r| parent_of(&r.para_id).is_none()) {
        let replies: Vec<Value> = records
            .iter()
            .filter(|r| parent_of(&r.para_id).as_deref() == Some(root.para_id.as_str()))
            .map(|reply| comment_json(reply, &root.id, Some(&root.id)))
            .collect();
        let anchor: Vec<Value> = ranges
            .iter()
            .filter(|(id, _)| *id == root.id)
            .map(|(_, locator)| json!({ "locator": locator }))
            .collect();
        threads.push(json!({
            "id": root.id,
            "ooxmlId": root.id,
            "durableId": durable_of(&root.para_id),
            "status": if done_of(&root.para_id) { "resolved" } else { "open" },
            "anchor": { "ranges": anchor },
            "root": comment_json(root, &root.id, None),
            "replies": replies,
            "createdAt": root.date,
        }));
    }
    Ok(threads)
}

pub fn list_comments(package: &Package) -> Outcome {
    match comment_threads(package) {
        Ok(threads) => Outcome::ok(
            format!("comments {}", threads.len()),
            json!({ "comments": threads }),
        ),
        Err(outcome) => outcome,
    }
}

fn find_record<'a>(records: &'a [CommentRecord], comment_id: &str) -> Option<&'a CommentRecord> {
    records.iter().find(|r| r.id == comment_id)
}

pub fn reply_comment_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    comment_id: &str,
    text: &str,
    author: &str,
    ctx: &ExecCtx,
) -> Result<(String, Value), Outcome> {
    let comments = parse_part(package, COMMENTS_PART, EMPTY_COMMENTS)?;
    let records = comment_records(&comments);
    let Some(parent) = find_record(&records, comment_id) else {
        return Err(Outcome::error(format!("comment not found: {comment_id}")));
    };
    let parent_para_id = parent.para_id.clone();
    let id = next_comment_id(&records);
    let para_id = format!("{:08X}", next_para_id(&records, &document_para_ids(xml)));
    append_comment_record(
        package,
        id,
        &para_id,
        Some(&parent_para_id),
        author,
        &ctx.now,
        text,
    )?;

    Ok((
        format!("comment {id} replied"),
        json!({ "id": id.to_string(), "parentId": comment_id }),
    ))
}

pub fn resolve_comment_typed(
    package: &mut Package,
    _xml: &mut DocxXml,
    comment_id: &str,
    resolved: bool,
) -> Result<(String, Value), Outcome> {
    let comments = parse_part(package, COMMENTS_PART, EMPTY_COMMENTS)?;
    let records = comment_records(&comments);
    let Some(record) = find_record(&records, comment_id) else {
        return Err(Outcome::error(format!("comment not found: {comment_id}")));
    };
    let para_id = record.para_id.clone();

    let mut extended = parse_part(package, EXTENDED_PART, EMPTY_EXTENDED)?;
    let root = extended
        .doc
        .root_element()
        .ok_or_else(|| Outcome::error("malformed commentsExtended.xml"))?;
    let entry = extended.doc.children(root).find(|&node| {
        extended.is_local(node, "commentEx")
            && extended.attr(node, "paraId") == Some(para_id.as_str())
    });
    let done = if resolved { "1" } else { "0" };
    match entry {
        Some(node) => {
            // Update whichever storage form the attribute already has.
            let name = extended
                .doc
                .attributes(node)
                .iter()
                .find(|a| a.name == "done" || a.name.ends_with(":done"))
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "w15:done".to_string());
            extended.doc.set_attribute(node, &name, done);
        }
        None => {
            let node = extended.doc.create_element("w15:commentEx");
            extended.doc.set_attribute(node, "w15:paraId", &para_id);
            extended.doc.set_attribute(node, "w15:done", done);
            extended.doc.append_child(root, node);
            extended.ensure_prefix_declared("w15", W15_NS);
        }
    }
    package.set(EXTENDED_PART, extended.serialize());
    register_comment_parts(package)?;

    let verb = if resolved { "resolved" } else { "reopened" };
    Ok((
        format!("comment {comment_id} {verb}"),
        json!({ "id": comment_id, "resolved": resolved }),
    ))
}

pub fn delete_comment_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    comment_id: &str,
) -> Result<(String, Value), Outcome> {
    let mut comments = parse_part(package, COMMENTS_PART, EMPTY_COMMENTS)?;
    let records = comment_records(&comments);
    let Some(root_record) = find_record(&records, comment_id) else {
        return Err(Outcome::error(format!("comment not found: {comment_id}")));
    };

    // The thread = the root + every reply whose paraIdParent chains to it.
    let extended = parse_part(package, EXTENDED_PART, EMPTY_EXTENDED)?;
    let extended_map = extended_records(&extended);
    let mut thread_para_ids = vec![root_record.para_id.clone()];
    loop {
        let before = thread_para_ids.len();
        for (para_id, _, parent) in &extended_map {
            if let Some(parent) = parent {
                if thread_para_ids.contains(parent) && !thread_para_ids.contains(para_id) {
                    thread_para_ids.push(para_id.clone());
                }
            }
        }
        if thread_para_ids.len() == before {
            break;
        }
    }
    let thread_ids: Vec<String> = records
        .iter()
        .filter(|r| thread_para_ids.contains(&r.para_id))
        .map(|r| r.id.clone())
        .collect();

    // comments.xml: drop the thread's w:comment nodes.
    if let Some(root) = comments.doc.root_element() {
        let doomed: Vec<NodeId> = comments
            .doc
            .children(root)
            .filter(|&node| {
                comments.is_local(node, "comment")
                    && comments
                        .attr(node, "id")
                        .is_some_and(|id| thread_ids.iter().any(|t| t == id))
            })
            .collect();
        for node in doomed {
            comments.doc.remove_node(node);
        }
    }
    package.set(COMMENTS_PART, comments.serialize());

    // commentsExtended / commentsIds: drop the thread's records.
    let mut extended = parse_part(package, EXTENDED_PART, EMPTY_EXTENDED)?;
    if let Some(root) = extended.doc.root_element() {
        let doomed: Vec<NodeId> = extended
            .doc
            .children(root)
            .filter(|&node| {
                extended.is_local(node, "commentEx")
                    && extended
                        .attr(node, "paraId")
                        .is_some_and(|p| thread_para_ids.iter().any(|t| t == p))
            })
            .collect();
        for node in doomed {
            extended.doc.remove_node(node);
        }
    }
    package.set(EXTENDED_PART, extended.serialize());

    let mut ids = parse_part(package, IDS_PART, EMPTY_IDS)?;
    let durable_to_drop: HashSet<String> = durable_ids(&ids)
        .into_iter()
        .filter(|(para_id, _)| thread_para_ids.iter().any(|t| t == para_id))
        .map(|(_, durable)| durable)
        .collect();
    if let Some(root) = ids.doc.root_element() {
        let doomed: Vec<NodeId> = ids
            .doc
            .children(root)
            .filter(|&node| {
                ids.is_local(node, "commentId")
                    && ids
                        .attr(node, "paraId")
                        .is_some_and(|p| thread_para_ids.iter().any(|t| t == p))
            })
            .collect();
        for node in doomed {
            ids.doc.remove_node(node);
        }
    }
    package.set(IDS_PART, ids.serialize());

    let mut extensible = parse_part(package, EXTENSIBLE_PART, EMPTY_EXTENSIBLE)?;
    if let Some(root) = extensible.doc.root_element() {
        let doomed: Vec<NodeId> = extensible
            .doc
            .children(root)
            .filter(|&node| {
                extensible.is_local(node, "commentExtensible")
                    && extensible
                        .attr(node, "durableId")
                        .is_some_and(|d| durable_to_drop.contains(d))
            })
            .collect();
        for node in doomed {
            extensible.doc.remove_node(node);
        }
    }
    package.set(EXTENSIBLE_PART, extensible.serialize());

    // document.xml: strip the root's anchors (replies carry none).
    let Some(body) = xml.body() else {
        return Ok((
            format!("comment {comment_id} deleted"),
            json!({ "id": comment_id }),
        ));
    };
    let doomed: Vec<NodeId> = xml
        .doc
        .descendants(body)
        .filter(|&node| {
            if (xml.is_w(node, "commentRangeStart") || xml.is_w(node, "commentRangeEnd"))
                && xml.attr(node, "id") == Some(comment_id)
            {
                return true;
            }
            if xml.is_w(node, "r")
                && xml.doc.children(node).any(|child| {
                    xml.is_w(child, "commentReference") && xml.attr(child, "id") == Some(comment_id)
                })
            {
                return true;
            }
            false
        })
        .collect();
    for node in doomed {
        xml.doc.remove_node(node);
    }

    Ok((
        format!("comment {comment_id} deleted"),
        json!({ "id": comment_id }),
    ))
}

// Namespace constants (W14_NS/W15_NS/W16CID_NS/W16CEX_NS) are declared at
// the top of this module and used by the part-registration and append paths;
// they stay available for the parser helpers that are namespace-agnostic by
// design.
