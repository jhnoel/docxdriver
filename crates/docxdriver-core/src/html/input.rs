//! HTML5 syntax → the small internal DOCX atom grammar. The intermediate
//! serialization is private; all public reads emit real HTML.
use super::{escape_attr, escape_text};
use xmloxide::{
    html5::{parse_html5_full_with_options, Html5ParseOptions},
    tree::{Document, NodeId, NodeKind},
};

pub fn normalize(input: &str, document: bool) -> Result<String, String> {
    // Legacy empty story/section markers are accepted on input only.
    let input = input
        .replace("<section/>", "<section></section>")
        .replace("<section first-page/>", "<section first-page></section>")
        .replace("<header/>", "<header></header>")
        .replace("<footer/>", "<footer></footer>");
    let input = regex_lite::Regex::new(
        r"<(image|comment-start|comment-end|header|footer|section)\b([^<>]*?)/>",
    )
    .expect("constant regex")
    .replace_all(&input, "<$1$2></$1>");
    let full = input.trim_start().to_ascii_lowercase();
    let parsed = parse_html5_full_with_options(
        &input,
        &Html5ParseOptions {
            scripting: false,
            fragment_context: if full.starts_with("<!doctype") || full.starts_with("<html") {
                None
            } else {
                Some("body".into())
            },
        },
    );
    // Structural repair can discard/move user content. Optional end tags and
    // HTML character references are accepted, but parser errors are explicit.
    if let Some(error) = parsed.errors.first() {
        return Err(format!("invalid HTML: {error:?}"));
    }
    let mut state = Lower {
        doc: &parsed.document,
        next_list: 0,
        lists: Vec::new(),
        document,
    };
    let root = state
        .doc
        .root_element()
        .ok_or("HTML has no fragment root")?;
    state.children(root, false)
}

