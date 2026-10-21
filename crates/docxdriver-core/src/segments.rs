//! Paragraph segment model: the bridge between all-view text offsets and the run
//! tree. Every text-bearing leaf (w:t / w:delText) becomes one segment carrying
//! its char span in the paragraph's ALL-view text (deleted text still occupies
//! positions, matching the reference engine's locator space).
//!
//! Mutations invalidate spans, so the primitives here each perform one complete
//! operation; callers rebuild the segment list (cheap) before the next one.

use xmloxide::tree::NodeId;

use crate::document::DocxXml;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevWrap {
    None,
    Ins,
    Del,
}

pub struct Segment {
    /// The w:t or w:delText element.
    pub text_element: NodeId,
    /// The enclosing w:r.
    pub run: NodeId,
    /// The enclosing w:ins/w:del wrapper, when inside one.
    pub wrapper: Option<NodeId>,
    pub wrap: RevWrap,
    /// Char span in the paragraph's all-view text.
    pub start: usize,
    pub end: usize,
    pub text: String,
}

pub fn build_segments(xml: &DocxXml, paragraph: NodeId) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut offset = 0usize;
    collect(
        xml,
        paragraph,
        None,
        RevWrap::None,
        &mut offset,
        &mut segments,
    );
    segments
}

fn collect(
    xml: &DocxXml,
    node: NodeId,
    wrapper: Option<NodeId>,
    wrap: RevWrap,
    offset: &mut usize,
    out: &mut Vec<Segment>,
) {
    let children: Vec<NodeId> = xml.doc.children(node).collect();
    for child in children {
        if !xml.doc.is_element(child) {
            continue;
        }
        if xml.is_w(child, "p") {
            // Nested textbox/drawing paragraphs are separate stories and do
            // not occupy offsets in the selected paragraph.
            continue;
        }
        if xml.is_w(child, "pPr") {
            continue;
        }
        if xml.is_rev_ins(child) {
            collect(xml, child, Some(child), RevWrap::Ins, offset, out);
            continue;
        }
        if xml.is_rev_del(child) {
            collect(xml, child, Some(child), RevWrap::Del, offset, out);
            continue;
        }
        if xml.is_w(child, "t") || xml.is_w(child, "delText") {
            let text = xml.doc.text_content(child);
            let chars = text.chars().count();
            let run = enclosing_run(xml, child).expect("text element inside a run");
            out.push(Segment {
                text_element: child,
                run,
                wrapper,
                wrap,
                start: *offset,
                end: *offset + chars,
                text,
            });
            *offset += chars;
            continue;
        }
        if xml.is_local(child, "AlternateContent") {
            if let Some(branch) = xml.alternate_content_branch(child) {
                collect(xml, branch, wrapper, wrap, offset, out);
            }
            continue;
        }
        collect(xml, child, wrapper, wrap, offset, out);
    }
}

fn enclosing_run(xml: &DocxXml, mut node: NodeId) -> Option<NodeId> {
    while let Some(parent) = xml.doc.parent(node) {
        if xml.is_w(parent, "r") {
            return Some(parent);
        }
        if xml.is_w(parent, "p") {
            return None;
        }
        node = parent;
    }
    None
}

pub fn all_text(segments: &[Segment]) -> String {
    segments.iter().map(|s| s.text.as_str()).collect()
}

