//! Explicit list membership. Definitions are append-only, so tracked rejection
//! can restore prior references without invalidating another paragraph's list.
use serde_json::{json, Value};
use crate::{document::DocxXml, outcome::Outcome, package::Package};
use super::{ExecCtx, layout::{ensure_ppr, ensure_ppr_child}, typed::resolve_paragraph_by_para_id};

fn parse(bytes: &[u8]) -> Result<DocxXml, Outcome> { DocxXml::parse(bytes).map_err(Outcome::error) }
fn child(xml: &mut DocxXml, parent: xmloxide::tree::NodeId, name: &str, value: &str) {
    let node = xml.doc.create_element(&format!("w:{name}"));
    xml.doc.set_attribute(node, "w:val", value);
    xml.doc.append_child(parent, node);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn set_list(package: &mut Package, xml: &mut DocxXml, source: Option<[u8;32]>, at: &[String], num_id: Option<u32>, format: Option<&str>, level: u8, start: Option<u32>, remove: bool, levels: Option<&[crate::api::ListLevel]>, author: &str, ctx: &ExecCtx) -> Result<(String, Value), Outcome> {
    // Resolve every target and reject conflicting revisions before any write.
    let mut targets = Vec::new();
    for address in at {
        let paragraph = resolve_paragraph_by_para_id(xml, package, source, address)?;
        if xml.doc.children(paragraph).any(|n| xml.is_w(n,"bookmarkStart") && xml.attr(n,"name").is_some_and(|v| v.starts_with("_docxdriver_cont_"))) {
            return Err(Outcome::error("set_list cannot change an explicit continuation paragraph independently; target the numbered item paragraph"));
        }
        if targets.contains(&paragraph) { return Err(Outcome::error("set_list contains a duplicate paragraph")); }
        if xml.child_w(paragraph, "pPr").is_some_and(|p| xml.child_w(p, "pPrChange").is_some()) {
            return Err(Outcome::error("set_list target has a pending paragraph property revision; settle it first"));
        }
        targets.push(paragraph);
    }
    let mut assigned = 0;
    if !remove {
        let main = package.document_part_name();
        let (dir, file) = main.rsplit_once('/').unwrap_or(("", main.as_str()));
        let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
        let rel_path = format!("{prefix}_rels/{file}.rels");
        let mut rels = parse(package.get(&rel_path).unwrap_or(b"<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>"))?;
        let rel_root = rels.doc.root_element().ok_or_else(|| Outcome::error("missing relationships root"))?;
        let existing_rel = rels.doc.children(rel_root).find(|&n| rels.doc.attribute(n,"Type").is_some_and(|t| t.ends_with("/numbering")));
        let part = if let Some(rel) = existing_rel {
            let target = rels.doc.attribute(rel,"Target").ok_or_else(|| Outcome::error("numbering relationship has no target"))?;
            if rels.doc.attribute(rel,"TargetMode") == Some("External") { return Err(Outcome::error("external numbering is unsupported")); }
            crate::media::resolve_rel_target(&main, target)
        } else { format!("{prefix}numbering.xml") };
        let mut numbering = parse(package.get(&part).unwrap_or(b"<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"/>"))?;
        let root = numbering.doc.root_element().ok_or_else(|| Outcome::error("missing numbering root"))?;
        let max = |name: &str, attr: &str| numbering.doc.children(root).filter(|&n| numbering.is_w(n,name)).filter_map(|n| numbering.attr(n,attr)?.parse::<u32>().ok()).max().unwrap_or(0);
        let next_num = max("num","numId").checked_add(1).ok_or_else(|| Outcome::error("numbering IDs exhausted"))?;
        let requested = num_id.and_then(|id| numbering.doc.children(root).find(|&n| numbering.is_w(n,"num") && numbering.attr(n,"numId") == Some(id.to_string().as_str())));
        if num_id.is_some() && requested.is_none() { return Err(Outcome::error("set_list num_id does not exist")); }
        if let Some(existing) = requested {
            let abstract_id = numbering.child_w(existing,"abstractNumId").and_then(|n| numbering.attr(n,"val"));
            let abstract_node = numbering.doc.children(root).find(|&n| numbering.is_w(n,"abstractNum") && numbering.attr(n,"abstractNumId") == abstract_id);
            let level_text = level.to_string();
            let has_level = abstract_node.is_some_and(|n| numbering.doc.children(n).any(|c| numbering.is_w(c,"lvl") && numbering.attr(c,"ilvl") == Some(level_text.as_str()))) || numbering.doc.children(existing).any(|n| numbering.is_w(n,"lvlOverride") && numbering.attr(n,"ilvl") == Some(level_text.as_str()) && numbering.child_w(n,"lvl").is_some());
            if !has_level { return Err(Outcome::error("requested level is not defined by the existing list")); }
            if start.is_none() { assigned = num_id.unwrap(); }
            else {
                // Clone the instance and add a level override: never change a shared definition.
                assigned = next_num;
                let clone = numbering.doc.clone_node(existing, true);
                super::layout::set_attr(&mut numbering,clone,"w:numId", &assigned.to_string());
                let over = numbering.doc.children(clone).find(|&n| numbering.is_w(n,"lvlOverride") && numbering.attr(n,"ilvl") == Some(level.to_string().as_str()));
                let over = over.unwrap_or_else(|| { let n = numbering.doc.create_element("w:lvlOverride"); numbering.doc.set_attribute(n,"w:ilvl",&level.to_string()); numbering.doc.append_child(clone,n); n });
                if let Some(old) = numbering.child_w(over,"startOverride") { numbering.doc.detach(old); }
                let n = numbering.doc.create_element("w:startOverride"); numbering.doc.set_attribute(n,"w:val",&start.unwrap().to_string());
                if let Some(first) = numbering.doc.children(over).next() { numbering.doc.insert_before(first,n); } else { numbering.doc.append_child(over,n); }
                // CT_Numbering requires all num instances before numIdMacAtCleanup.
                if let Some(last) = numbering.child_w(root,"numIdMacAtCleanup") { numbering.doc.insert_before(last,clone); } else { numbering.doc.append_child(root,clone); }
            }
        } else {
            assigned = next_num;
            let abs_id = max("abstractNum","abstractNumId").checked_add(1).ok_or_else(|| Outcome::error("abstract IDs exhausted"))?;
            let abstract_node = numbering.doc.create_element("w:abstractNum");
            numbering.doc.set_attribute(abstract_node,"w:abstractNumId",&abs_id.to_string());
            child(&mut numbering,abstract_node,"multiLevelType","multilevel");
            for lev in 0..levels.map_or(9, |v| v.len()) {
                let def = levels.and_then(|v| v.get(lev));
                let lvl = numbering.doc.create_element("w:lvl"); numbering.doc.set_attribute(lvl,"w:ilvl",&lev.to_string());
                child(&mut numbering,lvl,"start",&def.and_then(|v| v.start).or(start).unwrap_or(1).to_string());
                let fmt = def.map(|v| v.format.as_str()).or(format).unwrap_or("decimal");
                child(&mut numbering,lvl,"numFmt",fmt);
                if let Some(after) = def.and_then(|v| v.restart_after) { child(&mut numbering,lvl,"lvlRestart",&(after as u32 + 1).to_string()); }
                let label = def.map(|v| v.label.clone()).unwrap_or_else(|| if fmt == "bullet" { "•".into() } else { format!("%{}.",lev+1) });
                child(&mut numbering,lvl,"lvlText",&label);
                if let Some(def) = def {
                    if def.indent_left.is_some() || def.indent_right.is_some() || def.first_line_indent.is_some() {
                        let ppr = numbering.doc.create_element("w:pPr"); let ind = numbering.doc.create_element("w:ind");
                        for (key, value) in [("left",def.indent_left),("right",def.indent_right)] { if let Some(value) = value { numbering.doc.set_attribute(ind,&format!("w:{key}"),&value.0.to_string()); } }
                        if let Some(v) = def.first_line_indent { numbering.doc.set_attribute(ind,if v.0 < 0 { "w:hanging" } else { "w:firstLine" },&v.0.abs().to_string()); }
                        numbering.doc.append_child(ppr,ind); numbering.doc.append_child(lvl,ppr);
                    }
                    if def.marker_bold.is_some() || def.marker_italic.is_some() || def.marker_font.is_some() || def.marker_color.is_some() {
                        let rpr = numbering.doc.create_element("w:rPr");
                        if let Some(font) = &def.marker_font { let n = numbering.doc.create_element("w:rFonts"); numbering.doc.set_attribute(n,"w:ascii",font); numbering.doc.set_attribute(n,"w:hAnsi",font); numbering.doc.append_child(rpr,n); }
                        for (key,value) in [("b",def.marker_bold),("i",def.marker_italic)] { if let Some(value) = value { child(&mut numbering,rpr,key,if value { "1" } else { "0" }); } }
                        if let Some(color) = &def.marker_color { child(&mut numbering,rpr,"color",color); }
                        numbering.doc.append_child(lvl,rpr);
                    }
                }
                numbering.doc.append_child(abstract_node,lvl);
            }
            if let Some(successor) = numbering.doc.children(root).find(|&n| numbering.is_w(n,"num") || numbering.is_w(n,"numIdMacAtCleanup")) { numbering.doc.insert_before(successor,abstract_node); } else { numbering.doc.append_child(root,abstract_node); }
            let num = numbering.doc.create_element("w:num"); numbering.doc.set_attribute(num,"w:numId",&assigned.to_string()); child(&mut numbering,num,"abstractNumId",&abs_id.to_string());
            if let Some(last) = numbering.child_w(root,"numIdMacAtCleanup") { numbering.doc.insert_before(last,num); } else { numbering.doc.append_child(root,num); }
        }
        if existing_rel.is_none() {
            let mut i = 1;
            while rels.doc.children(rel_root).any(|n| rels.doc.attribute(n,"Id") == Some(format!("rId{i}").as_str())) { i += 1; }
            let rel = rels.doc.create_element("Relationship");
            for (k,v) in [("Id",format!("rId{i}")),("Type","http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering".into()),("Target","numbering.xml".into())] { rels.doc.set_attribute(rel,k,&v); }
            rels.doc.append_child(rel_root,rel); package.set(&rel_path,rels.serialize());
        }
        let mut types = parse(package.get("[Content_Types].xml").ok_or_else(|| Outcome::error("missing content types"))?)?;
        let tr = types.doc.root_element().ok_or_else(|| Outcome::error("missing types root"))?;
        let pn = format!("/{part}");
        if !types.doc.children(tr).any(|n| types.doc.attribute(n,"PartName") == Some(pn.as_str())) {
            let n = types.doc.create_element("Override"); types.doc.set_attribute(n,"PartName",&pn); types.doc.set_attribute(n,"ContentType","application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"); types.doc.append_child(tr,n); package.set("[Content_Types].xml",types.serialize());
        }
        package.set(&part,numbering.serialize());
    }
    for paragraph in targets {
        let snapshot = xml.doc.create_element("w:pPr");
        if let Some(ppr) = xml.child_w(paragraph,"pPr") {
            for c in xml.doc.children(ppr).collect::<Vec<_>>() {
                if ["rPr","sectPr","pPrChange"].iter().any(|name| xml.is_w(c,name)) { continue; }
                let clone = xml.doc.clone_node(c,true); xml.doc.append_child(snapshot,clone);
            }
        }
        let ppr = ensure_ppr(xml,paragraph);
        // Replace only membership; preserve indents and every other pPr child.
        let np = ensure_ppr_child(xml,ppr,"numPr");
        for c in xml.doc.children(np).collect::<Vec<_>>() { if xml.is_w(c,"ilvl") || xml.is_w(c,"numId") { xml.doc.detach(c); } }
        let num = xml.doc.create_element("w:numId"); xml.doc.set_attribute(num,"w:val",&assigned.to_string());
        if let Some(first) = xml.doc.children(np).next() { xml.doc.insert_before(first,num); } else { xml.doc.append_child(np,num); }
        if !remove { let lev = xml.doc.create_element("w:ilvl"); xml.doc.set_attribute(lev,"w:val",&level.to_string()); xml.doc.insert_before(num,lev); }
        let id = xml.next_revision_id();
        let change = xml.doc.create_element("w:pPrChange");
        for (key,val) in [("w:id",id.to_string()),("w:author",author.into()),("w:date",ctx.now.clone())] { xml.doc.set_attribute(change,key,&val); }
        xml.doc.append_child(change,snapshot); xml.doc.append_child(ppr,change);
    }
    Ok(("updated list membership".into(),json!({"num_id":assigned,"paragraphs":at,"tracked":true})))
}
