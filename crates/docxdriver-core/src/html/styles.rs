//! Bounded HTML/CSS formatting and the DOCX style cascade.
use super::{escape_attr, Fmt, VertAlign};
use crate::{document::DocxXml, package::Package};
use std::collections::{BTreeMap, HashMap, HashSet};
use xmloxide::tree::NodeId;

pub const CSS: &str = r#".docxdriver { font-family: Calibri, Arial, sans-serif; line-height: 1.2; color: #111; }
.docxdriver p, .docxdriver h1, .docxdriver h2, .docxdriver h3, .docxdriver h4, .docxdriver h5, .docxdriver h6 { white-space: pre-wrap; tab-size: 4; min-height: 1em; }
.docxdriver p { margin: 0 0 .6em; }
.docxdriver table { border-collapse: collapse; max-width: 100%; }
.docxdriver td, .docxdriver th { border: 1px solid #aaa; padding: .3em .5em; vertical-align: top; }
.docxdriver img { max-width: 100%; }
.docxdriver ins { color: #067a35; text-decoration: underline; }
.docxdriver del { color: #b32323; text-decoration: line-through; }
.docxdriver [data-docx-format-change] { background: #fff1b8; }
.docxdriver [data-docx-field] { background: #f2f4f7; }
.docxdriver ol, .docxdriver ul { padding-left: 2em; }
.docxdriver li > p { margin-bottom: .25em; }
.docxdriver [data-docx-marker]::marker { content: attr(data-docx-marker) " "; }
.docxdriver aside { border-top: 1px solid #ddd; margin-top: 1em; font-size: .9em; }
.docxdriver [data-docx-break="page"] { break-after: page; }
.docxdriver [data-docx-break="column"] { break-after: column; }
.docxdriver header, .docxdriver footer { color: #555; }
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontFamily {
    bytes: [u8; 128],
    len: u8,
}
impl FontFamily {
    pub fn parse(value: &str) -> Result<Self, String> {
        let value = value.trim().trim_matches(['\'', '"']);
        if value.len() > 127
            || value
                .chars()
                .any(|c| c.is_control() || matches!(c, ';' | '<' | '>' | '{' | '}' | '\\'))
        {
            return Err("invalid font family".into());
        }
        let mut bytes = [0; 128];
        bytes[..value.len()].copy_from_slice(value.as_bytes());
        Ok(Self {
            bytes,
            len: value.len() as u8,
        })
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as usize]).expect("validated UTF-8")
    }
}

pub fn declarations(css: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for part in css.split(';').map(str::trim).filter(|v| !v.is_empty()) {
        let (name, value) = part
            .split_once(':')
            .ok_or("CSS declarations require property: value")?;
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty()
            || value
                .chars()
                .any(|c| c.is_control() || matches!(c, '<' | '>' | '{' | '}'))
            || value.to_ascii_lowercase().contains("url(")
            || value.contains('!')
        {
            return Err("unsupported CSS value".into());
        }
        out.insert(name, value.to_string());
    }
    Ok(out)
}
pub fn points(value: &str) -> Result<f64, String> {
    let value = value.trim();
    let (number, scale) = if let Some(v) = value.strip_suffix("pt") {
        (v, 1.0)
    } else if let Some(v) = value.strip_suffix("px") {
        (v, 0.75)
    } else if value == "0" {
        (value, 1.0)
    } else {
        return Err("lengths require pt or px".into());
    };
    let n = number.parse::<f64>().map_err(|_| "invalid CSS length")? * scale;
    if !n.is_finite() || n < 0.0 || n > 100000.0 {
        return Err("CSS length out of range".into());
    }
    Ok(n)
}
pub fn apply_inline_css(mut fmt: Fmt, css: &str) -> Result<Fmt, String> {
    for (name, value) in declarations(css)? {
        match name.as_str() {
            "font-weight" => {
                fmt.resets |= 1;
                fmt.bold = match value.as_str() {
                    "bold" | "bolder" | "600" | "700" | "800" | "900" => true,
                    "normal" | "400" => false,
                    _ => return Err("unsupported font-weight".into()),
                }
            }
            "font-style" => {
                fmt.resets |= 2;
                fmt.italic = match value.as_str() {
                    "italic" | "oblique" => true,
                    "normal" => false,
                    _ => return Err("unsupported font-style".into()),
                }
            }
            "text-decoration" | "text-decoration-line" => {
                fmt.resets |= 4 | 8;
                fmt.underline = value.split_whitespace().any(|v| v == "underline");
                fmt.strike = value.split_whitespace().any(|v| v == "line-through");
                if !value
                    .split_whitespace()
                    .all(|v| matches!(v, "none" | "underline" | "line-through"))
                {
                    return Err("unsupported text-decoration".into());
                }
            }
            "vertical-align" => {
                fmt.resets |= 16;
                fmt.vert_align = match value.as_str() {
                    "super" => Some(VertAlign::Superscript),
                    "sub" => Some(VertAlign::Subscript),
                    "baseline" => None,
                    _ => return Err("unsupported vertical-align".into()),
                }
            }
            "color" => {
                let hex = value
                    .strip_prefix('#')
                    .ok_or("colors require #RGB or #RRGGBB")?;
                let hex = if hex.len() == 3 {
                    hex.chars().flat_map(|c| [c, c]).collect::<String>()
                } else {
                    hex.to_owned()
                };
                if hex.len() != 6 {
                    return Err("invalid color".into());
                }
                fmt.color = Some(u32::from_str_radix(&hex, 16).map_err(|_| "invalid color")?);
            }
            "font-size" => {
                let size = points(&value)? * 2.0;
                if size.fract().abs() > 0.00001 || size < 1.0 {
                    return Err("font-size must be a positive multiple of .5pt".into());
                }
                fmt.font_size = Some(size as u32);
            }
            "font-family" => fmt.font_family = Some(FontFamily::parse(&value)?),
            "white-space" if matches!(value.as_str(), "pre-wrap" | "normal" | "pre") => {}
            _ => return Err(format!("unsupported inline CSS property {name}")),
        }
    }
    Ok(fmt)
}
pub fn inline_css(fmt: Fmt) -> String {
    let mut out = String::new();
    if fmt.resets & 1 != 0 && !fmt.bold {
        out.push_str("font-weight:normal;");
    }
    if fmt.resets & 2 != 0 && !fmt.italic {
        out.push_str("font-style:normal;");
    }
    if fmt.resets & 12 != 0 && !fmt.underline && !fmt.strike {
        out.push_str("text-decoration:none;");
    }
    if fmt.resets & 16 != 0 && fmt.vert_align.is_none() {
        out.push_str("vertical-align:baseline;");
    }
    if let Some(color) = fmt.color {
        out.push_str(&format!("color:#{color:06X};"));
    }
    if let Some(size) = fmt.font_size {
        out.push_str(&format!("font-size:{}pt;", size as f64 / 2.0));
    }
    if let Some(font) = fmt.font_family {
        out.push_str(&format!("font-family:{};", font.as_str()));
    }
    out
}

/// Remove run properties already inherited from the paragraph. These
/// declarations stay on the run when CSS inheritance would change their
/// meaning (for example, font size on superscript text).
pub fn without_inherited_css(mut fmt: Fmt, inherited: &BTreeMap<String, String>) -> Fmt {
    if let Some(color) = fmt.color {
        if inherited
            .get("color")
            .is_some_and(|value| value == &format!("#{color:06X}"))
        {
            fmt.color = None;
        }
    }
    if let Some(size) = fmt.font_size {
        let value = format!("{}pt", size as f64 / 2.0);
        if fmt.vert_align.is_none()
            && inherited
                .get("font-size")
                .is_some_and(|inherited| inherited == &value)
        {
            fmt.font_size = None;
        }
    }
    if let Some(font) = fmt.font_family {
        if inherited
            .get("font-family")
            .is_some_and(|value| value == font.as_str())
        {
            fmt.font_family = None;
        }
    }
    if fmt.resets & 1 != 0
        && !fmt.bold
        && inherited
            .get("font-weight")
            .is_some_and(|value| value == "normal")
    {
        fmt.resets &= !1;
    }
    if fmt.resets & 2 != 0
        && !fmt.italic
        && inherited
            .get("font-style")
            .is_some_and(|value| value == "normal")
    {
        fmt.resets &= !2;
    }
    fmt
}

/// Merge common run declarations into a paragraph's existing declarations.
/// Run formatting takes precedence if the same property appears in both.
pub fn merge_paragraph_css(paragraph_css: &str, run_css: &BTreeMap<String, String>) -> String {
    let mut map = declarations(paragraph_css).unwrap_or_default();
    map.extend(
        run_css
            .iter()
            .map(|(name, value)| (name.clone(), value.clone())),
    );
    css_string(map)
}

pub fn extra_rpr(fmt: Fmt) -> String {
    let mut out = String::new();
    for (name, value) in disabled_properties(fmt) {
        out.push_str(&format!("<w:{name} w:val=\"{value}\"/>"));
    }
    if let Some(color) = fmt.color {
        out.push_str(&format!("<w:color w:val=\"{color:06X}\"/>"));
    }
    if let Some(size) = fmt.font_size {
        out.push_str(&format!("<w:sz w:val=\"{size}\"/>"));
    }
    if let Some(font) = fmt.font_family {
        out.push_str(&format!(
            "<w:rFonts w:ascii=\"{}\" w:hAnsi=\"{}\"/>",
            escape_attr(font.as_str()),
            escape_attr(font.as_str())
        ));
    }
    out
}
fn disabled_properties(fmt: Fmt) -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    for (bit, enabled, name, value) in [
        (1, fmt.bold, "b", "0"),
        (2, fmt.italic, "i", "0"),
        (4, fmt.underline, "u", "none"),
        (8, fmt.strike, "strike", "0"),
        (16, fmt.vert_align.is_some(), "vertAlign", "baseline"),
    ] {
        if fmt.resets & bit != 0 && !enabled {
            out.push((name, value));
        }
    }
    out
}
pub fn append_extra_rpr(xml: &mut DocxXml, rpr: NodeId, fmt: Fmt) {
    for (name, value) in disabled_properties(fmt) {
        let n = xml.doc.create_element(&format!("w:{name}"));
        xml.doc.set_attribute(n, "w:val", value);
        xml.doc.append_child(rpr, n);
    }
    if let Some(color) = fmt.color {
        let n = xml.doc.create_element("w:color");
        xml.doc.set_attribute(n, "w:val", &format!("{color:06X}"));
        xml.doc.append_child(rpr, n);
    }
    if let Some(size) = fmt.font_size {
        let n = xml.doc.create_element("w:sz");
        xml.doc.set_attribute(n, "w:val", &size.to_string());
        xml.doc.append_child(rpr, n);
    }
    if let Some(font) = fmt.font_family {
        let n = xml.doc.create_element("w:rFonts");
        xml.doc.set_attribute(n, "w:ascii", font.as_str());
        xml.doc.set_attribute(n, "w:hAnsi", font.as_str());
        xml.doc.append_child(rpr, n);
    }
}

pub fn paragraph_properties(css: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut spacing = BTreeMap::new();
    let mut indent = BTreeMap::new();
    for (name, value) in declarations(css)? {
        match name.as_str() {
            "text-align" if matches!(value.as_str(), "left" | "right" | "center" | "justify") => {
                out.push_str(&format!(
                    "<w:jc w:val=\"{}\"/>",
                    if value == "justify" { "both" } else { &value }
                ))
            }
            "margin-top" => {
                spacing.insert("before", (points(&value)? * 20.0).round() as u32);
            }
            "margin-bottom" => {
                spacing.insert("after", (points(&value)? * 20.0).round() as u32);
            }
            "margin-left" => {
                indent.insert("left", (points(&value)? * 20.0).round() as u32);
            }
            "margin-right" => {
                indent.insert("right", (points(&value)? * 20.0).round() as u32);
            }
            "text-indent" => {
                indent.insert("firstLine", (points(&value)? * 20.0).round() as u32);
            }
            "line-height" => {
                let multiple = value
                    .parse::<f64>()
                    .map_err(|_| "line-height must be a positive number")?;
                if !multiple.is_finite() || !(0.1..=100.0).contains(&multiple) {
                    return Err("invalid line-height".into());
                }
                spacing.insert("line", (multiple * 240.0).round() as u32);
            }
            // Run properties are inherited by the paragraph's content.
            "color" | "font-size" | "font-family" | "font-weight" | "font-style"
            | "text-decoration" | "vertical-align" | "white-space" => {}
            _ => return Err(format!("unsupported paragraph CSS property {name}")),
        }
    }
    for (tag, values) in [("spacing", spacing), ("ind", indent)] {
        if !values.is_empty() {
            out.push_str(&format!("<w:{tag}"));
            for (k, v) in values {
                out.push_str(&format!(" w:{k}=\"{v}\""));
            }
            out.push_str("/>");
        }
    }
    Ok(out)
}

#[derive(Default)]
pub struct Styles {
    xml: Option<DocxXml>,
    nodes: HashMap<String, NodeId>,
}
impl Styles {
    pub fn load(package: &Package) -> Self {
        let main = package.document_part_name();
        let dir = main
            .rsplit_once('/')
            .map(|(d, _)| format!("{d}/"))
            .unwrap_or_default();
        let xml = package
            .get(&format!("{dir}styles.xml"))
            .and_then(|b| DocxXml::parse(b).ok());
        let nodes = xml
            .as_ref()
            .map(|x| {
                x.doc
                    .descendants(x.doc.root())
                    .filter(|&n| x.is_w(n, "style"))
                    .filter_map(|n| x.attr(n, "styleId").map(|id| (id.to_string(), n)))
                    .collect()
            })
            .unwrap_or_default();
        Self { xml, nodes }
    }
    pub fn paragraph_style(&self, x: &DocxXml, n: NodeId, original: bool) -> Option<String> {
        if let Some(id) = property_bag(x, n, "pPr", original)
            .and_then(|p| x.child_w(p, "pStyle"))
            .and_then(|p| x.attr(p, "val"))
        {
            return Some(id.into());
        }
        let styles = self.xml.as_ref()?;
        styles
            .doc
            .descendants(styles.doc.root())
            .find(|&n| {
                styles.is_w(n, "style")
                    && styles.attr(n, "type") == Some("paragraph")
                    && matches!(styles.attr(n, "default"), Some("1" | "true"))
            })
            .and_then(|n| styles.attr(n, "styleId"))
            .map(str::to_owned)
    }
    fn chain(&self, id: Option<&str>, run: bool, out: &mut BTreeMap<String, String>) {
        let Some(x) = &self.xml else { return };
        let mut ids = Vec::new();
        let mut id = id.map(str::to_owned);
        let mut seen = HashSet::new();
        while let Some(i) = id {
            if !seen.insert(i.clone()) {
                break;
            }
            let Some(&n) = self.nodes.get(&i) else { break };
            ids.push(n);
            id = x
                .child_w(n, "basedOn")
                .and_then(|n| x.attr(n, "val"))
                .map(str::to_owned);
        }
        for n in ids.into_iter().rev() {
            if let Some(p) = x.child_w(n, if run { "rPr" } else { "pPr" }) {
                collect_css(x, p, run, out);
            }
        }
    }
    pub fn paragraph_css(&self, x: &DocxXml, n: NodeId, original: bool) -> String {
        let mut map = BTreeMap::new();
        self.defaults(false, &mut map);
        self.chain(
            self.paragraph_style(x, n, original).as_deref(),
            false,
            &mut map,
        );
        if let Some(p) = property_bag(x, n, "pPr", original) {
            collect_css(x, p, false, &mut map);
        }
        css_string(map)
    }
    /// Resolved style CSS, keyed by the exact OOXML style ID.
    pub fn catalog(&self) -> std::collections::BTreeMap<String, String> {
        self.nodes.keys().map(|id| {
            let mut map = BTreeMap::new();
            self.defaults(false, &mut map);
            self.chain(Some(id), false, &mut map);
            self.defaults(true, &mut map);
            self.chain(Some(id), true, &mut map);
            (id.clone(), css_string(map))
        }).collect()
    }
    fn defaults(&self, run: bool, out: &mut BTreeMap<String, String>) {
        if let Some(x) = &self.xml {
            if let Some(n) = x
                .doc
                .descendants(x.doc.root())
                .find(|&n| x.is_w(n, if run { "rPrDefault" } else { "pPrDefault" }))
            {
                if let Some(p) = x.child_w(n, if run { "rPr" } else { "pPr" }) {
                    collect_css(x, p, run, out);
                }
            }
        }
    }
    pub fn run_format(&self, x: &DocxXml, n: NodeId, original: bool) -> Fmt {
        let mut map = BTreeMap::new();
        self.defaults(true, &mut map);
        let mut parent = x.doc.parent(n);
        while let Some(p) = parent {
            if x.is_w(p, "p") {
                self.chain(
                    self.paragraph_style(x, p, original).as_deref(),
                    true,
                    &mut map,
                );
                break;
            }
            parent = x.doc.parent(p);
        }
        if let Some(p) = property_bag(x, n, "rPr", original) {
            let id = x.child_w(p, "rStyle").and_then(|n| x.attr(n, "val"));
            self.chain(id, true, &mut map);
            collect_css(x, p, true, &mut map);
        }
        // Values collected from OOXML are bounded by their respective properties.
        apply_inline_css(Fmt::default(), &css_string(map)).unwrap_or_default()
    }
}
fn css_string(map: BTreeMap<String, String>) -> String {
    map.into_iter().map(|(k, v)| format!("{k}:{v};")).collect()
}
fn collect_css(x: &DocxXml, p: NodeId, run: bool, map: &mut BTreeMap<String, String>) {
    for n in x.doc.children(p) {
        let name = x
            .doc
            .node_name(n)
            .unwrap_or("")
            .rsplit(':')
            .next()
            .unwrap_or("");
        let val = x.attr(n, "val").unwrap_or("");
        if run {
            match name {
                "b" => {
                    map.insert(
                        "font-weight".into(),
                        if matches!(val, "0" | "false") {
                            "normal"
                        } else {
                            "bold"
                        }
                        .into(),
                    );
                }
                "i" => {
                    map.insert(
                        "font-style".into(),
                        if matches!(val, "0" | "false") {
                            "normal"
                        } else {
                            "italic"
                        }
                        .into(),
                    );
                }
                "u" | "strike" => {
                    let token = if name == "u" {
                        "underline"
                    } else {
                        "line-through"
                    };
                    let old = map.get("text-decoration").cloned().unwrap_or_default();
                    let mut decoration: Vec<_> = old
                        .split_whitespace()
                        .filter(|v| *v != token && *v != "none")
                        .collect();
                    if !matches!(val, "0" | "none" | "false") {
                        decoration.push(token);
                    }
                    map.insert(
                        "text-decoration".into(),
                        if decoration.is_empty() {
                            "none".into()
                        } else {
                            decoration.join(" ")
                        },
                    );
                }
                "vertAlign" => {
                    map.insert(
                        "vertical-align".into(),
                        match val {
                            "superscript" => "super",
                            "subscript" => "sub",
                            _ => "baseline",
                        }
                        .into(),
                    );
                }
                "color" if val.len() == 6 && val.chars().all(|c| c.is_ascii_hexdigit()) => {
                    map.insert("color".into(), format!("#{val}"));
                }
                "sz" => {
                    if let Ok(v) = val.parse::<u32>() {
                        map.insert("font-size".into(), format!("{}pt", v as f64 / 2.0));
                    }
                }
                "rFonts" => {
                    if let Some(v) = x.attr(n, "ascii").or_else(|| x.attr(n, "hAnsi")) {
                        if FontFamily::parse(v).is_ok() {
                            map.insert("font-family".into(), v.into());
                        }
                    }
                }
                _ => {}
            }
        } else {
            match name {
                "jc" => {
                    if matches!(val, "left" | "right" | "center" | "both") {
                        map.insert(
                            "text-align".into(),
                            if val == "both" { "justify" } else { val }.into(),
                        );
                    }
                }
                "spacing" => {
                    for (a, k) in [("before", "margin-top"), ("after", "margin-bottom")] {
                        if let Some(v) = x.attr(n, a).and_then(|v| v.parse::<u32>().ok()) {
                            map.insert(k.into(), format!("{}pt", v as f64 / 20.0));
                        }
                    }
                    if let Some(v) = x.attr(n, "line").and_then(|v| v.parse::<u32>().ok()) {
                        let rule = x.attr(n, "lineRule").unwrap_or("auto");
                        map.insert(
                            "line-height".into(),
                            if rule == "auto" {
                                (v as f64 / 240.0).to_string()
                            } else {
                                format!("{}pt", v as f64 / 20.0)
                            },
                        );
                    }
                }
                "ind" => {
                    for (a, k) in [
                        ("left", "margin-left"),
                        ("right", "margin-right"),
                        ("firstLine", "text-indent"),
                    ] {
                        if let Some(v) = x.attr(n, a).and_then(|v| v.parse::<u32>().ok()) {
                            map.insert(k.into(), format!("{}pt", v as f64 / 20.0));
                        }
                    }
                    if let Some(v) = x.attr(n, "hanging").and_then(|v| v.parse::<u32>().ok()) {
                        map.insert("text-indent".into(), format!("-{}pt", v as f64 / 20.0));
                    }
                }
                _ => {}
            }
        }
    }
}

fn property_bag(x: &DocxXml, n: NodeId, name: &str, original: bool) -> Option<NodeId> {
    let p = x.child_w(n, name)?;
    if original {
        if let Some(change) = x.child_w(p, &format!("{name}Change")) {
            return x.child_w(change, name);
        }
    }
    Some(p)
}
