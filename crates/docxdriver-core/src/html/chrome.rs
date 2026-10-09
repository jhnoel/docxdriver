//! Header/footer package inventory and section walks.
//!
//! Sections are enumerated by walking `w:body` children for `w:sectPr`
//! boundaries. Word links each of six chains independently (`hdr`/`ftr` ×
//! `default`/`first`/`even`): an omitted reference inherits the previous
//! section's same type; the first section starts blank; an explicit unresolved
//! relationship clears that chain. The public projection (see `surface.rs`)
//! emits only **explicit** relationships — omission means inheritance.

use xmloxide::tree::NodeId;

use crate::document::DocxXml;
use crate::media::{relationship_targets, resolve_rel_target};
use crate::package::Package;

pub const SLOT_ORDER: &[&str] = &["default", "first", "even"];

/// One header or footer reference on a section.
#[derive(Debug, Clone)]
pub struct ChromeSlot {
    pub scope: &'static str, // "hdr" | "ftr"
    pub kind: String,        // default | first | even
    pub part: String,
    /// True when this section's sectPr carries an explicit reference (not
    /// merely inherited from a prior section). The projection emits only
    /// explicit slots; omission means Word inheritance.
    pub explicit: bool,
}

/// One section's chrome: its sectPr facts plus resolved slots.
#[derive(Debug, Clone)]
pub struct ChromeSection {
    pub n: usize,
    pub title_page: bool,
    pub slots: Vec<ChromeSlot>,
}

/// Package-wide chrome inventory used by render and scoped edit.
#[derive(Debug, Clone)]
pub struct ChromeMap {
    pub sections: Vec<ChromeSection>,
}

/// Walk body sections and resolve header/footer relationships.
///
/// Six independent carry-forward chains (`hdr`/`ftr` × `default`/`first`/
/// `even`) mirror Word's "Link to Previous" behaviour: omitted refs inherit
/// the same type from the prior section; section 1 starts blank; an explicit
/// reference that does not resolve to a package part clears that chain.
pub fn chrome_map(package: &Package, xml: &DocxXml) -> ChromeMap {
    let rels = relationship_targets(package);
    let document_part = package.document_part_name();
    let mut sections = Vec::new();
    let Some(body) = xml.body() else {
        return ChromeMap { sections };
    };

    // [hdr|ftr][default|first|even] — independent of titlePg / evenAndOddHeaders.
    let mut chains: [[Option<String>; 3]; 2] = Default::default();

    let mut n = 0usize;
    for child in xml.doc.children(body).collect::<Vec<_>>() {
        let sect_pr = if xml.is_w(child, "p") || xml.is_w(child, "tbl") {
            find_sect_pr(xml, child)
        } else if xml.is_w(child, "sectPr") {
            Some(child)
        } else {
            None
        };
        let Some(sect_pr) = sect_pr else { continue };
        n += 1;
        let title_page = xml.doc.children(sect_pr).any(|c| xml.is_w(c, "titlePg"));
        let mut slots = Vec::new();
        // Slot order: hdr default/first/even, then ftr default/first/even.
        for (scope_i, (scope, tag)) in [("hdr", "headerReference"), ("ftr", "footerReference")]
            .into_iter()
            .enumerate()
        {
            for (kind_i, kind) in SLOT_ORDER.iter().enumerate() {
                let mut explicit = false;
                if let Some(node) = xml.doc.children(sect_pr).find(|&c| {
                    xml.is_w(c, tag) && xml.attr(c, "type").unwrap_or("default") == *kind
                }) {
                    explicit = true;
                    // Explicit ref: valid part replaces the chain; unresolved clears it.
                    let part = xml
                        .attr(node, "id")
                        .and_then(|rid| rels.get(rid))
                        .map(|target| resolve_rel_target(&document_part, target))
                        .filter(|part| package.get(part).is_some());
                    chains[scope_i][kind_i] = part;
                }
                // Omitted ref: leave the chain unchanged (inherit prior section).
                // Inventory keeps inherited slots for shared-part detection;
                // the projection filters on `explicit`.
                if let Some(part) = chains[scope_i][kind_i].clone() {
                    slots.push(ChromeSlot {
                        scope,
                        kind: (*kind).to_string(),
                        part,
                        explicit,
                    });
                }
            }
        }
        sections.push(ChromeSection {
            n,
            title_page,
            slots,
        });
    }

    ChromeMap { sections }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::api::ChromeKind;
    use crate::commands::headers::set_header_typed;
    use crate::commands::WorkState;

    #[test]
    fn chrome_map_sees_explicit_header_after_set_header() {
        let body = concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\">",
            "<w:body><w:p w14:paraId=\"11111111\"><w:r><w:t>text</w:t></w:r></w:p></w:body></w:document>",
        );
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, content) in [
            ("[Content_Types].xml", b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"/>" as &[u8]),
            ("word/document.xml", body.as_bytes()),
            ("word/_rels/document.xml.rels", b"<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>"),
        ] {
            writer.start_file(name, options).unwrap();
            writer.write_all(content).unwrap();
        }
        let bytes = writer.finish().unwrap().into_inner();
        let mut state = WorkState::parse(&bytes).unwrap();
        set_header_typed(
            &mut state.package,
            &mut state.xml,
            1,
            ChromeKind::Default,
            "<p>Acme Inc</p>",
        )
        .unwrap();
        let map = chrome_map(&state.package, &state.xml);
        assert_eq!(map.sections.len(), 1, "sections: {:?}", map.sections);
        let hdr = map.sections[0]
            .slots
            .iter()
            .find(|s| s.scope == "hdr" && s.kind == "default")
            .expect("default header slot");
        assert!(hdr.explicit, "header slot should be explicit");
        assert!(state.package.get(&hdr.part).is_some(), "part missing");
        let rendered = crate::html::surface::render_document(
            &state.package,
            &state.xml,
            crate::html::render::RenderView::Markup,
            None,
        );
        assert!(
            rendered.html.contains("<header>") && rendered.html.contains("Acme Inc"),
            "html={}",
            rendered.html
        );
        let out = crate::commands::emit(
            crate::commands::WorkState {
                mutated: true,
                ..state
            },
            crate::outcome::Outcome::ok("ok", serde_json::json!({})),
            &crate::commands::ExecCtx {
                dry_run: false,
                now: "2000-01-01T00:00:00Z".into(),
            },
        );
        let bytes = out.bytes.expect("bytes");
        let state = WorkState::parse(&bytes).unwrap();
        let rendered = crate::html::surface::render_document(
            &state.package,
            &state.xml,
            crate::html::render::RenderView::Markup,
            state.source_hash.as_ref(),
        );
        assert!(
            rendered.html.contains("<header>") && rendered.html.contains("Acme Inc"),
            "after emit html={}",
            rendered.html
        );
    }
}

