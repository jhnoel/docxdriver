//! Canonical renderer: DOCX → the HTML dialect. Byte-deterministic — fixed
//! attribute order, fixed entity choices, one block element per line, no
//! cosmetic whitespace inside elements — because exact-match edit anchors
//! depend on it.

use std::collections::BTreeMap;

use xmloxide::tree::NodeId;

use crate::document::DocxXml;

use super::ctx::RenderCtx;
use super::{escape_attr, escape_text, BrKind, Fmt, FmtPending};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderView {
    /// Both projections interleaved as revision markup — the editing surface.
    Markup,
    /// Every change accepted (no revision tags).
    Final,
    /// Every change rejected (no revision tags).
    Original,
}

impl RenderView {
    pub fn from_json(value: Option<&serde_json::Value>) -> Result<RenderView, String> {
        match value.and_then(|v| v.as_str()) {
            None | Some("markup") => Ok(RenderView::Markup),
            Some("final") => Ok(RenderView::Final),
            Some("original") => Ok(RenderView::Original),
            Some(other) => Err(format!(
                "unknown view: {other} (expected markup, final, or original)"
            )),
        }
    }
}

/// One top-level block (body-child `w:p` or `w:tbl`) in the rendered string.
/// `start..end` are byte offsets into the html (excluding the `\n` separator);
/// `paragraphs` are every `w:p` inside, in document order — the fragment
/// parser recovers paragraphs in exactly this order.
#[derive(Debug)]
pub struct BlockSpan {
    pub start: usize,
    pub end: usize,
    pub paragraphs: Vec<NodeId>,
}

#[derive(Debug)]
pub struct RenderedBody {
    pub html: String,
    pub blocks: Vec<BlockSpan>,
}

/// Render an explicit list of body blocks (`w:p` / `w:tbl`) in order.
pub fn render_block_list(
    xml: &DocxXml,
    ctx: &RenderCtx,
    view: RenderView,
    children: &[NodeId],
    mut table_number: Option<&mut u32>,
) -> RenderedBody {
    let mut html = String::new();
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < children.len() {
        let node = children[index];
        let mut paragraphs = Vec::new();
        let mut text = String::new();
        if xml.is_w(node, "tbl") {
            render_table(
                xml,
                ctx,
                node,
                view,
                &mut text,
                &mut paragraphs,
                table_number.as_deref_mut(),
            );
            index += 1;
        } else {
            let mut group = vec![node];
            while view != RenderView::Markup
                && break_hidden(xml, *group.last().expect("non-empty group"), view)
                && children
                    .get(index + group.len())
                    .is_some_and(|&next| xml.is_w(next, "p"))
            {
                group.push(children[index + group.len()]);
            }
            index += group.len();
            paragraphs.extend(&group);
            render_paragraph_group(xml, ctx, &group, view, &mut text);
        }
        if !html.is_empty() {
            html.push('\n');
        }
        let start = html.len();
        html.push_str(&text);
        blocks.push(BlockSpan {
            start,
            end: html.len(),
            paragraphs,
        });
    }
    group_lists(ctx, RenderedBody { html, blocks })
}

/// Group numbered paragraph blocks into real nested HTML lists. The enclosing
/// list is one block span carrying every underlying paragraph identity.
fn group_lists(ctx: &RenderCtx, body: RenderedBody) -> RenderedBody {
    let mut html = String::new();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < body.blocks.len() {
        if !html.is_empty() {
            html.push('\n');
        }
        let start = html.len();
        let mut paragraphs = Vec::new();
        if body.blocks[i]
            .paragraphs
            .first()
            .is_some_and(|p| ctx.lists.contains_key(p))
            && !body.html[body.blocks[i].start..body.blocks[i].end].starts_with("<table")
        {
            let begin = i;
            while i < body.blocks.len()
                && body.blocks[i]
                    .paragraphs
                    .first()
                    .is_some_and(|p| ctx.lists.contains_key(p))
                && !body.html[body.blocks[i].start..body.blocks[i].end].starts_with("<table")
            {
                i += 1;
            }
            let mut cursor = begin;
            while cursor < i {
                let level = ctx.lists[&body.blocks[cursor].paragraphs[0]].level;
                render_list(ctx, &body, &mut cursor, i, level, &mut html);
            }
            for b in &body.blocks[begin..i] {
                paragraphs.extend(&b.paragraphs);
            }
        } else {
            let b = &body.blocks[i];
            html.push_str(&body.html[b.start..b.end]);
            paragraphs.extend(&b.paragraphs);
            i += 1;
        }
        blocks.push(BlockSpan {
            start,
            end: html.len(),
            paragraphs,
        });
    }
    RenderedBody { html, blocks }
}
fn render_list(
    ctx: &RenderCtx,
    body: &RenderedBody,
    cursor: &mut usize,
    end: usize,
    level: u32,
    out: &mut String,
) {
    let first = &ctx.lists[&body.blocks[*cursor].paragraphs[0]];
    let tag = if first.format == "bullet" { "ul" } else { "ol" };
    let kind = match first.format.as_str() {
        "lowerLetter" => "a",
        "upperLetter" => "A",
        "lowerRoman" => "i",
        "upperRoman" => "I",
        _ => "1",
    };
    out.push_str(&format!(
        "<{tag} data-docx-list=\"{}\" data-docx-level=\"{level}\"",
        escape_attr(&first.id)
    ));
    if tag == "ol" {
        out.push_str(&format!(" start=\"{}\" type=\"{kind}\"", first.value));
    }
    out.push('>');
    let identity = first.id.clone();
    while *cursor < end {
        let b = &body.blocks[*cursor];
        let info = &ctx.lists[&b.paragraphs[0]];
        if info.level != level || info.id != identity {
            break;
        }
        let marker = ctx
            .markers
            .get(&b.paragraphs[0])
            .map(String::as_str)
            .unwrap_or("");
        out.push_str(&format!(
            "<li data-docx-marker=\"{}\">",
            escape_attr(marker)
        ));
        out.push_str(&body.html[b.start..b.end]);
        *cursor += 1;
        while *cursor < end {
            let next = &ctx.lists[&body.blocks[*cursor].paragraphs[0]];
            if next.level <= level {
                break;
            }
            render_list(ctx, body, cursor, end, next.level, out);
        }
        out.push_str("</li>");
    }
    out.push_str(&format!("</{tag}>"));
}

