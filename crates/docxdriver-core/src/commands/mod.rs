//! Internal typed operation runtime: typed commands only.
pub(crate) mod bookmarks;
pub(crate) mod comments;
pub(crate) mod equations;
pub(crate) mod headers;
pub(crate) mod layout;
pub(crate) mod sections;
pub(crate) mod lists;
pub(crate) mod tables;
pub(crate) mod table_structure;
pub(crate) mod reading;
pub(crate) mod readonly;
pub(crate) mod revisions;
pub(crate) mod typed;

use std::collections::HashMap;

use serde_json::Value;
use sha2::Digest;

use crate::document::DocxXml;
use crate::outcome::Outcome;
use crate::package::Package;

/// Execution context shared by typed operations.
pub struct ExecCtx {
    pub dry_run: bool,
    pub now: String,
}

const STRICT_W_NS: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";

/// The parsed working set the operation fold runs over: one unzip + one
/// document.xml parse per typed plan, however many operations follow.
pub(crate) struct WorkState {
    pub(crate) package: Package,
    pub(crate) xml: DocxXml,
    pub(crate) mutated: bool,
    pub(crate) list_state: Option<lists::ListState>,
    pub(crate) table_handles: HashMap<String, xmloxide::tree::NodeId>,
    pub(crate) table_positions: HashMap<xmloxide::tree::NodeId, String>,
    /// sha256 of the exact input package bytes — the inspection `source`, the
    /// stale-plan binding anchor, and the seed for deterministic provisional
    /// paragraph-ID repair. None when the working set was created from nothing
    /// (a create-led operation list).
    pub(crate) source_hash: Option<[u8; 32]>,
}

impl WorkState {
    pub(crate) fn flush_lists(&mut self) {
        if let Some(lists)=&mut self.list_state {lists.flush(&mut self.package);}
    }
    pub(crate) fn parse(bytes: &[u8]) -> Result<WorkState, Outcome> {
        let package =
            Package::from_bytes(bytes).map_err(|error| Outcome::error(error.to_string()))?;
        let mut state = Self::from_package(package)?;
        state.source_hash = Some(package_sha256(bytes));
        Ok(state)
    }

    pub(crate) fn from_package(package: Package) -> Result<WorkState, Outcome> {
        let xml = {
            let bytes = package
                .document_xml()
                .map_err(|error| Outcome::error(error.to_string()))?;
            DocxXml::parse(bytes).map_err(Outcome::error)?
        };
        // ISO Strict markup lives in other namespaces: the engine would read
        // it as empty and write Transitional markup into it.
        if xml.doc.root_element().and_then(|root| xml.doc.node_namespace(root)) == Some(STRICT_W_NS) {
            return Err(Outcome::error(
                "this document is saved as Strict Open XML, which docxdriver does not support; \
                 save it as a standard Word Document (.docx) and try again",
            ));
        }
        let table_positions = table_positions(&xml);
        let table_handles = table_positions.iter().map(|(node, address)| (address.clone(), *node)).collect();
        Ok(WorkState {
            list_state: None,
            table_handles,
            table_positions,
            package,
            xml,
            mutated: false,
            source_hash: None,
        })
    }
}

/// Source handles bind to XML objects, rather than shifting ordinal positions.
pub(crate) fn table_positions(xml: &DocxXml) -> HashMap<xmloxide::tree::NodeId, String> {
    let mut map = HashMap::new();
    for (ti, table) in tables::tables(xml).into_iter().enumerate() {
        let at = format!("table:{}", ti + 1);
        map.insert(table, at.clone());
        for (ri, row) in tables::rows(xml, table).into_iter().enumerate() {
            let ra = format!("{at}/row:{}", ri + 1);
            map.insert(row, ra.clone());
            for (ci, cell) in tables::cells(xml, row).into_iter().enumerate() {
                map.insert(cell, format!("{ra}/cell:{}", ci + 1));
            }
        }
    }
    map
}

/// The relationships part of the main document part (`word/_rels/document.xml.rels`
/// for the conventional name; a renamed main part has its own).
pub(crate) fn document_rels_part(package: &crate::package::Package) -> String {
    let main = package.document_part_name();
    match main.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{main}.rels"),
    }
}

