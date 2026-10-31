//! Typed plan operations — Phase 2 selection and content ops.
//!
//! `replace_text` and `replace_paragraph` address paragraphs by their native
//! `w14:paraId` (the only typed content address) and select by strict
//! plain-text matching (§4). `with` accepts only plain text plus the safe
//! HTML (`<strong> <em> <u> <s> <sup> <sub> <a href> <br> <span> <math>`).
//! The engine authors revisions (tracked mode) or performs a gated direct
//! edit (untracked mode, opt-in at the plan layer). Everything else — fields,
//! drawings, notes, comment milestones, and ambiguous revision overlap —
//! blocks with a diagnostic instead of guessing.

use serde_json::{json, Value};
use std::collections::BTreeSet;
use xmloxide::tree::NodeId;

use crate::document::{format_para_id, is_valid_para_id, DocxXml, ParaIdAllocator};
use crate::html::{Atom, Fmt, Item};
use crate::outcome::{Outcome, Status};
use crate::package::Package;
use crate::segments::{
    build_segments, delete_tracked_range, insert_ins_pieces_at, is_own_insertion, merge_own_ins,
    split_boundary, split_own_ins, InsPiece, RevWrap, Segment,
};

use super::layout::{
    ensure_ppr, ensure_ppr_child, ensure_rpr_child, paragraph_style_exists, set_attr, set_child_val,
};
use super::ExecCtx;

/// Internal engine policy; intentionally absent from the public plan schema.
const WORD_LEVEL_REDLINES: bool = true;

struct SelectionProjection {
    current: String,
    to_all: Vec<usize>,
}

fn selection_projection(xml: &DocxXml, paragraph: NodeId) -> SelectionProjection {
    let segments = build_segments(xml, paragraph);
    let mut current = String::new();
    let mut to_all = Vec::new();
    for segment in &segments {
        if segment.wrap == RevWrap::Del {
            continue;
        }
        for (index, ch) in segment.text.chars().enumerate() {
            current.push(ch);
            to_all.push(segment.start + index);
        }
    }
    SelectionProjection { current, to_all }
}

/// A paragraph's selectable text: its visible text without tracked deletions.
pub(super) fn visible_text(xml: &DocxXml, paragraph: NodeId) -> String {
    selection_projection(xml, paragraph).current
}

pub(super) fn select_in_paragraph(
    xml: &DocxXml,
    paragraph: NodeId,
    select: &str,
    occurrence: Option<usize>,
) -> Result<(usize, usize, String, usize), Outcome> {
    let projection = selection_projection(xml, paragraph);
    let spans: Vec<(usize, usize)> = projection
        .current
        .match_indices(select)
        .map(|(byte, matched)| {
            let start = projection.current[..byte].chars().count();
            (start, start + matched.chars().count())
        })
        .collect();
    let (current_start, current_end) = match (spans.len(), occurrence) {
        (0, _) => {
            return Err(Outcome::blocked(format!(
                "select matched nothing (current visible text: {:?}) — hidden deleted text is not selectable; accept or reject the deletion before editing that content",
                projection.current
            )))
        }
        (count, Some(n)) if n >= 1 && n <= count => spans[n - 1],
        (count, Some(n)) => {
            return Err(Outcome::blocked(format!(
                "occurrence {n} is out of range: select matched {count} times in this paragraph"
            )))
        }
        (1, None) => spans[0],
        (count, None) => {
            return Err(Outcome::blocked(format!(
                "select matched {count} times in this paragraph — pass occurrence (1-based) or extend select with more context (current visible text: {:?})",
                projection.current
            )))
        }
    };
    let all_start = projection.to_all[current_start];
    let all_end = projection.to_all[current_end - 1] + 1;
    Ok((all_start, all_end, projection.current, spans.len()))
}

/// Blocking structure labels by element local name. Any of these inside a
/// selected span (or a whole paragraph for `replace_paragraph`) makes safe
/// preservation ambiguous.
const BLOCKERS: &[(&str, &str)] = &[
    ("commentRangeStart", "comment"),
    ("commentRangeEnd", "comment"),
    ("commentReference", "comment"),
    ("fldSimple", "field"),
    ("fldChar", "field"),
    ("drawing", "drawing"),
    ("object", "drawing"),
    ("pict", "drawing"),
    ("footnoteReference", "note"),
    ("endnoteReference", "note"),
    ("sdt", "structured document tag"),
];

fn blocker_label(xml: &DocxXml, node: NodeId) -> Option<&'static str> {
    if !xml.doc.is_element(node) {
        return None;
    }
    BLOCKERS
        .iter()
        .find(|(local, _)| xml.is_local(node, local))
        .map(|(_, label)| *label)
}

/// A blocked diagnostic carrying the target address so the plan layer can
/// report the affected paragraph's current markup.
pub(super) fn blocked_at(at: &str, message: impl Into<String>) -> Outcome {
    let address = at.to_uppercase();
    Outcome {
        status: Status::Blocked,
        summary: format!("blocked in paragraph {address}: {}", message.into()),
        result: Some(json!({ "paraId": address })),
        bytes: None,
    }
}

/// Resolve `at` (eight-digit `w14:paraId`) to its paragraph in
/// `word/document.xml`. Provisional repairs from inspection are applied in
/// memory first, so the deterministic addresses the agent saw resolve here
/// exactly as they will at commit.
pub(super) fn resolve_paragraph_by_para_id(
    xml: &mut DocxXml,
    package: &Package,
    source_hash: Option<[u8; 32]>,
    at: &str,
) -> Result<NodeId, Outcome> {
    let needle = at.to_uppercase();
    // Fast path: when every paragraph already carries a valid, unique id
    // (always true after the plan's up-front repair unless an operation
    // created an unaddressed paragraph), repair would be a no-op and one walk
    // finds the target.
    if let Some(found) = find_with_clean_ids(xml, &needle) {
        return found.ok_or_else(|| missing_paragraph(&needle));
    }
    let part = package.document_part_name();
    let seed = match source_hash {
        Some(hash) => hash,
        None => {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            if let Some(bytes) = package.get(&part) {
                hasher.update(bytes);
            }
            hasher.finalize().into()
        }
    };
    let index = xml.resolve_paragraph_ids(&part, &seed);
    xml.apply_paragraph_ids(&index).map_err(Outcome::error)?;
    xml.all_paragraphs()
        .into_iter()
        .find(|&node| {
            xml.para_id(node)
                .is_some_and(|value| value.to_uppercase() == needle)
        })
        .ok_or_else(|| missing_paragraph(&needle))
}

/// One walk over the part: `None` when some paragraph would need id repair,
/// otherwise the first paragraph whose id matches `needle` (if any).
fn find_with_clean_ids(xml: &DocxXml, needle: &str) -> Option<Option<NodeId>> {
    let root = xml.doc.root_element()?;
    let mut seen: Vec<u32> = Vec::new();
    let mut target = None;
    for node in xml.doc.descendants(root) {
        if !xml.is_w(node, "p") {
            continue;
        }
        let value = xml
            .para_id(node)
            .filter(|value| crate::document::is_valid_para_id(value))?;
        seen.push(u32::from_str_radix(value, 16).ok()?);
        if target.is_none() && value.eq_ignore_ascii_case(needle) {
            target = Some(node);
        }
    }
    seen.sort_unstable();
    if seen.windows(2).any(|pair| pair[0] == pair[1]) {
        return None;
    }
    Some(target)
}

fn missing_paragraph(needle: &str) -> Outcome {
    blocked_at(
        needle,
        "no paragraph with this paraId in word/document.xml — paragraph IDs in headers, footers, footnotes, endnotes, and comment bodies are outside the typed mutation surface",
    )
}

/// Track hyperlink context across `with` atoms: the whole content must either
/// be inside one hyperlink or contain no hyperlinks.
fn note_link(state: &mut Option<Option<String>>, link: Option<String>) -> Result<(), Outcome> {
    let Some(href) = link else {
        if state.as_ref().is_some_and(Option::is_some) {
            return Err(Outcome::blocked(
                "with mixes linked and unlinked content — keep a hyperlink's content whole, or split into separate operations",
            ));
        }
        *state = Some(None);
        return Ok(());
    };
    match state {
        Some(Some(existing)) if *existing != href => {
            return Err(Outcome::blocked(
                "with cannot mix different hyperlink targets in one operation — split into separate operations",
            ));
        }
        Some(None) => {
            return Err(Outcome::blocked(
                "with mixes linked and unlinked content — keep a hyperlink's content whole, or split into separate operations",
            ));
        }
        _ => {}
    }
    *state = Some(Some(href));
    Ok(())
}

/// The safe inline `with` dialect gate: parse `with` and reduce it to
/// insertion pieces. Rejects revision tags, pending-format tags, comment
/// milestones, paragraph/table/chrome tags, notes, images, and fields with
/// targeted diagnostics.
fn parse_with(with: &str) -> Result<(Vec<InsPiece>, Option<String>), Outcome> {
    let wrapped = format!("<p>{with}</p>");
    let items = crate::html::parse::parse_fragment(&wrapped).map_err(|error| {
        Outcome::blocked(format!(
            "with does not parse as safe inline content: {error} — with accepts plain text and safe inline HTML <strong> <em> <u> <s> <sup> <sub> <a href=\"…\"> <br> <span style> <math>"
        ))
    })?;

    if items
        .iter()
        .filter(|i| matches!(i, Item::ParaOpen(_)))
        .count()
        != 1
    {
        return Err(Outcome::blocked(
            "with accepts inline HTML only; use insert_paragraph for blocks",
        ));
    }
    let mut pieces: Vec<InsPiece> = Vec::new();
    let mut link_state: Option<Option<String>> = None;

    for item in items {
        match item {
            Item::ParaOpen(_) | Item::ParaClose => {}
            Item::Struct(tag) => {
                return Err(Outcome::blocked(format!(
                    "with contains table structure <{tag}> — only inline content and plain text are allowed"
                )));
            }
            Item::Atom(atom) => {
                if atom
                    .wrap()
                    .is_some_and(|(ins, del)| ins.is_some() || del.is_some())
                {
                    return Err(Outcome::blocked(
                        "with contains revision tags (<ins>/<del>) — the engine authors revisions; with holds only the new content",
                    ));
                }
                if atom.field().is_some() {
                    return Err(Outcome::blocked(
                        "with contains a field — fields are protected through this surface",
                    ));
                }
                match atom {
                    Atom::Char {
                        ch,
                        fmt,
                        fmt_pending,
                        link,
                        ..
                    } => {
                        if fmt_pending.is_some() {
                            return Err(Outcome::blocked(
                                "with contains a pending-format tag (<b pending>/<format pending>) — format changes are separate operations",
                            ));
                        }
                        note_link(&mut link_state, link)?;
                        match pieces.last_mut() {
                            Some(InsPiece::Text(text, piece_fmt)) if *piece_fmt == fmt => {
                                text.push(ch)
                            }
                            _ => pieces.push(InsPiece::Text(ch.to_string(), fmt)),
                        }
                    }
                    Atom::Br { kind, link, .. } => {
                        note_link(&mut link_state, link)?;
                        pieces.push(InsPiece::Br(kind));
                    }
                    Atom::Tab { link, .. } => {
                        note_link(&mut link_state, link)?;
                        pieces.push(InsPiece::Tab);
                    }
                    Atom::Equation { text, display, .. } => {
                        note_link(&mut link_state, None)?;
                        pieces.push(InsPiece::Equation {
                            mathml: text,
                            display,
                        });
                    }
                    Atom::Note { .. } => {
                        return Err(Outcome::blocked(
                            "with contains a footnote/endnote — only the safe inline set is allowed",
                        ));
                    }
                    Atom::Image { .. } => {
                        return Err(Outcome::blocked(
                            "with contains an image — images are not in the safe inline set",
                        ));
                    }
                    Atom::Milestone { .. } => {
                        return Err(Outcome::blocked(
                            "with contains a comment milestone — comments are separate operations",
                        ));
                    }
                }
            }
        }
    }
    let link = match link_state {
        Some(Some(href)) => Some(href),
        _ => None,
    };
    Ok((pieces, link))
}

/// Structural scan of the paragraph children (and their descendants) between
/// two anchors, inclusive. Returns the first blocking structure label.
fn scan_blockers_between(xml: &DocxXml, first: NodeId, last: NodeId) -> Option<&'static str> {
    let mut cursor = Some(first);
    while let Some(node) = cursor {
        for descendant in std::iter::once(node).chain(xml.doc.descendants(node)) {
            if let Some(label) = blocker_label(xml, descendant) {
                return Some(label);
            }
        }
        if node == last {
            break;
        }
        cursor = xml.doc.next_sibling(node);
    }
    None
}

/// Whole-paragraph structural scan for `replace_paragraph`: any blocking
/// structure or run-level tracked change makes content replacement ambiguous.
fn paragraph_blocker(xml: &DocxXml, paragraph: NodeId) -> Option<String> {
    for node in xml.doc.descendants(paragraph) {
        if !xml.doc.is_element(node) {
            continue;
        }
        if let Some(label) = blocker_label(xml, node) {
            return Some(label.to_string());
        }
        // Paragraph-mark revisions (w:pPr/w:rPr/w:ins|del) concern the break,
        // not the content — replacing content preserves them.
        if (xml.is_rev_ins(node) || xml.is_rev_del(node)) && !inside_ppr(xml, paragraph, node) {
            return Some("tracked change".to_string());
        }
    }
    None
}

/// Paragraph content that holds runs: a run, or a hyperlink, smart tag or
/// bidi wrapper (`w:dir`, `w:bdo`) around runs.
fn is_run_content(xml: &DocxXml, node: NodeId) -> bool {
    ["r", "hyperlink", "smartTag", "dir", "bdo"]
        .iter()
        .any(|name| xml.is_w(node, name))
}

