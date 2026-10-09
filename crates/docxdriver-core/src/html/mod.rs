//! The agent-facing HTML surface: canonical render + tracked-edit application.
//!
//! docs/agent-surface.md is the contract. The markup render is the canonical
//! editing surface: byte-deterministic, whitespace-significant, one block
//! element per line. `editHtml` matches the render exactly and is validated by
//! the original-projection-preservation invariant — every mutation must be
//! expressed as revision markup, so untracked edits are mechanically rejected.

pub mod assets;
pub mod build;
pub mod chrome;
pub mod compact;
pub mod ctx;
pub mod input;
pub mod mathml_omml;
#[cfg(test)]
pub mod omml_latex;
pub mod omml_mathml;
pub mod parse;
pub mod render;
pub mod styles;
pub mod surface;

/// Vertical alignment for superscript/subscript (`w:vertAlign`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertAlign {
    Superscript,
    Subscript,
}

impl VertAlign {
    pub fn dialect_tag(self) -> &'static str {
        match self {
            VertAlign::Superscript => "sup",
            VertAlign::Subscript => "sub",
        }
    }

    pub fn docx_val(self) -> &'static str {
        match self {
            VertAlign::Superscript => "superscript",
            VertAlign::Subscript => "subscript",
        }
    }

    pub fn from_docx(value: &str) -> Option<VertAlign> {
        match value {
            "superscript" => Some(VertAlign::Superscript),
            "subscript" => Some(VertAlign::Subscript),
            _ => None,
        }
    }
}

/// Bold/italic/underline/strike/super/sub formatting state of a character.
/// Canonical tag nesting order is b → i → u → s → (sup|sub).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fmt {
    /// Explicit disabled properties: bold, italic, underline, strike, baseline.
    pub resets: u8,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub vert_align: Option<VertAlign>,
    pub color: Option<u32>,
    pub font_size: Option<u32>,
    pub font_family: Option<styles::FontFamily>,
}

/// Pending run-format revision metadata (`<b pending …>` / `<sup pending …>` /
/// `<format pending …>` / `w:rPrChange`). `id`/`author` are `None` for a newly
/// authored pending change (engine assigns).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FmtPending {
    pub id: Option<String>,
    pub author: Option<String>,
}

impl FmtPending {}

/// Revision-wrap identity: `None` = not wrapped, `Some(None)` = new
/// agent-authored (id-less) wrapper, `Some(Some(id))` = existing revision.
pub type WrapId = Option<Option<String>>;

/// `w:br` break type as the dialect surfaces it: `<br/>` is an in-paragraph
/// line break; `<br type="page"/>` / `<br type="column"/>` are hard page and
/// column breaks. (`w:type="textWrapping"` is the explicit spelling of the
/// default and renders as a plain `<br/>`.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrKind {
    #[default]
    Line,
    Page,
    Column,
}

impl BrKind {
    /// From the `w:br` element's `w:type` attribute value.
    pub fn from_docx(value: Option<&str>) -> BrKind {
        match value {
            Some("page") => BrKind::Page,
            Some("column") => BrKind::Column,
            _ => BrKind::Line,
        }
    }

    /// From the dialect's `<br type="…"/>` attribute value.
    pub fn from_dialect(value: Option<&str>) -> Result<BrKind, String> {
        match value {
            None => Ok(BrKind::Line),
            Some("page") => Ok(BrKind::Page),
            Some("column") => Ok(BrKind::Column),
            Some(other) => Err(format!(
                "unknown break type \"{other}\" (expected page or column)"
            )),
        }
    }

    /// The dialect attribute value; `None` for a plain line break. Doubles as
    /// the `w:type` value to stamp when creating a `w:br` (same vocabulary).
    pub fn dialect_type(self) -> Option<&'static str> {
        match self {
            BrKind::Line => None,
            BrKind::Page => Some("page"),
            BrKind::Column => Some("column"),
        }
    }
}