fn char_slice(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

/// Ensure a run boundary exists at all-view char `offset` inside the paragraph:
/// if `offset` falls strictly inside a segment, split that segment's run in two
/// (deep-cloning w:rPr so formatting survives). Idempotent at existing boundaries.
///
/// A run may hold several leaves (`<w:t/><w:br/><w:t/>`), and callers wrap or
/// format whole runs, so the boundary is a run boundary even where it falls
/// between two leaves of one run: content on either side goes to its own run.
pub fn split_boundary(xml: &mut DocxXml, paragraph: NodeId, offset: usize) {
    let segments = build_segments(xml, paragraph);
    let Some(segment) = segments.iter().find(|s| s.start < offset && offset < s.end) else {
        for segment in &segments {
            if segment.end == offset {
                if let Some(next) = next_run_content(xml, segment.text_element) {
                    split_run_before(xml, next);
                }
            }
            if segment.start == offset && previous_run_content(xml, segment.text_element) {
                split_run_before(xml, segment.text_element);
            }
        }
        return;
    };
    // Content after the split leaf keeps its place after the tail.
    if let Some(next) = next_run_content(xml, segment.text_element) {
        split_run_before(xml, next);
    }
    let head = char_slice(&segment.text, 0, offset - segment.start);
    let tail = char_slice(
        &segment.text,
        offset - segment.start,
        segment.end - segment.start,
    );

    xml.set_text(segment.text_element, &head);
    let rpr = xml.run_rpr(segment.run);
    let new_run = xml.create_run(&tail, rpr);
    if matches!(segment.wrap, RevWrap::Del) {
        // A split inside deleted content must keep producing w:delText leaves.
        if let Some(t) = xml.child_w(new_run, "t") {
            let name = rename_for(xml, t, true);
            xml.doc.rename_element(t, name);
        }
    }
    xml.doc.insert_after(segment.run, new_run);
}

/// The element after `leaf` in its run, when `leaf` is a direct run child.
fn next_run_content(xml: &DocxXml, leaf: NodeId) -> Option<NodeId> {
    let parent = xml.doc.parent(leaf)?;
    if !xml.is_w(parent, "r") {
        return None;
    }
    std::iter::successors(xml.doc.next_sibling(leaf), |&n| xml.doc.next_sibling(n))
        .find(|&n| xml.doc.is_element(n))
}

/// Whether run content other than w:rPr precedes `leaf` in its run.
fn previous_run_content(xml: &DocxXml, leaf: NodeId) -> bool {
    let Some(parent) = xml.doc.parent(leaf) else {
        return false;
    };
    xml.is_w(parent, "r")
        && std::iter::successors(xml.doc.prev_sibling(leaf), |&n| xml.doc.prev_sibling(n))
            .any(|n| xml.doc.is_element(n) && !xml.is_w(n, "rPr"))
}

/// Move `child` and everything after it in its run into a new run, with a
/// deep copy of the run's w:rPr, placed right after the original run.
fn split_run_before(xml: &mut DocxXml, child: NodeId) {
    let Some(run) = xml.doc.parent(child) else {
        return;
    };
    let new_run = xml.doc.create_element("w:r");
    if let Some(rpr) = xml.run_rpr(run) {
        let clone = xml.doc.clone_node(rpr, true);
        xml.doc.append_child(new_run, clone);
    }
    let moved: Vec<NodeId> =
        std::iter::successors(Some(child), |&n| xml.doc.next_sibling(n)).collect();
    for node in moved {
        xml.doc.detach(node);
        xml.doc.append_child(new_run, node);
    }
    xml.doc.insert_after(run, new_run);
}

/// The correct new tag for renaming THIS leaf, respecting its own storage form:
/// a parsed leaf keeps its "w" prefix (local name only), a created leaf carries
/// the qualified name. Renaming a prefixed node to "w:delText" would serialize
/// as `w:w:delText`.
pub fn rename_for(xml: &DocxXml, leaf: NodeId, del: bool) -> &'static str {
    let has_prefix = xml.doc.node_prefix(leaf).is_some();
    match (del, has_prefix) {
        (true, true) => "delText",
        (true, false) => "w:delText",
        (false, true) => "t",
        (false, false) => "w:t",
    }
}

/// Rename the run leaves under `root` for a tracked deletion (`del`) or its
/// rejection: `w:t` and `w:delText`, `w:instrText` and `w:delInstrText`.
/// A text box (`w:txbxContent`) is its own story with its own revision
/// marks, so leaves inside one are left alone.
pub fn mark_deleted_leaves(xml: &mut DocxXml, root: NodeId, del: bool) {
    let (from, to): (&[&str], [&str; 2]) = if del {
        (&["t", "instrText"], ["delText", "delInstrText"])
    } else {
        (&["delText", "delInstrText"], ["t", "instrText"])
    };
    let leaves: Vec<(NodeId, usize)> = xml
        .doc
        .descendants(root)
        .filter_map(|n| from.iter().position(|name| xml.is_w(n, name)).map(|i| (n, i)))
        .filter(|&(n, _)| {
            !xml.doc
                .ancestors(n)
                .take_while(|&a| a != root)
                .any(|a| xml.is_w(a, "txbxContent"))
        })
        .collect();
    for (leaf, index) in leaves {
        // A parsed leaf keeps its prefix (local name only); a created leaf
        // carries the qualified name.
        let name = if xml.doc.node_prefix(leaf).is_some() {
            to[index].to_string()
        } else {
            format!("w:{}", to[index])
        };
        xml.doc.rename_element(leaf, &name);
    }
}

/// Range markers that keep their place, collapsed, when the content around
/// them goes: their counterpart may sit outside the removed content.
const RANGE_MARKERS: &[&str] = &[
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
];

