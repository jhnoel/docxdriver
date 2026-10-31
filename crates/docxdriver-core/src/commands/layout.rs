//! Typed page-margin and paragraph-property primitives.

use serde_json::{json, Value};
use xmloxide::tree::NodeId;

use crate::document::DocxXml;
use crate::html::chrome::sect_pr_nodes;
use crate::outcome::Outcome;
use crate::package::Package;

pub(crate) fn ensure_sect_pr(xml: &mut DocxXml, section: usize) -> Result<NodeId, Outcome> {
    let nodes = sect_pr_nodes(xml);
    if let Some(&node) = nodes.get(section - 1) {
        return Ok(node);
    }
    if section == 1 && nodes.is_empty() {
        let body = xml
            .body()
            .ok_or_else(|| Outcome::error("document has no w:body"))?;
        let sect = xml.doc.create_element("w:sectPr");
        xml.doc.append_child(body, sect);
        return Ok(sect);
    }
    Err(Outcome::error(format!(
        "section {section} not found (document has {} sections)",
        nodes.len()
    )))
}

fn ensure_pg_mar(sect: NodeId, xml: &mut DocxXml) -> NodeId {
    if let Some(node) = xml.child_w(sect, "pgMar") {
        return node;
    }
    // CT_PageMar requires all seven attributes; default to Word margins.
    let node = ensure_sect_pr_child(xml, sect, "pgMar");
    for (attr, twips) in [("top", 1440), ("right", 1440), ("bottom", 1440), ("left", 1440), ("header", 720), ("footer", 720), ("gutter", 0)] {
        set_twips_attr(xml, node, attr, twips);
    }
    node
}

fn set_twips_attr(xml: &mut DocxXml, node: NodeId, attr: &str, value: i64) {
    set_attr(xml, node, &format!("w:{attr}"), &value.to_string());
}

pub(crate) fn set_attr(xml: &mut DocxXml, node: NodeId, name: &str, value: &str) {
    xml.doc.remove_attribute(node, name);
    if let Some((_, local)) = name.split_once(':') {
        xml.doc.remove_attribute(node, local);
    }
    xml.doc.set_attribute(node, name, value);
}

pub fn set_page_margins_typed(
    _package: &mut Package,
    xml: &mut DocxXml,
    section: usize,
    top: Option<i64>,
    right: Option<i64>,
    bottom: Option<i64>,
    left: Option<i64>,
    header: Option<i64>,
    footer: Option<i64>,
    gutter: Option<i64>,
    tracked: bool,
    author: &str,
    ctx: &super::ExecCtx,
) -> Result<(String, Value), Outcome> {
    if section == 0 {
        return Err(Outcome::error("section must be >= 1"));
    }
    if [top, right, bottom, left, header, footer, gutter]
        .into_iter()
        .all(|value| value.is_none())
    {
        return Err(Outcome::error("setPageMargins requires at least one margin field (top/right/bottom/left/header/footer/gutter, twips)"));
    }
    let sect = ensure_sect_pr(xml, section)?;
    // Tracked, the old margins go into a section property change, as Word
    // records them.
    if tracked {
        snapshot_section_properties(xml, sect, author, ctx)?;
    }
    let pg_mar = ensure_pg_mar(sect, xml);
    let mut applied = serde_json::Map::new();
    applied.insert("section".into(), json!(section));
    applied.insert("tracked".into(), json!(tracked));
    for (key, value) in [
        ("top", top),
        ("right", right),
        ("bottom", bottom),
        ("left", left),
        ("header", header),
        ("footer", footer),
        ("gutter", gutter),
    ] {
        if let Some(value) = value {
            set_twips_attr(xml, pg_mar, key, value);
            applied.insert(key.into(), json!(value));
        }
    }
    Ok((
        format!("section {section} page margins updated"),
        Value::Object(applied),
    ))
}

pub(crate) fn ensure_ppr(xml: &mut DocxXml, paragraph: NodeId) -> NodeId {
    if let Some(ppr) = xml.child_w(paragraph, "pPr") {
        return ppr;
    }
    let ppr = xml.doc.create_element("w:pPr");
    xml.doc.prepend_child(paragraph, ppr);
    ppr
}

