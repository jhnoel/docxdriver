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

const CONTENT_TYPES_PART: &str = "[Content_Types].xml";
const DOCUMENT_RELS_PART: &str = "word/_rels/document.xml.rels";
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
    let mut rels = parse_part(package, DOCUMENT_RELS_PART, default)?;
    let root = rels
        .doc
        .root_element()
        .ok_or_else(|| Outcome::error("malformed document.xml.rels"))?;
    if let Some(existing) = rels.doc.children(root).find(|&child| {
        rels.is_local(child, "Relationship") && rels.doc.attribute(child, "Target") == Some(target)
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
    package.set(DOCUMENT_RELS_PART, rels.serialize());
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
    if xml.child_w(sect, "titlePg").is_none() {
        let node = xml.doc.create_element("w:titlePg");
        xml.doc.append_child(sect, node);
    }
}

fn set_chrome_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    scope: &str,
    section: usize,
    kind: ChromeKind,
    with: &str,
) -> Result<(String, Value), Outcome> {
    if section == 0 {
        return Err(Outcome::error("section must be >= 1"));
    }
    let kind_str = kind.as_type();
    let inner = inner_xml_from_with(with)?;
    let sect = ensure_sect_pr(xml, section)?;
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
        package.set(&part, chrome_part_xml(scope, &inner).into_bytes());
        return Ok((
            format!("section {section} {kind_str} {scope} updated"),
            json!({ "section": section, "kind": kind_str, "scope": scope, "part": part }),
        ));
    }

    let n = max_chrome_part_n(package, prefix) + 1;
    let part_name = format!("word/{prefix}{n}.xml");
    let target = format!("{prefix}{n}.xml");
    ensure_content_type(package, &part_name, ct)?;
    let rid = ensure_document_rel(package, &target, rel_type)?;
    ensure_r_ns(xml)?;
    package.set(&part_name, chrome_part_xml(scope, &inner).into_bytes());
    let ref_tag = chrome_ref_tag(scope);
    let ref_node = xml.doc.create_element(&format!("w:{ref_tag}"));
    xml.doc.set_attribute(ref_node, "w:type", kind_str);
    xml.doc.set_attribute(ref_node, "r:id", &rid);
    xml.doc.append_child(sect, ref_node);

    Ok((
        format!("section {section} {kind_str} {scope} set"),
        json!({ "section": section, "kind": kind_str, "scope": scope, "part": part_name }),
    ))
}

fn clear_chrome_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    scope: &str,
    section: usize,
    kind: ChromeKind,
) -> Result<(String, Value), Outcome> {
    if section == 0 {
        return Err(Outcome::error("section must be >= 1"));
    }
    let kind_str = kind.as_type();
    let sect = ensure_sect_pr(xml, section)?;
    let Some(ref_node) = find_explicit_chrome_ref(xml, sect, scope, kind_str) else {
        return Err(Outcome::blocked(format!(
            "section {section} has no explicit {kind_str} {scope} to clear"
        )));
    };
    xml.doc.detach(ref_node);
    let _ = package;
    Ok((
        format!("section {section} {kind_str} {scope} cleared"),
        json!({ "section": section, "kind": kind_str, "scope": scope }),
    ))
}

pub fn set_header_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    section: usize,
    kind: ChromeKind,
    with: &str,
) -> Result<(String, Value), Outcome> {
    set_chrome_typed(package, xml, "hdr", section, kind, with)
}

pub fn set_footer_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    section: usize,
    kind: ChromeKind,
    with: &str,
) -> Result<(String, Value), Outcome> {
    set_chrome_typed(package, xml, "ftr", section, kind, with)
}

pub fn clear_header_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    section: usize,
    kind: ChromeKind,
) -> Result<(String, Value), Outcome> {
    clear_chrome_typed(package, xml, "hdr", section, kind)
}

pub fn clear_footer_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    section: usize,
    kind: ChromeKind,
) -> Result<(String, Value), Outcome> {
    clear_chrome_typed(package, xml, "ftr", section, kind)
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
            settings.doc.append_child(root, node);
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
