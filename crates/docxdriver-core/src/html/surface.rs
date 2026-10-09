//! Package-wide document projection: sections, headers, footers, and notes
//! interleaved with body content into one canonical HTML surface.

use std::collections::HashMap;

use xmloxide::tree::NodeId;

use crate::document::DocxXml;
use crate::package::Package;

use super::chrome::{chrome_map, find_sect_pr, ChromeSlot};
use super::ctx::RenderCtx;
use super::render::{render_block_list, render_blocks, BlockSpan, RenderView, RenderedBody};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceRegionKind {
    Body,
    Header,
    Footer,
    Section,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Region offsets are retained for future surface edits.
pub struct SurfaceRegion {
    pub kind: SurfaceRegionKind,
    pub start: usize,
    pub end: usize,
    pub part: Option<String>,
    pub section: Option<usize>,
    pub slot_kind: Option<String>,
    pub scope: Option<&'static str>,
    pub paragraphs: Vec<NodeId>,
}

#[derive(Debug)]
#[allow(dead_code)] // The projection also carries repair metadata for callers.
pub struct RenderedDocument {
    pub html: String,
    pub regions: Vec<SurfaceRegion>,
    /// Concatenated body-only HTML (no chrome/section markers) for callers that
    /// still need the body block list (e.g. block counts in renderHtml).
    pub body: RenderedBody,
    /// Total provisional paragraph-ID repairs across every rendered part
    /// (document, chrome, notes). Inspection reports it; a write persists it.
    pub repairs: usize,
}

/// Render the full package-wide projection (comments footer is appended by the
/// command layer — html must not depend on commands). `source_hash` is the
/// sha256 of the exact input package bytes; provisional paragraph IDs derive
/// from it so inspection, preview, and commit resolve identical addresses.
pub fn render_document(
    package: &Package,
    xml: &DocxXml,
    view: RenderView,
    source_hash: Option<&[u8; 32]>,
) -> RenderedDocument {
    let chrome = chrome_map(package, xml);
    let mut notes = NoteBodies::load(package, view, source_hash);
    assign_note_labels(xml, &mut notes, view);
    let ctx = RenderCtx::build_with_notes(package, xml, notes, source_hash);
    let mut repairs = ctx.repairs;

    let Some(body) = xml.body() else {
        return empty_doc();
    };

    let sections = partition_sections(xml, body);
    let mut html = String::new();
    let mut regions = Vec::new();
    let mut body_blocks = Vec::new();
    let mut body_html = String::new();
    // Body-level table numbers are document-wide so they match
    // `table:N` insert anchors (`body_table_insert_targets`).
    let mut table_n = 1u32;

    for (section_index, section) in sections.iter().enumerate() {
        let section_n = section_index + 1;
        let chrome_sect = chrome.sections.iter().find(|s| s.n == section_n);
        let title_page = chrome_sect.map(|s| s.title_page).unwrap_or(false);

        if !html.is_empty() {
            html.push('\n');
        }
        let start = html.len();
        html.push_str(&format!(
            "<section data-docx-section=\"{section_n}\"{}>",
            if title_page {
                " data-docx-first-page=\"true\""
            } else {
                ""
            }
        ));
        regions.push(SurfaceRegion {
            kind: SurfaceRegionKind::Section,
            start,
            end: html.len(),
            part: None,
            section: Some(section_n),
            slot_kind: None,
            scope: None,
            paragraphs: Vec::new(),
        });

        if let Some(sect) = chrome_sect {
            // Canonical order: header default/first/even, then footer default/first/even.
            for scope in ["hdr", "ftr"] {
                for kind in super::chrome::SLOT_ORDER {
                    if let Some(slot) = sect
                        .slots
                        .iter()
                        .find(|s| s.explicit && s.scope == scope && s.kind == *kind)
                    {
                        emit_chrome_slot(
                            package,
                            slot,
                            section_n,
                            view,
                            source_hash,
                            &mut html,
                            &mut regions,
                            &mut repairs,
                        );
                    }
                }
            }
        }

        if section.blocks.is_empty() {
            html.push_str("</section>");
            continue;
        }

        let rendered = render_block_list(xml, &ctx, view, &section.blocks, Some(&mut table_n));
        for block in &rendered.blocks {
            if !html.is_empty() {
                html.push('\n');
            }
            let start = html.len();
            let text = &rendered.html[block.start..block.end];
            html.push_str(text);
            regions.push(SurfaceRegion {
                kind: SurfaceRegionKind::Body,
                start,
                end: html.len(),
                part: Some(package.document_part_name()),
                section: Some(section_n),
                slot_kind: None,
                scope: None,
                paragraphs: block.paragraphs.clone(),
            });
            if !body_html.is_empty() {
                body_html.push('\n');
            }
            let body_start = body_html.len();
            body_html.push_str(text);
            body_blocks.push(BlockSpan {
                start: body_start,
                end: body_html.len(),
                paragraphs: block.paragraphs.clone(),
            });
        }
        html.push_str("\n</section>");
    }

    for (kind, notes) in [
        ("footnote", &ctx.notes.footnotes),
        ("endnote", &ctx.notes.endnotes),
    ] {
        let mut ids: Vec<_> = notes
            .keys()
            .filter(|id| html.contains(&format!("href=\"#{}\"", ctx.note_dom_id(kind, id))))
            .collect();
        ids.sort_by_key(|id| id.parse::<u32>().unwrap_or(0));
        if !ids.is_empty() {
            html.push_str(&format!("\n<aside data-docx-notes=\"{kind}\">"));
        }
        for id in &ids {
            let note = &notes[*id];
            html.push_str(&format!(
                "<div id=\"{}\" data-docx-note-body=\"{kind}\" data-docx-note-id=\"{}\">{}</div>",
                super::escape_attr(&ctx.note_dom_id(kind, id)),
                super::escape_attr(id),
                note.html
            ));
        }
        if !ids.is_empty() {
            html.push_str("</aside>");
        }
    }

    RenderedDocument {
        html,
        regions,
        repairs,
        body: RenderedBody {
            html: body_html,
            blocks: body_blocks,
        },
    }
}

fn empty_doc() -> RenderedDocument {
    RenderedDocument {
        html: String::new(),
        regions: Vec::new(),
        repairs: 0,
        body: RenderedBody {
            html: String::new(),
            blocks: Vec::new(),
        },
    }
}

struct SectionPart {
    blocks: Vec<NodeId>,
}

fn partition_sections(xml: &DocxXml, body: NodeId) -> Vec<SectionPart> {
    let mut children = Vec::new();
    collect_effective_section_children(xml, body, &mut children);
    let mut sections = Vec::new();
    let mut current = SectionPart { blocks: Vec::new() };

    for child in children {
        if xml.is_w(child, "sectPr") {
            sections.push(std::mem::replace(
                &mut current,
                SectionPart { blocks: Vec::new() },
            ));
            continue;
        }
        if xml.is_w(child, "p") || xml.is_w(child, "tbl") {
            current.blocks.push(child);
            if find_sect_pr(xml, child).is_some() {
                sections.push(std::mem::replace(
                    &mut current,
                    SectionPart { blocks: Vec::new() },
                ));
            }
        }
    }
    if !current.blocks.is_empty() || sections.is_empty() {
        sections.push(current);
    }
    if sections.len() > 1 && sections.last().is_some_and(|s| s.blocks.is_empty()) {
        sections.pop();
    }
    if sections.is_empty() {
        sections.push(SectionPart { blocks: Vec::new() });
    }
    sections
}

fn collect_effective_section_children(xml: &DocxXml, container: NodeId, out: &mut Vec<NodeId>) {
    out.extend(super::render::effective_story_children(
        xml,
        container,
        &["p", "tbl", "sectPr"],
    ));
}

fn emit_chrome_slot(
    package: &Package,
    slot: &ChromeSlot,
    section_n: usize,
    view: RenderView,
    source_hash: Option<&[u8; 32]>,
    html: &mut String,
    regions: &mut Vec<SurfaceRegion>,
    repairs: &mut usize,
) {
    let tag = if slot.scope == "hdr" {
        "header"
    } else {
        "footer"
    };
    let kind_attr = match slot.kind.as_str() {
        "first" => " data-docx-kind=\"first\"",
        "even" => " data-docx-kind=\"even\"",
        _ => "",
    };
    let region_kind = if slot.scope == "hdr" {
        SurfaceRegionKind::Header
    } else {
        SurfaceRegionKind::Footer
    };

    let (inner, paragraphs, blank, slot_repairs) =
        render_chrome_inner(package, &slot.part, view, source_hash);
    *repairs += slot_repairs;
    if !html.is_empty() {
        html.push('\n');
    }
    let start = html.len();
    if blank {
        html.push_str(&format!("<{tag}{kind_attr}></{tag}>"));
    } else {
        html.push_str(&format!("<{tag}{kind_attr}>\n{inner}\n</{tag}>"));
    }
    let end = html.len();
    regions.push(SurfaceRegion {
        kind: region_kind,
        start,
        end,
        part: Some(slot.part.clone()),
        section: Some(section_n),
        slot_kind: Some(slot.kind.clone()),
        scope: Some(slot.scope),
        paragraphs,
    });
}

fn render_chrome_inner(
    package: &Package,
    part: &str,
    view: RenderView,
    source_hash: Option<&[u8; 32]>,
) -> (String, Vec<NodeId>, bool, usize) {
    let Some(bytes) = package.get(part) else {
        return (String::new(), Vec::new(), true, 0);
    };
    let Ok(part_xml) = DocxXml::parse(bytes) else {
        return (String::new(), Vec::new(), true, 0);
    };
    let Some(root) = part_xml.doc.root_element() else {
        return (String::new(), Vec::new(), true, 0);
    };
    let ctx = RenderCtx::build_for_part(package, &part_xml, part, source_hash);
    let rendered = render_blocks(&part_xml, &ctx, view, root);
    let blank = is_blank_chrome(&rendered.html);
    let paragraphs: Vec<NodeId> = rendered
        .blocks
        .iter()
        .flat_map(|b| b.paragraphs.iter().copied())
        .collect();
    if blank {
        (String::new(), paragraphs, true, ctx.repairs)
    } else if rendered.html.is_empty() {
        ("<p></p>".into(), paragraphs, false, ctx.repairs)
    } else {
        (rendered.html, paragraphs, false, ctx.repairs)
    }
}

fn is_blank_chrome(html: &str) -> bool {
    let trimmed = html.trim();
    trimmed.is_empty() || trimmed == "<p></p>" || trimmed == "<p/>"
}

/// Footnote/endnote body cache keyed by note id.
#[derive(Debug, Clone, Default)]
pub struct NoteBodies {
    pub footnotes: HashMap<String, NoteBody>,
    pub endnotes: HashMap<String, NoteBody>,
}

#[derive(Debug, Clone)]
pub struct NoteBody {
    pub html: String,
    pub label: String,
}

impl NoteBodies {
    pub fn load(package: &Package, view: RenderView, source_hash: Option<&[u8; 32]>) -> Self {
        let mut out = NoteBodies::default();
        load_note_part(
            package,
            "word/footnotes.xml",
            "footnote",
            view,
            source_hash,
            &mut out.footnotes,
        );
        load_note_part(
            package,
            "word/endnotes.xml",
            "endnote",
            view,
            source_hash,
            &mut out.endnotes,
        );
        out
    }

    pub fn get(&self, endnote: bool, id: &str) -> Option<&NoteBody> {
        if endnote {
            self.endnotes.get(id)
        } else {
            self.footnotes.get(id)
        }
    }
}

fn load_note_part(
    package: &Package,
    part: &str,
    item_tag: &str,
    view: RenderView,
    source_hash: Option<&[u8; 32]>,
    map: &mut HashMap<String, NoteBody>,
) {
    let Some(bytes) = package.get(part) else {
        return;
    };
    let Ok(xml) = DocxXml::parse(bytes) else {
        return;
    };
    let Some(root) = xml.doc.root_element() else {
        return;
    };
    let ctx = RenderCtx::build_for_part(package, &xml, part, source_hash);
    for note in xml.doc.children(root).collect::<Vec<_>>() {
        if !xml.is_w(note, item_tag) || xml.attr(note, "type").is_some() {
            continue;
        }
        let id = xml.attr(note, "id").unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let rendered = render_blocks(&xml, &ctx, view, note);
        map.insert(
            id.clone(),
            NoteBody {
                html: rendered.html,
                label: id.clone(),
            },
        );
    }
}

/// Short form for a single plain paragraph; otherwise keep block dialect.
/// Note bodies are outside the typed mutation surface and their paragraph
/// ids are never persisted, so the engine-owned `id`/`ord` attributes are
/// stripped from every note paragraph before the short-form check and from
/// the block form alike — the public projection never carries them.
pub fn canonicalize_note_inner(rendered: &str) -> String {
    let trimmed = rendered.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let stripped = strip_para_address_attrs(trimmed);
    if let Some(inner) = stripped
        .strip_prefix("<p>")
        .and_then(|s| s.strip_suffix("</p>"))
    {
        if !inner.contains('<') && !stripped.contains('\n') {
            return inner.to_string();
        }
    }
    stripped
}

/// Remove ` id="…"` / ` ord="…"` from every paragraph opening tag in a
/// rendered fragment, keeping every other attribute and all content. Used
/// when an agent-authored fragment (which never carries engine-owned
/// addresses) is compared against a rendered fragment (which may).
pub fn strip_para_address_attrs(markup: &str) -> String {
    let mut out = String::with_capacity(markup.len());
    let mut rest = markup;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let after = &rest[lt + 1..];
        let name_end = after
            .find(|c: char| !(c.is_ascii_alphanumeric()))
            .unwrap_or(after.len());
        let name = &after[..name_end];
        let is_para_tag = name == "p"
            || (name.len() == 2
                && name.starts_with('h')
                && name
                    .as_bytes()
                    .get(1)
                    .is_some_and(|b| (b'1'..=b'6').contains(b)));
        if is_para_tag && after.as_bytes().get(name_end) == Some(&b' ') {
            let tag_end = after.find('>').map(|k| k + 1).unwrap_or(after.len());
            let tag = &after[..tag_end];
            out.push('<');
            out.push_str(&strip_address_attrs(tag));
            rest = &after[tag_end..];
        } else {
            out.push('<');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// Remove the engine-owned ` id="…"` and ` ord="…"` attributes from one
/// rendered opening tag, keeping every other attribute.
fn strip_address_attrs(tag: &str) -> String {
    strip_one_attr(&strip_one_attr(tag, "id"), "ord")
}

fn strip_one_attr(tag: &str, attr: &str) -> String {
    let needle = format!(" {attr}=\"");
    let mut out = String::with_capacity(tag.len());
    let mut rest = tag;
    loop {
        let Some(pos) = rest.find(&needle) else { break };
        out.push_str(&rest[..pos]);
        let after = &rest[pos + needle.len()..];
        match after.find('"') {
            Some(close) => rest = &after[close + 1..],
            None => {
                out.push_str(&rest[pos..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn assign_note_labels(xml: &DocxXml, notes: &mut NoteBodies, view: RenderView) {
    let mut foot = 0;
    let mut end = 0;
    let mut seen = std::collections::HashSet::new();
    for n in xml.doc.descendants(xml.doc.root()) {
        let endnote = xml.is_w(n, "endnoteReference");
        if !endnote && !xml.is_w(n, "footnoteReference") {
            continue;
        }
        let mut parent = xml.doc.parent(n);
        let mut hidden = false;
        while let Some(p) = parent {
            if (view == RenderView::Final && xml.is_rev_del(p))
                || (view == RenderView::Original && xml.is_rev_ins(p))
            {
                hidden = true;
                break;
            }
            parent = xml.doc.parent(p);
        }
        if hidden {
            continue;
        }
        let id = xml.attr(n, "id").unwrap_or("");
        if !seen.insert((endnote, id.to_string())) {
            continue;
        }
        let (counter, map) = if endnote {
            (&mut end, &mut notes.endnotes)
        } else {
            (&mut foot, &mut notes.footnotes)
        };
        *counter += 1;
        if let Some(note) = map.get_mut(id) {
            note.label = counter.to_string();
        }
    }
}
