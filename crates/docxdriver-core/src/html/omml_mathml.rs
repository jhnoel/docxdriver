//! Direct Office Math → presentation MathML. No reconstructed LaTeX is involved.
use super::{escape_attr, escape_text};
use crate::document::DocxXml;
use xmloxide::tree::NodeId;

pub const MATH_NS: &str = "http://www.w3.org/1998/Math/MathML";

/// Office Math defaults: integral limits beside the operator, other n-ary
/// limits above/below. Package settings may override either default.
#[derive(Clone, Copy, Debug, Default)]
pub struct Limits {
    pub integral_under: bool,
    pub nary_side: bool,
}
impl Limits {
    pub fn load(package: &crate::package::Package) -> Self {
        let main = package.document_part_name();
        let dir = main
            .rsplit_once('/')
            .map(|(d, _)| format!("{d}/"))
            .unwrap_or_default();
        let mut out = Self::default();
        if let Some(xml) = package
            .get(&format!("{dir}settings.xml"))
            .and_then(|b| DocxXml::parse(b).ok())
        {
            for n in xml.doc.descendants(xml.doc.root()) {
                if xml.is_local(n, "intLim") {
                    out.integral_under = xml.attr(n, "val") == Some("undOvr");
                }
                if xml.is_local(n, "naryLim") {
                    out.nary_side = xml.attr(n, "val") == Some("subSup");
                }
            }
        }
        out
    }
}

pub fn omml_to_mathml(xml: &DocxXml, node: NodeId, limits: Limits) -> String {
    let display = if xml.is_local(node, "oMathPara") {
        "block"
    } else {
        "inline"
    };
    format!(
        "<math xmlns=\"{MATH_NS}\" display=\"{display}\">{}</math>",
        children(xml, node, limits)
    )
}