/// Every header/footer part path referenced by the document (deduped).
pub fn chrome_part_names(package: &Package, xml: &DocxXml) -> Vec<String> {
    let map = chrome_map(package, xml);
    let mut parts = Vec::new();
    for sect in &map.sections {
        for slot in &sect.slots {
            if !parts.contains(&slot.part) {
                parts.push(slot.part.clone());
            }
        }
    }
    parts
}

/// Locate `w:sectPr` owned by a body block (`w:p` / `w:tbl`), if any.
pub fn find_sect_pr(xml: &DocxXml, container: NodeId) -> Option<NodeId> {
    // Prefer the conventional pPr/sectPr on a paragraph; fall back to any
    // descendant (tables can nest a section-ending paragraph).
    if xml.is_w(container, "p") {
        if let Some(ppr) = xml.child_w(container, "pPr") {
            if let Some(sect) = xml.child_w(ppr, "sectPr") {
                return Some(sect);
            }
        }
    }
    xml.doc
        .descendants(container)
        .find(|&n| xml.is_w(n, "sectPr"))
}

/// Every `w:sectPr` in body order (mid-body on blocks, then final body child).
pub fn sect_pr_nodes(xml: &DocxXml) -> Vec<NodeId> {
    let Some(body) = xml.body() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for child in xml.doc.children(body).collect::<Vec<_>>() {
        if xml.is_w(child, "p") || xml.is_w(child, "tbl") {
            if let Some(sect) = find_sect_pr(xml, child) {
                out.push(sect);
            }
        } else if xml.is_w(child, "sectPr") {
            out.push(child);
        }
    }
    out
}

/// Settings part path next to the main document (usually `word/settings.xml`).

/// Ensure an external hyperlink relationship exists in the document part.
pub(crate) fn ensure_external_hyperlink(
    package: &mut Package,
    href: &str,
) -> Result<String, String> {
    const RELS: &str = "word/_rels/document.xml.rels";
    const HYPERLINK_REL: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
    let default = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>",
    );
    let bytes = package
        .get(RELS)
        .map(|b| b.to_vec())
        .unwrap_or_else(|| default.as_bytes().to_vec());
    let mut rels = DocxXml::parse(&bytes).map_err(|e| e)?;
    let root = rels
        .doc
        .root_element()
        .ok_or("malformed document.xml.rels")?;
    for child in rels.doc.children(root) {
        if rels.is_local(child, "Relationship")
            && rels.doc.attribute(child, "Type") == Some(HYPERLINK_REL)
            && rels.doc.attribute(child, "Target") == Some(href)
        {
            return Ok(rels
                .doc
                .attribute(child, "Id")
                .unwrap_or("rId1")
                .to_string());
        }
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
    rels.doc.set_attribute(node, "Type", HYPERLINK_REL);
    rels.doc.set_attribute(node, "Target", href);
    rels.doc.set_attribute(node, "TargetMode", "External");
    rels.doc.append_child(root, node);
    package.set(RELS, rels.serialize());
    Ok(id)
}

pub(crate) fn wrap_revision_in_hyperlink(
    xml: &mut DocxXml,
    paragraph: NodeId,
    rev_id: u64,
    rid: &str,
) -> Result<(), String> {
    const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let Some(root) = xml.doc.root_element() else {
        return Err("document has no root element".into());
    };
    let has_r = xml
        .doc
        .attributes(root)
        .iter()
        .any(|a| a.name == "xmlns:r" || (a.name == "r" && a.value.contains("relationships")));
    if !has_r {
        xml.doc.set_attribute(root, "xmlns:r", R_NS);
    }
    let target = xml
        .doc
        .descendants(paragraph)
        .find(|&n| {
            xml.is_rev_ins(n)
                && xml.attr(n, "id").and_then(|v| v.parse::<u64>().ok()) == Some(rev_id)
        })
        .ok_or_else(|| format!("inserted revision {rev_id} not found for hyperlink wrap"))?;
    let mut node = target;
    while let Some(parent) = xml.doc.parent(node) {
        if xml.is_w(parent, "hyperlink") {
            return Ok(());
        }
        if xml.is_w(parent, "p") {
            break;
        }
        node = parent;
    }
    let link = xml.doc.create_element("w:hyperlink");
    xml.doc.set_attribute(link, "r:id", rid);
    xml.doc.insert_before(target, link);
    xml.doc.detach(target);
    xml.doc.append_child(link, target);
    Ok(())
}
