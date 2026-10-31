//! Remove presentation-only attributes without changing text, structure or source addresses.
//! This scans generated start tags, respecting quoted values; it never rewrites
//! text or attribute contents (field instructions can contain markup-like text).
pub fn model_html(html: &str) -> String {
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut from = 0;
    let mut pos = 0;
    while pos < bytes.len() {
        if bytes[pos] != b'<' || !bytes.get(pos + 1).is_some_and(u8::is_ascii_alphabetic) {
            pos += 1;
            continue;
        }
        out.push_str(&html[from..pos]);
        let start = pos;
        pos += 1;
        while pos < bytes.len() && !bytes[pos].is_ascii_whitespace() && bytes[pos] != b'>' {
            pos += 1;
        }
        out.push_str(&html[start..pos]);
        while pos < bytes.len() && bytes[pos] != b'>' {
            let attr_start = pos;
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            let name_start = pos;
            while pos < bytes.len()
                && !bytes[pos].is_ascii_whitespace()
                && !matches!(bytes[pos], b'=' | b'>' | b'/')
            {
                pos += 1;
            }
            let name = &html[name_start..pos];
            if name.is_empty() {
                if pos < bytes.len() && bytes[pos] != b'>' {
                    pos += 1;
                }
                out.push_str(&html[attr_start..pos]);
                continue;
            }
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos < bytes.len() && bytes[pos] == b'=' {
                pos += 1;
                while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                if pos < bytes.len() && matches!(bytes[pos], b'\'' | b'"') {
                    let quote = bytes[pos];
                    pos += 1;
                    while pos < bytes.len() && bytes[pos] != quote {
                        pos += 1;
                    }
                    if pos < bytes.len() {
                        pos += 1;
                    }
                } else {
                    while pos < bytes.len()
                        && !bytes[pos].is_ascii_whitespace()
                        && bytes[pos] != b'>'
                    {
                        pos += 1;
                    }
                }
            }
            // HTML parsing assigns the MathML namespace to <math> and its
            // children automatically. Standalone equation reads retain xmlns.
            let math_namespace = name == "xmlns"
                && html[attr_start..pos].contains("http://www.w3.org/1998/Math/MathML");
            if !matches!(name, "data-docx-ord" | "contenteditable") && !math_namespace {
                out.push_str(&html[attr_start..pos]);
            }
        }
        if pos < bytes.len() {
            out.push('>');
            pos += 1;
        }
        from = pos;
    }
    out.push_str(&html[from..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_literal_text_and_attribute_values() {
        let html = "<p id=\"ABC\" data-docx-ord=\"1\">😀 data-docx-ord=\"9\" <span data-docx-field=\"REF &quot;data-docx-ord=&quot; &gt; value\" contenteditable=\"false\">Field</span></p>";
        assert_eq!(model_html(html), "<p id=\"ABC\">😀 data-docx-ord=\"9\" <span data-docx-field=\"REF &quot;data-docx-ord=&quot; &gt; value\">Field</span></p>");
    }
    #[test]
    fn preserves_math_and_significant_whitespace() {
        let html = "<p id=\"A\"> A\t B\n<math xmlns=\"http://www.w3.org/1998/Math/MathML\"><mtext> x  y </mtext></math><br data-docx-break=\"page\"></p>";
        assert_eq!(
            model_html(html),
            html.replace(" xmlns=\"http://www.w3.org/1998/Math/MathML\"", "")
        );
    }
}

/// Attribute ranges from generated start tags, respecting quoted values.
/// Text and apparent attributes inside field instructions are never rewritten.
fn attributes(html: &str) -> Vec<(usize, usize, String, String, bool)> {
    tags(html)
        .into_iter()
        .flat_map(|tag| {
            let has_class = tag.attrs.iter().any(|a| a.2 == "class");
            tag.attrs
                .into_iter()
                .map(move |(s, e, n, v)| (s, e, n, v, has_class))
        })
        .collect()
}

/// A generated start tag: its name, the byte range of the whole tag, and
/// each attribute's (start, end, name, raw value).
struct Tag {
    name: String,
    start: usize,
    end: usize,
    attrs: Vec<(usize, usize, String, String)>,
}

fn tags(html: &str) -> Vec<Tag> {
    let b = html.as_bytes();
    let mut p = 0;
    let mut result = Vec::new();
    while p < b.len() {
        if b[p] != b'<' || !b.get(p + 1).is_some_and(u8::is_ascii_alphabetic) {
            p += 1;
            continue;
        }
        let tag_start = p;
        p += 1;
        while p < b.len() && !b[p].is_ascii_whitespace() && b[p] != b'>' {
            p += 1;
        }
        let tag_name = html[tag_start + 1..p].trim_end_matches('/').to_string();
        let mut tag_attrs = Vec::new();
        while p < b.len() && b[p] != b'>' {
            let start = p;
            while p < b.len() && b[p].is_ascii_whitespace() {
                p += 1;
            }
            let ns = p;
            while p < b.len() && !b[p].is_ascii_whitespace() && !matches!(b[p], b'=' | b'>' | b'/')
            {
                p += 1;
            }
            let name = &html[ns..p];
            if name.is_empty() {
                if p < b.len() && b[p] != b'>' {
                    p += 1;
                }
                continue;
            }
            while p < b.len() && b[p].is_ascii_whitespace() {
                p += 1;
            }
            let mut value = "";
            if p < b.len() && b[p] == b'=' {
                p += 1;
                while p < b.len() && b[p].is_ascii_whitespace() {
                    p += 1;
                }
                if p < b.len() && matches!(b[p], b'\'' | b'"') {
                    let q = b[p];
                    p += 1;
                    let vs = p;
                    while p < b.len() && b[p] != q {
                        p += 1;
                    }
                    value = &html[vs..p];
                    if p < b.len() {
                        p += 1;
                    }
                } else {
                    let vs = p;
                    while p < b.len() && !b[p].is_ascii_whitespace() && b[p] != b'>' {
                        p += 1;
                    }
                    value = &html[vs..p];
                }
            }
            tag_attrs.push((start, p, name.to_string(), value.to_string()));
        }
        if p < b.len() {
            p += 1;
        }
        result.push(Tag {
            name: tag_name,
            start: tag_start,
            end: p,
            attrs: tag_attrs,
        });
    }
    result
}

fn rewrite(html: &str, edits: Vec<(usize, usize, String)>) -> String {
    let mut out = String::new();
    let mut from = 0;
    for (start, end, replacement) in edits {
        out.push_str(&html[from..start]);
        out.push_str(&replacement);
        from = end;
    }
    out.push_str(&html[from..]);
    out
}

/// Short image URLs map once to canonical content-addressed URLs in the manifest.
/// They stay relative, renderable URLs; payload reads use the canonical URL.
pub(crate) fn asset_aliases(html: &str, assets: &mut [serde_json::Value]) -> String {
    use std::collections::BTreeMap;
    let mut aliases = BTreeMap::new();
    for (i, asset) in assets.iter_mut().enumerate() {
        let Some(url) = asset["url"].as_str().map(str::to_string) else {
            continue;
        };
        let ext = url.rsplit('.').next().unwrap_or("bin");
        let alias = format!("assets/i{}.{}", i + 1, ext);
        asset["url"] = serde_json::json!(alias);
        asset["asset_url"] = serde_json::json!(url);
        aliases.insert(url, alias);
    }
    let edits = attributes(html)
        .into_iter()
        .filter_map(|(s, e, n, v, _)| {
            if n != "src" {
                return None;
            }
            aliases
                .get(&v)
                .map(|alias| (s, e, format!(" src=\"{alias}\"")))
        })
        .collect();
    rewrite(html, edits)
}

#[cfg(test)]
mod alias_tests {
    use super::*;
    #[test]
    fn image_aliases_only_rewrite_src_attributes() {
        let input = "<p>assets/hash.png<img src=\"assets/hash.png\" alt=\"assets/hash.png\"></p>";
        let mut assets =
            vec![serde_json::json!({"url":"assets/hash.png","parts":["word/media/a.png"]})];
        let out = asset_aliases(input, &mut assets);
        assert_eq!(
            out,
            "<p>assets/hash.png<img src=\"assets/i1.png\" alt=\"assets/hash.png\"></p>"
        );
        assert_eq!(assets[0]["asset_url"], "assets/hash.png");
        assert_eq!(assets[0]["parts"][0], "word/media/a.png");
    }
}

/// Agent reads drop what the rest of the read already states:
/// - a paragraph's `style` lists only declarations that differ from its
///   named style in `styles` (`prop:unset` where the paragraph has no value
///   for a style declaration), and is omitted when nothing differs;
/// - `data-docx-style` is omitted when it names `default_style`;
/// - `data-docx-number` is omitted when it repeats the enclosing
///   `<li data-docx-marker>`;
/// - note links and note bodies drop their page-scoped DOM ids, which
///   `data-docx-note`/`data-docx-note-id` already identify.
///
/// Run and other element styles stay complete, so the markup is edit input
/// as it stands.
pub fn agent_html(
    html: &str,
    styles: &std::collections::BTreeMap<String, String>,
    default_style: Option<&str>,
) -> String {
    let default_attr = default_style.map(super::escape_attr);
    let mut edits = Vec::new();
    let mut previous: Option<Tag> = None;
    for tag in tags(html) {
        let get = |name: &str| {
            tag.attrs
                .iter()
                .find(|a| a.2 == name)
                .map(|a| a.3.as_str())
        };
        let paragraph = matches!(
            tag.name.as_str(),
            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        );
        let base = if paragraph {
            match get("data-docx-style") {
                Some(id) => decode_attr(id).and_then(|id| styles.get(&id)),
                None => default_style.and_then(|id| styles.get(id)),
            }
        } else {
            None
        };
        // Attributes of the start tag that immediately precedes this one.
        let adjacent = |tag_name: &str, attr: &str| {
            previous
                .as_ref()
                .filter(|p| p.end == tag.start && p.name == tag_name)
                .and_then(|p| p.attrs.iter().find(|a| a.2 == attr))
                .map(|a| a.3.clone())
        };
        for (s, e, name, value) in &tag.attrs {
            let drop = match name.as_str() {
                "style" => {
                    let (Some(base), Some(decoded)) = (base, decode_attr(value)) else {
                        continue;
                    };
                    let delta = css_delta(&decoded, base);
                    if !delta.is_empty() {
                        edits.push((*s, *e, format!(" style=\"{}\"", super::escape_attr(&delta))));
                        continue;
                    }
                    true
                }
                "data-docx-style" => paragraph && default_attr.as_deref() == Some(value),
                "data-docx-number" => {
                    tag.name == "p" && adjacent("li", "data-docx-marker").as_ref() == Some(value)
                }
                "href" => {
                    tag.name == "a"
                        && value.starts_with("#docx-")
                        && adjacent("sup", "data-docx-note").is_some()
                }
                "id" => {
                    tag.name == "div"
                        && value.starts_with("docx-")
                        && get("data-docx-note-body").is_some()
                }
                _ => false,
            };
            if drop {
                edits.push((*s, *e, String::new()));
            }
        }
        previous = Some(tag);
    }
    model_html(&rewrite(html, edits))
}

/// One view of the document read: its agent markup, the resolved style
/// catalog, and the default paragraph style.
pub struct AgentRead {
    pub markup: String,
    pub catalog: std::collections::BTreeMap<String, String>,
    pub default_style: Option<String>,
}

/// The agent read of `html`, one rendered view of `package`.
pub fn agent_read(html: &str, package: &crate::package::Package) -> AgentRead {
    let styles = super::styles::Styles::load(package);
    let catalog = styles.catalog();
    let default_style = styles.default_paragraph_style();
    let markup = agent_html(html, &catalog, default_style.as_deref());
    AgentRead {
        markup,
        catalog,
        default_style,
    }
}

/// The decoded `data-docx-style` IDs that `html` names.
pub fn style_ids(html: &str) -> std::collections::BTreeSet<String> {
    attributes(html)
        .into_iter()
        .filter(|a| a.2 == "data-docx-style")
        .filter_map(|a| decode_attr(&a.3))
        .collect()
}

/// An attribute value with its character references resolved.
fn decode_attr(value: &str) -> Option<String> {
    let fragment = format!("<x v=\"{value}\"/>");
    let doc = xmloxide::tree::Document::parse_str(&fragment).ok()?;
    let node = doc.children(doc.root()).next()?;
    doc.attribute(node, "v").map(str::to_string)
}

/// `prop:value;` pairs, splitting only on separators outside quotes.
fn css_pairs(css: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let mut quote = None;
    let mut from = 0;
    for (i, c) in css.char_indices().chain([(css.len(), ';')]) {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, ';') => {
                if let Some((k, v)) = css[from..i].split_once(':') {
                    out.insert(k.to_string(), v.to_string());
                }
                from = i + 1;
            }
            _ => {}
        }
    }
    out
}

