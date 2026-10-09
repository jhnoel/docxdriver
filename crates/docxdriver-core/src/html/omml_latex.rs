//! OMML → LaTeX conversion for the equation dialect body.
//!
//! Adapted from kklimuk/docx-cli (`src/core/equation/`, MIT License,
//! Copyright (c) 2026 Kirill Klimuk): walk `m:oMath` / `m:oMathPara`, emit
//! reconstructed LaTeX, and degrade unknown subtrees to plaintext `m:t`
//! concatenation so only that piece loses structure.
//!
//! The LaTeX string is the edit-anchor identity in `<equation>…</equation>`.

use xmloxide::tree::NodeId;

use crate::document::DocxXml;

/// Convert one `m:oMath` / `m:oMathPara` subtree to LaTeX. Empty when the
/// subtree holds no text (mirrors empty-equation omission on render).
pub fn omml_to_latex(xml: &DocxXml, node: NodeId) -> String {
    let root = if xml.is_local(node, "oMathPara") {
        xml.child_local(node, "oMath").unwrap_or(node)
    } else {
        node
    };
    let mut out = String::new();
    convert_children(xml, root, &mut out);
    // Strip trailing spaces; drop disambiguation space before ) or ]
    // (`\xi )` → `\xi)`).
    let trimmed = out.trim_end();
    let mut cleaned = String::with_capacity(trimmed.len());
    let chars: Vec<char> = trimmed.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            cleaned.push('\\');
            i += 1;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                cleaned.push(chars[i]);
                i += 1;
            }
            if i < chars.len()
                && chars[i] == ' '
                && i + 1 < chars.len()
                && matches!(chars[i + 1], ')' | ']')
            {
                i += 1;
            }
            continue;
        }
        cleaned.push(chars[i]);
        i += 1;
    }
    cleaned
}

fn convert_children(xml: &DocxXml, node: NodeId, out: &mut String) {
    for child in xml.doc.children(node).collect::<Vec<_>>() {
        if !xml.doc.is_element(child) {
            continue;
        }
        convert_element(xml, child, out);
    }
}