/// sha256 of the exact input package bytes.
pub(crate) fn package_sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}

/// Attach the serialized package to a successful outcome when something
/// actually mutated. The one zip write. A dry run builds the bytes too, so
/// review runs the same integrity gate as commit; callers withhold them.
///
/// Every engine write persists paragraph identity first: existing valid
/// `w14:paraId` values are untouched, while missing/invalid/duplicate and
/// engine-created paragraphs receive the same deterministic values inspection
/// showed — one atomic write of repairs plus content operations.
pub(crate) fn emit(mut state: WorkState, outcome: Outcome, ctx: &ExecCtx) -> Outcome {
    let _ = ctx;
    if outcome.status != crate::outcome::Status::Ok || !state.mutated {
        return outcome;
    }
    state.flush_lists();
    let mut package = state.package;
    let document_part = package.document_part_name();
    let seed_hash = state.source_hash.unwrap_or_else(|| {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        if let Some(bytes) = package.get(&document_part) {
            hasher.update(bytes);
        }
        hasher.finalize().into()
    });
    let index = state.xml.resolve_paragraph_ids(&document_part, &seed_hash);
    if let Err(error) = state.xml.apply_paragraph_ids(&index) {
        return Outcome::error(error);
    }
    package.set(&document_part, state.xml.serialize());
    match package.to_bytes() {
        Ok(bytes) => Outcome {
            bytes: Some(bytes),
            ..outcome
        },
        Err(error) => Outcome::error(error.to_string()),
    }
}

/// Typed edit entry point used by the public Request API. The operation
/// identity is carried by `EditOp`; this adapter is intentionally adjacent to
/// the operation implementations rather than part of the public API module.
/// Seed for deterministic per-plan allocations (inserted paragraph ids):
/// the sha256 of the canonical plan JSON. Computed once per plan.
pub(crate) fn plan_seed(plan: &crate::api::Plan) -> [u8; 32] {
    sha2::Sha256::digest(
        crate::api::canonical_json(plan)
            .unwrap_or_default()
            .as_bytes(),
    )
    .into()
}

pub(crate) fn run_typed_edit(
    state: &mut WorkState, op: &crate::api::EditOp, author: &str,
    mode: crate::api::ChangeMode, index: usize, source: &[u8;32],
    seed: &[u8;32], ctx: &ExecCtx,
) -> Outcome {
    use crate::api::EditOp;
    let mut resolved = op.clone();
    let target = match &mut resolved {
        EditOp::FormatTable { at, .. } | EditOp::FormatRow { at, .. }
        | EditOp::FormatCell { at, .. } | EditOp::EditTable { at, .. } => Some(at),
        _ => None,
    };
    if let Some(at) = target {
        let Some(node) = state.table_handles.get(at) else { return Outcome::error(format!("unknown source-bound table handle {at:?}")); };
        let Some(current) = state.table_positions.get(node) else { return Outcome::error(format!("table handle {at:?} targets a deleted object")); };
        *at = current.clone();
    }
    let before = state.xml.revision_checkpoint();
    let outcome = run_typed_edit_inner(state, &resolved, author, mode, index, source, seed, ctx);
    // A direct edit leaves no revision: operations that only write the
    // tracked form (formatting records, paragraph deletion) are settled
    // here, so direct output is exactly the accepted tracked output.
    let mut outcome = outcome;
    if mode == crate::api::ChangeMode::Direct && outcome.status == crate::outcome::Status::Ok {
        revisions::accept_new_revisions(&mut state.xml, before, author);
        if let Some(Value::Object(result)) = outcome.result.as_mut() {
            if result.get("tracked") == Some(&Value::Bool(true)) {
                result.insert("tracked".into(), Value::Bool(false));
            }
        }
    }
    state.xml.renumber_allocated_revision_ids(before);
    if matches!(op, EditOp::EditTable { .. } | EditOp::InsertTable { .. } | EditOp::DeleteParagraphs { .. } | EditOp::ReplaceParagraph { .. }) {
        state.table_positions = table_positions(&state.xml);
        if let EditOp::InsertTable { alias: Some(alias), .. } = op {
            if let Some(at) = outcome.result.as_ref().and_then(|r|r.get("table")).and_then(|v|v.as_str()) {
                let prefix = format!("{at}/");
                for (node, current) in &state.table_positions {
                    if current == at { state.table_handles.insert(format!("${alias}"), *node); }
                    else if let Some(suffix) = current.strip_prefix(&prefix) { state.table_handles.insert(format!("${alias}/{suffix}"), *node); }
                }
            }
        }
        if let EditOp::EditTable { action, .. } = op {
            let alias = match action {
                crate::api::TableEdit::InsertRows { alias, .. } | crate::api::TableEdit::InsertColumns { alias, .. } => alias.as_ref(),
                _ => None,
            };
            if let (Some(alias), Some(result)) = (alias, outcome.result.as_ref()) {
                if let Some(paths) = result.get("alias_paths").and_then(Value::as_object) {
                    let current_nodes: HashMap<_,_> = state.table_positions.iter().map(|(node, at)|(at.as_str(), *node)).collect();
                    for (suffix, address) in paths {
                        if let Some(address) = address.as_str() {
                            if let Some(node) = current_nodes.get(address) { state.table_handles.insert(format!("${alias}/{suffix}"), *node); }
                        }
                    }
                }
            }
        }
    }
    outcome
}

