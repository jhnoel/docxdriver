//! Package-derived render context: hyperlink targets (from the document rels
//! part) and computed list markers. Built once per render; keyed by node id,
//! so it must be rebuilt after any mutation.
//!
//! The marker computation is a port of the reference engine's numbering module
//! (zebra `src/browser/docx-numbering.ts`): effective numId/ilvl from direct
//! `w:numPr` or the paragraph style's `w:basedOn` chain, counters chained by
//! **abstractNumId** (two `w:num` instances sharing an abstract list continue
//! one running count), `startOverride` restarting once per (numId, level)
//! instance, and `lvlOverride` honoured as a full level replacement. Shared
//! scope limits: raw `w:p` document order (no per-view recount around tracked
//! paragraph marks), no `lvlRestart` beyond the reset-deeper default, no
//! LISTNUM fields, no `numStyleLink` indirection. One deliberate deviation:
//! every bullet renders as the canonical `•`, not the symbol-font glyph.

use std::collections::{HashMap, HashSet};

use xmloxide::tree::NodeId;

use crate::document::DocxXml;
use crate::package::Package;

/// Numbering/styles paths siblings of the main document part (usually
/// `word/document.xml`, but OPC discovers the real name via the package rels).
struct PartPaths {
    numbering: String,
    styles: String,
}

fn part_paths(package: &Package) -> PartPaths {
    let main = package.document_part_name();
    let (dir, _) = main.rsplit_once('/').unwrap_or(("", main.as_str()));
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    PartPaths {
        numbering: format!("{prefix}numbering.xml"),
        styles: format!("{prefix}styles.xml"),
    }
}

pub struct RenderCtx {
    note_scope: String,
    /// w:hyperlink node → resolved href (relationship target or `#anchor`).
    pub links: HashMap<NodeId, String>,
    pub images: HashMap<String, String>,
    pub image_parts: HashMap<String, String>,
    pub styles: super::styles::Styles,
    pub math_limits: super::omml_mathml::Limits,
    pub lists: HashMap<NodeId, ListInfo>,
    /// w:p node → rendered list marker ("1.", "a)", "•", …).
    pub markers: HashMap<NodeId, String>,
    /// Footnote/endnote bodies for inline projection (empty for chrome parts).
    pub notes: super::surface::NoteBodies,
    /// w:p node → (canonical eight-digit `w14:paraId`, 1-based story ord).
    /// Engine-owned paragraph addresses; rendered as `id`/`ord` attributes.
    pub para_ids: HashMap<NodeId, (String, usize)>,
    /// Provisional paragraph-ID repairs needed by this part (deterministic;
    /// inspection reports them, a write persists them).
    pub repairs: usize,
}

impl RenderCtx {
    pub fn note_dom_id(&self, kind: &str, id: &str) -> String {
        format!("docx-{}-{kind}-{id}", self.note_scope)
    }

    /// [`RenderCtx::build_with_source`] without an input hash: provisional
    /// paragraph IDs then seed from the part's own bytes.
    pub fn build_with_source(
        package: &Package,
        xml: &DocxXml,
        source_hash: Option<&[u8; 32]>,
    ) -> RenderCtx {
        let main = package.document_part_name();
        RenderCtx::build_for_part(package, xml, &main, source_hash)
    }

    pub fn build_with_notes(
        package: &Package,
        xml: &DocxXml,
        notes: super::surface::NoteBodies,
        source_hash: Option<&[u8; 32]>,
    ) -> RenderCtx {
        let mut ctx = RenderCtx::build_with_source(package, xml, source_hash);
        ctx.notes = notes;
        ctx
    }

