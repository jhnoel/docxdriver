//! MathML → OMML conversion for equation writes.
//!
//! Adapted from kklimuk/docx-cli (`src/core/equation/mathml-to-omml.tsx`, MIT
//! License, Copyright (c) 2026 Kirill Klimuk): walk MathML Core (as produced by
//! `latex2mathml`) and emit Office Math ML suitable for `m:oMath` /
//! `m:oMathPara`.

use xmloxide::tree::{Document, NodeId};

const M_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";

/// Convert LaTeX to an `m:oMath` / `m:oMathPara` XML fragment (with `xmlns:m`).
#[cfg(test)]
pub fn latex_to_omml_xml(latex: &str, display: bool) -> Result<String, String> {
    require_braced_scripts(latex)?;
    let style = if display {
        latex2mathml::DisplayStyle::Block
    } else {
        latex2mathml::DisplayStyle::Inline
    };
    let mathml = latex2mathml::latex_to_mathml(latex, style)
        .map_err(|e| format!("invalid LaTeX equation: {e}"))?;
    let inner = mathml_string_to_omml_inner(&mathml)?;
    if display {
        Ok(format!(
            "<m:oMathPara xmlns:m=\"{M_NS}\"><m:oMath>{inner}</m:oMath></m:oMathPara>"
        ))
    } else {
        Ok(format!("<m:oMath xmlns:m=\"{M_NS}\">{inner}</m:oMath>"))
    }
}

/// Compile validated presentation MathML directly to Office Math.
pub fn mathml_to_omml_xml(mathml: &str, display: bool) -> Result<String, String> {
    validate_mathml(mathml)?;
    let inner = mathml_string_to_omml_inner(mathml)?;
    if display {
        Ok(format!(
            "<m:oMathPara xmlns:m=\"{M_NS}\"><m:oMath>{inner}</m:oMath></m:oMathPara>"
        ))
    } else {
        Ok(format!("<m:oMath xmlns:m=\"{M_NS}\">{inner}</m:oMath>"))
    }
}

pub fn validate_mathml(mathml: &str) -> Result<(), String> {
    let doc =
        Document::parse_bytes(mathml.as_bytes()).map_err(|e| format!("invalid MathML: {e}"))?;
    let root = doc.root_element().ok_or("MathML requires a <math> root")?;
    if local_name(&doc, root) != "math" {
        return Err("MathML requires a <math> root".into());
    }
    for node in doc.descendants(root).filter(|&n| doc.is_element(n)) {
        let name = local_name(&doc, node);
        let arity = match name.as_str() {
            "math" | "mrow" | "mstyle" | "semantics" | "mi" | "mn" | "mo" | "mtext" | "msqrt"
            | "mtable" | "mtr" | "mtd" | "mspace" | "annotation" | "mphantom" | "menclose"
            | "mmultiscripts" | "mprescripts" | "none" => None,
            "mfrac" | "msup" | "msub" | "munder" | "mover" | "mroot" => Some(2),
            "msubsup" | "munderover" => Some(3),
            _ => return Err(format!("unsupported MathML element <{name}>")),
        };
        if arity.is_some_and(|n| element_children(&doc, node).len() != n) {
            return Err(format!(
                "invalid MathML: <{name}> has the wrong number of operands"
            ));
        }
        if let Some(ns) = doc.node_namespace(node) {
            if ns != super::omml_mathml::MATH_NS {
                return Err("MathML contains a foreign namespace".into());
            }
        }
        if matches!(name.as_str(), "munder" | "mover") && is_accent(&doc, node) {
            let operator = accent_operator(&doc, node)
                .ok_or("unsupported MathML accent: requires one operator character")?;
            if attr(&doc, operator, "stretchy") == Some("false") {
                return Err(
                    "unsupported MathML accent: non-stretching decoration cannot be represented"
                        .into(),
                );
            }
        }
        if name == "munderover"
            && (is_accent_at(&doc, node, "accentunder", 1) || is_accent_at(&doc, node, "accent", 2))
        {
            return Err(
                "unsupported MathML combined accents: use nested munder/mover decorations".into(),
            );
        }
        if name == "mmultiscripts" {
            let kids = element_children(&doc, node);
            let pre = kids
                .iter()
                .position(|&n| local_name(&doc, n) == "mprescripts");
            let valid = match pre {
                Some(i) => (i == 1 || i == 3) && kids.len() == i + 3,
                None => kids.len() == 3,
            };
            if !valid {
                return Err(
                    "unsupported MathML multiscript shape: one pre/post pair is supported".into(),
                );
            }
        }
        for a in doc.attributes(node) {
            if !matches!(
                a.name.as_str(),
                "xmlns"
                    | "display"
                    | "mathvariant"
                    | "stretchy"
                    | "form"
                    | "accent"
                    | "accentunder"
                    | "movablelimits"
                    | "bevelled"
                    | "linethickness"
                    | "width"
                    | "notation"
                    | "encoding"
                    | "data-docx-legacy-display"
            ) {
                return Err(format!("unsupported MathML attribute {}", a.name));
            }
            if matches!(a.name.as_str(), "accent" | "accentunder" | "stretchy")
                && !matches!(a.value.as_str(), "true" | "false")
            {
                return Err(format!("unsupported MathML {} value", a.name));
            }
            if a.name == "mathvariant"
                && !matches!(
                    a.value.as_str(),
                    "normal" | "italic" | "bold" | "bold-italic"
                )
            {
                return Err("unsupported MathML mathvariant".into());
            }
            if a.name == "notation" && a.value != "box" {
                return Err("unsupported MathML enclosure notation".into());
            }

            if a.name.starts_with("on")
                || a.name == "href"
                || a.name == "src"
                || a.name == "style"
                || a.name == "data-docx-unsupported"
            {
                return Err(format!("unsupported MathML attribute {}", a.name));
            }
        }
    }
    Ok(())
}

