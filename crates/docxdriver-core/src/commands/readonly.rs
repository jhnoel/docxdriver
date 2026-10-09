//! create + the read-only command set.

use serde_json::{json, Value};

use crate::document::{escape_xml_text, DocxXml, View, W_NS};
use crate::outcome::Outcome;
use crate::package::{Package, DOCUMENT_PART};

use super::ExecCtx;

fn view_of(command: &Value) -> View {
    View::from_json(command.get("view"))
}

/// Paragraph JSON entries for typed read/find: index, text, style,
/// `id` (the canonical eight-digit `w14:paraId` address), and (when the
/// paragraph carries a computed list label) `number` — the rendered label on
/// the same paragraph (see
/// `html::ctx::list_markers`). Building the whole `RenderCtx` here (not just
/// the marker table) keeps paragraph identity and list labels consistent with
/// rendered document blocks.
pub fn paragraph_entries(
    package: &Package,
    xml: &DocxXml,
    view: View,
    source_hash: Option<&[u8; 32]>,
) -> (Vec<Value>, usize) {
    let ctx = crate::html::ctx::RenderCtx::build_with_source(package, xml, source_hash);
    let repairs = ctx.repairs;
    let entries = xml
        .paragraphs()
        .iter()
        .map(|paragraph| {
            let mut entry = serde_json::Map::new();
            entry.insert("index".into(), json!(paragraph.index));
            if let Some((id, _)) = ctx.para_ids.get(&paragraph.node) {
                entry.insert("id".into(), json!(id));
            }
            entry.insert(
                "text".into(),
                json!(xml.paragraph_text(paragraph.node, view)),
            );
            entry.insert(
                "selectable_text".into(),
                json!(xml.paragraph_text(paragraph.node, View::Current)),
            );
            let equations = super::equations::inspect(xml, paragraph.node, ctx.math_limits);
            if !equations.is_empty() {
                entry.insert("equations".into(), json!(equations));
            }
            if let Some(style) = xml.paragraph_style(paragraph.node) {
                entry.insert("style".into(), json!(style));
            }
            if let Some(number) = ctx.markers.get(&paragraph.node) {
                entry.insert("number".into(), json!(number));
            }
            Value::Object(entry)
        })
        .collect();
    (entries, repairs)
}

/// The inspection envelope: exact input package hash (`source`) and the
/// provisional paragraph-ID repair count.
pub fn inspection_header(source_hash: Option<&[u8; 32]>, repairs: usize) -> Value {
    let mut result = serde_json::Map::new();
    result.insert("idRepairsNeeded".into(), json!(repairs));
    if let Some(hash) = source_hash {
        result.insert(
            "source".into(),
            json!(format!("sha256:{}", hex::encode(hash))),
        );
    }
    Value::Object(result)
}

pub fn plural(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}

pub fn read(
    package: &Package,
    xml: &DocxXml,
    command: &Value,
    source_hash: Option<&[u8; 32]>,
) -> Outcome {
    let (paragraphs, repairs) = paragraph_entries(package, xml, view_of(command), source_hash);
    // Document inspection discards this field before returning; a corrupt
    // comments part must not fail the whole document read.
    let comments = super::comments::comment_threads(package)
        .ok()
        .unwrap_or_default();
    let mut result = inspection_header(source_hash, repairs);
    if let Value::Object(map) = &mut result {
        map.insert("paragraphs".into(), json!(paragraphs));
        map.insert("comments".into(), json!(comments));
    }
    let summary = format!(
        "read {}",
        plural(paragraphs.len(), "paragraph", "paragraphs")
    );
    Outcome::ok(summary, result)
}

pub fn find_text(
    package: &Package,
    xml: &DocxXml,
    command: &Value,
    source_hash: Option<&[u8; 32]>,
) -> Outcome {
    let Some(query) = command.get("query").and_then(Value::as_str) else {
        return Outcome::error("find requires a query");
    };
    let ignore_case = command
        .get("ignoreCase")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let needle = if ignore_case {
        query.to_lowercase()
    } else {
        query.to_string()
    };
    let matches: Vec<Value> = paragraph_entries(package, xml, view_of(command), source_hash)
        .0
        .into_iter()
        .filter(|entry| {
            let text = entry
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let haystack = if ignore_case {
                text.to_lowercase()
            } else {
                text.to_string()
            };
            haystack.contains(&needle)
        })
        .collect();
    Outcome::ok(
        format!("matches {}", matches.len()),
        json!({ "matches": matches }),
    )
}