    /// Build context from a specific part's relationships (document, header,
    /// or footer). Images/links in headers resolve through that part's rels.
    pub fn build_for_part(
        package: &Package,
        xml: &DocxXml,
        part_name: &str,
        source_hash: Option<&[u8; 32]>,
    ) -> RenderCtx {
        let scope = xml.body().or_else(|| xml.doc.root_element());
        // Body document loads note bodies; chrome/note parts leave notes empty
        // (nested notes are not projected).
        let notes = if part_name.ends_with("document.xml") {
            super::surface::NoteBodies::load(
                package,
                super::render::RenderView::Markup,
                source_hash,
            )
        } else {
            super::surface::NoteBodies::default()
        };
        // Paragraph identity is scoped to the main document part: the typed
        // surface addresses paragraphs in word/document.xml only, so chrome
        // (header/footer) and note parts resolve no ids and render plain
        // `<p>` elements. Provisional repair is a pure function of the part
        // bytes and the input hash: when no input hash is available
        // (create-led lists), the part's own bytes seed the derivation.
        let (repairs, para_ids) = if part_name == package.document_part_name() {
            let seed_hash = match source_hash {
                Some(hash) => *hash,
                None => part_bytes_hash(package, part_name),
            };
            let index = xml.resolve_paragraph_ids(part_name, &seed_hash);
            (index.repairs, index.ids)
        } else {
            (0, HashMap::new())
        };
        let (images, image_parts) = super::assets::image_sources(package, part_name);
        let document = package.document_part_name();
        let note_scope = hex::encode(
            source_hash
                .copied()
                .unwrap_or_else(|| part_bytes_hash(package, &document)),
        )[..24]
            .to_string();
        RenderCtx {
            note_scope,
            links: link_targets(package, xml, part_name, scope),
            images,
            image_parts,
            styles: super::styles::Styles::load(package),
            math_limits: super::omml_mathml::Limits::load(package),
            lists: list_info(package, xml, scope),
            markers: list_markers(package, xml, scope),
            notes,
            repairs,
            para_ids,
        }
    }
}

/// sha256 of a part's bytes — the deterministic fallback seed when the exact
/// input package hash is not available.
fn part_bytes_hash(package: &Package, part_name: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    if let Some(bytes) = package.get(part_name) {
        hasher.update(bytes);
    }
    hasher.finalize().into()
}

fn rels_part_for(part_name: &str) -> String {
    let (dir, file) = part_name.rsplit_once('/').unwrap_or(("", part_name));
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    format!("{prefix}_rels/{file}.rels")
}

fn link_targets(
    package: &Package,
    xml: &DocxXml,
    part_name: &str,
    scope: Option<NodeId>,
) -> HashMap<NodeId, String> {
    let mut rels: HashMap<String, String> = HashMap::new();
    if let Some(bytes) = package.get(&rels_part_for(part_name)) {
        if let Ok(rels_xml) = DocxXml::parse(bytes) {
            let root = rels_xml.doc.root();
            for node in rels_xml.doc.descendants(root).collect::<Vec<_>>() {
                if rels_xml.is_local(node, "Relationship") {
                    if let (Some(id), Some(target)) =
                        (rels_xml.attr(node, "Id"), rels_xml.attr(node, "Target"))
                    {
                        rels.insert(id.to_string(), target.to_string());
                    }
                }
            }
        }
    }

    let mut links = HashMap::new();
    let Some(scope) = scope else { return links };
    for node in xml.doc.descendants(scope).collect::<Vec<_>>() {
        if !xml.is_w(node, "hyperlink") {
            continue;
        }
        let href = if let Some(rel_id) = xml.attr(node, "id") {
            rels.get(rel_id).cloned().unwrap_or_default()
        } else if let Some(anchor) = xml.attr(node, "anchor") {
            format!("#{anchor}")
        } else {
            String::new()
        };
        links.insert(node, href);
    }
    links
}

/// One numbering level definition (from abstractNum or an overriding lvl).
#[derive(Clone)]
struct LevelDef {
    fmt: String,
    text: String,
    start: i64,
}

/// A `w:num` instance: its abstract list plus per-level overrides.
struct NumDef {
    abstract_id: String,
    overrides: HashMap<u32, LevelOverride>,
}

#[derive(Default)]
struct LevelOverride {
    start_override: Option<i64>,
    level: Option<LevelDef>,
}

/// A paragraph style's numbering contribution (styles.xml).
#[derive(Default)]
struct StyleNumbering {
    num_id: Option<String>,
    ilvl: Option<u32>,
    based_on: Option<String>,
}

struct NumberingTables {
    abstracts: HashMap<String, HashMap<u32, LevelDef>>,
    nums: HashMap<String, NumDef>,
    styles: HashMap<String, StyleNumbering>,
}

impl NumberingTables {
    /// Effective level definition for (numId, level): a lvlOverride's nested
    /// w:lvl wins over the abstract definition.
    fn effective_level(&self, num_id: &str, level: u32) -> Option<&LevelDef> {
        let num = self.nums.get(num_id)?;
        if let Some(over) = num.overrides.get(&level) {
            if let Some(def) = &over.level {
                return Some(def);
            }
        }
        self.abstracts.get(&num.abstract_id)?.get(&level)
    }

