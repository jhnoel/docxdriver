//! Semantic reading metadata. HTML positions and presentation mechanics stay in the UI profile.
use crate::document::{DocxXml, ParaIdIndex};
use crate::segments::{build_segments, RevWrap};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use xmloxide::tree::NodeId;

pub(crate) fn selections(xml: &DocxXml, ids: &ParaIdIndex) -> Vec<Value> {
    xml.paragraphs()
        .iter()
        .filter_map(|p| {
            // Ordinary HTML paragraph text is already the selection text. Supply
            // exact source text when generated objects, revisions or boundaries
            // make stripping the displayed HTML ambiguous.
            let complex = xml.doc.descendants(p.node).any(|n| {
                xml.is_rev_ins(n)
                    || xml.is_rev_del(n)
                    || [
                        "oMath",
                        "oMathPara",
                        "drawing",
                        "pict",
                        "tab",
                        "br",
                        "footnoteReference",
                        "endnoteReference",
                        "fldChar",
                        "fldSimple",
                        "AlternateContent",
                    ]
                    .iter()
                    .any(|name| xml.is_local(n, name))
            });
            if !complex {
                return None;
            }
            let (id, _) = ids.ids.get(&p.node)?;
            let text: String = build_segments(xml, p.node)
                .into_iter()
                .filter(|s| s.wrap != RevWrap::Del)
                .map(|s| s.text)
                .collect();
            Some(json!({"at":id,"selectable_text":text}))
        })
        .collect()
}

/// Source ranges span paragraph boundaries; their selectors use the exact same
/// current-text segment space as comment_add, not HTML byte positions.
pub(crate) fn comment_anchors(xml: &DocxXml, ids: &ParaIdIndex) -> BTreeMap<String, Vec<Value>> {
    fn walk(
        xml: &DocxXml,
        node: NodeId,
        offset: &mut usize,
        active: &mut HashMap<String, usize>,
        spans: &mut Vec<(String, usize, usize)>,
    ) {
        for child in xml.doc.children(node) {
            if xml.is_w(child, "p") || xml.is_w(child, "pPr") {
                continue;
            }
            if xml.is_w(child, "commentRangeStart") {
                if let Some(id) = xml.attr(child, "id") {
                    active.insert(id.into(), *offset);
                }
            } else if xml.is_w(child, "commentRangeEnd") {
                if let Some(id) = xml.attr(child, "id") {
                    if let Some(start) = active.remove(id) {
                        spans.push((id.into(), start, *offset));
                    }
                }
            } else if xml.is_w(child, "t") || xml.is_w(child, "delText") {
                *offset += xml.doc.text_content(child).chars().count();
            } else if xml.is_local(child, "AlternateContent") {
                if let Some(branch) = xml.alternate_content_branch(child) {
                    walk(xml, branch, offset, active, spans);
                }
            } else {
                walk(xml, child, offset, active, spans);
            }
        }
    }
    let mut result: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut active = HashMap::new();
    for p in xml.paragraphs() {
        for start in active.values_mut() {
            *start = 0;
        }
        let mut offset = 0;
        let mut spans = Vec::new();
        walk(xml, p.node, &mut offset, &mut active, &mut spans);
        spans.extend(
            active
                .iter()
                .map(|(id, &start)| (id.clone(), start, offset)),
        );
        let Some((at, _)) = ids.ids.get(&p.node) else {
            continue;
        };
        let segments = build_segments(xml, p.node);
        let current: String = segments
            .iter()
            .filter(|s| s.wrap != RevWrap::Del)
            .map(|s| s.text.as_str())
            .collect();
        let to_current = |boundary: usize| {
            segments
                .iter()
                .filter(|s| s.wrap != RevWrap::Del)
                .map(|s| boundary.min(s.end).saturating_sub(s.start))
                .sum::<usize>()
        };
        let all: String = segments.iter().map(|s| s.text.as_str()).collect();
        for (id, all_start, all_end) in spans {
            let start = to_current(all_start);
            let end = to_current(all_end);
            let select: String = current.chars().skip(start).take(end - start).collect();
            let occurrence = if select.is_empty() {
                None
            } else {
                current
                    .match_indices(&select)
                    .position(|(byte, _)| current[..byte].chars().count() == start)
                    .map(|n| n + 1)
            };
            let mut range = json!({"at":at,"start":start,"end":end,"select":select,"occurrence":occurrence,"locator":format!("p{}:{all_start}-{all_end}",p.index)});
            let marked: String = all
                .chars()
                .skip(all_start)
                .take(all_end - all_start)
                .collect();
            if marked != select {
                range["marked_text"] = json!(marked);
            }
            result.entry(id).or_default().push(range);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anchors_keep_unicode_occurrences_and_cross_paragraph_context() {
        let xml = DocxXml::parse(r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"><w:body>
<w:p w14:paraId="11111111"><w:r><w:t>😀 target and </w:t></w:r><w:commentRangeStart w:id="0"/><w:r><w:t>target</w:t></w:r><w:commentRangeEnd w:id="0"/><w:commentRangeStart w:id="1"/><w:r><w:t> tail</w:t></w:r></w:p>
<w:p w14:paraId="22222222"><w:r><w:t>head</w:t></w:r><w:commentRangeEnd w:id="1"/><w:r><w:t> rest</w:t></w:r></w:p>
</w:body></w:document>"#.as_bytes()).unwrap();
        let ids = xml.resolve_paragraph_ids("word/document.xml", &[0; 32]);
        assert!(
            selections(&xml, &ids).is_empty(),
            "comment markers alone must not duplicate text"
        );
        let anchors = comment_anchors(&xml, &ids);
        assert_eq!(anchors["0"][0]["select"], "target");
        assert_eq!(anchors["0"][0]["occurrence"], 2);
        assert_eq!(anchors["0"][0]["start"], 13);
        assert_eq!(anchors["0"][0]["end"], 19);
        assert_eq!(anchors["1"].len(), 2);
        assert_eq!(anchors["1"][0]["select"], " tail");
        assert_eq!(anchors["1"][1]["select"], "head");
        assert_ne!(anchors["1"][0]["at"], anchors["1"][1]["at"]);
    }
}