/// Render the block children (`w:p` / `w:tbl`) of any container — body,
/// header, or footer — with the same dialect rules as the main body.
pub fn render_blocks(
    xml: &DocxXml,
    ctx: &RenderCtx,
    view: RenderView,
    container: NodeId,
) -> RenderedBody {
    let children = effective_block_children(xml, container);
    render_block_list(xml, ctx, view, &children, None)
}

pub(crate) fn effective_block_children(xml: &DocxXml, container: NodeId) -> Vec<NodeId> {
    let mut blocks = Vec::new();
    collect_effective_blocks(xml, container, &mut blocks);
    blocks
}

pub(crate) fn body_table_insert_targets(xml: &DocxXml) -> Vec<NodeId> {
    xml.body()
        .map(|body| {
            effective_block_children(xml, body)
                .into_iter()
                .filter(|&node| xml.is_w(node, "tbl"))
                .collect()
        })
        .unwrap_or_default()
}

/// Effective story children, including transparent content-control/custom XML
/// wrappers, but never descending into a paragraph, table or wrapper properties.
pub(crate) fn effective_story_children(
    xml: &DocxXml,
    container: NodeId,
    names: &[&str],
) -> Vec<NodeId> {
    fn collect(xml: &DocxXml, container: NodeId, names: &[&str], out: &mut Vec<NodeId>) {
        for child in xml.doc.children(container) {
            if names.iter().any(|name| xml.is_w(child, name)) {
                out.push(child);
            } else if xml.is_local(child, "AlternateContent") {
                if let Some(branch) = xml.alternate_content_branch(child) {
                    collect(xml, branch, names, out);
                }
            } else if xml.is_w(child, "sdt") {
                if let Some(content) = xml.child_w(child, "sdtContent") {
                    collect(xml, content, names, out);
                }
            } else if xml.is_w(child, "sdtContent") || xml.is_w(child, "customXml") {
                collect(xml, child, names, out);
            }
        }
    }
    let mut out = Vec::new();
    collect(xml, container, names, &mut out);
    out
}

fn collect_effective_blocks(xml: &DocxXml, container: NodeId, blocks: &mut Vec<NodeId>) {
    blocks.extend(effective_story_children(xml, container, &["p", "tbl"]));
}

/// Whether the paragraph's terminating break is hidden in this view: an
/// inserted break vanishes from the original, a deleted break from the final.
fn break_hidden(xml: &DocxXml, paragraph: NodeId, view: RenderView) -> bool {
    match view {
        RenderView::Markup => false,
        RenderView::Final => xml.paragraph_mark_del(paragraph).is_some(),
        RenderView::Original => xml.paragraph_mark_ins(paragraph).is_some(),
    }
}

/// The dialect tag + class for a paragraph style: Heading1..Heading6 project
/// to h1..h6, anything else keeps the style name as a class.
fn paragraph_tag(xml: &DocxXml, paragraph: NodeId) -> (String, Option<String>) {
    match xml.paragraph_style(paragraph) {
        Some(style) => match style
            .strip_prefix("Heading")
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|n| (1..=6).contains(n))
        {
            Some(level) => (format!("h{level}"), None),
            None => ("p".to_string(), Some(style)),
        },
        None => ("p".to_string(), None),
    }
}

