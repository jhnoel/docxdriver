//! Explicit list membership. Edits only append definitions, so tracked
//! rejection can restore prior references without invalidating another
//! paragraph's list; reject-all then drops the definitions nothing refers
//! to any more (`drop_orphaned_lists`).
use super::{
    layout::{ensure_ppr, ensure_ppr_child},
    typed::resolve_paragraph_by_para_id,
    ExecCtx,
};
use crate::{document::DocxXml, outcome::Outcome, package::Package};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use xmloxide::tree::NodeId;

fn parse(bytes: &[u8]) -> Result<DocxXml, Outcome> {
    DocxXml::parse(bytes).map_err(Outcome::error)
}
fn child(xml: &mut DocxXml, parent: xmloxide::tree::NodeId, name: &str, value: &str) {
    let node = xml.doc.create_element(&format!("w:{name}"));
    xml.doc.set_attribute(node, "w:val", value);
    xml.doc.append_child(parent, node);
}

/// Per-plan append-only numbering working set. Only flush at report/emit boundaries.
pub(crate) struct ListState {
    protected_lists: HashSet<String>,
    numbering: Option<NumberingState>,
}
impl ListState {
    pub(crate) fn new(xml: &DocxXml) -> Self {
        let protected_lists = xml
            .doc
            .descendants(xml.doc.root())
            .filter(|&n| xml.is_w(n, "bookmarkStart"))
            .filter_map(|n| {
                xml.attr(n, "name")?
                    .strip_prefix("_docxdriver_cont_")?
                    .split('_')
                    .next()
                    .map(str::to_owned)
            })
            .collect();
        Self {
            protected_lists,
            numbering: None,
        }
    }
    pub(crate) fn flush(&mut self, package: &mut Package) {
        if let Some(state) = &mut self.numbering {
            if state.dirty {
                package.set(&state.part, state.xml.serialize());
                state.dirty = false;
            }
        }
    }
}
struct NumberingState {
    xml: DocxXml,
    part: String,
    root: NodeId,
    nums: HashMap<u32, NodeId>,
    abstracts: HashMap<String, NodeId>,
    max_num: u32,
    max_abstract: u32,
    first_num: Option<NodeId>,
    cleanup: Option<NodeId>,
    dirty: bool,
}
impl NumberingState {
    fn new(package: &mut Package) -> Result<Self, Outcome> {
        let main = package.document_part_name();
        let (dir, file) = main.rsplit_once('/').unwrap_or(("", main.as_str()));
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let rel_path = format!("{prefix}_rels/{file}.rels");
        let mut rels = parse(package.get(&rel_path).unwrap_or(b"<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>"))?;
        let rel_root = rels
            .doc
            .root_element()
            .ok_or_else(|| Outcome::error("missing relationships root"))?;
        let existing_rel = rels.doc.children(rel_root).find(|&n| {
            rels.doc
                .attribute(n, "Type")
                .is_some_and(|t| t.ends_with("/numbering"))
        });
        let part = if let Some(rel) = existing_rel {
            let target = rels
                .doc
                .attribute(rel, "Target")
                .ok_or_else(|| Outcome::error("numbering relationship has no target"))?;
            if rels.doc.attribute(rel, "TargetMode") == Some("External") {
                return Err(Outcome::error("external numbering is unsupported"));
            }
            crate::media::resolve_rel_target(&main, target)
        } else {
            format!("{prefix}numbering.xml")
        };
        let numbering = parse(package.get(&part).unwrap_or(b"<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"/>"))?;
        let root = numbering
            .doc
            .root_element()
            .ok_or_else(|| Outcome::error("missing numbering root"))?;
        if existing_rel.is_none() {
            let mut i = 1;
            while rels
                .doc
                .children(rel_root)
                .any(|n| rels.doc.attribute(n, "Id") == Some(format!("rId{i}").as_str()))
            {
                i += 1;
            }
            let rel = rels.doc.create_element("Relationship");
            for (k, v) in [
                ("Id", format!("rId{i}")),
                (
                    "Type",
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering"
                        .into(),
                ),
                ("Target", "numbering.xml".into()),
            ] {
                rels.doc.set_attribute(rel, k, &v);
            }
            rels.doc.append_child(rel_root, rel);
            package.set(&rel_path, rels.serialize());
        }
        let mut types = parse(
            package
                .get("[Content_Types].xml")
                .ok_or_else(|| Outcome::error("missing content types"))?,
        )?;
        let tr = types
            .doc
            .root_element()
            .ok_or_else(|| Outcome::error("missing types root"))?;
        let pn = format!("/{part}");
        if !types
            .doc
            .children(tr)
            .any(|n| types.doc.attribute(n, "PartName") == Some(pn.as_str()))
        {
            let n = types.doc.create_element("Override");
            types.doc.set_attribute(n, "PartName", &pn);
            types.doc.set_attribute(
                n,
                "ContentType",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml",
            );
            types.doc.append_child(tr, n);
            package.set("[Content_Types].xml", types.serialize());
        }

        let mut nums = HashMap::new();
        let mut abstracts = HashMap::new();
        let mut max_num = 0;
        let mut max_abstract = 0;
        let mut first_num = None;
        let mut cleanup = None;
        for node in numbering.doc.children(root) {
            if numbering.is_w(node, "num") {
                if let Some(id) = numbering
                    .attr(node, "numId")
                    .and_then(|v| v.parse::<u32>().ok())
                {
                    nums.insert(id, node);
                    max_num = max_num.max(id);
                    first_num.get_or_insert(node);
                }
            }
            if numbering.is_w(node, "abstractNum") {
                if let Some(id) = numbering.attr(node, "abstractNumId") {
                    abstracts.insert(id.to_owned(), node);
                    if let Ok(value) = id.parse::<u32>() {
                        max_abstract = max_abstract.max(value);
                    }
                }
            }
            if numbering.is_w(node, "numIdMacAtCleanup") {
                cleanup = Some(node);
            }
        }
        Ok(Self {
            xml: numbering,
            part,
            root,
            nums,
            abstracts,
            max_num,
            max_abstract,
            first_num,
            cleanup,
            dirty: false,
        })
    }
}

#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn set_list(
    package: &mut Package,
    xml: &mut DocxXml,
    source: Option<[u8; 32]>,
    at: &[String],
    num_id: Option<u32>,
    format: Option<&str>,
    level: u8,
    start: Option<u32>,
    remove: bool,
    levels: Option<&[crate::api::ListLevel]>,
    author: &str,
    ctx: &ExecCtx,
) -> Result<(String, Value), Outcome> {
    let mut cache = ListState::new(xml);
    let result = set_list_cached(
        package, xml, source, at, num_id, format, level, start, remove, levels, author, ctx,
        &mut cache,
    );
    cache.flush(package);
    result
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn set_list_cached(
    package: &mut Package,
    xml: &mut DocxXml,
    source: Option<[u8; 32]>,
    at: &[String],
    num_id: Option<u32>,
    format: Option<&str>,
    level: u8,
    start: Option<u32>,
    remove: bool,
    levels: Option<&[crate::api::ListLevel]>,
    author: &str,
    ctx: &ExecCtx,
    cache: &mut ListState,
) -> Result<(String, Value), Outcome> {
    // Resolve every target and reject conflicting revisions before any write.
    let protected_lists = &cache.protected_lists;
    if num_id.is_some_and(|id| protected_lists.contains(&id.to_string())) {
        return Err(Outcome::error(
            "set_list cannot target a list with explicit continuation paragraphs",
        ));
    }
    let mut targets = Vec::new();
    for address in at {
        let paragraph = resolve_paragraph_by_para_id(xml, package, source, address)?;
        if xml.doc.children(paragraph).any(|n| {
            xml.is_w(n, "bookmarkStart")
                && xml
                    .attr(n, "name")
                    .is_some_and(|v| v.starts_with("_docxdriver_cont_"))
        }) {
            return Err(Outcome::error("set_list cannot change explicit continuation membership independently; typed membership changes on multi-paragraph lists are unsupported"));
        }
        let current_list = xml
            .child_w(paragraph, "pPr")
            .and_then(|n| xml.child_w(n, "numPr"))
            .and_then(|n| xml.child_w(n, "numId"))
            .and_then(|n| xml.attr(n, "val"));
        if current_list.is_some_and(|id| protected_lists.contains(id)) {
            return Err(Outcome::error("set_list cannot change a list with explicit continuation paragraphs; typed membership changes on multi-paragraph lists are unsupported"));
        }
        if targets.contains(&paragraph) {
            return Err(Outcome::error("set_list contains a duplicate paragraph"));
        }
        if xml
            .child_w(paragraph, "pPr")
            .is_some_and(|p| xml.child_w(p, "pPrChange").is_some())
        {
            return Err(Outcome::error(
                "set_list target has a pending paragraph property revision; settle it first",
            ));
        }
        targets.push(paragraph);
    }
    let mut assigned = 0;
    if !remove {
        if cache.numbering.is_none() {
            cache.numbering = Some(NumberingState::new(package)?);
        }
        let state = cache.numbering.as_mut().unwrap();
        let numbering = &mut state.xml;
        let root = state.root;
        let next_num = if num_id.is_none() || start.is_some() {
            state
                .max_num
                .checked_add(1)
                .filter(|v| *v <= i32::MAX as u32)
                .ok_or_else(|| Outcome::error("numbering IDs exhausted"))?
        } else {
            num_id.unwrap()
        };
        let requested = num_id.and_then(|id| state.nums.get(&id).copied());
        if num_id.is_some() && requested.is_none() {
            return Err(Outcome::error("set_list num_id does not exist"));
        }
        if let Some(existing) = requested {
            let abstract_id = numbering
                .child_w(existing, "abstractNumId")
                .and_then(|n| numbering.attr(n, "val"));
            let abstract_node = abstract_id.and_then(|id| state.abstracts.get(id).copied());
            let level_text = level.to_string();
            let has_level = abstract_node.is_some_and(|n| {
                numbering.doc.children(n).any(|c| {
                    numbering.is_w(c, "lvl")
                        && numbering.attr(c, "ilvl") == Some(level_text.as_str())
                })
            }) || numbering.doc.children(existing).any(|n| {
                numbering.is_w(n, "lvlOverride")
                    && numbering.attr(n, "ilvl") == Some(level_text.as_str())
                    && numbering.child_w(n, "lvl").is_some()
            });
            if !has_level {
                return Err(Outcome::error(
                    "requested level is not defined by the existing list",
                ));
            }
            if start.is_none() {
                assigned = num_id.unwrap();
            } else {
                // Clone the instance and add a level override: never change a shared definition.
                assigned = next_num;
                let clone = numbering.doc.clone_node(existing, true);
                super::layout::set_attr(numbering, clone, "w:numId", &assigned.to_string());
                let over = numbering.doc.children(clone).find(|&n| {
                    numbering.is_w(n, "lvlOverride")
                        && numbering.attr(n, "ilvl") == Some(level.to_string().as_str())
                });
                let over = over.unwrap_or_else(|| {
                    let n = numbering.doc.create_element("w:lvlOverride");
                    numbering.doc.set_attribute(n, "w:ilvl", &level.to_string());
                    numbering.doc.append_child(clone, n);
                    n
                });
                if let Some(old) = numbering.child_w(over, "startOverride") {
                    numbering.doc.detach(old);
                }
                let n = numbering.doc.create_element("w:startOverride");
                numbering
                    .doc
                    .set_attribute(n, "w:val", &start.unwrap().to_string());
                if let Some(first) = numbering.doc.children(over).next() {
                    numbering.doc.insert_before(first, n);
                } else {
                    numbering.doc.append_child(over, n);
                }
                // CT_Numbering requires all num instances before numIdMacAtCleanup.
                if let Some(last) = state.cleanup {
                    numbering.doc.insert_before(last, clone);
                } else {
                    numbering.doc.append_child(root, clone);
                }
                state.nums.insert(assigned, clone);
                state.first_num.get_or_insert(clone);
                state.max_num = assigned;
                state.dirty = true;
            }
        } else {
            assigned = next_num;
            let abs_id = state
                .max_abstract
                .checked_add(1)
                .filter(|v| *v <= i32::MAX as u32)
                .ok_or_else(|| Outcome::error("abstract IDs exhausted"))?;
            let abstract_node = numbering.doc.create_element("w:abstractNum");
            numbering
                .doc
                .set_attribute(abstract_node, "w:abstractNumId", &abs_id.to_string());
            child(numbering, abstract_node, "multiLevelType", "multilevel");
            for lev in 0..levels.map_or(9, |v| v.len()) {
                let def = levels.and_then(|v| v.get(lev));
                let lvl = numbering.doc.create_element("w:lvl");
                numbering.doc.set_attribute(lvl, "w:ilvl", &lev.to_string());
                child(
                    numbering,
                    lvl,
                    "start",
                    &def.and_then(|v| v.start)
                        .or_else(|| if lev == level as usize { start } else { None })
                        .unwrap_or(1)
                        .to_string(),
                );
                let fmt = def
                    .map(|v| v.format.as_str())
                    .or(format)
                    .unwrap_or("decimal");
                child(numbering, lvl, "numFmt", fmt);
                if let Some(after) = def.and_then(|v| v.restart_after) {
                    child(
                        numbering,
                        lvl,
                        "lvlRestart",
                        &(after as u32 + 1).to_string(),
                    );
                }
                let label = def.map(|v| v.label.clone()).unwrap_or_else(|| {
                    if fmt == "bullet" {
                        "•".into()
                    } else {
                        format!("%{}.", lev + 1)
                    }
                });
                child(numbering, lvl, "lvlText", &label);
                if let Some(def) = def {
                    if def.indent_left.is_some()
                        || def.indent_right.is_some()
                        || def.first_line_indent.is_some()
                    {
                        let ppr = numbering.doc.create_element("w:pPr");
                        let ind = numbering.doc.create_element("w:ind");
                        for (key, value) in [("left", def.indent_left), ("right", def.indent_right)]
                        {
                            if let Some(value) = value {
                                numbering.doc.set_attribute(
                                    ind,
                                    &format!("w:{key}"),
                                    &value.0.to_string(),
                                );
                            }
                        }
                        if let Some(v) = def.first_line_indent {
                            numbering.doc.set_attribute(
                                ind,
                                if v.0 < 0 { "w:hanging" } else { "w:firstLine" },
                                &v.0.abs().to_string(),
                            );
                        }
                        numbering.doc.append_child(ppr, ind);
                        numbering.doc.append_child(lvl, ppr);
                    }
                    if def.marker_bold.is_some()
                        || def.marker_italic.is_some()
                        || def.marker_font.is_some()
                        || def.marker_color.is_some()
                    {
                        let rpr = numbering.doc.create_element("w:rPr");
                        if let Some(font) = &def.marker_font {
                            let n = numbering.doc.create_element("w:rFonts");
                            numbering.doc.set_attribute(n, "w:ascii", font);
                            numbering.doc.set_attribute(n, "w:hAnsi", font);
                            numbering.doc.append_child(rpr, n);
                        }
                        for (key, value) in [("b", def.marker_bold), ("i", def.marker_italic)] {
                            if let Some(value) = value {
                                child(numbering, rpr, key, if value { "1" } else { "0" });
                            }
                        }
                        if let Some(color) = &def.marker_color {
                            child(numbering, rpr, "color", color);
                        }
                        numbering.doc.append_child(lvl, rpr);
                    }
                }
                numbering.doc.append_child(abstract_node, lvl);
            }
            if let Some(successor) = state.first_num.or(state.cleanup) {
                numbering.doc.insert_before(successor, abstract_node);
            } else {
                numbering.doc.append_child(root, abstract_node);
            }
            let num = numbering.doc.create_element("w:num");
            numbering
                .doc
                .set_attribute(num, "w:numId", &assigned.to_string());
            child(numbering, num, "abstractNumId", &abs_id.to_string());
            if let Some(last) = state.cleanup {
                numbering.doc.insert_before(last, num);
            } else {
                numbering.doc.append_child(root, num);
            }
            state.nums.insert(assigned, num);
            state.abstracts.insert(abs_id.to_string(), abstract_node);
            state.first_num.get_or_insert(num);
            state.max_num = assigned;
            state.max_abstract = abs_id;
            state.dirty = true;
        }
    }
    for paragraph in targets {
        let snapshot = xml.doc.create_element("w:pPr");
        if let Some(ppr) = xml.child_w(paragraph, "pPr") {
            for c in xml.doc.children(ppr).collect::<Vec<_>>() {
                if ["rPr", "sectPr", "pPrChange"]
                    .iter()
                    .any(|name| xml.is_w(c, name))
                {
                    continue;
                }
                let clone = xml.doc.clone_node(c, true);
                xml.doc.append_child(snapshot, clone);
            }
        }
        let ppr = ensure_ppr(xml, paragraph);
        // Replace only membership; preserve indents and every other pPr child.
        let np = ensure_ppr_child(xml, ppr, "numPr");
        for c in xml.doc.children(np).collect::<Vec<_>>() {
            if xml.is_w(c, "ilvl") || xml.is_w(c, "numId") {
                xml.doc.detach(c);
            }
        }
        let num = xml.doc.create_element("w:numId");
        xml.doc.set_attribute(num, "w:val", &assigned.to_string());
        if let Some(first) = xml.doc.children(np).next() {
            xml.doc.insert_before(first, num);
        } else {
            xml.doc.append_child(np, num);
        }
        if !remove {
            let lev = xml.doc.create_element("w:ilvl");
            xml.doc.set_attribute(lev, "w:val", &level.to_string());
            xml.doc.insert_before(num, lev);
        }
        let id = xml.next_revision_id();
        let change = xml.doc.create_element("w:pPrChange");
        for (key, val) in [
            ("w:id", id.to_string()),
            ("w:author", author.into()),
            ("w:date", ctx.now.clone()),
        ] {
            xml.doc.set_attribute(change, key, &val);
        }
        xml.doc.append_child(change, snapshot);
        xml.doc.append_child(ppr, change);
    }
    Ok((
        "updated list membership".into(),
        json!({"num_id":assigned,"paragraphs":at,"tracked":true}),
    ))
}

/// Every `w:numId` value the document refers to: the main document part
/// (from the working `xml`, pending property snapshots included) and every
/// other WordprocessingML part (headers, footers, notes, comments, styles).
pub(crate) fn referenced_num_ids(package: &Package, xml: &DocxXml) -> std::collections::HashSet<String> {
    let mut ids = std::collections::HashSet::new();
    let mut collect = |x: &DocxXml| {
        let root = x.doc.root();
        for node in x.doc.descendants(root) {
            if x.is_w(node, "numId") {
                if let Some(value) = x.attr(node, "val") {
                    ids.insert(value.to_string());
                }
            }
        }
    };
    collect(xml);
    let main = package.document_part_name();
    let numbering = crate::html::ctx::numbering_part_name(package);
    for (name, bytes) in package.parts() {
        if name == main || name == numbering || !name.starts_with("word/") || !name.ends_with(".xml") {
            continue;
        }
        if let Ok(part) = DocxXml::parse(bytes) {
            collect(&part);
        }
    }
    ids
}

/// Remove the list instances (`w:num`) named in `orphaned`, and each
/// abstract definition left with no instance that is not linked to a
/// numbering style: reject-all takes back the definitions a rejected list
/// change added. Only instances that were referenced before settling and
/// are not after it are passed in; definitions the input already left
/// unreferenced stay. Returns how many elements were removed.
pub(crate) fn drop_orphaned_lists(package: &mut Package, orphaned: &std::collections::HashSet<String>) -> usize {
    if orphaned.is_empty() {
        return 0;
    }
    let part = crate::html::ctx::numbering_part_name(package);
    let Some(mut numbering) = package.get(&part).and_then(|bytes| DocxXml::parse(bytes).ok()) else {
        return 0;
    };
    let Some(root) = numbering.doc.root_element() else { return 0 };
    let mut removed = 0;
    let mut abstracts = std::collections::HashSet::new();
    for num in numbering.doc.children(root).filter(|&n| numbering.is_w(n, "num")).collect::<Vec<_>>() {
        if numbering.attr(num, "numId").is_some_and(|id| orphaned.contains(id)) {
            if let Some(abstract_id) = numbering.child_w(num, "abstractNumId").and_then(|n| numbering.attr(n, "val")) {
                abstracts.insert(abstract_id.to_string());
            }
            numbering.doc.remove_node(num);
            removed += 1;
        }
    }
    let still_used: std::collections::HashSet<String> = numbering
        .doc
        .children(root)
        .filter(|&n| numbering.is_w(n, "num"))
        .filter_map(|n| numbering.child_w(n, "abstractNumId").and_then(|a| numbering.attr(a, "val")).map(str::to_owned))
        .collect();
    for definition in numbering.doc.children(root).filter(|&n| numbering.is_w(n, "abstractNum")).collect::<Vec<_>>() {
        let Some(id) = numbering.attr(definition, "abstractNumId").map(str::to_owned) else { continue };
        let linked = numbering.child_w(definition, "styleLink").is_some() || numbering.child_w(definition, "numStyleLink").is_some();
        if abstracts.contains(&id) && !still_used.contains(&id) && !linked {
            numbering.doc.remove_node(definition);
            removed += 1;
        }
    }
    if removed > 0 {
        package.set(&part, numbering.serialize());
    }
    removed
}