fn inside_ppr(xml: &DocxXml, paragraph: NodeId, mut node: NodeId) -> bool {
    while let Some(parent) = xml.doc.parent(node) {
        if parent == paragraph {
            return false;
        }
        if xml.is_w(parent, "pPr") {
            return true;
        }
        node = parent;
    }
    false
}

/// Common input validation for both typed ops.
struct OpInput {
    at: String,
    with: String,
    author: String,
    tracked: bool,
}

#[derive(Debug, Clone)]
pub struct ReplaceTextArgs {
    pub at: String,
    pub select: String,
    pub with: String,
    pub occurrence: Option<usize>,
    pub author: String,
    pub tracked: bool,
}

/// Typed `replace_text` implementation. Legacy JSON callers are adapted by
/// the wrapper below; Request execution calls this function directly.
pub fn replace_text_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    args: &ReplaceTextArgs,
    ctx: &ExecCtx,
    source_hash: Option<[u8; 32]>,
) -> Result<(String, Value), Outcome> {
    let input = OpInput {
        at: args.at.clone(),
        with: args.with.clone(),
        author: args.author.clone(),
        tracked: args.tracked,
    };
    let select = args.select.clone();
    if select.contains('<') {
        return Err(blocked_at(
            &input.at,
            "select must be plain text without HTML markup — select addresses the paragraph's visible text, never markup",
        ));
    }
    let occurrence = args.occurrence;

    let (pieces, link_href) = parse_with(&input.with)?;
    let paragraph = resolve_paragraph_by_para_id(xml, package, source_hash, &input.at)?;
    let address = input.at.to_uppercase();

    let (start, end, _current, matched) = select_in_paragraph(xml, paragraph, &select, occurrence)
        .map_err(|outcome| blocked_at(&address, outcome.summary))?;

    replace_range(
        package,
        xml,
        paragraph,
        &address,
        (start, end),
        (&pieces, link_href),
        &input.author,
        input.tracked,
        WORD_LEVEL_REDLINES,
        ctx,
    )?;

    let summary = match occurrence {
        Some(n) => format!("replaced occurrence {n} of {matched} in paragraph {address}"),
        None => format!("replaced 1 of 1 match in paragraph {address}"),
    };
    let result = json!({
        "paraId": address,
        "matched": matched,
        "occurrence": occurrence,
        "tracked": input.tracked,
    });
    Ok((summary, result))
}

/// Split words, whitespace runs, and individual punctuation characters. Offsets
/// are Unicode scalar offsets, matching the document segment engine.
fn diff_tokens(text: &str) -> (Vec<String>, Vec<usize>) {
    let mut tokens = Vec::<String>::new();
    let mut offsets = vec![0];
    let mut previous = None;
    for (index, ch) in text.chars().enumerate() {
        let kind = if ch.is_alphanumeric() || ch == '_' {
            0
        } else if ch.is_whitespace() {
            1
        } else {
            2
        };
        if previous == Some(kind) && kind != 2 {
            tokens.last_mut().unwrap().push(ch);
            *offsets.last_mut().unwrap() = index + 1;
        } else {
            tokens.push(ch.to_string());
            offsets.push(index + 1);
        }
        previous = Some(kind);
    }
    (tokens, offsets)
}

/// Unmatched old/new character spans around a deterministic token LCS.
fn word_diff(old: &str, new: &str) -> Result<Vec<(usize, usize, usize, usize)>, Outcome> {
    if old == new {
        return Ok(Vec::new());
    }
    let (a, ao) = diff_tokens(old);
    let (b, bo) = diff_tokens(new);
    let width = b.len() + 1;
    let cells = (a.len() + 1)
        .checked_mul(width)
        .filter(|&n| n <= 4_000_000)
        .ok_or_else(|| {
            Outcome::blocked("word-level diff selection is too large; split it into smaller replacements")
        })?;
    let mut lcs = vec![0u32; cells];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i * width + j] = if a[i] == b[j] {
                1 + lcs[(i + 1) * width + j + 1]
            } else {
                lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j, mut ai, mut bj) = (0, 0, 0, 0);
    let mut spans = Vec::new();
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            if ai != i || bj != j {
                spans.push((ao[ai], ao[i], bo[bj], bo[j]));
            }
            i += 1;
            j += 1;
            ai = i;
            bj = j;
        } else if lcs[(i + 1) * width + j] >= lcs[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    if ai != a.len() || bj != b.len() {
        spans.push((ao[ai], ao[a.len()], bo[bj], bo[b.len()]));
    }
    // Separator-only matches are weak anchors: keep a rewritten phrase as
    // one deletion followed by one insertion rather than alternating revisions
    // for every word. A retained word always separates independent changes.
    let chars: Vec<char> = old.chars().collect();
    let mut grouped: Vec<(usize, usize, usize, usize)> = Vec::new();
    for span in spans {
        if let Some(previous) = grouped.last_mut() {
            if chars[previous.1..span.0].iter().all(|ch| !ch.is_alphanumeric() && *ch != '_') {
                previous.1 = span.1;
                previous.3 = span.3;
                continue;
            }
        }
        grouped.push(span);
    }
    Ok(grouped)
}

// Text diffing is an engine policy, not a public operation or plan option.
// Rich replacements retain the existing path so requested formatting is applied.
fn diff_eligible(pieces: &[InsPiece], link: Option<&str>) -> bool {
    link.is_none() && pieces.iter().all(|piece| matches!(piece, InsPiece::Text(_, fmt) if *fmt == Fmt::default()))
}

// Any tracked change but the author's own pending insertion keeps a range
// off the diff path; own insertions are rewritten in place.
fn blocks_diff(xml: &DocxXml, segment: &Segment, author: &str) -> bool {
    segment.wrap != RevWrap::None && !is_own_insertion(xml, segment, author)
}