fn convert_element(xml: &DocxXml, node: NodeId, out: &mut String) {
    let name = local_name(xml, node);
    // Presentation-only property bags.
    if name.ends_with("Pr") || name == "ctrlPr" {
        return;
    }
    match name.as_str() {
        "r" => convert_run(xml, node, out),
        "e" => convert_children(xml, node, out),
        "sSup" => {
            let base = child_latex(xml, node, "e");
            let sup = child_latex(xml, node, "sup");
            out.push_str(&script_base(&base));
            out.push('^');
            out.push_str(&grouped(&sup));
        }
        "sSub" => {
            let base = child_latex(xml, node, "e");
            let sub = child_latex(xml, node, "sub");
            out.push_str(&script_base(&base));
            out.push('_');
            out.push_str(&grouped(&sub));
        }
        "sSubSup" => {
            let base = child_latex(xml, node, "e");
            let sub = child_latex(xml, node, "sub");
            let sup = child_latex(xml, node, "sup");
            out.push_str(&script_base(&base));
            out.push('_');
            out.push_str(&grouped(&sub));
            out.push('^');
            out.push_str(&grouped(&sup));
        }
        "sPre" => {
            let sub = child_latex(xml, node, "sub");
            let base = child_latex(xml, node, "e");
            out.push_str("{}_");
            out.push_str(&grouped(&sub));
            out.push_str(&base);
        }
        "f" => {
            let num = child_latex(xml, node, "num").trim_end().to_string();
            let den = child_latex(xml, node, "den").trim_end().to_string();
            let f_type = xml
                .child_local(node, "fPr")
                .and_then(|pr| prop_val(xml, pr, "type"));
            match f_type {
                Some("noBar") => {
                    out.push_str("\\binom{");
                    out.push_str(&num);
                    out.push_str("}{");
                    out.push_str(&den);
                    out.push('}');
                }
                Some("skw") => {
                    out.push_str("{}^{");
                    out.push_str(&num);
                    out.push_str("}/_{");
                    out.push_str(&den);
                    out.push('}');
                }
                _ => {
                    out.push_str("\\frac{");
                    out.push_str(&num);
                    out.push_str("}{");
                    out.push_str(&den);
                    out.push('}');
                }
            }
        }
        "rad" => {
            let deg_hidden = xml
                .child_local(node, "radPr")
                .is_some_and(|pr| prop_on(xml, pr, "degHide"));
            let body = child_latex(xml, node, "e").trim_end().to_string();
            let deg = if deg_hidden {
                String::new()
            } else {
                child_latex(xml, node, "deg").trim_end().to_string()
            };
            if deg.is_empty() {
                out.push_str("\\sqrt{");
                out.push_str(&body);
                out.push('}');
            } else {
                out.push_str("\\sqrt[");
                out.push_str(&deg);
                out.push_str("]{");
                out.push_str(&body);
                out.push('}');
            }
        }
        "nary" => {
            let pr = xml.child_local(node, "naryPr");
            let chr = pr
                .and_then(|pr| prop_val(xml, pr, "chr"))
                .unwrap_or("\u{222b}");
            let cmd = nary_cmd(chr);
            let sub_hidden = pr.is_some_and(|pr| prop_on(xml, pr, "subHide"));
            let sup_hidden = pr.is_some_and(|pr| prop_on(xml, pr, "supHide"));
            let sub = if sub_hidden {
                String::new()
            } else {
                child_latex(xml, node, "sub").trim_end().to_string()
            };
            let sup = if sup_hidden {
                String::new()
            } else {
                child_latex(xml, node, "sup").trim_end().to_string()
            };
            let body = child_latex(xml, node, "e").trim_end().to_string();
            out.push_str(cmd);
            if !sub.is_empty() {
                out.push('_');
                out.push_str(&grouped(&sub));
            }
            if !sup.is_empty() {
                out.push('^');
                out.push_str(&grouped(&sup));
            }
            if !body.is_empty() {
                out.push(' ');
                out.push_str(&body);
            }
        }
        "limLow" | "limUpp" => {
            let raw_base = child_latex(xml, node, "e").trim_end().to_string();
            let lim = child_latex(xml, node, "lim").trim_end().to_string();
            let promoted = promote_operator(&raw_base);
            let is_upper = name == "limUpp";
            if promoted != raw_base || uses_sub_sup_form(&raw_base) {
                out.push_str(&promoted);
                out.push(if is_upper { '^' } else { '_' });
                out.push_str(&grouped(&lim));
            } else if is_upper {
                out.push_str("\\overset{");
                out.push_str(&lim);
                out.push_str("}{");
                out.push_str(&raw_base);
                out.push('}');
            } else {
                out.push_str("\\underset{");
                out.push_str(&lim);
                out.push_str("}{");
                out.push_str(&raw_base);
                out.push('}');
            }
        }
        "func" => {
            let mut f_name = child_latex(xml, node, "fName").trim_end().to_string();
            f_name = promote_operator(&f_name);
            let body = child_latex(xml, node, "e").trim_end().to_string();
            out.push_str(&f_name);
            if !body.is_empty() {
                out.push(' ');
                out.push_str(&body);
            }
        }
        "acc" => {
            let chr = xml
                .child_local(node, "accPr")
                .and_then(|pr| prop_val(xml, pr, "chr"))
                .unwrap_or("\u{0302}");
            let body = child_latex(xml, node, "e").trim_end().to_string();
            out.push_str(accent_cmd(chr));
            out.push('{');
            out.push_str(&body);
            out.push('}');
        }
        "bar" => {
            let top = xml
                .child_local(node, "barPr")
                .and_then(|pr| prop_val(xml, pr, "pos"))
                != Some("bot");
            let body = child_latex(xml, node, "e").trim_end().to_string();
            if top {
                out.push_str("\\overline{");
            } else {
                out.push_str("\\underline{");
            }
            out.push_str(&body);
            out.push('}');
        }
        "groupChr" => {
            let pr = xml.child_local(node, "groupChrPr");
            let chr = pr.and_then(|pr| prop_val(xml, pr, "chr")).unwrap_or("");
            let top = pr.and_then(|pr| prop_val(xml, pr, "pos")) != Some("bot");
            let body = child_latex(xml, node, "e").trim_end().to_string();
            let cmd =
                group_chr_cmd(chr).unwrap_or(if top { "\\overbrace" } else { "\\underbrace" });
            out.push_str(cmd);
            out.push('{');
            out.push_str(&body);
            out.push('}');
        }
        "d" => convert_delimiter(xml, node, out),
        "m" => {
            out.push_str("\\begin{matrix}");
            push_matrix_rows(xml, node, out);
            out.push_str("\\end{matrix}");
        }
        "eqArr" => {
            out.push_str("\\begin{aligned}");
            let mut first = true;
            for row in xml.doc.children(node).collect::<Vec<_>>() {
                if !xml.is_local(row, "e") {
                    continue;
                }
                if !first {
                    out.push_str(" \\\\ ");
                }
                first = false;
                convert_children(xml, row, out);
            }
            out.push_str("\\end{aligned}");
        }
        "box" | "borderBox" | "phant" => convert_children(xml, node, out),
        // Unknown: plaintext fallback for this subtree only.
        _ => out.push_str(&collect_plain_text(xml, node)),
    }
}