fn render_paragraph_group(
    xml: &DocxXml,
    ctx: &RenderCtx,
    group: &[NodeId],
    view: RenderView,
    out: &mut String,
) {
    let first = group[0];
    let (tag, _) = paragraph_tag(xml, first);
    let class = ctx.styles.paragraph_style(xml, first, view == RenderView::Original);
    out.push('<');
    out.push_str(&tag);
    // Canonical attribute order: id, ord, class, num, break-ins, break-del.
    // id/ord are the engine-owned paragraph address (w14:paraId + story
    // position); ord is informational only and never an address.
    if let Some((id, ord)) = ctx.para_ids.get(&first) {
        out.push_str(&format!(" id=\"{}\"", escape_attr(id)));
        out.push_str(&format!(" data-docx-ord=\"{ord}\""));
    }
    if let Some(class) = &class {
        out.push_str(&format!(" data-docx-style=\"{}\"", escape_attr(class)));
    }
    // num is computed, never authored: the paragraph's rendered list marker.
    if let Some(marker) = ctx.markers.get(&first) {
        if !marker.is_empty() {
            out.push_str(&format!(" data-docx-number=\"{}\"", escape_attr(marker)));
        }
    }
    if view == RenderView::Markup {
        // Canonical attribute order: class, num, break-ins, break-del.
        let last = *group.last().expect("non-empty group");
        if let Some(marker) = xml.paragraph_mark_ins(last) {
            let id = xml.attr(marker, "id").unwrap_or("");
            out.push_str(&format!(" data-docx-break-ins=\"{}\"", escape_attr(id)));
        }
        if let Some(marker) = xml.paragraph_mark_del(last) {
            let id = xml.attr(marker, "id").unwrap_or("");
            out.push_str(&format!(" data-docx-break-del=\"{}\"", escape_attr(id)));
        }
    }
    let inherited_run_css = common_run_css(xml, ctx, group, view);
    let css = super::styles::merge_paragraph_css(
        &ctx.styles
            .paragraph_css(xml, first, view == RenderView::Original),
        &inherited_run_css,
    );
    if !css.is_empty() {
        out.push_str(&format!(" style=\"{}\"", escape_attr(&css)));
    }
    out.push('>');
    let mut emitter = Emitter::new(
        out,
        view,
        field_instructions(xml, group),
        ctx,
        inherited_run_css,
    );
    for &paragraph in group {
        if group.len() > 1 {
            if let Some((id, _)) = ctx.para_ids.get(&paragraph) {
                emitter.out.push_str(&format!(
                    "<span data-docx-paragraph=\"{}\">",
                    escape_attr(id)
                ));
            }
        }
        if group.len() > 1 && emitter.field_depth > 0 {
            emitter.out.push_str(&format!(
                "<span data-docx-field=\"{}\" contenteditable=\"false\">",
                escape_attr(&emitter.field_active_instr)
            ));
        }
        emit_content(xml, ctx, paragraph, view, None, None, &mut emitter);
        if group.len() > 1 {
            emitter.finish();
            if emitter.field_depth > 0 {
                emitter.out.push_str("</span>");
            }
            emitter.out.push_str("</span>");
        }
    }
    emitter.finish();
    // A complex field left open (its end fldChar lives in a later block — a
    // v1 limit) closes at the paragraph boundary so the dialect stays balanced.
    if emitter.field_depth > 0 {
        emitter.field_depth = 0;
        if group.len() == 1 {
            emitter.out.push_str("</span>");
        }
    }
    out.push_str(&format!("</{tag}>"));
}

/// Declarations shared by every run in the paragraph group can live on the
/// paragraph and flow through its inline content. Keep non-inherited or
/// context-sensitive properties on their original runs.
fn common_run_css(
    xml: &DocxXml,
    ctx: &RenderCtx,
    group: &[NodeId],
    view: RenderView,
) -> BTreeMap<String, String> {
    let has_hyperlink = group.iter().any(|&paragraph| {
        xml.doc.descendants(paragraph).any(|node| {
            xml.is_w(node, "hyperlink") && nearest_paragraph(xml, node) == Some(paragraph)
        })
    });
    let mut common: Option<BTreeMap<String, String>> = None;
    for &paragraph in group {
        for run in xml
            .doc
            .descendants(paragraph)
            .filter(|&node| xml.is_w(node, "r") && nearest_paragraph(xml, node) == Some(paragraph))
        {
            let format = ctx
                .styles
                .run_format(xml, run, view == RenderView::Original);
            let mut declarations =
                super::styles::declarations(&super::styles::inline_css(format)).unwrap_or_default();
            declarations.retain(|name, _| {
                matches!(
                    name.as_str(),
                    "color" | "font-family" | "font-size" | "font-style" | "font-weight"
                )
            });
            if has_hyperlink {
                // Browsers apply a color to anchors themselves, which can
                // override a color inherited from the paragraph.
                declarations.remove("color");
            }
            common = Some(match common {
                None => declarations,
                Some(mut common) => {
                    common.retain(|name, value| declarations.get(name) == Some(value));
                    common
                }
            });
        }
    }
    common.unwrap_or_default()
}

fn nearest_paragraph(xml: &DocxXml, node: NodeId) -> Option<NodeId> {
    let mut parent = xml.doc.parent(node);
    while let Some(current) = parent {
        if xml.is_w(current, "p") {
            return Some(current);
        }
        parent = xml.doc.parent(current);
    }
    None
}

/// Pre-scan for complex (fldChar-delimited) fields: begin node → the field's
/// instruction text, collected from the w:instrText runs between `begin` and
/// `separate`/`end`. Nested fields attach instruction text to the innermost
/// open field.
fn field_instructions(
    xml: &DocxXml,
    group: &[NodeId],
) -> std::collections::HashMap<NodeId, String> {
    let mut map = std::collections::HashMap::new();
    let mut stack: Vec<(NodeId, String, bool)> = Vec::new();
    for &paragraph in group {
        for node in xml.doc.descendants(paragraph) {
            if xml.is_w(node, "fldChar") {
                match xml.attr(node, "fldCharType") {
                    Some("begin") => stack.push((node, String::new(), true)),
                    Some("separate") => {
                        if let Some(top) = stack.last_mut() {
                            top.2 = false;
                        }
                    }
                    Some("end") => {
                        if let Some((begin, instr, _)) = stack.pop() {
                            map.insert(begin, instr.trim().to_string());
                        }
                    }
                    _ => {}
                }
            } else if xml.is_w(node, "instrText") {
                if let Some((_, instr, true)) = stack.last_mut() {
                    instr.push_str(&xml.doc.text_content(node));
                }
            }
        }
    }
    while let Some((begin, instr, _)) = stack.pop() {
        map.insert(begin, instr.trim().to_string());
    }
    map
}

