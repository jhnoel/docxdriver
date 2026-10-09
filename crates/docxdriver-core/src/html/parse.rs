//! Parser for the HTML dialect — the inverse of the canonical renderer, plus
//! the leniencies agent-authored fragments need (bare/empty attributes for new
//! break markers, attribute order tolerance, id-less ins/del).
//!
//! Errors are written for the agent that will read them: they name the rule,
//! not just the position.

use super::{Atom, BrKind, Fmt, FmtPending, Item, ParaAttrs, VertAlign, WrapId};

struct Lexer<'a> {
    input: &'a str,
    pos: usize,
}

#[derive(Debug)]
struct Tag {
    name: String,
    attrs: Vec<(String, Option<String>)>,
    closing: bool,
    self_closing: bool,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Lexer { input, pos: 0 }
    }

    fn rest(&self) -> &'a str {
        &self.input[self.pos..]
    }

    /// Text up to the next tag, entities decoded. Returns None at a tag or EOF.
    fn next_text(&mut self) -> Result<Option<String>, String> {
        if self.rest().is_empty() || self.rest().starts_with('<') {
            return Ok(None);
        }
        let mut out = String::new();
        while !self.rest().is_empty() && !self.rest().starts_with('<') {
            let rest = self.rest();
            let mut chars = rest.char_indices();
            let (_, ch) = chars.next().expect("non-empty");
            if ch == '&' {
                let entity_end = rest
                    .find(';')
                    .ok_or("unterminated entity: text after '&' has no ';'")?;
                let entity = &rest[1..entity_end];
                out.push(match entity {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    other => {
                        return Err(format!(
                            "unrecognized entity: &{other}; (the dialect uses amp/lt/gt/quot/apos)"
                        ))
                    }
                });
                self.pos += entity_end + 1;
            } else {
                out.push(ch);
                self.pos += ch.len_utf8();
            }
        }
        Ok(Some(out))
    }

    fn next_tag(&mut self) -> Result<Option<Tag>, String> {
        if !self.rest().starts_with('<') {
            return Ok(None);
        }
        let rest = self.rest();
        let end = rest
            .find('>')
            .ok_or_else(|| format!("unterminated tag: {}", preview(rest)))?;
        let inner = &rest[1..end];
        self.pos += end + 1;

        let (closing, inner) = match inner.strip_prefix('/') {
            Some(stripped) => (true, stripped),
            None => (false, inner),
        };
        let (self_closing, inner) = match inner.strip_suffix('/') {
            Some(stripped) => (true, stripped),
            None => (false, inner),
        };
        let inner = inner.trim();
        let name_end = inner.find(char::is_whitespace).unwrap_or(inner.len());
        let name = inner[..name_end].to_string();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(format!("malformed tag: <{inner}>"));
        }

        let mut attrs = Vec::new();
        let mut attr_rest = inner[name_end..].trim_start();
        while !attr_rest.is_empty() {
            let attr_name_end = attr_rest
                .find(|c: char| c.is_whitespace() || c == '=')
                .unwrap_or(attr_rest.len());
            let attr_name = &attr_rest[..attr_name_end];
            if attr_name.is_empty() {
                return Err(format!("malformed attributes in <{inner}>"));
            }
            attr_rest = attr_rest[attr_name_end..].trim_start();
            if let Some(after_eq) = attr_rest.strip_prefix('=') {
                let after_eq = after_eq.trim_start();
                let Some(quoted) = after_eq.strip_prefix('"') else {
                    return Err(format!(
                        "attribute {attr_name} must be double-quoted in <{inner}>"
                    ));
                };
                let close = quoted.find('"').ok_or_else(|| {
                    format!("unterminated attribute value for {attr_name} in <{inner}>")
                })?;
                let value = decode_entities(&quoted[..close])?;
                attrs.push((attr_name.to_string(), Some(value)));
                attr_rest = quoted[close + 1..].trim_start();
            } else {
                // Bare attribute (agent shorthand for a new marker: break-ins).
                attrs.push((attr_name.to_string(), None));
            }
        }
        Ok(Some(Tag {
            name,
            attrs,
            closing,
            self_closing,
        }))
    }
}

fn preview(text: &str) -> String {
    let cut: String = text.chars().take(40).collect();
    if cut.len() < text.len() {
        format!("{cut}…")
    } else {
        cut
    }
}