/// `CT_PPr` child sequence (`CT_PPrBase` followed by `rPr`, `sectPr`,
/// `pPrChange`).
const PPR_SEQUENCE: &[&str] = &[
    "pStyle",
    "keepNext",
    "keepLines",
    "pageBreakBefore",
    "framePr",
    "widowControl",
    "numPr",
    "suppressLineNumbers",
    "pBdr",
    "shd",
    "tabs",
    "suppressAutoHyphens",
    "kinsoku",
    "wordWrap",
    "overflowPunct",
    "topLinePunct",
    "autoSpaceDE",
    "autoSpaceDN",
    "bidi",
    "adjustRightInd",
    "snapToGrid",
    "spacing",
    "ind",
    "contextualSpacing",
    "mirrorIndents",
    "suppressOverlap",
    "jc",
    "textDirection",
    "textAlignment",
    "textboxTightWrap",
    "outlineLvl",
    "divId",
    "cnfStyle",
    "rPr",
    "sectPr",
    "pPrChange",
];

/// Return the `w:pPr` child `local`, creating it at its schema sequence
/// position.
pub(crate) fn ensure_ppr_child(xml: &mut DocxXml, ppr: NodeId, local: &str) -> NodeId {
    ensure_property_child(xml, ppr, local, PPR_SEQUENCE)
}

/// Run-property order. ECMA-376 Transitional relaxes EG_RPrBase to a repeated
/// choice, but Word writes, and the Open XML SDK validates, this sequence
/// (the CT_ParaRPr revision marks lead; rPrChange trails). Existing children
/// are never reordered; new ones go before their first present successor.
pub(crate) const RPR_SEQUENCE: &[&str] = &[
    "ins", "del", "moveFrom", "moveTo", "rStyle", "rFonts", "b", "bCs", "i", "iCs", "caps",
    "smallCaps", "strike", "dstrike", "outline", "shadow", "emboss", "imprint", "noProof",
    "snapToGrid", "vanish", "webHidden", "color", "spacing", "w", "kern", "position", "sz",
    "szCs", "highlight", "u", "effect", "bdr", "shd", "fitText", "vertAlign", "rtl", "cs", "em",
    "lang", "eastAsianLayout", "specVanish", "oMath", "rPrChange",
];

/// Return the `w:rPr` child `local`, creating it at its sequence position.
pub(crate) fn ensure_rpr_child(xml: &mut DocxXml, rpr: NodeId, local: &str) -> NodeId {
    ensure_property_child(xml, rpr, local, RPR_SEQUENCE)
}

/// Sequence rank of a run property, for builders that emit markup as text.
pub(crate) fn rpr_rank(local: &str) -> usize {
    RPR_SEQUENCE.iter().position(|name| *name == local).unwrap_or(RPR_SEQUENCE.len())
}

fn ensure_property_child(
    xml: &mut DocxXml,
    parent: NodeId,
    local: &str,
    sequence: &[&str],
) -> NodeId {
    if let Some(node) = xml.child_w(parent, local) {
        return node;
    }
    let node = xml.doc.create_element(&format!("w:{local}"));
    let successors = sequence
        .iter()
        .position(|name| *name == local)
        .map_or(&[][..], |index| &sequence[index + 1..]);
    // Office extension elements (w14 text effects and the like) close the
    // base sequence, before only the trailing revision record.
    let trailing = sequence.last() == Some(&local);
    let successor = xml.doc.children(parent).find(|&child| {
        successors.iter().any(|name| xml.is_w(child, name))
            || (!trailing && xml.doc.is_element(child) && is_extension(xml, child))
    });
    if let Some(next) = successor {
        xml.doc.insert_before(next, node);
    } else {
        xml.doc.append_child(parent, node);
    }
    node
}

/// An element outside WordprocessingML: an Office extension, or the markup
/// compatibility wrapper that carries one.
fn is_extension(xml: &DocxXml, node: NodeId) -> bool {
    let ns = xml.doc.node_namespace(node);
    ns.is_some() && ns != Some(crate::document::W_NS)
}