/// Cell span facts read from w:tcPr: gridSpan → colspan; vMerge restart opens
/// a vertical run, valueless/continue vMerge cells are covered by it.
struct CellInfo {
    node: NodeId,
    grid_start: u32,
    colspan: u32,
    vmerge_restart: bool,
    vmerge_continue: bool,
}

fn cell_infos(xml: &DocxXml, row: NodeId) -> Vec<CellInfo> {
    let mut cells = Vec::new();
    let mut grid = 0u32;
    for cell in effective_story_children(xml, row, &["tc"]) {
        if !xml.is_w(cell, "tc") {
            continue;
        }
        let mut colspan = 1u32;
        let mut vmerge_restart = false;
        let mut vmerge_continue = false;
        if let Some(tcpr) = xml.doc.children(cell).find(|&n| xml.is_w(n, "tcPr")) {
            for prop in xml.doc.children(tcpr) {
                if xml.is_w(prop, "gridSpan") {
                    colspan = xml
                        .attr(prop, "val")
                        .and_then(|v| v.parse().ok())
                        .filter(|&v| v >= 1)
                        .unwrap_or(1);
                } else if xml.is_w(prop, "vMerge") {
                    match xml.attr(prop, "val") {
                        Some("restart") => vmerge_restart = true,
                        // Valueless or "continue": covered by the run above.
                        _ => vmerge_continue = true,
                    }
                }
            }
        }
        cells.push(CellInfo {
            node: cell,
            grid_start: grid,
            colspan,
            vmerge_restart,
            vmerge_continue,
        });
        grid += colspan;
    }
    cells
}

/// Rows of a table (direct w:tr children) with their cell facts.
fn table_rows(xml: &DocxXml, table: NodeId) -> Vec<Vec<CellInfo>> {
    effective_story_children(xml, table, &["tr"])
        .into_iter()
        .map(|row| cell_infos(xml, row))
        .collect()
}

/// Rows a vMerge run spans, counting the restart cell and the covered cells
/// directly below it at the same grid column.
fn rowspan_of(rows: &[Vec<CellInfo>], row_index: usize, grid_start: u32) -> u32 {
    let mut span = 1u32;
    for row in &rows[row_index + 1..] {
        let continues = row
            .iter()
            .any(|cell| cell.grid_start == grid_start && cell.vmerge_continue);
        if !continues {
            break;
        }
        span += 1;
    }
    span
}

fn render_table(
    xml: &DocxXml,
    ctx: &RenderCtx,
    table: NodeId,
    view: RenderView,
    out: &mut String,
    paragraphs: &mut Vec<NodeId>,
    table_number: Option<&mut u32>,
) {
    let rows = table_rows(xml, table);
    if let Some(counter) = table_number {
        out.push_str(&format!("<table data-docx-table=\"{counter}\">"));
        *counter += 1;
    } else {
        out.push_str("<table>");
    }
    // Grid columns currently covered by a vMerge run from above → rows left.
    let mut covered: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    out.push_str("<tbody>");
    for (row_index, row) in rows.iter().enumerate() {
        out.push_str("\n<tr>");
        // Runs opened this row start covering at the NEXT row, so they merge
        // into `covered` only after this row's end-of-row decrement.
        let mut pending: Vec<(u32, u32)> = Vec::new();
        for cell in row {
            // A continue cell under an open run is covered — merged into the
            // restart cell above, so it does not render. An orphan continue
            // (no restart above) degrades to a plain cell.
            if cell.vmerge_continue && covered.get(&cell.grid_start).copied().unwrap_or(0) > 0 {
                continue;
            }
            out.push_str("\n<td");
            if cell.colspan > 1 {
                out.push_str(&format!(" colspan=\"{}\"", cell.colspan));
            }
            if cell.vmerge_restart {
                let rowspan = rowspan_of(&rows, row_index, cell.grid_start);
                if rowspan > 1 {
                    out.push_str(&format!(" rowspan=\"{rowspan}\""));
                    pending.push((cell.grid_start, rowspan - 1));
                }
            }
            out.push('>');
            // Cells hold block content: paragraphs (joined per view, like the
            // body) and nested tables.
            let blocks = effective_block_children(xml, cell.node);
            let rendered = render_block_list(xml, ctx, view, &blocks, None);
            for block in &rendered.blocks {
                paragraphs.extend(&block.paragraphs);
            }
            out.push('\n');
            out.push_str(&rendered.html);
            out.push_str("\n</td>");
        }
        // Consume one covered row per grid column, then open this row's runs.
        covered.retain(|_, remaining| {
            *remaining -= 1;
            *remaining > 0
        });
        covered.extend(pending);
        out.push_str("\n</tr>");
    }
    out.push_str("\n</tbody></table>");
}

/// Wrapper identity as rendered: (revision id, author).
type Wrapper = Option<(String, String)>;

/// Streaming emitter with minimal-transition tag management. Wrapper elements
/// (ins/del) nest outside formatting tags; formatting nests b → i → u and
/// keeps the longest common prefix open across transitions. Milestones emit
/// in place without disturbing open tags.
struct Emitter<'a> {
    out: &'a mut String,
    view: RenderView,
    ctx: &'a RenderCtx,
    inherited_run_css: BTreeMap<String, String>,
    ins: Wrapper,
    del: Wrapper,
    fmt: Fmt,
    fmt_pending: Option<FmtPending>,
    /// Open `<format pending>` wrapper (live B/I/U all off with rPrChange).
    neutral_pending_open: bool,
    /// Complex-field state: instruction per begin-fldChar (pre-scanned) and
    /// the current nesting depth — only the outermost field emits a boundary.
    field_instr: std::collections::HashMap<NodeId, String>,
    field_depth: usize,
    field_active_instr: String,
}

