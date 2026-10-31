//! Conservative direct table grid edits with preflight of content boundaries.
use super::{
    tables::{cells, resolve, rows},
    typed::resolve_paragraph_by_para_id,
};
use crate::{
    api::{InsertPosition, TableEdit},
    document::DocxXml,
    outcome::Outcome,
    package::Package,
};
use serde_json::{json, Value};
use xmloxide::tree::NodeId;
fn error(message: &str) -> Outcome {
    Outcome::blocked(message)
}
pub fn validate(at: &str, a: &TableEdit) -> Result<(), String> {
    if !at.starts_with("table:") && !at.starts_with('$') {
        return Err("edit_table requires a table handle".into());
    }
    if let TableEdit::InsertRows {
        alias: Some(name), ..
    }
    | TableEdit::InsertColumns {
        alias: Some(name), ..
    } = a
    {
        let mut chars = name.chars();
        if !matches!(chars.next(),Some(c) if c.is_ascii_alphabetic() || c=='_')
            || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err("invalid inserted table group alias".into());
        }
    }
    let valid = match a {
        TableEdit::InsertRows { row, count, .. } | TableEdit::DeleteRows { row, count } => {
            *row > 0 && *count > 0 && *count <= 100
        }
        TableEdit::InsertColumns { column, count, .. }
        | TableEdit::DeleteColumns { column, count } => *column > 0 && *count > 0 && *count <= 100,
        TableEdit::MergeCells {
            row,
            column,
            row_span,
            column_span,
        } => {
            *row > 0
                && *column > 0
                && *row_span > 0
                && *column_span > 0
                && (*row_span > 1 || *column_span > 1)
        }
        TableEdit::SplitCell { row, column } => *row > 0 && *column > 0,
        TableEdit::Delete => true,
    };
    if valid {
        Ok(())
    } else {
        Err("table edit coordinates/counts must be positive; inserted count <=100".into())
    }
}
fn guarded(x: &DocxXml, t: NodeId) -> Result<(), Outcome> {
    let mut ancestor = x.doc.parent(t);
    while let Some(node) = ancestor {
        if [
            "ins",
            "del",
            "moveFrom",
            "moveTo",
            "sdt",
            "sdtContent",
            "customXml",
            "AlternateContent",
        ]
        .iter()
        .any(|tag| x.is_local(node, tag))
        {
            return Err(error("table structural editing inside revision or content-control wrappers is unsupported"));
        }
        ancestor = x.doc.parent(node);
    }
    if x.doc.descendants(t).any(|n| {
        [
            "ins",
            "del",
            "moveFrom",
            "moveTo",
            "bookmarkStart",
            "bookmarkEnd",
            "commentRangeStart",
            "commentRangeEnd",
            "commentReference",
            "fldChar",
            "fldSimple",
            "sdt",
            "customXml",
            "tblPrChange",
            "tblGridChange",
            "trPrChange",
            "tcPrChange",
            "pPrChange",
            "rPrChange",
            "cellIns",
            "cellDel",
            "cellMerge",
            "AlternateContent",
        ]
        .iter()
        .any(|tag| x.is_local(n, tag))
    }) {
        return Err(error("table structural edits require settled revisions and no bookmarks, comments, fields or content controls in the target table"));
    }
    Ok(())
}
fn span(x: &DocxXml, c: NodeId) -> usize {
    x.child_w(c, "tcPr")
        .and_then(|p| x.child_w(p, "gridSpan"))
        .and_then(|n| x.attr(n, "val"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}
fn merged(x: &DocxXml, c: NodeId) -> bool {
    span(x, c) != 1
        || x.child_w(c, "tcPr")
            .is_some_and(|p| x.child_w(p, "vMerge").is_some() || x.child_w(p, "hMerge").is_some())
}
fn regular(x: &DocxXml, t: NodeId) -> Result<(Vec<NodeId>, usize), Outcome> {
    let rs = rows(x, t);
    let width = rs.first().map(|r| cells(x, *r).len()).unwrap_or(0);
    if width == 0
        || rs.iter().any(|r| {
            cells(x, *r).len() != width
                || cells(x, *r).iter().any(|c| merged(x, *c))
                || x.child_w(*r, "trPr").is_some_and(|p| {
                    x.child_w(p, "gridBefore").is_some() || x.child_w(p, "gridAfter").is_some()
                })
        })
    {
        return Err(error("this grid edit requires a rectangular table without existing merges or omitted cells; split merged cells first"));
    }
    Ok((rs, width))
}
fn empty_cell(x: &mut DocxXml) -> NodeId {
    let c = x.doc.create_element("w:tc");
    let p = x.doc.create_element("w:p");
    x.doc.append_child(c, p);
    c
}
fn blank_like(x: &mut DocxXml, source: NodeId) -> NodeId {
    let c = empty_cell(x);
    if let Some(old) = x.child_w(source, "tcPr") {
        let copy = x.doc.clone_node(old, true);
        let first = x.doc.children(c).next().unwrap();
        x.doc.insert_before(first, copy);
    }
    c
}
fn tcpr(x: &mut DocxXml, c: NodeId) -> NodeId {
    if let Some(p) = x.child_w(c, "tcPr") {
        return p;
    }
    let p = x.doc.create_element("w:tcPr");
    if let Some(first) = x.doc.children(c).next() {
        x.doc.insert_before(first, p);
    } else {
        x.doc.append_child(c, p);
    }
    p
}
fn merge_props(x: &mut DocxXml, c: NodeId, width: usize, vertical: Option<bool>) {
    let p = tcpr(x, c);
    for n in x
        .doc
        .children(p)
        .filter(|&n| x.is_w(n, "gridSpan") || x.is_w(n, "vMerge") || x.is_w(n, "hMerge"))
        .collect::<Vec<_>>()
    {
        x.doc.detach(n);
    }
    let next = x
        .doc
        .children(p)
        .find(|&n| !x.is_w(n, "cnfStyle") && !x.is_w(n, "tcW"));
    if width > 1 {
        let n = x.doc.create_element("w:gridSpan");
        x.doc.set_attribute(n, "w:val", &width.to_string());
        if let Some(next) = next {
            x.doc.insert_before(next, n);
        } else {
            x.doc.append_child(p, n);
        }
    }
    if let Some(restart) = vertical {
        let n = x.doc.create_element("w:vMerge");
        x.doc
            .set_attribute(n, "w:val", if restart { "restart" } else { "continue" });
        if let Some(next) = next {
            x.doc.insert_before(next, n);
        } else {
            x.doc.append_child(p, n);
        }
    }
    // A merged cell's old preferred width is no longer authoritative.
    if let Some(w) = x.child_w(p, "tcW") {
        super::layout::set_attr(x, w, "w:type", "auto");
        super::layout::set_attr(x, w, "w:w", "0");
    }
}
fn grid(x: &DocxXml, t: NodeId) -> Result<(NodeId, Vec<NodeId>), Outcome> {
    let g = x.child_w(t, "tblGrid").ok_or_else(|| {
        error("table has no explicit grid; author column widths before structural editing")
    })?;
    Ok((
        g,
        x.doc
            .children(g)
            .filter(|&n| x.is_w(n, "gridCol"))
            .collect(),
    ))
}
fn check_range(start: u32, count: u32, len: usize) -> Result<std::ops::Range<usize>, Outcome> {
    let first = (start as usize)
        .checked_sub(1)
        .ok_or_else(|| error("table index must be positive"))?;
    let end = first
        .checked_add(count as usize)
        .ok_or_else(|| error("table range overflow"))?;
    if end > len {
        return Err(error("table edit range is out of bounds"));
    }
    Ok(first..end)
}
fn at_column(x: &DocxXml, r: NodeId, col: usize) -> Option<NodeId> {
    let mut offset = 0;
    for c in cells(x, r) {
        if offset == col {
            return Some(c);
        }
        offset += span(x, c);
    }
    None
}

pub fn edit(
    _package: &mut Package,
    x: &mut DocxXml,
    at: &str,
    a: &TableEdit,
    tracked: bool,
) -> Result<(String, Value), Outcome> {
    if tracked {
        return Err(error(
            "tracked table structural editing is unsupported; use an explicit direct plan",
        ));
    }
    let t = resolve(x, at, 1)?;
    guarded(x, t)?;
    let mut inserted: Vec<(String, NodeId)> = Vec::new();
    match a {
        TableEdit::Delete => {
            x.doc.detach(t);
        }
        TableEdit::InsertRows {
            row,
            position,
            count,
            ..
        } => {
            let (rs, width) = regular(x, t)?;
            let anchor = *rs
                .get(*row as usize - 1)
                .ok_or_else(|| error("row is out of bounds"))?;
            if rs.len() + *count as usize > 1000 {
                return Err(error("table row limit exceeded"));
            }
            let mut cursor = anchor;
            for i in 0..*count {
                let r = x.doc.create_element("w:tr");
                if let Some(pr) = x.child_w(anchor, "trPr") {
                    let cp = x.doc.clone_node(pr, true);
                    x.doc.append_child(r, cp);
                }
                let cs = cells(x, anchor);
                for (ci, c) in cs.iter().take(width).enumerate() {
                    let new = blank_like(x, *c);
                    x.doc.append_child(r, new);
                    inserted.push((format!("row:{}/cell:{}", i + 1, ci + 1), new));
                }
                match position {
                    InsertPosition::Before => x.doc.insert_before(anchor, r),
                    InsertPosition::After => {
                        x.doc.insert_after(cursor, r);
                        cursor = r;
                    }
                };
                inserted.push((format!("row:{}", i + 1), r));
            }
        }
        TableEdit::DeleteRows { row, count } => {
            let (rs, _) = regular(x, t)?;
            let range = check_range(*row, *count, rs.len())?;
            if range.len() == rs.len() {
                return Err(error("cannot remove every row; delete the table instead"));
            }
            for r in &rs[range] {
                x.doc.detach(*r);
            }
        }
        TableEdit::InsertColumns {
            column,
            position,
            count,
            ..
        } => {
            let (rs, width) = regular(x, t)?;
            let col = check_range(*column, 1, width)?.start;
            let (g, cols) = grid(x, t)?;
            if cols.len() != width {
                return Err(error("table grid and row widths disagree"));
            }
            if width + *count as usize > 1000 {
                return Err(error("table column limit exceeded"));
            }
            for (ri, r) in rs.into_iter().enumerate() {
                let anchor = cells(x, r)[col];
                let mut cursor = anchor;
                for ci in 0..*count {
                    let c = blank_like(x, anchor);
                    match position {
                        InsertPosition::Before => x.doc.insert_before(anchor, c),
                        InsertPosition::After => {
                            x.doc.insert_after(cursor, c);
                            cursor = c;
                        }
                    };
                    inserted.push((format!("row:{}/cell:{}", ri + 1, ci + 1), c));
                }
            }
            let _ = g;
            for _ in 0..*count {
                let c = x.doc.clone_node(cols[col], true);
                match position {
                    InsertPosition::Before => x.doc.insert_before(cols[col], c),
                    InsertPosition::After => x.doc.insert_after(cols[col], c),
                };
            }
        }
        TableEdit::DeleteColumns { column, count } => {
            let (rs, width) = regular(x, t)?;
            let range = check_range(*column, *count, width)?;
            if range.len() == width {
                return Err(error(
                    "cannot remove every column; delete the table instead",
                ));
            }
            let (_, cols) = grid(x, t)?;
            if cols.len() != width {
                return Err(error("table grid and row widths disagree"));
            }
            for r in rs {
                let cs = cells(x, r);
                for c in &cs[range.clone()] {
                    x.doc.detach(*c);
                }
            }
            for c in &cols[range] {
                x.doc.detach(*c);
            }
        }
        TableEdit::MergeCells {
            row,
            column,
            row_span,
            column_span,
        } => {
            let (rs, width) = regular(x, t)?;
            let rr = check_range(*row, *row_span, rs.len())?;
            let cr = check_range(*column, *column_span, width)?;
            let primary = cells(x, rs[rr.start])[cr.start];
            // Move content in row-major order. Nested tables remain intact.
            for ri in rr.clone() {
                let cs = cells(x, rs[ri]);
                for &c in &cs[cr.clone()] {
                    if c == primary {
                        continue;
                    }
                    for n in x.doc.children(c).collect::<Vec<_>>() {
                        if !x.is_w(n, "tcPr") {
                            x.doc.detach(n);
                            x.doc.append_child(primary, n);
                        }
                    }
                }
            }
            // Each cell requires a final paragraph, including merge continuations.
            if !x
                .doc
                .children(primary)
                .last()
                .is_some_and(|n| x.is_w(n, "p"))
            {
                let p = x.doc.create_element("w:p");
                x.doc.append_child(primary, p);
            }
            for ri in rr.clone() {
                let cs = cells(x, rs[ri]);
                let keep = cs[cr.start];
                for &c in &cs[cr.start + 1..cr.end] {
                    x.doc.detach(c);
                }
                if keep != primary {
                    let p = x.doc.create_element("w:p");
                    x.doc.append_child(keep, p);
                }
                merge_props(
                    x,
                    keep,
                    cr.len(),
                    if rr.len() > 1 {
                        Some(ri == rr.start)
                    } else {
                        None
                    },
                );
            }
        }
        TableEdit::SplitCell { row, column } => {
            let rs = rows(x, t);
            let r = *rs
                .get(*row as usize - 1)
                .ok_or_else(|| error("row out of bounds"))?;
            let col = *column as usize - 1;
            let c = at_column(x, r, col)
                .ok_or_else(|| error("column must address the logical start of a cell"))?;
            if x.child_w(c, "tcPr")
                .is_some_and(|p| x.child_w(p, "hMerge").is_some())
            {
                return Err(error("legacy hMerge splitting is unsupported"));
            }
            let vm = x.child_w(c, "tcPr").and_then(|p| x.child_w(p, "vMerge"));
            if vm.is_some_and(|n| x.attr(n, "val") != Some("restart")) {
                return Err(error(
                    "split the logical restart cell, not a vertical continuation",
                ));
            }
            let width = span(x, c);
            let first = *row as usize - 1;
            let mut end = first + 1;
            if vm.is_some() {
                while end < rs.len() {
                    let Some(next) = at_column(x, rs[end], col) else {
                        break;
                    };
                    let over = x.child_w(next, "tcPr").and_then(|p| x.child_w(p, "vMerge"));
                    if over.is_some_and(|n| x.attr(n, "val") != Some("restart")) {
                        if span(x, next) != width {
                            return Err(error("inconsistent vertical merge widths"));
                        }
                        end += 1;
                    } else {
                        break;
                    }
                }
            }
            if width == 1 && end == first + 1 {
                return Err(error("cell is not merged"));
            }
            let targets = (first..end)
                .map(|i| at_column(x, rs[i], col).unwrap())
                .collect::<Vec<_>>();
            for c in targets {
                merge_props(x, c, 1, None);
                for _ in 1..width {
                    let n = blank_like(x, c);
                    x.doc.insert_after(c, n);
                }
            }
        }
    }
    let positions = super::table_positions(x);
    let alias_paths = inserted
        .into_iter()
        .filter_map(|(suffix, node)| positions.get(&node).map(|address| (suffix, json!(address))))
        .collect::<serde_json::Map<String, Value>>();
    Ok((
        "edited table structure".into(),
        json!({"table":at,"tracked":false,"alias_paths":alias_paths}),
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn insert(
    package: &mut Package,
    x: &mut DocxXml,
    source: Option<[u8; 32]>,
    at: &str,
    position: InsertPosition,
    row_count: u32,
    column_count: u32,
    cell_content: &[Vec<String>],
    tracked: bool,
    plan_seed: &[u8; 32],
    op_index: usize,
) -> Result<(String, Value), Outcome> {
    if tracked {
        return Err(error(
            "tracked table insertion is unsupported; use an explicit direct plan",
        ));
    }
    let p = resolve_paragraph_by_para_id(x, package, source, at)?;
    if x.child_w(p, "pPr")
        .is_some_and(|pr| x.child_w(pr, "sectPr").is_some())
    {
        return Err(error(
            "table insertion at a section-boundary paragraph is unsupported",
        ));
    }
    let parent = x
        .doc
        .parent(p)
        .ok_or_else(|| error("paragraph has no block container"))?;
    if !x.is_w(parent, "body") && !x.is_w(parent, "tc") {
        return Err(error(
            "table insertion requires an ordinary body or table-cell paragraph",
        ));
    }
    let t = x.doc.create_element("w:tbl");
    let pr = x.doc.create_element("w:tblPr");
    let w = x.doc.create_element("w:tblW");
    x.doc.set_attribute(w, "w:w", "0");
    x.doc.set_attribute(w, "w:type", "auto");
    x.doc.append_child(pr, w);
    x.doc.append_child(t, pr);
    let g = x.doc.create_element("w:tblGrid");
    for _ in 0..column_count {
        let c = x.doc.create_element("w:gridCol");
        x.doc.set_attribute(c, "w:w", "2400");
        x.doc.append_child(g, c);
    }
    x.doc.append_child(t, g);
    let mut cell_paragraphs = Vec::new();
    for _ in 0..row_count {
        let r = x.doc.create_element("w:tr");
        for _ in 0..column_count {
            let c = empty_cell(x);
            cell_paragraphs.push(x.doc.children(c).next().unwrap());
            x.doc.append_child(r, c);
        }
        x.doc.append_child(t, r);
    }
    match position {
        InsertPosition::Before => x.doc.insert_before(p, t),
        InsertPosition::After => x.insert_after_in_order(p, t),
    };
    // Every cell paragraph gets a durable id now, so later operations in the
    // same plan can address the cells (as `$alias_rNcM` or by id).
    let ids = super::typed::allocate_table_cell_para_ids(
        x,
        source,
        plan_seed,
        op_index,
        cell_paragraphs.len(),
    )
    .map_err(|message| Outcome::error(message))?;
    for (paragraph, id) in cell_paragraphs.iter().zip(&ids) {
        x.doc.set_attribute(*paragraph, "w14:paraId", id);
    }
    x.ensure_w14_declared();
    let mut filled = 0;
    for (row, contents) in cell_content.iter().enumerate() {
        for (column, with) in contents.iter().enumerate() {
            if with.is_empty() {
                continue;
            }
            let paragraph = cell_paragraphs[row * column_count as usize + column];
            super::typed::fill_paragraph_direct(package, x, paragraph, with).map_err(|mut outcome| {
                outcome.summary = format!("cells[{row}][{column}]: {}", outcome.summary);
                outcome
            })?;
            filled += 1;
        }
    }
    let cell_ids = ids
        .chunks(column_count as usize)
        .map(|row| json!(row))
        .collect::<Vec<_>>();
    if x.is_w(parent, "tc") && x.doc.children(parent).last() == Some(t) {
        let end = x.doc.create_element("w:p");
        x.doc.append_child(parent, end);
    }
    let ordinal = super::tables::tables(x)
        .iter()
        .position(|n| *n == t)
        .unwrap()
        + 1;
    let summary = if cell_content.is_empty() {
        format!("inserted table table:{ordinal}")
    } else {
        format!(
            "inserted table table:{ordinal} with {filled} filled cell{}",
            if filled == 1 { "" } else { "s" }
        )
    };
    Ok((
        summary,
        json!({"table":format!("table:{ordinal}"),"tracked":false,"cells":cell_ids,"filled":filled}),
    ))
}
