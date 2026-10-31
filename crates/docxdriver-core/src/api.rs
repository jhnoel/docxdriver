//! The typed, source-bound request API.
//!
//! This module is deliberately independent of the historical JSON command
//! dispatcher.  JSON and TOML are only adapters for these values; execution
//! uses the same normalized operation list for both representations.

use schemars::{schema::Schema, JsonSchema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// The exact ECMA-376 ST_HighlightColor palette; None is an explicit off override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum HighlightColor {
    Black,
    Blue,
    Cyan,
    Green,
    Magenta,
    Red,
    Yellow,
    White,
    DarkBlue,
    DarkCyan,
    DarkGreen,
    DarkMagenta,
    DarkRed,
    DarkYellow,
    DarkGray,
    LightGray,
    None,
}
impl HighlightColor {
    pub fn as_str(self) -> &'static str { match self {
        Self::Black => "black",
        Self::Blue => "blue",
        Self::Cyan => "cyan",
        Self::Green => "green",
        Self::Magenta => "magenta",
        Self::Red => "red",
        Self::Yellow => "yellow",
        Self::White => "white",
        Self::DarkBlue => "darkBlue",
        Self::DarkCyan => "darkCyan",
        Self::DarkGreen => "darkGreen",
        Self::DarkMagenta => "darkMagenta",
        Self::DarkRed => "darkRed",
        Self::DarkYellow => "darkYellow",
        Self::DarkGray => "darkGray",
        Self::LightGray => "lightGray",
        Self::None => "none",
    }}
    pub fn css(self) -> &'static str { match self {
        Self::Black => "#000000",
        Self::Blue => "#0000FF",
        Self::Cyan => "#00FFFF",
        Self::Green => "#00FF00",
        Self::Magenta => "#FF00FF",
        Self::Red => "#FF0000",
        Self::Yellow => "#FFFF00",
        Self::White => "#FFFFFF",
        Self::DarkBlue => "#000080",
        Self::DarkCyan => "#008080",
        Self::DarkGreen => "#008000",
        Self::DarkMagenta => "#800080",
        Self::DarkRed => "#800000",
        Self::DarkYellow => "#808000",
        Self::DarkGray => "#808080",
        Self::LightGray => "#C0C0C0",
        Self::None => "transparent",
    }}
    pub fn parse(value: &str) -> Result<Self, String> { match value {
        "black" => Ok(Self::Black),
        "blue" => Ok(Self::Blue),
        "cyan" => Ok(Self::Cyan),
        "green" => Ok(Self::Green),
        "magenta" => Ok(Self::Magenta),
        "red" => Ok(Self::Red),
        "yellow" => Ok(Self::Yellow),
        "white" => Ok(Self::White),
        "darkBlue" => Ok(Self::DarkBlue),
        "darkCyan" => Ok(Self::DarkCyan),
        "darkGreen" => Ok(Self::DarkGreen),
        "darkMagenta" => Ok(Self::DarkMagenta),
        "darkRed" => Ok(Self::DarkRed),
        "darkYellow" => Ok(Self::DarkYellow),
        "darkGray" => Ok(Self::DarkGray),
        "lightGray" => Ok(Self::LightGray),
        "none" => Ok(Self::None),
        _ => Err(format!("unknown highlight palette value {value:?}")),
    }}
    /// The palette entry whose CSS color is `value`, as reads write it.
    pub fn from_css(value: &str) -> Result<Self, String> {
        const ALL: [HighlightColor; 17] = [
            HighlightColor::Black, HighlightColor::Blue, HighlightColor::Cyan,
            HighlightColor::Green, HighlightColor::Magenta, HighlightColor::Red,
            HighlightColor::Yellow, HighlightColor::White, HighlightColor::DarkBlue,
            HighlightColor::DarkCyan, HighlightColor::DarkGreen, HighlightColor::DarkMagenta,
            HighlightColor::DarkRed, HighlightColor::DarkYellow, HighlightColor::DarkGray,
            HighlightColor::LightGray, HighlightColor::None,
        ];
        ALL.into_iter()
            .find(|color| color.css().eq_ignore_ascii_case(value))
            .ok_or_else(|| format!("background-color {value:?} is not a Word highlight color; use one of the highlight palette's colors"))
    }
}

/// Table widths use auto, points, or percent (0 < value <= 100 for percent).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "unit", rename_all = "snake_case", deny_unknown_fields)]
pub enum TableWidth { Auto, Points { value: Points }, Percent { value: f64 } }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TableBorder {
    pub style: String,
    #[serde(default)] pub color: Option<String>,
    #[serde(default)] pub width: Option<Points>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct TableBorders {
    #[serde(default)] pub top: Option<TableBorder>,
    #[serde(default)] pub left: Option<TableBorder>,
    #[serde(default)] pub bottom: Option<TableBorder>,
    #[serde(default)] pub right: Option<TableBorder>,
    #[serde(default)] pub inside_horizontal: Option<TableBorder>,
    #[serde(default)] pub inside_vertical: Option<TableBorder>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct CellPadding {
    #[serde(default)] pub top: Option<Points>,
    #[serde(default)] pub right: Option<Points>,
    #[serde(default)] pub bottom: Option<Points>,
    #[serde(default)] pub left: Option<Points>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct TableFormat {
    #[serde(default)] pub width: Option<TableWidth>,
    #[serde(default)] pub column_widths: Option<Vec<Points>>,
    #[serde(default)] pub layout: Option<String>,
    #[serde(default)] pub alignment: Option<String>,
    #[serde(default)] pub borders: Option<TableBorders>,
    #[serde(default)] pub cell_spacing: Option<Points>,
    #[serde(default)] pub padding: Option<CellPadding>,
    #[serde(default)] pub style: Option<String>,
    #[serde(default)] pub clear: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct CellFormat {
    #[serde(default)] pub width: Option<TableWidth>,
    #[serde(default)] pub borders: Option<TableBorders>,
    #[serde(default)] pub fill: Option<String>,
    #[serde(default)] pub padding: Option<CellPadding>,
    #[serde(default)] pub vertical_alignment: Option<String>,
    #[serde(default)] pub clear: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct RowFormat {
    #[serde(default)] pub height: Option<Points>,
    #[serde(default)] pub height_rule: Option<String>,
    #[serde(default)] pub repeating_header: Option<bool>,
    #[serde(default)] pub allow_split: Option<bool>,
    #[serde(default)] pub clear: Vec<String>,
}

/// Direct structural table edits. Row and column positions are one-based.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TableEdit {
    InsertRows { row: u32, position: InsertPosition, count: u32, #[serde(rename = "as", default)] alias: Option<String> },
    DeleteRows { row: u32, count: u32 },
    InsertColumns { column: u32, position: InsertPosition, count: u32, #[serde(rename = "as", default)] alias: Option<String> },
    DeleteColumns { column: u32, count: u32 },
    MergeCells { row: u32, column: u32, row_span: u32, column_span: u32 },
    SplitCell { row: u32, column: u32 },
    Delete,
}

pub const EDIT_PROTOCOL_VERSION: &str = "1";

#[derive(Debug, Clone, PartialEq, Eq, Hash, JsonSchema)]
#[schemars(with = "String")]
pub struct SourceHash(pub [u8; 32]);

impl SourceHash {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
    pub fn parse(value: &str) -> Result<Self, String> {
        let hex = value
            .strip_prefix("sha256:")
            .ok_or_else(|| "source must be sha256:<64 lowercase hex>".to_string())?;
        if hex.len() != 64
            || !hex
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("source must be sha256:<64 lowercase hex>".into());
        }
        let mut out = [0; 32];
        hex::decode_to_slice(hex, &mut out)
            .map_err(|_| "source must be sha256:<64 lowercase hex>".to_string())?;
        Ok(Self(out))
    }
    pub fn as_str(&self) -> String {
        format!("sha256:{}", hex::encode(self.0))
    }
}
impl Serialize for SourceHash {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_str(&self.as_str())
    }
}
impl<'de> Deserialize<'de> for SourceHash {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, JsonSchema)]
#[schemars(with = "f64")]
pub struct Points(pub i64); // twentieths of a point
impl Points {
    pub fn from_points(value: f64) -> Result<Self, String> {
        if !value.is_finite() || value < 0.0 {
            return Err("point value must be finite and >= 0".into());
        }
        let n = (value * 20.0).round();
        if (value * 20.0 - n).abs() > 1e-8 {
            return Err("point values must be multiples of 0.05".into());
        }
        Ok(Self(n as i64))
    }
    pub fn as_points(self) -> f64 {
        self.0 as f64 / 20.0
    }
}
impl Serialize for Points {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.0 % 20 == 0 {
            s.serialize_i64(self.0 / 20)
        } else {
            s.serialize_f64(self.as_points())
        }
    }
}
impl<'de> Deserialize<'de> for Points {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let n = f64::deserialize(d)?;
        Self::from_points(n).map_err(serde::de::Error::custom)
    }
}

/// Signed paragraph indentation, in twentieths of a point. Other dimensions use `Points`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, JsonSchema)]
#[schemars(with = "f64")]
pub struct SignedPoints(pub i64);
impl SignedPoints {
    pub fn from_points(value: f64) -> Result<Self, String> {
        if !value.is_finite() || value.abs() > 100000.0 {
            return Err("indentation must be finite and between -100000 and 100000 points".into());
        }
        let n = (value * 20.0).round();
        if (value * 20.0 - n).abs() > 1e-8 {
            return Err("point values must be multiples of 0.05".into());
        }
        Ok(Self(n as i64))
    }
    pub fn as_points(self) -> f64 { self.0 as f64 / 20.0 }
}
impl Serialize for SignedPoints {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_f64(self.as_points())
    }
}
impl<'de> Deserialize<'de> for SignedPoints {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::from_points(f64::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, JsonSchema)]
#[schemars(with = "String")]
pub enum ParagraphAddress {
    Id(String),
    Alias(String),
}
impl ParagraphAddress {
    pub fn id(id: impl Into<String>) -> Self {
        Self::Id(id.into().to_ascii_uppercase())
    }
    pub fn alias(name: impl Into<String>) -> Self {
        Self::Alias(name.into())
    }
    pub fn as_text(&self) -> String {
        match self {
            Self::Id(x) => x.clone(),
            Self::Alias(x) => format!("${x}"),
        }
    }
}
impl Serialize for ParagraphAddress {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_str(&self.as_text())
    }
}
impl<'de> Deserialize<'de> for ParagraphAddress {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(d)?;
        parse_paragraph_address(&s).map_err(serde::de::Error::custom)
    }
}

fn parse_paragraph_address(s: &str) -> Result<ParagraphAddress, String> {
    if let Some(n) = s.strip_prefix('$') {
        if valid_alias(n) {
            return Ok(ParagraphAddress::Alias(n.into()));
        }
        return Err("alias must match [A-Za-z_][A-Za-z0-9_]*".into());
    }
    if s == s.to_ascii_uppercase() && valid_para_id(s) {
        Ok(ParagraphAddress::id(s))
    } else {
        Err("paragraph id must be eight uppercase hexadecimal digits".into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, JsonSchema)]
#[schemars(with = "String")]
pub enum InsertAnchor {
    Paragraph(ParagraphAddress),
    Table(u32),
}
impl InsertAnchor {
    pub fn paragraph(id: impl Into<String>) -> Self {
        Self::Paragraph(ParagraphAddress::id(id))
    }
    pub fn table(n: u32) -> Self {
        Self::Table(n)
    }
    pub fn as_text(&self) -> String {
        match self {
            Self::Paragraph(addr) => addr.as_text(),
            Self::Table(n) => format!("table:{n}"),
        }
    }
}
impl From<ParagraphAddress> for InsertAnchor {
    fn from(addr: ParagraphAddress) -> Self {
        Self::Paragraph(addr)
    }
}
impl Serialize for InsertAnchor {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_str(&self.as_text())
    }
}
impl<'de> Deserialize<'de> for InsertAnchor {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(d)?;
        if let Some(rest) = s.strip_prefix("table:") {
            let n: u32 = rest.parse().map_err(|_| {
                serde::de::Error::custom("table anchor must be table:N with N >= 1")
            })?;
            if n == 0 {
                return Err(serde::de::Error::custom(
                    "table anchor must be table:N with N >= 1",
                ));
            }
            return Ok(Self::Table(n));
        }
        parse_paragraph_address(&s)
            .map(Self::Paragraph)
            .map_err(serde::de::Error::custom)
    }
}