    fn effective_start(&self, num_id: &str, level: u32) -> i64 {
        if let Some(start) = self
            .nums
            .get(num_id)
            .and_then(|num| num.overrides.get(&level))
            .and_then(|over| over.start_override)
        {
            return start;
        }
        self.effective_level(num_id, level)
            .map(|def| def.start)
            .unwrap_or(1)
    }

    /// A style's effective numbering, walking w:basedOn until a numId is
    /// found (bounded against inheritance cycles).
    fn resolve_style(&self, style_id: &str) -> Option<(String, u32)> {
        let mut current = style_id;
        for _ in 0..32 {
            let entry = self.styles.get(current)?;
            if let Some(num_id) = &entry.num_id {
                return Some((num_id.clone(), entry.ilvl.unwrap_or(0)));
            }
            current = entry.based_on.as_deref()?;
        }
        None
    }
}

fn parse_level(numbering: &DocxXml, lvl: NodeId) -> LevelDef {
    let mut def = LevelDef {
        fmt: "decimal".into(),
        text: String::new(),
        start: 1,
    };
    for child in numbering.doc.children(lvl) {
        if numbering.is_w(child, "numFmt") {
            if let Some(val) = numbering.attr(child, "val") {
                def.fmt = val.to_string();
            }
        } else if numbering.is_w(child, "lvlText") {
            if let Some(val) = numbering.attr(child, "val") {
                def.text = val.to_string();
            }
        } else if numbering.is_w(child, "start") {
            if let Some(val) = numbering.attr(child, "val").and_then(|v| v.parse().ok()) {
                def.start = val;
            }
        }
    }
    def
}

fn parse_numbering_part(
    package: &Package,
) -> (
    HashMap<String, HashMap<u32, LevelDef>>,
    HashMap<String, NumDef>,
) {
    let mut abstracts: HashMap<String, HashMap<u32, LevelDef>> = HashMap::new();
    let mut nums: HashMap<String, NumDef> = HashMap::new();
    let Some(bytes) = package.get(&part_paths(package).numbering) else {
        return (abstracts, nums);
    };
    let Ok(numbering) = DocxXml::parse(bytes) else {
        return (abstracts, nums);
    };

    let root = numbering.doc.root();
    for node in numbering.doc.descendants(root).collect::<Vec<_>>() {
        if numbering.is_w(node, "abstractNum") {
            let Some(id) = numbering.attr(node, "abstractNumId") else {
                continue;
            };
            let levels = abstracts.entry(id.to_string()).or_default();
            for lvl in numbering.doc.children(node).collect::<Vec<_>>() {
                if !numbering.is_w(lvl, "lvl") {
                    continue;
                }
                let Some(ilvl) = numbering.attr(lvl, "ilvl").and_then(|v| v.parse().ok()) else {
                    continue;
                };
                levels.insert(ilvl, parse_level(&numbering, lvl));
            }
        } else if numbering.is_w(node, "num") {
            let Some(num_id) = numbering.attr(node, "numId") else {
                continue;
            };
            let mut abstract_id = None;
            let mut overrides: HashMap<u32, LevelOverride> = HashMap::new();
            for child in numbering.doc.children(node).collect::<Vec<_>>() {
                if numbering.is_w(child, "abstractNumId") {
                    abstract_id = numbering.attr(child, "val").map(str::to_string);
                } else if numbering.is_w(child, "lvlOverride") {
                    let Some(ilvl) = numbering.attr(child, "ilvl").and_then(|v| v.parse().ok())
                    else {
                        continue;
                    };
                    let mut over = LevelOverride::default();
                    for inner in numbering.doc.children(child).collect::<Vec<_>>() {
                        if numbering.is_w(inner, "startOverride") {
                            over.start_override =
                                numbering.attr(inner, "val").and_then(|v| v.parse().ok());
                        } else if numbering.is_w(inner, "lvl") {
                            over.level = Some(parse_level(&numbering, inner));
                        }
                    }
                    overrides.insert(ilvl, over);
                }
            }
            if let Some(abstract_id) = abstract_id {
                nums.insert(
                    num_id.to_string(),
                    NumDef {
                        abstract_id,
                        overrides,
                    },
                );
            }
        }
    }
    (abstracts, nums)
}