/// Tracked insertion of `pieces` at `offset` that joins `author`'s own
/// pending insertion it touches, never nesting `w:ins` in `w:ins`.
fn insert_own(
    xml: &mut DocxXml,
    paragraph: NodeId,
    offset: usize,
    pieces: &[InsPiece],
    author: &str,
    ctx: &ExecCtx,
) -> Result<(), Outcome> {
    split_own_ins(xml, paragraph, offset, author);
    let id = xml.next_revision_id();
    insert_ins_pieces_at(xml, paragraph, offset, pieces, id, author, &ctx.now, false)
        .map_err(Outcome::error)?;
    merge_own_ins(xml, paragraph, id, author);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn replace_range_diff(
    package: &mut Package,
    xml: &mut DocxXml,
    paragraph: NodeId,
    address: &str,
    (start, end): (usize, usize),
    pieces: &[InsPiece],
    link: Option<String>,
    author: &str,
    ctx: &ExecCtx,
) -> Result<(), Outcome> {
    debug_assert!(diff_eligible(pieces, link.as_deref()));
    let new: String = pieces.iter().filter_map(|piece| match piece {
        InsPiece::Text(text, _) => Some(text.as_str()),
        _ => None,
    }).collect();
    let segments = build_segments(xml, paragraph);
    let covered: Vec<_> = segments
        .iter()
        .filter(|s| s.start < end && s.end > start)
        .collect();
    if covered.iter().any(|s| blocks_diff(xml, s, author)) {
        return Err(blocked_at(
            address,
            "word-level diff selection overlaps existing tracked changes; accept or reject them first",
        ));
    }
    if let (Some(first), Some(last)) = (covered.first(), covered.last()) {
        if let Some(label) = scan_blockers_between(
            xml,
            first.wrapper.unwrap_or(first.run),
            last.wrapper.unwrap_or(last.run),
        ) {
            return Err(blocked_at(
                address,
                format!("word-level diff selection crosses a protected {label}"),
            ));
        }
    }
    let old: String = segments
        .iter()
        .flat_map(|s| s.text.chars())
        .skip(start)
        .take(end - start)
        .collect();
    let changes = match word_diff(&old, &new) {
        Ok(changes) => changes,
        Err(_) => return replace_range(package, xml, paragraph, address, (start, end), (pieces, link), author, true, false, ctx),
    };
    let chars: Vec<char> = new.chars().collect();
    for (a, b, c, d) in changes.into_iter().rev() {
        let replacement: String = chars[c..d].iter().collect();
        let pieces = if replacement.is_empty() {
            vec![]
        } else {
            vec![InsPiece::Text(replacement, Fmt::default())]
        };

        // Retyped text wears the formatting of what it replaces or follows,
        // as in Word: "<b>bold text</b>", text -> words, gives "<b>words</b>".
        let like = typing_rpr(xml, paragraph, start + a, a < b);
        let mark = xml.doc.node_count();
        if a == b {
            insert_own(xml, paragraph, start + a, &pieces, author, ctx)?;
        } else {
            replace_range(
                package,
                xml,
                paragraph,
                address,
                (start + a, start + b),
                (&pieces, None),
                author,
                true,
                false,
                ctx,
            )?;
        }
        if let Some(rpr) = like {
            dress_new_runs(xml, mark, rpr, author);
        }
    }
    Ok(())
}

/// A detached copy of the run properties typing at `offset` inherits: the
/// replaced text's when `replacing`, else the text just left of the
/// offset, else just right. No tracked format change comes along.
fn typing_rpr(xml: &mut DocxXml, paragraph: NodeId, offset: usize, replacing: bool) -> Option<NodeId> {
    let segments = build_segments(xml, paragraph);
    let source = match replacing {
        true => segments.iter().find(|s| s.start <= offset && offset < s.end),
        false => segments
            .iter()
            .rev()
            .find(|s| s.start < offset && offset <= s.end && s.wrap != RevWrap::Del)
            .or_else(|| segments.iter().find(|s| s.start == offset && s.start < s.end && s.wrap != RevWrap::Del)),
    }?;
    let rpr = xml.run_rpr(source.run)?;
    let clone = xml.doc.clone_node(rpr, true);
    let changes: Vec<NodeId> = xml.doc.children(clone).filter(|&n| xml.is_w(n, "rPrChange")).collect();
    for change in changes {
        xml.doc.remove_node(change);
    }
    Some(clone)
}

/// Gives `rpr` to every run allocated after `mark` that sits bare in one of
/// `author`'s insertions: the text just typed.
fn dress_new_runs(xml: &mut DocxXml, mark: usize, rpr: NodeId, author: &str) {
    let fresh: Vec<NodeId> = (mark + 1..=xml.doc.node_count())
        .filter_map(|raw| u32::try_from(raw).ok().and_then(NodeId::from_raw))
        .filter(|&run| xml.is_w(run, "r") && xml.run_rpr(run).is_none())
        .filter(|&run| {
            xml.doc
                .parent(run)
                .is_some_and(|ins| xml.is_w(ins, "ins") && xml.attr(ins, "author") == Some(author))
        })
        .collect();
    for run in fresh {
        let clone = xml.doc.clone_node(rpr, true);
        xml.doc.prepend_child(run, clone);
    }
}

/// Replace the `[start, end)` range of `paragraph` (all-text offsets, as
/// returned by [`select_in_paragraph`]) with parsed `with` pieces: a tracked
/// deletion plus insertion, or a direct rewrite. Offsets to the left of the
/// range stay valid afterwards, so callers editing several ranges of one
/// paragraph apply them right to left.
#[allow(clippy::too_many_arguments)]
fn replace_range(
    package: &mut Package,
    xml: &mut DocxXml,
    paragraph: NodeId,
    address: &str,
    (start, end): (usize, usize),
    (pieces, link_href): (&[InsPiece], Option<String>),
    author: &str,
    tracked: bool,
    auto_diff: bool,
    ctx: &ExecCtx,
) -> Result<(), Outcome> {
    if tracked && auto_diff && diff_eligible(pieces, link_href.as_deref())
        && !build_segments(xml, paragraph).iter().any(|s| s.start < end && s.end > start && blocks_diff(xml, s, author))
    {
        return replace_range_diff(
            package,
            xml,
            paragraph,
            address,
            (start, end),
            pieces,
            link_href,
            author,
            ctx,
        );
    }
    let address = address.to_string();
    if !tracked {
        let segments = build_segments(xml, paragraph);
        let overlaps_revision = segments
            .iter()
            .any(|s| s.start < end && s.end > start && s.wrap != RevWrap::None);
        if overlaps_revision {
            return Err(blocked_at(
                &address,
                "the selection overlaps existing tracked changes — untracked replacement requires accepting or rejecting them first",
            ));
        }
        let intersects_comment = comment_spans(xml, paragraph)
            .iter()
            .any(|(span_start, span_end)| start < *span_end && end > *span_start);
        if intersects_comment {
            return Err(blocked_at(
                &address,
                "the selection intersects a comment range — untracked replacement requires removing or resolving the comment first",
            ));
        }
    }

    split_boundary(xml, paragraph, end);
    split_boundary(xml, paragraph, start);
    let segments = build_segments(xml, paragraph);
    let covered: Vec<&crate::segments::Segment> = segments
        .iter()
        .filter(|s| s.start >= start && s.end <= end && s.start < s.end)
        .collect();
    if covered.is_empty() {
        return Err(blocked_at(&address, "the selection covers no text"));
    }
    let first_anchor = covered[0].wrapper.unwrap_or(covered[0].run);
    let last_anchor = covered[covered.len() - 1]
        .wrapper
        .unwrap_or(covered[covered.len() - 1].run);
    if let Some(label) = scan_blockers_between(xml, first_anchor, last_anchor) {
        return Err(blocked_at(
            &address,
            format!(
                "the selection crosses a {label} that cannot be safely preserved — update, resolve, or remove it before editing this content"
            ),
        ));
    }

    if tracked {
        let del_id = xml.next_revision_id();
        delete_tracked_range(xml, paragraph, start, end, del_id, author, &ctx.now)
            .map_err(Outcome::error)?;

        // A range of only the author's own pending insertion was removed
        // outright, leaving no deletion: the replacement joins that
        // insertion at the range start, as when retyping it in Word.
        let deleted = deleted_end(xml, paragraph, del_id);
        let ins_offset = deleted.unwrap_or(start);
        if !pieces.is_empty() {
            if deleted.is_none() {
                split_own_ins(xml, paragraph, start, author);
            }
            let ins_id = xml.next_revision_id();
            insert_ins_pieces_at(
                xml,
                paragraph,
                ins_offset,
                pieces,
                ins_id,
                author,
                &ctx.now,
                false,
            )
            .map_err(Outcome::error)?;
            if let Some(href) = link_href {
                let rid = crate::html::chrome::ensure_external_hyperlink(package, &href)
                    .map_err(Outcome::error)?;
                crate::html::chrome::wrap_revision_in_hyperlink(xml, paragraph, ins_id, &rid)
                    .map_err(Outcome::error)?;
            } else if deleted.is_none() {
                merge_own_ins(xml, paragraph, ins_id, author);
            }
        }
    } else {
        let insert_before = remove_range_direct(xml, first_anchor, last_anchor)?;
        insert_plain_pieces(
            xml,
            package,
            paragraph,
            insert_before,
            pieces,
            link_href.as_deref(),
        )
        .map_err(Outcome::error)?;
    }

    Ok(())
}

/// All-view end offset of the tracked deletion `id` in `paragraph`, or
/// `None` when no deletion with text was recorded.
fn deleted_end(xml: &DocxXml, paragraph: NodeId, id: u64) -> Option<usize> {
    let del = xml.doc.descendants(paragraph).find(|&node| {
        xml.is_rev_del(node) && xml.attr(node, "id").and_then(|v| v.parse::<u64>().ok()) == Some(id)
    })?;
    build_segments(xml, paragraph)
        .iter()
        .filter(|s| s.wrapper == Some(del))
        .map(|s| s.end)
        .max()
}

#[derive(Debug, Clone)]
pub struct ReviseTextArgs {
    pub at: Option<String>,
    pub within: Option<String>,
    pub occurrence: Option<usize>,
    pub edits: Vec<crate::api::TextEdit>,
    pub author: String,
    pub tracked: bool,
}

/// One paragraph region a `revise_text` scope covers, in the paragraph's
/// current-text character offsets.
struct ScopeRegion {
    paragraph: NodeId,
    address: String,
    projection: SelectionProjection,
    start: usize,
    end: usize,
}

/// Character-offset spans of every non-overlapping `needle` match in `text`.
fn char_matches(text: &str, needle: &str) -> Vec<(usize, usize)> {
    let len = needle.chars().count();
    let mut chars_before = 0;
    let mut last_byte = 0;
    text.match_indices(needle)
        .map(|(byte, _)| {
            chars_before += text[last_byte..byte].chars().count();
            last_byte = byte;
            (chars_before, chars_before + len)
        })
        .collect()
}

fn preview(text: &str) -> String {
    const LIMIT: usize = 240;
    if text.chars().count() <= LIMIT {
        format!("{text:?}")
    } else {
        let head: String = text.chars().take(LIMIT).collect();
        format!("{head:?}…")
    }
}

/// Every addressable paragraph in the part with its canonical id, repairing
/// ids first when some paragraph lacks a valid unique one.
fn addressable_paragraphs(
    xml: &mut DocxXml,
    package: &Package,
    source_hash: Option<[u8; 32]>,
) -> Result<Vec<(NodeId, String)>, Outcome> {
    if find_with_clean_ids(xml, "").is_none() {
        let part = package.document_part_name();
        let seed = source_hash.unwrap_or_else(|| {
            use sha2::{Digest, Sha256};
            Sha256::digest(package.get(&part).unwrap_or_default()).into()
        });
        let index = xml.resolve_paragraph_ids(&part, &seed);
        xml.apply_paragraph_ids(&index).map_err(Outcome::error)?;
    }
    Ok(xml
        .all_paragraphs()
        .into_iter()
        .filter_map(|node| xml.para_id(node).map(|id| (node, id.to_uppercase())))
        .collect())
}

/// `revise_text`: several exact replacements inside one bounded scope. The
/// scope and every edit resolve against the text as it stands when the
/// operation starts, so earlier edits never shift later selectors.
pub fn revise_text_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    args: &ReviseTextArgs,
    ctx: &ExecCtx,
    source_hash: Option<[u8; 32]>,
) -> Result<(String, Value), Outcome> {
    let fail = |at: Option<&str>, message: String| match at {
        Some(at) => blocked_at(at, message),
        None => Outcome::blocked(message),
    };
    let at = args.at.as_deref().map(str::to_uppercase);
    let mut parsed = Vec::with_capacity(args.edits.len());
    for (index, edit) in args.edits.iter().enumerate() {
        if edit.select.contains('<') {
            return Err(fail(
                at.as_deref(),
                format!("edits[{index}].select must be plain text without HTML markup — it addresses visible text, never markup"),
            ));
        }
        let (pieces, link) = parse_with(&edit.with).map_err(|outcome| {
            fail(at.as_deref(), format!("edits[{index}].with: {}", outcome.summary))
        })?;
        parsed.push((pieces, link));
    }

    let candidates = match &at {
        Some(address) => {
            vec![(resolve_paragraph_by_para_id(xml, package, source_hash, address)?, address.clone())]
        }
        None => addressable_paragraphs(xml, package, source_hash)?,
    };
    let mut regions: Vec<ScopeRegion> = Vec::new();
    match &args.within {
        Some(within) => {
            let mut found = Vec::new();
            for (paragraph, address) in &candidates {
                let projection = selection_projection(xml, *paragraph);
                for (start, end) in char_matches(&projection.current, within) {
                    found.push((*paragraph, address.clone(), start, end));
                }
            }
            let place = if at.is_some() { "this paragraph" } else { "the document" };
            let chosen = match (found.len(), args.occurrence) {
                (0, _) => {
                    let detail = match &at {
                        Some(_) => format!(
                            " (current visible text: {})",
                            preview(&selection_projection(xml, candidates[0].0).current)
                        ),
                        None => String::new(),
                    };
                    return Err(fail(
                        at.as_deref(),
                        format!("within {within:?} matched nothing in {place}{detail} — hidden deleted text is not selectable"),
                    ));
                }
                (count, Some(n)) if n >= 1 && n <= count => n - 1,
                (count, Some(n)) => {
                    return Err(fail(
                        at.as_deref(),
                        format!("occurrence {n} is out of range: within matched {count} times in {place}"),
                    ))
                }
                (1, None) => 0,
                (count, None) => {
                    let mut ids: Vec<&str> = found.iter().map(|f| f.1.as_str()).collect();
                    ids.dedup();
                    ids.truncate(8);
                    return Err(fail(
                        at.as_deref(),
                        format!(
                            "within {within:?} matched {count} times in {place} (paragraphs {}) — lengthen it, pass at, or pass occurrence (1-based)",
                            ids.join(", ")
                        ),
                    ));
                }
            };
            let (paragraph, address, start, end) = found.swap_remove(chosen);
            let projection = selection_projection(xml, paragraph);
            regions.push(ScopeRegion { paragraph, address, projection, start, end });
        }
        None => {
            for (paragraph, address) in candidates {
                let projection = selection_projection(xml, paragraph);
                let end = projection.current.chars().count();
                regions.push(ScopeRegion { paragraph, address, projection, start: 0, end });
            }
        }
    }
    let scope = match (&args.within, &at) {
        (Some(within), _) => format!("within {within:?}"),
        (None, Some(_)) => format!(
            "this paragraph (current visible text: {})",
            preview(&regions[0].projection.current)
        ),
        (None, None) => "the document".to_string(),
    };

    // (region, start, end, edit) in current-text offsets.
    let mut spans: Vec<(usize, usize, usize, usize)> = Vec::new();
    for (index, edit) in args.edits.iter().enumerate() {
        let mut matches = Vec::new();
        for (region_index, region) in regions.iter().enumerate() {
            let slice: String = region
                .projection
                .current
                .chars()
                .skip(region.start)
                .take(region.end - region.start)
                .collect();
            for (start, end) in char_matches(&slice, &edit.select) {
                matches.push((region_index, region.start + start, region.start + end, index));
            }
        }
        let select = &edit.select;
        match (matches.len(), edit.occurrence, edit.all) {
            (0, _, _) => {
                return Err(fail(
                    at.as_deref(),
                    format!("edits[{index}].select {select:?} matched nothing in {scope}"),
                ))
            }
            (_, _, true) => spans.extend(matches),
            (count, Some(n), _) if (n as usize) >= 1 && (n as usize) <= count => {
                spans.push(matches[n as usize - 1])
            }
            (count, Some(n), _) => {
                return Err(fail(
                    at.as_deref(),
                    format!("edits[{index}].occurrence {n} is out of range: select {select:?} matched {count} times in {scope}"),
                ))
            }
            (1, None, false) => spans.push(matches[0]),
            (count, None, false) => {
                return Err(fail(
                    at.as_deref(),
                    format!("edits[{index}].select {select:?} matched {count} times in {scope} — narrow the scope with within, lengthen select, or pass occurrence or all"),
                ))
            }
        }
    }

    // Each paragraph is exactly one region, and regions are in document order.
    spans.sort_by_key(|&(region, start, end, _)| (region, start, end));
    for pair in spans.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if a.0 == b.0 && b.1 < a.2 {
            return Err(fail(
                Some(&regions[a.0].address),
                format!(
                    "edits[{}] and edits[{}] select overlapping text; combine them into one edit",
                    a.3.min(b.3),
                    a.3.max(b.3)
                ),
            ));
        }
    }

    // Right to left within each paragraph keeps the remaining offsets valid.
    for &(region, start, end, index) in spans.iter().rev() {
        let region = &regions[region];
        let all_start = region.projection.to_all[start];
        let all_end = region.projection.to_all[end - 1] + 1;
        let (pieces, link) = &parsed[index];
        replace_range(
            package,
            xml,
            region.paragraph,
            &region.address,
            (all_start, all_end),
            (pieces, link.clone()),
            &args.author,
            args.tracked,
            WORD_LEVEL_REDLINES,
            ctx,
        )?;
    }

    let mut paragraphs: Vec<String> = Vec::new();
    for &(region, ..) in &spans {
        let address = &regions[region].address;
        if paragraphs.last() != Some(address) {
            paragraphs.push(address.clone());
        }
    }
    let summary = format!(
        "revised {} span{} in {} paragraph{} ({}) {scope}",
        spans.len(),
        if spans.len() == 1 { "" } else { "s" },
        paragraphs.len(),
        if paragraphs.len() == 1 { "" } else { "s" },
        paragraphs.join(", ")
    );
    let result = json!({
        "paragraphs": paragraphs,
        "replacements": spans.len(),
        "tracked": args.tracked,
    });
    Ok((summary, result))
}

/// `replace_paragraph`: replace all current live content while preserving
/// paragraph properties and `paraId`.
pub fn replace_paragraph_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    args: &ReplaceTextArgs,
    ctx: &ExecCtx,
    source_hash: Option<[u8; 32]>,
) -> Result<(String, Value), Outcome> {
    let input = OpInput {
        at: args.at.clone(),
        with: args.with.clone(),
        author: args.author.clone(),
        tracked: args.tracked,
    };
    let (pieces, link_href) = parse_with(&input.with)?;
    let paragraph = resolve_paragraph_by_para_id(xml, package, source_hash, &input.at)?;
    let address = input.at.to_uppercase();

    if let Some(label) = paragraph_blocker(xml, paragraph) {
        return Err(blocked_at(
            &address,
            format!(
                "the paragraph contains a {label} that cannot be safely preserved — replace_paragraph requires unambiguous content; accept, resolve, or remove it first"
            ),
        ));
    }

    let count = build_segments(xml, paragraph)
        .iter()
        .map(|s| s.end)
        .max()
        .unwrap_or(0);
    if input.tracked && WORD_LEVEL_REDLINES && diff_eligible(&pieces, link_href.as_deref()) {
        replace_range(
            package,
            xml,
            paragraph,
            &address,
            (0, count),
            (&pieces, link_href),
            &input.author,
            true,
            true,
            ctx,
        )?;
    } else if input.tracked {
        let del_id = xml.next_revision_id();
        wrap_all_content_in_del(xml, paragraph, del_id, &input.author, &ctx.now)
            .map_err(Outcome::error)?;
        if !pieces.is_empty() {
            // An empty paragraph has nothing to delete: insert at its end.
            let ins_offset = deleted_end(xml, paragraph, del_id).unwrap_or(count);
            let ins_id = xml.next_revision_id();
            insert_ins_pieces_at(
                xml,
                paragraph,
                ins_offset,
                &pieces,
                ins_id,
                &input.author,
                &ctx.now,
                false,
            )
            .map_err(Outcome::error)?;
            if let Some(href) = link_href {
                let rid = crate::html::chrome::ensure_external_hyperlink(package, &href)
                    .map_err(Outcome::error)?;
                crate::html::chrome::wrap_revision_in_hyperlink(xml, paragraph, ins_id, &rid)
                    .map_err(Outcome::error)?;
            }
        }
    } else {
        let content: Vec<NodeId> = xml
            .doc
            .children(paragraph)
            .filter(|&node| is_run_content(xml, node))
            .collect();
        let insert_before = content_insert_point(xml, paragraph, &content);
        for node in &content {
            // Range markers nested in removed content stay, collapsed.
            crate::segments::remove_content(xml, *node);
        }
        insert_plain_pieces(
            xml,
            package,
            paragraph,
            insert_before,
            &pieces,
            link_href.as_deref(),
        )
        .map_err(Outcome::error)?;
    }

    let summary = format!(
        "replaced paragraph {address} ({})",
        if input.tracked { "tracked" } else { "direct" }
    );
    let result = json!({ "paraId": address, "tracked": input.tracked });
    Ok((summary, result))
}