/// Remove an element and its content without breaking what pairs
/// with it from outside: range markers stay as zero-width markers, comment
/// references stay in a bare run (their comment would be left unanchored),
/// and a field cut in two loses its remaining field characters and code so
/// its surviving result reads as plain text.
pub fn remove_content(xml: &mut DocxXml, node: NodeId) {
    let in_own_story = |xml: &DocxXml, n: NodeId| {
        !xml.doc
            .ancestors(n)
            .take_while(|&a| a != node)
            .any(|a| xml.is_w(a, "txbxContent") || xml.is_local(a, "Fallback"))
    };
    let markers: Vec<NodeId> = xml
        .doc
        .descendants(node)
        .filter(|&n| RANGE_MARKERS.iter().any(|m| xml.is_w(n, m)))
        .filter(|&n| {
            in_own_story(xml, n)
                && !xml.doc.ancestors(n).take_while(|&a| a != node).any(|a| xml.is_w(a, "r"))
        })
        .collect();
    let references: Vec<NodeId> = xml
        .doc
        .descendants(node)
        .filter(|&n| xml.is_w(n, "commentReference") && in_own_story(xml, n))
        .collect();
    let orphans = broken_field_parts(xml, node);
    for marker in markers {
        xml.doc.detach(marker);
        xml.doc.insert_before(node, marker);
    }
    for reference in references {
        let run = xml.doc.create_element("w:r");
        xml.doc.detach(reference);
        xml.doc.append_child(run, reference);
        xml.doc.insert_before(node, run);
    }
    xml.doc.remove_node(node);
    for part in orphans {
        let run = xml.doc.parent(part).filter(|&r| xml.is_w(r, "r"));
        xml.doc.remove_node(part);
        if let Some(run) = run {
            let empty = xml.doc.children(run).all(|c| !xml.doc.is_element(c) || xml.is_w(c, "rPr"));
            if empty {
                xml.doc.remove_node(run);
            }
        }
    }
}

/// Field characters and code outside `node` that belong to a field `node`
/// cuts in two: one with some, but not all, of its begin/separate/end inside.
fn broken_field_parts(xml: &DocxXml, node: NodeId) -> Vec<NodeId> {
    // The story holding `node`: its text box, or the whole part.
    let story = xml
        .doc
        .ancestors(node)
        .find(|&a| xml.is_w(a, "txbxContent"))
        .or_else(|| xml.doc.root_element());
    let Some(story) = story else {
        return Vec::new();
    };
    let inside = |n: NodeId| xml.doc.ancestors(n).any(|a| a == node);
    struct Field {
        own: Vec<NodeId>,
        code: Vec<NodeId>,
        separated: bool,
    }
    let mut fields: Vec<Field> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    for n in xml.doc.descendants(story) {
        let field_char = xml.is_w(n, "fldChar");
        let code = xml.is_w(n, "instrText") || xml.is_w(n, "delInstrText");
        if !(field_char || code) {
            continue;
        }
        let nested = xml
            .doc
            .ancestors(n)
            .take_while(|&a| a != story)
            .any(|a| xml.is_w(a, "txbxContent") || xml.is_local(a, "Fallback"));
        if nested {
            continue;
        }
        // Everything met while a field is still in its code belongs to that
        // code, nested fields included.
        for &f in &open {
            if !fields[f].separated {
                fields[f].code.push(n);
            }
        }
        if !field_char {
            continue;
        }
        match xml.attr(n, "fldCharType") {
            Some("begin") => {
                open.push(fields.len());
                fields.push(Field { own: vec![n], code: Vec::new(), separated: false });
            }
            Some("separate") => {
                if let Some(&f) = open.last() {
                    fields[f].code.pop();
                    fields[f].own.push(n);
                    fields[f].separated = true;
                }
            }
            Some("end") => {
                if let Some(f) = open.pop() {
                    if !fields[f].separated {
                        fields[f].code.pop();
                    }
                    fields[f].own.push(n);
                }
            }
            _ => {}
        }
    }
    let mut parts: Vec<NodeId> = Vec::new();
    for field in fields {
        let cut = field.own.iter().any(|&n| inside(n)) && field.own.iter().any(|&n| !inside(n));
        if !cut {
            continue;
        }
        for n in field.own.into_iter().chain(field.code) {
            if !inside(n) && !parts.contains(&n) {
                parts.push(n);
            }
        }
    }
    parts
}

/// Placement anchor for a segment: its revision wrapper when present, otherwise
/// its run. Stays inside intermediate containers (hyperlinks) so text edits
/// inside an `<a>` do not yank the whole link into a sibling delete/insert.
fn anchor_outside_wrapper(_xml: &DocxXml, segment: &Segment) -> NodeId {
    segment.wrapper.unwrap_or(segment.run)
}