impl<'a> Emitter<'a> {
    fn new(
        out: &'a mut String,
        view: RenderView,
        field_instr: std::collections::HashMap<NodeId, String>,
        ctx: &'a RenderCtx,
        inherited_run_css: BTreeMap<String, String>,
    ) -> Self {
        Emitter {
            out,
            view,
            ctx,
            inherited_run_css,
            ins: None,
            del: None,
            fmt: Fmt::default(),
            fmt_pending: None,
            neutral_pending_open: false,
            field_instr,
            field_depth: 0,
            field_active_instr: String::new(),
        }
    }

    /// Open a field region at a begin-fldChar. The boundary is hard, like an
    /// anchor: open revision/format tags close before it and reopen inside.
    fn field_begin(&mut self, begin: NodeId) {
        self.field_depth += 1;
        if self.field_depth == 1 {
            let instr = self.field_instr.get(&begin).cloned().unwrap_or_default();
            self.field_active_instr = instr.clone();
            self.finish();
            self.out.push_str(&format!(
                "<span data-docx-field=\"{}\" contenteditable=\"false\">",
                escape_attr(&instr)
            ));
        }
    }

    fn field_end(&mut self) {
        if self.field_depth == 0 {
            return; // orphan end fldChar (begin in an earlier block)
        }
        self.field_depth -= 1;
        if self.field_depth == 0 {
            self.finish();
            self.out.push_str("</span>");
        }
    }