fn convert_delimiter(xml: &DocxXml, node: NodeId, out: &mut String) {
    let pr = xml.child_local(node, "dPr");
    let beg = pr.and_then(|pr| prop_val(xml, pr, "begChr")).unwrap_or("(");
    let end = pr.and_then(|pr| prop_val(xml, pr, "endChr")).unwrap_or(")");
    let sep = pr.and_then(|pr| prop_val(xml, pr, "sepChr")).unwrap_or(",");
    let elements: Vec<NodeId> = xml
        .doc
        .children(node)
        .filter(|&c| xml.is_local(c, "e"))
        .collect();

    if elements.len() == 1 {
        let sole = structural_children(xml, elements[0]);
        if sole.len() == 1 {
            let only = sole[0];
            if xml.is_local(only, "f")
                && beg == "("
                && end == ")"
                && xml
                    .child_local(only, "fPr")
                    .and_then(|pr| prop_val(xml, pr, "type"))
                    == Some("noBar")
            {
                let num = child_latex(xml, only, "num").trim_end().to_string();
                let den = child_latex(xml, only, "den").trim_end().to_string();
                out.push_str("\\binom{");
                out.push_str(&num);
                out.push_str("}{");
                out.push_str(&den);
                out.push('}');
                return;
            }
            if xml.is_local(only, "m") {
                if let Some(env) = matrix_shorthand(beg, end) {
                    out.push_str("\\begin{");
                    out.push_str(env);
                    out.push('}');
                    push_matrix_rows(xml, only, out);
                    out.push_str("\\end{");
                    out.push_str(env);
                    out.push('}');
                    return;
                }
            }
        }
    }

    out.push_str("\\left");
    out.push_str(&normalize_delim(beg));
    for (i, element) in elements.iter().enumerate() {
        if i > 0 {
            out.push_str(sep);
        }
        convert_children(xml, *element, out);
    }
    out.push_str("\\right");
    out.push_str(&normalize_delim(end));
}

fn push_matrix_rows(xml: &DocxXml, matrix: NodeId, out: &mut String) {
    let mut first_row = true;
    for row in xml.doc.children(matrix).collect::<Vec<_>>() {
        if !xml.is_local(row, "mr") {
            continue;
        }
        if !first_row {
            out.push_str(" \\\\ ");
        }
        first_row = false;
        let mut first_cell = true;
        for cell in xml.doc.children(row).collect::<Vec<_>>() {
            if !xml.is_local(cell, "e") {
                continue;
            }
            if !first_cell {
                out.push_str(" & ");
            }
            first_cell = false;
            convert_children(xml, cell, out);
        }
    }
}