/// styleId → numbering contribution, for paragraph styles (styles.xml).
fn parse_styles_part(package: &Package) -> HashMap<String, StyleNumbering> {
    let mut styles = HashMap::new();
    let Some(bytes) = package.get(&part_paths(package).styles) else {
        return styles;
    };
    let Ok(styles_xml) = DocxXml::parse(bytes) else {
        return styles;
    };

    let root = styles_xml.doc.root();
    for node in styles_xml.doc.descendants(root).collect::<Vec<_>>() {
        if !styles_xml.is_w(node, "style") || styles_xml.attr(node, "type") != Some("paragraph") {
            continue;
        }
        let Some(style_id) = styles_xml.attr(node, "styleId") else {
            continue;
        };
        let mut entry = StyleNumbering::default();
        for child in styles_xml.doc.children(node).collect::<Vec<_>>() {
            if styles_xml.is_w(child, "basedOn") {
                entry.based_on = styles_xml.attr(child, "val").map(str::to_string);
            } else if styles_xml.is_w(child, "pPr") {
                for prop in styles_xml.doc.children(child).collect::<Vec<_>>() {
                    if !styles_xml.is_w(prop, "numPr") {
                        continue;
                    }
                    for num_prop in styles_xml.doc.children(prop).collect::<Vec<_>>() {
                        if styles_xml.is_w(num_prop, "numId") {
                            entry.num_id = styles_xml.attr(num_prop, "val").map(str::to_string);
                        } else if styles_xml.is_w(num_prop, "ilvl") {
                            entry.ilvl = styles_xml
                                .attr(num_prop, "val")
                                .and_then(|v| v.parse().ok());
                        }
                    }
                }
            }
        }
        styles.insert(style_id.to_string(), entry);
    }
    styles
}

/// A paragraph's effective (numId, level): direct w:pPr/w:numPr with a numId
/// wins; otherwise the paragraph style's resolved numbering.
fn paragraph_numbering(
    xml: &DocxXml,
    tables: &NumberingTables,
    paragraph: NodeId,
) -> Option<(String, u32)> {
    let ppr = xml.doc.children(paragraph).find(|&n| xml.is_w(n, "pPr"))?;
    if let Some(numpr) = xml.doc.children(ppr).find(|&n| xml.is_w(n, "numPr")) {
        let mut num_id = None;
        let mut ilvl = 0u32;
        for child in xml.doc.children(numpr) {
            if xml.is_w(child, "numId") {
                num_id = xml.attr(child, "val").map(str::to_string);
            } else if xml.is_w(child, "ilvl") {
                ilvl = xml
                    .attr(child, "val")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
        }
        if let Some(num_id) = num_id {
            return Some((num_id, ilvl));
        }
    }
    let style_id = xml.paragraph_style(paragraph)?;
    tables.resolve_style(&style_id)
}

fn list_markers(
    package: &Package,
    xml: &DocxXml,
    scope: Option<NodeId>,
) -> HashMap<NodeId, String> {
    let mut markers = HashMap::new();
    let Some(scope) = scope else { return markers };
    let (abstracts, nums) = parse_numbering_part(package);
    if nums.is_empty() {
        return markers;
    }
    let tables = NumberingTables {
        abstracts,
        nums,
        styles: parse_styles_part(package),
    };

    // Counters chain by abstractNumId (falling back to numId if the instance
    // has no abstract ref); startOverride restarts once per (numId, level).
    let mut counters: HashMap<String, Vec<Option<i64>>> = HashMap::new();
    let mut applied_overrides: HashSet<(String, u32)> = HashSet::new();

    let paragraphs: Vec<NodeId> = xml
        .paragraphs_in(scope)
        .into_iter()
        .map(|paragraph| paragraph.node)
        .collect();
    for paragraph in paragraphs {
        let Some((num_id, level)) = paragraph_numbering(xml, &tables, paragraph) else {
            continue;
        };
        if num_id == "0" {
            continue; // explicit "no numbering"
        }
        if tables.effective_level(&num_id, level).is_none() {
            continue; // numId maps to no abstractNum / undefined level
        }

        let counter_key = tables
            .nums
            .get(&num_id)
            .map(|num| num.abstract_id.clone())
            .unwrap_or_else(|| num_id.clone());
        let slots = counters.entry(counter_key).or_default();
        if slots.len() <= level as usize {
            slots.resize(level as usize + 1, None);
        }

        let start_override = tables
            .nums
            .get(&num_id)
            .and_then(|num| num.overrides.get(&level))
            .and_then(|over| over.start_override);
        let override_key = (num_id.clone(), level);
        if let (Some(start), false) = (start_override, applied_overrides.contains(&override_key)) {
            // Explicit restart: this instance forces a fresh start at its
            // first paragraph, even if the shared abstract counter is running.
            slots[level as usize] = Some(start);
            applied_overrides.insert(override_key);
        } else if slots[level as usize].is_none() {
            slots[level as usize] = Some(tables.effective_start(&num_id, level));
        } else {
            slots[level as usize] = slots[level as usize].map(|v| v + 1);
        }
        // Restart all deeper levels so their next appearance starts fresh.
        for deeper in slots.iter_mut().skip(level as usize + 1) {
            *deeper = None;
        }

        let snapshot = slots.clone();
        let label = render_label(&tables, &num_id, level, &snapshot);
        if !label.is_empty() {
            markers.insert(paragraph, label);
        }
    }
    markers
}

/// Expand the level's lvlText, replacing %1..%9 (1-based → ilvl 0..8) with the
/// formatted counter of that level; an uncounted referenced level shows its
/// start value. Bullets canonicalize to `•` (the one deviation from the
/// reference engine, which emits the symbol-font glyph); `none` emits the
/// literal lvlText unsubstituted.
fn render_label(
    tables: &NumberingTables,
    num_id: &str,
    level: u32,
    counters: &[Option<i64>],
) -> String {
    let Some(def) = tables.effective_level(num_id, level) else {
        return String::new();
    };
    if def.fmt == "bullet" {
        return "•".to_string();
    }
    if def.fmt == "none" {
        return def.text.trim().to_string();
    }
    let mut out = String::new();
    let mut chars = def.text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        match chars.peek().and_then(|d| d.to_digit(10)) {
            Some(place) if place >= 1 => {
                chars.next();
                let referenced = place - 1;
                let Some(ref_def) = tables.effective_level(num_id, referenced) else {
                    continue;
                };
                let value = counters
                    .get(referenced as usize)
                    .copied()
                    .flatten()
                    .unwrap_or_else(|| tables.effective_start(num_id, referenced));
                out.push_str(&format_number(value, &ref_def.fmt));
            }
            _ => out.push('%'),
        }
    }
    out.trim().to_string()
}

