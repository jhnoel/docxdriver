//! create from dialect HTML: parse a document authored in the same HTML
//! dialect the render emits and compile it to body XML. Creation is where the
//! HTML surface is loss-free by construction — the document is born from the
//! HTML, so there is nothing to round-trip.
//!
//! Supports headings, b/i/u, breaks, tabs, tables, `<header>`/`<footer>`,
//! multi-section `<section/>` markers, `<a href>`, `<equation>`, and
//! `<image src="data:…"/>`. Revision markup is rejected (a new document has
//! no history). Fields remain protected (not authored at create).

use crate::document::escape_xml_text;
use crate::segments::InsPiece;

use super::parse::parse_dialect;
use super::{escape_attr, Atom, Fmt, Item};

/// One table under construction: rows of cells, each cell the XML of its
/// paragraphs.
struct TableBuilder {
    rows: Vec<Vec<Cell>>,
    in_row: bool,
    cell: Option<Cell>,
}

struct Cell {
    xml: String,
    colspan: u32,
    rowspan: u32,
}

/// Deferred package allocations from create HTML (applied when building the
/// package so rIds land after styles/chrome relationships).
#[derive(Debug, Clone, Default)]
pub struct CreateDeferred {
    /// External hyperlink targets, in placeholder order (`__HL{n}__`).
    pub hyperlinks: Vec<String>,
    /// Image payloads `(mime, bytes)` in placeholder order (`__IMG{n}__`).
    pub images: Vec<(String, Vec<u8>)>,
    pub lists: Vec<super::ListAttrs>,
}

impl CreateDeferred {
    pub fn is_empty(&self) -> bool {
        self.hyperlinks.is_empty() && self.images.is_empty() && self.lists.is_empty()
    }

    fn hyperlink_idx(&mut self, href: &str) -> usize {
        if let Some(i) = self.hyperlinks.iter().position(|h| h == href) {
            return i;
        }
        self.hyperlinks.push(href.to_string());
        self.hyperlinks.len() - 1
    }

    fn image_idx(&mut self, mime: String, bytes: Vec<u8>) -> usize {
        self.images.push((mime, bytes));
        self.images.len() - 1
    }
}

/// Compile dialect HTML to inner block XML for header/footer parts.
pub(crate) fn blocks_xml_from_html(html: &str) -> Result<String, String> {
    let trimmed = html.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let mut deferred = CreateDeferred::default();
    let normalized = super::input::normalize(trimmed, false)?;
    let (body, _) = body_xml_from_html_with(&normalized, &mut deferred)?;
    if !deferred.is_empty() {
        return Err("header/footer with does not yet support images or hyperlinks".into());
    }
    Ok(body)
}

pub fn hyperlink_placeholder(i: usize) -> String {
    format!("__HL{i}__")
}

pub fn image_placeholder(i: usize) -> String {
    format!("__IMG{i}__")
}

/// A piece of paragraph content with an optional hyperlink href index into
/// [`CreateDeferred::hyperlinks`].
struct LinkedPiece {
    href_idx: Option<usize>,
    piece: InsPiece,
}

/// A header or footer slot authored in create HTML.
#[derive(Debug, Clone)]
pub struct CreateChromeSlot {
    pub scope: &'static str, // "hdr" | "ftr"
    pub kind: String,
    /// Inner body XML (`<w:p>…</w:p>`…), never empty (at least `<w:p/>`).
    pub inner_xml: String,
}

/// Parsed create chrome plan for one or more sections.
#[derive(Debug, Clone, Default)]
pub struct CreateChrome {
    pub even_odd: bool,
    pub title_page: bool,
    pub slots: Vec<CreateChromeSlot>,
    pub extra_sections: Vec<CreateSection>,
}

#[derive(Debug, Clone, Default)]
pub struct CreateSection {
    pub title_page: bool,
    pub slots: Vec<CreateChromeSlot>,
}