pub(crate) fn set_child_val(xml: &mut DocxXml, parent: NodeId, local: &str, val: &str) {
    let node = if matches!(local, "pStyle" | "jc") {
        ensure_ppr_child(xml, parent, local)
    } else if let Some(node) = xml.child_w(parent, local) {
        node
    } else {
        let node = xml.doc.create_element(&format!("w:{local}"));
        xml.doc.append_child(parent, node);
        node
    };
    set_attr(xml, node, "w:val", val);
}

fn styles_part_name(package: &Package) -> String {
    let main = package.document_part_name();
    let (dir, _) = main.rsplit_once('/').unwrap_or(("", main.as_str()));
    if dir.is_empty() {
        "styles.xml".into()
    } else {
        format!("{dir}/styles.xml")
    }
}

pub(crate) fn paragraph_style_exists(package: &Package, style_id: &str) -> bool {
    let part = styles_part_name(package);
    let Some(bytes) = package.get(&part) else {
        return false;
    };
    let Ok(styles_xml) = DocxXml::parse(bytes) else {
        return false;
    };
    let root = styles_xml.doc.root();
    styles_xml.doc.descendants(root).any(|node| {
        styles_xml.is_w(node, "style")
            && styles_xml.attr(node, "styleId") == Some(style_id)
            && styles_xml.attr(node, "type").unwrap_or("paragraph") == "paragraph"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_insertion_respects_snapshot_tail_and_preserves_existing_siblings() {
        let mut xml = DocxXml::parse(br#"<w:rPr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:highlight w:val="yellow"/><w:rPrChange w:id="1" w:author="test"><w:rPr/></w:rPrChange></w:rPr>"#).unwrap();
        let rpr = xml.doc.root_element().unwrap();
        let children: Vec<_> = xml.doc.children(rpr).collect();
        let color = ensure_rpr_child(&mut xml, rpr, "color");
        assert_eq!(
            xml.doc.children(rpr).collect::<Vec<_>>(),
            vec![color, children[0], children[1]]
        );
        let lang = ensure_rpr_child(&mut xml, rpr, "lang");
        assert_eq!(
            xml.doc.children(rpr).collect::<Vec<_>>(),
            vec![color, children[0], lang, children[1]]
        );
        assert_eq!(ensure_rpr_child(&mut xml, rpr, "color"), color);
        assert_eq!(xml.attr(children[0], "val"), Some("yellow"));
    }
}

const SECT_PR_SEQUENCE: &[&str] = &[
    "headerReference", "footerReference", "footnotePr", "endnotePr", "type", "pgSz",
    "pgMar", "paperSrc", "pgBorders", "lnNumType", "pgNumType", "cols", "formProt",
    "vAlign", "noEndnote", "titlePg", "textDirection", "bidi", "rtlGutter", "docGrid",
    "printerSettings", "sectPrChange",
];

pub(crate) fn ensure_sect_pr_child(xml: &mut DocxXml, sect: NodeId, local: &str) -> NodeId {
    ensure_property_child(xml, sect, local, SECT_PR_SEQUENCE)
}

/// Section property snapshots exclude story relationships (CT_SectPrBase).
/// Existing stories remain independently editable and survive accept/reject.
pub(crate) fn snapshot_section_properties(
    xml: &mut DocxXml, sect: NodeId, author: &str, ctx: &super::ExecCtx,
) -> Result<(), Outcome> {
    // The author's own pending change keeps its snapshot, as Word folds
    // one author's successive changes into one record.
    if let Some(change) = xml.child_w(sect, "sectPrChange") {
        if xml.attr(change, "author") == Some(author) {
            return Ok(());
        }
        return Err(Outcome::blocked("section already has a pending property change by another author; accept or reject it first"));
    }
    let snapshot = xml.doc.create_element("w:sectPr");
    for child in xml.doc.children(sect).collect::<Vec<_>>() {
        if xml.is_w(child, "headerReference") || xml.is_w(child, "footerReference") { continue; }
        let copy = xml.doc.clone_node(child, true);
        xml.doc.append_child(snapshot, copy);
    }
    let id = xml.next_revision_id();
    let change = xml.doc.create_element("w:sectPrChange");
    set_attr(xml, change, "w:id", &id.to_string());
    set_attr(xml, change, "w:author", author);
    set_attr(xml, change, "w:date", &ctx.now);
    xml.doc.append_child(change, snapshot);
    xml.doc.append_child(sect, change);
    Ok(())
}

pub fn set_first_page_mode(
    xml: &mut DocxXml, section: usize, enabled: bool, tracked: bool,
    author: &str, ctx: &super::ExecCtx,
) -> Result<(String, Value), Outcome> {
    let sect = ensure_sect_pr(xml, section)?;
    if xml.child_w(sect, "sectPrChange").is_some_and(|change| !tracked || xml.attr(change, "author") != Some(author)) {
        return Err(Outcome::blocked("section already has a pending property change; accept or reject it first"));
    }
    let current = xml.child_w(sect, "titlePg").is_some_and(|node| {
        !matches!(xml.attr(node, "val"), Some("0" | "false" | "off"))
    });
    if current == enabled {
        return Ok((format!("section {section} first-page mode already matches"), json!({"section": section, "changed": false})));
    }
    if tracked { snapshot_section_properties(xml, sect, author, ctx)?; }
    let node = ensure_sect_pr_child(xml, sect, "titlePg");
    set_attr(xml, node, "w:val", if enabled { "1" } else { "0" });
    Ok((format!("section {section} first-page mode updated"), json!({"section": section, "enabled": enabled, "tracked": tracked, "changed": true})))
}

pub fn set_section_layout(
    xml: &mut DocxXml, section: usize, op: &crate::api::EditOp, tracked: bool,
    author: &str, ctx: &super::ExecCtx,
) -> Result<(String, Value), Outcome> {
    let crate::api::EditOp::SetSectionLayout { page_size, width, height, orientation,
        columns, column_spacing, column_widths, column_separator, break_type,
        page_number_format, page_number_start, page_number_continue, .. } = op else { unreachable!() };
    let sect = ensure_sect_pr(xml, section)?;
    if xml.child_w(sect, "sectPrChange").is_some_and(|change| !tracked || xml.attr(change, "author") != Some(author)) {
        return Err(Outcome::blocked("section already has a pending property change; accept or reject it first"));
    }
    let dimension = |tag: &str, attr: &str, default: i64| {
        xml.child_w(sect, tag).and_then(|n| xml.attr(n, attr)).and_then(|v| v.parse::<i64>().ok()).unwrap_or(default)
    };
    let mut w = dimension("pgSz", "w", 12240);
    let mut h = dimension("pgSz", "h", 15840);
    if let Some(size) = page_size {
        (w, h) = match size.as_str() {
            "letter" => (12240, 15840), "legal" => (12240, 20160),
            "a4" => (11906, 16838), "a3" => (16838, 23811), "a5" => (8391, 11906), _ => unreachable!(),
        };
    }
    if let (Some(width), Some(height)) = (width, height) { w = width.0; h = height.0; }
    let current_orientation = xml.child_w(sect, "pgSz").and_then(|n| xml.attr(n, "orient")).unwrap_or("portrait");
    let orient = orientation.as_deref().unwrap_or(current_orientation).to_owned();
    if page_size.is_some() || width.is_some() || orientation.is_some() {
        if (orient == "landscape" && w < h) || (orient == "portrait" && w > h) { std::mem::swap(&mut w, &mut h); }
    }
    let area = w.saturating_sub(dimension("pgMar", "left", 1440)).saturating_sub(dimension("pgMar", "right", 1440)).saturating_sub(dimension("pgMar", "gutter", 0));
    let cols = xml.child_w(sect, "cols");
    let old_widths: Vec<i64> = cols.map(|n| xml.doc.children(n).filter(|&n| xml.is_w(n, "col")).filter_map(|n| xml.attr(n, "w").and_then(|v| v.parse().ok())).collect()).unwrap_or_default();
    let count = columns.map(|v| v as i64).or_else(|| column_widths.as_ref().map(|v| v.len() as i64)).unwrap_or_else(|| dimension("cols", "num", if old_widths.is_empty() { 1 } else { old_widths.len() as i64 }));
    let gap = column_spacing.map(|p| p.0).unwrap_or_else(|| dimension("cols", "space", 720));
    let original_gaps = cols.map(|n| xml.doc.children(n).filter(|&n| xml.is_w(n, "col")).collect::<Vec<_>>()).unwrap_or_default();
    let inherited_gap_total = original_gaps.iter().take(original_gaps.len().saturating_sub(1)).fold(0_i64, |total, &n| total.saturating_add(xml.attr(n, "space").and_then(|v| v.parse::<i64>().ok()).unwrap_or(gap)));
    let gap_total = if column_spacing.is_none() && column_widths.is_none() && columns.is_none() && !old_widths.is_empty() { inherited_gap_total } else { gap.saturating_mul(count.saturating_sub(1)) };
    let unequal: Vec<i64> = column_widths.as_ref().map(|v| v.iter().map(|p| p.0).collect()).unwrap_or_else(|| old_widths.clone());
    let changes_columns = columns.is_some() || column_spacing.is_some() || column_widths.is_some() || column_separator.is_some();
    if changes_columns || width.is_some() || orientation.is_some() || page_size.is_some() {
        if area <= 0 || gap_total >= area { return Err(Outcome::blocked("columns do not fit the section text area")); }
        if !unequal.is_empty() && columns.is_none() && (unequal.iter().fold(0_i64, |total, width| total.saturating_add(*width)).saturating_add(gap_total) > area) {
            return Err(Outcome::blocked("column widths and spacing exceed the section text area"));
        }
        if let Some(widths) = column_widths { if widths.iter().fold(0_i64, |total, p| total.saturating_add(p.0)).saturating_add(gap_total) > area { return Err(Outcome::blocked("column widths and spacing exceed the section text area")); } }
    }
    if tracked { snapshot_section_properties(xml, sect, author, ctx)?; }
    if page_size.is_some() || width.is_some() || orientation.is_some() {
        let node = ensure_sect_pr_child(xml, sect, "pgSz");
        set_twips_attr(xml, node, "w", w); set_twips_attr(xml, node, "h", h);
        set_attr(xml, node, "w:orient", &orient);
    }
    if changes_columns {
        let node = ensure_sect_pr_child(xml, sect, "cols");
        if columns.is_some() || column_widths.is_some() { set_attr(xml, node, "w:num", &count.to_string()); }
        if let Some(spacing) = column_spacing { set_twips_attr(xml, node, "space", spacing.0); }
        if let Some(separator) = column_separator { set_attr(xml, node, "w:sep", if *separator { "1" } else { "0" }); }
        if column_widths.is_some() || columns.is_some() {
            for child in xml.doc.children(node).collect::<Vec<_>>() { if xml.is_w(child, "col") { xml.doc.remove_node(child); } }
            set_attr(xml, node, "w:equalWidth", if column_widths.is_some() { "0" } else { "1" });
        }
        if column_spacing.is_some() && column_widths.is_none() && columns.is_none() {
            let children: Vec<_> = xml.doc.children(node).filter(|&n| xml.is_w(n, "col")).collect();
            for (index, &col) in children.iter().enumerate() { set_twips_attr(xml, col, "space", if index + 1 == children.len() { 0 } else { gap }); }
        }
        if let Some(widths) = column_widths {
            for (index, width) in widths.iter().enumerate() {
                let col = xml.doc.create_element("w:col");
                set_twips_attr(xml, col, "w", width.0);
                set_twips_attr(xml, col, "space", if index + 1 == widths.len() { 0 } else { gap });
                xml.doc.append_child(node, col);
            }
        }
    }
    if let Some(kind) = break_type { let node = ensure_sect_pr_child(xml, sect, "type"); set_attr(xml, node, "w:val", kind); }
    if page_number_format.is_some() || page_number_start.is_some() || page_number_continue == &Some(true) {
        let node = ensure_sect_pr_child(xml, sect, "pgNumType");
        if let Some(format) = page_number_format { set_attr(xml, node, "w:fmt", format); }
        if let Some(start) = page_number_start { set_attr(xml, node, "w:start", &start.to_string()); }
        if page_number_continue == &Some(true) { xml.doc.remove_attribute(node, "w:start"); xml.doc.remove_attribute(node, "start"); }
    }
    Ok((format!("section {section} layout updated"), json!({"section":section,"tracked":tracked})))
}
