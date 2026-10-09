//! Paragraph-local equation discovery and atomic, reviewable equation edits.
use super::{typed::resolve_paragraph_by_para_id, ExecCtx};
use crate::html::{mathml_omml::mathml_to_omml_xml, omml_mathml::omml_to_mathml};
use crate::{document::DocxXml, outcome::Outcome, package::Package};
use serde_json::{json, Value};
use xmloxide::tree::NodeId;

fn current_equations(xml: &DocxXml, paragraph: NodeId) -> Vec<NodeId> {
    xml.doc
        .descendants(paragraph)
        .filter(|&node| {
            if !(xml.is_local(node, "oMath") || xml.is_local(node, "oMathPara")) {
                return false;
            }
            let mut parent = xml.doc.parent(node);
            while let Some(p) = parent {
                if p == paragraph {
                    return true;
                }
                if xml.is_rev_del(p) || xml.is_local(p, "oMathPara") || xml.is_w(p, "p") {
                    return false;
                }
                parent = xml.doc.parent(p);
            }
            false
        })
        .collect()
}

pub(super) fn inspect(
    xml: &DocxXml,
    paragraph: NodeId,
    limits: crate::html::omml_mathml::Limits,
) -> Vec<Value> {
    current_equations(xml, paragraph)
        .iter()
        .enumerate()
        .map(|(i, &node)| {
            json!({"equation": i + 1, "mathml": omml_to_mathml(xml, node, limits),
            "display": xml.is_local(node, "oMathPara"),
            "editable": xml.doc.parent(node) == Some(paragraph)})
        })
        .collect()
}

/// Addresses and editability only; equation content is already in document HTML.
pub(crate) fn descriptors(xml: &DocxXml, paragraph: NodeId) -> Vec<Value> {
    current_equations(xml, paragraph)
        .iter()
        .enumerate()
        .map(|(i, &node)| {
            json!({"equation": i + 1, "display": xml.is_local(node, "oMathPara"),
            "editable": xml.doc.parent(node) == Some(paragraph)})
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn replace(
    package: &mut Package,
    xml: &mut DocxXml,
    source: Option<[u8; 32]>,
    at: &str,
    mathml: &str,
    equation: Option<u32>,
    display: Option<bool>,
    tracked: bool,
    author: &str,
    ctx: &ExecCtx,
) -> Result<(String, Value), Outcome> {
    let paragraph = resolve_paragraph_by_para_id(xml, package, source, at)?;
    let candidates = current_equations(xml, paragraph);
    let blocked = |message: String| Outcome {
        result: Some(json!({"paraId": at})),
        ..Outcome::blocked(message)
    };
    let index = match (equation, candidates.len()) {
        (None, 1) => 0,
        (Some(n), len) if n > 0 && n as usize <= len => n as usize - 1,
        (_, 0) => return Err(blocked(format!("paragraph {at} has no current equations"))),
        (None, n) => return Err(blocked(format!("paragraph {at} has {n} equations; choose equation (1-based) from the paragraph's equations listing"))),
        (Some(n), len) => return Err(blocked(format!("equation {n} is out of range; paragraph {at} has {len} current equations"))),
    };
    let target = candidates[index];
    if xml.doc.parent(target) != Some(paragraph) {
        return Err(blocked("equation is inside a revision or structured wrapper; settle its tracked changes or unwrap it before replacing".into()));
    }
    let before = omml_to_mathml(xml, target, crate::html::omml_mathml::Limits::load(package));
    let display = display.unwrap_or_else(|| xml.is_local(target, "oMathPara"));
    let compiled = mathml_to_omml_xml(mathml, display).map_err(Outcome::error)?;
    // Compile first; no content changes are made on invalid input.
    let holder = xml.doc.create_element("w:ins");
    crate::segments::append_omml_xml(xml, holder, &compiled).map_err(Outcome::error)?;
    if tracked {
        let del_id = xml.next_revision_id();
        let deletion = xml.doc.create_element("w:del");
        xml.doc.set_attribute(deletion, "w:id", &del_id.to_string());
        xml.doc.set_attribute(deletion, "w:author", author);
        xml.doc.set_attribute(deletion, "w:date", &ctx.now);
        xml.doc.insert_before(target, deletion);
        xml.doc.insert_before(target, holder);
        xml.doc.detach(target);
        xml.doc.append_child(deletion, target);
        let ins_id = xml.next_revision_id();
        xml.doc.set_attribute(holder, "w:id", &ins_id.to_string());
        xml.doc.set_attribute(holder, "w:author", author);
        xml.doc.set_attribute(holder, "w:date", &ctx.now);
    } else {
        for child in xml.doc.children(holder).collect::<Vec<_>>() {
            xml.doc.detach(child);
            xml.doc.insert_before(target, child);
        }
        xml.doc.remove_node(target);
        xml.doc.remove_node(holder);
    }
    Ok((
        format!("replaced equation {} in paragraph {at}", index + 1),
        json!({"paraId": at, "equation": index + 1, "before": before, "mathml": mathml, "display": display, "tracked": tracked}),
    ))
}

pub(super) fn delete(
    package: &mut Package,
    xml: &mut DocxXml,
    source: Option<[u8; 32]>,
    at: &str,
    equation: Option<u32>,
    tracked: bool,
    author: &str,
    ctx: &ExecCtx,
) -> Result<(String, Value), Outcome> {
    let paragraph = resolve_paragraph_by_para_id(xml, package, source, at)?;
    let candidates = current_equations(xml, paragraph);
    let blocked = |message: String| Outcome {
        result: Some(json!({"paraId": at})),
        ..Outcome::blocked(message)
    };
    let index = match (equation, candidates.len()) {
        (None, 1) => 0,
        (Some(n), len) if n > 0 && n as usize <= len => n as usize - 1,
        (_, 0) => return Err(blocked(format!("paragraph {at} has no current equations"))),
        (None, n) => return Err(blocked(format!("paragraph {at} has {n} equations; choose equation (1-based) from the paragraph's equations listing"))),
        (Some(n), len) => return Err(blocked(format!("equation {n} is out of range; paragraph {at} has {len} current equations"))),
    };
    let target = candidates[index];
    if xml.doc.parent(target) != Some(paragraph) {
        return Err(blocked("equation is inside a revision or structured wrapper; settle its tracked changes or unwrap it before deleting".into()));
    }
    let before = omml_to_mathml(xml, target, crate::html::omml_mathml::Limits::load(package));
    if tracked {
        let deletion = xml.doc.create_element("w:del");
        let id = xml.next_revision_id();
        xml.doc.set_attribute(deletion, "w:id", &id.to_string());
        xml.doc.set_attribute(deletion, "w:author", author);
        xml.doc.set_attribute(deletion, "w:date", &ctx.now);
        xml.doc.insert_before(target, deletion);
        xml.doc.detach(target);
        xml.doc.append_child(deletion, target);
    } else {
        xml.doc.remove_node(target);
    }
    Ok((
        format!("deleted equation {} in paragraph {at}", index + 1),
        json!({"paraId": at, "equation": index + 1, "before": before, "tracked": tracked}),
    ))
}