fn run_typed_edit_inner(
    state: &mut WorkState,
    op: &crate::api::EditOp,
    author: &str,
    mode: crate::api::ChangeMode,
    index: usize,
    _source: &[u8; 32],
    plan_seed: &[u8; 32],
    ctx: &ExecCtx,
) -> Outcome {
    use crate::api::{
        ChromeKind, EditOp, InsertAnchor, LineSpacing, ParagraphAddress, ParagraphContent, RevisionAction,
        RevisionTarget,
    };
    let id = |at: &ParagraphAddress| match at {
        ParagraphAddress::Id(value) => Ok(value.clone()),
        ParagraphAddress::Alias(value) => {
            Err(Outcome::error(format!("unresolved alias ${}", value)))
        }
    };
    macro_rules! para_id {
        ($address:expr) => {
            match id($address) {
                Ok(value) => value,
                Err(error) => return error,
            }
        };
    }
    let tracked = mode != crate::api::ChangeMode::Direct;
    let plan_seed = *plan_seed;
    match op {
        EditOp::ReplaceEquation {
            at,
            mathml,
            equation,
            display,
        } => {
            let at = para_id!(at);
            mutate(state, |pkg, xml, source| {
                equations::replace(
                    pkg, xml, source, &at, mathml, *equation, *display, tracked, author, ctx,
                )
            })
        }
        EditOp::DeleteEquation { at, equation } => {
            let at = para_id!(at);
            mutate(state, |pkg, xml, source| {
                equations::delete(pkg, xml, source, &at, *equation, tracked, author, ctx)
            })
        }
        EditOp::ReplaceText {
            at,
            select,
            with,
            occurrence,
        } => {
            let args = typed::ReplaceTextArgs {
                at: para_id!(at),
                select: select.clone(),
                with: with.clone(),
                occurrence: occurrence.map(|v| v as usize),
                author: author.into(),
                tracked,
            };
            mutate(state, |pkg, xml, source| {
                typed::replace_text_typed(pkg, xml, &args, ctx, source)
            })
        }
        EditOp::ReviseText {
            at,
            within,
            occurrence,
            edits,
        } => {
            let args = typed::ReviseTextArgs {
                at: match at {
                    Some(address) => Some(para_id!(address)),
                    None => None,
                },
                within: within.clone(),
                occurrence: occurrence.map(|v| v as usize),
                edits: edits.clone(),
                author: author.into(),
                tracked,
            };
            mutate(state, |pkg, xml, source| {
                typed::revise_text_typed(pkg, xml, &args, ctx, source)
            })
        }
        EditOp::ReplaceParagraph { at, with } => {
            let args = typed::ReplaceTextArgs {
                at: para_id!(at),
                select: String::new(),
                with: with.clone(),
                occurrence: None,
                author: author.into(),
                tracked,
            };
            mutate(state, |pkg, xml, source| {
                typed::replace_paragraph_typed(pkg, xml, &args, ctx, source)
            })
        }
        EditOp::EditTable { at, action } => {
            mutate(state, |pkg, xml, _source| table_structure::edit(pkg, xml, at, action, tracked.then_some((author, ctx))))
        }
        EditOp::InsertTable { at, position, rows, columns, cells, .. } => {
            let at = para_id!(at);
            let cells = cells.as_deref().unwrap_or_default();
            mutate(state, |pkg, xml, source| table_structure::insert(pkg, xml, source, &at, *position, *rows, *columns, cells, tracked.then_some((author, ctx)), &plan_seed, index))
        }
        EditOp::FormatTable { at, format } => mutate(state, |pkg, xml, _| tables::format_table(Some(pkg), xml, at, format, tracked, author, ctx)),
        EditOp::FormatRow { at, format } => mutate(state, |_, xml, _| tables::format_row(xml, at, format, tracked, author, ctx)),
        EditOp::FormatCell { at, format } => mutate(state, |_, xml, _| tables::format_cell(xml, at, format, tracked, author, ctx)),
        EditOp::FormatText {
            at,
            select,
            occurrence,
            bold,
            italic,
            underline,
            strike,
            superscript,
            subscript,
            color,
            highlight,
            font_size,
            clear,
        } => {
            let args = typed::FormatTextArgs {
                at: para_id!(at),
                select: select.clone(),
                occurrence: occurrence.map(|v| v as usize),
                bold: *bold,
                italic: *italic,
                underline: *underline,
                strike: *strike,
                superscript: *superscript,
                subscript: *subscript,
                highlight: *highlight,
                color: color.clone(),
                font_size: font_size.map(|p| p.as_points()),
                clear: clear.clone(),
                author: author.into(),
            };
            mutate(state, |pkg, xml, source| {
                typed::format_text_typed(pkg, xml, &args, ctx, source)
            })
        }
        EditOp::FormatParagraph {
            at,
            style,
            alignment,
            indent_left,
            indent_right,
            first_line_indent,
            space_before,
            space_after,
            line_spacing,
            clear,
        } => {
            let (line, line_rule) = match line_spacing {
                Some(LineSpacing::Multiple(v)) => {
                    (Some((v * 240.0).round() as i64), Some("auto".into()))
                }
                Some(LineSpacing::Exact(v)) => (Some(v.0), Some("exact".into())),
                Some(LineSpacing::AtLeast(v)) => (Some(v.0), Some("atLeast".into())),
                None => (None, None),
            };
            let args = typed::FormatParagraphArgs {
                at: para_id!(at),
                style: style.clone(),
                alignment: alignment.clone(),
                indent_left: indent_left.map(|p| p.0),
                indent_right: indent_right.map(|p| p.0),
                first_line_indent: first_line_indent.map(|p| p.0),
                space_before: space_before.map(|p| p.0),
                space_after: space_after.map(|p| p.0),
                line,
                line_rule,
                clear: clear.clone(),
                author: author.into(),
            };
            mutate(state, |pkg, xml, source| {
                typed::format_paragraph_typed(pkg, xml, &args, ctx, source)
            })
        }
        EditOp::SetList { at, num_id, format, level, start, remove, levels } => {
            let mut addresses = Vec::new();
            for address in at { addresses.push(para_id!(address)); }
            let cache=state.list_state.get_or_insert_with(||lists::ListState::new(&state.xml));
            match lists::set_list_cached(&mut state.package,&mut state.xml,state.source_hash,&addresses,*num_id,format.as_deref(),*level,*start,*remove,levels.as_deref(),author,ctx,cache) {
                Ok((summary,result))=>{state.mutated=true;Outcome::ok(summary,result)},
                Err(outcome)=>outcome,
            }
        }
        EditOp::InsertParagraph {
            at,
            position,
            with,
            style,
            ..
        } => {
            let (at_para, at_table) = match at {
                InsertAnchor::Paragraph(addr) => (Some(para_id!(addr)), None),
                InsertAnchor::Table(n) => (None, Some(*n)),
            };
            let args = |with: &str, slot: usize| typed::InsertParagraphArgs {
                at_para: at_para.clone(),
                at_table,
                insert_before: matches!(position, crate::api::InsertPosition::Before),
                with: with.into(),
                style: style.clone(),
                tracked,
                author: author.into(),
                plan_seed,
                op_index: index,
                slot: slot as u64,
            };
            match with {
                ParagraphContent::One(with) => mutate(state, |pkg, xml, source| {
                    typed::insert_paragraph_typed(pkg, xml, &args(with, 0), ctx, source)
                }),
                // Each item goes in at the same anchor; inserts before it
                // and `insert_after_in_order` both keep list order.
                ParagraphContent::Many(items) => mutate(state, |pkg, xml, source| {
                    let mut ids = Vec::with_capacity(items.len());
                    let mut last = Value::Null;
                    for (slot, with) in items.iter().enumerate() {
                        let (_, result) = typed::insert_paragraph_typed(pkg, xml, &args(with, slot), ctx, source)?;
                        ids.push(result["paraId"].clone());
                        last = result;
                    }
                    let summary = format!(
                        "inserted {} paragraphs {} {} ({})",
                        ids.len(),
                        if last["before"] == true { "before" } else { "after" },
                        last["anchor"].as_str().unwrap_or_default(),
                        if tracked { "tracked" } else { "direct" }
                    );
                    Ok((summary, serde_json::json!({
                        "paraIds": ids,
                        "anchor": last["anchor"],
                        "before": last["before"],
                        "tracked": tracked,
                    })))
                }),
            }
        }
        EditOp::DeleteParagraphs { at } => {
            let targets = match at.iter().map(id).collect::<Result<Vec<_>, _>>() {
                Ok(values) => values,
                Err(error) => return error,
            };
            let args = typed::DeleteParagraphsArgs {
                at: targets,
                author: author.into(),
                tracked,
            };
            mutate(state, |pkg, xml, source| {
                typed::delete_paragraphs_typed(pkg, xml, &args, ctx, source)
            })
        }
        EditOp::SetPageMargins {
            section,
            top,
            right,
            bottom,
            left,
            header,
            footer,
            gutter,
        } => mutate(state, |pkg, xml, _| {
            layout::set_page_margins_typed(
                pkg,
                xml,
                section.unwrap_or(1) as usize,
                top.map(|p| p.0),
                right.map(|p| p.0),
                bottom.map(|p| p.0),
                left.map(|p| p.0),
                header.map(|p| p.0),
                footer.map(|p| p.0),
                gutter.map(|p| p.0),
                tracked,
                author,
                ctx,
            )
        }),
        EditOp::SetSectionLayout { section, section_handle, .. } => mutate(state, |pkg, xml, source| {
            if let Some(id)=section_handle.as_deref().and_then(|v|v.strip_prefix("section:")).filter(|v|*v!="final") { typed::resolve_paragraph_by_para_id(xml,pkg,source,id)?; }
            sections::ensure_final(xml)?;
            let selected=sections::select(xml,*section,section_handle.as_deref())?;
            layout::set_section_layout(xml, selected, op, tracked, author, ctx)
        }),
        EditOp::SetSectionBoundary {at,remove,break_type} => { let at=para_id!(at); mutate(state,|pkg,xml,source|sections::boundary(pkg,xml,source,&at,*remove,break_type.as_deref(),tracked)) },
        EditOp::SetFirstPageMode { section, enabled } => mutate(state, |_pkg, xml, _| {
            layout::set_first_page_mode(xml, section.unwrap_or(1) as usize, *enabled, tracked, author, ctx)
        }),
        // Word cannot track a document setting: refuse rather than apply it
        // unmarked.
        EditOp::SetEvenAndOddHeaders { .. } if tracked => Outcome::error(
            "set_even_and_odd_headers cannot be tracked: it changes a document setting (w:evenAndOddHeaders in settings.xml), which Word records no revision for; run it in a change_mode=direct plan",
        ),
        EditOp::SetEvenAndOddHeaders { even_and_odd } => mutate(state, |pkg, xml, _| {
            headers::set_even_and_odd_headers_typed(pkg, xml, *even_and_odd)
        }),
        EditOp::SetHeader {
            section,
            kind,
            with,
        } => mutate(state, |pkg, xml, _| {
            headers::set_header_typed(
                pkg,
                xml,
                section.unwrap_or(1) as usize,
                ChromeKind::resolve(*kind),
                with,
                tracked.then_some((author, ctx)),
            )
        }),
        EditOp::SetFooter {
            section,
            kind,
            with,
        } => mutate(state, |pkg, xml, _| {
            headers::set_footer_typed(
                pkg,
                xml,
                section.unwrap_or(1) as usize,
                ChromeKind::resolve(*kind),
                with,
                tracked.then_some((author, ctx)),
            )
        }),
        EditOp::ClearHeader { section, kind } => mutate(state, |pkg, xml, _| {
            headers::clear_header_typed(
                pkg,
                xml,
                section.unwrap_or(1) as usize,
                ChromeKind::resolve(*kind),
                tracked.then_some((author, ctx)),
            )
        }),
        EditOp::ClearFooter { section, kind } => mutate(state, |pkg, xml, _| {
            headers::clear_footer_typed(
                pkg,
                xml,
                section.unwrap_or(1) as usize,
                ChromeKind::resolve(*kind),
                tracked.then_some((author, ctx)),
            )
        }),
        EditOp::CommentAdd {
            at,
            select,
            occurrence,
            text,
        } => {
            let args = typed::CommentAddArgs {
                at: para_id!(at),
                select: select.clone(),
                occurrence: occurrence.map(|v| v as usize),
                text: text.clone(),
                author: author.into(),
            };
            mutate(state, |pkg, xml, source| {
                typed::comment_add_typed(pkg, xml, &args, ctx, source)
            })
        }
        EditOp::CommentReply { comment_id, text } => mutate(state, |pkg, xml, _| {
            comments::reply_comment_typed(pkg, xml, comment_id, text, author, ctx)
        }),
        EditOp::CommentSetStatus { comment_id, status } => mutate(state, |pkg, xml, _| {
            comments::resolve_comment_typed(
                pkg,
                xml,
                comment_id,
                matches!(status, crate::api::CommentStatus::Resolved),
            )
        }),
        EditOp::CommentDelete { comment_id } => mutate(state, |pkg, xml, _| {
            comments::delete_comment_typed(pkg, xml, comment_id)
        }),
        // Word does not track bookmarks: both change modes apply directly.
        EditOp::BookmarkAdd { at, name, select, occurrence } => {
            let at = para_id!(at);
            mutate(state, |pkg, xml, source| {
                let anchor = bookmarks::Anchor { at: &at, select: select.as_deref(), occurrence: occurrence.map(|v| v as usize) };
                bookmarks::add(pkg, xml, source, name, &anchor)
            })
        }
        EditOp::BookmarkRename { name, new_name } => {
            mutate(state, |pkg, xml, _| bookmarks::rename(pkg, xml, name, new_name))
        }
        EditOp::BookmarkMove { name, at, select, occurrence } => {
            let at = para_id!(at);
            mutate(state, |pkg, xml, source| {
                let anchor = bookmarks::Anchor { at: &at, select: select.as_deref(), occurrence: occurrence.map(|v| v as usize) };
                bookmarks::move_to(pkg, xml, source, name, &anchor)
            })
        }
        EditOp::BookmarkDelete { name } => mutate(state, |pkg, xml, _| bookmarks::delete(pkg, xml, name)),
        EditOp::ConvertRevisionsToFormatting { target, at } => {
            let mut paragraphs = None;
            if let Some(at) = at {
                let mut ids = Vec::new();
                for address in at {
                    ids.push(para_id!(address));
                }
                paragraphs = Some(ids);
            }
            mutate(state, |pkg, xml, source| revisions::convert_to_formatting(pkg, xml, source, target, paragraphs.as_deref()))
        }
        // The list cache writes numbering.xml at the end of the plan; flush
        // and drop it so settling sees, and may prune, the current part.
        EditOp::RevisionSettle { target, action } => match { state.flush_lists(); state.list_state = None; target } {
            RevisionTarget::All => mutate(state, |pkg, xml, _| {
                revisions::accept_reject_all(pkg, xml, matches!(action, RevisionAction::Accept))
            }),
            RevisionTarget::Id(revision_id) => mutate(state, |pkg, xml, _| {
                revisions::accept_reject_one_typed(
                    pkg,
                    xml,
                    revision_id,
                    matches!(action, RevisionAction::Accept),
                )
            }),
        },
    }
}