/// Walk up from `node` to the enclosing `w:hyperlink`, if any.
fn enclosing_hyperlink(xml: &DocxXml, mut node: NodeId) -> Option<NodeId> {
    while let Some(parent) = xml.doc.parent(node) {
        if xml.is_w(parent, "hyperlink") {
            return Some(parent);
        }
        if xml.is_w(parent, "p") {
            return None;
        }
        node = parent;
    }
    None
}

/// Content units of a tracked insertion: text (with its formatting) plus the
/// run-level break elements that occupy no position in the all-view char space,
/// plus opaque package objects (notes, equations, images).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsPiece {
    Text(String, crate::html::Fmt),
    Br(crate::html::BrKind),
    Tab,
    /// OMML equation. `mathml` is presentation MathML; `display` selects
    /// `m:oMathPara` vs inline `m:oMath`.
    Equation {
        mathml: String,
        display: bool,
    },
    /// DrawingML image referencing an already-registered media relationship.
    Image {
        rid: String,
        width: Option<u32>,
        height: Option<u32>,
        alt: Option<String>,
    },
}

/// Insert a `<w:ins>` at all-view char `offset`, one run per formatting
/// transition: a formatted text piece opens a fresh run carrying the b/i/u
/// properties; Br/Tab ride in whichever run is current.
///
/// When `inside_link` is true, the insert stays inside an enclosing hyperlink.
/// When false, an insert anchored at the end of linked text is placed *after*
/// the `w:hyperlink` element so unlinked content does not become linked.
pub fn insert_ins_pieces_at(
    xml: &mut DocxXml,
    paragraph: NodeId,
    offset: usize,
    pieces: &[InsPiece],
    id: u64,
    author: &str,
    date: &str,
    inside_link: bool,
) -> Result<(), String> {
    split_boundary(xml, paragraph, offset);
    let segments = build_segments(xml, paragraph);
    let ins = xml.doc.create_element("w:ins");
    xml.doc.set_attribute(ins, "w:id", &id.to_string());
    xml.doc.set_attribute(ins, "w:author", author);
    xml.doc.set_attribute(ins, "w:date", date);

    let mut run: Option<NodeId> = None;
    let mut run_fmt = crate::html::Fmt::default();
    let ensure_run = |xml: &mut DocxXml,
                      fmt: crate::html::Fmt,
                      run: &mut Option<NodeId>,
                      run_fmt: &mut crate::html::Fmt| {
        if run.is_none() || *run_fmt != fmt {
            let new_run = xml.doc.create_element("w:r");
            if fmt != crate::html::Fmt::default() {
                let rpr = xml.doc.create_element("w:rPr");
                crate::html::styles::append_rpr(xml, rpr, fmt);
                xml.doc.append_child(new_run, rpr);
            }
            xml.doc.append_child(ins, new_run);
            *run = Some(new_run);
            *run_fmt = fmt;
        }
        run.expect("just ensured")
    };
    for piece in pieces {
        match piece {
            InsPiece::Text(text, fmt) => {
                let run = ensure_run(xml, *fmt, &mut run, &mut run_fmt);
                let t = xml.doc.create_element("w:t");
                if text.starts_with(' ') || text.ends_with(' ') {
                    xml.ensure_space_preserve(t);
                }
                let content = xml.doc.create_text(text);
                xml.doc.append_child(t, content);
                xml.doc.append_child(run, t);
            }
            InsPiece::Br(kind) => {
                let run = ensure_run(xml, run_fmt, &mut run, &mut run_fmt);
                let br = xml.doc.create_element("w:br");
                if let Some(break_type) = kind.dialect_type() {
                    xml.doc.set_attribute(br, "w:type", break_type);
                }
                xml.doc.append_child(run, br);
            }
            InsPiece::Tab => {
                let run = ensure_run(xml, run_fmt, &mut run, &mut run_fmt);
                let tab = xml.doc.create_element("w:tab");
                xml.doc.append_child(run, tab);
            }
            InsPiece::Equation { mathml, display } => {
                match crate::html::mathml_omml::mathml_to_omml_xml(mathml, *display) {
                    Ok(omml_xml) => {
                        if let Err(err) = append_omml_xml(xml, ins, &omml_xml) {
                            return Err(err);
                        }
                    }
                    Err(err) => return Err(err),
                }
                run = None;
            }
            InsPiece::Image {
                rid,
                width,
                height,
                alt,
            } => {
                let new_run = xml.doc.create_element("w:r");
                // Minimal DrawingML inline picture so Word opens the part.
                let drawing = xml.doc.create_element("w:drawing");
                let inline = xml.doc.create_element("wp:inline");
                xml.doc.set_attribute(
                    inline,
                    "xmlns:wp",
                    "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing",
                );
                let extent = xml.doc.create_element("wp:extent");
                xml.doc.set_attribute(
                    extent,
                    "cx",
                    &(u64::from(width.unwrap_or(96)) * 9525).to_string(),
                );
                xml.doc.set_attribute(
                    extent,
                    "cy",
                    &(u64::from(height.unwrap_or(96)) * 9525).to_string(),
                );
                xml.doc.append_child(inline, extent);
                let doc_pr = xml.doc.create_element("wp:docPr");
                xml.doc.set_attribute(doc_pr, "id", "1");
                xml.doc.set_attribute(doc_pr, "name", "Picture");
                if let Some(alt) = alt {
                    xml.doc.set_attribute(doc_pr, "descr", alt);
                }
                xml.doc.append_child(inline, doc_pr);
                let graphic = xml.doc.create_element("a:graphic");
                xml.doc.set_attribute(
                    graphic,
                    "xmlns:a",
                    "http://schemas.openxmlformats.org/drawingml/2006/main",
                );
                let gdata = xml.doc.create_element("a:graphicData");
                xml.doc.set_attribute(
                    gdata,
                    "uri",
                    "http://schemas.openxmlformats.org/drawingml/2006/picture",
                );
                let pic = xml.doc.create_element("pic:pic");
                xml.doc.set_attribute(
                    pic,
                    "xmlns:pic",
                    "http://schemas.openxmlformats.org/drawingml/2006/picture",
                );
                let blip_fill = xml.doc.create_element("pic:blipFill");
                let blip = xml.doc.create_element("a:blip");
                xml.doc.set_attribute(
                    blip,
                    "xmlns:r",
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
                );
                xml.doc.set_attribute(blip, "r:embed", rid);
                xml.doc.append_child(blip_fill, blip);
                xml.doc.append_child(pic, blip_fill);
                let sp_pr = xml.doc.create_element("pic:spPr");
                xml.doc.append_child(pic, sp_pr);
                xml.doc.append_child(gdata, pic);
                xml.doc.append_child(graphic, gdata);
                xml.doc.append_child(inline, graphic);
                xml.doc.append_child(drawing, inline);
                xml.doc.append_child(new_run, drawing);
                xml.doc.append_child(ins, new_run);
                run = None;
            }
        }
    }

    let resolve_anchor = |xml: &DocxXml, segment: &Segment, at_end: bool| -> NodeId {
        let anchor = anchor_outside_wrapper(xml, segment);
        if !inside_link && at_end {
            if let Some(link) = enclosing_hyperlink(xml, anchor) {
                // Only escape when this segment is the last text inside the link.
                let link_end = segments
                    .iter()
                    .filter(|s| enclosing_hyperlink(xml, s.run) == Some(link))
                    .map(|s| s.end)
                    .max()
                    .unwrap_or(0);
                if segment.end == link_end {
                    return link;
                }
            }
        }
        anchor
    };

    if let Some(after) = segments.iter().rev().find(|s| s.end <= offset) {
        let anchor = resolve_anchor(xml, after, after.end == offset);
        xml.doc.insert_after(anchor, ins);
    } else if let Some(before) = segments.iter().find(|s| s.start >= offset) {
        let mut anchor = anchor_outside_wrapper(xml, before);
        if !inside_link {
            if let Some(link) = enclosing_hyperlink(xml, anchor) {
                let link_start = segments
                    .iter()
                    .filter(|s| enclosing_hyperlink(xml, s.run) == Some(link))
                    .map(|s| s.start)
                    .min()
                    .unwrap_or(0);
                if before.start == link_start {
                    anchor = link;
                }
            }
        }
        xml.doc.insert_before(anchor, ins);
    } else {
        xml.doc.append_child(paragraph, ins);
    }
    Ok(())
}

