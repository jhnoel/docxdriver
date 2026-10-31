//! Bounded table, row and logical-cell formatting with property snapshots.
use super::{layout::set_attr, ExecCtx};
use crate::{
    api::{CellFormat, CellPadding, RowFormat, TableBorders, TableFormat, TableWidth},
    document::DocxXml,
    outcome::Outcome,
    package::Package,
};
use serde_json::{json, Value};
use xmloxide::tree::NodeId;
const TBL: &[&str] = &[
    "tblStyle",
    "tblpPr",
    "tblOverlap",
    "bidiVisual",
    "tblStyleRowBandSize",
    "tblStyleColBandSize",
    "tblW",
    "jc",
    "tblCellSpacing",
    "tblInd",
    "tblBorders",
    "shd",
    "tblLayout",
    "tblCellMar",
    "tblLook",
    "tblCaption",
    "tblDescription",
    "tblPrChange",
];
const CELL: &[&str] = &[
    "cnfStyle",
    "tcW",
    "gridSpan",
    "hMerge",
    "vMerge",
    "tcBorders",
    "shd",
    "noWrap",
    "tcMar",
    "textDirection",
    "tcFitText",
    "vAlign",
    "hideMark",
    "headers",
    "cellIns",
    "cellDel",
    "cellMerge",
    "tcPrChange",
];
const ROW: &[&str] = &[
    "cnfStyle",
    "divId",
    "gridBefore",
    "gridAfter",
    "wBefore",
    "wAfter",
    "cantSplit",
    "trHeight",
    "tblHeader",
    "tblCellSpacing",
    "jc",
    "hidden",
    "ins",
    "del",
    "trPrChange",
];
const SIDES: &[&str] = &[
    "top", "start", "left", "bottom", "end", "right", "insideH", "insideV", "tl2br", "tr2bl",
];
fn ensure(x: &mut DocxXml, parent: NodeId, tag: &str, seq: &[&str]) -> NodeId {
    if let Some(n) = x.child_w(parent, tag) {
        return n;
    }
    let n = x.doc.create_element(&format!("w:{tag}"));
    let index = seq.iter().position(|s| *s == tag).unwrap_or(seq.len());
    let next = x.doc.children(parent).find(|&n| {
        seq.iter()
            .position(|s| x.is_w(n, s))
            .is_some_and(|i| i > index)
    });
    if let Some(next) = next {
        x.doc.insert_before(next, n);
    } else {
        x.doc.append_child(parent, n);
    }
    n
}
fn remove(x: &mut DocxXml, p: NodeId, tag: &str) {
    for n in x
        .doc
        .children(p)
        .filter(|&n| x.is_w(n, tag))
        .collect::<Vec<_>>()
    {
        x.doc.detach(n);
    }
}
fn val(x: &mut DocxXml, p: NodeId, tag: &str, value: &str, seq: &[&str]) {
    let n = ensure(x, p, tag, seq);
    set_attr(x, n, "w:val", value);
}
/// On/off row properties (tblHeader, cantSplit) are ST_OnOffOnly in the
/// Microsoft schema: write Word's bare element for on and "off" for off.
fn on_off(x: &mut DocxXml, p: NodeId, tag: &str, on: bool, seq: &[&str]) {
    let n = ensure(x, p, tag, seq);
    if on {
        x.doc.remove_attribute(n, "w:val");
        x.doc.remove_attribute(n, "val");
    } else {
        set_attr(x, n, "w:val", "off");
    }
}
fn number(part: &str, prefix: &str) -> Result<usize, String> {
    part.strip_prefix(prefix)
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("invalid table address component {part:?}"))
}
fn address(at: &str, depth: usize) -> Result<Vec<usize>, String> {
    let parts = at.split('/').collect::<Vec<_>>();
    if parts.len() != depth {
        return Err(format!(
            "expected {} address, got {at:?}",
            ["table:N", "table:N/row:N", "table:N/row:N/cell:N"][depth - 1]
        ));
    }
    parts
        .iter()
        .zip(["table:", "row:", "cell:"])
        .map(|(p, prefix)| {
            if prefix == "table:"
                && p.starts_with('$')
                && p.len() > 1
                && p[1..]
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                Ok(1)
            } else {
                number(p, prefix)
            }
        })
        .collect()
}
pub(crate) fn tables(x: &DocxXml) -> Vec<NodeId> {
    x.body()
        .map(|b| x.doc.descendants(b).filter(|&n| x.is_w(n, "tbl")).collect())
        .unwrap_or_default()
}
pub(crate) fn rows(x: &DocxXml, t: NodeId) -> Vec<NodeId> {
    x.doc.children(t).filter(|&n| x.is_w(n, "tr")).collect()
}
pub(crate) fn cells(x: &DocxXml, r: NodeId) -> Vec<NodeId> {
    x.doc.children(r).filter(|&n| x.is_w(n, "tc")).collect()
}
pub(crate) fn resolve(x: &DocxXml, at: &str, depth: usize) -> Result<NodeId, Outcome> {
    let ids = address(at, depth).map_err(Outcome::error)?;
    let mut n = *tables(x)
        .get(ids[0] - 1)
        .ok_or_else(|| Outcome::error(format!("table address {at:?} is out of range")))?;
    if depth > 1 {
        n = *rows(x, n)
            .get(ids[1] - 1)
            .ok_or_else(|| Outcome::error(format!("row address {at:?} is out of range")))?;
    }
    if depth > 2 {
        n = *cells(x, n)
            .get(ids[2] - 1)
            .ok_or_else(|| Outcome::error(format!("cell address {at:?} is out of range")))?;
        if x.child_w(n, "tcPr")
            .and_then(|p| x.child_w(p, "vMerge"))
            .is_some_and(|v| x.attr(v, "val") != Some("restart"))
        {
            return Err(Outcome::blocked(format!(
                "{at} is a physical vertical-merge continuation; address its logical restart cell"
            )));
        }
        if x.child_w(n, "tcPr")
            .and_then(|p| x.child_w(p, "hMerge"))
            .is_some()
        {
            return Err(Outcome::blocked(format!(
                "{at} uses legacy hMerge; normalize to gridSpan before editing"
            )));
        }
    }
    Ok(n)
}
fn dimension_valid(value: crate::api::Points) -> Result<(), String> {
    if !(0..=i32::MAX as i64).contains(&value.0) {
        return Err("table dimensions must be nonnegative and fit signed 32-bit twips".into());
    }
    Ok(())
}
fn padding_valid(value: &Option<CellPadding>) -> Result<(), String> {
    if let Some(v) = value {
        for p in [v.top, v.right, v.bottom, v.left].into_iter().flatten() {
            dimension_valid(p)?;
        }
    }
    Ok(())
}
fn width_valid(w: &Option<TableWidth>) -> Result<(), String> {
    if let Some(TableWidth::Points { value }) = w {
        dimension_valid(*value)?;
    }
    if let Some(TableWidth::Percent { value }) = w {
        if !value.is_finite() || *value <= 0.0 || *value > 100.0 {
            return Err("table percentage width must be > 0 and <= 100".into());
        }
    }
    Ok(())
}
fn color_valid(c: &str) -> bool {
    c.len() == 7
        && c.starts_with('#')
        && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
        && c == c.to_uppercase()
}
fn borders_valid(b: &Option<TableBorders>) -> Result<(), String> {
    if let Some(b) = b {
        for v in [
            &b.top,
            &b.left,
            &b.bottom,
            &b.right,
            &b.inside_horizontal,
            &b.inside_vertical,
        ]
        .into_iter()
        .flatten()
        {
            if ![
                "nil",
                "none",
                "single",
                "double",
                "dotted",
                "dashed",
                "dotDash",
                "dotDotDash",
                "triple",
                "wave",
                "doubleWave",
            ]
            .contains(&v.style.as_str())
            {
                return Err("unsupported table border style".into());
            }
            if v.color.as_ref().is_some_and(|c| !color_valid(c)) {
                return Err("border color must be #RRGGBB".into());
            }
            if v.width
                .is_some_and(|w| w.as_points() > 12.0 || w.as_points() < 0.25 || w.0 % 5 != 0)
            {
                return Err(
                    "border width must be 0.25..12 points in quarter-point increments".into(),
                );
            }
        }
    }
    Ok(())
}
fn clear_valid(clear: &[String], names: &[&str]) -> Result<(), String> {
    if let Some(v) = clear.iter().find(|v| !names.contains(&v.as_str())) {
        return Err(format!("unknown clear property {v:?}"));
    }
    Ok(())
}
pub fn validate_table(at: &str, f: &TableFormat) -> Result<(), String> {
    address(at, 1)?;
    padding_valid(&f.padding)?;
    if let Some(v) = f.cell_spacing {
        dimension_valid(v)?;
    }
    if let Some(widths) = &f.column_widths {
        for w in widths {
            dimension_valid(*w)?;
        }
    }
    if f.clear.iter().any(|c| c == "column_widths") {
        return Err("column_widths cannot be cleared: the grid must retain its dimensions".into());
    }
    width_valid(&f.width)?;
    borders_valid(&f.borders)?;
    clear_valid(
        &f.clear,
        &[
            "width",
            "column_widths",
            "layout",
            "alignment",
            "borders",
            "cell_spacing",
            "padding",
            "style",
        ],
    )?;
    if f.layout
        .as_ref()
        .is_some_and(|v| !["autofit", "fixed"].contains(&v.as_str()))
    {
        return Err("layout must be autofit or fixed".into());
    }
    if f.alignment
        .as_ref()
        .is_some_and(|v| !["left", "center", "right"].contains(&v.as_str()))
    {
        return Err("table alignment must be left, center or right".into());
    }
    if f.column_widths
        .as_ref()
        .is_some_and(|v| v.is_empty() || v.iter().any(|w| w.as_points() <= 0.0))
    {
        return Err("column_widths must contain positive point widths".into());
    }
    if f == &TableFormat::default() {
        return Err("format_table requires a property".into());
    }
    Ok(())
}
pub fn validate_cell(at: &str, f: &CellFormat) -> Result<(), String> {
    address(at, 3)?;
    padding_valid(&f.padding)?;
    width_valid(&f.width)?;
    borders_valid(&f.borders)?;
    clear_valid(
        &f.clear,
        &["width", "borders", "fill", "padding", "vertical_alignment"],
    )?;
    if f.fill.as_ref().is_some_and(|c| !color_valid(c)) {
        return Err("cell fill must be #RRGGBB".into());
    }
    if f.vertical_alignment
        .as_ref()
        .is_some_and(|v| !["top", "center", "bottom"].contains(&v.as_str()))
    {
        return Err("vertical_alignment must be top, center or bottom".into());
    }
    if f == &CellFormat::default() {
        return Err("format_cell requires a property".into());
    }
    Ok(())
}
pub fn validate_row(at: &str, f: &RowFormat) -> Result<(), String> {
    address(at, 2)?;
    if let Some(v) = f.height {
        dimension_valid(v)?;
    }
    clear_valid(
        &f.clear,
        &["height", "height_rule", "repeating_header", "allow_split"],
    )?;
    if f.height_rule
        .as_ref()
        .is_some_and(|v| !["auto", "at_least", "exact"].contains(&v.as_str()))
    {
        return Err("height_rule must be auto, at_least or exact".into());
    }
    if f == &RowFormat::default() {
        return Err("format_row requires a property".into());
    }
    Ok(())
}
/// The properties element of a table, row or cell. It precedes all content —
/// including cells wrapped in w:sdt or w:customXml, which `ensure` would not
/// recognize — and follows only w:tblPrEx in a row.
fn bag(x: &mut DocxXml, n: NodeId, tag: &str) -> NodeId {
    if let Some(existing) = x.child_w(n, tag) {
        return existing;
    }
    let props = x.doc.create_element(&format!("w:{tag}"));
    let next = x
        .doc
        .children(n)
        .find(|&c| x.doc.is_element(c) && !(tag == "trPr" && x.is_w(c, "tblPrEx")));
    match next {
        Some(next) => x.doc.insert_before(next, props),
        None => x.doc.append_child(n, props),
    }
    props
}
fn snapshot(
    x: &mut DocxXml,
    p: NodeId,
    tag: &str,
    tracked: bool,
    author: &str,
    ctx: &ExecCtx,
) -> Result<(), Outcome> {
    let change = format!("{tag}Change");
    if x.child_w(p, &change).is_some() {
        return Err(Outcome::blocked(format!(
            "{tag} has a pending change; accept or reject it first"
        )));
    }
    if !tracked {
        return Ok(());
    }
    if tag == "trPr" && ["ins", "del"].iter().any(|t| x.child_w(p, t).is_some()) {
        return Err(Outcome::blocked("row has a pending structural revision"));
    }
    let old = x.doc.create_element(&format!("w:{tag}"));
    for n in x.doc.children(p).collect::<Vec<_>>() {
        let copy = x.doc.clone_node(n, true);
        x.doc.append_child(old, copy);
    }
    let id = x.next_revision_id();
    let n = x.doc.create_element(&format!("w:{change}"));
    set_attr(x, n, "w:id", &id.to_string());
    if tag != "tblGrid" {
        set_attr(x, n, "w:author", author);
        set_attr(x, n, "w:date", &ctx.now);
    }
    x.doc.append_child(n, old);
    x.doc.append_child(p, n);
    Ok(())
}
fn write_width(x: &mut DocxXml, p: NodeId, tag: &str, w: &TableWidth, seq: &[&str]) {
    let n = ensure(x, p, tag, seq);
    let (kind, value) = match w {
        TableWidth::Auto => ("auto", 0),
        TableWidth::Points { value } => ("dxa", (value.as_points() * 20.0).round() as i64),
        TableWidth::Percent { value } => ("pct", (value * 50.0).round() as i64),
    };
    set_attr(x, n, "w:type", kind);
    set_attr(x, n, "w:w", &value.to_string());
}
fn padding(x: &mut DocxXml, p: NodeId, tag: &str, f: &CellPadding, seq: &[&str]) {
    let n = ensure(x, p, tag, seq);
    for (side, w) in [
        ("top", f.top),
        ("left", f.left),
        ("bottom", f.bottom),
        ("right", f.right),
    ] {
        if let Some(w) = w {
            write_width(x, n, side, &TableWidth::Points { value: w }, SIDES);
        }
    }
}
fn borders(x: &mut DocxXml, p: NodeId, tag: &str, f: &TableBorders, seq: &[&str]) {
    let n = ensure(x, p, tag, seq);
    for (side, b) in [
        ("top", &f.top),
        ("left", &f.left),
        ("bottom", &f.bottom),
        ("right", &f.right),
        ("insideH", &f.inside_horizontal),
        ("insideV", &f.inside_vertical),
    ] {
        if let Some(b) = b {
            let s = ensure(x, n, side, SIDES);
            set_attr(x, s, "w:val", &b.style);
            if let Some(c) = &b.color {
                set_attr(x, s, "w:color", &c[1..]);
                for a in ["themeColor", "themeTint", "themeShade"] {
                    x.doc.remove_attribute(s, &format!("w:{a}"));
                    x.doc.remove_attribute(s, a);
                }
            }
            if let Some(w) = b.width {
                set_attr(
                    x,
                    s,
                    "w:sz",
                    &((w.as_points() * 8.0).round() as i64).to_string(),
                );
            }
        }
    }
}
fn clears(x: &mut DocxXml, p: NodeId, clear: &[String], map: &[(&str, &str)]) {
    for (field, tag) in map {
        if clear.iter().any(|v| v == field) {
            remove(x, p, tag);
        }
    }
}
pub fn format_table(
    pkg: Option<&Package>,
    x: &mut DocxXml,
    at: &str,
    f: &TableFormat,
    tracked: bool,
    author: &str,
    ctx: &ExecCtx,
) -> Result<(String, Value), Outcome> {
    validate_table(at, f).map_err(Outcome::error)?;
    let t = resolve(x, at, 1)?;
    if let Some(style) = &f.style {
        let bytes = pkg
            .and_then(|p| p.get("word/styles.xml"))
            .ok_or_else(|| Outcome::error("document has no table styles"))?;
        let sx = DocxXml::parse(bytes).map_err(|e| Outcome::error(e.to_string()))?;
        if !sx.doc.descendants(sx.doc.root()).any(|n| {
            sx.is_w(n, "style")
                && sx.attr(n, "type") == Some("table")
                && sx.attr(n, "styleId") == Some(style)
        }) {
            return Err(Outcome::error(format!("table style {style:?} not found")));
        }
    }
    if let Some(widths) = &f.column_widths {
        let grid = x
            .child_w(t, "tblGrid")
            .ok_or_else(|| Outcome::error("table has no grid"))?;
        let cols = x
            .doc
            .children(grid)
            .filter(|&n| x.is_w(n, "gridCol"))
            .collect::<Vec<_>>();
        if cols.len() != widths.len() {
            return Err(Outcome::error(format!(
                "column_widths has {} values; table grid has {} columns",
                widths.len(),
                cols.len()
            )));
        }
        snapshot(x, grid, "tblGrid", tracked, author, ctx)?;
        for (n, w) in cols.into_iter().zip(widths) {
            set_attr(
                x,
                n,
                "w:w",
                &((w.as_points() * 20.0).round() as i64).to_string(),
            );
        }
    }
    if f.clear.iter().any(|c| c == "column_widths") {
        return Err(Outcome::error(
            "column_widths cannot be cleared: the grid must retain its dimensions",
        ));
    }
    let p = bag(x, t, "tblPr");
    snapshot(x, p, "tblPr", tracked, author, ctx)?;
    if let Some(w) = &f.width {
        write_width(x, p, "tblW", w, TBL);
    }
    if let Some(v) = &f.layout {
        let n = ensure(x, p, "tblLayout", TBL);
        set_attr(x, n, "w:type", v);
    }
    if let Some(v) = &f.alignment {
        val(x, p, "jc", v, TBL);
    }
    if let Some(v) = &f.style {
        val(x, p, "tblStyle", v, TBL);
    }
    if let Some(v) = &f.padding {
        padding(x, p, "tblCellMar", v, TBL);
    }
    if let Some(v) = &f.borders {
        borders(x, p, "tblBorders", v, TBL);
    }
    if let Some(v) = f.cell_spacing {
        write_width(
            x,
            p,
            "tblCellSpacing",
            &TableWidth::Points { value: v },
            TBL,
        );
    }
    clears(
        x,
        p,
        &f.clear,
        &[
            ("width", "tblW"),
            ("layout", "tblLayout"),
            ("alignment", "jc"),
            ("borders", "tblBorders"),
            ("padding", "tblCellMar"),
            ("cell_spacing", "tblCellSpacing"),
            ("style", "tblStyle"),
        ],
    );
    Ok((
        format!("formatted {at}"),
        json!({"at":at,"tracked":tracked}),
    ))
}
pub fn format_row(
    x: &mut DocxXml,
    at: &str,
    f: &RowFormat,
    tracked: bool,
    author: &str,
    ctx: &ExecCtx,
) -> Result<(String, Value), Outcome> {
    validate_row(at, f).map_err(Outcome::error)?;
    let r = resolve(x, at, 2)?;
    let p = bag(x, r, "trPr");
    snapshot(x, p, "trPr", tracked, author, ctx)?;
    if let Some(v) = f.height {
        let n = ensure(x, p, "trHeight", ROW);
        set_attr(
            x,
            n,
            "w:val",
            &((v.as_points() * 20.0).round() as i64).to_string(),
        );
    }
    if let Some(v) = &f.height_rule {
        let n = ensure(x, p, "trHeight", ROW);
        if x.attr(n, "val").is_none() {
            set_attr(x, n, "w:val", "0");
        }
        set_attr(x, n, "w:hRule", if v == "at_least" { "atLeast" } else { v });
    }
    if let Some(v) = f.repeating_header {
        on_off(x, p, "tblHeader", v, ROW);
    }
    if let Some(v) = f.allow_split {
        on_off(x, p, "cantSplit", !v, ROW);
    }
    clears(
        x,
        p,
        &f.clear,
        &[
            ("height", "trHeight"),
            ("repeating_header", "tblHeader"),
            ("allow_split", "cantSplit"),
        ],
    );
    if f.clear.iter().any(|c| c == "height_rule") {
        if let Some(n) = x.child_w(p, "trHeight") {
            x.doc.remove_attribute(n, "w:hRule");
            x.doc.remove_attribute(n, "hRule");
        }
    }
    Ok((
        format!("formatted {at}"),
        json!({"at":at,"tracked":tracked}),
    ))
}
pub fn format_cell(
    x: &mut DocxXml,
    at: &str,
    f: &CellFormat,
    tracked: bool,
    author: &str,
    ctx: &ExecCtx,
) -> Result<(String, Value), Outcome> {
    validate_cell(at, f).map_err(Outcome::error)?;
    let c = resolve(x, at, 3)?;
    let backing = logical_cells(x, c)?;
    for c in backing {
        let p = bag(x, c, "tcPr");
        snapshot(x, p, "tcPr", tracked, author, ctx)?;
        if let Some(v) = &f.width {
            write_width(x, p, "tcW", v, CELL);
        }
        if let Some(v) = &f.borders {
            borders(x, p, "tcBorders", v, CELL);
        }
        if let Some(v) = &f.padding {
            padding(x, p, "tcMar", v, CELL);
        }
        if let Some(v) = &f.fill {
            let n = ensure(x, p, "shd", CELL);
            if x.attr(n, "val").is_none() { set_attr(x, n, "w:val", "clear"); }
            set_attr(x, n, "w:fill", &v[1..]);
            for a in ["themeFill", "themeFillTint", "themeFillShade"] {
                x.doc.remove_attribute(n, &format!("w:{a}"));
                x.doc.remove_attribute(n, a);
            }
        }
        if let Some(v) = &f.vertical_alignment {
            val(x, p, "vAlign", v, CELL);
        }
        clears(
            x,
            p,
            &f.clear,
            &[
                ("width", "tcW"),
                ("borders", "tcBorders"),
                ("fill", "shd"),
                ("padding", "tcMar"),
                ("vertical_alignment", "vAlign"),
            ],
        );
    }
    Ok((
        format!("formatted {at}"),
        json!({"at":at,"tracked":tracked}),
    ))
}
fn properties(x: &DocxXml, n: NodeId, tag: &str) -> Value {
    let mut props = serde_json::Map::new();
    if let Some(p) = x.child_w(n, tag) {
        for c in x.doc.children(p) {
            let name = x
                .doc
                .node_name(c)
                .unwrap_or("")
                .rsplit(':')
                .next()
                .unwrap_or("");
            if name.ends_with("Change") {
                continue;
            }
            let attrs: serde_json::Map<String, Value> = x
                .doc
                .attributes(c)
                .iter()
                .map(|a| (a.name.clone(), json!(a.value)))
                .collect();
            props.insert(name.into(), Value::Object(attrs));
        }
    }
    Value::Object(props)
}
pub fn inventory(x: &DocxXml) -> Value {
    Value::Array(tables(x).into_iter().enumerate().map(|(i,t)|{let at=format!("table:{}",i+1);let grid=x.child_w(t,"tblGrid").map(|g|x.doc.children(g).filter(|&n|x.is_w(n,"gridCol")).map(|n|x.attr(n,"w").and_then(|v|v.parse::<i64>().ok())).collect::<Vec<_>>()).unwrap_or_default();let rows=rows(x,t).into_iter().enumerate().map(|(ri,r)|{let ra=format!("{at}/row:{}",ri+1);let cells=cells(x,r).into_iter().enumerate().map(|(ci,c)|{let p=x.child_w(c,"tcPr");let span=p.and_then(|p|x.child_w(p,"gridSpan")).and_then(|n|x.attr(n,"val")).and_then(|v|v.parse::<usize>().ok()).unwrap_or(1);let merge=p.and_then(|p|x.child_w(p,"vMerge")).map(|n|x.attr(n,"val").unwrap_or("continue"));json!({"at":format!("{ra}/cell:{}",ci+1),"physical_index":ci+1,"grid_span":span,"vertical_merge":merge,"logical":merge!=Some("continue"),"properties":properties(x,c,"tcPr")})}).collect::<Vec<_>>();json!({"at":ra,"properties":properties(x,r,"trPr"),"cells":cells})}).collect::<Vec<_>>();json!({"at":at,"grid_twips":grid,"properties":properties(x,t,"tblPr"),"rows":rows})}).collect())
}