/// Mutation harness. A failed operation stops the plan and its working state
/// is discarded (`fold_typed` returns no bytes), so there is no per-operation
/// rollback snapshot: serializing the whole part before every operation made
/// plans quadratic in document size.
fn mutate(
    state: &mut WorkState,
    run: impl FnOnce(&mut Package, &mut DocxXml, Option<[u8; 32]>) -> Result<(String, Value), Outcome>,
) -> Outcome {
    let source_hash = state.source_hash;
    match run(&mut state.package, &mut state.xml, source_hash) {
        Ok((summary, result)) => {
            state.mutated = true;
            Outcome::ok(summary, result)
        }
        Err(outcome) => outcome,
    }
}

/// The selectable text of paragraph `para_id` in the unmodified input
/// package, for explaining selectors that stopped matching mid-plan.
pub(crate) fn base_visible_text(input: &[u8], para_id: &str) -> Option<String> {
    let mut state = WorkState::parse(input).ok()?;
    let paragraph = typed::resolve_paragraph_by_para_id(
        &mut state.xml,
        &state.package,
        state.source_hash,
        para_id,
    )
    .ok()?;
    Some(typed::visible_text(&state.xml, paragraph))
}

/// The paragraph ids a per-op report affects: the single `paraId` address or
/// the `delete_paragraphs` paragraphs array.
pub(crate) fn affected_para_ids(report: &Value) -> Vec<String> {
    let result = report.get("result");
    if let Some(ids) = result
        .and_then(|r| r.get("paragraphs"))
        .and_then(Value::as_array)
    {
        return ids
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
    }
    result
        .and_then(|r| r.get("paraId"))
        .and_then(Value::as_str)
        .map(|id| vec![id.to_owned()])
        .unwrap_or_default()
}