/// Parse an `m:oMath` / `m:oMathPara` XML fragment and append it under `parent`,
/// recreating nodes in the destination arena.
pub(crate) fn append_omml_xml(
    xml: &mut DocxXml,
    parent: NodeId,
    omml_xml: &str,
) -> Result<(), String> {
    let wrapped = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><w:body><w:p>{omml_xml}</w:p></w:body></w:document>"#
    );
    let frag = DocxXml::parse(wrapped.as_bytes())?;
    let body = frag
        .body()
        .ok_or_else(|| "OMML fragment parse lost body".to_string())?;
    let p = frag
        .doc
        .children(body)
        .find(|&n| frag.is_w(n, "p"))
        .ok_or_else(|| "OMML fragment parse lost paragraph".to_string())?;
    for child in frag.doc.children(p).collect::<Vec<_>>() {
        if !frag.doc.is_element(child) {
            continue;
        }
        let imported = import_element(xml, &frag, child);
        xml.doc.append_child(parent, imported);
    }
    Ok(())
}

pub(crate) fn import_element(dest: &mut DocxXml, src: &DocxXml, node: NodeId) -> NodeId {
    let name = src.doc.node_name(node).unwrap_or("unknown");
    // Prefer qualified names with the prefix already present on the source.
    let qname = if name.contains(':') {
        name.to_string()
    } else if let Some(prefix) = src.doc.node_prefix(node) {
        format!("{prefix}:{name}")
    } else {
        name.to_string()
    };
    let new_node = dest.doc.create_element(&qname);
    for attr in src.doc.attributes(node) {
        let aname = if attr.name.contains(':') {
            attr.name.clone()
        } else if let Some(prefix) = attr.prefix.as_deref() {
            format!("{prefix}:{}", attr.name)
        } else {
            attr.name.clone()
        };
        dest.doc
            .set_attribute(new_node, &aname, attr.value.as_str());
    }
    for child in src.doc.children(node).collect::<Vec<_>>() {
        if src.doc.is_element(child) {
            let imported = import_element(dest, src, child);
            dest.doc.append_child(new_node, imported);
        } else if let Some(text) = src.doc.node_text(child) {
            let t = dest.doc.create_text(text);
            dest.doc.append_child(new_node, t);
        }
    }
    new_node
}