fn format_number(value: i64, fmt: &str) -> String {
    match fmt {
        "decimalZero" if value < 10 => format!("0{value}"),
        "lowerLetter" => alpha(value),
        "upperLetter" => alpha(value).to_uppercase(),
        "lowerRoman" => roman(value),
        "upperRoman" => roman(value).to_uppercase(),
        // decimal and unknown formats (ordinal, cardinalText, …) → arabic.
        _ => value.to_string(),
    }
}

/// Spreadsheet-style letters: 1→a, 26→z, 27→aa … (reference-engine parity).
fn alpha(value: i64) -> String {
    if value <= 0 {
        return value.to_string();
    }
    let mut remaining = value;
    let mut out = Vec::new();
    while remaining > 0 {
        remaining -= 1;
        out.push(b'a' + (remaining % 26) as u8);
        remaining /= 26;
    }
    out.reverse();
    String::from_utf8(out).expect("ascii")
}

fn roman(mut value: i64) -> String {
    if value <= 0 {
        return value.to_string();
    }
    const TABLE: &[(i64, &str)] = &[
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for &(n, s) in TABLE {
        while value >= n {
            out.push_str(s);
            value -= n;
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct ListInfo {
    pub id: String,
    pub level: u32,
    pub format: String,
    pub value: i64,
}
fn list_info(package: &Package, xml: &DocxXml, scope: Option<NodeId>) -> HashMap<NodeId, ListInfo> {
    let (abstracts, nums) = parse_numbering_part(package);
    let tables = NumberingTables {
        abstracts,
        nums,
        styles: parse_styles_part(package),
    };
    let mut counters: HashMap<(String, u32), i64> = HashMap::new();
    let mut out = HashMap::new();
    if let Some(scope) = scope {
        for p in xml.paragraphs_in(scope) {
            if let Some((id, level)) = paragraph_numbering(xml, &tables, p.node) {
                if let Some(def) = tables.effective_level(&id, level) {
                    let count = counters
                        .entry((id.clone(), level))
                        .or_insert(tables.effective_start(&id, level) - 1);
                    *count += 1;
                    out.insert(
                        p.node,
                        ListInfo {
                            id,
                            level,
                            format: def.fmt.clone(),
                            value: *count,
                        },
                    );
                }
            }
        }
    }
    out
}