/// Compile dialect HTML to body XML (+ optional create chrome plan + deferred
/// media/hyperlink allocations).
pub fn body_and_chrome_from_html(
    html: &str,
) -> Result<(String, usize, Option<CreateChrome>, CreateDeferred), String> {
    if html.contains("<page-chrome") {
        return Err(
            "<page-chrome> was removed from create — author <header>/<footer> before the body (and <section/> between sections)"
                .into(),
        );
    }
    let mut deferred = CreateDeferred::default();
    let normalized = super::input::normalize(html, true)?;
    let sections = parse_create_sections(&normalized, &mut deferred)?;
    if sections.is_empty()
        || (sections.len() == 1
            && sections[0].body_html.trim().is_empty()
            && sections[0].slots.is_empty()
            && !sections[0].title_page)
    {
        return Err(
            "create html produced no body content — it needs at least one paragraph".into(),
        );
    }

    let multi = sections.len() > 1 || !sections[0].slots.is_empty() || sections[0].title_page;

    if !multi {
        let (body, count) = body_xml_from_html_with(&sections[0].body_html, &mut deferred)?;
        if body.is_empty() {
            return Err(
                "create html produced no body content — it needs at least one paragraph".into(),
            );
        }
        return Ok((body, count, None, deferred));
    }

    let mut chrome = CreateChrome {
        title_page: sections[0].title_page,
        slots: sections[0].slots.clone(),
        ..CreateChrome::default()
    };
    let mut total_paras = 0usize;
    let mut body = String::new();

    for (i, sect) in sections.iter().enumerate() {
        let is_last = i + 1 == sections.len();
        let (sect_body, count) = if sect.body_html.trim().is_empty() {
            ("<w:p/>".to_string(), 1)
        } else {
            body_xml_from_html_with(&sect.body_html, &mut deferred)?
        };
        total_paras += count;
        if !is_last {
            // Mid-section boundary: placeholder filled by build_package.
            body.push_str(&attach_sectpr_placeholder(
                &sect_body,
                &format!("<!--SECT{i}-->"),
            ));
            if i > 0 {
                chrome.extra_sections.push(CreateSection {
                    title_page: sect.title_page,
                    slots: sect.slots.clone(),
                });
            }
        } else {
            body.push_str(&sect_body);
            if i > 0 {
                chrome.extra_sections.push(CreateSection {
                    title_page: sect.title_page,
                    slots: sect.slots.clone(),
                });
            }
        }
    }
    Ok((body, total_paras, Some(chrome), deferred))
}

fn attach_sectpr_placeholder(body_xml: &str, placeholder: &str) -> String {
    if let Some(pos) = body_xml.rfind("</w:p>") {
        let mut out = String::with_capacity(body_xml.len() + placeholder.len() + 48);
        out.push_str(&body_xml[..pos]);
        out.push_str("<w:pPr><w:sectPr>");
        out.push_str(placeholder);
        out.push_str("</w:sectPr></w:pPr>");
        out.push_str(&body_xml[pos..]);
        out
    } else {
        format!("{body_xml}<w:p><w:pPr><w:sectPr>{placeholder}</w:sectPr></w:pPr></w:p>")
    }
}

#[derive(Debug, Clone, Default)]
struct ParsedCreateSection {
    title_page: bool,
    slots: Vec<CreateChromeSlot>,
    body_html: String,
}

fn parse_create_sections(
    html: &str,
    deferred: &mut CreateDeferred,
) -> Result<Vec<ParsedCreateSection>, String> {
    let mut rest = html.trim_start();
    let mut sections: Vec<ParsedCreateSection> = Vec::new();
    let mut cur = ParsedCreateSection::default();
    let mut saw_marker = false;

    loop {
        rest = rest.trim_start();
        if rest.starts_with("<section") {
            let end = rest
                .find("/>")
                .ok_or("malformed <section/> in create html")?
                + 2;
            let tag = &rest[..end];
            let title = tag.contains("first-page");
            rest = rest[end..].trim_start();
            if !cur.body_html.is_empty() || !cur.slots.is_empty() || saw_marker || cur.title_page {
                sections.push(std::mem::take(&mut cur));
            }
            cur.title_page = title;
            saw_marker = true;
            continue;
        }
        if rest.starts_with("<header") || rest.starts_with("<footer") {
            let scope = if rest.starts_with("<header") {
                "hdr"
            } else {
                "ftr"
            };
            let tag_name = if scope == "hdr" { "header" } else { "footer" };
            let (kind, blank, inner, consumed) = parse_leading_chrome_element(rest, tag_name)?;
            rest = rest[consumed..].trim_start();
            let inner_xml = if blank {
                "<w:p/>".to_string()
            } else {
                let (xml, _) = body_xml_from_html_with(inner, deferred)?;
                if xml.is_empty() {
                    "<w:p/>".to_string()
                } else {
                    xml
                }
            };
            cur.slots.push(CreateChromeSlot {
                scope,
                kind,
                inner_xml,
            });
            continue;
        }
        if rest.is_empty() {
            break;
        }
        let next_sect = rest
            .find("\n<section")
            .or_else(|| rest.find("<section"))
            .unwrap_or(rest.len());
        let chunk = rest[..next_sect].trim();
        if !chunk.is_empty() {
            if !cur.body_html.is_empty() {
                cur.body_html.push('\n');
            }
            cur.body_html.push_str(chunk);
        }
        if next_sect == rest.len() {
            break;
        }
        rest = rest[next_sect..].trim_start_matches('\n');
    }
    sections.push(cur);

    if sections.len() == 1
        && sections[0].body_html.trim().is_empty()
        && !sections[0].slots.is_empty()
    {
        sections[0].body_html = "<p></p>".into();
    }
    let _ = saw_marker;
    Ok(sections)
}