/// Declarations of `effective` that `base` does not already give. A base
/// declaration absent from `effective` is reset with `unset`, so
/// `base` overlaid with the result is exactly `effective`.
fn css_delta(effective: &str, base: &str) -> String {
    let effective = css_pairs(effective);
    let base = css_pairs(base);
    let mut delta: std::collections::BTreeMap<&str, &str> = effective
        .iter()
        .filter(|(k, v)| base.get(*k) != Some(v))
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    for k in base.keys() {
        if !effective.contains_key(k) {
            delta.insert(k, "unset");
        }
    }
    delta.into_iter().map(|(k, v)| format!("{k}:{v};")).collect()
}

#[cfg(test)]
mod agent_tests {
    #[test]
    fn keeps_run_styles_inline_and_preserves_style_identity_and_literals() {
        let html = super::agent_html("<h1 data-docx-style=\"Heading1\" style=\"font-family:&quot;A &amp; B&quot;\">Title</h1><span style=\"font-family:&quot;A &amp; B&quot;\">Body</span><span data-docx-field='REF style=\"literal\"'>x</span>", &Default::default(), None);
        assert!(html.contains("<h1 data-docx-style=\"Heading1\" style=\"font-family:&quot;A &amp; B&quot;\">Title"), "{html}");
        assert!(html.contains("<span style=\"font-family:&quot;A &amp; B&quot;\">Body"), "{html}");
        assert!(html.contains("data-docx-field='REF style=\"literal\"'"));
        assert!(!html.contains("<style"));
    }