pub(crate) fn decode_entities(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let end = rest
            .find(';')
            .ok_or("unterminated entity in attribute value")?;
        out.push(match &rest[1..end] {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            other => return Err(format!("unrecognized entity: &{other};")),
        });
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn attr<'t>(tag: &'t Tag, name: &str) -> Option<&'t Option<String>> {
    tag.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

/// A marker attribute (break-ins / id on ins/del): absent → None; bare or
/// empty → Some(None) (new, engine assigns the id); value → Some(Some(id)).
fn marker_attr(tag: &Tag, name: &str) -> WrapId {
    match attr(tag, name) {
        None => None,
        Some(None) => Some(None),
        Some(Some(value)) if value.is_empty() => Some(None),
        Some(Some(value)) => Some(Some(value.clone())),
    }
}

/// Parse a fragment of the dialect into items. The fragment must be balanced
/// at the paragraph level (block boundaries — editHtml expands matches to
/// whole blocks before parsing).
pub fn parse_fragment(input: &str) -> Result<Vec<Item>, String> {
    parse_dialect(&super::input::normalize(input, false)?)
}

pub(crate) fn parse_dialect(input: &str) -> Result<Vec<Item>, String> {
    let mut lexer = Lexer::new(input);
    let mut items = Vec::new();

    // Open-element state.
    let mut in_para = false;
    let mut para_start = 0;
    let mut fmt = Fmt::default();
    let mut pending_b: Option<FmtPending> = None;
    let mut pending_i: Option<FmtPending> = None;
    let mut pending_u: Option<FmtPending> = None;
    let mut pending_s: Option<FmtPending> = None;
    let mut pending_vert: Option<FmtPending> = None;
    let mut pending_format: Option<FmtPending> = None;
    let mut ins: WrapId = None;
    let mut del: WrapId = None;
    let mut link: Option<String> = None;
    let mut field: Option<String> = None;
    let mut spans: Vec<(Fmt, Option<FmtPending>)> = Vec::new();

    loop {
        if let Some(text) = lexer.next_text()? {
            if in_para {
                if pending_format.is_some() && text.chars().any(|c| c == '\t') {
                    return Err(
                        "<format pending> cannot wrap tabs — only character content is supported"
                            .into(),
                    );
                }
                let fmt_pending = pending_format
                    .clone()
                    .or_else(|| pending_b.clone())
                    .or_else(|| pending_i.clone())
                    .or_else(|| pending_u.clone())
                    .or_else(|| pending_s.clone())
                    .or_else(|| pending_vert.clone());
                for ch in text.chars() {
                    if ch == '\t' {
                        items.push(Item::Atom(Atom::Tab {
                            ins: ins.clone(),
                            del: del.clone(),
                            link: link.clone(),
                            field: field.clone(),
                        }));
                    } else if ch == '\n' || ch == '\r' {
                        return Err(
                            "raw newline inside a paragraph: paragraph content is a single line. \
                             For a NEW PARAGRAPH, close and reopen the paragraph element — replace \
                             <p>whole paragraph</p> with <p break-ins>first half</p> + newline + <p>second half</p> \
                             (the bare break-ins records the break as a tracked split). \
                             For a line break within the paragraph, use <br/>."
                                .to_string(),
                        );
                    } else {
                        items.push(Item::Atom(Atom::Char {
                            ch,
                            fmt,
                            fmt_pending: fmt_pending.clone(),
                            ins: ins.clone(),
                            del: del.clone(),
                            link: link.clone(),
                            field: field.clone(),
                        }));
                    }
                }
            } else if !text.chars().all(char::is_whitespace) {
                return Err(format!(
                    "text outside a paragraph element: {:?} — content lives inside <p>/<h1..6>",
                    preview(&text)
                ));
            }
            continue;
        }
        let Some(tag) = lexer.next_tag()? else {
            break;
        };
        let name = tag.name.as_str();
        match (name, tag.closing) {
            ("p", false)
            | ("h1", false)
            | ("h2", false)
            | ("h3", false)
            | ("h4", false)
            | ("h5", false)
            | ("h6", false) => {
                if in_para {
                    return Err(format!(
                        "<{name}> inside a paragraph: close the previous paragraph first"
                    ));
                }
                in_para = true;
                para_start = items.len();
                fmt = Fmt::default();
                pending_b = None;
                pending_i = None;
                pending_u = None;
                pending_s = None;
                pending_vert = None;
                pending_format = None;
                ins = None;
                del = None;
                link = None;
                field = None;
                items.push(Item::ParaOpen(ParaAttrs {
                    tag: name.to_string(),
                    class: attr(&tag, "class").and_then(|v| v.clone()),
                    break_ins: marker_attr(&tag, "break-ins"),
                    break_del: marker_attr(&tag, "break-del"),
                    num: attr(&tag, "num").and_then(|v| v.clone()),
                    css: attr(&tag, "style").and_then(|v| v.clone()),
                    list: attr(&tag, "list-id")
                        .and_then(|v| v.as_ref())
                        .map(|id| {
                            Ok::<_, String>(super::ListAttrs {
                                id: id.parse().map_err(|_| "invalid list id")?,
                                level: attr(&tag, "list-level")
                                    .and_then(|v| v.as_ref())
                                    .and_then(|v| v.parse().ok())
                                    .unwrap_or(0),
                                format: attr(&tag, "list-format")
                                    .and_then(|v| v.clone())
                                    .unwrap_or_else(|| "decimal".into()),
                                start: attr(&tag, "list-start")
                                    .and_then(|v| v.as_ref())
                                    .and_then(|v| v.parse().ok())
                                    .unwrap_or(1),
                            })
                        })
                        .transpose()?,
                }));
            }
            ("p", true)
            | ("h1", true)
            | ("h2", true)
            | ("h3", true)
            | ("h4", true)
            | ("h5", true)
            | ("h6", true) => {
                if !in_para {
                    return Err(format!("</{name}> without an open paragraph"));
                }
                if ins.is_some() || del.is_some() {
                    return Err(format!("</{name}> with an unclosed <ins>/<del>"));
                }
                if fmt != Fmt::default() {
                    return Err(format!("</{name}> with unclosed formatting tags"));
                }
                if pending_format.is_some() {
                    return Err(format!("</{name}> with an unclosed <format pending>"));
                }
                if link.is_some() {
                    return Err(format!("</{name}> with an unclosed <a>"));
                }
                if field.is_some() {
                    return Err(format!("</{name}> with an unclosed <field>"));
                }
                in_para = false;
                infer_equation_display(&mut items[para_start..]);
                items.push(Item::ParaClose);
            }
            ("ins", false) => {
                if !in_para {
                    return Err("<ins> outside a paragraph".to_string());
                }
                if ins.is_some() {
                    return Err("<ins> nested inside <ins>".to_string());
                }
                ins = Some(marker_attr(&tag, "id").unwrap_or(None));
            }
            ("ins", true) => {
                if ins.take().is_none() {
                    return Err("</ins> without an open <ins>".to_string());
                }
            }
            ("del", false) => {
                if !in_para {
                    return Err("<del> outside a paragraph".to_string());
                }
                if del.is_some() {
                    return Err("<del> nested inside <del>".to_string());
                }
                del = Some(marker_attr(&tag, "id").unwrap_or(None));
            }
            ("del", true) => {
                if del.take().is_none() {
                    return Err("</del> without an open <del>".to_string());
                }
            }
            ("span", false) => {
                spans.push((fmt, pending_format.clone()));
                if let Some(Some(style)) = attr(&tag, "style") {
                    fmt = super::styles::apply_inline_css(fmt, style)?;
                }
                if attr(&tag, "pending").is_some() {
                    pending_format = Some(FmtPending {
                        id: attr(&tag, "id").and_then(|v| v.clone()),
                        author: attr(&tag, "author").and_then(|v| v.clone()),
                    });
                }
            }
            ("span", true) => {
                let (old, pending) = spans.pop().ok_or("</span> without open span")?;
                fmt = old;
                pending_format = pending;
            }
            ("b", false) | ("i", false) | ("u", false) | ("s", false) => {
                if pending_format.is_some() {
                    return Err(format!(
                        "<{name}> inside <format pending> — the neutral wrapper is only for live dialect format all off"
                    ));
                }
                let flag = match name {
                    "b" => &mut fmt.bold,
                    "i" => &mut fmt.italic,
                    "u" => &mut fmt.underline,
                    _ => &mut fmt.strike,
                };
                if *flag {
                    return Err(format!("<{name}> nested inside <{name}>"));
                }
                *flag = true;
                let pending_slot = match name {
                    "b" => &mut pending_b,
                    "i" => &mut pending_i,
                    "u" => &mut pending_u,
                    _ => &mut pending_s,
                };
                if attr(&tag, "pending").is_some() {
                    if pending_slot.is_some() {
                        return Err(format!(
                            "<{name} pending> nested inside another pending <{name}>"
                        ));
                    }
                    *pending_slot = Some(FmtPending {
                        id: attr(&tag, "id")
                            .and_then(|v| v.clone())
                            .filter(|s| !s.is_empty()),
                        author: attr(&tag, "author")
                            .and_then(|v| v.clone())
                            .filter(|s| !s.is_empty()),
                    });
                }
            }
            ("b", true) | ("i", true) | ("u", true) | ("s", true) => {
                let flag = match name {
                    "b" => &mut fmt.bold,
                    "i" => &mut fmt.italic,
                    "u" => &mut fmt.underline,
                    _ => &mut fmt.strike,
                };
                if !*flag {
                    return Err(format!("</{name}> without an open <{name}>"));
                }
                *flag = false;
                match name {
                    "b" => pending_b = None,
                    "i" => pending_i = None,
                    "u" => pending_u = None,
                    _ => pending_s = None,
                }
            }
            ("sup", false) | ("sub", false) => {
                if pending_format.is_some() {
                    return Err(format!(
                        "<{name}> inside <format pending> — the neutral wrapper is only for live dialect format all off"
                    ));
                }
                let align = if name == "sup" {
                    VertAlign::Superscript
                } else {
                    VertAlign::Subscript
                };
                if let Some(existing) = fmt.vert_align {
                    return Err(format!(
                        "<{name}> nested inside <{}> — superscript and subscript are mutually exclusive",
                        existing.dialect_tag()
                    ));
                }
                fmt.vert_align = Some(align);
                if attr(&tag, "pending").is_some() {
                    if pending_vert.is_some() {
                        return Err(format!(
                            "<{name} pending> nested inside another pending vertical-align tag"
                        ));
                    }
                    pending_vert = Some(FmtPending {
                        id: attr(&tag, "id")
                            .and_then(|v| v.clone())
                            .filter(|s| !s.is_empty()),
                        author: attr(&tag, "author")
                            .and_then(|v| v.clone())
                            .filter(|s| !s.is_empty()),
                    });
                }
            }
            ("sup", true) | ("sub", true) => {
                let expected = if name == "sup" {
                    VertAlign::Superscript
                } else {
                    VertAlign::Subscript
                };
                match fmt.vert_align {
                    Some(align) if align == expected => {
                        fmt.vert_align = None;
                        pending_vert = None;
                    }
                    Some(align) => {
                        return Err(format!(
                            "</{name}> does not match open <{}>",
                            align.dialect_tag()
                        ));
                    }
                    None => return Err(format!("</{name}> without an open <{name}>")),
                }
            }
            ("format", false) => {
                if !in_para {
                    return Err("<format> outside a paragraph".to_string());
                }
                if attr(&tag, "pending").is_none() {
                    return Err(
                        "<format> requires the pending attribute (<format pending>…)".into(),
                    );
                }
                if pending_format.is_some() {
                    return Err("<format pending> nested inside another <format pending>".into());
                }
                if fmt != Fmt::default()
                    || pending_b.is_some()
                    || pending_i.is_some()
                    || pending_u.is_some()
                    || pending_s.is_some()
                    || pending_vert.is_some()
                {
                    return Err(
                        "<format pending> cannot nest with <b>/<i>/<u>/<s>/<sup>/<sub> — it means live dialect format is all off"
                            .into(),
                    );
                }
                pending_format = Some(FmtPending {
                    id: attr(&tag, "id")
                        .and_then(|v| v.clone())
                        .filter(|s| !s.is_empty()),
                    author: attr(&tag, "author")
                        .and_then(|v| v.clone())
                        .filter(|s| !s.is_empty()),
                });
            }
            ("format", true) => {
                if pending_format.take().is_none() {
                    return Err("</format> without an open <format pending>".into());
                }
            }
            ("br", false) => {
                if !in_para {
                    return Err("<br/> outside a paragraph".to_string());
                }
                if pending_format.is_some() {
                    return Err(
                        "<format pending> cannot wrap breaks — only character content is supported"
                            .into(),
                    );
                }
                let kind = match attr(&tag, "type") {
                    None => BrKind::Line,
                    Some(Some(value)) => BrKind::from_dialect(Some(value))?,
                    Some(None) => {
                        return Err("<br type> requires a value (page or column)".to_string())
                    }
                };
                items.push(Item::Atom(Atom::Br {
                    kind,
                    ins: ins.clone(),
                    del: del.clone(),
                    link: link.clone(),
                    field: field.clone(),
                }));
            }
            ("image", false) => {
                if !in_para {
                    return Err("<image/> outside a paragraph".to_string());
                }
                if !tag.self_closing {
                    return Err("<image> must be self-closing (<image/>)".to_string());
                }
                // Bound existing sites omit src (rid recovered from XML).
                // Authored inserts carry src="data:<mime>;base64,<payload>".
                let src = attr(&tag, "src")
                    .and_then(|v| v.clone())
                    .unwrap_or_default();
                if !src.is_empty() && !src.starts_with("data:") {
                    return Err(
                        "image src must be a data: URL (data:<mime>;base64,…); existing images reappear as bare <image/>"
                            .into(),
                    );
                }
                let rid = attr(&tag, "rid")
                    .and_then(|v| v.clone())
                    .unwrap_or_default();
                items.push(Item::Atom(Atom::Image {
                    src,
                    rid,
                    width: attr(&tag, "w")
                        .and_then(|v| v.as_ref())
                        .map(|v| v.parse::<u32>().map_err(|_| "invalid image width"))
                        .transpose()?,
                    height: attr(&tag, "h")
                        .and_then(|v| v.as_ref())
                        .map(|v| v.parse::<u32>().map_err(|_| "invalid image height"))
                        .transpose()?,
                    alt: attr(&tag, "alt").and_then(|v| v.clone()),
                    ins: ins.clone(),
                    del: del.clone(),
                    link: link.clone(),
                    field: field.clone(),
                }));
            }
            ("image", true) => {
                return Err(
                    "</image> without a matching <image/> — images are self-closing".to_string(),
                );
            }
            ("field", false) => {
                if !in_para {
                    return Err("<field> outside a paragraph".to_string());
                }
                if field.is_some() {
                    return Err("<field> nested inside <field>".to_string());
                }
                let Some(Some(instr)) = attr(&tag, "instr") else {
                    return Err("<field> requires instr=\"…\"".to_string());
                };
                field = Some(instr.clone());
            }
            ("field", true) => {
                if field.take().is_none() {
                    return Err("</field> without an open <field>".to_string());
                }
            }
            ("equation", false) => {
                if !in_para {
                    return Err("<equation> outside a paragraph".to_string());
                }
                if tag.self_closing {
                    return Err(
                        "<equation> must not be self-closing — it wraps math text".to_string()
                    );
                }
                // Read legacy attribute/MathML markup, then normalize the
                // paragraph's display mode from its substantive content.
                let text = match attr(&tag, "latex") {
                    Some(Some(latex)) => {
                        let Some(math) = lexer.next_tag()? else {
                            return Err("<equation latex=\"…\"> requires a <math> child".into());
                        };
                        if math.name != "math" || math.closing || math.self_closing {
                            return Err(
                                "<equation latex=\"…\"> requires exactly one <math>…</math> child"
                                    .into(),
                            );
                        }
                        take_balanced_inner(&mut lexer, "math")?;
                        match lexer.next_tag()? {
                            Some(close) if close.name == "equation" && close.closing => {}
                            _ => return Err("<equation latex=\"…\"> must close immediately after its <math> child".into()),
                        }
                        latex.clone()
                    }
                    Some(None) => {
                        return Err("equation latex attribute must be double-quoted".into())
                    }
                    None => {
                        let text = lexer.next_text()?.unwrap_or_default();
                        match lexer.next_tag()? {
                            Some(close) if close.name == "equation" && close.closing => {}
                            _ => {
                                return Err(
                                    "<equation> must be immediately closed by </equation> — equations are atomic and cannot contain markup".to_string(),
                                )
                            }
                        }
                        text
                    }
                };
                items.push(Item::Atom(Atom::Equation {
                    display: if text.trim_start().starts_with("<math") {
                        xmloxide::Document::parse_str(&text)
                            .ok()
                            .and_then(|d| {
                                d.root_element()
                                    .map(|n| d.attribute(n, "display") == Some("block"))
                            })
                            .unwrap_or(false)
                    } else {
                        false
                    },
                    text,
                    ins: ins.clone(),
                    del: del.clone(),
                }));
            }
            ("equation", true) => {
                return Err("</equation> without a matching <equation>".to_string());
            }
            ("a", false) => {
                if !in_para {
                    return Err("<a> outside a paragraph".to_string());
                }
                if link.is_some() {
                    return Err("<a> nested inside <a>".to_string());
                }
                let Some(Some(href)) = attr(&tag, "href") else {
                    return Err("<a> requires href=\"…\"".to_string());
                };
                link = Some(href.clone());
            }
            ("a", true) => {
                if link.take().is_none() {
                    return Err("</a> without an open <a>".to_string());
                }
            }
            ("comment-start", false) | ("comment-end", false) => {
                if !tag.self_closing {
                    return Err(format!(
                        "<{name}> must be self-closing (<{name} id=\"…\"/>)"
                    ));
                }
                let Some(Some(id)) = attr(&tag, "id") else {
                    return Err(format!("<{name}/> requires an id"));
                };
                items.push(Item::Atom(Atom::Milestone {
                    start: name == "comment-start",
                    id: id.clone(),
                }));
            }
            ("footnote-ref", _) | ("endnote-ref", _) => {
                return Err(format!(
                    "<{name}> was removed — notes are inline <footnote>/<endnote> elements at the reference site"
                ));
            }
            ("footnote", false) | ("endnote", false) => {
                if !in_para {
                    return Err(format!("<{name}> outside a paragraph"));
                }
                let endnote = name == "endnote";
                let inner = if tag.self_closing {
                    String::new()
                } else {
                    take_balanced_inner(&mut lexer, name)?
                };
                items.push(Item::Atom(Atom::Note {
                    endnote,
                    id: None,
                    inner: super::surface::canonicalize_note_inner(&inner),
                    ins: ins.clone(),
                    del: del.clone(),
                }));
            }
            ("footnote", true) | ("endnote", true) => {
                return Err(format!("</{name}> without a matching <{name}>"));
            }
            ("table", closing) | ("tr", closing) | ("td", closing) => {
                if in_para {
                    return Err(format!("<{name}> inside a paragraph", name = name));
                }
                let mut token = if closing {
                    format!("/{name}")
                } else {
                    name.to_string()
                };
                if !closing && name == "td" {
                    // Canonical span attrs ride in the token so the lockstep
                    // walk compares them verbatim like the rest of the structure.
                    for span in ["colspan", "rowspan"] {
                        if let Some(Some(value)) = attr(&tag, span) {
                            token.push_str(&format!(" {span}=\"{value}\""));
                        }
                    }
                }
                items.push(Item::Struct(token));
            }
            ("comments", _) | ("comment", _) => {
                return Err(
                    "the <comments> footer is read-only: it is not part of the editable body"
                        .to_string(),
                );
            }
            ("footnotes", _) | ("endnotes", _) => {
                return Err(format!(
                    "<{name}> footers were removed — note bodies render inline inside <footnote>/<endnote> at the reference"
                ));
            }
            ("page-chrome", _) | ("sect", _) | ("hdr", _) | ("ftr", _) => {
                return Err(format!(
                    "<{name}> was removed — use <section/>, <header>, and <footer> in the package-wide projection"
                ));
            }
            ("section", _) | ("header", _) | ("footer", _) => {
                return Err(format!(
                    "<{name}> is a document-surface element — editHtml matches the full projection; structural chrome edits are applied by the surface compiler, not the body fragment parser"
                ));
            }
            _ => {
                return Err(format!(
                    "unsupported tag <{}{name}> — the dialect allows p, h1-h6, a, field, equation, image, footnote, endnote, ins, del, b, i, u, sup, sub, format, br, comment-start/end, table, tr, td",
                    if tag.closing { "/" } else { "" }
                ));
            }
        }
    }
    if in_para {
        return Err("fragment ends inside an open paragraph".to_string());
    }
    if ins.is_some()
        || del.is_some()
        || fmt != Fmt::default()
        || pending_format.is_some()
        || link.is_some()
        || field.is_some()
    {
        return Err("fragment ends with unclosed tags".to_string());
    }
    Ok(items)
}