/// The inventory without what positions already give: row and cell handles
/// and physical indexes follow from array order (`table:T/row:R/cell:C`, all
/// one-based), `grid_span` defaults to 1, `logical` to true, and absent
/// `vertical_merge`/empty `properties` are omitted.
pub fn agent_inventory(x: &DocxXml) -> Value {
    let mut tables = inventory(x);
    for table in tables.as_array_mut().into_iter().flatten() {
        for row in table["rows"].as_array_mut().into_iter().flatten() {
            let row = row.as_object_mut().unwrap();
            row.remove("at");
            for cell in row["cells"].as_array_mut().into_iter().flatten() {
                let cell = cell.as_object_mut().unwrap();
                cell.remove("at");
                cell.remove("physical_index");
                if cell.get("grid_span") == Some(&json!(1)) {
                    cell.remove("grid_span");
                }
                if cell.get("logical") == Some(&json!(true)) {
                    cell.remove("logical");
                }
            }
        }
    }
    super::prune_empty(tables)
}

/// Render supported direct table geometry and formatting. Source metadata remains
/// in the read inventory; CSS communicates intent rather than pagination.
pub(crate) fn css(x: &DocxXml, n: NodeId, tag: &str, original: bool) -> String {
    let Some(mut p) = x.child_w(n, tag) else {
        return String::new();
    };
    if original {
        if let Some(old) = x
            .child_w(p, &format!("{tag}Change"))
            .and_then(|c| x.child_w(c, tag))
        {
            p = old;
        }
    }
    let mut out = String::new();
    for c in x.doc.children(p) {
        let name = x
            .doc
            .node_name(c)
            .unwrap_or("")
            .rsplit(':')
            .next()
            .unwrap_or("");
        let v = x.attr(c, "val").unwrap_or("");
        match name {
            "tblW" | "tcW" => {
                if let Some(w) = x
                    .attr(c, "w")
                    .and_then(|v| v.parse::<f64>().ok())
                    .filter(|v| v.is_finite() && *v >= 0.0)
                {
                    match x.attr(c, "type") {
                        Some("dxa") => out.push_str(&format!("width:{}pt;", w / 20.0)),
                        Some("pct") => out.push_str(&format!("width:{}%;", w / 50.0)),
                        Some("auto") => out.push_str("width:auto;"),
                        _ => {}
                    }
                }
            }
            "tblLayout" => {
                if matches!(x.attr(c, "type"), Some("fixed" | "autofit")) {
                    out.push_str(if x.attr(c, "type") == Some("fixed") {
                        "table-layout:fixed;"
                    } else {
                        "table-layout:auto;"
                    });
                }
            }
            "jc" => match v {
                "center" => out.push_str("margin-left:auto;margin-right:auto;"),
                "right" => out.push_str("margin-left:auto;margin-right:0;"),
                "left" => out.push_str("margin-left:0;margin-right:auto;"),
                _ => {}
            },
            "shd" => {
                if let Some(v) = x
                    .attr(c, "fill")
                    .filter(|v| v.len() == 6 && v.bytes().all(|b| b.is_ascii_hexdigit()))
                {
                    out.push_str(&format!("background-color:#{v};"));
                }
            }
            "vAlign" => {
                if ["top", "center", "bottom"].contains(&v) {
                    out.push_str(&format!(
                        "vertical-align:{};",
                        if v == "center" { "middle" } else { v }
                    ));
                }
            }
            "trHeight" => {
                if let Some(v) = x
                    .attr(c, "val")
                    .and_then(|v| v.parse::<f64>().ok())
                    .filter(|v| v.is_finite() && *v >= 0.0)
                {
                    out.push_str(&format!("height:{}pt;", v / 20.0));
                }
            }
            "cantSplit" => {
                if !matches!(v, "0" | "false" | "off") {
                    out.push_str("break-inside:avoid;");
                }
            }
            "tblCellSpacing" => {
                if let Some(v) = x
                    .attr(c, "w")
                    .and_then(|v| v.parse::<f64>().ok())
                    .filter(|v| v.is_finite() && *v >= 0.0)
                {
                    out.push_str(&format!(
                        "border-collapse:separate;border-spacing:{}pt;",
                        v / 20.0
                    ));
                }
            }
            "tcMar" | "tblCellMar" => {
                for side in ["top", "left", "bottom", "right"] {
                    if let Some(v) = x
                        .child_w(c, side)
                        .and_then(|s| x.attr(s, "w"))
                        .and_then(|v| v.parse::<f64>().ok())
                        .filter(|v| v.is_finite() && *v >= 0.0)
                    {
                        out.push_str(&format!("padding-{side}:{}pt;", v / 20.0));
                    }
                }
            }
            "tcBorders" | "tblBorders" => {
                for side in ["top", "left", "bottom", "right"] {
                    if let Some(b) = x.child_w(c, side) {
                        let style = match x.attr(b, "val") {
                            Some("nil" | "none") => "none",
                            Some("double") => "double",
                            Some("dotted") => "dotted",
                            Some("dashed" | "dotDash" | "dotDotDash") => "dashed",
                            Some("single" | "triple" | "wave" | "doubleWave") => "solid",
                            _ => continue,
                        };
                        let width = x
                            .attr(b, "sz")
                            .and_then(|v| v.parse::<u32>().ok())
                            .map(|v| v as f64 / 8.0)
                            .unwrap_or(0.5);
                        let color = x
                            .attr(b, "color")
                            .filter(|v| v.len() == 6 && v.bytes().all(|b| b.is_ascii_hexdigit()))
                            .map(|v| format!("#{v}"))
                            .unwrap_or_else(|| "currentColor".into());
                        out.push_str(&format!("border-{side}:{width}pt {style} {color};"));
                    }
                }
            }
            _ => {}
        }
    }
    out
}
pub(crate) fn render_attrs(
    x: &DocxXml,
    n: NodeId,
    tag: &str,
    original: bool,
    handle: Option<&String>,
) -> String {
    let mut out = String::new();
    if let Some(handle) = handle {
        out.push_str(&format!(" data-docx-handle=\"{handle}\""));
    }
    let css = css(x, n, tag, original);
    if !css.is_empty() {
        out.push_str(&format!(" style=\"{}\"", crate::html::escape_attr(&css)));
    }
    out
}