/// Comment anchor spans in all-view offset space: `(start, end)` per comment
/// range, derived from the paragraph's `w:commentRangeStart`/`w:commentRangeEnd`
/// milestone positions (milestones themselves carry no characters). Used by
/// the untracked gate, which must not silently edit comment-anchored content.
fn comment_spans(xml: &DocxXml, paragraph: NodeId) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut open: Vec<(String, usize)> = Vec::new();
    let mut offset = 0usize;
    let mut cursor = xml.doc.children(paragraph).find(|&n| xml.doc.is_element(n));
    while let Some(node) = cursor {
        if xml.is_w(node, "commentRangeStart") {
            if let Some(id) = xml.attr(node, "id") {
                open.push((id.to_string(), offset));
            }
        } else if xml.is_w(node, "commentRangeEnd") {
            if let Some(id) = xml.attr(node, "id") {
                if let Some(pos) = open.iter().rposition(|(open_id, _)| open_id == id) {
                    let (_, start) = open.remove(pos);
                    spans.push((start, offset));
                }
            }
        } else {
            offset += xml
                .paragraph_text(node, crate::document::View::All)
                .chars()
                .count();
        }
        cursor = xml.doc.next_sibling(node);
    }
    spans
}

/// Remove the paragraph children from `first_anchor` to `last_anchor`
/// inclusive (the covered range after boundary splits). Returns the element
/// to insert before, or None to append.
fn remove_range_direct(
    xml: &mut DocxXml,
    first_anchor: NodeId,
    last_anchor: NodeId,
) -> Result<Option<NodeId>, Outcome> {
    let mut nodes = Vec::new();
    let mut cursor = Some(first_anchor);
    while let Some(node) = cursor {
        nodes.push(node);
        if node == last_anchor {
            break;
        }
        cursor = xml.doc.next_sibling(node);
    }
    if nodes.last() != Some(&last_anchor) {
        return Err(Outcome::error("internal: covered range is not contiguous"));
    }
    let insert_before = xml
        .doc
        .next_sibling(last_anchor)
        .filter(|&n| xml.doc.is_element(n));
    for node in nodes {
        // Collect hyperlink ancestors first (the node is still attached).
        let mut ancestors = Vec::new();
        let mut parent = xml.doc.parent(node);
        while let Some(candidate) = parent {
            if !xml.is_w(candidate, "hyperlink") {
                break;
            }
            ancestors.push(candidate);
            parent = xml.doc.parent(candidate);
        }
        // Range markers (bookmarks, permissions, …) are not content: they
        // stay, collapsed, so a pair is never cut in two.
        if crate::segments::is_range_marker(xml, node) {
            continue;
        }
        crate::segments::remove_content(xml, node);
        for ancestor in ancestors {
            let markers_only = xml
                .doc
                .children(ancestor)
                .filter(|&n| xml.doc.is_element(n))
                .all(|n| crate::segments::is_range_marker(xml, n));
            if markers_only {
                let markers: Vec<NodeId> = xml.doc.children(ancestor).filter(|&n| xml.doc.is_element(n)).collect();
                for marker in markers {
                    xml.doc.detach(marker);
                    xml.doc.insert_before(ancestor, marker);
                }
                xml.doc.remove_node(ancestor);
            }
        }
    }
    Ok(insert_before)
}

/// Where to insert new content in a paragraph whose content runs were removed:
/// the first element sibling after the last removed node, or (when the
/// paragraph had no content) the first non-pPr element child.
fn content_insert_point(xml: &DocxXml, paragraph: NodeId, content: &[NodeId]) -> Option<NodeId> {
    if let Some(last_content) = content.last() {
        let mut cursor = xml.doc.next_sibling(*last_content);
        while let Some(node) = cursor {
            if xml.doc.is_element(node) {
                return Some(node);
            }
            cursor = xml.doc.next_sibling(node);
        }
        return None;
    }
    let mut cursor = xml.doc.children(paragraph).find(|&n| xml.doc.is_element(n));
    while let Some(node) = cursor {
        if !xml.is_w(node, "pPr") {
            return Some(node);
        }
        cursor = xml.doc.next_sibling(node);
    }
    None
}

/// Tracked whole-paragraph deletion: one `<w:del>` wrapping every content run
/// (and hyperlink/smartTag wrapper), text leaves renamed to `w:delText` and
/// field codes to `w:delInstrText`.
fn wrap_all_content_in_del(
    xml: &mut DocxXml,
    paragraph: NodeId,
    id: u64,
    author: &str,
    date: &str,
) -> Result<(), String> {
    let content: Vec<NodeId> = xml
        .doc
        .children(paragraph)
        .filter(|&node| is_run_content(xml, node))
        .collect();
    if content.is_empty() {
        return Ok(());
    }
    // Adjacent runs share one deletion, so milestones between them keep their
    // positions; a hyperlink or bidi wrapper is never
    // wrapped (a deletion holds runs only), so its runs are deleted inside it.
    let mut del: Option<NodeId> = None;
    for node in content {
        if xml.is_w(node, "hyperlink") || xml.is_w(node, "dir") || xml.is_w(node, "bdo") {
            move_content_into_del(xml, node, id, author, date)?;
            del = None;
            continue;
        }
        let adjacent = del.filter(|&wrapper| xml.doc.prev_sibling(node) == Some(wrapper));
        let wrapper = match adjacent {
            Some(wrapper) => wrapper,
            None => {
                let wrapper = xml.doc.create_element("w:del");
                xml.doc.set_attribute(wrapper, "w:id", &id.to_string());
                xml.doc.set_attribute(wrapper, "w:author", author);
                xml.doc.set_attribute(wrapper, "w:date", date);
                xml.doc.insert_before(node, wrapper);
                del = Some(wrapper);
                wrapper
            }
        };
        xml.doc.detach(node);
        xml.doc.append_child(wrapper, node);
        crate::segments::mark_deleted_leaves(xml, node, true);
    }
    Ok(())
}

/// Attach a freshly created paragraph-level node at the anchor position.
fn attach_created(xml: &mut DocxXml, paragraph: NodeId, before: Option<NodeId>, node: NodeId) {
    match before {
        Some(anchor) => xml.doc.insert_before(anchor, node),
        None => xml.doc.append_child(paragraph, node),
    }
}

/// Create (and attach) a plain run with the given formatting, reusing the
/// current run while the formatting is unchanged.
#[allow(clippy::too_many_arguments)]
fn ensure_plain_run(
    xml: &mut DocxXml,
    paragraph: NodeId,
    before: Option<NodeId>,
    first: &mut Option<NodeId>,
    run: &mut Option<NodeId>,
    run_fmt: &mut Fmt,
    fmt: Fmt,
) -> NodeId {
    if run.is_none() || *run_fmt != fmt {
        let new_run = xml.doc.create_element("w:r");
        if fmt != Fmt::default() {
            let rpr = xml.doc.create_element("w:rPr");
            crate::html::styles::append_rpr(xml, rpr, fmt);
            xml.doc.append_child(new_run, rpr);
        }
        attach_created(xml, paragraph, before, new_run);
        if first.is_none() {
            *first = Some(new_run);
        }
        *run = Some(new_run);
        *run_fmt = fmt;
    }
    run.expect("just ensured")
}

/// Insert untracked (plain) content pieces at an anchor position, optionally
/// wrapped in one hyperlink.
fn insert_plain_pieces(
    xml: &mut DocxXml,
    package: &mut Package,
    paragraph: NodeId,
    before: Option<NodeId>,
    pieces: &[InsPiece],
    link_href: Option<&str>,
) -> Result<(), String> {
    let mut first: Option<NodeId> = None;
    let mut last: Option<NodeId> = None;
    let mut run: Option<NodeId> = None;
    let mut run_fmt = Fmt::default();

    for piece in pieces {
        match piece {
            InsPiece::Text(text, fmt) => {
                let node = ensure_plain_run(
                    xml,
                    paragraph,
                    before,
                    &mut first,
                    &mut run,
                    &mut run_fmt,
                    *fmt,
                );
                let t = xml.doc.create_element("w:t");
                if text.starts_with(' ') || text.ends_with(' ') {
                    xml.ensure_space_preserve(t);
                }
                let content = xml.doc.create_text(text);
                xml.doc.append_child(t, content);
                xml.doc.append_child(node, t);
                last = Some(node);
            }
            InsPiece::Br(kind) => {
                let keep = run_fmt;
                let node = ensure_plain_run(
                    xml,
                    paragraph,
                    before,
                    &mut first,
                    &mut run,
                    &mut run_fmt,
                    keep,
                );
                let br = xml.doc.create_element("w:br");
                if let Some(break_type) = kind.dialect_type() {
                    xml.doc.set_attribute(br, "w:type", break_type);
                }
                xml.doc.append_child(node, br);
                last = Some(node);
            }
            InsPiece::Tab => {
                let keep = run_fmt;
                let node = ensure_plain_run(
                    xml,
                    paragraph,
                    before,
                    &mut first,
                    &mut run,
                    &mut run_fmt,
                    keep,
                );
                let tab = xml.doc.create_element("w:tab");
                xml.doc.append_child(node, tab);
                last = Some(node);
            }
            InsPiece::Equation { mathml, display } => {
                let omml = crate::html::mathml_omml::mathml_to_omml_xml(mathml, *display)?;
                let existing: Vec<NodeId> = xml.doc.children(paragraph).collect();
                crate::segments::append_omml_xml(xml, paragraph, &omml)?;
                let created: Vec<NodeId> = xml
                    .doc
                    .children(paragraph)
                    .filter(|&node| !existing.contains(&node))
                    .collect();
                for node in created {
                    xml.doc.detach(node);
                    attach_created(xml, paragraph, before, node);
                    if first.is_none() {
                        first = Some(node);
                    }
                    last = Some(node);
                }
                run = None;
            }
            _ => return Err("internal: unsupported piece for untracked insertion".to_string()),
        }
    }

    if let (Some(href), Some(first_node), Some(last_node)) = (link_href, first, last) {
        let rid = crate::html::chrome::ensure_external_hyperlink(package, href)?;
        let mut nodes = Vec::new();
        let mut cursor = Some(first_node);
        while let Some(node) = cursor {
            nodes.push(node);
            if node == last_node {
                break;
            }
            cursor = xml.doc.next_sibling(node);
        }
        // A part without hyperlinks may not declare the relationships prefix.
        if let Some(root) = xml.doc.root_element() {
            let declared = xml.doc.attributes(root).iter().any(|a| {
                a.name == "xmlns:r" || (a.name == "r" && a.value.contains("relationships"))
            });
            if !declared {
                xml.doc.set_attribute(
                    root,
                    "xmlns:r",
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
                );
            }
        }
        let link = xml.doc.create_element("w:hyperlink");
        xml.doc.set_attribute(link, "r:id", &rid);
        attach_created(xml, paragraph, before, link);
        for node in nodes {
            xml.doc.detach(node);
            xml.doc.append_child(link, node);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Phase 3: typed formatting and paragraph structure.
// ---------------------------------------------------------------------------

#[cfg(any())]
fn opt_bool(command: &Value, name: &str) -> Result<Option<bool>, Outcome> {
    match command.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(Outcome::error(format!("{name} must be a boolean"))),
    }
}

#[cfg(any())]
fn optional_string<'a>(command: &'a Value, name: &str) -> Result<Option<&'a str>, Outcome> {
    match command.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.is_empty() => Ok(Some(s)),
        Some(_) => Err(Outcome::error(format!("{name} must be a non-empty string"))),
    }
}

#[cfg(any())]
fn optional_twips(command: &Value, name: &str) -> Result<Option<i64>, Outcome> {
    match command.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            let v = n
                .as_i64()
                .ok_or_else(|| Outcome::error(format!("{name} must be an integer (twips)")))?;
            if v < 0 {
                return Err(Outcome::error(format!("{name} must be >= 0")));
            }
            Ok(Some(v))
        }
        Some(_) => Err(Outcome::error(format!("{name} must be an integer (twips)"))),
    }
}

/// One requested character-property change for the typed `format_text` op.
/// Omitted fields are untouched; `clear` resets named properties to their
/// inherited values. False is an explicit off override. Superscript and
/// subscript cannot both be enabled.
struct CharFormat {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    superscript: Option<bool>,
    subscript: Option<bool>,
    /// Normalized `#RRGGBB` color as authored.
    highlight: Option<crate::api::HighlightColor>,
    color: Option<String>,
    /// Point size.
    font_size: Option<f64>,
    clear: Vec<String>,
}

#[cfg(any())]
const CHAR_PROPERTY_NAMES: &[&str] = &[
    "bold",
    "italic",
    "underline",
    "strike",
    "superscript",
    "subscript",
    "color",
    "font_size",
];

