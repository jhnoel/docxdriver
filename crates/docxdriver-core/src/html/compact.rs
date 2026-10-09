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
    factor_styles(&out)
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
    let b = html.as_bytes();
    let mut p = 0;
    let mut result = Vec::new();
    while p < b.len() {
        if b[p] != b'<' || !b.get(p + 1).is_some_and(u8::is_ascii_alphabetic) {
            p += 1;
            continue;
        }
        p += 1;
        while p < b.len() && !b[p].is_ascii_whitespace() && b[p] != b'>' {
            p += 1;
        }
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
        let has_class = tag_attrs.iter().any(|a| a.2 == "class");
        result.extend(
            tag_attrs
                .into_iter()
                .map(|(s, e, n, v)| (s, e, n, v, has_class)),
        );
        if p < b.len() {
            p += 1;
        }
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

/// Keep small or unique styles inline; share repeated declarations only when
/// the definition plus all class references is substantially smaller.
fn factor_styles(html: &str) -> String {
    use std::collections::BTreeMap;
    let attrs = attributes(html);
    let mut counts = BTreeMap::<String, usize>::new();
    for (_, _, name, value, has_class) in &attrs {
        if name == "style" && !has_class {
            *counts.entry(value.clone()).or_default() += 1;
        }
    }
    let mut shared = BTreeMap::new();
    let mut css = String::new();
    let mut rules = Vec::new();
    for (style, count) in counts {
        let class = format!("dxs{}", shared.len() + 1);
        let fragment = format!("<x style=\"{style}\"/>");
        let Ok(doc) = xmloxide::tree::Document::parse_str(&fragment) else {
            continue;
        };
        let Some(node) = doc.children(doc.root()).next() else {
            continue;
        };
        let Some(decoded) = doc.attribute(node, "style") else {
            continue;
        };
        let decoded = decoded.replace('<', "\\3c ");
        let rule = format!(".docxdriver .{class}{{{decoded}}}");
        let before = count * (style.len() + 9);
        let after = count * (class.len() + 9) + rule.len() + 130;
        if count > 1 && after * 5 < before * 4 {
            css.push_str(&rule);
            rules.push((class.clone(), decoded));
            shared.insert(style, class);
        }
    }
    if shared.is_empty() {
        return html.into();
    }
    use sha2::{Digest, Sha256};
    let scope = format!(
        "dxscope{}",
        &hex::encode(Sha256::digest(css.as_bytes()))[..24]
    );
    let css = rules
        .into_iter()
        .map(|(class, declaration)| format!(".{scope} .{class}{{{declaration}}}"))
        .collect::<String>();
    let edits = attrs
        .into_iter()
        .filter_map(|(s, e, n, v, has_class)| {
            if n != "style" || has_class {
                return None;
            }
            shared
                .get(&v)
                .map(|class| (s, e, format!(" class=\"{class}\"")))
        })
        .collect();
    format!(
        "<div class=\"docxdriver {scope}\"><style data-docx-styles=\"true\">{css}</style>{}</div>",
        rewrite(html, edits)
    )
}

#[cfg(test)]
mod factoring_tests {
    use super::*;
    #[test]
    fn repeated_styles_preserve_values_and_quoted_instruction_literals() {
        let style =
            "font-family:&quot;A &amp; B&quot;;font-size:12pt;color:#123456;text-align:left";
        let input = format!(
            "{}<span data-docx-field='REF style=\"literal\"'>literal</span>",
            (0..20)
                .map(|i| format!("<p id=\"{i}\" style=\"{style}\">😀 {i}</p>"))
                .collect::<String>()
        );
        let out = model_html(&input);
        assert!(out.len() < input.len());
        assert!(out
            .contains(".dxs1{font-family:\"A & B\";font-size:12pt;color:#123456;text-align:left}"));
        assert_eq!(out.matches("class=\"dxs1\"").count(), 20);
        assert!(out.contains("data-docx-field='REF style=\"literal\"'"));
        assert!(out.contains("😀 19"));
        assert_eq!(
            model_html("<p style=\"color:red\">one</p>"),
            "<p style=\"color:red\">one</p>"
        );
    }
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

/// Agent reads keep all presentation declarations out of the markup. Rule keys
/// are separate from DOCX style IDs: direct formatting is not a named style.
pub fn agent_html(html: &str) -> (String, std::collections::BTreeMap<String, String>) {
    let mut rules = std::collections::BTreeMap::new();
    let mut keys = std::collections::BTreeMap::<String, String>::new();
    let edits = attributes(html).into_iter().filter_map(|(s, e, name, value, _)| {
        if name != "style" { return None; }
        let key = if let Some(key) = keys.get(&value) { key.clone() } else {
            let key = format!("dx{}", keys.len() + 1);
            let fragment = format!("<x style=\"{value}\"/>");
            let doc = xmloxide::tree::Document::parse_str(&fragment).ok()?;
            let node = doc.children(doc.root()).next()?;
            rules.insert(key.clone(), doc.attribute(node, "style")?.to_string());
            keys.insert(value, key.clone());
            key
        };
        Some((s, e, format!(" data-docx-css=\"{key}\"")))
    }).collect();
    // With styles removed, model_html cannot introduce an embedded style block.
    (model_html(&rewrite(html, edits)), rules)
}

#[cfg(test)]
mod agent_tests {
    #[test]
    fn separates_css_and_preserves_style_identity_and_literals() {
        let (html, css) = super::agent_html("<h1 data-docx-style=\"Heading1\" style=\"font-family:&quot;A &amp; B&quot;\">Title</h1><p style=\"font-family:&quot;A &amp; B&quot;\">Body</p><span data-docx-field='REF style=\"literal\"'>x</span>");
        assert_eq!(css.len(), 1);
        assert_eq!(css["dx1"], "font-family:\"A & B\"");
        assert!(html.contains("data-docx-style=\"Heading1\""));
        assert_eq!(html.matches("data-docx-css=\"dx1\"").count(), 2);
        assert!(html.contains("data-docx-field='REF style=\"literal\"'"));
        assert!(!html.contains("<style"));
    }
}