fn parse_leading_chrome_element<'a>(
    html: &'a str,
    tag: &str,
) -> Result<(String, bool, &'a str, usize), String> {
    if !html.starts_with(&format!("<{tag}")) {
        return Err(format!("expected <{tag}>"));
    }
    let after = &html[tag.len() + 1..];
    let gt = after.find('>').ok_or("malformed chrome tag")?;
    let attrs = after[..gt].trim().trim_end_matches('/').trim();
    let self_closing = after[..gt].contains('/');
    let kind = if attrs.split_whitespace().any(|a| a == "first") {
        "first"
    } else if attrs.split_whitespace().any(|a| a == "even") {
        "even"
    } else {
        "default"
    };
    let open_end = tag.len() + 1 + gt + 1;
    if self_closing {
        return Ok((kind.into(), true, "", open_end));
    }
    let close = format!("</{tag}>");
    let rest = &html[open_end..];
    let close_at = rest
        .find(&close)
        .ok_or_else(|| format!("unclosed <{tag}>"))?;
    let inner = rest[..close_at].trim_matches('\n');
    Ok((kind.into(), false, inner, open_end + close_at + close.len()))
}

fn body_xml_from_html_with(
    html: &str,
    deferred: &mut CreateDeferred,
) -> Result<(String, usize), String> {
    let items = parse_dialect(html)?;
    let mut body = String::new();
    let mut tables: Vec<TableBuilder> = Vec::new();
    let mut para_style: Option<String> = None;
    let mut para_extra = String::new();
    let mut list_ids = std::collections::HashMap::new();
    let mut pieces: Vec<LinkedPiece> = Vec::new();
    let mut para_count = 0usize;

    for item in &items {
        match item {
            Item::ParaOpen(attrs) => {
                if attrs.break_ins.is_some() || attrs.break_del.is_some() {
                    return Err(
                        "revision markup is not allowed in create — a new document has no revision history"
                            .into(),
                    );
                }
                if attrs.num.is_some() {
                    return Err(
                        "num markers are computed from numbering.xml, never authored — creating lists is not supported yet"
                            .into(),
                    );
                }
                para_extra.clear();
                if let Some(css) = &attrs.css {
                    para_extra.push_str(&super::styles::paragraph_properties(css)?);
                }
                if let Some(list) = &attrs.list {
                    if list.level > 8 {
                        return Err("DOCX lists support at most nine levels".into());
                    }
                    let id = *list_ids.entry(list.id).or_insert_with(|| {
                        deferred.lists.push(list.clone());
                        deferred.lists.len() as u32
                    });
                    para_extra.push_str(&format!(
                        "<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{id}\"/></w:numPr>",
                        list.level
                    ));
                }
                para_style = match attrs.tag.as_str() {
                    "p" => attrs.class.clone(),
                    heading => {
                        if attrs.class.is_some() {
                            return Err(format!(
                                "<{heading}> takes its style from the tag — class is only for <p>"
                            ));
                        }
                        Some(format!("Heading{}", &heading[1..]))
                    }
                };
                pieces.clear();
            }
            Item::ParaClose => {
                let paragraph = paragraph_xml(para_style.take(), &pieces, &para_extra)?;
                pieces.clear();
                para_count += 1;
                match tables.last_mut() {
                    Some(builder) => match &mut builder.cell {
                        Some(cell) => cell.xml.push_str(&paragraph),
                        None => {
                            return Err("a paragraph inside a table must be inside a <td>".into())
                        }
                    },
                    None => body.push_str(&paragraph),
                }
            }
            Item::Atom(atom) => {
                let (ins, del) = atom
                    .wrap()
                    .map(|(i, d)| (i.is_some(), d.is_some()))
                    .unwrap_or((false, false));
                if ins || del {
                    return Err(
                        "revision markup is not allowed in create — a new document has no revision history"
                            .into(),
                    );
                }
                if atom.field().is_some() {
                    return Err(
                        "fields are not supported in create — their results are computed by Word"
                            .into(),
                    );
                }
                let href_idx = atom.link().map(|href| deferred.hyperlink_idx(href));
                match atom {
                    Atom::Char {
                        fmt_pending: Some(_),
                        ..
                    } => {
                        return Err(
                            "revision markup is not allowed in create — a new document has no revision history"
                                .into(),
                        );
                    }
                    Atom::Char { ch, fmt, .. } => match pieces.last_mut() {
                        Some(LinkedPiece {
                            href_idx: open_href,
                            piece: InsPiece::Text(text, piece_fmt),
                        }) if *open_href == href_idx && piece_fmt == fmt =>
                        {
                            text.push(*ch);
                        }
                        _ => pieces.push(LinkedPiece {
                            href_idx,
                            piece: InsPiece::Text(ch.to_string(), *fmt),
                        }),
                    },
                    Atom::Br { kind, .. } => pieces.push(LinkedPiece {
                        href_idx,
                        piece: InsPiece::Br(*kind),
                    }),
                    Atom::Tab { .. } => pieces.push(LinkedPiece {
                        href_idx,
                        piece: InsPiece::Tab,
                    }),
                    Atom::Milestone { .. } => {
                        return Err(
                            "comments are not part of create — use addComment after creating"
                                .into(),
                        )
                    }
                    Atom::Note { .. } => {
                        return Err(
                            "footnotes and endnotes in create: author <footnote>/<endnote> via editHtml after create, or include them in the surface create path"
                                .into(),
                        )
                    }
                    Atom::Equation { text, display, .. } => pieces.push(LinkedPiece {
                        href_idx,
                        piece: InsPiece::Equation {
                            mathml: text.clone(),
                            display: *display,
                        },
                    }),
                    Atom::Image { src, rid, width,height,alt,.. } => {
                        if !rid.is_empty() && src.is_empty() {
                            return Err(
                                "create images require src=\"data:<mime>;base64,…\" — package rids are allocated by the engine"
                                    .into(),
                            );
                        }
                        if src.is_empty() {
                            return Err(
                                "inserting an <image/> requires src=\"data:<mime>;base64,…\"".into(),
                            );
                        }
                        let (mime, bytes) = crate::media::parse_data_url(src)?;
                        let idx = deferred.image_idx(mime, bytes);
                        pieces.push(LinkedPiece {
                            href_idx,
                            piece: InsPiece::Image {
                                rid: image_placeholder(idx),
                                width:*width,height:*height,alt:alt.clone(),
                            },
                        });
                    }
                }
            }
            Item::Struct(token) => match token.as_str() {
                "table" => {
                    if tables.last().is_some_and(|t| t.cell.is_none()) {
                        return Err("nested tables require a cell".into());
                    }
                    tables.push(TableBuilder {
                        rows: Vec::new(),
                        in_row: false,
                        cell: None,
                    });
                }
                "/table" => {
                    let builder = tables.pop().ok_or("</table> without an open <table>")?;
                    if builder.in_row || builder.cell.is_some() {
                        return Err("</table> with an open row or cell".into());
                    }
                    let xml = table_xml(&builder.rows)?;
                    if let Some(parent) = tables.last_mut() {
                        parent
                            .cell
                            .as_mut()
                            .ok_or("nested table outside cell")?
                            .xml
                            .push_str(&xml);
                    } else {
                        body.push_str(&xml);
                    }
                }
                "tr" => {
                    let builder = tables.last_mut().ok_or("<tr> outside a <table>")?;
                    if builder.in_row {
                        return Err("<tr> inside an open row".into());
                    }
                    builder.in_row = true;
                    builder.rows.push(Vec::new());
                }
                "/tr" => {
                    let builder = tables.last_mut().ok_or("</tr> outside a <table>")?;
                    if !builder.in_row || builder.cell.is_some() {
                        return Err("</tr> without an open row (or with an open cell)".into());
                    }
                    builder.in_row = false;
                }
                "td" => {
                    let builder = tables.last_mut().ok_or("<td> outside a <table>")?;
                    if !builder.in_row {
                        return Err("<td> outside a <tr>".into());
                    }
                    if builder.cell.is_some() {
                        return Err("<td> inside an open cell".into());
                    }
                    builder.cell = Some(Cell {
                        xml: String::new(),
                        colspan: 1,
                        rowspan: 1,
                    });
                }
                "/td" => {
                    let builder = tables.last_mut().ok_or("</td> outside a <table>")?;
                    let mut cell = builder.cell.take().ok_or("</td> without an open <td>")?;
                    let row = builder.rows.last_mut().ok_or("</td> outside a row")?;
                    // Word requires at least one paragraph per cell.
                    if cell.xml.is_empty() || cell.xml.ends_with("</w:tbl>") {
                        cell.xml.push_str("<w:p/>");
                    }
                    row.push(cell);
                }
                spanned if spanned.starts_with("td ") => {
                    let doc = xmloxide::Document::parse_str(&format!("<{spanned}/>"))
                        .map_err(|e| format!("invalid table cell: {e}"))?;
                    let n = doc.root_element().ok_or("missing table cell")?;
                    let span = |name: &str| -> Result<u32, String> {
                        let value = doc
                            .attribute(n, name)
                            .unwrap_or("1")
                            .parse::<u32>()
                            .map_err(|_| "invalid cell span")?;
                        if !(1..=512).contains(&value) {
                            return Err("cell spans must be between 1 and 512".into());
                        }
                        Ok(value)
                    };
                    let builder = tables.last_mut().ok_or("cell outside table")?;
                    if !builder.in_row || builder.cell.is_some() {
                        return Err("invalid table cell nesting".into());
                    }
                    builder.cell = Some(Cell {
                        xml: String::new(),
                        colspan: span("colspan")?,
                        rowspan: span("rowspan")?,
                    });
                }
                other => return Err(format!("unsupported structure <{other}> in create")),
            },
        }
    }
    if !tables.is_empty() {
        return Err("html ends inside an open <table>".into());
    }
    if body.is_empty() {
        return Err("create html produced no content — it needs at least one paragraph".into());
    }
    // Word never ends a body with a bare table (the editor keeps a paragraph
    // after a trailing table), and consumers are untested on that shape —
    // LibreOffice 26.2's text export infinite-loops on it. Follow Word.
    if body.ends_with("</w:tbl>") {
        body.push_str("<w:p/>");
        para_count += 1;
    }
    Ok((body, para_count))
}