#[cfg(any())]
fn parse_char_format(command: &Value) -> Result<CharFormat, Outcome> {
    let bold = opt_bool(command, "bold")?;
    let italic = opt_bool(command, "italic")?;
    let underline = opt_bool(command, "underline")?;
    let strike = opt_bool(command, "strike")?;
    let superscript = opt_bool(command, "superscript")?;
    let subscript = opt_bool(command, "subscript")?;
    if superscript == Some(true) && subscript == Some(true) {
        return Err(Outcome::error(
            "format_text: superscript and subscript cannot both be enabled",
        ));
    }
    let color = match command.get("color") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            let hex = s.strip_prefix('#').unwrap_or(s);
            if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Outcome::error(format!(
                    "format_text: color must be a #RRGGBB hex value, not {s:?}"
                )));
            }
            Some(s.clone())
        }
        Some(_) => return Err(Outcome::error("format_text: color must be a string")),
    };
    let font_size = match command.get("font_size") {
        None | Some(Value::Null) => None,
        Some(Value::Number(n)) => match n.as_f64() {
            Some(v) if v > 0.0 => Some(v),
            _ => {
                return Err(Outcome::error(
                    "format_text: font_size must be a positive number (points)",
                ))
            }
        },
        Some(_) => {
            return Err(Outcome::error(
                "format_text: font_size must be a number (points)",
            ))
        }
    };
    let clear = match command.get("clear") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => {
            let mut names = Vec::new();
            for item in items {
                let name = item
                    .as_str()
                    .ok_or_else(|| Outcome::error("format_text: clear entries must be strings"))?;
                if !CHAR_PROPERTY_NAMES.contains(&name) {
                    return Err(Outcome::error(format!(
                        "format_text: clear entry {name:?} is not a format_text property (allowed: {})",
                        CHAR_PROPERTY_NAMES.join(", ")
                    )));
                }
                names.push(name.to_string());
            }
            names
        }
        Some(_) => {
            return Err(Outcome::error(
                "format_text: clear must be an array of property names",
            ))
        }
    };
    let any = bold.is_some()
        || italic.is_some()
        || underline.is_some()
        || strike.is_some()
        || superscript.is_some()
        || subscript.is_some()
        || color.is_some()
        || font_size.is_some()
        || !clear.is_empty();
    if !any {
        return Err(Outcome::error(
            "format_text requires at least one of bold, italic, underline, strike, superscript, subscript, color, font_size, clear",
        ));
    }
    Ok(CharFormat {
        bold,
        italic,
        underline,
        strike,
        superscript,
        subscript,
        color,
        font_size,
        clear,
    })
}

/// Requested direct properties only. `false` writes an explicit override;
/// `clear` removes the direct property so styles/defaults can supply it again.
/// Clear wins when a property is also set in the same operation.
fn char_format_properties(
    fmt: &CharFormat,
) -> std::collections::BTreeMap<&'static str, Option<String>> {
    let mut properties = std::collections::BTreeMap::new();
    for (name, value, on, off) in [
        ("b", fmt.bold, "1", "0"),
        ("i", fmt.italic, "1", "0"),
        ("u", fmt.underline, "single", "none"),
        ("strike", fmt.strike, "1", "0"),
        ("vertAlign", fmt.superscript, "superscript", "baseline"),
        ("vertAlign", fmt.subscript, "subscript", "baseline"),
    ] {
        if let Some(value) = value {
            properties.insert(name, Some(if value { on } else { off }.to_owned()));
        }
    }
    if let Some(highlight) = fmt.highlight {
        properties.insert("highlight", Some(highlight.as_str().to_owned()));
    }
    if let Some(color) = &fmt.color {
        properties.insert("color", Some(color.trim_start_matches('#').to_uppercase()));
    }
    if let Some(size) = fmt.font_size {
        properties.insert("sz", Some((size * 2.0).round().max(1.0).to_string()));
    }
    for name in &fmt.clear {
        let tag = match name.as_str() {
            "bold" => "b",
            "italic" => "i",
            "underline" => "u",
            "strike" => "strike",
            "superscript" | "subscript" => "vertAlign",
            "highlight" => "highlight",
            "color" => "color",
            "font_size" => "sz",
            _ => continue,
        };
        properties.insert(tag, None);
    }
    properties
}

/// Filter out requests already represented by the direct property bag. Never
/// reduce an unrequested property to a bool/RGB/size: that would lose explicit
/// off values, themed colors, underline variants and other source metadata.
fn changed_run_properties(
    xml: &DocxXml,
    run: NodeId,
    requested: &std::collections::BTreeMap<&'static str, Option<String>>,
) -> Vec<(&'static str, Option<String>)> {
    requested
        .iter()
        .filter_map(|(&name, value)| {
            let nodes = xml
                .run_rpr(run)
                .map(|rpr| {
                    xml.doc
                        .children(rpr)
                        .filter(|&n| xml.is_w(n, name))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let matches = match (nodes.as_slice(), value) {
                ([], None) => true,
                ([node], Some(value)) => {
                    // An explicit set replaces metadata on the requested property
                    // (e.g. a theme color), but leaves all other properties intact.
                    let attributes = xml.doc.attributes(*node);
                    let direct_value = xml.attr(*node, "val");
                    let same_value = if matches!(name, "b" | "i" | "strike") {
                        matches!(
                            (direct_value, value.as_str()),
                            (None | Some("1" | "true" | "on"), "1")
                                | (Some("0" | "false" | "off"), "0")
                        )
                    } else {
                        direct_value == Some(value.as_str())
                    };
                    same_value
                        && (attributes.is_empty()
                            || (attributes.len() == 1 && direct_value.is_some()))
                        && xml.doc.children(*node).next().is_none()
                }
                _ => false,
            };
            (!matches).then(|| (name, value.clone()))
        })
        .collect()
}

fn write_run_properties(xml: &mut DocxXml, run: NodeId, changes: &[(&str, Option<String>)]) {
    let rpr = match xml.run_rpr(run) {
        Some(rpr) => rpr,
        None => {
            let rpr = xml.doc.create_element("w:rPr");
            if let Some(first) = xml.doc.children(run).next() {
                xml.doc.insert_before(first, rpr);
            } else {
                xml.doc.append_child(run, rpr);
            }
            rpr
        }
    };
    for (name, value) in changes {
        let old: Vec<_> = xml
            .doc
            .children(rpr)
            .filter(|&n| xml.is_w(n, name))
            .collect();
        for node in old {
            xml.doc.remove_node(node);
        }
        if let Some(value) = value {
            let node = ensure_rpr_child(xml, rpr, name);
            xml.doc.set_attribute(node, "w:val", value);
        }
    }
}

/// Apply character formatting to all-view char span `[start, end)` inside the
/// paragraph: split boundaries, then per covered run record a `w:rPrChange`
/// with the complete prior run-property snapshot. A covered run carrying an
/// existing pending `w:rPrChange` is incompatible and blocks the operation.
fn apply_char_format(
    xml: &mut DocxXml,
    paragraph: NodeId,
    start: usize,
    end: usize,
    fmt: &CharFormat,
    id: u64,
    author: &str,
    now: &str,
) -> Result<usize, String> {
    if start >= end {
        return Err("format_text selection is empty".into());
    }
    split_boundary(xml, paragraph, start);
    split_boundary(xml, paragraph, end);
    let segments = build_segments(xml, paragraph);
    let runs: Vec<NodeId> = segments
        .iter()
        .filter(|s| s.start < end && s.end > start)
        .map(|s| s.run)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if runs.is_empty() {
        return Err("format_text matched no runs".into());
    }
    let requested = char_format_properties(fmt);
    let mut changed = 0usize;
    for run in runs {
        if xml
            .run_rpr(run)
            .is_some_and(|rpr| xml.child_w(rpr, "rPrChange").is_some())
        {
            return Err(
                "a covered run already has a pending format change (w:rPrChange) — incompatible with a new change; accept or reject it before formatting again"
                    .into(),
            );
        }
        let changes = changed_run_properties(xml, run, &requested);
        if changes.is_empty() {
            continue;
        }
        let snapshot = xml.doc.create_element("w:rPr");
        if let Some(rpr) = xml.run_rpr(run) {
            for child in xml.doc.children(rpr).collect::<Vec<_>>() {
                if xml.is_w(child, "rPrChange") {
                    continue;
                }
                let clone = xml.doc.clone_node(child, true);
                xml.doc.append_child(snapshot, clone);
            }
        }
        write_run_properties(xml, run, &changes);
        let rpr = xml.run_rpr(run).expect("just ensured");
        let change = xml.doc.create_element("w:rPrChange");
        xml.doc.set_attribute(change, "w:id", &id.to_string());
        xml.doc.set_attribute(change, "w:author", author);
        xml.doc.set_attribute(change, "w:date", now);
        xml.doc.append_child(change, snapshot);
        xml.doc.append_child(rpr, change);
        changed += 1;
    }
    Ok(changed)
}

/// `format_text`: apply character formatting to a selected plain-text range.
/// Always tracked: every covered run whose modeled properties change receives
/// a `w:rPrChange` snapshot of its complete prior run properties. A covered
/// run with an incompatible pending `w:rPrChange` blocks.
#[derive(Debug, Clone)]
pub struct FormatTextArgs {
    pub at: String,
    pub select: String,
    pub occurrence: Option<usize>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub superscript: Option<bool>,
    pub subscript: Option<bool>,
    pub highlight: Option<crate::api::HighlightColor>,
    pub color: Option<String>,
    pub font_size: Option<f64>,
    pub clear: Vec<String>,
    pub author: String,
}
pub fn format_text_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    args: &FormatTextArgs,
    ctx: &ExecCtx,
    source_hash: Option<[u8; 32]>,
) -> Result<(String, Value), Outcome> {
    let at = &args.at;
    if !is_valid_para_id(&at.to_uppercase()) {
        return Err(Outcome::error(format!(
            "at must be a valid w14:paraId (eight hexadecimal digits with 00000000 < value < 80000000): {at:?}"
        )));
    }
    let select = args.select.clone();
    if select.contains('<') {
        return Err(blocked_at(
            &at.to_uppercase(),
            "select must be plain text without HTML markup — select addresses the paragraph's visible text, never markup",
        ));
    }
    let occurrence = args.occurrence;
    if args.superscript == Some(true) && args.subscript == Some(true) {
        return Err(Outcome::error(
            "superscript and subscript cannot both be enabled",
        ));
    }
    let fmt = CharFormat {
        bold: args.bold,
        italic: args.italic,
        underline: args.underline,
        strike: args.strike,
        superscript: args.superscript,
        subscript: args.subscript,
        highlight: args.highlight,
        color: args.color.clone(),
        font_size: args.font_size,
        clear: args.clear.clone(),
    };
    let author = args.author.clone();

    let paragraph = resolve_paragraph_by_para_id(xml, package, source_hash, at)?;
    let address = at.to_uppercase();
    let (start, end, _current, matched) = select_in_paragraph(xml, paragraph, &select, occurrence)
        .map_err(|outcome| blocked_at(&address, outcome.summary))?;

    let id = xml.next_revision_id();
    let changed = apply_char_format(xml, paragraph, start, end, &fmt, id, &author, &ctx.now)
        .map_err(|e| blocked_at(&address, e))?;

    let summary = if changed == 0 {
        format!("paragraph {address} formatting already matches (no change recorded)")
    } else {
        format!(
            "formatted {changed} run{} in paragraph {address}",
            if changed == 1 { "" } else { "s" }
        )
    };
    let result = json!({
        "paraId": address,
        "matched": matched,
        "occurrence": occurrence,
        "runsChanged": changed,
        "tracked": true,
    });
    Ok((summary, result))
}

#[cfg(any())]
pub fn format_text(
    package: &mut Package,
    xml: &mut DocxXml,
    command: &Value,
    ctx: &ExecCtx,
    source_hash: Option<[u8; 32]>,
) -> Result<(String, Value), Outcome> {
    let at = required_string(command, "at")?.to_owned();
    let select = required_string(command, "select")?.to_owned();
    let args = FormatTextArgs {
        at,
        select,
        occurrence: command
            .get("occurrence")
            .and_then(Value::as_u64)
            .map(|n| n as usize),
        bold: command.get("bold").and_then(Value::as_bool),
        italic: command.get("italic").and_then(Value::as_bool),
        underline: command.get("underline").and_then(Value::as_bool),
        strike: command.get("strike").and_then(Value::as_bool),
        superscript: command.get("superscript").and_then(Value::as_bool),
        subscript: command.get("subscript").and_then(Value::as_bool),
        color: command
            .get("color")
            .and_then(Value::as_str)
            .map(str::to_owned),
        font_size: command.get("font_size").and_then(Value::as_f64),
        clear: command
            .get("clear")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        author: command
            .get("author")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_AUTHOR)
            .into(),
    };
    format_text_typed(package, xml, &args, ctx, source_hash)
}

/// One requested paragraph-property change for the typed `format_paragraph`
/// op. Measurements arrive as twips from the plan layer; `line`/`line_rule`
/// carry the resolved spacing. Omitted fields are untouched; `clear` resets
/// named properties to their defaults.
struct ParaFormat {
    style: Option<String>,
    alignment: Option<String>,
    indent_left: Option<i64>,
    indent_right: Option<i64>,
    first_line_indent: Option<i64>,
    space_before: Option<i64>,
    space_after: Option<i64>,
    line: Option<i64>,
    line_rule: Option<String>,
    clear: Vec<String>,
}

#[cfg(any())]
const PARA_PROPERTY_NAMES: &[&str] = &[
    "style",
    "alignment",
    "indent_left",
    "indent_right",
    "space_before",
    "space_after",
    "line_spacing",
];