#[derive(Clone)]
struct List {
    id: usize,
    level: usize,
    format: String,
    start: u32,
}
struct Lower<'a> {
    doc: &'a Document,
    next_list: usize,
    lists: Vec<List>,
    document: bool,
}
impl Lower<'_> {
    fn children(&mut self, node: NodeId, inline: bool) -> Result<String, String> {
        let mut out = String::new();
        for child in self.doc.children(node).collect::<Vec<_>>() {
            out.push_str(&self.node(child, inline)?);
        }
        Ok(out)
    }
    fn attr(&self, node: NodeId, name: &str) -> Option<&str> {
        self.doc.attribute(node, name)
    }
    fn attrs(&self, n: NodeId, names: &[(&str, &str)]) -> String {
        let mut out = String::new();
        for (from, to) in names {
            if let Some(v) = self.attr(n, from) {
                out.push_str(&format!(" {to}=\"{}\"", escape_attr(v)));
            }
        }
        out
    }
    fn check_attrs(&self, node: NodeId) -> Result<(), String> {
        for a in self.doc.attributes(node) {
            if a.name.starts_with("on") || matches!(a.name.as_str(), "srcdoc" | "srcset") {
                return Err(format!("unsupported HTML attribute {}", a.name));
            }
        }
        Ok(())
    }
    fn node(&mut self, n: NodeId, inline: bool) -> Result<String, String> {
        let Some(name) = self.doc.node_name(n) else {
            if !matches!(
                self.doc.node(n).kind,
                NodeKind::Text { .. } | NodeKind::CData { .. }
            ) {
                return Ok(String::new());
            }
            return Ok(self
                .doc
                .node_text(n)
                .map(|t| {
                    escape_text(&if inline {
                        t.replace(['\n', '\r'], " ")
                    } else {
                        t.to_string()
                    })
                })
                .unwrap_or_default());
        };
        self.check_attrs(n)?;
        match name {
            "html" | "body" | "main" | "article" => self.children(n, inline),
            "head" => {
                for c in self.doc.children(n) {
                    if let Some(name) = self.doc.node_name(c) {
                        if !matches!(name, "meta" | "title" | "style") {
                            return Err(format!("unsupported document head element <{name}>"));
                        }
                        if name == "style"
                            && self.doc.text_content(c).trim() != super::styles::CSS.trim()
                        {
                            return Err("authored stylesheets are unsupported; use inline document formatting".into());
                        }
                    }
                }
                Ok(String::new())
            }
            "div" | "blockquote" => {
                if inline {
                    return Err(format!(
                        "<{name}> is block content; use a paragraph operation"
                    ));
                }
                let has_blocks = self.doc.children(n).any(|c| {
                    self.doc.node_name(c).is_some_and(|s| {
                        matches!(
                            s,
                            "p" | "h1"
                                | "h2"
                                | "h3"
                                | "h4"
                                | "h5"
                                | "h6"
                                | "div"
                                | "table"
                                | "ul"
                                | "ol"
                                | "blockquote"
                        )
                    })
                });
                if has_blocks {
                    self.children(n, false)
                } else {
                    Ok(format!(
                        "<p{}>{}</p>",
                        if name == "blockquote" {
                            " class=\"Quote\""
                        } else {
                            ""
                        },
                        self.children(n, true)?
                    ))
                }
            }
            "section" => {
                if inline || !self.document {
                    return Err("sections require document HTML".into());
                }
                let marker = if self.attr(n, "first-page").is_some()
                    || self.attr(n, "data-docx-first-page") == Some("true")
                {
                    "<section first-page/>"
                } else {
                    "<section/>"
                };
                Ok(format!("{marker}{}", self.children(n, false)?))
            }
            "header" | "footer" => {
                if inline || !self.document {
                    return Err("headers/footers require document HTML".into());
                }
                let kind =
                    self.attr(n, "data-docx-kind")
                        .unwrap_or(if self.attr(n, "first").is_some() {
                            "first"
                        } else if self.attr(n, "even").is_some() {
                            "even"
                        } else {
                            "default"
                        });
                let flag = match kind {
                    "default" => "",
                    "first" => " first",
                    "even" => " even",
                    _ => return Err("unknown header/footer kind".into()),
                };
                Ok(format!(
                    "<{name}{flag}>{}</{name}>",
                    self.children(n, false)?
                ))
            }
            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if inline {
                    return Err("paragraph elements are not allowed in inline content".into());
                }
                let mut attrs = self.attrs(
                    n,
                    &[
                        ("id", "id"),
                        ("data-docx-style", "class"),
                        ("class", "class"),
                        ("style", "style"),
                        ("break-ins", "break-ins"),
                        ("break-del", "break-del"),
                        ("data-docx-break-ins", "break-ins"),
                        ("data-docx-break-del", "break-del"),
                    ],
                );
                if let Some(list) = self.lists.last() {
                    attrs.push_str(&format!(
                        " list-id=\"{}\" list-level=\"{}\" list-format=\"{}\" list-start=\"{}\"",
                        list.id, list.level, list.format, list.start
                    ));
                }
                let mut body = self.children(n, true)?;
                if let Some(css) = self.attr(n, "style") {
                    super::styles::paragraph_properties(css)?;
                    let run_css: String = super::styles::declarations(css)?
                        .into_iter()
                        .filter(|(k, _)| {
                            matches!(
                                k.as_str(),
                                "color"
                                    | "font-family"
                                    | "font-size"
                                    | "font-weight"
                                    | "font-style"
                                    | "text-decoration"
                                    | "vertical-align"
                            )
                        })
                        .map(|(k, v)| format!("{k}:{v};"))
                        .collect();
                    if !run_css.is_empty() {
                        body = format!("<span style=\"{}\">{body}</span>", escape_attr(&run_css));
                    }
                }
                Ok(format!("<{name}{attrs}>{body}</{name}>\n"))
            }
            "ol" | "ul" => {
                if inline {
                    return Err("lists are not inline content".into());
                }
                self.next_list += 1;
                let start = self
                    .attr(n, "start")
                    .unwrap_or("1")
                    .parse::<u32>()
                    .map_err(|_| "list start must be a positive integer")?;
                if start == 0 {
                    return Err("list start must be positive".into());
                }
                let format = if name == "ul" {
                    "bullet"
                } else {
                    match self.attr(n, "type").unwrap_or("1") {
                        "1" => "decimal",
                        "a" => "lowerLetter",
                        "A" => "upperLetter",
                        "i" => "lowerRoman",
                        "I" => "upperRoman",
                        _ => return Err("unsupported ordered list type".into()),
                    }
                };
                self.lists.push(List {
                    id: self.next_list,
                    level: self.lists.len(),
                    format: format.into(),
                    start,
                });
                let out = self.children(n, false)?;
                self.lists.pop();
                Ok(out)
            }
            "li" => {
                if inline || self.lists.is_empty() {
                    return Err("<li> requires an enclosing list".into());
                }
                let mut out = String::new();
                let mut pending = String::new();
                for c in self.doc.children(n).collect::<Vec<_>>() {
                    if self.doc.node_name(c).is_some_and(|s| {
                        matches!(
                            s,
                            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ol" | "ul"
                        )
                    }) {
                        if !pending.trim().is_empty() {
                            out.push_str(&self.list_paragraph(&pending));
                        }
                        pending.clear();
                        out.push_str(&self.node(c, false)?);
                    } else {
                        pending.push_str(&self.node(c, true)?);
                    }
                }
                if !pending.trim().is_empty() || out.is_empty() {
                    out.push_str(&self.list_paragraph(&pending));
                }
                // The DOCX list model currently represents one paragraph per
                // item. Reject continuation paragraphs rather than renumbering.
                let item_paragraphs = out
                    .matches(&format!(" list-id=\"{}\"", self.lists.last().unwrap().id))
                    .count();
                if item_paragraphs > 1 {
                    return Err("multiple paragraphs in one list item are unsupported; use one paragraph with <br>".into());
                }
                Ok(out)
            }
            "table" | "tr" | "td" | "th" => {
                if inline {
                    return Err("tables are not inline content".into());
                }
                let tag = if name == "th" { "td" } else { name };
                let attrs = self.attrs(n, &[("colspan", "colspan"), ("rowspan", "rowspan")]);
                let has_paragraph = self.doc.children(n).any(|c| {
                    self.doc.node_name(c).is_some_and(|s| {
                        matches!(s, "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "table")
                    })
                });
                let body = if matches!(name, "td" | "th") && !has_paragraph {
                    format!("<p>{}</p>", self.children(n, true)?)
                } else {
                    self.children(n, false)?
                };
                Ok(format!("<{tag}{attrs}>{body}</{tag}>"))
            }
            "tbody" | "thead" | "tfoot" => self.children(n, false),
            "strong" | "b" | "em" | "i" | "u" | "s" | "strike" | "sup" | "sub" | "ins" | "del" => {
                if !inline {
                    return Err(format!("<{name}> requires a paragraph"));
                }
                let tag = match name {
                    "strong" => "b",
                    "em" => "i",
                    "strike" => "s",
                    _ => name,
                };
                if !matches!(tag, "ins" | "del")
                    && self.attr(n, "data-docx-format-change").is_none()
                    && self.attr(n, "pending").is_none()
                {
                    let base = match tag {
                        "b" => "font-weight:bold",
                        "i" => "font-style:italic",
                        "u" => "text-decoration:underline",
                        "s" => "text-decoration:line-through",
                        "sup" => "vertical-align:super",
                        _ => "vertical-align:sub",
                    };
                    let style = format!("{base};{}", self.attr(n, "style").unwrap_or(""));
                    // Nested native formatting is legal HTML; a stack of spans
                    // preserves the inherited format instead of toggling it off.
                    return Ok(format!(
                        "<span style=\"{}\">{}</span>",
                        escape_attr(&style),
                        self.children(n, true)?
                    ));
                }
                let attrs = self.attrs(
                    n,
                    &[
                        ("data-docx-revision", "id"),
                        ("data-docx-author", "author"),
                        ("data-docx-format-change", "pending"),
                        ("id", "id"),
                        ("author", "author"),
                        ("pending", "pending"),
                    ],
                );
                Ok(format!("<{tag}{attrs}>{}</{tag}>", self.children(n, true)?))
            }
            "span" => {
                if !inline {
                    return Err("<span> requires a paragraph".into());
                }
                if let Some(id) = self.attr(n, "data-docx-comment-start") {
                    return Ok(format!("<comment-start id=\"{}\"/>", escape_attr(id)));
                }
                if let Some(id) = self.attr(n, "data-docx-comment-end") {
                    return Ok(format!("<comment-end id=\"{}\"/>", escape_attr(id)));
                }
                if let Some(instr) = self.attr(n, "data-docx-field") {
                    return Ok(format!(
                        "<field instr=\"{}\">{}</field>",
                        escape_attr(instr),
                        self.children(n, true)?
                    ));
                }
                let attrs = self.attrs(
                    n,
                    &[
                        ("style", "style"),
                        ("data-docx-format-change", "pending"),
                        ("data-docx-revision", "id"),
                        ("data-docx-author", "author"),
                    ],
                );
                Ok(format!("<span{attrs}>{}</span>", self.children(n, true)?))
            }
            "a" => {
                let href = self.attr(n, "href").ok_or("<a> requires href")?;
                if !safe_url(href) {
                    return Err("unsupported hyperlink URL scheme".into());
                }
                Ok(format!(
                    "<a href=\"{}\">{}</a>",
                    escape_attr(href),
                    self.children(n, true)?
                ))
            }
            "br" => Ok(format!(
                "<br{}/>",
                self.attrs(n, &[("data-docx-break", "type"), ("type", "type")])
            )),
            "img" | "image" => {
                if !inline {
                    return Err("images require a paragraph".into());
                }
                let attrs = self.attrs(
                    n,
                    &[
                        ("src", "src"),
                        ("width", "w"),
                        ("height", "h"),
                        ("w", "w"),
                        ("h", "h"),
                        ("alt", "alt"),
                    ],
                );
                Ok(format!("<image{attrs}/>"))
            }
            "math" => {
                if !inline {
                    return Err("math requires a paragraph".into());
                }
                let math = serialize_math(self.doc, n)?;
                super::mathml_omml::validate_mathml(&math)?;
                Ok(format!("<equation>{}</equation>", escape_text(&math)))
            }
            // Compatibility is input-only. Canonical reads never emit these tags.
            "equation" => {
                let text = self
                    .attr(n, "latex")
                    .map(str::to_owned)
                    .unwrap_or_else(|| self.doc.text_content(n));
                let math = if text.trim_start().starts_with("<math") {
                    text
                } else {
                    latex2mathml::latex_to_mathml(&text, latex2mathml::DisplayStyle::Inline)
                        .map_err(|e| format!("legacy equation: {e}"))?
                        .replace("<math ", "<math data-docx-legacy-display=\"auto\" ")
                };
                Ok(format!("<equation>{}</equation>", escape_text(&math)))
            }
            "field" | "format" | "footnote" | "endnote" => {
                let attrs = self.attrs(
                    n,
                    &[
                        ("instr", "instr"),
                        ("pending", "pending"),
                        ("id", "id"),
                        ("author", "author"),
                    ],
                );
                Ok(format!(
                    "<{name}{attrs}>{}</{name}>",
                    self.children(n, true)?
                ))
            }
            "comment-start" | "comment-end" => {
                Ok(format!("<{name}{}/>", self.attrs(n, &[("id", "id")])))
            }
            _ => Err(format!("unsupported HTML element <{name}>")),
        }
    }
    fn list_paragraph(&self, text: &str) -> String {
        let l = self.lists.last().expect("list context");
        format!(
            "<p list-id=\"{}\" list-level=\"{}\" list-format=\"{}\" list-start=\"{}\">{text}</p>",
            l.id, l.level, l.format, l.start
        )
    }
}