fn paragraph_xml(
    style: Option<String>,
    pieces: &[LinkedPiece],
    extra: &str,
) -> Result<String, String> {
    let props = style
        .map(|s| format!("<w:pStyle w:val=\"{}\"/>", escape_attr(&s)))
        .unwrap_or_default()
        + extra;
    let ppr = if props.is_empty() {
        String::new()
    } else {
        format!("<w:pPr>{props}</w:pPr>")
    };
    let mut runs = String::new();
    let mut i = 0usize;
    while i < pieces.len() {
        let href = pieces[i].href_idx;
        let mut j = i + 1;
        while j < pieces.len() && pieces[j].href_idx == href {
            j += 1;
        }
        let mut inner = String::new();
        for p in &pieces[i..j] {
            inner.push_str(&run_xml(&p.piece)?);
        }
        if let Some(idx) = href {
            runs.push_str(&format!(
                "<w:hyperlink r:id=\"{}\">{inner}</w:hyperlink>",
                hyperlink_placeholder(idx)
            ));
        } else {
            runs.push_str(&inner);
        }
        i = j;
    }
    if ppr.is_empty() && runs.is_empty() {
        return Ok("<w:p/>".to_string());
    }
    Ok(format!("<w:p>{ppr}{runs}</w:p>"))
}