#[cfg(any())]
fn parse_para_format(command: &Value) -> Result<ParaFormat, Outcome> {
    let style = optional_string(command, "style")?.map(str::to_string);
    let alignment = match command.get("alignment") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => match s.to_ascii_lowercase().as_str() {
            "left" | "center" | "right" => Some(s.to_ascii_lowercase()),
            "justify" | "both" | "justified" => Some("both".to_string()),
            other => {
                return Err(Outcome::error(format!(
                    "alignment must be left|center|right|justify, not {other:?}"
                )))
            }
        },
        Some(_) => return Err(Outcome::error("alignment must be a string")),
    };
    let indent_left = optional_twips(command, "indentLeft")?;
    let indent_right = optional_twips(command, "indentRight")?;
    let space_before = optional_twips(command, "spaceBefore")?;
    let space_after = optional_twips(command, "spaceAfter")?;
    let line = optional_twips(command, "line")?;
    let line_rule = match optional_string(command, "lineRule")? {
        Some(rule) => match rule.to_ascii_lowercase().as_str() {
            "auto" | "exact" => Some(rule.to_ascii_lowercase()),
            "atleast" | "at_least" | "at-least" => Some("atLeast".to_string()),
            other => {
                return Err(Outcome::error(format!(
                    "lineRule must be auto|exact|atLeast, not {other:?}"
                )))
            }
        },
        None => None,
    };
    let clear = match command.get("clear") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => {
            let mut names = Vec::new();
            for item in items {
                let name = item.as_str().ok_or_else(|| {
                    Outcome::error("format_paragraph: clear entries must be strings")
                })?;
                if !PARA_PROPERTY_NAMES.contains(&name) {
                    return Err(Outcome::error(format!(
                        "format_paragraph: clear entry {name:?} is not a format_paragraph property (allowed: {})",
                        PARA_PROPERTY_NAMES.join(", ")
                    )));
                }
                names.push(name.to_string());
            }
            names
        }
        Some(_) => {
            return Err(Outcome::error(
                "format_paragraph: clear must be an array of property names",
            ))
        }
    };
    let any = style.is_some()
        || alignment.is_some()
        || indent_left.is_some()
        || indent_right.is_some()
        || space_before.is_some()
        || space_after.is_some()
        || line.is_some()
        || line_rule.is_some()
        || !clear.is_empty();
    if !any {
        return Err(Outcome::error(
            "format_paragraph requires at least one of style, alignment, indent_left, indent_right, space_before, space_after, line_spacing, clear",
        ));
    }
    Ok(ParaFormat {
        style,
        alignment,
        indent_left,
        indent_right,
        space_before,
        space_after,
        line,
        line_rule,
        clear,
    })
}

/// The modeled live paragraph-property state (pPr children excluding any
/// w:pPrChange).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct PPrState {
    style: Option<String>,
    jc: Option<String>,
    ind_first_line_chars: Option<i64>,
    ind_hanging_chars: Option<i64>,
    ind_first_line: Option<i64>,
    ind_hanging: Option<i64>,
    ind_left: Option<i64>,
    ind_right: Option<i64>,
    space_before: Option<i64>,
    space_after: Option<i64>,
    line: Option<i64>,
    line_rule: Option<String>,
}

fn ppr_state(xml: &DocxXml, paragraph: NodeId) -> PPrState {
    let mut state = PPrState::default();
    let Some(ppr) = xml.child_w(paragraph, "pPr") else {
        return state;
    };
    if let Some(style) = xml.child_w(ppr, "pStyle") {
        state.style = xml.attr(style, "val").map(str::to_string);
    }
    if let Some(jc) = xml.child_w(ppr, "jc") {
        state.jc = xml.attr(jc, "val").map(str::to_string);
    }
    if let Some(ind) = xml.child_w(ppr, "ind") {
        state.ind_first_line_chars = xml.attr(ind, "firstLineChars").and_then(|v| v.parse().ok());
        state.ind_hanging_chars = xml.attr(ind, "hangingChars").and_then(|v| v.parse().ok());
        state.ind_first_line = xml.attr(ind, "firstLine").and_then(|v| v.parse().ok());
        state.ind_hanging = xml.attr(ind, "hanging").and_then(|v| v.parse().ok());
        state.ind_left = xml.attr(ind, "left").and_then(|v| v.parse().ok());
        state.ind_right = xml.attr(ind, "right").and_then(|v| v.parse().ok());
    }
    if let Some(spacing) = xml.child_w(ppr, "spacing") {
        state.space_before = xml.attr(spacing, "before").and_then(|v| v.parse().ok());
        state.space_after = xml.attr(spacing, "after").and_then(|v| v.parse().ok());
        state.line = xml.attr(spacing, "line").and_then(|v| v.parse().ok());
        state.line_rule = xml.attr(spacing, "lineRule").map(str::to_string);
    }
    state
}

/// The target paragraph-property state after applying the request to `current`.
fn para_format_target(current: &PPrState, fmt: &ParaFormat) -> PPrState {
    let mut target = current.clone();
    if let Some(s) = &fmt.style {
        target.style = Some(s.clone());
    }
    if let Some(a) = &fmt.alignment {
        // The API says `justify`; ST_Jc spells justified text `both`.
        target.jc = Some(if a == "justify" { "both".into() } else { a.clone() });
    }
    if let Some(v) = fmt.indent_left {
        target.ind_left = Some(v);
    }
    if let Some(v) = fmt.indent_right {
        target.ind_right = Some(v);
    }
    if let Some(v) = fmt.first_line_indent {
        target.ind_first_line_chars = None;
        target.ind_hanging_chars = None;
        target.ind_first_line = if v >= 0 { Some(v) } else { None };
        target.ind_hanging = if v < 0 { Some(-v) } else { None };
    }
    if let Some(v) = fmt.space_before {
        target.space_before = Some(v);
    }
    if let Some(v) = fmt.space_after {
        target.space_after = Some(v);
    }
    if let Some(v) = fmt.line {
        target.line = Some(v);
    }
    if let Some(r) = &fmt.line_rule {
        target.line_rule = Some(r.clone());
    }
    for name in &fmt.clear {
        match name.as_str() {
            "style" => target.style = None,
            "alignment" => target.jc = None,
            "indent_left" => target.ind_left = None,
            "indent_right" => target.ind_right = None,
            "first_line_indent" => { target.ind_first_line = None; target.ind_hanging = None; target.ind_first_line_chars = None; target.ind_hanging_chars = None; },
            "space_before" => target.space_before = None,
            "space_after" => target.space_after = None,
            "line_spacing" => {
                target.line = None;
                target.line_rule = None;
            }
            _ => {}
        }
    }
    target
}

fn remove_attr_local(xml: &mut DocxXml, node: NodeId, local: &str) {
    let suffix = format!(":{local}");
    let names: Vec<String> = xml
        .doc
        .attributes(node)
        .iter()
        .filter(|a| a.name == local || a.name.ends_with(&suffix))
        .map(|a| a.name.clone())
        .collect();
    for name in names {
        xml.doc.remove_attribute(node, &name);
    }
}

fn remove_ppr_child(xml: &mut DocxXml, ppr: NodeId, local: &str) {
    if let Some(node) = xml.child_w(ppr, local) {
        xml.doc.remove_node(node);
    }
}

fn remove_empty_ppr_child(xml: &mut DocxXml, ppr: NodeId, local: &str) {
    if let Some(node) = xml.child_w(ppr, local) {
        if xml.doc.attributes(node).is_empty() {
            xml.doc.remove_node(node);
        }
    }
}

fn set_or_clear_attr(xml: &mut DocxXml, node: NodeId, local: &str, value: Option<i64>) {
    match value {
        Some(v) => set_attr(xml, node, &format!("w:{local}"), &v.to_string()),
        None => remove_attr_local(xml, node, local),
    }
}

/// Rewrite the paragraph's modeled pPr children to `target` (schema order is
/// maintained by the shared pPr helpers). The caller attaches the pPrChange.
fn apply_para_format(xml: &mut DocxXml, ppr: NodeId, current: &PPrState, target: &PPrState) {
    if current.style != target.style {
        match &target.style {
            Some(style) => set_child_val(xml, ppr, "pStyle", style),
            None => remove_ppr_child(xml, ppr, "pStyle"),
        }
    }
    if current.jc != target.jc {
        match &target.jc {
            Some(jc) => set_child_val(xml, ppr, "jc", jc),
            None => remove_ppr_child(xml, ppr, "jc"),
        }
    }
    if current.ind_first_line != target.ind_first_line || current.ind_hanging != target.ind_hanging || current.ind_first_line_chars != target.ind_first_line_chars || current.ind_hanging_chars != target.ind_hanging_chars {
        let ind = ensure_ppr_child(xml, ppr, "ind");
        set_or_clear_attr(xml, ind, "firstLineChars", target.ind_first_line_chars);
        set_or_clear_attr(xml, ind, "hangingChars", target.ind_hanging_chars);
        set_or_clear_attr(xml, ind, "firstLine", target.ind_first_line);
        set_or_clear_attr(xml, ind, "hanging", target.ind_hanging);
        remove_empty_ppr_child(xml, ppr, "ind");
    }
    if current.ind_left != target.ind_left || current.ind_right != target.ind_right {
        let ind = ensure_ppr_child(xml, ppr, "ind");
        set_or_clear_attr(xml, ind, "left", target.ind_left);
        set_or_clear_attr(xml, ind, "right", target.ind_right);
        remove_empty_ppr_child(xml, ppr, "ind");
    }
    if current.space_before != target.space_before
        || current.space_after != target.space_after
        || current.line != target.line
        || current.line_rule != target.line_rule
    {
        let spacing = ensure_ppr_child(xml, ppr, "spacing");
        set_or_clear_attr(xml, spacing, "before", target.space_before);
        set_or_clear_attr(xml, spacing, "after", target.space_after);
        set_or_clear_attr(xml, spacing, "line", target.line);
        match &target.line_rule {
            Some(rule) => set_attr(xml, spacing, "w:lineRule", rule),
            None => remove_attr_local(xml, spacing, "lineRule"),
        }
        remove_empty_ppr_child(xml, ppr, "spacing");
    }
}

/// `format_paragraph`: apply paragraph properties without changing content or
/// identity. Always tracked: a `w:pPrChange` with the complete prior
/// paragraph-property snapshot is recorded. A pending `w:pPrChange` on the
/// target paragraph is incompatible and blocks. `style` must be an existing
/// declared paragraph style.
pub struct FormatParagraphArgs {
    pub at: String,
    pub style: Option<String>,
    pub alignment: Option<String>,
    pub indent_left: Option<i64>,
    pub indent_right: Option<i64>,
    pub first_line_indent: Option<i64>,
    pub space_before: Option<i64>,
    pub space_after: Option<i64>,
    pub line: Option<i64>,
    pub line_rule: Option<String>,
    pub clear: Vec<String>,
    pub author: String,
}

pub fn format_paragraph_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    args: &FormatParagraphArgs,
    ctx: &ExecCtx,
    source_hash: Option<[u8; 32]>,
) -> Result<(String, Value), Outcome> {
    let at = &args.at;
    if !is_valid_para_id(&at.to_uppercase()) {
        return Err(Outcome::error(format!(
            "at must be a valid w14:paraId (eight hexadecimal digits with 00000000 < value < 80000000): {at:?}"
        )));
    }
    let fmt = ParaFormat {
        style: args.style.clone(),
        alignment: args.alignment.clone(),
        indent_left: args.indent_left,
        indent_right: args.indent_right,
        first_line_indent: args.first_line_indent,
        space_before: args.space_before,
        space_after: args.space_after,
        line: args.line,
        line_rule: args.line_rule.clone(),
        clear: args.clear.clone(),
    };
    if let Some(style) = &fmt.style {
        if !paragraph_style_exists(package, style) {
            return Err(blocked_at(
                &at.to_uppercase(),
                format!("style {style:?} is not a declared paragraph style — format_paragraph requires an existing paragraph style"),
            ));
        }
    }
    let author = &args.author;

    let paragraph = resolve_paragraph_by_para_id(xml, package, source_hash, at)?;
    let address = at.to_uppercase();
    if xml
        .child_w(paragraph, "pPr")
        .is_some_and(|ppr| xml.child_w(ppr, "pPrChange").is_some())
    {
        return Err(blocked_at(
            &address,
            "the paragraph already has a pending format change (w:pPrChange) — incompatible with a new change; accept or reject it before formatting again",
        ));
    }

    let current = ppr_state(xml, paragraph);
    let target = para_format_target(&current, &fmt);
    if target == current {
        return Ok((
            format!("paragraph {address} formatting already matches (no change recorded)"),
            json!({ "paraId": address, "changed": false, "tracked": true }),
        ));
    }

    let snapshot = xml.doc.create_element("w:pPr");
    if let Some(ppr) = xml.child_w(paragraph, "pPr") {
        for child in xml.doc.children(ppr).collect::<Vec<_>>() {
            // The snapshot is CT_PPrBase: no rPr, sectPr or pPrChange.
            if ["rPr", "sectPr", "pPrChange"]
                .iter()
                .any(|name| xml.is_w(child, name))
            {
                continue;
            }
            let clone = xml.doc.clone_node(child, true);
            xml.doc.append_child(snapshot, clone);
        }
    }
    let ppr = ensure_ppr(xml, paragraph);
    apply_para_format(xml, ppr, &current, &target);
    let id = xml.next_revision_id();
    let change = xml.doc.create_element("w:pPrChange");
    xml.doc.set_attribute(change, "w:id", &id.to_string());
    xml.doc.set_attribute(change, "w:author", &author);
    xml.doc.set_attribute(change, "w:date", &ctx.now);
    xml.doc.append_child(change, snapshot);
    xml.doc.append_child(ppr, change);

    Ok((
        format!("formatted paragraph {address}"),
        json!({ "paraId": address, "changed": true, "tracked": true }),
    ))
}