/// `listStyles`: catalog entries from the package `styles.xml` part (paragraph,
/// character, table, numbering). Documents without a styles part return an
/// empty list — Word synthesizes defaults; we do not invent them here.
pub fn list_styles(package: &Package) -> Outcome {
    let styles_part = styles_part_name(package);
    let Some(bytes) = package.get(&styles_part) else {
        return Outcome::ok("styles 0", json!({ "styles": [] }));
    };
    let styles_xml = match DocxXml::parse(bytes) {
        Ok(xml) => xml,
        Err(error) => return Outcome::error(format!("styles.xml parse failed: {error}")),
    };
    let root = styles_xml.doc.root();
    let mut styles = Vec::new();
    for node in styles_xml.doc.descendants(root).collect::<Vec<_>>() {
        if !styles_xml.is_w(node, "style") {
            continue;
        }
        let Some(style_id) = styles_xml.attr(node, "styleId") else {
            continue;
        };
        let style_type = styles_xml.attr(node, "type").unwrap_or("paragraph");
        let mut entry = serde_json::Map::new();
        entry.insert("styleId".into(), json!(style_id));
        entry.insert("type".into(), json!(style_type));
        if styles_xml.attr(node, "default") == Some("1") {
            entry.insert("default".into(), json!(true));
        }
        for child in styles_xml.doc.children(node).collect::<Vec<_>>() {
            if styles_xml.is_w(child, "name") {
                if let Some(name) = styles_xml.attr(child, "val") {
                    entry.insert("name".into(), json!(name));
                }
            } else if styles_xml.is_w(child, "basedOn") {
                if let Some(based) = styles_xml.attr(child, "val") {
                    entry.insert("basedOn".into(), json!(based));
                }
            } else if styles_xml.is_w(child, "next") {
                if let Some(next) = styles_xml.attr(child, "val") {
                    entry.insert("next".into(), json!(next));
                }
            }
        }
        styles.push(Value::Object(entry));
    }
    Outcome::ok(
        format!("styles {}", plural(styles.len(), "style", "styles")),
        json!({ "styles": styles }),
    )
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

pub fn create(command: &Value, ctx: &ExecCtx) -> Outcome {
    let (package, summary, result) = match build_package(command) {
        Ok(built) => built,
        Err(outcome) => return outcome,
    };
    if ctx.dry_run {
        return Outcome::ok(summary, result);
    }
    match package.to_bytes() {
        Ok(bytes) => Outcome::ok_bytes(summary, result, bytes),
        Err(error) => Outcome::error(error.to_string()),
    }
}

/// The package-building half of `create`, shared with the ops-list runner
/// (which continues editing the parts in memory instead of zipping here).
/// Two input forms: `paragraphs` (array of plain strings) or `html` (the
/// dialect — headings, b/i/u, br, tabs, simple tables). The html form also
/// ships a minimal styles.xml so headings look like headings; the paragraphs
/// form keeps its historical byte-identical output.
pub fn build_package(command: &Value) -> Result<(Package, String, Value), Outcome> {
    let mut package = Package::from_bytes(include_bytes!("../../assets/blank.docx"))
        .map_err(|error| Outcome::error(format!("blank document template: {error}")))?;
    let template_document = String::from_utf8_lossy(
        package
            .document_xml()
            .map_err(|error| Outcome::error(format!("blank document template: {error}")))?,
    )
    .into_owned();
    let template_sect_pr = final_section_properties(&template_document);
    let html = command.get("html").and_then(Value::as_str);
    if html.is_some() && command.get("paragraphs").is_some() {
        return Err(Outcome::error("create takes paragraphs or html, not both"));
    }
    let (mut body, para_count, with_styles, chrome, deferred) = match html {
        Some(html) => {
            let (body, count, chrome, deferred) =
                crate::html::build::body_and_chrome_from_html(html)
                    .map_err(|error| Outcome::error(format!("create html: {error}")))?;
            (body, count, true, chrome, deferred)
        }
        None => {
            let Some(paragraphs) = command.get("paragraphs").and_then(Value::as_array) else {
                return Err(Outcome::error("create requires paragraphs or html"));
            };
            let texts: Vec<&str> = paragraphs.iter().filter_map(Value::as_str).collect();
            if texts.len() != paragraphs.len() {
                return Err(Outcome::error("create paragraphs must be strings"));
            }
            if texts.iter().any(|text| text.contains(['\n', '\r'])) {
                return Err(Outcome::error(
                    "create paragraphs must not contain newlines — pass one array entry per paragraph",
                ));
            }
            let body: String = texts
                .iter()
                .map(|text| {
                    format!(
                        "<w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                        escape_xml_text(text)
                    )
                })
                .collect();
            (
                body,
                texts.len(),
                false,
                None,
                crate::html::build::CreateDeferred::default(),
            )
        }
    };

    let r_ns = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let mut overrides = String::new();
    let mut rels = String::new();
    if !deferred.lists.is_empty() {
        let mut numbering = String::new();
        for (index, list) in deferred.lists.iter().enumerate() {
            let id = index + 1;
            numbering.push_str(&format!(
                "<w:abstractNum w:abstractNumId=\"{id}\"><w:multiLevelType w:val=\"multilevel\"/>"
            ));
            for level in 0..9 {
                let text = if list.format == "bullet" {
                    "•".into()
                } else {
                    format!("%{}.", level + 1)
                };
                numbering.push_str(&format!("<w:lvl w:ilvl=\"{level}\"><w:start w:val=\"{}\"/><w:numFmt w:val=\"{}\"/><w:lvlText w:val=\"{text}\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>",list.start,list.format,(level+1)*720));
            }
            numbering.push_str(&format!(
                "</w:abstractNum><w:num w:numId=\"{id}\"><w:abstractNumId w:val=\"{id}\"/></w:num>"
            ));
        }
        package.set(
            "word/numbering.xml",
            format!("<w:numbering xmlns:w=\"{W_NS}\">{numbering}</w:numbering>").into_bytes(),
        );
        overrides.push_str("<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>");
        rels.push_str("<Relationship Id=\"rIdNumbering\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>");
    }
    // rId1–rId5 belong to the embedded template (styles, settings, font
    // table, theme, and web settings). New parts must not collide with them.
    let mut next_rid = 6u64;
    let mut chrome_parts: Vec<(String, String)> = Vec::new(); // (part path, xml)
    let mut need_r_ns = !deferred.is_empty();
    let mut settings_xml: Option<String> = None;
    let mut hdr_n = 0u32;
    let mut ftr_n = 0u32;
    let mut media_parts: Vec<(String, Vec<u8>)> = Vec::new();
    let mut content_type_defaults: Vec<(String, String)> = Vec::new(); // ext, mime

    // Allocate hyperlink + image relationships after styles (if any).
    const HYPERLINK_REL: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
    const IMAGE_REL: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
    for (i, href) in deferred.hyperlinks.iter().enumerate() {
        let rid = format!("rId{next_rid}");
        next_rid += 1;
        rels.push_str(&format!(
            "<Relationship Id=\"{rid}\" Type=\"{HYPERLINK_REL}\" Target=\"{}\" TargetMode=\"External\"/>",
            crate::html::escape_attr(href)
        ));
        let ph = crate::html::build::hyperlink_placeholder(i);
        body = body.replace(&ph, &rid);
    }
    for (i, (mime, bytes)) in deferred.images.iter().enumerate() {
        let rid = format!("rId{next_rid}");
        next_rid += 1;
        let ext = match mime.as_str() {
            "image/png" => "png",
            "image/jpeg" | "image/jpg" => "jpeg",
            "image/gif" => "gif",
            "image/bmp" => "bmp",
            "image/webp" => "webp",
            "image/svg+xml" => "svg",
            _ => "bin",
        };
        let part = format!("word/media/image{}.{ext}", i + 1);
        let target = part.strip_prefix("word/").unwrap_or(&part);
        rels.push_str(&format!(
            "<Relationship Id=\"{rid}\" Type=\"{IMAGE_REL}\" Target=\"{target}\"/>"
        ));
        if !content_type_defaults.iter().any(|(e, _)| e == ext) {
            content_type_defaults.push((ext.to_string(), mime.clone()));
        }
        media_parts.push((part, bytes.clone()));
        let ph = crate::html::build::image_placeholder(i);
        body = body.replace(&ph, &rid);
    }

    if let Some(chrome) = &chrome {
        need_r_ns = need_r_ns
            || !chrome.slots.is_empty()
            || chrome.extra_sections.iter().any(|s| !s.slots.is_empty());

        let emit_slots = |slots: &[crate::html::build::CreateChromeSlot],
                          title_page: bool,
                          hdr_n: &mut u32,
                          ftr_n: &mut u32,
                          next_rid: &mut u64,
                          overrides: &mut String,
                          rels: &mut String,
                          chrome_parts: &mut Vec<(String, String)>|
         -> String {
            let mut sect_pr = String::new();
            for slot in slots {
                let (part_name, root, ct, rel_type) = if slot.scope == "hdr" {
                    *hdr_n += 1;
                    (
                            format!("word/header{hdr_n}.xml"),
                            "hdr",
                            "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml",
                            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header",
                        )
                } else {
                    *ftr_n += 1;
                    (
                            format!("word/footer{ftr_n}.xml"),
                            "ftr",
                            "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml",
                            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer",
                        )
                };
                let target = part_name.strip_prefix("word/").unwrap_or(&part_name);
                let rid = format!("rId{next_rid}");
                *next_rid += 1;
                overrides.push_str(&format!(
                    "<Override PartName=\"/{part_name}\" ContentType=\"{ct}\"/>"
                ));
                rels.push_str(&format!(
                    "<Relationship Id=\"{rid}\" Type=\"{rel_type}\" Target=\"{target}\"/>"
                ));
                let part_xml = format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:{root} xmlns:w=\"{W_NS}\">{}</w:{root}>",
                        slot.inner_xml
                    );
                chrome_parts.push((part_name, part_xml));
                let ref_tag = if slot.scope == "hdr" {
                    "headerReference"
                } else {
                    "footerReference"
                };
                sect_pr.push_str(&format!(
                    "<w:{ref_tag} w:type=\"{}\" r:id=\"{rid}\"/>",
                    slot.kind
                ));
            }
            if title_page {
                sect_pr.push_str("<w:titlePg/>");
            }
            sect_pr
        };

        // Section 0 props: either fill <!--SECT0--> mid-marker or final sectPr.
        let sect0 = emit_slots(
            &chrome.slots,
            chrome.title_page,
            &mut hdr_n,
            &mut ftr_n,
            &mut next_rid,
            &mut overrides,
            &mut rels,
            &mut chrome_parts,
        );
        if body.contains("<!--SECT0-->") {
            body = body.replace("<!--SECT0-->", &sect0);
        }

        for (i, extra) in chrome.extra_sections.iter().enumerate() {
            let marker = format!("<!--SECT{}-->", i + 1);
            let sect_i = emit_slots(
                &extra.slots,
                extra.title_page,
                &mut hdr_n,
                &mut ftr_n,
                &mut next_rid,
                &mut overrides,
                &mut rels,
                &mut chrome_parts,
            );
            if body.contains(&marker) {
                body = body.replace(&marker, &sect_i);
            } else if i + 1 == chrome.extra_sections.len() {
                // Last section: final body-level sectPr.
                if !sect_i.is_empty() || !extra.slots.is_empty() || extra.title_page {
                    body.push_str(&format!("<w:sectPr>{sect_i}</w:sectPr>"));
                }
            }
        }

        // Single-section create: append final sectPr when no mid markers.
        if chrome.extra_sections.is_empty() && !body.contains("<!--SECT") {
            if !sect0.is_empty() {
                body.push_str(&format!("<w:sectPr>{sect0}</w:sectPr>"));
            }
        }

        if chrome.even_odd {
            let template_settings = package.get("word/settings.xml").ok_or_else(|| {
                Outcome::error("blank document template is missing word/settings.xml")
            })?;
            let settings = String::from_utf8_lossy(template_settings);
            settings_xml = Some(append_before_close(
                &settings,
                "</w:settings>",
                "<w:evenAndOddHeaders/>",
            ));
        }
    }
    let _ = next_rid;
    let _ = (hdr_n, ftr_n);

    let xmlns_r = if need_r_ns {
        format!(" xmlns:r=\"{r_ns}\"")
    } else {
        String::new()
    };
    if chrome.is_none() && !body.contains("<w:sectPr") {
        if let Some(sect_pr) = &template_sect_pr {
            body.push_str(sect_pr);
        }
    }
    let document_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"{W_NS}\"{xmlns_r}><w:body>{body}</w:body></w:document>"
    );

    let defaults: String = content_type_defaults
        .iter()
        .map(|(ext, mime)| format!("<Default Extension=\"{ext}\" ContentType=\"{mime}\"/>"))
        .collect();
    let content_types =
        String::from_utf8_lossy(package.get("[Content_Types].xml").ok_or_else(|| {
            Outcome::error("blank document template is missing [Content_Types].xml")
        })?);
    package.set(
        "[Content_Types].xml",
        append_before_close(
            &content_types,
            "</Types>",
            &format!("{defaults}{overrides}"),
        )
        .into_bytes(),
    );
    package.set(DOCUMENT_PART, document_xml.into_bytes());
    let document_rels =
        String::from_utf8_lossy(package.get("word/_rels/document.xml.rels").ok_or_else(|| {
            Outcome::error("blank document template is missing document relationships")
        })?);
    package.set(
        "word/_rels/document.xml.rels",
        append_before_close(&document_rels, "</Relationships>", &rels).into_bytes(),
    );
    if with_styles {
        let template_styles =
            String::from_utf8_lossy(package.get("word/styles.xml").ok_or_else(|| {
                Outcome::error("blank document template is missing word/styles.xml")
            })?);
        package.set(
            "word/styles.xml",
            append_before_close(&template_styles, "</w:styles>", &minimal_heading_styles())
                .into_bytes(),
        );
    }
    for (name, xml) in &chrome_parts {
        package.set(name, xml.as_bytes().to_vec());
    }
    for (name, bytes) in media_parts {
        package.set(&name, bytes);
    }
    if let Some(settings) = settings_xml {
        package.set("word/settings.xml", settings.into_bytes());
    }

    // Every engine-created part is born fully addressed: each paragraph gets
    // a fresh `w14:paraId` and the root declares the w14 namespace (plus w14
    // in mc:Ignorable). The per-document seed derives from the document's own
    // bytes, so identical input creates byte-identical output while different
    // documents diverge (and the occupied set keeps every id unique). The
    // typed address surface is word/document.xml only: created chrome parts
    // (headers/footers) stay outside it and carry no ids.
    let document_part = package.document_part_name();
    address_created_part(&mut package, &document_part)?;

    let result = json!({ "paragraphs": para_count });
    let summary = format!("created {}", plural(para_count, "paragraph", "paragraphs"));
    Ok((package, summary, result))
}