fn children(xml: &DocxXml, node: NodeId, limits: Limits) -> String {
    xml.doc
        .children(node)
        .filter(|&n| xml.doc.is_element(n))
        .map(|n| element(xml, n, limits))
        .collect()
}
fn child(xml: &DocxXml, node: NodeId, name: &str, limits: Limits) -> String {
    format!(
        "<mrow>{}</mrow>",
        xml.child_local(node, name)
            .map(|n| children(xml, n, limits))
            .unwrap_or_default()
    )
}
fn prop<'a>(xml: &'a DocxXml, node: NodeId, bag: &str, name: &str) -> Option<&'a str> {
    xml.child_local(node, bag)
        .and_then(|p| xml.child_local(p, name))
        .and_then(|n| xml.attr(n, "val"))
}
fn token(tag: &str, text: &str) -> String {
    format!("<{tag}>{}</{tag}>", escape_text(text))
}
fn named_operator(text: &str) -> bool {
    [
        "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh",
        "tanh", "coth", "ln", "log", "exp", "lim", "max", "min", "sup", "inf", "det", "ker", "deg",
        "dim", "hom", "arg", "gcd", "lcm", "Pr",
    ]
    .contains(&text)
}
fn element(xml: &DocxXml, n: NodeId, limits: Limits) -> String {
    let name = xml
        .doc
        .node_name(n)
        .unwrap_or("")
        .rsplit(':')
        .next()
        .unwrap_or("");
    if name.ends_with("Pr") || name == "ctrlPr" {
        return String::new();
    }
    match name {
        "r" => {
            let text: String = xml.doc.children(n).filter(|&c| xml.is_local(c,"t") || xml.is_local(c,"delText")).map(|c| xml.doc.text_content(c)).collect();
            let style=prop(xml,n,"rPr","sty");
            let upright = style == Some("p") || xml.child_local(n,"rPr").is_some_and(|p| xml.child_local(p,"nor").is_some());
            let variant=match style {Some("b")=>Some("bold"),Some("bi")=>Some("bold-italic"),Some("i")=>Some("italic"),_ if upright=>Some("normal"),_=>None};
            let emit=|tag:&str,t:&str| { if let Some(v)=variant {format!("<{tag} mathvariant=\"{v}\">{}</{tag}>",escape_text(t))} else {token(tag,t)} };
            let operator_context=xml.doc.parent(n).is_some_and(|p|xml.is_local(p,"fName") || (xml.is_local(p,"e") && xml.doc.parent(p).is_some_and(|p|xml.is_local(p,"limLow") || xml.is_local(p,"limUpp"))));
            if (upright || operator_context) && named_operator(&text) {
                format!("<mo mathvariant=\"{}\">{}</mo>",variant.unwrap_or("normal"),escape_text(&text))
            } else if (upright && text.chars().any(char::is_whitespace)) || xml.doc.children(n).any(|c|xml.is_local(c,"delText")) { emit("mtext", &text) }
            else if !text.is_empty() && text.chars().all(|c| c.is_ascii_digit() || c == '.') { emit("mn", &text) }
            else if upright && text.chars().all(|c| c.is_alphabetic()) { emit("mi", &text) }
            else { text.chars().map(|c| emit(if c.is_alphabetic() {"mi"} else if c.is_ascii_digit() {"mn"} else if c.is_whitespace(){"mtext"} else {"mo"}, &c.to_string())).collect() }
        }
        "oMath"|"oMathPara"|"e"|"num"|"den"|"sub"|"sup"|"deg"|"lim"|"fName"|"box" => children(xml,n,limits),
        "sSup" => format!("<msup>{}{}</msup>",child(xml,n,"e",limits),child(xml,n,"sup",limits)),
        "sSub" => format!("<msub>{}{}</msub>",child(xml,n,"e",limits),child(xml,n,"sub",limits)),
        "sSubSup" => format!("<msubsup>{}{}{}</msubsup>",child(xml,n,"e",limits),child(xml,n,"sub",limits),child(xml,n,"sup",limits)),
        "sPre" => format!("<mmultiscripts>{}<mrow></mrow><mrow></mrow><mprescripts></mprescripts>{}{}</mmultiscripts>",child(xml,n,"e",limits),child(xml,n,"sub",limits),child(xml,n,"sup",limits)),
        "f" => {
            let attr = match prop(xml,n,"fPr","type") { Some("noBar") => " linethickness=\"0\"", Some("skw") => " bevelled=\"true\"", _ => "" };
            format!("<mfrac{attr}>{}{}</mfrac>",child(xml,n,"num",limits),child(xml,n,"den",limits))
        }
        "rad" => {
            if prop(xml,n,"radPr","degHide").is_some_and(|v| v == "1" || v == "true") || xml.child_local(n,"deg").is_none_or(|d| xml.doc.text_content(d).is_empty()) {
                format!("<msqrt>{}</msqrt>",child(xml,n,"e",limits))
            } else { format!("<mroot>{}{}</mroot>",child(xml,n,"e",limits),child(xml,n,"deg",limits)) }
        }
        "limLow"|"limUpp" => { let tag = if name == "limLow" {"munder"} else {"mover"}; let accent = if name == "limLow" { "accentunder" } else { "accent" }; format!("<{tag} {accent}=\"false\">{}{}</{tag}>",child(xml,n,"e",limits),child(xml,n,"lim",limits)) }
        "nary" => {
            let chr=prop(xml,n,"naryPr","chr").unwrap_or("∫");
            let control=xml.child_local(n,"naryPr").and_then(|p|xml.child_local(p,"ctrlPr")).and_then(|p|xml.child_w(p,"rPr"));
            let enabled=|name:&str|control.and_then(|p|xml.child_w(p,name)).is_some_and(|p|!matches!(xml.attr(p,"val"),Some("0"|"false"|"off")));
            let variant=match (enabled("b"),enabled("i")) {(true,true)=>"bold-italic",(true,false)=>"bold",(false,true)=>"italic",_=>"normal"};
            let op=if control.is_some(){format!("<mo mathvariant=\"{variant}\">{}</mo>",escape_text(chr))}else{token("mo",chr)};
            let sub = !matches!(prop(xml,n,"naryPr","subHide"),Some("1"|"true"|"on")) && xml.child_local(n,"sub").is_some_and(|s| !xml.doc.text_content(s).is_empty());
            let sup = !matches!(prop(xml,n,"naryPr","supHide"),Some("1"|"true"|"on")) && xml.child_local(n,"sup").is_some_and(|s| !xml.doc.text_content(s).is_empty());
            let chr=prop(xml,n,"naryPr","chr").unwrap_or("∫");
            let under=xml.child_local(n,"naryPr").and_then(|p|xml.child_local(p,"limLoc")).map(|p|xml.attr(p,"val").unwrap_or("undOvr")=="undOvr").unwrap_or(if matches!(chr,"∫"|"∬"|"∭"|"∮"|"∯"|"∰") {limits.integral_under} else {!limits.nary_side});
            let (both,low,high)=if under {("munderover","munder","mover")}else{("msubsup","msub","msup")};
            let rendered_limits = match (sub,sup) {
                (true,true) => format!("<{both}>{op}{}{}</{both}>",child(xml,n,"sub",limits),child(xml,n,"sup",limits)),
                (true,false) => format!("<{low}>{op}{}</{low}>",child(xml,n,"sub",limits)),
                (false,true) => format!("<{high}>{op}{}</{high}>",child(xml,n,"sup",limits)),
                _ => op,
            };
            format!("<mrow>{rendered_limits}{}</mrow>",child(xml,n,"e",limits))
        }
        "acc"|"bar"|"groupChr" => {
            let bag = format!("{name}Pr");
            let below = prop(xml,n,&bag,"pos") == Some("bot");
            let tag = if below {"munder"} else {"mover"};
            let chr = prop(xml,n,&bag,"chr").unwrap_or(match name {"bar"=>"¯","groupChr" if below=>"⏟","groupChr"=>"⏞",_=>"ˆ"});
            let accent=if below {"accentunder"} else {"accent"};
            format!("<{tag} {accent}=\"true\">{}<mo stretchy=\"true\">{}</mo></{tag}>",child(xml,n,"e",limits),escape_text(chr))
        }
        "d" => {
            let beg = escape_text(prop(xml,n,"dPr","begChr").unwrap_or("("));
            let end = escape_text(prop(xml,n,"dPr","endChr").unwrap_or(")"));
            let sep = escape_text(prop(xml,n,"dPr","sepChr").unwrap_or("|"));
            let entries: Vec<_> = xml.doc.children(n).filter(|&c| xml.is_local(c,"e")).map(|c| format!("<mrow>{}</mrow>",children(xml,c,limits))).collect();
            format!("<mrow><mo stretchy=\"true\" form=\"prefix\">{beg}</mo>{}<mo stretchy=\"true\" form=\"postfix\">{end}</mo></mrow>",entries.join(&format!("<mo>{sep}</mo>")))
        }
        "m" => {
            let rows: String = xml.doc.children(n).filter(|&c| xml.is_local(c,"mr")).map(|r| {
                let cells: String = xml.doc.children(r).filter(|&c| xml.is_local(c,"e")).map(|c| format!("<mtd>{}</mtd>",children(xml,c,limits))).collect();
                format!("<mtr>{cells}</mtr>")
            }).collect();
            format!("<mtable>{rows}</mtable>")
        }
        "eqArr" => {
            let rows: String = xml.doc.children(n).filter(|&c| xml.is_local(c,"e")).map(|c| format!("<mtr><mtd>{}</mtd></mtr>",children(xml,c,limits))).collect();
            format!("<mtable>{rows}</mtable>")
        }
        "func" => format!("<mrow>{}<mo>\u{2061}</mo>{}</mrow>",child(xml,n,"fName",limits),child(xml,n,"e",limits)),
        "phant" => format!("<mphantom>{}</mphantom>",child(xml,n,"e",limits)),
        "borderBox" => format!("<menclose notation=\"box\">{}</menclose>",child(xml,n,"e",limits)),
        _ => format!("<mtext data-docx-unsupported=\"{}\">{}</mtext>",escape_attr(name),escape_text(&xml.doc.text_content(n))),
    }
}