pub fn safe_url(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    if value.chars().any(char::is_control) {
        return false;
    }
    !value.contains(':')
        || ["https:", "http:", "mailto:", "tel:"]
            .iter()
            .any(|s| value.starts_with(s))
}

pub fn serialize_math(doc: &Document, node: NodeId) -> Result<String, String> {
    let Some(name) = doc.node_name(node) else {
        if !matches!(
            doc.node(node).kind,
            NodeKind::Text { .. } | NodeKind::CData { .. }
        ) {
            return Ok(String::new());
        }
        return Ok(doc.node_text(node).map(escape_text).unwrap_or_default());
    };
    let mut out = format!("<{name}");
    if name == "math" {
        out.push_str(&format!(" xmlns=\"{}\"", super::omml_mathml::MATH_NS));
    }
    for a in doc.attributes(node) {
        if a.name != "xmlns" {
            out.push_str(&format!(" {}=\"{}\"", a.name, escape_attr(&a.value)));
        }
    }
    out.push('>');
    for child in doc.children(node) {
        out.push_str(&serialize_math(doc, child)?);
    }
    out.push_str(&format!("</{name}>"));
    Ok(out)
}

/// Normalize native HTML MathML syntax (including entities) at the typed API.
pub fn canonical_mathml(input: &str) -> Result<String, String> {
    let parsed = parse_html5_full_with_options(
        input,
        &Html5ParseOptions {
            scripting: false,
            fragment_context: Some("body".into()),
        },
    );
    if let Some(error) = parsed.errors.first() {
        return Err(format!("invalid MathML HTML: {error:?}"));
    }
    let doc = &parsed.document;
    let root = doc.root_element().ok_or("MathML requires a math root")?;
    let nodes: Vec<_> = doc.children(root).filter(|&n| doc.is_element(n)).collect();
    if nodes.len() != 1
        || doc.node_name(nodes[0]) != Some("math")
        || doc.children(root).any(|n| {
            matches!(
                doc.node(n).kind,
                NodeKind::Text { .. } | NodeKind::CData { .. }
            ) && doc.node_text(n).is_some_and(|t| !t.trim().is_empty())
        })
    {
        return Err("MathML requires exactly one <math> root".into());
    }
    let math = serialize_math(doc, nodes[0])?;
    super::mathml_omml::validate_mathml(&math)?;
    Ok(math)
}