    #[test]
    fn paragraph_rules_are_differences_from_the_named_style() {
        let styles = [
            ("Normal", "color:#000000;font-size:12pt;line-height:1;"),
            ("Quote", "font-family:\"A;B\";font-style:italic;"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let html = super::agent_html(
            concat!(
                "<p id=\"A\" data-docx-style=\"Normal\" style=\"color:#000000;font-size:12pt;line-height:1;\">same</p>",
                "<p id=\"B\" data-docx-style=\"Normal\" style=\"color:#000000;font-size:12pt;line-height:2;\">double</p>",
                "<p id=\"C\" style=\"font-size:12pt;line-height:1;\">no colour</p>",
                "<p id=\"D\" data-docx-style=\"Quote\" style=\"font-family:&quot;A;B&quot;;font-style:italic;\"><span style=\"color:#000000;\">run</span></p>",
            ),
            &styles,
            Some("Normal"),
        );
        assert_eq!(
            html,
            concat!(
                "<p id=\"A\">same</p>",
                "<p id=\"B\" style=\"line-height:2;\">double</p>",
                "<p id=\"C\" style=\"color:unset;\">no colour</p>",
                // Runs have no named style: their styles stay complete.
                "<p id=\"D\" data-docx-style=\"Quote\"><span style=\"color:#000000;\">run</span></p>",
            )
        );
    }

    #[test]
    fn repeated_list_numbers_and_note_dom_ids_are_dropped() {
        let html = super::agent_html(
            concat!(
                "<ol data-docx-list=\"1\" data-docx-level=\"0\" start=\"1\" type=\"1\"><li data-docx-marker=\"1.\"><p id=\"A\" data-docx-number=\"1.\">x",
                "<sup data-docx-note=\"footnote\" data-docx-note-id=\"7\"><a href=\"#docx-0a-footnote-7\">1</a></sup></p>",
                "<p id=\"B\" data-docx-number=\"9.\">kept</p><a href=\"#docx-literal\">link</a></li></ol>",
                "<aside data-docx-notes=\"footnote\"><div id=\"docx-0a-footnote-7\" data-docx-note-body=\"footnote\" data-docx-note-id=\"7\"><p>n</p></div></aside>",
            ),
            &Default::default(),
            None,
        );
        assert_eq!(
            html,
            concat!(
                "<ol data-docx-list=\"1\" data-docx-level=\"0\" start=\"1\" type=\"1\"><li data-docx-marker=\"1.\"><p id=\"A\">x",
                "<sup data-docx-note=\"footnote\" data-docx-note-id=\"7\"><a>1</a></sup></p>",
                "<p id=\"B\" data-docx-number=\"9.\">kept</p><a href=\"#docx-literal\">link</a></li></ol>",
                "<aside data-docx-notes=\"footnote\"><div data-docx-note-body=\"footnote\" data-docx-note-id=\"7\"><p>n</p></div></aside>",
            )
        );
    }
}
