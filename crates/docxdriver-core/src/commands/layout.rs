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
    let node = xml.doc.create_element("w:pgMar");
    if let Some(successor) = xml.doc.children(sect).find(|&child| {
        [
            "pgBorders",
            "paperSrc",
            "lnNumType",
            "pgNumType",
            "cols",
            "formProtection",
            "vAlign",
            "noEndnote",
            "textDirection",
            "bidi",
            "rtlGutter",
            "docGrid",
            "printerSettings",
            "sectPrChange",
        ]
        .iter()
        .any(|local| xml.is_w(child, local))
    }) {
        xml.doc.insert_before(successor, node);
    } else {
        xml.doc.append_child(sect, node);
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
    let pg_mar = ensure_pg_mar(sect, xml);
    let mut applied = serde_json::Map::new();
    applied.insert("section".into(), json!(section));
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

pub(crate) fn ensure_ppr_child(xml: &mut DocxXml, ppr: NodeId, local: &str) -> NodeId {
    if let Some(node) = xml.child_w(ppr, local) {
        return node;
    }
    let successors: &[&str] = match local {
        "pStyle" => &["*"],
        "spacing" => &[
            "ind",
            "contextualSpacing",
            "mirrorIndents",
            "suppressOverlap",
            "jc",
            "textDirection",
            "textAlignment",
            "textboxTightWrap",
            "outlineLvl",
        ],
        "ind" => &[
            "contextualSpacing",
            "mirrorIndents",
            "suppressOverlap",
            "jc",
            "textDirection",
            "textAlignment",
            "textboxTightWrap",
            "outlineLvl",
        ],
        "jc" => &[
            "textDirection",
            "textAlignment",
            "textboxTightWrap",
            "outlineLvl",
        ],
        _ => &[],
    };
    let node = xml.doc.create_element(&format!("w:{local}"));
    let successor = if successors.contains(&"*") {
        xml.doc.children(ppr).next()
    } else {
        xml.doc
            .children(ppr)
            .find(|&child| successors.iter().any(|name| xml.is_w(child, name)))
    };
    if let Some(next) = successor {
        xml.doc.insert_before(next, node);
    } else {
        xml.doc.append_child(ppr, node);
    }
    node
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