fn run_xml(piece: &InsPiece) -> Result<String, String> {
    Ok(match piece {
        InsPiece::Text(text, fmt) => {
            format!(
                "<w:r>{}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
                rpr_xml(*fmt),
                escape_xml_text(text)
            )
        }
        InsPiece::Br(kind) => match kind.dialect_type() {
            None => "<w:r><w:br/></w:r>".to_string(),
            Some(kind) => format!("<w:r><w:br w:type=\"{kind}\"/></w:r>"),
        },
        InsPiece::Tab => "<w:r><w:tab/></w:r>".to_string(),
        InsPiece::Equation { mathml, display } => {
            crate::html::mathml_omml::mathml_to_omml_xml(mathml, *display)?
        }
        InsPiece::Image {
            rid,
            width,
            height,
            alt,
        } => {
            format!(
                concat!(
                    "<w:r><w:drawing><wp:inline xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\">",
                    "<wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{doc_id}\" name=\"Picture\" descr=\"{alt}\"/>",
                    "<a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">",
                    "<a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">",
                    "<pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">",
                    "<pic:blipFill><a:blip xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" r:embed=\"{rid}\"/></pic:blipFill>",
                    "<pic:spPr/></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"
                ),
                rid = rid,
                cx=u64::from(width.unwrap_or(96))*9525,cy=u64::from(height.unwrap_or(96))*9525,
                doc_id=rid.chars().filter(char::is_ascii_digit).collect::<String>().parse::<u32>().unwrap_or(0)+1,
                alt=escape_attr(alt.as_deref().unwrap_or(""))
            )
        }
    })
}