/// Assign a fresh `w14:paraId` to every paragraph of a freshly created part
/// and declare the w14 namespace on its root. Existing valid IDs are
/// untouched; the per-document seed derives from the part bytes.
fn address_created_part(package: &mut Package, part_name: &str) -> Result<(), Outcome> {
    let bytes = package
        .get(part_name)
        .ok_or_else(|| Outcome::error(format!("created part missing: {part_name}")))?
        .to_vec();
    let mut xml =
        DocxXml::parse(&bytes).map_err(|error| Outcome::error(format!("{part_name}: {error}")))?;
    let seed: [u8; 32] = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        hasher.finalize().into()
    };
    let index = xml.resolve_paragraph_ids(part_name, &seed);
    xml.apply_paragraph_ids(&index).map_err(Outcome::error)?;
    package.set(part_name, xml.serialize());
    Ok(())
}

/// Return the body-level section properties from the embedded blank document.
/// They carry Word's default page geometry and remain in every document created
/// without explicit chrome.
fn final_section_properties(document_xml: &str) -> Option<String> {
    let start = document_xml.rfind("<w:sectPr")?;
    let end = document_xml[start..].find("</w:sectPr>")? + start + "</w:sectPr>".len();
    Some(document_xml[start..end].to_string())
}

fn append_before_close(xml: &str, closing: &str, addition: &str) -> String {
    let Some(index) = xml.rfind(closing) else {
        return xml.to_string();
    };
    let mut out = String::with_capacity(xml.len() + addition.len());
    out.push_str(&xml[..index]);
    out.push_str(addition);
    out.push_str(&xml[index..]);
    out
}

/// Heading styles missing from the embedded blank template. They are appended
/// to its richer Word-generated styles part instead of replacing it.
fn minimal_heading_styles() -> String {
    let sizes = [48, 36, 32, 28, 26, 24];
    let headings: String = sizes
        .iter()
        .enumerate()
        .map(|(i, size)| {
            let level = i + 1;
            format!(
                "<w:style w:type=\"paragraph\" w:styleId=\"Heading{level}\"><w:name w:val=\"heading {level}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"120\"/><w:outlineLvl w:val=\"{i}\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/></w:rPr></w:style>"
            )
        })
        .collect();
    headings
}