/// `value` without object fields that are null, `{}` or `[]`; agent reads
/// treat an absent field as empty. OOXML `properties` bags are kept as they
/// are inside, where an empty element is still a source fact.
pub(crate) fn prune_empty(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| {
                    let v = if k == "properties" { v } else { prune_empty(v) };
                    (k, v)
                })
                .filter(|(_, v)| match v {
                    Value::Null => false,
                    Value::Object(m) => !m.is_empty(),
                    Value::Array(a) => !a.is_empty(),
                    _ => true,
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(prune_empty).collect()),
        other => other,
    }
}

/// The markup block carrying each requested `w14:paraId` in the current
/// in-memory state — the "current affected markup" diagnostic. One render
/// serves every id, so a plan pays for a single document render however many
/// operations it reports on.
pub(crate) fn affected_markup(
    state: &WorkState,
    para_ids: &std::collections::BTreeSet<String>,
) -> HashMap<String, String> {
    use crate::html::render::RenderView;
    use crate::html::surface::render_document;
    let mut found = HashMap::new();
    if para_ids.is_empty() {
        return found;
    }
    let doc = render_document(
        &state.package,
        &state.xml,
        RenderView::Markup,
        state.source_hash.as_ref(),
    );
    let body = &doc.body;
    let mut block_of: HashMap<&str, usize> = HashMap::new();
    for (index, block) in body.blocks.iter().enumerate() {
        for &node in &block.paragraphs {
            if let Some(id) = state.xml.para_id(node) {
                block_of.entry(id).or_insert(index);
            }
        }
    }
    let styles = crate::html::styles::Styles::load(&state.package);
    let catalog = styles.catalog();
    let default_style = styles.default_paragraph_style();
    let mut rendered: HashMap<usize, String> = HashMap::new();
    for para_id in para_ids {
        let index = block_of.get(para_id.as_str()).copied().or_else(|| {
            // Nested paragraphs a block does not list (textboxes, alternate
            // content) are still located by their rendered id attribute.
            let needle = format!("id=\"{para_id}\"");
            body.blocks
                .iter()
                .position(|block| body.html[block.start..block.end].contains(&needle))
        });
        if let Some(index) = index {
            let markup = rendered.entry(index).or_insert_with(|| {
                let block = &body.blocks[index];
                crate::html::compact::agent_html(
                    &body.html[block.start..block.end],
                    &catalog,
                    default_style.as_deref(),
                )
            });
            found.insert(para_id.clone(), markup.clone());
        }
    }
    found
}