fn convert_run(xml: &DocxXml, run: NodeId, out: &mut String) {
    let r_pr = xml.child_local(run, "rPr");
    let is_plain = r_pr
        .and_then(|pr| xml.child_local(pr, "sty"))
        .and_then(|sty| xml.attr(sty, "val"))
        == Some("p");
    let mut text = String::new();
    for child in xml.doc.children(run).collect::<Vec<_>>() {
        if xml.is_local(child, "t") || xml.is_local(child, "delText") {
            text.push_str(&xml.doc.text_content(child));
        }
    }
    if text.is_empty() {
        return;
    }
    // Space-only runs → thin/quad spacing commands.
    if r_pr.is_none() && text.chars().all(|c| c == ' ') {
        if text.len() >= 6 {
            out.push_str("\\qquad ");
        } else if text.len() >= 3 {
            out.push_str("\\quad ");
        } else {
            out.push_str("\\, ");
        }
        return;
    }
    if is_plain && KNOWN_OPERATORS.contains(&text.as_str()) {
        out.push('\\');
        out.push_str(&text);
        out.push(' ');
        return;
    }
    if is_plain && text.chars().any(|c| c.is_ascii_alphabetic()) {
        out.push_str("\\text{");
        out.push_str(&escape_latex_specials(&text));
        out.push('}');
        return;
    }
    out.push_str(&escape_and_map(&text));
}

fn child_latex(xml: &DocxXml, node: NodeId, local: &str) -> String {
    let mut out = String::new();
    if let Some(child) = xml.child_local(node, local) {
        convert_children(xml, child, &mut out);
    }
    out
}

fn collect_plain_text(xml: &DocxXml, node: NodeId) -> String {
    let mut out = String::new();
    collect_plain_text_into(xml, node, &mut out);
    out
}

fn collect_plain_text_into(xml: &DocxXml, node: NodeId, out: &mut String) {
    if xml.is_local(node, "t") || xml.is_local(node, "delText") {
        out.push_str(&xml.doc.text_content(node));
        return;
    }
    for child in xml.doc.children(node).collect::<Vec<_>>() {
        if xml.doc.is_element(child) {
            collect_plain_text_into(xml, child, out);
        }
    }
}

fn structural_children(xml: &DocxXml, node: NodeId) -> Vec<NodeId> {
    xml.doc
        .children(node)
        .filter(|&c| {
            if !xml.doc.is_element(c) {
                return false;
            }
            let name = local_name(xml, c);
            !name.ends_with("Pr") && name != "ctrlPr"
        })
        .collect()
}

fn local_name(xml: &DocxXml, node: NodeId) -> String {
    match xml.doc.node_name(node) {
        Some(name) => name.rsplit(':').next().unwrap_or(name).to_string(),
        None => String::new(),
    }
}

fn prop_val<'a>(xml: &'a DocxXml, pr: NodeId, local: &str) -> Option<&'a str> {
    let child = xml.child_local(pr, local)?;
    xml.attr(child, "val")
}

fn prop_on(xml: &DocxXml, pr: NodeId, local: &str) -> bool {
    match xml.child_local(pr, local) {
        None => false,
        Some(child) => !matches!(
            xml.attr(child, "val"),
            Some("0") | Some("false") | Some("off")
        ),
    }
}

fn grouped(inner: &str) -> String {
    let trimmed = inner.trim_end();
    format!("{{{trimmed}}}")
}

fn script_base(inner: &str) -> String {
    let trimmed = inner.trim_end();
    if trimmed.chars().count() == 1
        || (trimmed.starts_with('\\') && trimmed[1..].chars().all(|c| c.is_ascii_alphabetic()))
    {
        trimmed.to_string()
    } else {
        grouped(trimmed)
    }
}