fn span(x: &DocxXml, c: NodeId) -> usize {
    x.child_w(c, "tcPr")
        .and_then(|p| x.child_w(p, "gridSpan"))
        .and_then(|n| x.attr(n, "val"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}
fn logical_cells(x: &DocxXml, c: NodeId) -> Result<Vec<NodeId>, Outcome> {
    let mut result = vec![c];
    if !x
        .child_w(c, "tcPr")
        .and_then(|p| x.child_w(p, "vMerge"))
        .is_some_and(|n| x.attr(n, "val") == Some("restart"))
    {
        return Ok(result);
    }
    let row = x
        .doc
        .parent(c)
        .ok_or_else(|| Outcome::error("cell has no row"))?;
    let table = x
        .doc
        .parent(row)
        .ok_or_else(|| Outcome::error("row has no table"))?;
    let start = cells(x, row)
        .into_iter()
        .take_while(|n| *n != c)
        .map(|n| span(x, n))
        .sum::<usize>();
    let rs = rows(x, table);
    let ri = rs.iter().position(|n| *n == row).unwrap();
    for r in rs.into_iter().skip(ri + 1) {
        let mut col = 0;
        let mut next = None;
        for cell in cells(x, r) {
            if col == start {
                next = Some(cell);
                break;
            }
            col += span(x, cell);
        }
        let Some(next) = next else { break };
        let continued = x
            .child_w(next, "tcPr")
            .and_then(|p| x.child_w(p, "vMerge"))
            .is_some_and(|n| x.attr(n, "val") != Some("restart"));
        if !continued {
            break;
        }
        if span(x, next) != span(x, c) {
            return Err(Outcome::blocked(
                "vertical merge continuation has a conflicting grid span",
            ));
        }
        result.push(next);
    }
    Ok(result)
}
