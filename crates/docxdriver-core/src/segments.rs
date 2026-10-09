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
pub fn split_boundary(xml: &mut DocxXml, paragraph: NodeId, offset: usize) {
    let segments = build_segments(xml, paragraph);
    let Some(segment) = segments.iter().find(|s| s.start < offset && offset < s.end) else {
        return;
    };
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
                if fmt.bold {
                    let b = xml.doc.create_element("w:b");
                    xml.doc.append_child(rpr, b);
                }
                if fmt.italic {
                    let i = xml.doc.create_element("w:i");
                    xml.doc.append_child(rpr, i);
                }
                if fmt.underline {
                    let u = xml.doc.create_element("w:u");
                    xml.doc.set_attribute(u, "w:val", "single");
                    xml.doc.append_child(rpr, u);
                }
                if fmt.strike {
                    let s = xml.doc.create_element("w:strike");
                    xml.doc.append_child(rpr, s);
                }
                if let Some(align) = fmt.vert_align {
                    let vert = xml.doc.create_element("w:vertAlign");
                    xml.doc.set_attribute(vert, "w:val", align.docx_val());
                    xml.doc.append_child(rpr, vert);
                }
                crate::html::styles::append_extra_rpr(xml, rpr, fmt);
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