#[cfg(any())]
pub fn format_paragraph(
    package: &mut Package,
    xml: &mut DocxXml,
    command: &Value,
    ctx: &ExecCtx,
    source_hash: Option<[u8; 32]>,
) -> Result<(String, Value), Outcome> {
    let at = required_string(command, "at")?.to_owned();
    let fmt = parse_para_format(command)?;
    format_paragraph_typed(
        package,
        xml,
        &FormatParagraphArgs {
            at,
            style: fmt.style,
            alignment: fmt.alignment,
            indent_left: fmt.indent_left,
            indent_right: fmt.indent_right,
            space_before: fmt.space_before,
            space_after: fmt.space_after,
            line: fmt.line,
            line_rule: fmt.line_rule,
            clear: fmt.clear,
            author: command
                .get("author")
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_AUTHOR)
                .to_owned(),
        },
        ctx,
        source_hash,
    )
}

/// Seed material for an inserted paragraph's paraId: the exact inspected
/// package hash, the canonical plan hash, the operation index, and the
/// inserted-item index (one per op). All four are fixed across preview and
/// commit, so the allocated address is identical in both runs.
#[cfg(any())]
fn insert_seed_material(command: &Value, source_hash: Option<[u8; 32]>) -> Vec<u8> {
    let mut material = Vec::with_capacity(96);
    match command
        .get("sourceHash")
        .and_then(Value::as_str)
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        Some(hex_str) => material.extend_from_slice(hex_str.as_bytes()),
        None => match source_hash {
            Some(hash) => material.extend_from_slice(&hash),
            None => material.extend_from_slice(&[0u8; 32]),
        },
    }
    material.push(b'|');
    if let Some(seed) = command.get("batchSeed").and_then(Value::as_str) {
        material.extend_from_slice(seed.as_bytes());
    }
    material.push(b'|');
    let op_index = command.get("opIndex").and_then(Value::as_u64).unwrap_or(0);
    material.extend_from_slice(&op_index.to_le_bytes());
    material.push(b'|');
    material.extend_from_slice(&0u64.to_le_bytes());
    material
}

/// Allocate an unused `w14:paraId` for an inserted paragraph, reserving every
/// valid id already present in the part first. Allocation failure after
/// exhausting the valid id space is a hard error.
#[cfg(any())]
fn allocate_insert_para_id(
    xml: &DocxXml,
    command: &Value,
    source_hash: Option<[u8; 32]>,
) -> Result<String, String> {
    let mut allocator = ParaIdAllocator::new();
    for node in xml.all_paragraphs() {
        if let Some(value) = xml.para_id(node).filter(|v| is_valid_para_id(v)) {
            allocator.reserve(u32::from_str_radix(value, 16).unwrap_or(0));
        }
    }
    let seed = insert_seed_material(command, source_hash);
    allocator.allocate(&seed).map(format_para_id)
}

fn allocate_insert_para_id_typed(
    xml: &DocxXml,
    source_hash: Option<[u8; 32]>,
    plan_seed: &[u8; 32],
    op_index: usize,
) -> Result<String, String> {
    let mut allocator = ParaIdAllocator::new();
    for node in xml.all_paragraphs() {
        if let Some(value) = xml.para_id(node).filter(|v| is_valid_para_id(v)) {
            allocator.reserve(u32::from_str_radix(value, 16).unwrap_or(0));
        }
    }
    let mut seed = Vec::with_capacity(96);
    seed.extend_from_slice(&source_hash.unwrap_or([0; 32]));
    seed.push(b'|');
    seed.extend_from_slice(plan_seed);
    seed.push(b'|');
    seed.extend_from_slice(&(op_index as u64).to_le_bytes());
    seed.push(b'|');
    seed.extend_from_slice(&0u64.to_le_bytes());
    allocator.allocate(&seed).map(format_para_id)
}

/// Unused `w14:paraId`s for the cell paragraphs of a table inserted by
/// operation `op_index`, one per cell in row-major order. Cell `n` (from 1)
/// seeds probe slot `n`; slot 0 is the one `insert_paragraph` uses.
pub(crate) fn allocate_table_cell_para_ids(
    xml: &DocxXml,
    source_hash: Option<[u8; 32]>,
    plan_seed: &[u8; 32],
    op_index: usize,
    count: usize,
) -> Result<Vec<String>, String> {
    let mut allocator = ParaIdAllocator::new();
    for node in xml.all_paragraphs() {
        if let Some(value) = xml.para_id(node).filter(|v| is_valid_para_id(v)) {
            allocator.reserve(u32::from_str_radix(value, 16).unwrap_or(0));
        }
    }
    let mut prefix = Vec::with_capacity(96);
    prefix.extend_from_slice(&source_hash.unwrap_or([0; 32]));
    prefix.push(b'|');
    prefix.extend_from_slice(plan_seed);
    prefix.push(b'|');
    prefix.extend_from_slice(&(op_index as u64).to_le_bytes());
    prefix.push(b'|');
    (1..=count as u64)
        .map(|slot| {
            let mut seed = prefix.clone();
            seed.extend_from_slice(&slot.to_le_bytes());
            allocator.allocate(&seed).map(format_para_id)
        })
        .collect()
}

/// Fill an empty paragraph with `with`, the inline content `insert_paragraph`
/// accepts, as direct (untracked) runs.
pub(crate) fn fill_paragraph_direct(
    package: &mut Package,
    xml: &mut DocxXml,
    paragraph: NodeId,
    with: &str,
) -> Result<(), Outcome> {
    let (pieces, link_href) = parse_with(with)?;
    insert_plain_pieces(xml, package, paragraph, None, &pieces, link_href.as_deref())
        .map_err(Outcome::error)
}

/// `insert_paragraph`: insert one paragraph before or after an existing
/// paragraph, assigning an unused `w14:paraId` in the same part. With
/// tracking enabled the engine records both the new content and the new
/// paragraph boundary (w:pPr/w:rPr/w:ins). The address is fixed during
/// preview and persisted identically at commit.
pub struct InsertParagraphArgs {
    pub at_para: Option<String>,
    pub at_table: Option<u32>,
    pub insert_before: bool,
    pub with: String,
    pub style: Option<String>,
    pub tracked: bool,
    pub author: String,
    pub plan_seed: [u8; 32],
    pub op_index: usize,
}

pub fn insert_paragraph_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    args: &InsertParagraphArgs,
    ctx: &ExecCtx,
    source_hash: Option<[u8; 32]>,
) -> Result<(String, Value), Outcome> {
    let insert_before = args.insert_before;
    let with = &args.with;
    let style = &args.style;
    let tracked = args.tracked;
    let author = &args.author;

    let (anchor, address) = match (&args.at_para, &args.at_table) {
        (Some(at), None) => {
            if !is_valid_para_id(&at.to_uppercase()) {
                return Err(Outcome::error(format!(
                    "at must be a valid w14:paraId (eight hexadecimal digits with 00000000 < value < 80000000): {at:?}"
                )));
            }
            let anchor = resolve_paragraph_by_para_id(xml, package, source_hash, at)?;
            (anchor, at.to_uppercase())
        }
        (None, Some(n)) => {
            let targets = crate::html::render::body_table_insert_targets(xml);
            let anchor = targets.get(*n as usize - 1).copied().ok_or_else(|| {
                Outcome::blocked(format!(
                    "no body-level table {n} (document has {})",
                    targets.len()
                ))
            })?;
            (anchor, format!("table:{n}"))
        }
        _ => {
            return Err(Outcome::error(
                "insert_paragraph requires exactly one paragraph or table anchor",
            ))
        }
    };

    let (pieces, link_href) = parse_with(with)?;

    if let Some(style_id) = &style {
        if !paragraph_style_exists(package, style_id) {
            return Err(blocked_at(
                &address,
                format!("style {style_id:?} is not a declared paragraph style — insert_paragraph requires an existing paragraph style"),
            ));
        }
    }

    let new_p = xml.doc.create_element("w:p");
    if insert_before {
        xml.doc.insert_before(anchor, new_p);
    } else {
        xml.doc.insert_after(anchor, new_p);
    }
    let new_id = allocate_insert_para_id_typed(xml, source_hash, &args.plan_seed, args.op_index)
        .map_err(Outcome::error)?;
    xml.doc.set_attribute(new_p, "w14:paraId", &new_id);
    xml.ensure_w14_declared();

    if let Some(style_id) = &style {
        let ppr = ensure_ppr(xml, new_p);
        set_child_val(xml, ppr, "pStyle", style_id);
    }
    if tracked {
        let rev_id = xml.next_revision_id();
        if !pieces.is_empty() {
            insert_ins_pieces_at(xml, new_p, 0, &pieces, rev_id, &author, &ctx.now, false)
                .map_err(Outcome::error)?;
            if let Some(href) = link_href {
                let rid = crate::html::chrome::ensure_external_hyperlink(package, &href)
                    .map_err(Outcome::error)?;
                crate::html::chrome::wrap_revision_in_hyperlink(xml, new_p, rev_id, &rid)
                    .map_err(Outcome::error)?;
            }
        }
        xml.add_paragraph_mark_marker(new_p, "ins", rev_id, &author, &ctx.now);
    } else {
        insert_plain_pieces(xml, package, new_p, None, &pieces, link_href.as_deref())
            .map_err(Outcome::error)?;
    }

    let anchor_kind = if args.at_table.is_some() {
        "table"
    } else {
        "paragraph"
    };
    let summary = format!(
        "inserted paragraph {new_id} {} {anchor_kind} {address} ({})",
        if insert_before { "before" } else { "after" },
        if tracked { "tracked" } else { "direct" }
    );
    let result = json!({
        "paraId": new_id,
        "anchor": address,
        "before": insert_before,
        "tracked": tracked,
    });
    Ok((summary, result))
}

#[cfg(any())]
pub fn insert_paragraph(
    package: &mut Package,
    xml: &mut DocxXml,
    command: &Value,
    ctx: &ExecCtx,
    source_hash: Option<[u8; 32]>,
) -> Result<(String, Value), Outcome> {
    let at = required_string(command, "at")?.to_owned();
    let with = match command.get("with") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(_) => return Err(Outcome::error("with must be a string (may be empty)")),
    };
    let style = optional_string(command, "style")?.map(str::to_owned);
    let plan_seed = command
        .get("batchSeed")
        .and_then(Value::as_str)
        .and_then(|s| hex::decode(s).ok())
        .and_then(|v| v.try_into().ok())
        .unwrap_or([0; 32]);
    insert_paragraph_typed(
        package,
        xml,
        &InsertParagraphArgs {
            at_para: Some(at),
            at_table: None,
            insert_before: bool_field(command, "insertBefore", false),
            with,
            style,
            tracked: bool_field(command, "tracked", true),
            author: command
                .get("author")
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_AUTHOR)
                .to_owned(),
            plan_seed,
            op_index: command.get("opIndex").and_then(Value::as_u64).unwrap_or(0) as usize,
        },
        ctx,
        source_hash,
    )
}

/// Whether a paragraph is a direct child (or AlternateContent-fallback child)
/// of the body story — the only paragraphs `delete_paragraphs` addresses.
/// Paragraphs inside tables, textboxes, structured document tags, and other
/// embedded scopes are outside the typed mutation surface.
fn is_top_level_body_paragraph(xml: &DocxXml, node: NodeId) -> bool {
    let Some(body) = xml.body() else {
        return false;
    };
    let mut cursor = Some(node);
    while let Some(current) = cursor {
        let Some(parent) = xml.doc.parent(current) else {
            return false;
        };
        if parent == body {
            return true;
        }
        if xml.is_w(parent, "tbl")
            || xml.is_w(parent, "tc")
            || xml.is_w(parent, "txbxContent")
            || xml.is_w(parent, "sdt")
            || xml.is_w(parent, "drawing")
            || xml.is_w(parent, "object")
        {
            return false;
        }
        cursor = Some(parent);
    }
    false
}

/// Body-level milestone elements that can sit between paragraphs and are not
/// blocks themselves.
const BODY_MILESTONES: &[&str] = &[
    "bookmarkStart",
    "bookmarkEnd",
    "commentRangeStart",
    "commentRangeEnd",
    "commentReference",
    "permStart",
    "permEnd",
    "proofErr",
    "moveFromRangeStart",
    "moveFromRangeEnd",
    "moveToRangeStart",
    "moveToRangeEnd",
];

/// Whether the block immediately following `paragraph` is a mergeable
/// paragraph (its terminating break can be marked deleted so accepting the
/// deletion joins the two paragraphs). Tables, section properties, and other
/// non-paragraph blocks are not mergeable; milestone elements are skipped.
fn next_block_is_mergeable_paragraph(xml: &DocxXml, paragraph: NodeId) -> bool {
    let mut cursor = xml.doc.next_sibling(paragraph);
    while let Some(node) = cursor {
        if !xml.doc.is_element(node) {
            cursor = xml.doc.next_sibling(node);
            continue;
        }
        if xml.is_w(node, "p") {
            return true;
        }
        if BODY_MILESTONES.iter().any(|local| xml.is_w(node, local)) {
            cursor = xml.doc.next_sibling(node);
            continue;
        }
        return false;
    }
    false
}