/// A script must have an explicit extent, even when its argument is one token.
#[cfg(test)]
fn require_braced_scripts(latex: &str) -> Result<(), String> {
    let mut chars = latex.char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        if ch == '\\' {
            // Escaped punctuation is literal; control words consume their letters.
            if let Some((_, next)) = chars.next() {
                if next.is_ascii_alphabetic() {
                    let mut command = String::from(next);
                    while chars.peek().is_some_and(|(_, c)| c.is_ascii_alphabetic()) {
                        command.push(chars.next().unwrap().1);
                    }
                    if is_accent_command(&command) && chars.peek().map(|(_, c)| *c) != Some('{') {
                        return Err(format!(
                            "invalid LaTeX equation: accent \\{command} must be followed by a braced argument ({{...}})"
                        ));
                    }
                }
            }
        } else if matches!(ch, '^' | '_') && chars.peek().map(|(_, c)| *c) != Some('{') {
            return Err(format!(
                "invalid LaTeX equation: script {ch} at byte {offset} must be followed by a braced argument ({{...}})"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
fn is_accent_command(command: &str) -> bool {
    matches!(
        command,
        "hat"
            | "widehat"
            | "bar"
            | "overline"
            | "vec"
            | "dot"
            | "ddot"
            | "tilde"
            | "widetilde"
            | "acute"
            | "grave"
            | "breve"
            | "check"
            | "mathring"
            | "overrightarrow"
            | "overleftarrow"
            | "underline"
    )
}

fn mathml_string_to_omml_inner(mathml: &str) -> Result<String, String> {
    let doc = Document::parse_bytes(mathml.as_bytes())
        .map_err(|e| format!("MathML parse failed: {e}"))?;
    let root = doc
        .root_element()
        .ok_or_else(|| "MathML has no root element".to_string())?;
    let mut out = String::new();
    convert_siblings(&doc, root, &mut out);
    Ok(out)
}

fn local_name(doc: &Document, node: NodeId) -> String {
    match doc.node_name(node) {
        Some(name) => name.rsplit(':').next().unwrap_or(name).to_string(),
        None => String::new(),
    }
}

fn attr<'a>(doc: &'a Document, node: NodeId, name: &str) -> Option<&'a str> {
    doc.attribute(node, name).or_else(|| {
        // Prefixed form after parse.
        doc.attributes(node)
            .iter()
            .find(|a| a.name == name || a.name.ends_with(&format!(":{name}")))
            .map(|a| a.value.as_str())
    })
}

fn inherited_variant(doc: &Document, mut node: NodeId) -> Option<&str> {
    loop {
        if let Some(v) = attr(doc, node, "mathvariant") {
            return Some(v);
        }
        node = doc.parent(node)?;
    }
}
fn limit_location(doc: &Document, node: NodeId) -> &'static str {
    match local_name(doc, node).as_str() {
        "munder" | "mover" | "munderover" => "undOvr",
        _ => "subSup",
    }
}

fn element_children(doc: &Document, node: NodeId) -> Vec<NodeId> {
    doc.children(node).filter(|&c| doc.is_element(c)).collect()
}

fn convert_siblings(doc: &Document, parent: NodeId, out: &mut String) {
    convert_sequence(doc, &element_children(doc, parent), out);
}

fn convert_sequence(doc: &Document, children: &[NodeId], out: &mut String) {
    let mut index = 0;
    while index < children.len() {
        if let Some((op, sub, sup)) = nary_parts(doc, children[index]) {
            let mut end = index + 1;
            let mut delimiter_depth = 0usize;
            while end < children.len() {
                if local_name(doc, children[end]) == "mo" {
                    let token = clean_text(&doc.text_content(children[end]));
                    if delimiter_depth == 0 && end > index + 1 && is_nary_body_boundary(&token) {
                        break;
                    }
                    match token.as_str() {
                        "(" | "[" | "{" => delimiter_depth += 1,
                        ")" | "]" | "}" => delimiter_depth = delimiter_depth.saturating_sub(1),
                        _ => {}
                    }
                }
                end += 1;
            }
            push_nary(
                doc,
                op,
                sub,
                sup,
                &children[index + 1..end],
                limit_location(doc, children[index]),
                out,
            );
            index = end;
        } else {
            convert_dispatch(doc, children[index], out);
            index += 1;
        }
    }
}

fn nary_parts(doc: &Document, node: NodeId) -> Option<(NodeId, Option<NodeId>, Option<NodeId>)> {
    if is_nary_op(doc, node) {
        return Some((node, None, None));
    }
    if matches!(local_name(doc, node).as_str(), "munder" | "mover") && is_accent(doc, node) {
        return None;
    }
    let kids = element_children(doc, node);
    if kids.first().is_none_or(|&kid| !is_nary_op(doc, kid)) {
        return None;
    }
    match local_name(doc, node).as_str() {
        "msubsup" | "munderover" if kids.len() >= 3 => {
            Some((kids[0], Some(kids[1]), Some(kids[2])))
        }
        "msub" | "munder" if kids.len() >= 2 => Some((kids[0], Some(kids[1]), None)),
        "msup" | "mover" if kids.len() >= 2 => Some((kids[0], None, Some(kids[1]))),
        _ => None,
    }
}

fn is_nary_body_boundary(token: &str) -> bool {
    // latex2mathml emits a flat sibling sequence for large-operator operands.
    // Stop at an outer additive/relation separator, while retaining grouped
    // expressions and a leading unary sign inside the operand.
    matches!(
        token,
        "+" | "−" | "-" | "±" | "∓" | "=" | "<" | ">" | "≤" | "≥" | "," | ";"
    )
}

fn convert_dispatch(doc: &Document, node: NodeId, out: &mut String) {
    if local_name(doc, node) == "mrow" && convert_mrow_fences(doc, node, out) {
        return;
    }
    convert_node(doc, node, out);
}

fn convert_mrow_fences(doc: &Document, node: NodeId, out: &mut String) -> bool {
    let kids = element_children(doc, node);
    if kids.len() < 2 {
        return false;
    }
    let first = kids[0];
    let last = kids[kids.len() - 1];
    if local_name(doc, first) != "mo" || local_name(doc, last) != "mo" {
        return false;
    }
    let stretchy_first =
        attr(doc, first, "stretchy") == Some("true") || attr(doc, first, "form") == Some("prefix");
    let stretchy_last =
        attr(doc, last, "stretchy") == Some("true") || attr(doc, last, "form") == Some("postfix");
    if !(stretchy_first && stretchy_last) {
        return false;
    }
    let beg = clean_text(&doc.text_content(first));
    let end = clean_text(&doc.text_content(last));
    out.push_str("<m:d><m:dPr><m:begChr m:val=\"");
    out.push_str(&escape_xml(&beg));
    out.push_str("\"/><m:endChr m:val=\"");
    out.push_str(&escape_xml(&end));
    out.push_str("\"/></m:dPr><m:e>");
    convert_sequence(doc, &kids[1..kids.len() - 1], out);
    out.push_str("</m:e></m:d>");
    true
}

fn convert_node(doc: &Document, node: NodeId, out: &mut String) {
    let name = local_name(doc, node);
    match name.as_str() {
        "math" | "mrow" | "mstyle" | "semantics" => convert_siblings(doc, node, out),
        "mi" | "mn" | "mtext" | "mo" => {
            let text = clean_text(&doc.text_content(node));
            if text.is_empty() {
                return;
            }
            let variant = inherited_variant(doc, node);
            let upright = matches!(name.as_str(), "mtext" | "mo") || variant == Some("normal");
            let before = out.len();
            push_run(out, &text, upright && variant.is_none_or(|v| v == "normal"));
            if let Some(style) = variant.filter(|v| *v != "normal") {
                let val = match style {
                    "bold" => "b",
                    "bold-italic" => "bi",
                    _ => "i",
                };
                out.insert_str(
                    before + 5,
                    &format!("<m:rPr><m:sty m:val=\"{val}\"/></m:rPr>"),
                );
            }
        }
        "msup" => {
            let kids = element_children(doc, node);
            if kids.len() >= 2 {
                out.push_str("<m:sSup><m:e>");
                convert_dispatch(doc, kids[0], out);
                out.push_str("</m:e><m:sup>");
                convert_dispatch(doc, kids[1], out);
                out.push_str("</m:sup></m:sSup>");
            }
        }
        "msub" => {
            let kids = element_children(doc, node);
            if kids.len() >= 2 {
                out.push_str("<m:sSub><m:e>");
                convert_dispatch(doc, kids[0], out);
                out.push_str("</m:e><m:sub>");
                convert_dispatch(doc, kids[1], out);
                out.push_str("</m:sub></m:sSub>");
            }
        }
        "msubsup" => {
            let kids = element_children(doc, node);
            if kids.len() >= 3 {
                if is_nary_op(doc, kids[0]) {
                    push_nary(
                        doc,
                        kids[0],
                        Some(kids[1]),
                        Some(kids[2]),
                        &[],
                        limit_location(doc, node),
                        out,
                    );
                } else {
                    out.push_str("<m:sSubSup><m:e>");
                    convert_dispatch(doc, kids[0], out);
                    out.push_str("</m:e><m:sub>");
                    convert_dispatch(doc, kids[1], out);
                    out.push_str("</m:sub><m:sup>");
                    convert_dispatch(doc, kids[2], out);
                    out.push_str("</m:sup></m:sSubSup>");
                }
            }
        }
        "munderover" => {
            let kids = element_children(doc, node);
            if kids.len() >= 3 && is_nary_op(doc, kids[0]) {
                push_nary(
                    doc,
                    kids[0],
                    Some(kids[1]),
                    Some(kids[2]),
                    &[],
                    limit_location(doc, node),
                    out,
                );
            } else if kids.len() >= 3 {
                out.push_str("<m:limUpp><m:e><m:limLow><m:e>");
                convert_dispatch(doc, kids[0], out);
                out.push_str("</m:e><m:lim>");
                convert_dispatch(doc, kids[1], out);
                out.push_str("</m:lim></m:limLow></m:e><m:lim>");
                convert_dispatch(doc, kids[2], out);
                out.push_str("</m:lim></m:limUpp>");
            }
        }
        "munder" | "mover" => {
            let kids = element_children(doc, node);
            if is_accent(doc, node) {
                let operator = accent_operator(doc, node).expect("validated accent operator");
                let chr = clean_text(&doc.text_content(operator));
                let lower = name == "munder";
                if chr == "¯" {
                    let pos = if lower { "bot" } else { "top" };
                    out.push_str(&format!(
                        "<m:bar><m:barPr><m:pos m:val=\"{pos}\"/></m:barPr><m:e>"
                    ));
                    convert_dispatch(doc, kids[0], out);
                    out.push_str("</m:e></m:bar>");
                } else if lower || matches!(chr.as_str(), "⏞" | "⏟" | "⏜" | "⏝") {
                    let pos = if lower { "bot" } else { "top" };
                    let justify = if lower { "top" } else { "bot" };
                    out.push_str(&format!("<m:groupChr><m:groupChrPr><m:chr m:val=\"{}\"/><m:pos m:val=\"{pos}\"/><m:vertJc m:val=\"{justify}\"/></m:groupChrPr><m:e>", escape_xml(&chr)));
                    convert_dispatch(doc, kids[0], out);
                    out.push_str("</m:e></m:groupChr>");
                } else {
                    out.push_str(&format!(
                        "<m:acc><m:accPr><m:chr m:val=\"{}\"/></m:accPr><m:e>",
                        escape_xml(&chr)
                    ));
                    convert_dispatch(doc, kids[0], out);
                    out.push_str("</m:e></m:acc>");
                }
            } else if is_nary_op(doc, kids[0]) {
                push_nary(
                    doc,
                    kids[0],
                    if name == "munder" {
                        Some(kids[1])
                    } else {
                        None
                    },
                    if name == "mover" { Some(kids[1]) } else { None },
                    &[],
                    limit_location(doc, node),
                    out,
                );
            } else {
                let tag = if name == "munder" { "limLow" } else { "limUpp" };
                out.push_str(&format!("<m:{tag}><m:e>"));
                convert_dispatch(doc, kids[0], out);
                out.push_str("</m:e><m:lim>");
                convert_dispatch(doc, kids[1], out);
                out.push_str(&format!("</m:lim></m:{tag}>"));
            }
        }
        "mfrac" => {
            let kids = element_children(doc, node);
            if kids.len() >= 2 {
                let no_bar = attr(doc, node, "linethickness")
                    .is_some_and(|v| v == "0" || v == "0px" || v == "0pt");
                out.push_str("<m:f>");
                if no_bar || attr(doc, node, "bevelled") == Some("true") {
                    let kind = if no_bar { "noBar" } else { "skw" };
                    out.push_str(&format!("<m:fPr><m:type m:val=\"{kind}\"/></m:fPr>"));
                }
                out.push_str("<m:num>");
                convert_dispatch(doc, kids[0], out);
                out.push_str("</m:num><m:den>");
                convert_dispatch(doc, kids[1], out);
                out.push_str("</m:den></m:f>");
            }
        }
        "msqrt" => {
            out.push_str("<m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg/><m:e>");
            convert_siblings(doc, node, out);
            out.push_str("</m:e></m:rad>");
        }
        "mroot" => {
            let kids = element_children(doc, node);
            if kids.len() >= 2 {
                out.push_str("<m:rad><m:deg>");
                convert_dispatch(doc, kids[1], out);
                out.push_str("</m:deg><m:e>");
                convert_dispatch(doc, kids[0], out);
                out.push_str("</m:e></m:rad>");
            }
        }
        "mtable" => {
            out.push_str("<m:m>");
            for row in element_children(doc, node) {
                if local_name(doc, row) != "mtr" {
                    continue;
                }
                out.push_str("<m:mr>");
                for cell in element_children(doc, row) {
                    if local_name(doc, cell) != "mtd" {
                        continue;
                    }
                    out.push_str("<m:e>");
                    convert_siblings(doc, cell, out);
                    out.push_str("</m:e>");
                }
                out.push_str("</m:mr>");
            }
            out.push_str("</m:m>");
        }
        "mspace" => {
            let width = attr(doc, node, "width").unwrap_or("0.167em");
            let count = if width.contains('2') {
                8
            } else if width.contains('1') {
                4
            } else {
                1
            };
            out.push_str("<m:r><m:t xml:space=\"preserve\">");
            out.push_str(&" ".repeat(count));
            out.push_str("</m:t></m:r>");
        }
        "mphantom" => {
            out.push_str("<m:phant><m:e>");
            convert_siblings(doc, node, out);
            out.push_str("</m:e></m:phant>");
        }
        "menclose" => {
            out.push_str("<m:borderBox><m:e>");
            convert_siblings(doc, node, out);
            out.push_str("</m:e></m:borderBox>");
        }
        "mmultiscripts" => {
            let kids = element_children(doc, node);
            let pre = kids
                .iter()
                .position(|&n| local_name(doc, n) == "mprescripts");
            let mut base = String::new();
            convert_dispatch(doc, kids[0], &mut base);
            if pre != Some(1) {
                let mut sub = String::new();
                let mut sup = String::new();
                convert_dispatch(doc, kids[1], &mut sub);
                convert_dispatch(doc, kids[2], &mut sup);
                if !sub.is_empty() || !sup.is_empty() {
                    base=format!("<m:sSubSup><m:e>{base}</m:e><m:sub>{sub}</m:sub><m:sup>{sup}</m:sup></m:sSubSup>");
                }
            }
            if let Some(pre) = pre {
                out.push_str(&format!("<m:sPre><m:e>{base}</m:e><m:sub>"));
                convert_dispatch(doc, kids[pre + 1], out);
                out.push_str("</m:sub><m:sup>");
                convert_dispatch(doc, kids[pre + 2], out);
                out.push_str("</m:sup></m:sPre>");
            } else {
                out.push_str(&base);
            }
        }
        "mprescripts" | "none" | "annotation" | "annotation-xml" => {}
        _ => {
            let kids = element_children(doc, node);
            if kids.is_empty() {
                let text = clean_text(&doc.text_content(node));
                if !text.is_empty() {
                    push_run(out, &text, false);
                }
            } else {
                convert_siblings(doc, node, out);
            }
        }
    }
}

fn push_run(out: &mut String, text: &str, upright: bool) {
    out.push_str("<m:r>");
    if upright {
        out.push_str("<m:rPr><m:sty m:val=\"p\"/></m:rPr>");
    }
    let preserve = text.starts_with(' ') || text.ends_with(' ') || text.contains("  ");
    if preserve {
        out.push_str("<m:t xml:space=\"preserve\">");
    } else {
        out.push_str("<m:t>");
    }
    out.push_str(&escape_xml(text));
    out.push_str("</m:t></m:r>");
}

fn push_nary(
    doc: &Document,
    op: NodeId,
    sub: Option<NodeId>,
    sup: Option<NodeId>,
    body: &[NodeId],
    location: &str,
    out: &mut String,
) {
    let chr = clean_text(&doc.text_content(op));
    out.push_str("<m:nary><m:naryPr><m:chr m:val=\"");
    out.push_str(&escape_xml(&chr));
    out.push_str(&format!("\"/><m:limLoc m:val=\"{location}\"/>"));
    if sub.is_none() {
        out.push_str("<m:subHide m:val=\"1\"/>");
    }
    if sup.is_none() {
        out.push_str("<m:supHide m:val=\"1\"/>");
    }
    if let Some(variant) = inherited_variant(doc, op) {
        let bold = matches!(variant, "bold" | "bold-italic");
        let italic = matches!(variant, "italic" | "bold-italic");
        out.push_str(&format!("<m:ctrlPr><w:rPr xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:b w:val=\"{}\"/><w:i w:val=\"{}\"/></w:rPr></m:ctrlPr>",u8::from(bold),u8::from(italic)));
    }
    out.push_str("</m:naryPr><m:sub>");
    if let Some(sub) = sub {
        convert_dispatch(doc, sub, out);
    }
    out.push_str("</m:sub><m:sup>");
    if let Some(sup) = sup {
        convert_dispatch(doc, sup, out);
    }
    out.push_str("</m:sup><m:e>");
    convert_sequence(doc, body, out);
    out.push_str("</m:e></m:nary>");
}

fn is_nary_op(doc: &Document, node: NodeId) -> bool {
    if local_name(doc, node) != "mo" {
        return false;
    }
    let t = clean_text(&doc.text_content(node));
    matches!(
        t.as_str(),
        "∑" | "∏" | "∫" | "∬" | "∭" | "∮" | "⋃" | "⋂" | "⋁" | "⋀" | "∐"
    )
}

fn accent_operator(doc: &Document, node: NodeId) -> Option<NodeId> {
    accent_operator_at(doc, node, 1)
}

fn accent_operator_at(doc: &Document, node: NodeId, index: usize) -> Option<NodeId> {
    let mut operator = *element_children(doc, node).get(index)?;
    while matches!(local_name(doc, operator).as_str(), "mrow" | "mstyle") {
        let children = element_children(doc, operator);
        if children.len() != 1 {
            return None;
        }
        operator = children[0];
    }
    (local_name(doc, operator) == "mo"
        && clean_text(&doc.text_content(operator)).chars().count() == 1)
        .then_some(operator)
}

fn is_accent(doc: &Document, node: NodeId) -> bool {
    let attribute = if local_name(doc, node) == "munder" {
        "accentunder"
    } else {
        "accent"
    };
    is_accent_at(doc, node, attribute, 1)
}

fn is_accent_at(doc: &Document, node: NodeId, attribute: &str, index: usize) -> bool {
    match attr(doc, node, attribute) {
        Some("true") => true,
        Some("false") => false,
        _ => accent_operator_at(doc, node, index).is_some_and(|operator| {
            let text = clean_text(&doc.text_content(operator));
            is_accent_char(&text) || matches!(text.as_str(), "⏞" | "⏟" | "⏜" | "⏝" | "_")
        }),
    }
}

fn is_accent_char(text: &str) -> bool {
    matches!(
        text,
        "^" | "~"
            | "¯"
            | "\u{0302}"
            | "\u{0303}"
            | "\u{0304}"
            | "\u{0307}"
            | "\u{0308}"
            | "\u{0301}"
            | "\u{0300}"
            | "\u{030c}"
            | "\u{20d7}"
            | "ˆ"
            | "˜"
            | "˙"
            | "¨"
            | "´"
            | "`"
    )
}

fn clean_text(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !matches!(
                *c,
                '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' | '\u{fe00}'
                    ..='\u{fe0f}' | '\u{2061}'
            )
        })
        .collect()
}

fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fraction_and_script() {
        let xml = latex_to_omml_xml(r"x^{2}+\frac{a}{b}", false).unwrap();
        assert!(xml.contains("<m:sSup>"), "{xml}");
        assert!(xml.contains("<m:f>"), "{xml}");
        assert!(xml.contains("<m:t>x</m:t>"), "{xml}");
    }

    #[test]
    fn scripts_and_accents_require_braces() {
        for latex in [r"A_B", r"x^2", r"x^{a}_b", r"\sum_{i=1}^n", r"\hat x"] {
            assert!(latex_to_omml_xml(latex, false).is_err(), "{latex}");
        }
        for latex in [
            r"A_{B}",
            r"x^{2}",
            r"x^{a}_{b}",
            r"A_{B_{N}}^{\hat{x}}",
            r"\hat{x}",
            r"\vec{A}_{i}",
        ] {
            assert!(latex_to_omml_xml(latex, false).is_ok(), "{latex}");
        }
        assert!(require_braced_scripts(r"\_ + \^ + A_{\alpha}").is_ok());
    }

    #[test]
    fn display_wraps_omath_para() {
        let xml = latex_to_omml_xml(r"a", true).unwrap();
        assert!(xml.starts_with("<m:oMathPara"), "{xml}");
    }

    #[test]
    fn large_operator_operand_lives_inside_nary_e() {
        for (latex, operand) in [
            (r"\sum_{i=1}^{n} x_{i}", "xi"),
            (r"\prod_{k=1}^{m} (k+1)", "(k+1)"),
            (r"\int_{0}^{1} x^{2}\,dx", "x2    dx"),
            (r"\bigcup_{i=1}^{n} A_{i}", "Ai"),
            (r"\bigcap_{i=1}^{n} A_{i}", "Ai"),
            (r"\coprod_{i=1}^{n} A_{i}", "Ai"),
        ] {
            let xml = latex_to_omml_xml(latex, true).unwrap();
            let doc = Document::parse_bytes(xml.as_bytes()).unwrap();
            let root = doc.root_element().unwrap();
            let nary = doc
                .descendants(root)
                .find(|&node| local_name(&doc, node) == "nary")
                .unwrap();
            let body = element_children(&doc, nary)
                .into_iter()
                .find(|&node| local_name(&doc, node) == "e")
                .unwrap();
            assert_eq!(doc.text_content(body), operand, "{latex}: {xml}");
        }
    }

    #[test]
    fn nary_body_stops_before_outer_addition_and_keeps_inner_addition() {
        let xml = latex_to_omml_xml(r"\sum_{i=1}^{n} (x_{i}+y_{i})+z", true).unwrap();
        let doc = Document::parse_bytes(xml.as_bytes()).unwrap();
        let root = doc.root_element().unwrap();
        let nary = doc
            .descendants(root)
            .find(|&node| local_name(&doc, node) == "nary")
            .unwrap();
        let body = element_children(&doc, nary)
            .into_iter()
            .find(|&node| local_name(&doc, node) == "e")
            .unwrap();
        assert_eq!(doc.text_content(body), "(xi+yi)", "{xml}");
        assert!(xml.contains("</m:nary><m:r>"), "{xml}");
    }

    #[test]
    fn nary_body_keeps_leading_unary_minus() {
        let xml = latex_to_omml_xml(r"\sum_{i=1}^{n} -x_{i}+y", false).unwrap();
        let doc = Document::parse_bytes(xml.as_bytes()).unwrap();
        let root = doc.root_element().unwrap();
        let nary = doc
            .descendants(root)
            .find(|&node| local_name(&doc, node) == "nary")
            .unwrap();
        let body = element_children(&doc, nary)
            .into_iter()
            .find(|&node| local_name(&doc, node) == "e")
            .unwrap();
        assert_eq!(doc.text_content(body), "-xi", "{xml}");
    }
}
