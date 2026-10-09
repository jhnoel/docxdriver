//! Internal typed operation runtime: typed commands only.
pub(crate) mod comments;
pub(crate) mod equations;
pub(crate) mod headers;
pub(crate) mod layout;
pub(crate) mod reading;
pub(crate) mod readonly;
pub(crate) mod revisions;
pub(crate) mod typed;

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

/// The parsed working set the operation fold runs over: one unzip + one
/// document.xml parse per typed plan, however many operations follow.
pub(crate) struct WorkState {
    pub(crate) package: Package,
    pub(crate) xml: DocxXml,
    pub(crate) mutated: bool,
    /// sha256 of the exact input package bytes — the inspection `source`, the
    /// stale-plan binding anchor, and the seed for deterministic provisional
    /// paragraph-ID repair. None when the working set was created from nothing
    /// (a create-led operation list).
    pub(crate) source_hash: Option<[u8; 32]>,
}

impl WorkState {
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
        Ok(WorkState {
            package,
            xml,
            mutated: false,
            source_hash: None,
        })
    }
}

/// sha256 of the exact input package bytes.
pub(crate) fn package_sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}

/// Attach the serialized package to a successful outcome when something
/// actually mutated (and dryRun doesn't withhold it). The one zip write.
///
/// Every engine write persists paragraph identity first: existing valid
/// `w14:paraId` values are untouched, while missing/invalid/duplicate and
/// engine-created paragraphs receive the same deterministic values inspection
/// showed — one atomic write of repairs plus content operations.
pub(crate) fn emit(mut state: WorkState, outcome: Outcome, ctx: &ExecCtx) -> Outcome {
    if outcome.status != crate::outcome::Status::Ok || !state.mutated || ctx.dry_run {
        return outcome;
    }
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
pub(crate) fn run_typed_edit(
    state: &mut WorkState,
    op: &crate::api::EditOp,
    author: &str,
    mode: crate::api::ChangeMode,
    index: usize,
    _source: &[u8; 32],
    plan: &crate::api::Plan,
    ctx: &ExecCtx,
) -> Outcome {
    use crate::api::{
        ChromeKind, EditOp, InsertAnchor, LineSpacing, ParagraphAddress, RevisionAction,
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
    let tracked = matches!(mode, crate::api::ChangeMode::Track);
    let plan_seed: [u8; 32] = sha2::Sha256::digest(
        crate::api::canonical_json(plan)
            .unwrap_or_default()
            .as_bytes(),
    )
    .into();
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
            let args = typed::InsertParagraphArgs {
                at_para,
                at_table,
                insert_before: matches!(position, crate::api::InsertPosition::Before),
                with: with.clone(),
                style: style.clone(),
                tracked,
                author: author.into(),
                plan_seed,
                op_index: index,
            };
            mutate(state, |pkg, xml, source| {
                typed::insert_paragraph_typed(pkg, xml, &args, ctx, source)
            })
        }
        EditOp::DeleteParagraphs { at } => {
            let targets = match at.iter().map(id).collect::<Result<Vec<_>, _>>() {
                Ok(values) => values,
                Err(error) => return error,
            };
            let args = typed::DeleteParagraphsArgs {
                at: targets,
                author: author.into(),
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
            )
        }),
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
            )
        }),
        EditOp::ClearHeader { section, kind } => mutate(state, |pkg, xml, _| {
            headers::clear_header_typed(
                pkg,
                xml,
                section.unwrap_or(1) as usize,
                ChromeKind::resolve(*kind),
            )
        }),
        EditOp::ClearFooter { section, kind } => mutate(state, |pkg, xml, _| {
            headers::clear_footer_typed(
                pkg,
                xml,
                section.unwrap_or(1) as usize,
                ChromeKind::resolve(*kind),
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
        EditOp::RevisionSettle { target, action } => match target {
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

/// Mutation harness: snapshot, run, roll back on failure. Operations may fail
/// after partially editing (e.g. replaceText erroring on a later paragraph), so
/// the snapshot is what makes skip-on-failure safe. Both halves stay in memory —
/// document.xml re-serializes and the parts vector memcpys; no zip round-trip.
fn mutate(
    state: &mut WorkState,
    run: impl FnOnce(&mut Package, &mut DocxXml, Option<[u8; 32]>) -> Result<(String, Value), Outcome>,
) -> Outcome {
    let doc_snapshot = state.xml.serialize();
    let package_snapshot = state.package.clone();
    let source_hash = state.source_hash;
    match run(&mut state.package, &mut state.xml, source_hash) {
        Ok((summary, result)) => {
            state.mutated = true;
            Outcome::ok(summary, result)
        }
        Err(outcome) => {
            state.package = package_snapshot;
            match DocxXml::parse(&doc_snapshot) {
                Ok(xml) => state.xml = xml,
                // Unreachable: doc_snapshot is our own serializer's output.
                Err(error) => return Outcome::error(format!("rollback failed: {error}")),
            }
            outcome
        }
    }
}

/// Attach the current markup of the affected paragraph(s) to a per-op report:
/// the single `paraId` address or the `delete_paragraphs` paragraphs array.
pub(crate) fn attach_affected_markup(state: &WorkState, mut report: Value) -> Value {
    if let Some(ids) = report
        .get("result")
        .and_then(|r| r.get("paragraphs"))
        .and_then(Value::as_array)
    {
        let markups: Vec<Value> = ids
            .iter()
            .filter_map(|v| {
                v.as_str()
                    .and_then(|id| affected_markup(state, id))
                    .map(Value::String)
            })
            .collect();
        if !markups.is_empty() {
            if let Some(object) = report.as_object_mut() {
                object.insert("markup".into(), Value::Array(markups));
            }
        }
    } else if let Some(para_id) = report
        .get("result")
        .and_then(|r| r.get("paraId"))
        .and_then(Value::as_str)
    {
        if let Some(markup) = affected_markup(state, para_id) {
            if let Some(object) = report.as_object_mut() {
                object.insert("markup".into(), Value::String(markup));
            }
        }
    }
    report
}

/// The markup block carrying this paragraph's `w14:paraId` in the current
/// in-memory state — the "current affected markup" diagnostic.
fn affected_markup(state: &WorkState, para_id: &str) -> Option<String> {
    use crate::html::render::RenderView;
    use crate::html::surface::render_document;
    let doc = render_document(
        &state.package,
        &state.xml,
        RenderView::Markup,
        state.source_hash.as_ref(),
    );
    let needle = format!("id=\"{}\"", para_id);
    let body = &doc.body;
    body.blocks
        .iter()
        .find(|block| body.html[block.start..block.end].contains(&needle))
        .map(|block| crate::html::compact::model_html(&body.html[block.start..block.end]))
}