fn rpr_xml(fmt: Fmt) -> String {
    if fmt == Fmt::default() {
        return String::new();
    }
    let mut inner = String::new();
    if fmt.bold {
        inner.push_str("<w:b/>");
    }
    if fmt.italic {
        inner.push_str("<w:i/>");
    }
    if fmt.underline {
        inner.push_str("<w:u w:val=\"single\"/>");
    }
    if fmt.strike {
        inner.push_str("<w:strike/>");
    }
    if let Some(align) = fmt.vert_align {
        inner.push_str(&format!("<w:vertAlign w:val=\"{}\"/>", align.docx_val()));
    }
    inner.push_str(&super::styles::extra_rpr(fmt));
    format!("<w:rPr>{inner}</w:rPr>")
}

fn table_xml(rows: &[Vec<Cell>]) -> Result<String, String> {
    let mut covered = std::collections::BTreeMap::<u32, (u32, u32)>::new();
    let mut body = String::new();
    let mut columns = 0;
    for row in rows {
        let mut col = 0;
        let mut pending = Vec::new();
        body.push_str("<w:tr>");
        for cell in row {
            while let Some(&(width, _)) = covered.get(&col) {
                body.push_str(&continuation(width));
                col += width;
            }
            if (col..col + cell.colspan).any(|c| covered.contains_key(&c)) {
                return Err("overlapping table spans".into());
            }
            let span = if cell.colspan > 1 {
                format!("<w:gridSpan w:val=\"{}\"/>", cell.colspan)
            } else {
                String::new()
            };
            let merge = if cell.rowspan > 1 {
                "<w:vMerge w:val=\"restart\"/>"
            } else {
                ""
            };
            body.push_str(&format!(
                "<w:tc><w:tcPr>{span}{merge}</w:tcPr>{}</w:tc>",
                cell.xml
            ));
            if cell.rowspan > 1 {
                pending.push((col, (cell.colspan, cell.rowspan - 1)));
            }
            col += cell.colspan;
        }
        for (&start, &(width, _)) in &covered {
            if start >= col {
                while col < start {
                    body.push_str("<w:tc><w:p/></w:tc>");
                    col += 1;
                }
                body.push_str(&continuation(width));
                col += width;
            }
        }
        columns = columns.max(col);
        body.push_str("</w:tr>");
        covered.retain(|_, (_, remaining)| {
            *remaining -= 1;
            *remaining > 0
        });
        covered.extend(pending);
    }
    if columns == 0 {
        return Err("a table needs at least one cell".into());
    }
    if !covered.is_empty() {
        return Err("rowspan extends beyond the table".into());
    }
    let grid = "<w:gridCol w:w=\"2400\"/>".repeat(columns as usize);
    let borders: String = ["top", "left", "bottom", "right", "insideH", "insideV"]
        .iter()
        .map(|side| format!("<w:{side} w:val=\"single\" w:sz=\"4\" w:color=\"auto\"/>"))
        .collect();
    Ok(format!("<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/><w:tblBorders>{borders}</w:tblBorders></w:tblPr><w:tblGrid>{grid}</w:tblGrid>{body}</w:tbl>"))
}
fn continuation(width: u32) -> String {
    let span = if width > 1 {
        format!("<w:gridSpan w:val=\"{width}\"/>")
    } else {
        String::new()
    };
    format!("<w:tc><w:tcPr>{span}<w:vMerge/></w:tcPr><w:p/></w:tc>")
}