fn prev_element_sibling(xml: &DocxXml, node: NodeId) -> Option<NodeId> {
    let mut cursor = xml.doc.prev_sibling(node);
    while let Some(candidate) = cursor {
        if xml.doc.is_element(candidate) {
            return Some(candidate);
        }
        cursor = xml.doc.prev_sibling(candidate);
    }
    None
}

fn has_text_leaves(xml: &DocxXml, node: NodeId) -> bool {
    xml.doc
        .descendants(node)
        .any(|n| xml.is_w(n, "t") || xml.is_w(n, "delText"))
}

/// Tracked deletion over MIXED content (reference-engine `deleteTrackedText`
/// semantics). Per covered slice: already-deleted content is skipped; the
/// author's own pending insertion is hard-deleted (Word removes your own
/// unaccepted text outright); anything else — plain runs or another author's
/// insertion — is wrapped in a `<w:del>` in place, which nests the del inside
/// the foreign `<w:ins>` naturally (the run's parent is the wrapper). Every
/// del created by one call shares `id`.
pub fn delete_tracked_range(
    xml: &mut DocxXml,
    paragraph: NodeId,
    start: usize,
    end: usize,
    id: u64,
    author: &str,
    date: &str,
) -> Result<(), String> {
    split_boundary(xml, paragraph, end);
    split_boundary(xml, paragraph, start);
    let segments = build_segments(xml, paragraph);
    let covered: Vec<&Segment> = segments
        .iter()
        .filter(|s| s.start >= start && s.end <= end && s.start < s.end)
        .collect();
    if covered.is_empty() {
        return Err("delete range covers no text".into());
    }

    let mut current_del: Option<NodeId> = None;
    for segment in covered {
        if segment.wrap == RevWrap::Del {
            current_del = None;
            continue;
        }
        if segment.wrap == RevWrap::Ins {
            let wrapper = segment.wrapper.expect("ins segment has a wrapper");
            if xml.attr(wrapper, "author") == Some(author) {
                // Hard delete: the author removes their own pending insertion.
                xml.doc.remove_node(segment.text_element);
                if !has_text_leaves(xml, segment.run) {
                    xml.doc.remove_node(segment.run);
                }
                if !has_text_leaves(xml, wrapper) {
                    let children: Vec<NodeId> = xml.doc.children(wrapper).collect();
                    for child in children {
                        xml.doc.detach(child);
                        xml.doc.insert_before(wrapper, child);
                    }
                    xml.doc.remove_node(wrapper);
                }
                current_del = None;
                continue;
            }
        }
        let Some(parent) = xml.doc.parent(segment.run) else {
            return Err("run has no parent".into());
        };
        match current_del {
            Some(del)
                if xml.doc.parent(del) == Some(parent)
                    && prev_element_sibling(xml, segment.run) == Some(del) =>
            {
                xml.doc.detach(segment.run);
                xml.doc.append_child(del, segment.run);
            }
            _ => {
                let del = xml.doc.create_element("w:del");
                xml.doc.set_attribute(del, "w:id", &id.to_string());
                xml.doc.set_attribute(del, "w:author", author);
                xml.doc.set_attribute(del, "w:date", date);
                xml.doc.insert_before(segment.run, del);
                xml.doc.detach(segment.run);
                xml.doc.append_child(del, segment.run);
                current_del = Some(del);
            }
        }
        if xml.is_w(segment.text_element, "t") {
            let name = rename_for(xml, segment.text_element, true);
            xml.doc.rename_element(segment.text_element, name);
        }
    }
    Ok(())
}