    fn fmt_stack(fmt: Fmt) -> Vec<&'static str> {
        let mut stack = Vec::new();
        if fmt.bold {
            stack.push("b");
        }
        if fmt.italic {
            stack.push("i");
        }
        if fmt.underline {
            stack.push("u");
        }
        if fmt.strike {
            stack.push("s");
        }
        if let Some(align) = fmt.vert_align {
            stack.push(align.dialect_tag());
        }
        if !super::styles::inline_css(fmt).is_empty() {
            stack.insert(0, "span");
        }
        stack
    }

    fn close_fmt_to(&mut self, keep: usize) {
        let stack = Self::fmt_stack(self.fmt);
        for tag in stack.iter().skip(keep).rev() {
            self.out.push_str(&format!("</{tag}>"));
        }
    }

    fn close_neutral_pending(&mut self) {
        if self.neutral_pending_open {
            self.out.push_str("</span>");
            self.neutral_pending_open = false;
        }
    }

    fn open_neutral_pending(&mut self, pending: &FmtPending) {
        self.out.push_str("<span data-docx-format-change=\"true\"");
        if let Some(id) = pending.id.as_deref() {
            self.out
                .push_str(&format!(" data-docx-revision=\"{}\"", escape_attr(id)));
        }
        if let Some(author) = pending.author.as_deref() {
            self.out
                .push_str(&format!(" data-docx-author=\"{}\"", escape_attr(author)));
        }
        self.out.push('>');
        self.neutral_pending_open = true;
    }

    fn transition(&mut self, ins: &Wrapper, del: &Wrapper, fmt: Fmt, pending: Option<&FmtPending>) {
        let fmt = super::styles::without_inherited_css(fmt, &self.inherited_run_css);
        if self.view != RenderView::Markup {
            // No revision tags in projected views; only formatting moves.
            self.transition_fmt(fmt, None);
            return;
        }
        if &self.ins != ins || &self.del != del {
            self.close_fmt_to(0);
            self.close_neutral_pending();
            self.fmt = Fmt::default();
            self.fmt_pending = None;
            // del nests inside ins, so it always closes first and reopens last.
            if self.del.is_some() {
                self.out.push_str("</del>");
            }
            if &self.ins != ins {
                if self.ins.is_some() {
                    self.out.push_str("</ins>");
                }
                if let Some((id, author)) = ins {
                    self.out.push_str(&format!(
                        "<ins data-docx-revision=\"{}\" data-docx-author=\"{}\">",
                        escape_attr(id),
                        escape_attr(author)
                    ));
                }
                self.ins = ins.clone();
            }
            if let Some((id, author)) = del {
                self.out.push_str(&format!(
                    "<del data-docx-revision=\"{}\" data-docx-author=\"{}\">",
                    escape_attr(id),
                    escape_attr(author)
                ));
            }
            self.del = del.clone();
        }
        self.transition_fmt(fmt, pending);
    }

    fn open_fmt_tag(&mut self, tag: &str, pending: Option<&FmtPending>, outermost: bool, fmt: Fmt) {
        self.out.push('<');
        self.out.push_str(tag);
        if tag == "span" {
            self.out.push_str(&format!(
                " style=\"{}\"",
                escape_attr(&super::styles::inline_css(fmt))
            ));
        }
        if outermost {
            if let Some(pending) = pending {
                self.out.push_str(" data-docx-format-change=\"true\"");
                if let Some(id) = pending.id.as_deref() {
                    self.out
                        .push_str(&format!(" data-docx-revision=\"{}\"", escape_attr(id)));
                }
                if let Some(author) = pending.author.as_deref() {
                    self.out
                        .push_str(&format!(" data-docx-author=\"{}\"", escape_attr(author)));
                }
            }
        }
        self.out.push('>');
    }

    fn transition_fmt(&mut self, fmt: Fmt, pending: Option<&FmtPending>) {
        let pending_eq = self.fmt_pending.as_ref() == pending;
        if self.fmt == fmt && pending_eq {
            return;
        }
        let target = Self::fmt_stack(fmt);
        // Neutral wrapper: rPrChange with live dialect format all off.
        if pending.is_some() && target.is_empty() {
            if self.neutral_pending_open && pending_eq && self.fmt == fmt {
                return;
            }
            self.close_fmt_to(0);
            self.close_neutral_pending();
            if let Some(pending) = pending {
                self.open_neutral_pending(pending);
            }
            self.fmt = fmt;
            self.fmt_pending = pending.cloned();
            return;
        }
        self.close_neutral_pending();
        // Pending identity is per-run; any pending change forces a full fmt reopen
        // so attributes land on the outermost tag.
        if !pending_eq {
            self.close_fmt_to(0);
            for (i, tag) in target.iter().enumerate() {
                self.open_fmt_tag(tag, pending, i == 0, fmt);
            }
            self.fmt = fmt;
            self.fmt_pending = pending.cloned();
            return;
        }
        let current = Self::fmt_stack(self.fmt);
        let mut keep = current
            .iter()
            .zip(&target)
            .take_while(|(a, b)| a == b)
            .count();
        if self.fmt.resets != fmt.resets
            || self.fmt.color != fmt.color
            || self.fmt.font_size != fmt.font_size
            || self.fmt.font_family != fmt.font_family
        {
            keep = 0;
        }
        self.close_fmt_to(keep);
        for (i, tag) in target.iter().enumerate().skip(keep) {
            self.open_fmt_tag(tag, pending, i == 0 && keep == 0, fmt);
        }
        self.fmt = fmt;
    }

    fn text(
        &mut self,
        text: &str,
        ins: &Wrapper,
        del: &Wrapper,
        fmt: Fmt,
        pending: Option<&FmtPending>,
    ) {
        self.transition(ins, del, fmt, pending);
        self.out.push_str(&escape_text(text));
    }

    fn tab(&mut self, ins: &Wrapper, del: &Wrapper, fmt: Fmt, pending: Option<&FmtPending>) {
        self.transition(ins, del, fmt, pending);
        self.out.push_str("<span data-docx-tab=\"true\">\t</span>");
    }

    fn br(&mut self, kind: BrKind, ins: &Wrapper, del: &Wrapper) {
        let pending = self.fmt_pending.clone();
        self.transition(ins, del, self.fmt, pending.as_ref());
        match kind.dialect_type() {
            None => self.out.push_str("<br>"),
            Some(kind) => self
                .out
                .push_str(&format!("<br data-docx-break=\"{kind}\">")),
        }
    }

    /// An embedded picture: same wrapper discipline as `<br/>`. Public markup
    /// carries only display attrs; package rid identity is recovered from the
    /// XML when editing.
    fn image(&mut self, info: &crate::media::ImageInfo, ins: &Wrapper, del: &Wrapper) {
        let pending = self.fmt_pending.clone();
        self.transition(ins, del, self.fmt, pending.as_ref());
        let Some(src) = self.ctx.images.get(&info.rid) else {
            self.out.push_str(&format!(
                "<span data-docx-image-unavailable=\"true\" data-docx-image-rid=\"{}\"",
                escape_attr(&info.rid)
            ));
            if let Some(part) = self.ctx.image_parts.get(&info.rid) {
                self.out
                    .push_str(&format!(" data-docx-image-part=\"{}\"", escape_attr(part)));
            }
            if let Some(width) = info.width {
                self.out
                    .push_str(&format!(" data-docx-image-width=\"{width}\""));
            }
            if let Some(height) = info.height {
                self.out
                    .push_str(&format!(" data-docx-image-height=\"{height}\""));
            }
            if let Some(alt) = &info.alt {
                self.out
                    .push_str(&format!(" data-docx-image-alt=\"{}\"", escape_attr(alt)));
            }
            self.out.push_str(&format!(
                ">{}</span>",
                escape_text(info.alt.as_deref().unwrap_or("[Image]"))
            ));
            return;
        };
        self.out
            .push_str(&format!("<img src=\"{}\"", escape_attr(src)));
        if let Some(part) = self.ctx.image_parts.get(&info.rid) {
            self.out
                .push_str(&format!(" data-docx-image-part=\"{}\"", escape_attr(part)));
        }
        if let Some(width) = info.width {
            self.out.push_str(&format!(" width=\"{width}\""));
        }
        if let Some(height) = info.height {
            self.out.push_str(&format!(" height=\"{height}\""));
        }
        if let Some(alt) = &info.alt {
            self.out.push_str(&format!(" alt=\"{}\"", escape_attr(alt)));
        }
        self.out.push('>');
    }

    /// Milestones sit at their true wrapper depth (a marker outside `w:del`
    /// must not render inside `<del>`), but don't force formatting changes.
    fn milestone(&mut self, start: bool, id: &str, ins: &Wrapper, del: &Wrapper) {
        let fmt = self.fmt;
        let pending = self.fmt_pending.clone();
        self.transition(ins, del, fmt, pending.as_ref());
        let tag = if start {
            "data-docx-comment-start"
        } else {
            "data-docx-comment-end"
        };
        self.out.push_str(&format!(
            "<span {tag}=\"{}\" hidden></span>",
            escape_attr(id)
        ));
    }

    /// Inline footnote/endnote at the reference site (ids bound, not rendered).
    fn note_ref(&mut self, endnote: bool, id: &str, ins: &Wrapper, del: &Wrapper) {
        let fmt = self.fmt;
        let pending = self.fmt_pending.clone();
        self.transition(ins, del, fmt, pending.as_ref());
        let kind = if endnote { "endnote" } else { "footnote" };
        self.out.push_str(&format!(
            "<sup data-docx-note=\"{kind}\" data-docx-note-id=\"{}\"><a href=\"#{}\">{}</a></sup>",
            escape_attr(id),
            escape_attr(&self.ctx.note_dom_id(kind, id)),
            escape_text(
                self.ctx
                    .notes
                    .get(endnote, id)
                    .map(|n| n.label.as_str())
                    .unwrap_or(id)
            )
        ));
    }

    /// An equation: same milestone discipline as comments/notes — it rides
    /// along inside whatever `<ins>`/`<del>` wrapper currently encloses it
    /// (so a tracked insertion/deletion around the equation reappears in the
    /// markup), but doesn't force a formatting transition of its own.
    /// Presentation MathML is the editable equation representation.
    fn equation(&mut self, text: &str, ins: &Wrapper, del: &Wrapper) {
        let fmt = self.fmt;
        let pending = self.fmt_pending.clone();
        self.transition(ins, del, fmt, pending.as_ref());
        self.out.push_str(text);
    }

    fn finish(&mut self) {
        self.close_fmt_to(0);
        self.close_neutral_pending();
        self.fmt = Fmt::default();
        self.fmt_pending = None;
        if self.view == RenderView::Markup {
            if self.del.is_some() {
                self.out.push_str("</del>");
                self.del = None;
            }
            if self.ins.is_some() {
                self.out.push_str("</ins>");
                self.ins = None;
            }
        }
    }
}

