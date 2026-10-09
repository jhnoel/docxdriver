//! Embedded pictures: extraction of image references from run content
//! (`w:drawing` DrawingML, `w:pict`/`w:object` VML, `mc:AlternateContent`
//! wrappers) and resolution of their relationship targets to package parts.
//!
//! The renderer emits each site as `<image w="…" h="…" alt="…"/>`
//! (package rid is recovered from the XML when editing); `listMedia` resolves the
//! referenced parts to bytes so a host can display them. Both go through this
//! module so "what counts as an image" has one definition.

use xmloxide::tree::NodeId;

use crate::document::DocxXml;
use crate::package::Package;

/// One image site in run content: the relationship id plus display metadata.
pub struct ImageInfo {
    pub rid: String,
    /// Pixel dimensions (EMU/9525 for DrawingML, pt·96/72 for VML), when stated.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Alt text: `wp:docPr` descr (fallback name) or the VML shape's alt/title.
    pub alt: Option<String>,
}

/// Is this run child a container that can hold an embedded picture?
pub fn is_image_container(xml: &DocxXml, node: NodeId) -> bool {
    xml.is_w(node, "drawing")
        || xml.is_w(node, "pict")
        || xml.is_w(node, "object")
        || xml.is_local(node, "AlternateContent")
}

/// Extract the image reference from a container, if it holds one. A drawing
/// with no blip (a chart, a shape) is None — those still render as nothing.
pub fn image_info(xml: &DocxXml, container: NodeId) -> Option<ImageInfo> {
    // mc:AlternateContent: use the Fallback branch (interoperable VML of the
    // same picture). Unsupported Choice content is not effective content.
    let container = if xml.is_local(container, "AlternateContent") {
        xml.alternate_content_branch(container)?
    } else {
        container
    };

    let mut rid: Option<String> = None;
    let mut vml_shape: Option<NodeId> = None;
    for node in xml.doc.descendants(container).collect::<Vec<_>>() {
        if !xml.doc.is_element(node) {
            continue;
        }
        if xml.is_local(node, "blip") {
            // a:blip — r:embed for an embedded part, r:link for a linked one.
            if let Some(embed) = xml.attr(node, "embed").or_else(|| xml.attr(node, "link")) {
                rid = Some(embed.to_string());
                break;
            }
        }
        if xml.is_local(node, "imagedata") {
            // v:imagedata r:id, inside a v:shape whose style carries the size.
            if let Some(id) = xml.attr(node, "id") {
                rid = Some(id.to_string());
                vml_shape = xml.doc.parent(node);
                break;
            }
        }
    }
    let rid = rid?;

    let mut width = None;
    let mut height = None;
    let mut alt = None;

    // DrawingML: wp:extent cx/cy in EMU; wp:docPr descr/name for alt text.
    for node in xml.doc.descendants(container).collect::<Vec<_>>() {
        if !xml.doc.is_element(node) {
            continue;
        }
        if xml.is_local(node, "extent") && width.is_none() {
            width = xml.attr(node, "cx").and_then(emu_to_px);
            height = xml.attr(node, "cy").and_then(emu_to_px);
        }
        if xml.is_local(node, "docPr") && alt.is_none() {
            alt = xml
                .attr(node, "descr")
                .filter(|s| !s.is_empty())
                .or_else(|| xml.attr(node, "name").filter(|s| !s.is_empty()))
                .map(str::to_string);
        }
    }

    // VML: the shape's style string ("width:451.3pt;height:284.1pt").
    if let Some(shape) = vml_shape {
        if let Some(style) = xml.attr(shape, "style") {
            if width.is_none() {
                width = style_dimension_px(style, "width");
                height = style_dimension_px(style, "height");
            }
        }
        if alt.is_none() {
            alt = xml
                .attr(shape, "alt")
                .filter(|s| !s.is_empty())
                .or_else(|| xml.attr(shape, "title").filter(|s| !s.is_empty()))
                .map(str::to_string);
        }
    }

    Some(ImageInfo {
        rid,
        width,
        height,
        alt,
    })
}

fn emu_to_px(value: &str) -> Option<u32> {
    let emu: f64 = value.parse().ok()?;
    if !(emu > 0.0) {
        return None;
    }
    Some((emu / 9525.0).round().max(1.0) as u32)
}