/// Whether `segment` lies in `author`'s own pending `w:ins`: text they may
/// still rewrite without a deletion.
pub fn is_own_insertion(xml: &DocxXml, segment: &Segment, author: &str) -> bool {
    segment.wrap == RevWrap::Ins
        && segment
            .wrapper
            .is_some_and(|ins| xml.is_w(ins, "ins") && xml.attr(ins, "author") == Some(author))
}

/// Split `author`'s own `w:ins` at all-view `offset` when the offset falls
/// strictly inside it, so a new insertion lands there as a sibling instead
/// of after the whole wrapper. The second half takes a fresh revision id.
///
///   <ins 1>" v|ery"</ins>  ->  <ins 1>" v"</ins>|<ins 2>"ery"</ins>
pub fn split_own_ins(xml: &mut DocxXml, paragraph: NodeId, offset: usize, author: &str) {
    split_boundary(xml, paragraph, offset);
    let segments = build_segments(xml, paragraph);

    // The text just left of the offset must sit directly in an own w:ins
    // that continues right of the offset.
    let Some(before) = segments.iter().rev().find(|s| s.end <= offset && s.start < s.end) else {
        return;
    };
    if !is_own_insertion(xml, before, author) {
        return;
    }
    let ins = before.wrapper.expect("own insertion has a wrapper");
    if xml.doc.parent(before.run) != Some(ins) {
        return;
    }
    let continues = segments
        .iter()
        .any(|s| s.start >= offset && s.wrapper == Some(ins));
    if !continues {
        return;
    }

    // Move everything after the left run into a twin wrapper.
    let twin = xml.doc.create_element("w:ins");
    xml.doc
        .set_attribute(twin, "w:id", &xml.next_revision_id().to_string());
    for name in ["author", "date"] {
        if let Some(value) = xml.attr(ins, name).map(str::to_string) {
            xml.doc.set_attribute(twin, &format!("w:{name}"), &value);
        }
    }
    xml.doc.insert_after(ins, twin);
    while let Some(next) = xml.doc.next_sibling(before.run) {
        xml.doc.detach(next);
        xml.doc.append_child(twin, next);
    }
}

/// Fold the `w:ins` with revision `id` into `author`'s adjacent own
/// insertions, as Word extends your pending text rather than stacking
/// revisions: `<ins 1>" v"</ins><ins 3>"X"</ins><ins 2>"ery"</ins>` becomes
/// `<ins 1>" vXery"</ins>`.
pub fn merge_own_ins(xml: &mut DocxXml, paragraph: NodeId, id: u64, author: &str) {
    let Some(mut ins) = xml.doc.descendants(paragraph).find(|&node| {
        xml.is_w(node, "ins") && xml.attr(node, "id").and_then(|v| v.parse::<u64>().ok()) == Some(id)
    }) else {
        return;
    };
    let own = |xml: &DocxXml, node: Option<NodeId>| {
        node.filter(|&n| xml.is_w(n, "ins") && xml.attr(n, "author") == Some(author))
    };

    // Left neighbor absorbs this insertion.
    if let Some(left) = own(xml, prev_element_sibling(xml, ins)) {
        move_children(xml, ins, left);
        xml.doc.remove_node(ins);
        ins = left;
    }

    // This insertion absorbs its right neighbor.
    if let Some(right) = own(xml, next_element_sibling(xml, ins)) {
        move_children(xml, right, ins);
        xml.doc.remove_node(right);
    }
}

fn move_children(xml: &mut DocxXml, from: NodeId, to: NodeId) {
    let children: Vec<NodeId> = xml.doc.children(from).collect();
    for child in children {
        xml.doc.detach(child);
        xml.doc.append_child(to, child);
    }
}

fn next_element_sibling(xml: &DocxXml, node: NodeId) -> Option<NodeId> {
    let mut cursor = xml.doc.next_sibling(node);
    while let Some(candidate) = cursor {
        if xml.doc.is_element(candidate) {
            return Some(candidate);
        }
        cursor = xml.doc.next_sibling(candidate);
    }
    None
}