fn wrapper_of(xml: &DocxXml, node: NodeId) -> Wrapper {
    Some((
        xml.attr(node, "id").unwrap_or("").to_string(),
        xml.attr(node, "author").unwrap_or("").to_string(),
    ))
}

fn run_fmt_pending(xml: &DocxXml, run: NodeId) -> Option<FmtPending> {
    let rpr = xml.run_rpr(run)?;
    let change = xml.doc.children(rpr).find(|&n| xml.is_w(n, "rPrChange"))?;
    Some(FmtPending {
        id: xml
            .attr(change, "id")
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
        author: xml
            .attr(change, "author")
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
    })
}

fn emit_content(
    xml: &DocxXml,
    ctx: &RenderCtx,
    node: NodeId,
    view: RenderView,
    ins: Option<NodeId>,
    del: Option<NodeId>,
    emitter: &mut Emitter,
) {
    for child in xml.doc.children(node).collect::<Vec<_>>() {
        if !xml.doc.is_element(child) {
            continue;
        }
        if xml.is_w(child, "pPr") {
            continue;
        }
        if xml.is_w(child, "p") {
            // Nested textbox/drawing paragraphs belong to another story.
            continue;
        }
        if xml.is_local(child, "AlternateContent") {
            if let Some(branch) = xml.alternate_content_branch(child) {
                emit_content(xml, ctx, branch, view, ins, del, emitter);
            }
            continue;
        }
        if xml.is_rev_ins(child) {
            emit_content(xml, ctx, child, view, Some(child), del, emitter);
            continue;
        }
        if xml.is_rev_del(child) {
            emit_content(xml, ctx, child, view, ins, Some(child), emitter);
            continue;
        }
        if xml.is_w(child, "hyperlink") {
            // The anchor is a hard boundary: open revision/format tags close
            // before it and reopen inside, so nesting stays well-formed.
            let href = ctx.links.get(&child).cloned().unwrap_or_default();
            emitter.finish();
            let safe = super::input::safe_url(&href);
            emitter.out.push_str(&format!(
                "<a href=\"{}\"{}>",
                escape_attr(if safe { &href } else { "#" }),
                if safe {
                    String::new()
                } else {
                    format!(" data-docx-href=\"{}\"", escape_attr(&href))
                }
            ));
            emit_content(xml, ctx, child, view, ins, del, emitter);
            emitter.finish();
            emitter.out.push_str("</a>");
            continue;
        }
        if xml.is_w(child, "fldSimple") {
            // Same boundary discipline as complex fields; nested fields fold
            // into the outermost region (depth tracking suppresses inner tags).
            emitter.field_depth += 1;
            if emitter.field_depth == 1 {
                let instr = xml.attr(child, "instr").unwrap_or("").trim().to_string();
                emitter.finish();
                emitter.out.push_str(&format!(
                    "<span data-docx-field=\"{}\" contenteditable=\"false\">",
                    escape_attr(&instr)
                ));
            }
            emit_content(xml, ctx, child, view, ins, del, emitter);
            if emitter.field_depth == 1 {
                emitter.finish();
                emitter.out.push_str("</span>");
            }
            emitter.field_depth -= 1;
            continue;
        }
        if xml.is_w(child, "commentRangeStart") || xml.is_w(child, "commentRangeEnd") {
            if view == RenderView::Markup {
                let start = xml.is_w(child, "commentRangeStart");
                let ins_wrap = ins.and_then(|node| wrapper_of(xml, node));
                let del_wrap = del.and_then(|node| wrapper_of(xml, node));
                emitter.milestone(
                    start,
                    xml.attr(child, "id").unwrap_or(""),
                    &ins_wrap,
                    &del_wrap,
                );
            }
            continue;
        }
        if xml.is_w(child, "r") {
            emit_run(xml, child, view, ins, del, emitter);
            continue;
        }
        if xml.is_local(child, "oMath") || xml.is_local(child, "oMathPara") {
            // Equations retain their native presentation structure as MathML.
            let text = super::omml_mathml::omml_to_mathml(xml, child, emitter.ctx.math_limits);
            let hidden = match view {
                RenderView::Markup => false,
                RenderView::Final => del.is_some(),
                RenderView::Original => ins.is_some(),
            };
            if !hidden && !text.is_empty() {
                let ins_wrap = ins.and_then(|node| wrapper_of(xml, node));
                let del_wrap = del.and_then(|node| wrapper_of(xml, node));
                emitter.equation(&text, &ins_wrap, &del_wrap);
            }
            continue;
        }
        emit_content(xml, ctx, child, view, ins, del, emitter);
    }
}