/// A paragraph containing one equation and no other visible content is a
/// display equation. Revision wrappers and milestones do not count as content.
fn infer_equation_display(items: &mut [Item]) {
    let mut equation_index = None;
    for (index, item) in items.iter().enumerate() {
        match item {
            Item::ParaOpen(_) | Item::Atom(Atom::Milestone { .. }) => {}
            Item::Atom(Atom::Char { ch, .. }) if ch.is_whitespace() => {}
            Item::Atom(Atom::Equation { .. }) if equation_index.is_none() => {
                equation_index = Some(index);
            }
            _ => return,
        }
    }
    if let Some(index) = equation_index {
        if let Item::Atom(Atom::Equation { display, text, .. }) = &mut items[index] {
            if text.trim_start().starts_with("<math") && !text.contains("data-docx-legacy-display")
            {
                return;
            }
            *display = true;
        }
    }
}

/// Collect raw inner markup of a nonempty `<tag>…</tag>`, respecting nesting.
fn take_balanced_inner(lexer: &mut Lexer<'_>, name: &str) -> Result<String, String> {
    let start = lexer.pos;
    let mut depth = 1usize;
    loop {
        if lexer.rest().is_empty() {
            return Err(format!("unclosed <{name}>"));
        }
        if !lexer.rest().starts_with('<') {
            // Skip text.
            let _ = lexer.next_text()?;
            continue;
        }
        let tag_start = lexer.pos;
        let Some(tag) = lexer.next_tag()? else {
            return Err(format!("unclosed <{name}>"));
        };
        if tag.name == name {
            if tag.closing {
                depth -= 1;
                if depth == 0 {
                    // Inner is everything before this closing tag.
                    return Ok(lexer.input[start..tag_start].to_string());
                }
            } else if !tag.self_closing {
                depth += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_dialect as parse_fragment;
    use super::*;

    #[test]
    fn equation_uses_mathml_child_and_latex_source() {
        let items = parse_fragment(
            r#"<p><equation display latex="x^2"><math display="block"><msup><mi>x</mi><mn>2</mn></msup></math></equation></p>"#,
        )
        .unwrap();

        assert!(matches!(
            items.as_slice(),
            [Item::ParaOpen(_), Item::Atom(Atom::Equation { text, display: true, .. }), Item::ParaClose] if text == "x^2"
        ));
    }

    #[test]
    fn equation_mathml_child_must_be_followed_by_equation_close() {
        let err =
            parse_fragment(r#"<p><equation latex="x"><math><mi>x</mi></math>extra</equation></p>"#)
                .unwrap_err();
        assert!(err.contains("must close immediately"), "{err}");
    }

    #[test]
    fn equation_display_follows_paragraph_content() {
        let sole = parse_fragment(r#"<p><ins><equation>\frac{a}{b}</equation></ins></p>"#).unwrap();
        assert!(matches!(
            sole.as_slice(),
            [Item::ParaOpen(_), Item::Atom(Atom::Equation { text, display: true, .. }), Item::ParaClose]
                if text == r"\frac{a}{b}"
        ));

        let inline = parse_fragment("<p>x <equation>y</equation>.</p>").unwrap();
        assert!(inline
            .iter()
            .any(|item| matches!(item, Item::Atom(Atom::Equation { display: false, .. }))));

        let multiple =
            parse_fragment("<p><equation>x</equation><equation>y</equation></p>").unwrap();
        assert_eq!(
            multiple
                .iter()
                .filter(|item| matches!(item, Item::Atom(Atom::Equation { display: false, .. })))
                .count(),
            2
        );
    }

    #[test]
    fn legacy_equation_markup_normalizes_to_paragraph_rule() {
        let items = parse_fragment(
            r#"<p>before <equation display latex="x"><math><mi>x</mi></math></equation></p>"#,
        )
        .unwrap();
        assert!(items.iter().any(|item| matches!(
            item,
            Item::Atom(Atom::Equation { text, display: false, .. }) if text == "x"
        )));
    }

    #[test]
    fn equation_text_decodes_entities_without_parsing_nested_markup() {
        let items = parse_fragment("<p><equation>a&amp;b&lt;c</equation></p>").unwrap();
        assert!(matches!(
            items.as_slice(),
            [Item::ParaOpen(_), Item::Atom(Atom::Equation { text, display: true, .. }), Item::ParaClose]
                if text == "a&b<c"
        ));
        assert!(parse_fragment("<p><equation><b>x</b></equation></p>").is_err());
    }
}