/// `insert_paragraph`'s `with`: one paragraph's inline content, or a list of
/// them inserted as consecutive paragraphs in list order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, expecting = "with must be a string or a list of strings")]
pub enum ParagraphContent {
    One(String),
    Many(Vec<String>),
}
impl ParagraphContent {
    pub fn items(&self) -> &[String] {
        match self {
            Self::One(s) => std::slice::from_ref(s),
            Self::Many(items) => items,
        }
    }
}
impl Default for ParagraphContent {
    fn default() -> Self {
        Self::One(String::new())
    }
}
impl From<&str> for ParagraphContent {
    fn from(s: &str) -> Self {
        Self::One(s.into())
    }
}
impl From<String> for ParagraphContent {
    fn from(s: String) -> Self {
        Self::One(s)
    }
}
impl From<Vec<String>> for ParagraphContent {
    fn from(items: Vec<String>) -> Self {
        Self::Many(items)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ChromeKind {
    Default,
    First,
    Even,
}
impl ChromeKind {
    pub fn as_type(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::First => "first",
            Self::Even => "even",
        }
    }
    pub fn resolve(kind: Option<Self>) -> Self {
        kind.unwrap_or(Self::Default)
    }
}

fn valid_alias(s: &str) -> bool {
    let mut i = s.chars();
    matches!(i.next(),Some(c) if c.is_ascii_alphabetic()||c=='_')
        && i.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn valid_para_id(s: &str) -> bool {
    s.len() == 8
        && s.bytes().all(|b| b.is_ascii_hexdigit())
        && u32::from_str_radix(s, 16).is_ok_and(|n| n > 0 && n < 0x8000_0000)
}
pub fn is_valid_para_id(s: &str) -> bool {
    valid_para_id(&s.to_ascii_uppercase())
}
pub fn is_valid_alias(s: &str) -> bool {
    valid_alias(s)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChangeMode {
    Track,
    Direct,
}
impl Default for ChangeMode {
    fn default() -> Self {
        Self::Track
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum InsertPosition {
    Before,
    After,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CommentStatus {
    Open,
    Resolved,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum RevisionAction {
    Accept,
    Reject,
}
#[derive(Debug, Clone, PartialEq, Eq, JsonSchema)]
#[schemars(with = "String")]
pub enum RevisionTarget {
    Id(String),
    All,
}
impl Serialize for RevisionTarget {
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        s.serialize_str(match self {
            Self::Id(x) => x,
            Self::All => "all",
        })
    }
}
impl<'de> Deserialize<'de> for RevisionTarget {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(d)?;
        if s == "all" {
            Ok(Self::All)
        } else if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
            Ok(Self::Id(s))
        } else {
            Err(serde::de::Error::custom(
                "revision target must be a decimal id or all",
            ))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(tag = "mode", content = "value", rename_all = "snake_case")]
pub enum LineSpacing {
    Multiple(f64),
    Exact(Points),
    AtLeast(Points),
}

/// One bounded level in a new multilevel list definition. Array position is level (0..8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListLevel {
    pub format: String,
    pub label: String,
    #[serde(default)] pub start: Option<u32>,
    /// Restart after this zero-based ancestor level; omitted uses Word's default.
    #[serde(default)] pub restart_after: Option<u8>,
    #[serde(default)] pub indent_left: Option<SignedPoints>,
    #[serde(default)] pub indent_right: Option<SignedPoints>,
    #[serde(default)] pub first_line_indent: Option<SignedPoints>,
    #[serde(default)] pub marker_bold: Option<bool>,
    #[serde(default)] pub marker_italic: Option<bool>,
    #[serde(default)] pub marker_font: Option<String>,
    #[serde(default)] pub marker_color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditOp {
    /// Replace one current equation without matching reconstructed LaTeX.
    /// Tracked: the old equation deleted, the new one inserted. Direct: replaced.
    ReplaceEquation {
        at: ParagraphAddress,
        mathml: String,
        #[serde(default)]
        equation: Option<u32>,
        #[serde(default)]
        display: Option<bool>,
    },
    /// Delete one current equation while preserving the surrounding paragraph.
    /// Tracked: a tracked deletion. Direct: removed.
    DeleteEquation {
        at: ParagraphAddress,
        #[serde(default)]
        equation: Option<u32>,
    },
    /// Tracked: w:del / w:ins (the author's own pending insertion is edited in place). Direct: replaced, no revision.
    ReplaceText {
        at: ParagraphAddress,
        select: String,
        with: String,
        #[serde(default)]
        occurrence: Option<u32>,
    },
    /// Several exact replacements inside one bounded scope: the paragraph
    /// `at`, the unique `within` span (in `at`, or anywhere in the body when
    /// `at` is omitted), or the whole body. Every edit resolves against the
    /// scope as it stands when this operation starts.
    /// Tracked: w:del / w:ins per edit. Direct: replaced, no revision.
    ReviseText {
        #[serde(default)]
        at: Option<ParagraphAddress>,
        #[serde(default)]
        within: Option<String>,
        #[serde(default)]
        occurrence: Option<u32>,
        edits: Vec<TextEdit>,
    },
    /// Tracked: old content deleted, new inserted. Direct: replaced, no revision.
    ReplaceParagraph {
        at: ParagraphAddress,
        with: String,
    },
    /// Tracked: `insert_rows`, `delete_rows` and `delete` are row insertions/deletions (w:trPr/w:ins|w:del, deleted cell content; the author's own inserted rows go outright); `insert_columns`, `delete_columns`, `merge_cells` and `split_cell` are refused (Word does not track them). Direct: applied, no revision.
    EditTable { at: String, action: TableEdit },
    /// A `rows` × `columns` grid of new cells. `cells` fills them row by
    /// row with the inline content `insert_paragraph`'s `with` accepts;
    /// missing trailing rows and cells stay empty. With `as`, cell row N
    /// column M is the paragraph alias `$<as>_rNcM` for later operations.
    /// Tracked: an inserted table (every row w:trPr/w:ins, cell content w:ins); refused as the last block of a table cell. Direct: inserted, no revision.
    InsertTable {
        at: ParagraphAddress,
        position: InsertPosition,
        rows: u32,
        columns: u32,
        #[serde(default)]
        cells: Option<Vec<Vec<String>>>,
        #[serde(rename = "as", default)]
        alias: Option<String>,
    },
    /// Tracked: w:tblPrChange. Direct: applied, no revision.
    FormatTable { at: String, format: TableFormat },
    /// Tracked: w:trPrChange. Direct: applied, no revision.
    FormatRow { at: String, format: RowFormat },
    /// Tracked: w:tcPrChange. Direct: applied, no revision.
    FormatCell { at: String, format: CellFormat },
    /// Tracked: w:rPrChange. Direct: applied, no revision.
    FormatText {
        at: ParagraphAddress,
        select: String,
        #[serde(default)]
        occurrence: Option<u32>,
        #[serde(default)]
        bold: Option<bool>,
        #[serde(default)]
        italic: Option<bool>,
        #[serde(default)]
        underline: Option<bool>,
        #[serde(default)]
        strike: Option<bool>,
        #[serde(default)]
        superscript: Option<bool>,
        #[serde(default)]
        subscript: Option<bool>,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        highlight: Option<HighlightColor>,
        #[serde(default)]
        font_size: Option<Points>,
        #[serde(default)]
        clear: Vec<String>,
    },
    /// Tracked: w:pPrChange. Direct: applied, no revision.
    FormatParagraph {
        at: ParagraphAddress,
        #[serde(default)]
        style: Option<String>,
        #[serde(default)]
        alignment: Option<String>,
        #[serde(default)]
        indent_left: Option<SignedPoints>,
        #[serde(default)]
        indent_right: Option<SignedPoints>,
        /// Signed first-line offset: negative means hanging; zero explicitly disables it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        first_line_indent: Option<SignedPoints>,
        #[serde(default)]
        space_before: Option<Points>,
        #[serde(default)]
        space_after: Option<Points>,
        #[serde(default)]
        line_spacing: Option<LineSpacing>,
        #[serde(default)]
        clear: Vec<String>,
    },
    /// Assign selected paragraphs to a new or existing list. `remove` explicitly
    /// suppresses inherited numbering; new definitions remain after rejection.
    /// Tracked: w:pPrChange (new list definitions stay after reject-all). Direct: applied, no revision.
    SetList {
        #[serde(default)]
        levels: Option<Vec<ListLevel>>,
        at: Vec<ParagraphAddress>,
        #[serde(default)]
        num_id: Option<u32>,
        #[serde(default)]
        format: Option<String>,
        #[serde(default)]
        level: u8,
        #[serde(default)]
        start: Option<u32>,
        #[serde(default)]
        remove: bool,
    },
    /// One paragraph, or with a list `with`, one per item in list order.
    /// With `as`, a single paragraph is `$<as>`; for a list, `$<as>` is the
    /// group (expanded in paragraph lists, its first or last paragraph as an
    /// insert anchor) and item N is `$<as>_N`.
    /// Without `style`, a new paragraph takes the paragraph properties of a paragraph anchor (style, numbering, indents, spacing; not its mark's run properties, section properties or pending changes), as Word's Enter does: inserted into a list it is a list item. After a paragraph whose style names a different next style (a heading), it takes that style instead. Tracked: inserted content and paragraph mark; reject-all removes the paragraph. Direct: inserted, no revision.
    InsertParagraph {
        at: InsertAnchor,
        position: InsertPosition,
        #[serde(default)]
        with: ParagraphContent,
        #[serde(default)]
        style: Option<String>,
        #[serde(rename = "as", default)]
        alias: Option<String>,
    },
    /// Tracked: content and paragraph mark deleted; accept-all leaves the following paragraph's id and properties as they were. A paragraph whose mark cannot join forward (the last of the body, one before a table, one ending a section) takes the previous paragraph's mark instead, as Word does: it gets that paragraph's properties (its own in a pPrChange) and keeps its section properties, and accept-all leaves the previous paragraph, with its id, in its place. Where no previous paragraph can join (a table or another author's mark revision before it) it is left empty. Direct: the same edit accepted, no revision.
    DeleteParagraphs {
        at: Vec<ParagraphAddress>,
    },
    /// Tracked: w:sectPrChange. Direct: applied, no revision.
    SetPageMargins {
        #[serde(default)]
        section: Option<u32>,
        #[serde(default)]
        top: Option<Points>,
        #[serde(default)]
        right: Option<Points>,
        #[serde(default)]
        bottom: Option<Points>,
        #[serde(default)]
        left: Option<Points>,
        #[serde(default)]
        header: Option<Points>,
        #[serde(default)]
        footer: Option<Points>,
        #[serde(default)]
        gutter: Option<Points>,
    },
    /// Selective section-property edits. Dimensions are points; omitted fields preserve source.
    /// Refused in track mode: Word records no revision for adding or removing a section break. Direct only.
    SetSectionBoundary { at: ParagraphAddress, #[serde(default)] remove: bool, #[serde(default)] break_type: Option<String> },
    /// Tracked: w:sectPrChange. Direct: applied, no revision.
    SetSectionLayout {
        #[serde(default)] section_handle: Option<String>,
        #[serde(default)]
        section: Option<u32>,
        #[serde(default)]
        page_size: Option<String>,
        #[serde(default)]
        width: Option<Points>,
        #[serde(default)]
        height: Option<Points>,
        #[serde(default)]
        orientation: Option<String>,
        #[serde(default)]
        columns: Option<u32>,
        #[serde(default)]
        column_spacing: Option<Points>,
        #[serde(default)]
        column_widths: Option<Vec<Points>>,
        #[serde(default)]
        column_separator: Option<bool>,
        #[serde(default)]
        break_type: Option<String>,
        #[serde(default)]
        page_number_format: Option<String>,
        #[serde(default)]
        page_number_start: Option<u32>,
        #[serde(default)]
        page_number_continue: Option<bool>,
    },
    /// Tracked: w:sectPrChange. Direct: applied, no revision.
    SetFirstPageMode {
        #[serde(default)]
        section: Option<u32>,
        enabled: bool,
    },
    /// Refused in track mode: a document setting (settings.xml) Word records no revision for. Direct only.
    SetEvenAndOddHeaders {
        even_and_odd: bool,
    },
    /// Tracked: the old content a tracked deletion and the new a tracked insertion (simple fields written as complex field runs); a new header's part and reference cannot be tracked, so reject-all leaves it empty. `kind: first` records titlePg in a w:sectPrChange; `kind: even` is refused while even/odd headers are off. Direct: replaced, no revision.
    SetHeader {
        #[serde(default)]
        section: Option<u32>,
        #[serde(default)]
        kind: Option<ChromeKind>,
        with: String,
    },
    /// Tracked and direct as `set_header`.
    SetFooter {
        #[serde(default)]
        section: Option<u32>,
        #[serde(default)]
        kind: Option<ChromeKind>,
        with: String,
    },
    /// Tracked: the content becomes a tracked deletion and the (then empty) header stays referenced; refused after section 1, where removing a reference changes inheritance. Direct: the reference is removed.
    ClearHeader {
        #[serde(default)]
        section: Option<u32>,
        #[serde(default)]
        kind: Option<ChromeKind>,
    },
    /// Tracked and direct as `clear_header`.
    ClearFooter {
        #[serde(default)]
        section: Option<u32>,
        #[serde(default)]
        kind: Option<ChromeKind>,
    },
    /// Applied directly in both change modes: Word does not track comments.
    CommentAdd {
        at: ParagraphAddress,
        #[serde(default)]
        select: Option<String>,
        #[serde(default)]
        occurrence: Option<u32>,
        text: String,
    },
    /// Applied directly in both change modes: Word does not track comments.
    CommentReply {
        comment_id: String,
        text: String,
    },
    /// Applied directly in both change modes: Word does not track comments.
    CommentSetStatus {
        comment_id: String,
        status: CommentStatus,
    },
    /// Applied directly in both change modes: Word does not track comments.
    CommentDelete {
        comment_id: String,
    },
    /// Bookmark the paragraph at `at`: its whole content, or the `select`
    /// span. Applied directly in both change modes (Word does not track
    /// bookmarks). Body paragraphs only.
    BookmarkAdd {
        at: ParagraphAddress,
        name: String,
        #[serde(default)]
        select: Option<String>,
        #[serde(default)]
        occurrence: Option<u32>,
    },
    /// Rename a body bookmark; `new_name` follows the add rules.
    /// Applied directly in both change modes (Word does not track bookmarks).
    BookmarkRename {
        name: String,
        new_name: String,
    },
    /// Re-anchor a body bookmark, keeping its name and `w:id`.
    /// Applied directly in both change modes (Word does not track bookmarks).
    BookmarkMove {
        name: String,
        at: ParagraphAddress,
        #[serde(default)]
        select: Option<String>,
        #[serde(default)]
        occurrence: Option<u32>,
    },
    /// Remove a body bookmark's start and end markers; the text stays.
    /// Applied directly in both change modes (Word does not track bookmarks).
    BookmarkDelete {
        name: String,
    },
    /// Settles revisions in both change modes; it writes none.
    RevisionSettle {
        target: RevisionTarget,
        action: RevisionAction,
    },
}

/// One replacement inside a `revise_text` scope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextEdit {
    pub select: String,
    pub with: String,
    /// 1-based match within the scope; omit when `select` is unique there.
    #[serde(default)]
    pub occurrence: Option<u32>,
    /// Replace every match within the scope.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCommand {
    #[serde(default)]
    pub paragraphs: Vec<String>,
    #[serde(default)]
    pub html: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadCommand {
    #[serde(default)]
    pub read_kind: Option<String>,
    #[serde(default)]
    pub view: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FindCommand {
    pub query: String,
    #[serde(default)]
    pub ignore_case: bool,
    #[serde(default)]
    pub view: Option<FindView>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FindView {
    Markup,
    Final,
    Original,
}
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Create(CreateCommand),
    Read(ReadCommand),
    Find(FindCommand),
    Edit(EditCommand),
}
impl JsonSchema for Command {
    fn schema_name() -> String {
        "Command".into()
    }
    fn json_schema(_generator: &mut schemars::gen::SchemaGenerator) -> Schema {
        serde_json::from_value(serde_json::json!({
            "oneOf": [
                {"type":"object","additionalProperties":false,"required":["kind"],"properties":{"kind":{"const":"create"},"paragraphs":{"type":"array","items":{"type":"string"}},"html":{"type":["string","null"]}}},
                {"type":"object","additionalProperties":false,"required":["kind"],"properties":{"kind":{"const":"read"},"read_kind":{"type":["string","null"]},"view":{"type":["string","null"]}}},
                {"type":"object","additionalProperties":false,"required":["kind","query"],"properties":{"kind":{"const":"find"},"query":{"type":"string"},"ignore_case":{"type":"boolean"},"view":{"anyOf":[{"$ref":"#/definitions/FindView"},{"type":"null"}]}}},
                {"type":"object","additionalProperties":false,"required":["kind","author","change_mode","op"],"properties":{"kind":{"const":"edit"},"author":{"type":"string","minLength":1},"change_mode":{"$ref":"#/definitions/ChangeMode"},"op":{"$ref":"#/definitions/EditOp"}}}
            ]
        })).unwrap()
    }
}
impl Serialize for Command {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut value = match self {
            Self::Create(x) => serde_json::to_value(x),
            Self::Read(x) => serde_json::to_value(x),
            Self::Find(x) => serde_json::to_value(x),
            Self::Edit(x) => serde_json::to_value(x),
        }
        .map_err(serde::ser::Error::custom)?
        .as_object()
        .cloned()
        .unwrap();
        let kind = match self {
            Self::Create(_) => "create",
            Self::Read(_) => "read",
            Self::Find(_) => "find",
            Self::Edit(_) => "edit",
        };
        value.insert("kind".into(), Value::String(kind.into()));
        Value::Object(value).serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Command {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = Value::deserialize(d)?;
        let mut o = s
            .as_object()
            .cloned()
            .ok_or_else(|| serde::de::Error::custom("command must be an object"))?;
        let kind = o
            .remove("kind")
            .and_then(|x| x.as_str().map(str::to_string))
            .ok_or_else(|| serde::de::Error::custom("command kind is required"))?;
        let value = Value::Object(o);
        match kind.as_str() {
            "create" => serde_json::from_value(value)
                .map(Self::Create)
                .map_err(serde::de::Error::custom),
            "read" => serde_json::from_value(value)
                .map(Self::Read)
                .map_err(serde::de::Error::custom),
            "find" => serde_json::from_value(value)
                .map(Self::Find)
                .map_err(serde::de::Error::custom),
            "edit" => serde_json::from_value(value)
                .map(Self::Edit)
                .map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom("unknown command kind")),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditCommand {
    pub author: String,
    pub change_mode: ChangeMode,
    pub op: EditOp,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandRequest {
    pub command: Command,
    #[serde(default)]
    pub expected_source: Option<SourceHash>,
    /// As [`PlanRequest::now`], for an `edit` command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub base: SourceHash,
    pub author: String,
    pub change_mode: ChangeMode,
    pub ops: Vec<EditOp>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanRequest {
    pub plan: Plan,
    #[serde(default)]
    pub preview_key: Option<PreviewKey>,
    /// The time stamped on the revisions and comments a commit writes, as
    /// `YYYY-MM-DDTHH:MM:SSZ` (UTC, second precision, as Word writes
    /// `w:date`). Omitted, the current UTC time. Pass a fixed time for
    /// byte-stable output (tests, golden files).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub now: Option<String>,
}
impl JsonSchema for PlanRequest {
    fn schema_name() -> String {
        "PlanRequest".into()
    }
    fn json_schema(_generator: &mut schemars::gen::SchemaGenerator) -> Schema {
        serde_json::from_value(serde_json::json!({
            "type":"object","additionalProperties":false,"required":["plan"],
            "properties":{"plan":{"$ref":"#/definitions/Plan"},"preview_key":{"anyOf":[{"$ref":"#/definitions/PreviewKey"},{"type":"null"}]},"now":{"type":"string","pattern":"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$","description":"Revision and comment time (UTC, second precision); omitted, the current time."}}
        })).unwrap()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct PreviewKey(pub String);
impl PreviewKey {
    pub fn new(plan: &Plan) -> Self {
        Self(compute_preview_key(&plan.base, plan))
    }
}

impl Plan {
    pub fn from_json(text: &str) -> Result<Self, Diagnostic> {
        parse_plan_json(text)
    }
    pub fn from_toml(text: &str) -> Result<Self, Diagnostic> {
        parse_plan_toml(text)
    }
    pub fn canonical_json(&self) -> Result<String, String> {
        canonical_json(self)
    }
    pub fn canonical_toml(&self) -> Result<String, String> {
        canonical_toml(self)
    }
    pub fn preview_key(&self) -> PreviewKey {
        PreviewKey::new(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum Request {
    Command(CommandRequest),
    Plan(PlanRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Span {
    pub line: usize,
    pub column: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_column: Option<usize>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct OperationReport {
    pub index: usize,
    pub op: String,
    pub outcome: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected: Vec<Value>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub context_truncated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct PlanReport {
    pub completed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_at: Option<usize>,
    pub ops: Vec<OperationReport>,
    #[serde(default)]
    pub repaired_ids: Vec<String>,
    #[serde(default)]
    pub allocated_ids: Vec<String>,
    #[serde(default)]
    pub aliases: Map<String, Value>,
    /// Group aliases: a list `insert_paragraph`'s `as`, naming its new
    /// paragraph ids in order.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub groups: Map<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum CommandResult {
    Completed {
        source: Option<SourceHash>,
        result: Option<Value>,
        bytes: Option<Vec<u8>>,
    },
    Rejected {
        source: Option<SourceHash>,
        result: Option<Value>,
        diagnostic: Diagnostic,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum PlanResult {
    Previewed {
        source: SourceHash,
        plan: Plan,
        canonical_toml: String,
        preview_key: PreviewKey,
        report: PlanReport,
    },
    Committed {
        source: SourceHash,
        plan: Plan,
        canonical_toml: String,
        report: PlanReport,
        bytes: Vec<u8>,
    },
    Rejected {
        source: Option<SourceHash>,
        plan: Option<Plan>,
        diagnostic: Diagnostic,
        #[serde(skip_serializing_if = "Option::is_none")]
        report: Option<PlanReport>,
    },
}

pub fn compute_preview_key(source: &SourceHash, plan: &Plan) -> String {
    let canonical = canonical_json(plan).unwrap_or_default();
    let mut h = Sha256::new();
    h.update(b"docxdriver-plan-preview\0");
    h.update(EDIT_PROTOCOL_VERSION.as_bytes());
    h.update([0]);
    h.update(source.as_str().as_bytes());
    h.update([0]);
    h.update(canonical.as_bytes());
    format!("p1:sha256:{}", hex::encode(h.finalize()))
}
pub fn canonical_json(plan: &Plan) -> Result<String, String> {
    let normalized = validate_plan(plan.clone()).map_err(|e| e.message)?;
    let mut v = serde_json::to_value(&normalized).map_err(|e| e.to_string())?;
    strip_nulls(&mut v);
    canonicalize_value(&mut v);
    serde_json::to_string(&v).map_err(|e| e.to_string())
}
fn canonicalize_value(v: &mut Value) {
    match v {
        Value::Object(m) => {
            let keys: mapi::Vec<_> = m.keys().cloned().collect();
            for k in keys {
                if let Some(x) = m.get_mut(&k) {
                    canonicalize_value(x)
                }
            }
            let mut n = Map::new();
            for (k, x) in std::mem::take(m) {
                n.insert(k, x);
            }
            *m = n;
        }
        Value::Array(a) => {
            for x in a {
                canonicalize_value(x)
            }
        }
        _ => {}
    }
}
fn strip_nulls(v: &mut Value) {
    match v {
        Value::Object(map) => {
            map.retain(|_, value| !value.is_null());
            for value in map.values_mut() {
                strip_nulls(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                strip_nulls(value);
            }
        }
        _ => {}
    }
}
mod mapi {
    pub type Vec<T> = std::vec::Vec<T>;
}

pub fn canonical_toml(plan: &Plan) -> Result<String, String> {
    let normalized = validate_plan(plan.clone()).map_err(|e| e.message)?;
    let mut value = serde_json::to_value(&normalized).map_err(|e| e.to_string())?;
    strip_nulls(&mut value);
    let mut root = toml::map::Map::new();
    let o = value.as_object().unwrap();
    root.insert(
        "base".into(),
        toml::Value::String(o["base"].as_str().unwrap().into()),
    );
    root.insert(
        "author".into(),
        toml::Value::String(o["author"].as_str().unwrap().into()),
    );
    root.insert(
        "change_mode".into(),
        toml::Value::String(o["change_mode"].as_str().unwrap().into()),
    );
    let ops = o["ops"]
        .as_array()
        .unwrap()
        .iter()
        .map(json_to_toml_table)
        .collect::<Result<Vec<_>, _>>()?;
    root.insert(
        "ops".into(),
        toml::Value::Array(ops.into_iter().map(toml::Value::Table).collect()),
    );
    toml::to_string(&toml::Value::Table(root)).map_err(|e| e.to_string())
}
fn json_to_toml_table(v: &Value) -> Result<toml::map::Map<String, toml::Value>, String> {
    let mut out = toml::map::Map::new();
    for (k, x) in v.as_object().ok_or("operation must be object")? {
        out.insert(k.clone(), json_to_toml(x)?);
    }
    Ok(out)
}
fn json_to_toml(v: &Value) -> Result<toml::Value, String> {
    Ok(match v {
        Value::Null => return Err("null is not valid in canonical TOML".into()),
        Value::Bool(x) => toml::Value::Boolean(*x),
        Value::Number(x) => {
            if let Some(integer) = x.as_i64() {
                toml::Value::Integer(integer)
            } else {
                toml::Value::Float(x.as_f64().unwrap())
            }
        }
        Value::String(x) => toml::Value::String(x.clone()),
        Value::Array(xs) => {
            toml::Value::Array(xs.iter().map(json_to_toml).collect::<Result<_, _>>()?)
        }
        Value::Object(x) => toml::Value::Table(
            x.iter()
                .map(|(k, v)| (k.clone(), json_to_toml(v).unwrap()))
                .collect(),
        ),
    })
}

pub fn parse_plan_json(text: &str) -> Result<Plan, Diagnostic> {
    let v: Value = serde_json::from_str(text).map_err(|e| Diagnostic {
        code: "invalid_json".into(),
        message: e.to_string(),
        path: None,
        span: Some(Span {
            line: e.line(),
            column: e.column(),
            end_line: Some(e.line()),
            end_column: Some(e.column().saturating_add(1)),
        }),
    })?;
    let p: Plan =
        serde_json::from_value(v).map_err(|e| diag("invalid_plan", e.to_string(), None))?;
    validate_plan(p).map_err(|mut error| {
        if error.span.is_none() {
            error.span = error.path.as_deref().and_then(|path| {
                diag_span(
                    error.code.clone(),
                    error.message.clone(),
                    Some(path.into()),
                    text,
                )
                .span
            });
        }
        error
    })
}
pub fn parse_plan_toml(text: &str) -> Result<Plan, Diagnostic> {
    let v: toml::Value = toml::from_str(text).map_err(|e| {
        let span = e.span().map(|r| make_span(text, r.start, r.end));
        Diagnostic {
            code: "invalid_toml".into(),
            message: e.to_string(),
            path: None,
            span,
        }
    })?;
    let j = toml_to_json(v);
    let p: Plan = serde_json::from_value(j).map_err(|e| {
        let message = e.to_string();
        let path = serde_error_field(&message)
            .map(|field| format!("ops[0].{field}"))
            .unwrap_or_else(|| "ops".into());
        diag_span("invalid_plan", message, Some(path), text)
    })?;
    validate_plan(p).map_err(|mut error| {
        if error.span.is_none() {
            error.span = error.path.as_deref().and_then(|path| {
                diag_span(
                    error.code.clone(),
                    error.message.clone(),
                    Some(path.into()),
                    text,
                )
                .span
            });
        }
        error
    })
}
fn serde_error_field(message: &str) -> Option<&str> {
    let rest = message.strip_prefix("unknown field `")?;
    rest.split('`').next()
}
fn toml_to_json(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => serde_json::json!(i),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(a) => Value::Array(a.into_iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => {
            Value::Object(t.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect())
        }
    }
}
fn validate_plan(mut p: Plan) -> Result<Plan, Diagnostic> {
    if p.ops.is_empty() {
        return Err(diag(
            "empty_plan",
            "plan must contain at least one operation",
            Some("ops".into()),
        ));
    }
    if p.author.is_empty() {
        return Err(diag(
            "invalid_author",
            "author must not be empty",
            Some("author".into()),
        ));
    }
    for (i, op) in p.ops.iter_mut().enumerate() {
        normalize_op(op).map_err(|m| {
            let field = validation_field(&m);
            let path = field
                .map(|field| format!("ops[{i}].{field}"))
                .unwrap_or_else(|| format!("ops[{i}]"));
            diag("invalid_operation", m, Some(path))
        })?;
    }
    Ok(p)
}

fn validation_field(message: &str) -> Option<&'static str> {
    let field = if message.starts_with("new_name ") {
        "new_name"
    } else if message.starts_with("name ") {
        "name"
    } else if message.contains("MathML") {
        "mathml"
    } else if message.contains("equation must") {
        "equation"
    } else if message.contains("edits") {
        "edits"
    } else if message.contains("within") {
        "within"
    } else if message.contains("select") {
        "select"
    } else if message.contains("occurrence") {
        "occurrence"
    } else if message.contains("color") {
        "color"
    } else if message.contains("superscript") {
        "superscript"
    } else if message.contains("subscript") {
        "subscript"
    } else if message.contains("clear") {
        "clear"
    } else if message.contains("alignment") {
        "alignment"
    } else if message.contains("style") {
        "style"
    } else if message.contains("alias") {
        "alias"
    } else if message == "at must not be empty" {
        "at"
    } else if message.contains("section") {
        "section"
    } else if message.contains("margin") {
        "top"
    } else if message.contains("comment_id") {
        "comment_id"
    } else if message.contains("comment text") {
        "text"
    } else if message.contains("revision target") {
        "target"
    } else {
        return None;
    };
    Some(field)
}
fn normalize_op(op: &mut EditOp) -> Result<(), String> {
    match op {
        EditOp::ReplaceEquation {
            mathml, equation, ..
        } => {
            *mathml = crate::html::input::canonical_mathml(mathml)?;
            if *equation == Some(0) {
                return Err("equation must be positive (1-based)".into());
            }
        }
        EditOp::DeleteEquation { equation, .. } => {
            if *equation == Some(0) {
                return Err("equation must be positive (1-based)".into());
            }
        }
        EditOp::ReplaceText {
            select, occurrence, ..
        } => {
            if select.is_empty() {
                return Err("select must be non-empty".into());
            }
            if occurrence == &Some(0) {
                return Err("occurrence must be positive".into());
            }
        }
        EditOp::ReviseText {
            within,
            occurrence,
            edits,
            ..
        } => {
            if within.as_deref() == Some("") {
                return Err("within must be non-empty when given".into());
            }
            if occurrence == &Some(0) {
                return Err("occurrence must be positive".into());
            }
            if occurrence.is_some() && within.is_none() {
                return Err("occurrence picks a within match; it requires within".into());
            }
            if edits.is_empty() {
                return Err("edits must list at least one replacement".into());
            }
            for (index, edit) in edits.iter().enumerate() {
                if edit.select.is_empty() {
                    return Err(format!("edits[{index}].select must be non-empty"));
                }
                if edit.occurrence == Some(0) {
                    return Err(format!("edits[{index}].occurrence must be positive"));
                }
                if edit.all && edit.occurrence.is_some() {
                    return Err(format!(
                        "edits[{index}] sets both all and occurrence; choose one"
                    ));
                }
            }
        }
        EditOp::EditTable { at, action } => crate::commands::table_structure::validate(at, action)?,
        EditOp::InsertTable { rows, columns, cells, alias, .. } => {
            if *rows == 0 || *columns == 0 || *rows > 100 || *columns > 100 { return Err("insert_table dimensions must be 1..100".into()); }
            if alias.as_ref().is_some_and(|a| !valid_alias(a)) { return Err("invalid alias".into()); }
            let cells = cells.as_deref().unwrap_or_default();
            if cells.len() > *rows as usize {
                let extra = cells.len() - *rows as usize;
                return Err(format!(
                    "cells has {} rows but the table has {rows}: remove cells[{rows}..] (row {}{})",
                    cells.len(),
                    *rows + 1,
                    if extra > 1 { format!(" to row {}", cells.len()) } else { String::new() },
                ));
            }
            for (index, row) in cells.iter().enumerate() {
                if row.len() > *columns as usize {
                    let extra = row.len() - *columns as usize;
                    return Err(format!(
                        "cells[{index}] (row {}) has {} cells but the table has {columns} columns: remove cells[{index}][{columns}..] (column {}{})",
                        index + 1,
                        row.len(),
                        *columns + 1,
                        if extra > 1 { format!(" to column {}", row.len()) } else { String::new() },
                    ));
                }
            }
        }
        EditOp::FormatTable { at, format } => crate::commands::tables::validate_table(at, format)?,
        EditOp::FormatRow { at, format } => crate::commands::tables::validate_row(at, format)?,
        EditOp::FormatCell { at, format } => crate::commands::tables::validate_cell(at, format)?,
        EditOp::ReplaceParagraph { .. } => {}
        EditOp::FormatText {
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
            ..
        } => {
            if select.is_empty() {
                return Err("select must be non-empty".into());
            }
            if occurrence == &Some(0) {
                return Err("occurrence must be positive".into());
            }
            if superscript == &Some(true) && subscript == &Some(true) {
                return Err("superscript and subscript cannot both be enabled".into());
            }
            if let Some(c) = color {
                let x = c.strip_prefix('#').unwrap_or(c);
                if !c.starts_with('#')
                    || x.len() != 6
                    || !x.bytes().all(|b| b.is_ascii_hexdigit())
                    || c != &c.to_ascii_uppercase()
                {
                    return Err("color must be #RRGGBB".into());
                }
            }
            if bold.is_none()
                && italic.is_none()
                && underline.is_none()
                && strike.is_none()
                && superscript.is_none()
                && subscript.is_none()
                && highlight.is_none()
                && color.is_none()
                && font_size.is_none()
                && clear.is_empty()
            {
                return Err("format_text requires a property".into());
            }
            if clear.iter().any(|x| {
                ![
                    "bold",
                    "italic",
                    "underline",
                    "strike",
                    "superscript",
                    "subscript",
                    "highlight",
                    "color",
                    "font_size",
                ]
                .contains(&x.as_str())
            }) {
                return Err("format_text clear contains an unknown property".into());
            }
        }
        EditOp::FormatParagraph {
            style,
            alignment,
            indent_left,
            indent_right,
            first_line_indent,
            space_before,
            space_after,
            line_spacing,
            clear,
            ..
        } => {
            if [*indent_left, *indent_right, *first_line_indent].into_iter().flatten().any(|v| v.0.unsigned_abs() > 2_000_000) {
                return Err("paragraph indentation must be between -100000 and 100000 points".into());
            }
            if let Some(s) = style {
                if s.is_empty() {
                    return Err("style must be non-empty".into());
                }
            }
            if let Some(a) = alignment {
                let x = a.to_ascii_lowercase();
                if x == "both" {
                    return Err("alignment must be justify, not both".into());
                }
                if !matches!(x.as_str(), "left" | "center" | "right" | "justify") {
                    return Err("invalid alignment".into());
                }
                *alignment = Some(x);
            }
            if indent_left.is_none()
                && indent_right.is_none()
                && first_line_indent.is_none()
                && space_before.is_none()
                && space_after.is_none()
                && line_spacing.is_none()
                && style.is_none()
                && alignment.is_none()
                && clear.is_empty()
            {
                return Err("format_paragraph requires a property".into());
            }
        }
        EditOp::SetList { at, num_id, format, level, start, remove, levels } => {
            if let Some(defs) = levels {
                if defs.is_empty() || defs.len() > 9 || num_id.is_some() || *remove || format.is_some() || *level as usize >= defs.len() {
                    return Err("levels requires a new list with 1..9 definitions covering the selected level".into());
                }
                for (i, def) in defs.iter().enumerate() {
                    if !["bullet","decimal","lowerLetter","upperLetter","lowerRoman","upperRoman"].contains(&def.format.as_str()) || def.start.is_some_and(|v|v == 0 || v > i32::MAX as u32) || def.label.is_empty() || def.label.len() > 255 || def.restart_after.is_some_and(|v| v as usize >= i) {
                        return Err("invalid list level format, label, start or restart ancestor".into());
                    }
                    if [def.indent_left,def.indent_right,def.first_line_indent].into_iter().flatten().any(|v| v.0.unsigned_abs() > 2_000_000) { return Err("list indentation is out of range".into()); }
                    let bytes = def.label.as_bytes();
                    for j in 0..bytes.len() {
                        if bytes[j] == b'%' && (j + 1 == bytes.len() || !(b'1'..=b'1' + i as u8).contains(&bytes[j+1])) { return Err("list labels may only reference this level or ancestor %1..%9 counters".into()); }
                    }
                    if def.marker_font.as_ref().is_some_and(|v| v.is_empty()) || def.marker_color.as_ref().is_some_and(|v| v.len() != 6 || !v.bytes().all(|c| c.is_ascii_hexdigit())) { return Err("invalid list marker font or six-digit RGB color".into()); }
                }
            }
            if at.is_empty() || *level > 8 || start.is_some_and(|v|v == 0 || v > i32::MAX as u32) || num_id.is_some_and(|v|v == 0 || v > i32::MAX as u32) {
                return Err("set_list requires paragraphs, level 0..8 and IDs/starts in 1..2147483647".into());
            }
            if *remove && (num_id.is_some() || format.is_some() || start.is_some() || *level != 0) {
                return Err("set_list remove cannot be combined with list settings".into());
            }
            if let Some(fmt) = format {
                if !["bullet", "decimal", "lowerLetter", "upperLetter", "lowerRoman", "upperRoman"].contains(&fmt.as_str()) {
                    return Err("unsupported set_list format".into());
                }
                if num_id.is_some() { return Err("format cannot restyle an existing list; create a new list".into()); }
            }
        }
        EditOp::InsertParagraph { alias, with, .. } => {
            if let Some(a) = alias {
                if !valid_alias(a) {
                    return Err("invalid alias".into());
                }
            }
            if let ParagraphContent::Many(items) = with {
                if items.is_empty() {
                    return Err("a with list must not be empty".into());
                }
            }
        }
        EditOp::DeleteParagraphs { at } => {
            if at.is_empty() {
                return Err("at must not be empty".into());
            }
        }
        EditOp::SetSectionBoundary {remove,break_type,..} => {
            if *remove && break_type.is_some(){return Err("removing a boundary cannot set break_type".into());}
            if break_type.as_ref().is_some_and(|v|!matches!(v.as_str(),"nextPage"|"continuous"|"oddPage"|"evenPage"|"nextColumn")){return Err("unsupported section break_type".into());}
        }
        EditOp::SetSectionLayout { section, section_handle, page_size, width, height, orientation, columns, column_spacing, column_widths, column_separator, break_type, page_number_format, page_number_start, page_number_continue } => {
            if section.is_some() && section_handle.is_some() {return Err("section and section_handle are mutually exclusive".into());}
            if section_handle.as_ref().is_some_and(|v|!v.starts_with("section:")){return Err("section_handle must use an inventory handle".into());}
            if section == &Some(0) { return Err("section must be positive".into()); }
            if let Some(size) = page_size { if !matches!(size.as_str(), "letter" | "legal" | "a4" | "a3" | "a5") { return Err("page_size must be letter/legal/a4/a3/a5".into()); } }
            if page_size.is_some() && (width.is_some() || height.is_some()) { return Err("page_size conflicts with width/height".into()); }
            if width.is_some() != height.is_some() { return Err("width and height must be supplied together".into()); }
            if [width.as_ref(), height.as_ref()].into_iter().flatten().any(|value| value.0 <= 0 || value.0 > 31680) { return Err("page dimensions must be >0 and <=1584 points".into()); }
            if orientation.as_ref().is_some_and(|v| !matches!(v.as_str(), "portrait" | "landscape")) { return Err("orientation must be portrait/landscape".into()); }
            if columns.is_some_and(|v| v == 0 || v > 45) { return Err("columns must be between 1 and 45".into()); }
            if column_spacing.is_some_and(|p| !(0..=31680).contains(&p.0)) { return Err("column_spacing must be between 0 and 1584 points".into()); }
            if column_widths.as_ref().is_some_and(|v| v.is_empty() || v.len() > 45 || v.iter().any(|p| p.0 <= 0 || p.0 > 31680)) { return Err("column_widths requires 1..45 positive widths of at most 1584 points".into()); }
            if let (Some(count), Some(widths)) = (columns.as_ref(), column_widths.as_ref()) { if *count as usize != widths.len() { return Err("columns must match column_widths length".into()); } }
            if break_type.as_ref().is_some_and(|v| !matches!(v.as_str(), "nextPage" | "continuous" | "oddPage" | "evenPage" | "nextColumn")) { return Err("unsupported section break_type".into()); }
            if page_number_format.as_ref().is_some_and(|v| !matches!(v.as_str(), "decimal" | "upperRoman" | "lowerRoman" | "upperLetter" | "lowerLetter")) { return Err("unsupported page_number_format".into()); }
            if page_number_continue == &Some(false) { return Err("page_number_continue only accepts true; use page_number_start to restart".into()); }
            if page_number_start.is_some_and(|value| value == 0 || value > i32::MAX as u32) { return Err("page_number_start must be between 1 and 2147483647".into()); }
            if page_number_start.is_some() && page_number_continue == &Some(true) { return Err("page_number_start conflicts with page_number_continue".into()); }
            if page_size.is_none() && width.is_none() && orientation.is_none() && columns.is_none() && column_spacing.is_none() && column_widths.is_none() && column_separator.is_none() && break_type.is_none() && page_number_format.is_none() && page_number_start.is_none() && page_number_continue.is_none() { return Err("set_section_layout requires a property".into()); }
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
        } => {
            if section == &Some(0) {
                return Err("section must be positive".into());
            }
            if top.is_none()
                && right.is_none()
                && bottom.is_none()
                && left.is_none()
                && header.is_none()
                && footer.is_none()
                && gutter.is_none()
            {
                return Err("set_page_margins requires a margin".into());
            }
        }
        EditOp::BookmarkAdd {
            name,
            select,
            occurrence,
            ..
        } => {
            crate::commands::bookmarks::validate_new_name(name, "name")?;
            validate_bookmark_select(select, occurrence)?;
        }
        EditOp::BookmarkMove {
            name,
            select,
            occurrence,
            ..
        } => {
            crate::commands::bookmarks::validate_existing_name(name)?;
            validate_bookmark_select(select, occurrence)?;
        }
        EditOp::BookmarkRename { name, new_name } => {
            crate::commands::bookmarks::validate_existing_name(name)?;
            crate::commands::bookmarks::validate_new_name(new_name, "new_name")?;
        }
        EditOp::BookmarkDelete { name } => {
            crate::commands::bookmarks::validate_existing_name(name)?;
        }
        EditOp::CommentAdd {
            text,
            occurrence,
            select,
            ..
        } => {
            if text.is_empty() {
                return Err("comment text must be non-empty".into());
            }
            if occurrence == &Some(0) {
                return Err("occurrence must be positive".into());
            }
            if occurrence.is_some() && select.is_none() {
                return Err("occurrence requires select".into());
            }
        }
        EditOp::CommentReply { comment_id, text } => {
            if comment_id.is_empty() || !comment_id.bytes().all(|b| b.is_ascii_digit()) {
                return Err("comment_id must be decimal".into());
            }
            if text.is_empty() {
                return Err("comment text must be non-empty".into());
            }
        }
        EditOp::CommentSetStatus { comment_id, .. } | EditOp::CommentDelete { comment_id } => {
            if comment_id.is_empty() || !comment_id.bytes().all(|b| b.is_ascii_digit()) {
                return Err("comment_id must be decimal".into());
            }
        }
        EditOp::RevisionSettle {
            target: RevisionTarget::Id(id),
            ..
        } => {
            if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
                return Err("revision target must be decimal".into());
            }
        }
        EditOp::RevisionSettle {
            target: RevisionTarget::All,
            ..
        }
        | EditOp::SetEvenAndOddHeaders { .. } => {}
        EditOp::SetFirstPageMode { section, .. }
        | EditOp::SetHeader { section, .. }
        | EditOp::SetFooter { section, .. }
        | EditOp::ClearHeader { section, .. }
        | EditOp::ClearFooter { section, .. } => {
            if section == &Some(0) {
                return Err("section must be positive".into());
            }
        }
    }
    Ok(())
}
fn validate_bookmark_select(select: &Option<String>, occurrence: &Option<u32>) -> Result<(), String> {
    if select.as_deref() == Some("") {
        return Err("select must be non-empty; omit it to bookmark the whole paragraph".into());
    }
    if occurrence == &Some(0) {
        return Err("occurrence must be positive".into());
    }
    if occurrence.is_some() && select.is_none() {
        return Err("occurrence requires select".into());
    }
    Ok(())
}
fn diag(code: impl Into<String>, message: impl Into<String>, path: Option<String>) -> Diagnostic {
    Diagnostic {
        code: code.into(),
        message: message.into(),
        path,
        span: None,
    }
}
fn diag_span(
    code: impl Into<String>,
    message: String,
    path: Option<String>,
    text: &str,
) -> Diagnostic {
    let span = path
        .as_deref()
        .and_then(|path| exact_field_span(text, path))
        .or_else(|| {
            path.as_deref()
                .and_then(|path| path.rsplit('.').next())
                .and_then(|key| {
                    text.find(key)
                        .map(|start| make_span(text, start, start + key.len()))
                })
        })
        .or_else(|| (!text.is_empty()).then_some(make_span(text, 0, text.len().min(1))));
    Diagnostic {
        code: code.into(),
        message,
        path,
        span,
    }
}
fn exact_field_span(text: &str, path: &str) -> Option<Span> {
    let rest = path.strip_prefix("ops[")?;
    let (index, field) = rest.split_once("]")?;
    let index = index.parse::<usize>().ok()?;
    let field = field.strip_prefix('.')?;
    if text.contains("[[ops]]") {
        let mut offset = 0usize;
        for (i, section) in text.split("[[ops]]").skip(1).enumerate() {
            let start = text[offset..].find("[[ops]]")? + offset;
            offset = start + "[[ops]]".len();
            if i == index {
                let at = section.find(field)? + offset;
                return Some(make_span(text, at, at + field.len()));
            }
        }
        return None;
    }
    let array_start = text.find("\"ops\"")?;
    let mut depth = 0i32;
    let mut object_start = None;
    let mut objects = Vec::new();
    for (offset, ch) in text[array_start..].char_indices() {
        match ch {
            '[' => depth += 1,
            '{' if depth > 0 => {
                if depth == 1 {
                    object_start = Some(array_start + offset);
                }
                depth += 1;
            }
            '}' if depth > 1 => {
                depth -= 1;
                if depth == 1 {
                    if let Some(start) = object_start.take() {
                        objects.push((start, array_start + offset + 1));
                    }
                }
            }
            ']' if depth > 0 => depth -= 1,
            _ => {}
        }
    }
    let (start, end) = *objects.get(index)?;
    let local = text[start..end].find(field)?;
    Some(make_span(text, start + local, start + local + field.len()))
}
fn make_span(text: &str, start: usize, end: usize) -> Span {
    fn lc(text: &str, at: usize) -> (usize, usize) {
        let prefix = &text[..at.min(text.len())];
        let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
        let column = prefix
            .rsplit('\n')
            .next()
            .map_or(1, |s| s.chars().count() + 1);
        (line, column)
    }
    let (line, column) = lc(text, start);
    let (end_line, end_column) = lc(text, end);
    Span {
        line,
        column,
        end_line: Some(end_line),
        end_column: Some(end_column),
    }
}
pub fn execute_plan(input: &[u8], request: &PlanRequest) -> PlanResult {
    let source = SourceHash::from_bytes(input);
    let plan = &request.plan;
    if let Err(error) = validate_plan(plan.clone()) {
        return PlanResult::Rejected {
            source: Some(source),
            plan: None,
            diagnostic: error,
            report: None,
        };
    }
    if source != plan.base {
        return PlanResult::Rejected {
            source: Some(source),
            plan: Some(plan.clone()),
            diagnostic: diag(
                "source_mismatch",
                "plan base does not match input bytes",
                Some("base".into()),
            ),
            report: None,
        };
    }
    let toml = canonical_toml(plan).unwrap_or_default();
    let expected = PreviewKey::new(plan);
    if let Some(key) = &request.preview_key {
        if key != &expected {
            return PlanResult::Rejected {
                source: Some(source),
                plan: Some(plan.clone()),
                diagnostic: diag(
                    "preview_key_mismatch",
                    "preview key does not match source and canonical plan",
                    Some("preview_key".into()),
                ),
                report: None,
            };
        }
    }
    let now = match revision_time(request.now.as_deref()) {
        Ok(now) => now,
        Err(diagnostic) => {
            return PlanResult::Rejected { source: Some(source), plan: Some(plan.clone()), diagnostic, report: None };
        }
    };
    let (report, bytes, failure) = fold_typed(input, plan, request.preview_key.is_some(), now);
    if let Some((diagnostic, report)) = failure {
        return PlanResult::Rejected {
            source: Some(source),
            plan: Some(plan.clone()),
            diagnostic,
            report: Some(report),
        };
    }
    if request.preview_key.is_some() {
        PlanResult::Committed {
            source,
            plan: plan.clone(),
            canonical_toml: toml,
            report,
            bytes: bytes.unwrap_or_default(),
        }
    } else {
        PlanResult::Previewed {
            source,
            plan: plan.clone(),
            canonical_toml: toml,
            preview_key: expected,
            report,
        }
    }
}
/// The time a commit stamps: `given` when valid, else the current UTC time.
pub fn revision_time(given: Option<&str>) -> Result<String, Diagnostic> {
    match given {
        Some(value) => {
            let valid = value.len() == 20
                && value.bytes().enumerate().all(|(i, b)| match i {
                    4 | 7 => b == b'-',
                    10 => b == b'T',
                    13 | 16 => b == b':',
                    19 => b == b'Z',
                    _ => b.is_ascii_digit(),
                });
            if valid {
                Ok(value.to_string())
            } else {
                Err(diag("invalid_now", format!("now must be a UTC time YYYY-MM-DDTHH:MM:SSZ, got {value:?}"), Some("now".into())))
            }
        }
        None => Ok(current_utc()),
    }
}

/// The current UTC time, second precision, as Word writes `w:date`.
fn current_utc() -> String {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // No clock without a host: the WASM binding passes the host's time.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let seconds = 0i64;
    format_utc(seconds)
}

/// `seconds` since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ`.
fn format_utc(seconds: i64) -> String {
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Civil date from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}

#[cfg(test)]
mod clock_tests {
    #[test]
    fn civil_dates() {
        assert_eq!(super::format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::format_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(super::format_utc(1_793_450_096), "2026-10-31T12:34:56Z");
    }
}

fn fold_typed(
    input: &[u8],
    plan: &Plan,
    commit: bool,
    now: String,
) -> (
    PlanReport,
    Option<Vec<u8>>,
    Option<(Diagnostic, PlanReport)>,
) {
    let mut state = match crate::commands::WorkState::parse(input) {
        Ok(s) => s,
        Err(error) => {
            let report = empty_report(plan);
            return (
                report.clone(),
                None,
                Some((diag("source_invalid", error.summary, None), report)),
            );
        }
    };
    let document_part = state.package.document_part_name();
    let seed = state
        .source_hash
        .unwrap_or_else(|| crate::commands::package_sha256(input));
    let repair_index = state.xml.resolve_paragraph_ids(&document_part, &seed);
    if let Some(error) = &repair_index.allocation_error {
        let report = empty_report(plan);
        return (
            report.clone(),
            None,
            Some((diag("paragraph_id_allocation", error.clone(), None), report)),
        );
    }
    let repaired_ids = repair_index
        .repaired
        .iter()
        .filter_map(|node| repair_index.ids.get(node).map(|(id, _)| id.clone()))
        .collect::<Vec<_>>();
    if let Err(error) = state.xml.apply_paragraph_ids(&repair_index) {
        let report = empty_report(plan);
        return (
            report.clone(),
            None,
            Some((diag("paragraph_id_repair", error, None), report)),
        );
    }
    let mut aliases: BTreeMap<String, Value> = BTreeMap::new();
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut reports = Vec::new();
    let mut affected_ids: Vec<Vec<String>> = Vec::new();
    let plan_seed = crate::commands::plan_seed(plan);
    let mut allocated_ids = Vec::new();
    let ctx = crate::commands::ExecCtx { dry_run: !commit, now };
    for (index, op) in plan.ops.iter().enumerate() {
        let resolved = match resolve_typed_aliases(op, &aliases, &groups) {
            Ok(value) => value,
            Err(error) => {
                attach_affected(&mut state, &mut reports, &affected_ids);
                let report = PlanReport {
                    completed: index,
                    stopped_at: Some(index),
                    ops: reports,
                    repaired_ids: repaired_ids.clone(),
                    allocated_ids: allocated_ids.clone(),
                    aliases: aliases.clone().into_iter().collect(),
                    groups: group_map(&groups),
                };
                return (report.clone(), None, Some((error, report)));
            }
        };
        let name = operation_name(op).into();
        let outcome = crate::commands::run_typed_edit(
            &mut state,
            &resolved,
            &plan.author,
            plan.change_mode,
            index,
            &seed,
            &plan_seed,
            &ctx,
        );
        let report_value = outcome.result_json();
        let applied = outcome.status == crate::outcome::Status::Ok;
        affected_ids.push(crate::commands::affected_para_ids(&report_value));
        reports.push(OperationReport {
            index,
            op: name,
            outcome: if applied { "applied" } else { "rejected" }.into(),
            summary: outcome.summary.clone(),
            affected: Vec::new(),
            context_truncated: false,
        });
        if !applied {
            if let Some(note) = explain_selector_drift(input, plan, &resolved, index, &affected_ids) {
                if let Some(report) = reports.last_mut() {
                    report.summary.push_str(&note);
                }
            }
            attach_affected(&mut state, &mut reports, &affected_ids);
            let report = PlanReport {
                completed: index,
                stopped_at: Some(index),
                ops: reports,
                repaired_ids: repaired_ids.clone(),
                allocated_ids: allocated_ids.clone(),
                aliases: aliases.clone().into_iter().collect(),
                groups: group_map(&groups),
            };
            let message = report.ops[index].summary.clone();
            return (
                report.clone(),
                None,
                Some((
                    diag("operation_rejected", message, Some(format!("ops[{index}]"))),
                    report,
                )),
            );
        }
        if let EditOp::InsertParagraph { alias: Some(alias), .. }
            | EditOp::InsertTable { alias: Some(alias), .. }
            | EditOp::EditTable { action: TableEdit::InsertRows { alias: Some(alias), .. } | TableEdit::InsertColumns { alias: Some(alias), .. }, .. } = op
        {
            if aliases.contains_key(alias) || groups.contains_key(alias) {
                attach_affected(&mut state, &mut reports, &affected_ids);
                let report = PlanReport {
                    completed: index,
                    stopped_at: Some(index),
                    ops: reports,
                    repaired_ids: repaired_ids.clone(),
                    allocated_ids: allocated_ids.clone(),
                    aliases: aliases.clone().into_iter().collect(),
                    groups: group_map(&groups),
                };
                return (
                    report.clone(),
                    None,
                    Some((
                        diag(
                            "duplicate_alias",
                            format!("alias {alias:?} is already defined"),
                            Some(format!("ops[{index}].as")),
                        ),
                        report,
                    )),
                );
            }
            if matches!(op, EditOp::InsertTable { .. } | EditOp::EditTable { .. }) {
                aliases.insert(alias.clone(), Value::String(format!("${alias}")));
            }
            let result = outcome.result.as_ref();
            // Cells of an inserted table bind as `$<as>_rNcM`; the items of a
            // list `insert_paragraph` as `$<as>_N`.
            let mut members: Vec<(String, Value)> = Vec::new();
            if let Some(rows) = result.and_then(|r| r.get("cells")).and_then(Value::as_array) {
                members.extend(rows.iter().enumerate().flat_map(|(r, row)| {
                    row.as_array().into_iter().flatten().enumerate().map(move |(c, id)| (format!("{alias}_r{}c{}", r + 1, c + 1), id.clone()))
                }));
            }
            if let Some(ids) = result.and_then(|r| r.get("paraIds")).and_then(Value::as_array) {
                let ids: Vec<String> = ids.iter().map(|id| id.as_str().unwrap_or_default().to_owned()).collect();
                for (n, id) in ids.iter().enumerate() {
                    allocated_ids.push(id.clone());
                    members.push((format!("{alias}_{}", n + 1), Value::String(id.clone())));
                }
                groups.insert(alias.clone(), ids);
            }
            let (kind, parent) = if matches!(op, EditOp::InsertParagraph { .. }) { ("paragraph alias", "alias") } else { ("cell alias", "table alias") };
            for (name, id) in members {
                if aliases.contains_key(&name) || groups.contains_key(&name) {
                    attach_affected(&mut state, &mut reports, &affected_ids);
                    let report = PlanReport {
                        completed: index,
                        stopped_at: Some(index),
                        ops: reports,
                        repaired_ids: repaired_ids.clone(),
                        allocated_ids: allocated_ids.clone(),
                        aliases: aliases.clone().into_iter().collect(),
                        groups: group_map(&groups),
                    };
                    return (
                        report.clone(),
                        None,
                        Some((
                            diag(
                                "duplicate_alias",
                                format!("{kind} {name:?} of {parent} {alias:?} is already defined"),
                                Some(format!("ops[{index}].as")),
                            ),
                            report,
                        )),
                    );
                }
                aliases.insert(name, id);
            }
            if let Some(id) = outcome
                .result
                .as_ref()
                .and_then(|r| r.get("paraId"))
                .cloned()
            {
                allocated_ids.push(id.as_str().unwrap_or_default().into());
                aliases.insert(alias.clone(), id);
            }
        }
        if let EditOp::InsertTable { .. } = op {
            let rows = outcome.result.as_ref().and_then(|r| r.get("cells")).and_then(Value::as_array);
            for id in rows.into_iter().flatten().filter_map(Value::as_array).flatten() {
                allocated_ids.push(id.as_str().unwrap_or_default().into());
            }
        }
    }
    attach_affected(&mut state, &mut reports, &affected_ids);
    let report = PlanReport {
        completed: plan.ops.len(),
        stopped_at: None,
        ops: reports,
        repaired_ids,
        allocated_ids,
        aliases: aliases.into_iter().collect(),
        groups: group_map(&groups),
    };
    let out = crate::commands::emit(
        state,
        crate::outcome::Outcome::ok("plan applied", serde_json::json!({})),
        &ctx,
    );
    if out.status != crate::outcome::Status::Ok {
        return (
            report.clone(),
            None,
            Some((diag("package_write", out.summary, None), report)),
        );
    }
    // Review and commit apply the same gate to the same bytes, so a plan
    // that reviews clean commits clean.
    if let Some(bytes) = &out.bytes {
        if let Err(failure) = crate::integrity::gate(input, bytes) {
            let diagnostic = diag("integrity_regression", failure.summary(), None);
            return (report.clone(), None, Some((diagnostic, report)));
        }
    }
    (report, if commit { out.bytes } else { None }, None)
}
/// When a selector matched nothing but matches the same paragraph as it was
/// before the plan, name the earlier operations that changed that text:
/// selectors address the text left by preceding operations, not the read.
fn explain_selector_drift(
    input: &[u8],
    plan: &Plan,
    op: &EditOp,
    index: usize,
    affected_ids: &[Vec<String>],
) -> Option<String> {
    let (at, selects): (&ParagraphAddress, Vec<&str>) = match op {
        EditOp::ReplaceText { at, select, .. } | EditOp::FormatText { at, select, .. } => {
            (at, vec![select.as_str()])
        }
        EditOp::CommentAdd {
            at,
            select: Some(select),
            ..
        }
        | EditOp::BookmarkAdd {
            at,
            select: Some(select),
            ..
        }
        | EditOp::BookmarkMove {
            at,
            select: Some(select),
            ..
        } => (at, vec![select.as_str()]),
        EditOp::ReviseText {
            at: Some(at),
            within,
            edits,
            ..
        } => (
            at,
            within
                .iter()
                .map(String::as_str)
                .chain(edits.iter().map(|e| e.select.as_str()))
                .collect(),
        ),
        _ => return None,
    };
    if index == 0 {
        return None;
    }
    let ParagraphAddress::Id(id) = at else {
        return None;
    };
    let id = id.to_uppercase();
    let base = crate::commands::base_visible_text(input, &id)?;
    if !selects.iter().all(|select| base.contains(select)) {
        return None;
    }
    let earlier: Vec<String> = plan.ops[..index]
        .iter()
        .enumerate()
        .filter(|(i, earlier)| {
            matches!(earlier, EditOp::RevisionSettle { .. })
                || affected_ids[*i].iter().any(|touched| touched.eq_ignore_ascii_case(&id))
        })
        .map(|(i, earlier)| format!("ops[{i}] {}", operation_name(earlier)))
        .collect();
    if earlier.is_empty() {
        return None;
    }
    Some(format!(
        "; it matches this paragraph as it was before the plan, but {} changed the text first — selectors address the text left by preceding operations (combine edits to one paragraph in a single revise_text, or settle revisions in a separate plan and re-read)",
        earlier.join(", ")
    ))
}

/// Fill each operation report with the rendered markup of the paragraphs it
/// affected, as they stand in `state`, from one render of the document.
fn attach_affected(
    state: &mut crate::commands::WorkState,
    reports: &mut [OperationReport],
    affected_ids: &[Vec<String>],
) {
    state.flush_lists();
    let wanted = affected_ids.iter().flatten().cloned().collect();
    let markup = crate::commands::affected_markup(state, &wanted);
    for (report, ids) in reports.iter_mut().zip(affected_ids) {
        report.affected = ids
            .iter()
            .filter_map(|id| {
                markup
                    .get(id)
                    .map(|m| serde_json::json!({"para_id": id, "markup": m}))
            })
            .collect();
    }
}
fn empty_report(_plan: &Plan) -> PlanReport {
    PlanReport {
        completed: 0,
        stopped_at: None,
        ops: Vec::new(),
        repaired_ids: Vec::new(),
        allocated_ids: Vec::new(),
        aliases: Map::new(),
        groups: Map::new(),
    }
}
fn operation_name(op: &EditOp) -> &'static str {
    match op {
        EditOp::ReplaceEquation { .. } => "replace_equation",
        EditOp::DeleteEquation { .. } => "delete_equation",
        EditOp::ReplaceText { .. } => "replace_text",
        EditOp::ReviseText { .. } => "revise_text",
        EditOp::ReplaceParagraph { .. } => "replace_paragraph",
        EditOp::EditTable { .. } => "edit_table",
        EditOp::InsertTable { .. } => "insert_table",
        EditOp::FormatTable { .. } => "format_table",
        EditOp::FormatRow { .. } => "format_row",
        EditOp::FormatCell { .. } => "format_cell",
        EditOp::FormatText { .. } => "format_text",
        EditOp::FormatParagraph { .. } => "format_paragraph",
        EditOp::SetList { .. } => "set_list",
        EditOp::InsertParagraph { .. } => "insert_paragraph",
        EditOp::DeleteParagraphs { .. } => "delete_paragraphs",
        EditOp::SetPageMargins { .. } => "set_page_margins",
        EditOp::SetSectionBoundary { .. } => "set_section_boundary",
        EditOp::SetSectionLayout { .. } => "set_section_layout",
        EditOp::SetFirstPageMode { .. } => "set_first_page_mode",
        EditOp::SetEvenAndOddHeaders { .. } => "set_even_and_odd_headers",
        EditOp::SetHeader { .. } => "set_header",
        EditOp::SetFooter { .. } => "set_footer",
        EditOp::ClearHeader { .. } => "clear_header",
        EditOp::ClearFooter { .. } => "clear_footer",
        EditOp::CommentAdd { .. } => "comment_add",
        EditOp::CommentReply { .. } => "comment_reply",
        EditOp::CommentSetStatus { .. } => "comment_set_status",
        EditOp::CommentDelete { .. } => "comment_delete",
        EditOp::BookmarkAdd { .. } => "bookmark_add",
        EditOp::BookmarkRename { .. } => "bookmark_rename",
        EditOp::BookmarkMove { .. } => "bookmark_move",
        EditOp::BookmarkDelete { .. } => "bookmark_delete",
        EditOp::RevisionSettle { .. } => "revision_settle",
    }
}
fn group_map(groups: &BTreeMap<String, Vec<String>>) -> Map<String, Value> {
    groups.iter().map(|(name, ids)| (name.clone(), serde_json::json!(ids))).collect()
}

fn resolve_typed_aliases(
    op: &EditOp,
    aliases: &BTreeMap<String, Value>,
    groups: &BTreeMap<String, Vec<String>>,
) -> Result<EditOp, Diagnostic> {
    let mut resolved = op.clone();
    // A single-paragraph field cannot take a group: which member is meant is
    // the author's call, so name it.
    let resolve = |address: &mut ParagraphAddress| -> Result<(), Diagnostic> {
        if let ParagraphAddress::Alias(name) = address {
            if let Some(ids) = groups.get(name.as_str()) {
                return Err(diag(
                    "group_alias",
                    format!("${name} names {} paragraphs; address one as ${name}_N", ids.len()),
                    None,
                ));
            }
            let Some(Value::String(id)) = aliases.get(name) else {
                return Err(diag(
                    "unknown_alias",
                    format!("unknown plan-local alias ${name}"),
                    None,
                ));
            };
            *address = ParagraphAddress::id(id);
        }
        Ok(())
    };
    // An insert anchored on a group goes after its last member or before
    // its first.
    let resolve_anchor = |address: &mut ParagraphAddress, position: InsertPosition| -> Result<(), Diagnostic> {
        if let ParagraphAddress::Alias(name) = address {
            if let Some(ids) = groups.get(name.as_str()) {
                let id = match position {
                    InsertPosition::Before => ids.first(),
                    InsertPosition::After => ids.last(),
                };
                *address = ParagraphAddress::id(id.expect("groups are never empty"));
                return Ok(());
            }
        }
        resolve(address)
    };
    // A paragraph list takes every member of a group, in order.
    let resolve_list = |at: &mut Vec<ParagraphAddress>| -> Result<(), Diagnostic> {
        let mut expanded = Vec::with_capacity(at.len());
        for mut address in at.drain(..) {
            if let ParagraphAddress::Alias(name) = &address {
                if let Some(ids) = groups.get(name.as_str()) {
                    expanded.extend(ids.iter().map(ParagraphAddress::id));
                    continue;
                }
            }
            resolve(&mut address)?;
            expanded.push(address);
        }
        *at = expanded;
        Ok(())
    };
    match &mut resolved {
        EditOp::ReplaceEquation { at, .. }
        | EditOp::DeleteEquation { at, .. }
        | EditOp::ReplaceText { at, .. }
        | EditOp::ReplaceParagraph { at, .. }
        | EditOp::FormatText { at, .. }
        | EditOp::SetSectionBoundary { at, .. }
        | EditOp::FormatParagraph { at, .. }
        | EditOp::CommentAdd { at, .. }
        | EditOp::BookmarkAdd { at, .. }
        | EditOp::BookmarkMove { at, .. } => resolve(at)?,
        EditOp::ReviseText { at: Some(at), .. } => resolve(at)?,
        EditOp::InsertTable { at, position, .. } => resolve_anchor(at, *position)?,
        EditOp::InsertParagraph { at, position, .. } => {
            if let InsertAnchor::Paragraph(addr) = at {
                resolve_anchor(addr, *position)?;
            }
        }
        EditOp::DeleteParagraphs { at } | EditOp::SetList { at, .. } => resolve_list(at)?,
        EditOp::SetSectionLayout { section_handle:Some(handle),.. } => {
            if let Some(alias)=handle.strip_prefix("section:$") {
                let Some(Value::String(id))=aliases.get(alias) else { return Err(diag("unknown_alias",format!("unknown section paragraph alias ${alias}"),None)); };
                *handle=format!("section:{id}");
            }
        }
        _ => {}
    }
    Ok(resolved)
}

pub fn execute_request(input: Option<&[u8]>, request: &Request) -> RequestResult {
    match request {
        Request::Plan(p) => RequestResult::Plan(execute_plan(input.unwrap_or_default(), p)),
        Request::Command(c) => RequestResult::Command(execute_command(input, c)),
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub enum RequestResult {
    Command(CommandResult),
    Plan(PlanResult),
}
pub fn execute_command(input: Option<&[u8]>, r: &CommandRequest) -> CommandResult {
    let source = input.map(SourceHash::from_bytes);
    if let (Some(expected), Some(actual)) = (&r.expected_source, &source) {
        if expected != actual {
            return CommandResult::Rejected {
                source,
                result: None,
                diagnostic: diag(
                    "source_mismatch",
                    "expected source does not match input bytes",
                    Some("expected_source".into()),
                ),
            };
        }
    }
    if let Command::Edit(edit) = &r.command {
        let Some(bytes) = input else {
            return CommandResult::Rejected {
                source,
                result: None,
                diagnostic: diag(
                    "input_required",
                    "edit commands require input bytes",
                    Some("command".into()),
                ),
            };
        };
        let plan = Plan {
            base: SourceHash::from_bytes(bytes),
            author: edit.author.clone(),
            change_mode: edit.change_mode,
            ops: vec![edit.op.clone()],
        };
        let plan = match validate_plan(plan) {
            Ok(plan) => plan,
            Err(diagnostic) => {
                return CommandResult::Rejected {
                    source,
                    result: None,
                    diagnostic,
                }
            }
        };
        let now = match revision_time(r.now.as_deref()) {
            Ok(now) => now,
            Err(diagnostic) => return CommandResult::Rejected { source, result: None, diagnostic },
        };
        let (report, output, failure) = fold_typed(bytes, &plan, true, now);
        if let Some((diagnostic, _)) = failure {
            return CommandResult::Rejected {
                source,
                result: Some(serde_json::to_value(report).unwrap()),
                diagnostic,
            };
        }
        return CommandResult::Completed {
            source,
            result: Some(serde_json::to_value(report).unwrap()),
            bytes: output,
        };
    }
    let out = match &r.command {
        Command::Create(CreateCommand { paragraphs, html }) => {
            let mut command = Map::new();
            if let Some(h) = html {
                command.insert("html".into(), Value::String(h.clone()));
            } else {
                command.insert(
                    "paragraphs".into(),
                    serde_json::to_value(paragraphs).unwrap(),
                );
            }
            crate::commands::readonly::create(
                &Value::Object(command),
                &crate::commands::ExecCtx {
                    dry_run: false,
                    now: "2000-01-01T00:00:00Z".into(),
                },
            )
        }
        Command::Read(ReadCommand { read_kind, view }) => typed_read(
            input,
            read_kind.as_deref().unwrap_or("document"),
            view.as_deref(),
        ),
        Command::Find(FindCommand {
            query,
            ignore_case,
            view,
        }) => typed_find(input, query, *ignore_case, *view),
        Command::Edit(_) => unreachable!(),
    };
    if out.status == crate::outcome::Status::Ok {
        CommandResult::Completed {
            source,
            result: out.result,
            bytes: out.bytes,
        }
    } else {
        CommandResult::Rejected {
            source,
            result: out.result,
            diagnostic: diag("command_rejected", out.summary, None),
        }
    }
}

fn typed_read(input: Option<&[u8]>, kind: &str, view: Option<&str>) -> crate::outcome::Outcome {
    let Some(bytes) = input else {
        return crate::outcome::Outcome::error("read commands require input bytes");
    };
    // Listing kinds ignore `view`; agents often pass view=markup on every
    // read, and rejecting that made comments/styles/revisions look broken.
    let state = match crate::commands::WorkState::parse(bytes) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let source = state.source_hash.as_ref();
    let mut command = Map::new();
    if let Some(v) = view {
        let paragraph_view = match v {
            "original" => "baseline",
            "final" => "current",
            "markup" => "all",
            other => other,
        };
        command.insert("view".into(), Value::String(paragraph_view.into()));
    }
    match kind {
        "document" | "document_ui" => {
            let ui = kind == "document_ui";
            let result = if ui {
                crate::commands::readonly::read(
                    &state.package,
                    &state.xml,
                    &Value::Object(command),
                    source,
                )
            } else {
                crate::outcome::Outcome::ok("read complete document", Value::Null)
            };
            let render_view = match crate::html::render::RenderView::from_json(
                view.map(|value| Value::String(value.into())).as_ref(),
            ) {
                Ok(view) => view,
                Err(error) => return crate::outcome::Outcome::error(error),
            };
            let rendered = crate::html::surface::render_document(
                &state.package,
                &state.xml,
                render_view,
                source,
            );
            let paragraph_ids = state.xml.resolve_paragraph_ids(
                &state.package.document_part_name(),
                source.unwrap_or(&[0; 32]),
            );
            if !ui {
                let equations: Vec<Value> = state
                    .xml
                    .paragraphs()
                    .iter()
                    .flat_map(|p| {
                        let at = paragraph_ids.ids.get(&p.node).map(|(id, _)| id.clone());
                        crate::commands::equations::descriptors(&state.xml, p.node)
                            .into_iter()
                            .map(move |mut e| {
                                e["at"] = serde_json::json!(at);
                                e
                            })
                    })
                    .collect();
                let mut value =
                    crate::commands::readonly::inspection_header(source, paragraph_ids.repairs);
                let map = value.as_object_mut().unwrap();
                map.insert("kind".into(), serde_json::json!("document"));
                map.insert("view".into(), serde_json::json!(view.unwrap_or("markup")));
                map.insert("projection_version".into(), serde_json::json!(4));
                map.insert("content_type".into(), serde_json::json!("text/html"));
                let crate::html::compact::AgentRead {
                    markup,
                    mut catalog,
                    default_style,
                } = crate::html::compact::agent_read(&rendered.html, &state.package);
                // Declarations only for styles the markup uses; `read_kind:
                // "styles"` has every definition.
                let mut used = crate::html::compact::style_ids(&markup);
                used.extend(default_style.clone());
                let other_styles: Vec<String> =
                    catalog.keys().filter(|id| !used.contains(*id)).cloned().collect();
                catalog.retain(|id, _| used.contains(id));
                map.insert("markup".into(), serde_json::json!(markup));
                map.insert("tables".into(), crate::commands::tables::agent_inventory(&state.xml));
                map.insert("sections".into(), crate::commands::prune_empty(crate::commands::sections::inventory(&state.package,&state.xml,&paragraph_ids,view==Some("original"))));
                map.insert("styles".into(), serde_json::json!(catalog));
                if let Some(id) = default_style {
                    map.insert("default_style".into(), serde_json::json!(id));
                }
                if !other_styles.is_empty() {
                    map.insert("other_styles".into(), serde_json::json!(other_styles));
                }
                if !equations.is_empty() {
                    map.insert("equations".into(), serde_json::json!(equations));
                }
                let selections = crate::commands::reading::selections(&state.xml, &paragraph_ids);
                if !selections.is_empty() {
                    map.insert(
                        "selection_space".into(),
                        serde_json::json!({"view":"final","offset_unit":"unicode_scalar"}),
                    );
                    map.insert(
                        "selections".into(),
                        serde_json::json!({"paragraphs":selections}),
                    );
                }
                let mut assets = crate::html::assets::manifest(&state.package, false);
                if !assets.is_empty() {
                    let markup = crate::html::compact::asset_aliases(
                        map["markup"].as_str().unwrap(),
                        &mut assets,
                    );
                    map.insert("markup".into(), serde_json::json!(markup));
                    map.insert("assets".into(), serde_json::json!(assets));
                }
                match crate::commands::comments::comment_threads(&state.package) {
                    Ok(mut comments) if !comments.is_empty() => {
                        let anchors =
                            crate::commands::reading::comment_anchors(&state.xml, &paragraph_ids);
                        for thread in &mut comments {
                            let id = thread["id"].as_str().unwrap_or("").to_string();
                            let mut ranges = anchors.get(&id).cloned().unwrap_or_default();
                            for range in &mut ranges {
                                range.as_object_mut().unwrap().remove("locator");
                            }
                            let mut root = thread["root"].clone();
                            let obj = root.as_object_mut().unwrap();
                            for key in ["id", "threadId", "parentId", "ooxmlCommentId", "durableId"]
                            {
                                obj.remove(key);
                            }
                            let mut replies = thread["replies"].clone();
                            for reply in replies.as_array_mut().unwrap() {
                                let obj = reply.as_object_mut().unwrap();
                                for key in ["threadId", "parentId", "ooxmlCommentId"] {
                                    obj.remove(key);
                                }
                            }
                            *thread = serde_json::json!({"id":id,"durableId":thread["durableId"],"status":thread["status"],"anchor":{"ranges":ranges},"root":root,"replies":replies});
                        }
                        map.insert(
                            "selection_space".into(),
                            serde_json::json!({"view":"final","offset_unit":"unicode_scalar"}),
                        );
                        map.insert("comments".into(), serde_json::json!(comments));
                    }
                    Err(error) => {
                        map.insert("comments_error".into(), serde_json::json!(error.summary));
                    }
                    _ => {}
                }
                return crate::outcome::Outcome::ok("read complete document", value);
            }
            let source_map: Vec<Value> = rendered.regions.iter().filter(|r|!r.paragraphs.is_empty()).map(|r| {
                let editable=r.kind==crate::html::surface::SurfaceRegionKind::Body;
                let paragraphs:Vec<_>=if editable {r.paragraphs.iter().filter_map(|n|paragraph_ids.ids.get(n).map(|(id,_)|id.clone())).collect()} else {vec![]};
                // Chrome nodes belong to another XML tree and use dedicated
                // operations; they must never borrow main-document NodeIds.
                serde_json::json!({"html_start":r.start,"html_end":r.end,"part":r.part,"editable":editable,"paragraphs":paragraphs})
            }).collect();
            let blocks: Vec<Value> = source_map.iter().filter(|r| r["editable"] == true).map(|r| {
                serde_json::json!({"html_start":r["html_start"],"html_end":r["html_end"],"paragraphs":r["paragraphs"]})
            }).collect();
            let mut result = result;
            if let Some(map) = result.result.as_mut().and_then(Value::as_object_mut) {
                map.insert("sections".into(),crate::commands::sections::inventory(&state.package,&state.xml,&paragraph_ids,view==Some("original")));
                if let Some(Value::Array(paragraphs)) = map.get("paragraphs") {
                    let equations: Vec<Value> = paragraphs
                        .iter()
                        .flat_map(|p| {
                            p.get("equations")
                                .and_then(Value::as_array)
                                .into_iter()
                                .flatten()
                                .map(move |e| {
                                    let mut e = e.clone();
                                    e["at"] = p["id"].clone();
                                    e
                                })
                        })
                        .collect();
                    if !equations.is_empty() {
                        map.insert("equations".into(), serde_json::json!(equations));
                    }
                }
                map.remove("comments");
                map.insert("blocks".into(), Value::Array(blocks));
                map.insert("source_map".into(),serde_json::json!({"html_offset_unit":"utf8_byte","text_offset_unit":"unicode_scalar","regions":source_map}));
                map.insert("projection_version".into(), serde_json::json!(4));
                map.insert("content_type".into(), serde_json::json!("text/html"));
                map.insert("css".into(), serde_json::json!(crate::html::styles::CSS));
                map.insert(
                    "assets".into(),
                    serde_json::json!(crate::html::assets::manifest(&state.package, false)),
                );
                map.insert("tables".into(), crate::commands::tables::inventory(&state.xml));
                map.insert("sections".into(), crate::commands::sections::inventory(&state.package,&state.xml,&paragraph_ids,view==Some("original")));
                map.insert("html".into(), Value::String(rendered.html));
            }
            if let Some(map) = result.result.as_mut().and_then(Value::as_object_mut) {
                map.insert("kind".into(), Value::String("document".into()));
                map.insert(
                    "view".into(),
                    Value::String(view.unwrap_or("markup").into()),
                );
            }
            result
        }
        "assets" => crate::outcome::Outcome::ok(
            "document assets",
            serde_json::json!({"kind":"assets","assets":crate::html::assets::manifest(&state.package,true)}),
        ),
        "styles" => typed_listing(
            crate::commands::readonly::list_styles(&state.package),
            "styles",
            source,
        ),
        "comments" => typed_listing(
            crate::commands::comments::list_comments(&state.package),
            "comments",
            source,
        ),
        "revisions" => typed_listing(
            crate::commands::revisions::list_revisions(&state.package, &state.xml),
            "revisions",
            source,
        ),
        "bookmarks" => typed_listing(
            crate::commands::bookmarks::list_bookmarks(&state.package, &state.xml, source),
            "bookmarks",
            source,
        ),
        other => crate::outcome::Outcome::error(format!("unknown read kind: {other}")),
    }
}
fn typed_listing(
    mut outcome: crate::outcome::Outcome,
    kind: &str,
    source: Option<&[u8; 32]>,
) -> crate::outcome::Outcome {
    if let Some(map) = outcome.result.as_mut().and_then(Value::as_object_mut) {
        map.insert("kind".into(), Value::String(kind.into()));
        if let Some(hash) = source {
            map.insert(
                "source".into(),
                Value::String(format!("sha256:{}", hex::encode(hash))),
            );
        }
    }
    outcome
}
fn typed_find(
    input: Option<&[u8]>,
    query: &str,
    ignore_case: bool,
    view: Option<FindView>,
) -> crate::outcome::Outcome {
    let Some(bytes) = input else {
        return crate::outcome::Outcome::error("find commands require input bytes");
    };
    let state = match crate::commands::WorkState::parse(bytes) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut command = Map::new();
    command.insert("query".into(), Value::String(query.into()));
    command.insert("ignoreCase".into(), Value::Bool(ignore_case));
    if let Some(v) = view {
        let internal = match v {
            FindView::Markup => "all",
            FindView::Final => "current",
            FindView::Original => "baseline",
        };
        command.insert("view".into(), Value::String(internal.into()));
    }
    crate::commands::readonly::find_text(
        &state.package,
        &state.xml,
        &Value::Object(command),
        state.source_hash.as_ref(),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_roundtrip() {
        let s = SourceHash::from_bytes(b"x");
        assert_eq!(SourceHash::parse(&s.as_str()), Ok(s));
    }
    #[test]
    fn points_reject_non_twentieth() {
        assert!(Points::from_points(1.001).is_err());
    }
    #[test]
    fn key_stable() {
        let p = Plan {
            base: SourceHash::from_bytes(b"x"),
            author: "docxdriver".into(),
            change_mode: ChangeMode::Track,
            ops: vec![EditOp::ReplaceText {
                at: ParagraphAddress::id("12345678"),
                select: "a".into(),
                with: "b".into(),
                occurrence: None,
            }],
        };
        assert_eq!(PreviewKey::new(&p), PreviewKey::new(&p));
    }
    #[test]
    fn preview_key_golden_vector() {
        let p = Plan {
            base: SourceHash::parse(
                "sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881",
            )
            .unwrap(),
            author: "docxdriver".into(),
            change_mode: ChangeMode::Track,
            ops: vec![EditOp::ReplaceText {
                at: ParagraphAddress::id("12345678"),
                select: "a".into(),
                with: "b".into(),
                occurrence: None,
            }],
        };
        assert_eq!(
            p.preview_key().0,
            "p1:sha256:595f84cd14bf048be009c8515951d1dd5f37e4d5f66e3385ef4b651e4dea48a4"
        );
    }
    #[test]
    fn canonical_toml_roundtrips() {
        let p = Plan {
            base: SourceHash::from_bytes(b"x"),
            author: "docxdriver".into(),
            change_mode: ChangeMode::Track,
            ops: vec![EditOp::ReplaceText {
                at: ParagraphAddress::id("12345678"),
                select: "a".into(),
                with: "b".into(),
                occurrence: None,
            }],
        };
        let parsed = Plan::from_toml(&p.canonical_toml().unwrap()).unwrap();
        assert_eq!(parsed, p);
    }

    #[test]
    fn read_selector_does_not_collide_with_command_discriminator() {
        let command = Command::Read(ReadCommand {
            read_kind: Some("styles".into()),
            view: None,
        });
        let value = serde_json::to_value(&command).unwrap();
        assert_eq!(value["kind"], "read");
        assert_eq!(value["read_kind"], "styles");
        assert_eq!(serde_json::from_value::<Command>(value).unwrap(), command);
    }

    #[test]
    fn alignment_only_format_is_a_valid_nonempty_operation() {
        let text = r#"{"base":"sha256:2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881","author":"a","change_mode":"track","ops":[{"op":"format_paragraph","at":"12345678","alignment":"center"}]}"#;
        assert!(Plan::from_json(text).is_ok());
    }
}