fn normalize_delim(ch: &str) -> String {
    match ch {
        "{" | "}" => format!("\\{ch}"),
        "|" => "|".into(),
        "‖" => "\\|".into(),
        other => other.to_string(),
    }
}

fn matrix_shorthand(beg: &str, end: &str) -> Option<&'static str> {
    match (beg, end) {
        ("(", ")") => Some("pmatrix"),
        ("[", "]") => Some("bmatrix"),
        ("{", "}") => Some("Bmatrix"),
        ("|", "|") => Some("vmatrix"),
        ("‖", "‖") => Some("Vmatrix"),
        _ => None,
    }
}

fn nary_cmd(chr: &str) -> &'static str {
    match chr {
        "∑" => "\\sum",
        "∏" => "\\prod",
        "∐" => "\\coprod",
        "∫" => "\\int",
        "∬" => "\\iint",
        "∭" => "\\iiint",
        "∮" => "\\oint",
        "⋃" => "\\bigcup",
        "⋂" => "\\bigcap",
        "⋁" => "\\bigvee",
        "⋀" => "\\bigwedge",
        _ => "\\int",
    }
}

fn accent_cmd(chr: &str) -> &'static str {
    match chr {
        "\u{0302}" | "^" => "\\hat",
        "\u{0307}" => "\\dot",
        "\u{0308}" => "\\ddot",
        "\u{0303}" | "~" => "\\tilde",
        "\u{0304}" | "¯" => "\\bar",
        "\u{0301}" | "´" => "\\acute",
        "\u{0300}" | "`" => "\\grave",
        "\u{030c}" => "\\check",
        "\u{20d7}" | "\u{2192}" => "\\vec",
        _ => "\\hat",
    }
}

fn group_chr_cmd(chr: &str) -> Option<&'static str> {
    match chr {
        "\u{23de}" => Some("\\overbrace"),
        "\u{23df}" => Some("\\underbrace"),
        "\u{23dc}" => Some("\\overparen"),
        "\u{23dd}" => Some("\\underparen"),
        _ => None,
    }
}

const KNOWN_OPERATORS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh",
    "coth", "ln", "log", "exp", "lim", "max", "min", "sup", "inf", "det", "ker", "deg", "dim",
    "hom", "arg", "gcd", "lcm", "Pr",
];

fn promote_operator(text: &str) -> String {
    let trimmed = text.trim_end();
    if KNOWN_OPERATORS.contains(&trimmed) {
        return format!("\\{trimmed} ");
    }
    // Already a command like `\lim ` or `\lim`.
    if let Some(rest) = trimmed.strip_prefix('\\') {
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        if KNOWN_OPERATORS.contains(&name.as_str()) {
            return format!("\\{name} ");
        }
    }
    text.to_string()
}

fn uses_sub_sup_form(text: &str) -> bool {
    let trimmed = text.trim_end();
    if let Some(rest) = trimmed.strip_prefix('\\') {
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        if KNOWN_OPERATORS.contains(&name.as_str()) {
            return true;
        }
        return matches!(
            name.as_str(),
            "underbrace" | "overbrace" | "underparen" | "overparen" | "undergroup" | "overgroup"
        );
    }
    false
}

fn escape_latex_specials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' | '$' | '%' | '#' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn escape_and_map(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        // Strip zero-width / variation selectors.
        if matches!(
            ch,
            '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' | '\u{fe00}'..='\u{fe0f}'
        ) {
            continue;
        }
        if let Some(cmd) = unicode_to_latex(ch) {
            out.push_str(cmd);
            if cmd.starts_with('\\') && cmd.chars().skip(1).all(|c| c.is_ascii_alphabetic()) {
                out.push(' ');
            }
            continue;
        }
        match ch {
            '\\' | '$' | '%' | '#' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(ch);
            }
            '−' | '⁻' => out.push('-'),
            '\u{00a0}' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