/* Wrap [start, end) in a `<w:del>`, also carrying non-text nodes sitting
/// between the covered runs (w:br/w:tab runs, comment markers) into the
/// delete so deleting "a\tb" deletes the tab with it. The covered text
/// must still be plain.
pub fn wrap_range_in_del_inclusive(
    xml: &mut DocxXml,
    paragraph: NodeId,
    start: usize,
    end: usize,
    id: u64,
    author: &str,
    date: &str,
) -> Result<(), String> {
    split_boundary(xml, paragraph, start);
    split_boundary(xml, paragraph, end);
    let segments = build_segments(xml, paragraph);
    let covered: Vec<&Segment> = segments
        .iter()
        .filter(|s| s.start >= start && s.end <= end && s.start < s.end)
        .collect();
    if covered.is_empty() {
        return Err("delete range covers no text".into());
    }
    if covered.iter().any(|s| s.wrap != RevWrap::None) {
        return Err("delete range overlaps existing tracked changes".into());
    }
    let first_anchor = anchor_outside_wrapper(xml, covered[0]);
    let last_anchor = anchor_outside_wrapper(xml, covered[covered.len() - 1]);

    // Every paragraph-child between the anchors moves into the del. Boundary
    // splits guarantee any text leaf in that span is covered; revision
    // wrappers in the span would have surfaced as wrapped segments above.
    let mut nodes = Vec::new();
    let mut cursor = Some(first_anchor);
    while let Some(node) = cursor {
        if xml.is_rev_ins(node) || xml.is_rev_del(node) {
            return Err("delete range overlaps existing tracked changes".into());
        }
        nodes.push(node);
        if node == last_anchor {
            break;
        }
        cursor = xml.doc.next_sibling(node);
    }
    if nodes.last() != Some(&last_anchor) {
        return Err("delete range is not contiguous".into());
    }

    let del = xml.doc.create_element("w:del");
    xml.doc.set_attribute(del, "w:id", &id.to_string());
    xml.doc.set_attribute(del, "w:author", author);
    xml.doc.set_attribute(del, "w:date", date);
    xml.doc.insert_before(first_anchor, del);
    for node in nodes {
        xml.doc.detach(node);
        xml.doc.append_child(del, node);
    }
    let leaves: Vec<NodeId> = xml
        .doc
        .descendants(del)
        .filter(|&n| xml.is_w(n, "t"))
        .collect();
    for leaf in leaves {
        let new_name = rename_for(xml, leaf, true);
        xml.doc.rename_element(leaf, new_name);
    }
    Ok(())
}
*/

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::View;

    #[test]
    fn segments_match_visible_fallback_text_and_ignore_nested_paragraphs() {
        let source = concat!(
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
            "xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\">",
            "<w:body><w:p>",
            "<w:r><w:t>abc</w:t></w:r>",
            "<w:r><mc:AlternateContent>",
            "<mc:Choice Requires=\"wps\"><w:r><w:t>wrong</w:t></w:r></mc:Choice>",
            "<mc:Fallback><w:t>def</w:t><w:drawing><w:txbxContent>",
            "<w:p><w:r><w:t>16′16′16′16′</w:t></w:r></w:p>",
            "</w:txbxContent></w:drawing></mc:Fallback>",
            "</mc:AlternateContent></w:r>",
            "<w:r><w:t>ghi</w:t></w:r>",
            "</w:p></w:body></w:document>"
        );
        let xml = DocxXml::parse(source.as_bytes()).unwrap();
        let paragraph = xml.paragraphs()[0].node;
        let segments = build_segments(&xml, paragraph);

        assert_eq!(all_text(&segments), "abcdefghi");
        assert_eq!(xml.paragraph_text(paragraph, View::All), "abcdefghi");
        assert_eq!(
            segments
                .iter()
                .map(|s| (s.text.as_str(), s.start, s.end))
                .collect::<Vec<_>>(),
            [("abc", 0, 3), ("def", 3, 6), ("ghi", 6, 9)]
        );
        assert!(xml.is_w(segments[1].run, "r"));
        assert_eq!(xml.doc.parent(segments[1].run), Some(paragraph));
    }

    #[test]
    fn segments_ignore_unsupported_choice_without_fallback() {
        let source = concat!(
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
            "xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\">",
            "<w:body><w:p><w:r><w:t>before</w:t><mc:AlternateContent>",
            "<mc:Choice Requires=\"wps\"><w:t>unsupported</w:t></mc:Choice>",
            "</mc:AlternateContent><w:t>after</w:t></w:r></w:p></w:body></w:document>"
        );
        let xml = DocxXml::parse(source.as_bytes()).unwrap();
        let segments = build_segments(&xml, xml.paragraphs()[0].node);

        assert_eq!(all_text(&segments), "beforeafter");
        assert_eq!(
            segments
                .iter()
                .map(|s| (s.text.as_str(), s.start, s.end))
                .collect::<Vec<_>>(),
            [("before", 0, 6), ("after", 6, 11)]
        );
    }
}
