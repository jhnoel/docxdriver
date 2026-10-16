//! Section identities follow their terminating paragraph, so preceding inserts do not renumber handles.
use crate::{
    document::{DocxXml, ParaIdIndex},
    html::chrome::{chrome_map, sect_pr_nodes},
    outcome::Outcome,
    package::Package,
};
use serde_json::{json, Value};
use xmloxide::tree::NodeId;

fn boundary_paragraph(x: &DocxXml, sect: NodeId) -> Option<NodeId> {
    let ppr = x.doc.parent(sect)?;
    let p = x.doc.parent(ppr)?;
    x.is_w(p, "p").then_some(p)
}
pub(crate) fn inventory(pkg: &Package, x: &DocxXml, ids: &ParaIdIndex, original: bool) -> Value {
    let chrome = chrome_map(pkg, x);
    let mut start = 0;
    let paragraphs = x.paragraphs();
    let positions: std::collections::HashMap<_, _> = paragraphs
        .iter()
        .enumerate()
        .map(|(i, p)| (p.node, i))
        .collect();
    let mut values:Vec<Value>=sect_pr_nodes(x).into_iter().enumerate().map(|(i,direct)|{
        let anchor=boundary_paragraph(x,direct);
        let id=anchor.and_then(|p|ids.ids.get(&p).map(|v|v.0.clone()));
        let handle=id.as_ref().map(|id|format!("section:{id}")).unwrap_or_else(||"section:final".into());
        let end=anchor.and_then(|p|positions.get(&p).copied()).map(|v|v+1).unwrap_or(paragraphs.len());
        let sect=if original {x.child_w(direct,"sectPrChange").and_then(|n|x.child_w(n,"sectPr")).unwrap_or(direct)}else{direct};
        let attrs=|name:&str|->Value {x.child_w(sect,name).map(|n|{
            let mut object:serde_json::Map<String,Value>=x.doc.attributes(n).iter().map(|a|(a.name.rsplit(':').next().unwrap_or(&a.name).to_string(),json!(a.value))).collect();
            if name=="cols" {object.insert("definitions".into(),json!(x.doc.children(n).filter(|&c|x.is_w(c,"col")).map(|c|json!({"width_twips":x.attr(c,"w"),"spacing_twips":x.attr(c,"space")})).collect::<Vec<_>>()));}
            Value::Object(object)
        }).unwrap_or(Value::Null)};
        let slots=chrome.sections.get(i).map(|s|s.slots.iter().map(|s|json!({"scope":s.scope,"kind":s.kind,"part":s.part,"explicit":s.explicit,"inherited":!s.explicit})).collect::<Vec<_>>()).unwrap_or_default();
        let value=json!({"handle":handle,"index":i+1,"boundary_after":id,"paragraph_start":start,"paragraph_end":end,"final":anchor.is_none(),"page_size":attrs("pgSz"),"margins":attrs("pgMar"),"columns":attrs("cols"),"page_numbering":attrs("pgNumType"),"break_type":x.child_w(sect,"type").and_then(|n|x.attr(n,"val")),"first_page":x.child_w(sect,"titlePg").is_some_and(|n|!matches!(x.attr(n,"val"),Some("0"|"false"|"off"))),"stories":slots});start=end;value
    }).collect();
    if !x.body().is_some_and(|b|x.child_w(b,"sectPr").is_some()) {
        let stories=chrome.sections.last().map(|s|s.slots.iter().map(|s|json!({"scope":s.scope,"kind":s.kind,"part":s.part,"explicit":false,"inherited":true})).collect::<Vec<_>>()).unwrap_or_default();
        values.push(json!({"handle":"section:final","index":values.len()+1,"boundary_after":null,"paragraph_start":start,"paragraph_end":paragraphs.len(),"final":true,"implicit":true,"page_size":null,"margins":null,"columns":null,"page_numbering":null,"break_type":null,"first_page":false,"stories":stories}));
    }
    Value::Array(values)
}
pub(crate) fn ensure_final(x:&mut DocxXml)->Result<(),Outcome>{
    let body=x.body().ok_or_else(||Outcome::error("document has no body"))?;
    if x.child_w(body,"sectPr").is_none(){let sect=x.doc.create_element("w:sectPr");x.doc.append_child(body,sect);}
    Ok(())
}
pub(crate) fn select(
    x: &DocxXml,
    section: Option<u32>,
    handle: Option<&str>,
) -> Result<usize, Outcome> {
    let Some(handle) = handle else {
        return Ok(section.unwrap_or(1) as usize);
    };
    let id = handle
        .strip_prefix("section:")
        .ok_or_else(|| Outcome::error("invalid section_handle"))?;
    let nodes = sect_pr_nodes(x);
    nodes
        .iter()
        .position(|&s| {
            if id == "final" {
                boundary_paragraph(x, s).is_none()
            } else {
                boundary_paragraph(x, s)
                    .is_some_and(|p| x.para_id(p).is_some_and(|v| v.eq_ignore_ascii_case(id)))
            }
        })
        .map(|v| v + 1)
        .ok_or_else(|| Outcome::error("section_handle no longer identifies a section boundary"))
}
pub(crate) fn boundary(
    pkg: &Package,
    x: &mut DocxXml,
    source: Option<[u8; 32]>,
    at: &str,
    remove: bool,
    kind: Option<&str>,
    tracked: bool,
) -> Result<(String, Value), Outcome> {
    if tracked {
        return Err(Outcome::blocked("section boundary edits require change_mode=direct; structural section revisions are unsupported"));
    }
    let p = super::typed::resolve_paragraph_by_para_id(x, pkg, source, at)?;
    if x.doc.parent(p) != x.body() {
        return Err(Outcome::blocked(
            "section boundaries require a body-level paragraph",
        ));
    }
    let ppr = x.child_w(p, "pPr");
    if ppr.is_some_and(|n| x.child_w(n, "pPrChange").is_some()) {
        return Err(Outcome::blocked(
            "settle paragraph property changes before editing a section boundary",
        ));
    }
    let existing = ppr.and_then(|n| x.child_w(n, "sectPr"));
    ensure_final(x)?;
    let nodes = sect_pr_nodes(x);
    let paragraphs = x.paragraphs();
    let index = paragraphs.iter().position(|v| v.node == p).unwrap();
    let following = nodes
        .iter()
        .copied()
        .find(|&n| {
            boundary_paragraph(x, n)
                .map(|b| paragraphs.iter().position(|v| v.node == b).unwrap_or(0) >= index)
                .unwrap_or(true)
        })
        .ok_or_else(|| Outcome::error("document has no final section properties"))?;
    if x.child_w(following, "sectPrChange").is_some() {
        return Err(Outcome::blocked(
            "settle section property changes before boundary edits",
        ));
    }
    if remove {
        let old = existing
            .ok_or_else(|| Outcome::error("paragraph has no section boundary to remove"))?;
        let next = nodes
            .iter()
            .position(|&v| v == old)
            .and_then(|i| nodes.get(i + 1))
            .copied()
            .ok_or_else(|| Outcome::error("cannot remove final body section"))?;
        if x.child_w(next, "sectPrChange").is_some() {
            return Err(Outcome::blocked(
                "settle following section property changes before merging",
            ));
        }
        // Preserve the following section's effective story chains when removing a link.
        for c in x.doc.children(old).collect::<Vec<_>>() {
            let name = if x.is_w(c, "headerReference") {
                Some("headerReference")
            } else if x.is_w(c, "footerReference") {
                Some("footerReference")
            } else {
                None
            };
            if let Some(name) = name {
                let kind = x.attr(c, "type").unwrap_or("default").to_string();
                if !x
                    .doc
                    .children(next)
                    .any(|v| x.is_w(v, name) && x.attr(v, "type").unwrap_or("default") == kind)
                {
                    let clone = x.doc.clone_node(c, true);
                    let successor = x.doc.children(next).find(|&n| {
                        if name == "headerReference" {
                            !x.is_w(n, "headerReference")
                        } else {
                            !x.is_w(n, "headerReference") && !x.is_w(n, "footerReference")
                        }
                    });
                    if let Some(n) = successor {
                        x.doc.insert_before(n, clone)
                    } else {
                        x.doc.append_child(next, clone)
                    }
                }
            }
        }
        x.doc.detach(old);
    } else {
        if existing.is_some() {
            return Err(Outcome::error("paragraph already terminates a section"));
        }
        let clone = x.doc.clone_node(following, true);
        if let Some(kind) = kind {
            let n = super::layout::ensure_sect_pr_child(x, clone, "type");
            super::layout::set_attr(x, n, "w:val", kind);
        }
        let ppr = super::layout::ensure_ppr(x, p);
        let placeholder = super::layout::ensure_ppr_child(x, ppr, "sectPr");
        x.doc.insert_before(placeholder, clone);
        x.doc.detach(placeholder);
    }
    Ok((
        "section boundary updated".into(),
        json!({"at":at,"removed":remove,"section_handle":format!("section:{at}"),"tracked":false}),
    ))
}