fn unicode_to_latex(ch: char) -> Option<&'static str> {
    Some(match ch {
        'α' => "\\alpha",
        'β' => "\\beta",
        'γ' => "\\gamma",
        'δ' => "\\delta",
        'ε' => "\\varepsilon",
        'ζ' => "\\zeta",
        'η' => "\\eta",
        'θ' => "\\theta",
        'ι' => "\\iota",
        'κ' => "\\kappa",
        'λ' => "\\lambda",
        'μ' => "\\mu",
        'ν' => "\\nu",
        'ξ' => "\\xi",
        'π' => "\\pi",
        'ρ' => "\\rho",
        'σ' => "\\sigma",
        'τ' => "\\tau",
        'υ' => "\\upsilon",
        'φ' => "\\varphi",
        'χ' => "\\chi",
        'ψ' => "\\psi",
        'ω' => "\\omega",
        'ϵ' => "\\epsilon",
        'ϕ' => "\\phi",
        'Γ' => "\\Gamma",
        'Δ' => "\\Delta",
        'Θ' => "\\Theta",
        'Λ' => "\\Lambda",
        'Ξ' => "\\Xi",
        'Π' => "\\Pi",
        'Σ' => "\\Sigma",
        'Υ' => "\\Upsilon",
        'Φ' => "\\Phi",
        'Ψ' => "\\Psi",
        'Ω' => "\\Omega",
        '×' => "\\times",
        '÷' => "\\div",
        '·' | '⋅' => "\\cdot",
        '±' => "\\pm",
        '∓' => "\\mp",
        '≤' => "\\leq",
        '≥' => "\\geq",
        '≠' => "\\neq",
        '≈' => "\\approx",
        '≡' => "\\equiv",
        '∈' => "\\in",
        '∉' => "\\notin",
        '⊂' => "\\subset",
        '⊃' => "\\supset",
        '⊆' => "\\subseteq",
        '⊇' => "\\supseteq",
        '∪' => "\\cup",
        '∩' => "\\cap",
        '∅' => "\\emptyset",
        '∂' => "\\partial",
        '∇' => "\\nabla",
        '∞' => "\\infty",
        '∑' => "\\sum",
        '∏' => "\\prod",
        '∫' => "\\int",
        '→' => "\\to",
        '←' => "\\leftarrow",
        '⇒' => "\\Rightarrow",
        '⇐' => "\\Leftarrow",
        '⇔' => "\\Leftrightarrow",
        '…' => "\\ldots",
        '⋯' => "\\cdots",
        'ℓ' => "\\ell",
        'ℏ' => "\\hbar",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocxXml;

    fn latex_of(inner: &str) -> String {
        let xml = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><w:body><w:p><m:oMath>{inner}</m:oMath></w:p></w:body></w:document>"#
        );
        let doc = DocxXml::parse(xml.as_bytes()).unwrap();
        let body = doc.body().unwrap();
        let p = doc.doc.children(body).next().unwrap();
        let math = doc.doc.children(p).next().unwrap();
        omml_to_latex(&doc, math)
    }

    #[test]
    fn fraction_and_scripts() {
        assert_eq!(
            latex_of("<m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f>"),
            "\\frac{a}{b}"
        );
        assert_eq!(
            latex_of("<m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup>"),
            "x^{2}"
        );
        assert_eq!(
            latex_of("<m:sSup><m:e><m:r><m:t>ab</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup>"),
            "{ab}^{2}"
        );
    }

    #[test]
    fn greek_and_sqrt() {
        assert_eq!(latex_of("<m:r><m:t>α</m:t></m:r>"), "\\alpha");
        assert_eq!(
            latex_of("<m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad>"),
            "\\sqrt{x}"
        );
    }

    #[test]
    fn accent_read_uses_braced_argument() {
        assert_eq!(
            latex_of("<m:acc><m:e><m:r><m:t>x</m:t></m:r></m:e></m:acc>"),
            "\\hat{x}"
        );
    }
}