/// One unit of paragraph content in the dialect. `Br`/`Tab` mirror `w:br`/
/// `w:tab` — they render as `<br/>` / literal tab but occupy no position in
/// the all-view char space the edit primitives address. `link` is the enclosing
/// `<a href>`, `field` the enclosing `<field instr>` (its cached result text).
/// Links are writable through editHtml; fields remain protected. They can nest
/// (a HYPERLINK field's result contains a hyperlink).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Atom {
    Char {
        ch: char,
        fmt: Fmt,
        /// When set, `fmt` on this character is (or becomes) a tracked
        /// `w:rPrChange` — authored as `<b pending>` / `<i pending>` /
        /// `<u pending>` / `<s pending>` / `<sup pending>` / `<sub pending>` when live
        /// formatting is nonempty, or `<format pending>` when live dialect
        /// format is all off.
        fmt_pending: Option<FmtPending>,
        ins: WrapId,
        del: WrapId,
        link: Option<String>,
        field: Option<String>,
    },
    Br {
        kind: BrKind,
        ins: WrapId,
        del: WrapId,
        link: Option<String>,
        field: Option<String>,
    },
    Tab {
        ins: WrapId,
        del: WrapId,
        link: Option<String>,
        field: Option<String>,
    },
    Milestone {
        start: bool,
        id: String,
    },
    /// An inline footnote/endnote (`<footnote>…</footnote>` /
    /// `<endnote>…</endnote>`). Package note ids are absent from public markup
    /// and recovered from the paragraph's references when editing. `id` is set
    /// after bind-from-XML; authored inserts leave it `None`. `inner` is the
    /// canonical note body (short text or block markup).
    /// `ins`/`del` carry tracked reference insertion/deletion like equations.
    Note {
        endnote: bool,
        id: Option<String>,
        inner: String,
        ins: WrapId,
        del: WrapId,
    },
    /// An equation (`m:oMath`/`m:oMathPara`), rendered as
    /// native `<math>`; its display attribute is preserved. Wrap identity
    /// tracks revisions.
    Equation {
        text: String,
        display: bool,
        ins: WrapId,
        del: WrapId,
    },
    /// An embedded picture, rendered as standard `<img>` (optional `w`/`h`/
    /// `alt`). Package rid is recovered from the XML when editing.
    /// Insertion uses `src="data:…"`; afterward the site re-renders bound.
    Image {
        /// Empty for bound existing sites; `data:…` URL for authored inserts.
        src: String,
        rid: String,
        width: Option<u32>,
        height: Option<u32>,
        alt: Option<String>,
        ins: WrapId,
        del: WrapId,
        link: Option<String>,
        field: Option<String>,
    },
}

impl Atom {
    pub fn wrap(&self) -> Option<(&WrapId, &WrapId)> {
        match self {
            Atom::Char { ins, del, .. }
            | Atom::Br { ins, del, .. }
            | Atom::Tab { ins, del, .. }
            | Atom::Note { ins, del, .. }
            | Atom::Equation { ins, del, .. }
            | Atom::Image { ins, del, .. } => Some((ins, del)),
            Atom::Milestone { .. } => None,
        }
    }

    pub fn link(&self) -> Option<&str> {
        match self {
            Atom::Char { link, .. }
            | Atom::Br { link, .. }
            | Atom::Tab { link, .. }
            | Atom::Image { link, .. } => link.as_deref(),
            Atom::Milestone { .. } | Atom::Note { .. } | Atom::Equation { .. } => None,
        }
    }

    pub fn field(&self) -> Option<&str> {
        match self {
            Atom::Char { field, .. }
            | Atom::Br { field, .. }
            | Atom::Tab { field, .. }
            | Atom::Image { field, .. } => field.as_deref(),
            Atom::Milestone { .. } | Atom::Note { .. } | Atom::Equation { .. } => None,
        }
    }
}

/// Paragraph open-tag data. `tag` is "p" or "h1".."h6"; `break_ins`/`break_del`
/// are the paragraph-mark revisions on the break that *terminates* this
/// paragraph. `num` is the computed list marker and `sect_end` the mid-body
/// section boundary — both pure annotations: the edit walker never compares
/// them; `create` rejects authored values by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListAttrs {
    pub id: u32,
    pub level: u32,
    pub format: String,
    pub start: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParaAttrs {
    pub tag: String,
    pub class: Option<String>,
    pub break_ins: WrapId,
    pub break_del: WrapId,
    pub num: Option<String>,
    pub list: Option<ListAttrs>,
    pub css: Option<String>,
}

/// A parsed fragment item. `Struct` carries table-structure tags verbatim
/// ("table", "/table", "tr", "/tr", "td", "/td") — structure is read-only
/// through editHtml, so old and new must match token-for-token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    ParaOpen(ParaAttrs),
    ParaClose,
    Atom(Atom),
    Struct(String),
}

pub fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn escape_attr(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}