/// Whether `paragraph` is wholly `author`'s pending insertion (its mark and
/// all its text) with nothing else to preserve: no section break, no
/// milestones, and another paragraph beside it.
fn is_own_inserted_paragraph(xml: &DocxXml, paragraph: NodeId, author: &str) -> bool {
    const PLAIN_RUN_CHILDREN: [&str; 4] = ["rPr", "t", "tab", "br"];
    let own = |node: NodeId| xml.is_w(node, "ins") && xml.attr(node, "author") == Some(author);

    let Some(mark) = xml.paragraph_mark_ins(paragraph) else {
        return false;
    };
    if !own(mark) {
        return false;
    }
    let ppr = xml.child_w(paragraph, "pPr");
    if ppr.is_some_and(|ppr| xml.child_w(ppr, "sectPr").is_some()) {
        return false;
    }
    let parent = xml.doc.parent(paragraph);
    let has_sibling = parent.is_some_and(|parent| {
        xml.doc.children(parent).any(|n| n != paragraph && xml.is_w(n, "p"))
    });
    if !has_sibling {
        return false;
    }

    // Content: own w:ins wrappers of plain runs only.
    xml.doc.children(paragraph).filter(|&n| xml.doc.is_element(n)).all(|child| {
        if Some(child) == ppr {
            return true;
        }
        own(child)
            && xml.doc.children(child).filter(|&n| xml.doc.is_element(n)).all(|run| {
                xml.is_w(run, "r")
                    && xml
                        .doc
                        .children(run)
                        .filter(|&n| xml.doc.is_element(n))
                        .all(|leaf| PLAIN_RUN_CHILDREN.iter().any(|name| xml.is_w(leaf, name)))
            })
    })
}

/// Move one content unit (run, hyperlink, or smartTag) under `parent` into a
/// fresh paragraph-local `<w:del>`, renaming its `w:t` leaves to `w:delText`
/// and `w:instrText` to `w:delInstrText`.
fn move_content_into_del(
    xml: &mut DocxXml,
    node: NodeId,
    id: u64,
    author: &str,
    date: &str,
) -> Result<(), String> {
    // A tracked deletion holds runs, never a hyperlink (CT_RunTrackChange);
    // the deletion goes inside the hyperlink instead. Bidi wrappers may hold
    // hyperlinks, so their runs are deleted inside them too.
    if xml.is_w(node, "hyperlink") || xml.is_w(node, "dir") || xml.is_w(node, "bdo") {
        let inner: Vec<NodeId> = xml.doc.children(node).collect();
        for child in inner {
            if xml.is_rev_del(child) {
                continue;
            }
            if is_run_content(xml, child) {
                move_content_into_del(xml, child, id, author, date)?;
            }
        }
        return Ok(());
    }
    let del = xml.doc.create_element("w:del");
    xml.doc.set_attribute(del, "w:id", &id.to_string());
    xml.doc.set_attribute(del, "w:author", author);
    xml.doc.set_attribute(del, "w:date", date);
    xml.doc.insert_before(node, del);
    xml.doc.detach(node);
    xml.doc.append_child(del, node);
    crate::segments::mark_deleted_leaves(xml, del, true);
    Ok(())
}

/// Mark one paragraph's current live content as deleted, paragraph-locally:
/// existing `w:del` content is preserved untouched, live content inside an
/// existing `w:ins` receives a nested `w:del` (Word's edit-within-edit), and
/// plain runs/hyperlinks/smartTags are wrapped in a fresh `w:del`. A
/// `w:del` never wraps multiple `w:p` elements.
fn wrap_live_content_in_del(
    xml: &mut DocxXml,
    paragraph: NodeId,
    id: u64,
    author: &str,
    date: &str,
) -> Result<(), String> {
    let children: Vec<NodeId> = xml.doc.children(paragraph).collect();
    for child in children {
        if xml.is_w(child, "pPr") {
            continue;
        }
        if xml.is_rev_del(child) {
            // Preserve existing deletion history: never wrap deleted content
            // again.
            continue;
        }
        if xml.is_w(child, "sdt") {
            return Err(
                "the paragraph contains a structured document tag (w:sdt) — delete_paragraphs requires unambiguous paragraph content; remove the tag first"
                    .into(),
            );
        }
        if xml.is_rev_ins(child) {
            let inner: Vec<NodeId> = xml.doc.children(child).collect();
            for run in inner {
                if xml.is_rev_del(run) {
                    continue;
                }
                if is_run_content(xml, run) {
                    move_content_into_del(xml, run, id, author, date)?;
                }
            }
            continue;
        }
        if is_run_content(xml, child) {
            move_content_into_del(xml, child, id, author, date)?;
        }
        // Milestones (bookmarks, comment ranges, proofErr) are structural
        // markers, not content — they stay in place.
    }
    Ok(())
}

/// `delete_paragraphs`: mark one or more top-level body paragraphs as deleted
/// in one logical operation. Targets resolve against the in-memory document
/// after prior operations, then sort by current position and deduplicate.
/// Existing deletion history is preserved; each paragraph's live content is
/// wrapped paragraph-locally, and a selected paragraph's terminating break is
/// marked deleted when its immediately following block is a mergeable
/// paragraph. A selected final paragraph has no following break to delete.
pub struct DeleteParagraphsArgs {
    pub at: Vec<String>,
    pub author: String,
}

pub fn delete_paragraphs_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    args: &DeleteParagraphsArgs,
    ctx: &ExecCtx,
    source_hash: Option<[u8; 32]>,
) -> Result<(String, Value), Outcome> {
    if args.at.is_empty() {
        return Err(Outcome::error("delete_paragraphs at must not be empty"));
    }
    let author = &args.author;

    let mut targets: Vec<(NodeId, String)> = Vec::with_capacity(args.at.len());
    for id in &args.at {
        let address = id.to_uppercase();
        if !is_valid_para_id(&address) {
            return Err(Outcome::error(format!(
                "at entries must be valid w14:paraId values: {id:?}"
            )));
        }
        let node = resolve_paragraph_by_para_id(xml, package, source_hash, &address)?;
        if !is_top_level_body_paragraph(xml, node) {
            return Err(blocked_at(
                &address,
                "the paragraph is not a top-level body paragraph in word/document.xml — delete_paragraphs targets top-level body paragraphs; table cells, textboxes, and other embedded scopes are outside the typed mutation surface",
            ));
        }
        targets.push((node, address));
    }
    let order: std::collections::HashMap<NodeId, usize> = xml
        .all_paragraphs()
        .into_iter()
        .enumerate()
        .map(|(i, node)| (node, i))
        .collect();
    targets.sort_by_key(|(node, _)| order.get(node).copied().unwrap_or(usize::MAX));
    targets.dedup_by(|a, b| a.0 == b.0);

    let mut break_marks = 0usize;
    let mut removed = 0usize;
    for (node, address) in &targets {
        // The author's own pending paragraph goes outright, as in Word.
        if is_own_inserted_paragraph(xml, *node, author) {
            xml.doc.remove_node(*node);
            removed += 1;
            continue;
        }

        let rev_id = xml.next_revision_id();
        wrap_live_content_in_del(xml, *node, rev_id, &author, &ctx.now)
            .map_err(|e| blocked_at(address, e))?;
        // A paragraph mark that is already a tracked deletion stays as it is.
        if next_block_is_mergeable_paragraph(xml, *node) && xml.paragraph_mark_del(*node).is_none() {
            xml.add_paragraph_mark_marker(*node, "del", rev_id, &author, &ctx.now);
            break_marks += 1;
        }
    }

    let ids: Vec<String> = targets.iter().map(|(_, id)| id.clone()).collect();
    let marked = ids.len() - removed;
    let mut summary = format!(
        "marked {} paragraph{} for tracked deletion ({} break{})",
        marked,
        if marked == 1 { "" } else { "s" },
        break_marks,
        if break_marks == 1 { "" } else { "s" }
    );
    if removed > 0 {
        summary.push_str(&format!(
            "; removed {removed} own pending paragraph{}",
            if removed == 1 { "" } else { "s" }
        ));
    }
    let result = json!({
        "paragraphs": ids,
        "breaks": break_marks,
        "revisions": marked,
        "tracked": true,
    });
    Ok((summary, result))
}

#[cfg(any())]
pub fn delete_paragraphs(
    package: &mut Package,
    xml: &mut DocxXml,
    command: &Value,
    ctx: &ExecCtx,
    source_hash: Option<[u8; 32]>,
) -> Result<(String, Value), Outcome> {
    let at = command
        .get("at")
        .and_then(Value::as_array)
        .ok_or_else(|| Outcome::error("delete_paragraphs requires an at array of paragraph IDs"))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| Outcome::error("delete_paragraphs at entries must be strings"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    delete_paragraphs_typed(
        package,
        xml,
        &DeleteParagraphsArgs {
            at,
            author: command
                .get("author")
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_AUTHOR)
                .to_owned(),
        },
        ctx,
        source_hash,
    )
}

// ---------------------------------------------------------------------------
// Phase 4: typed comments and revisions.
// ---------------------------------------------------------------------------

/// `comment_add`: anchor a comment to one paragraph, either the whole current
/// live content (when `select` is omitted) or one unambiguous plain-text span
/// (with the common occurrence rules). The paragraph is addressed by its
/// `w14:paraId` (provisional repairs resolve in memory first); comment IDs use
/// the dedicated `commentId` field and are never inferred from `at`. Anchoring
/// crosses a structure that cannot be safely preserved (fields, drawings,
/// notes, existing comment milestones) blocks instead of guessing.
pub struct CommentAddArgs {
    pub at: String,
    pub select: Option<String>,
    pub occurrence: Option<usize>,
    pub text: String,
    pub author: String,
}

pub fn comment_add_typed(
    package: &mut Package,
    xml: &mut DocxXml,
    args: &CommentAddArgs,
    ctx: &ExecCtx,
    source_hash: Option<[u8; 32]>,
) -> Result<(String, Value), Outcome> {
    let at = &args.at;
    if !is_valid_para_id(&at.to_uppercase()) {
        return Err(Outcome::error(format!(
            "at must be a valid w14:paraId (eight hexadecimal digits with 00000000 < value < 80000000): {at:?}"
        )));
    }
    let text = &args.text;
    let author = &args.author;
    let select = &args.select;
    if select.as_deref().is_some_and(|s| s.contains('<')) {
        return Err(blocked_at(
            &at.to_uppercase(),
            "select must be plain text without HTML markup — select addresses the paragraph's visible text, never markup",
        ));
    }
    let occurrence = args.occurrence;

    let paragraph = resolve_paragraph_by_para_id(xml, package, source_hash, at)?;
    let address = at.to_uppercase();
    let segments = build_segments(xml, paragraph);
    let total = all_text_len(&segments);

    let (start, end, matched) = match (&select, occurrence) {
        (Some(select), occurrence) => {
            let (start, end, _current, matched) =
                select_in_paragraph(xml, paragraph, select, occurrence)
                    .map_err(|outcome| blocked_at(&address, outcome.summary))?;
            if start >= end {
                return Err(blocked_at(&address, "the selection covers no text"));
            }
            // Anchoring markers across a field instruction, opaque drawing,
            // note reference, or an existing comment milestone would corrupt
            // or reorder the structure — block instead of guessing.
            split_boundary(xml, paragraph, end);
            split_boundary(xml, paragraph, start);
            let segments = build_segments(xml, paragraph);
            let covered: Vec<&crate::segments::Segment> = segments
                .iter()
                .filter(|s| s.start >= start && s.end <= end && s.start < s.end)
                .collect();
            if covered.is_empty() {
                return Err(blocked_at(&address, "the selection covers no text"));
            }
            let first_anchor = covered[0].wrapper.unwrap_or(covered[0].run);
            let last_anchor = covered[covered.len() - 1]
                .wrapper
                .unwrap_or(covered[covered.len() - 1].run);
            if let Some(label) = scan_blockers_between(xml, first_anchor, last_anchor) {
                return Err(blocked_at(
                    &address,
                    format!(
                        "the selection crosses a {label} that cannot be safely preserved — update, resolve, or remove it before commenting on this content"
                    ),
                ));
            }
            (start, end, matched)
        }
        (None, None) => (0, total, 0),
        (None, Some(_)) => {
            return Err(blocked_at(
                &address,
                "occurrence requires select — a whole-paragraph anchor has nothing to count",
            ))
        }
    };

    let comment_id = super::comments::anchor_comment_at(
        package, xml, paragraph, start, end, &author, &ctx.now, &text,
    )
    .map_err(|outcome| blocked_at(&address, outcome.summary))?;

    let summary = match (&select, occurrence) {
        (Some(_), Some(n)) => format!(
            "comment {comment_id} anchored to occurrence {n} of {matched} in paragraph {address}"
        ),
        (Some(_), None) => {
            format!("comment {comment_id} anchored to 1 of 1 match in paragraph {address}")
        }
        (None, _) => format!("comment {comment_id} anchored to paragraph {address}"),
    };
    let result = json!({
        "paraId": address,
        "commentId": comment_id,
        "matched": matched,
        "occurrence": occurrence,
        "wholeParagraph": select.is_none(),
    });
    Ok((summary, result))
}

#[cfg(any())]
pub fn comment_add(
    package: &mut Package,
    xml: &mut DocxXml,
    command: &Value,
    ctx: &ExecCtx,
    source_hash: Option<[u8; 32]>,
) -> Result<(String, Value), Outcome> {
    let at = required_string(command, "at")?.to_owned();
    let text = command
        .get("text")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Outcome::error("text is required"))?
        .to_owned();
    let select = match command.get("select") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Value::String(_)) => return Err(Outcome::error("select must be a non-empty string")),
        Some(_) => return Err(Outcome::error("select must be a string")),
    };
    let occurrence = command
        .get("occurrence")
        .and_then(Value::as_u64)
        .map(|n| n as usize);
    comment_add_typed(
        package,
        xml,
        &CommentAddArgs {
            at,
            select,
            occurrence,
            text,
            author: command
                .get("author")
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_AUTHOR)
                .to_owned(),
        },
        ctx,
        source_hash,
    )
}

/// Number of characters in the all-view text of a paragraph's segments.
fn all_text_len(segments: &[crate::segments::Segment]) -> usize {
    segments.iter().map(|s| s.text.chars().count()).sum()
}