fn emit_run(
    xml: &DocxXml,
    run: NodeId,
    view: RenderView,
    ins: Option<NodeId>,
    del: Option<NodeId>,
    emitter: &mut Emitter,
) {
    let fmt = emitter
        .ctx
        .styles
        .run_format(xml, run, view == RenderView::Original);
    let pending = if view == RenderView::Markup {
        run_fmt_pending(xml, run)
    } else {
        None
    };
    let ins_wrap = ins.map(|node| wrapper_of(xml, node).expect("wrapper"));
    let del_wrap = del.map(|node| wrapper_of(xml, node).expect("wrapper"));
    emit_run_children(
        xml,
        run,
        view,
        ins,
        del,
        &ins_wrap,
        &del_wrap,
        fmt,
        pending.as_ref(),
        emitter,
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_run_children(
    xml: &DocxXml,
    container: NodeId,
    view: RenderView,
    ins: Option<NodeId>,
    del: Option<NodeId>,
    ins_wrap: &Wrapper,
    del_wrap: &Wrapper,
    fmt: Fmt,
    pending: Option<&FmtPending>,
    emitter: &mut Emitter,
) {
    for child in xml.doc.children(container).collect::<Vec<_>>() {
        if !xml.doc.is_element(child) {
            continue;
        }
        if xml.is_local(child, "AlternateContent") {
            if let Some(branch) = xml.alternate_content_branch(child) {
                // AlternateContent inside w:r substitutes run content, so its
                // effective branch inherits the containing run's formatting
                // and revision wrappers.
                emit_run_children(
                    xml, branch, view, ins, del, ins_wrap, del_wrap, fmt, pending, emitter,
                );
            }
            continue;
        }
        if xml.is_w(child, "t") {
            let hidden = match view {
                RenderView::Markup => false,
                RenderView::Final => del.is_some(),
                RenderView::Original => ins.is_some(),
            };
            if !hidden {
                emitter.text(
                    &xml.doc.text_content(child),
                    &ins_wrap,
                    &del_wrap,
                    fmt,
                    pending,
                );
            }
            continue;
        }
        if xml.is_w(child, "delText") {
            let hidden = match view {
                RenderView::Markup => false,
                RenderView::Final => true,
                RenderView::Original => ins.is_some(),
            };
            if !hidden {
                emitter.text(
                    &xml.doc.text_content(child),
                    ins_wrap,
                    del_wrap,
                    fmt,
                    pending,
                );
            }
            continue;
        }
        if xml.is_w(child, "br") {
            let hidden = match view {
                RenderView::Markup => false,
                RenderView::Final => del.is_some(),
                RenderView::Original => ins.is_some(),
            };
            if !hidden {
                emitter.br(
                    BrKind::from_docx(xml.attr(child, "type")),
                    ins_wrap,
                    del_wrap,
                );
            }
            continue;
        }
        if crate::media::is_image_container(xml, child) {
            // Embedded pictures follow the same view projection as breaks; a
            // container with no resolvable picture (chart, shape) still
            // contributes nothing.
            let hidden = match view {
                RenderView::Markup => false,
                RenderView::Final => del.is_some(),
                RenderView::Original => ins.is_some(),
            };
            if !hidden {
                if let Some(info) = crate::media::image_info(xml, child) {
                    emitter.image(&info, ins_wrap, del_wrap);
                }
            }
            continue;
        }
        if xml.is_w(child, "tab") {
            let hidden = match view {
                RenderView::Markup => false,
                RenderView::Final => del.is_some(),
                RenderView::Original => ins.is_some(),
            };
            if !hidden {
                emitter.tab(ins_wrap, del_wrap, fmt, pending);
            }
            continue;
        }
        if xml.is_w(child, "footnoteReference") || xml.is_w(child, "endnoteReference") {
            if !matches!(
                (view, ins.is_some(), del.is_some()),
                (RenderView::Final, _, true) | (RenderView::Original, true, _)
            ) {
                let endnote = xml.is_w(child, "endnoteReference");
                emitter.note_ref(
                    endnote,
                    xml.attr(child, "id").unwrap_or(""),
                    ins_wrap,
                    del_wrap,
                );
            }
            continue;
        }
        if xml.is_w(child, "fldChar") {
            // Field boundaries follow the same view-projection as the runs
            // that carry them: a field whose fldChars are deleted disappears
            // from the final view along with its result.
            let hidden = match view {
                RenderView::Markup => false,
                RenderView::Final => del.is_some(),
                RenderView::Original => ins.is_some(),
            };
            if !hidden {
                match xml.attr(child, "fldCharType") {
                    Some("begin") => emitter.field_begin(child),
                    Some("end") => emitter.field_end(),
                    _ => {}
                }
            }
            continue;
        }
        // instrText is the field's instruction, never display text — it
        // surfaces as the <field instr> attribute, not content.
        // commentReference and other run content contribute nothing.
    }
}