/// Parse one `name:123.4pt` (or px/in/cm/mm) entry out of a VML style string.
fn style_dimension_px(style: &str, name: &str) -> Option<u32> {
    for entry in style.split(';') {
        let (key, value) = entry.split_once(':')?;
        if key.trim() != name {
            continue;
        }
        let value = value.trim();
        let split = value
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(value.len());
        let number: f64 = value[..split].parse().ok()?;
        let px = match &value[split..] {
            "pt" => number * 96.0 / 72.0,
            "in" => number * 96.0,
            "cm" => number * 96.0 / 2.54,
            "mm" => number * 96.0 / 25.4,
            "" | "px" => number,
            _ => return None,
        };
        if !(px > 0.0) {
            return None;
        }
        return Some(px.round().max(1.0) as u32);
    }
    None
}

/// Every image relationship id referenced from the document body, in document
/// order, deduplicated (the same part can be placed at several sites).
#[allow(dead_code)] // kept for call sites / tests that want body-only rids
pub fn document_image_rids(xml: &DocxXml) -> Vec<String> {
    let Some(body) = xml.body() else {
        return Vec::new();
    };
    image_rids_in(xml, body)
}

/// Image relationship ids under any container (body, hdr, ftr), document order,
/// deduplicated within that container.
pub fn image_rids_in(xml: &DocxXml, container: NodeId) -> Vec<String> {
    let mut rids = Vec::new();
    collect_image_rids(xml, container, &mut rids);
    rids
}

fn collect_image_rids(xml: &DocxXml, container: NodeId, rids: &mut Vec<String>) {
    for node in xml.doc.children(container).collect::<Vec<_>>() {
        if !xml.doc.is_element(node) {
            continue;
        }
        if xml.is_local(node, "AlternateContent") {
            if let Some(branch) = xml.alternate_content_branch(node) {
                collect_image_rids(xml, branch, rids);
            }
            continue;
        }
        if is_image_container(xml, node) {
            if let Some(info) = image_info(xml, node) {
                if !rids.contains(&info.rid) {
                    rids.push(info.rid);
                }
            }
            continue;
        }
        collect_image_rids(xml, node, rids);
    }
}

/// Resolve a relationship target (relative to the document part's directory,
/// e.g. "media/image1.png" → "word/media/image1.png") to a package part name.
pub fn resolve_rel_target(document_part: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_string();
    }
    let dir = document_part.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut segments: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for segment in target.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    segments.join("/")
}

/// Relationship id → target for the document part's rels.
pub fn relationship_targets(package: &Package) -> std::collections::HashMap<String, String> {
    let mut rels = std::collections::HashMap::new();
    let main = package.document_part_name();
    let (dir, file) = main.rsplit_once('/').unwrap_or(("", main.as_str()));
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    let rels_part = format!("{prefix}_rels/{file}.rels");
    if let Some(bytes) = package.get(&rels_part) {
        if let Ok(rels_xml) = DocxXml::parse(bytes) {
            let root = rels_xml.doc.root();
            for node in rels_xml.doc.descendants(root).collect::<Vec<_>>() {
                if rels_xml.is_local(node, "Relationship") {
                    if let (Some(id), Some(target)) =
                        (rels_xml.attr(node, "Id"), rels_xml.attr(node, "Target"))
                    {
                        rels.insert(id.to_string(), target.to_string());
                    }
                }
            }
        }
    }
    rels
}

/// Decode a standard base64 string (with optional whitespace/padding).
pub fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let cleaned: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    if cleaned.len() % 4 == 1 {
        return Err("invalid base64 length".into());
    }
    let mut out = Vec::with_capacity(cleaned.len() * 3 / 4);
    for chunk in cleaned.chunks(4) {
        let mut n = 0u32;
        let mut count = 0u32;
        for &c in chunk {
            let v = val(c).ok_or_else(|| format!("invalid base64 byte {c}"))?;
            n = (n << 6) | u32::from(v);
            count += 1;
        }
        n <<= 6 * (4 - count);
        if count >= 2 {
            out.push((n >> 16) as u8);
        }
        if count >= 3 {
            out.push((n >> 8) as u8);
        }
        if count >= 4 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

/// Parse `data:<mime>;base64,<payload>` into mime + bytes.
pub fn parse_data_url(src: &str) -> Result<(String, Vec<u8>), String> {
    let rest = src
        .strip_prefix("data:")
        .ok_or("image src must be a data: URL")?;
    let (meta, payload) = rest
        .split_once(',')
        .ok_or("data: URL missing comma before payload")?;
    if !meta.ends_with(";base64") {
        return Err("data: URL must use ;base64 encoding".into());
    }
    let mime = meta.trim_end_matches(";base64").to_string();
    if mime.is_empty() {
        return Err("data: URL missing mime type".into());
    }
    let bytes = base64_decode(payload)?;
    Ok((mime, bytes))
}
