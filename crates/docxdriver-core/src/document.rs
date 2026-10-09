//! WordprocessingML document model over the xmloxide arena DOM.
//!
//! Name normalization: parsed elements store local names with a separate prefix
//! ("p" + "w"), while elements this engine creates carry the qualified name
//! verbatim ("w:p", no prefix) — the serializer emits both identically. Every
//! lookup below accepts either storage form; every creation uses qualified names.
//!
//! Paragraph text is projected per view, mirroring the reference engine's
//! tracked-changes semantics:
//! - `current`  — suggestions applied: w:ins content shown, w:del content hidden
//! - `baseline` — suggestions rejected: w:ins content hidden, w:delText shown
//! - `all`      — both shown (deleted text still occupies its positions)

use std::collections::{HashMap, HashSet};

use xmloxide::tree::{Document, NodeId};

pub const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
/// The Office 2010 wordprocessing extension namespace (`w14`) that carries
/// `w14:paraId` — the native Word paragraph identifier.
pub const W14_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
/// The markup-compatibility namespace (`mc`) whose `mc:Ignorable` list the
/// engine extends with a prefix when it introduces that prefix's attributes.
pub const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Current,
    Baseline,
    All,
}

impl View {
    pub fn from_json(value: Option<&serde_json::Value>) -> View {
        match value.and_then(|v| v.as_str()) {
            Some("baseline") => View::Baseline,
            Some("all") => View::All,
            _ => View::Current,
        }
    }
}

pub struct DocxXml {
    pub doc: Document,
}

/// A projected paragraph: 1-based index plus its node.
pub struct Paragraph {
    pub index: usize,
    pub node: NodeId,
}

/// Whether an element declares `xmlns:{prefix}` (either storage form).
pub fn has_xmlns(doc: &xmloxide::tree::Document, node: NodeId, prefix: &str) -> bool {
    doc.attributes(node).iter().any(|a| {
        a.name == format!("xmlns:{prefix}")
            || (a.prefix.as_deref() == Some("xmlns") && a.name == prefix)
    })
}

/// True when `node` is already in `uri` / `prefix:` — the part root's own
/// vocabulary, which must not be listed in `mc:Ignorable`.
fn root_carries_prefix(
    doc: &xmloxide::tree::Document,
    node: NodeId,
    prefix: &str,
    uri: &str,
) -> bool {
    if doc.node_namespace(node) == Some(uri) {
        return true;
    }
    match doc.node_name(node) {
        Some(name) => name == prefix || name.strip_prefix(&format!("{prefix}:")).is_some(),
        None => false,
    }
}

/// A valid `w14:paraId`: exactly eight hexadecimal digits with a value
/// greater than `00000000` and less than `80000000`.
pub fn is_valid_para_id(value: &str) -> bool {
    if value.len() != 8 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    u32::from_str_radix(value, 16).is_ok_and(|v| v > 0 && v < 0x8000_0000)
}

/// The canonical address form: exactly eight uppercase hexadecimal digits.
pub fn format_para_id(value: u32) -> String {
    format!("{value:08X}")
}

/// Deterministic paragraph-ID allocation against a part-local occupied set.
///
/// Candidates are derived from the seed material and mapped into
/// `1..=0x7FFFFFFF`; a collision probes forward with wraparound until an
/// unused value is found, so correctness never depends on collision
/// probability. Exhausting the whole 31-bit space is a hard error.
pub struct ParaIdAllocator {
    occupied: HashSet<u32>,
}

impl ParaIdAllocator {
    pub fn new() -> Self {
        ParaIdAllocator {
            occupied: HashSet::new(),
        }
    }

    /// Reserve an existing value; returns false when it was already taken.
    pub fn reserve(&mut self, value: u32) -> bool {
        self.occupied.insert(value)
    }

    /// Allocate the first unused value derived from `seed_material`.
    pub fn allocate(&mut self, seed_material: &[u8]) -> Result<u32, String> {
        for probe in 0..0x7FFF_FFFFu64 {
            let candidate = para_id_candidate(seed_material, probe);
            if self.occupied.insert(candidate) {
                return Ok(candidate);
            }
        }
        Err("paragraph ID space exhausted: no unused w14:paraId remains".to_string())
    }
}

/// The seed material for one paragraph: exact input package hash, part name,
/// and the paragraph's structural path.
fn paragraph_id_seed(source_hash: &[u8; 32], part: &str, path: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(source_hash);
    hasher.update(b"|");
    hasher.update(part.as_bytes());
    hasher.update(b"|");
    hasher.update(path.as_bytes());
    hasher.finalize().to_vec()
}

/// A deterministic candidate in `1..=0x7FFFFFFF` from seed material plus a
/// collision-probe counter.
fn para_id_candidate(seed_material: &[u8], probe: u64) -> u32 {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(seed_material);
    hasher.update(probe.to_le_bytes());
    let digest = hasher.finalize();
    let head = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    (head % 0x7FFF_FFFF) + 1
}

/// The paragraph-identity resolution of one OOXML part: every paragraph's
/// canonical address and 1-based story position, which paragraphs needed
/// provisional repair, and (rarely) a hard allocation failure.
#[derive(Debug, Default)]
pub struct ParaIdIndex {
    /// paragraph node → (canonical eight-uppercase-hex id, story ord)
    pub ids: HashMap<NodeId, (String, usize)>,
    /// Paragraphs that received a provisional value (missing, invalid,
    /// duplicate, or newly created) and therefore need persistence.
    pub repaired: HashSet<NodeId>,
    pub repairs: usize,
    pub allocation_error: Option<String>,
}

impl DocxXml {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let doc = Document::parse_bytes(bytes)
            .map_err(|error| format!("document.xml parse failed: {error}"))?;
        Ok(DocxXml { doc })
    }

    pub fn serialize(&self) -> Vec<u8> {
        xmloxide::serial::serialize(&self.doc).into_bytes()
    }

    /// Local name regardless of storage form ("p" for both parsed `w:p` and created `"w:p"`).
    pub fn local_name(&self, node: NodeId) -> Option<&str> {
        let name = self.doc.node_name(node)?;
        Some(name.rsplit(':').next().unwrap_or(name))
    }

    pub fn is_w(&self, node: NodeId, local: &str) -> bool {
        if !self.doc.is_element(node) {
            return false;
        }
        match self.doc.node_name(node) {
            Some(name) if name == local => {
                let ns = self.doc.node_namespace(node);
                ns == Some(W_NS) || ns.is_none()
            }
            Some(name) => name.strip_prefix("w:") == Some(local),
            None => false,
        }
    }

    /// The `w14:paraId` attribute value, whichever storage form it carries.
    pub fn para_id(&self, node: NodeId) -> Option<&str> {
        self.attr(node, "paraId")
    }

    /// Every `w:p` element anywhere in the part — body story, table cells,
    /// textbox sub-stories, alternate-content branches — in document order.
    /// The occupied-ID set covers all of them: `w14:paraId` uniqueness is per
    /// part, so an id used by a paragraph the typed surface cannot address yet
    /// still blocks re-allocation.
    pub fn all_paragraphs(&self) -> Vec<NodeId> {
        let Some(root) = self.doc.root_element() else {
            return Vec::new();
        };
        self.doc
            .descendants(root)
            .filter(|&node| self.is_w(node, "p"))
            .collect()
    }

    /// Resolve every paragraph's identity in one OOXML part, purely in
    /// memory. Existing valid IDs are reserved first; paragraphs with no
    /// `w14:paraId`, a malformed or out-of-range value, or a value already
    /// used by an earlier paragraph in the part receive deterministic
    /// provisional values derived from the exact input package hash, the
    /// paragraph's structural path, and a collision-probe counter.
    ///
    /// The returned index renders (`ids`) and persists (`apply_paragraph_ids`)
    /// the same addresses, so inspection, preview, and commit agree.
    pub fn resolve_paragraph_ids(&self, part: &str, source_hash: &[u8; 32]) -> ParaIdIndex {
        let mut index = ParaIdIndex::default();
        let mut allocator = ParaIdAllocator::new();

        let all = self.all_paragraphs();
        let story_ord: HashMap<NodeId, usize> = self
            .paragraphs_in(
                self.body()
                    .or_else(|| self.doc.root_element())
                    .unwrap_or_else(|| self.doc.root()),
            )
            .into_iter()
            .map(|paragraph| (paragraph.node, paragraph.index))
            .collect();

        // Reserve existing valid IDs first: a later duplicate loses to the
        // earlier paragraph and is repaired like any other invalid value.
        for &node in &all {
            let Some(value) = self.para_id(node).filter(|v| is_valid_para_id(v)) else {
                continue;
            };
            let parsed = u32::from_str_radix(value, 16).unwrap_or(0);
            if allocator.reserve(parsed) {
                index.ids.insert(node, (format_para_id(parsed), 0));
            }
        }
        // Story position (the rendered `ord`) applies to valid and repaired
        // paragraphs alike.
        for (&node, &ord) in &story_ord {
            if let Some(entry) = index.ids.get_mut(&node) {
                entry.1 = ord;
            }
        }

        // Provisional repair for everything not covered by a reserved value.
        let mut nested = 0usize;
        for &node in &all {
            if index.ids.contains_key(&node) {
                continue;
            }
            let path = match story_ord.get(&node) {
                Some(ord) => format!("story/{ord}"),
                None => {
                    nested += 1;
                    format!("nested/{nested}")
                }
            };
            let seed = paragraph_id_seed(source_hash, part, &path);
            match allocator.allocate(&seed) {
                Ok(value) => {
                    let id = format_para_id(value);
                    index
                        .ids
                        .insert(node, (id, story_ord.get(&node).copied().unwrap_or(0)));
                    index.repaired.insert(node);
                    index.repairs += 1;
                }
                Err(error) => {
                    // Allocation failure after exhausting the valid space is a
                    // hard error surfaced through the calling command.
                    index.allocation_error = Some(error);
                    return index;
                }
            }
        }
        index
    }

    /// Persist a [`ParaIdIndex`] onto this part: set `w14:paraId` on every
    /// repaired paragraph and declare the `w14` namespace (adding `w14` to
    /// `mc:Ignorable`) on the part root. Existing valid attributes are never
    /// touched, so a fully addressed document round-trips byte-for-byte.
    pub fn apply_paragraph_ids(&mut self, index: &ParaIdIndex) -> Result<(), String> {
        if let Some(error) = &index.allocation_error {
            return Err(error.clone());
        }
        if index.repairs == 0 {
            return Ok(());
        }
        for &node in &index.repaired {
            if let Some((id, _)) = index.ids.get(&node) {
                // Update in place (a repaired paragraph may already carry a
                // malformed/duplicate attribute); never stack a second one.
                match self.attr_stored_name(node, "paraId") {
                    Some(name) => {
                        self.doc.set_attribute(node, &name, id);
                    }
                    None => {
                        self.doc.set_attribute(node, "w14:paraId", id);
                    }
                }
            }
        }
        self.ensure_w14_declared();
        Ok(())
    }

    /// The stored attribute name matching `local` in either storage form
    /// (parsed "id" + prefix, or created "w:id"), if present.
    fn attr_stored_name(&self, node: NodeId, local: &str) -> Option<String> {
        let suffix = format!(":{local}");
        self.doc
            .attributes(node)
            .iter()
            .find(|a| a.name == local || a.name.ends_with(&suffix))
            .map(|a| a.name.clone())
    }

    /// Declare the `w14` namespace on the part root and add `w14` to
    /// `mc:Ignorable`, without disturbing existing namespace or compatibility
    /// declarations.
    pub fn ensure_w14_declared(&mut self) {
        self.ensure_prefix_declared("w14", W14_NS);
    }

    /// Declare `xmlns:{prefix}` on the part root (if absent) and add `prefix`
    /// to `mc:Ignorable`, without disturbing existing namespace or
    /// compatibility declarations. Every attribute this engine introduces
    /// with a qualified name must be backed by a declaration on the part
    /// root; otherwise the serialized part carries an undeclared prefix,
    /// which strict XML consumers reject as corrupt.
    ///
    /// The root element's own vocabulary is never marked ignorable. Word
    /// treats `mc:Ignorable="w15"` on a `w15:commentsEx` part as "this part
    /// has no understood content" and recovers the package as unreadable.
    pub fn ensure_prefix_declared(&mut self, prefix: &str, uri: &str) {
        let Some(root) = self.doc.root_element() else {
            return;
        };
        if !has_xmlns(&self.doc, root, prefix) {
            self.doc
                .set_attribute(root, &format!("xmlns:{prefix}"), uri);
        }
        if root_carries_prefix(&self.doc, root, prefix, uri) {
            return;
        }
        let ignorable = self
            .doc
            .attributes(root)
            .iter()
            .find(|a| a.name == "Ignorable" || a.name == "mc:Ignorable")
            .map(|a| a.name.clone());
        let mut tokens: Vec<String> = match &ignorable {
            Some(name) => self
                .doc
                .attributes(root)
                .iter()
                .find(|a| a.name == *name)
                .map(|a| a.value.split_whitespace().map(str::to_string).collect())
                .unwrap_or_default(),
            None => Vec::new(),
        };
        if tokens.iter().any(|token| token == prefix) {
            return;
        }
        tokens.push(prefix.to_string());
        let value = tokens.join(" ");
        match ignorable {
            Some(name) => {
                self.doc.set_attribute(root, &name, &value);
            }
            None => {
                if !has_xmlns(&self.doc, root, "mc") {
                    self.doc.set_attribute(root, "xmlns:mc", MC_NS);
                }
                self.doc.set_attribute(root, "mc:Ignorable", &value);
            }
        }
    }
    /// Attribute by local name, accepting parsed ("id" + prefix) and created
    /// qualified ("w:id", "w15:paraId", …) storage forms.
    pub fn attr(&self, node: NodeId, local: &str) -> Option<&str> {
        let suffix = format!(":{local}");
        self.doc
            .attributes(node)
            .iter()
            .find(|a| a.name == local || a.name.ends_with(&suffix))
            .map(|a| a.value.as_str())
    }

    /// Element check by local name only, namespace-agnostic — for OPC metadata and
    /// the w14/w15/w16cid comment parts where the wordprocessingml check is wrong.
    pub fn is_local(&self, node: NodeId, local: &str) -> bool {
        if !self.doc.is_element(node) {
            return false;
        }
        match self.doc.node_name(node) {
            Some(name) => name == local || name.ends_with(&format!(":{local}")),
            None => false,
        }
    }

    pub fn child_local(&self, node: NodeId, local: &str) -> Option<NodeId> {
        self.doc
            .children(node)
            .find(|&child| self.is_local(child, local))
    }

    pub fn child_w(&self, node: NodeId, local: &str) -> Option<NodeId> {
        self.doc
            .children(node)
            .find(|&child| self.is_w(child, local))
    }

    pub fn body(&self) -> Option<NodeId> {
        let root = self.doc.root_element()?;
        self.doc
            .children(root)
            .find(|&child| self.is_w(child, "body"))
    }

    /// Every w:p under the body, in document order, indexed 1-based.
    pub fn paragraphs(&self) -> Vec<Paragraph> {
        let Some(body) = self.body() else {
            return Vec::new();
        };
        self.paragraphs_in(body)
    }

    /// Every story-level w:p under `container` (body, hdr, or ftr), document
    /// order, 1-based.
    ///
    /// A drawing/textbox can contain another complete WordprocessingML story
    /// (`w:txbxContent/w:p`) below a run in an outer paragraph.  Those nested
    /// paragraphs are not part of the containing body/header/footer story and
    /// must not acquire locators in that story.
    pub fn paragraphs_in(&self, container: NodeId) -> Vec<Paragraph> {
        let mut nodes = Vec::new();
        self.collect_story_paragraphs(container, &mut nodes);
        nodes
            .into_iter()
            .enumerate()
            .map(|(i, node)| Paragraph { index: i + 1, node })
            .collect()
    }

    fn collect_story_paragraphs(&self, container: NodeId, out: &mut Vec<NodeId>) {
        for child in self.doc.children(container) {
            if !self.doc.is_element(child) {
                continue;
            }
            if self.is_local(child, "AlternateContent") {
                if let Some(branch) = self.alternate_content_branch(child) {
                    self.collect_story_paragraphs(branch, out);
                }
                continue;
            }
            if self.is_w(child, "p") {
                // Once a story paragraph is found, do not descend into its
                // drawing/textbox sub-stories.
                out.push(child);
                continue;
            }
            self.collect_story_paragraphs(child, out);
        }
    }

    /// Effective branch for markup-compatibility AlternateContent.
    ///
    /// docxdriver does not advertise support for the extension namespaces named by
    /// `mc:Choice/@Requires`, so its interoperable projection is `mc:Fallback`.
    /// Without a fallback there is no effective content; selecting an
    /// unsupported Choice would contradict Markup Compatibility semantics.
    pub fn alternate_content_branch(&self, node: NodeId) -> Option<NodeId> {
        if !self.is_local(node, "AlternateContent") {
            return None;
        }
        self.child_local(node, "Fallback")
    }

    pub fn paragraph_style(&self, paragraph: NodeId) -> Option<String> {
        let ppr = self.child_w(paragraph, "pPr")?;
        let style = self.child_w(ppr, "pStyle")?;
        self.attr(style, "val").map(str::to_string)
    }

    /// Content-revision classification. A move is a paired insertion/deletion:
    /// `w:moveTo` content is inserted at the destination, `w:moveFrom` content
    /// deleted at the source — Word's accept/reject outcomes coincide with
    /// ins/del, so the engine treats them as one equivalence class everywhere
    /// it projects, guards, or resolves. (moveFrom keeps `w:t` leaves, unlike
    /// w:del's delText.)
    pub fn is_rev_ins(&self, node: NodeId) -> bool {
        self.is_w(node, "ins") || self.is_w(node, "moveTo")
    }

    pub fn is_rev_del(&self, node: NodeId) -> bool {
        self.is_w(node, "del") || self.is_w(node, "moveFrom")
    }

    /// The paragraph-mark insertion marker (w:pPr/w:rPr/w:ins|w:moveTo), if present.
    pub fn paragraph_mark_ins(&self, paragraph: NodeId) -> Option<NodeId> {
        let ppr = self.child_w(paragraph, "pPr")?;
        let rpr = self.child_w(ppr, "rPr")?;
        self.child_w(rpr, "ins")
            .or_else(|| self.child_w(rpr, "moveTo"))
    }

    /// The paragraph-mark deletion marker (w:pPr/w:rPr/w:del|w:moveFrom), if
    /// present — a tracked paragraph merge: accepting joins this paragraph
    /// with the next.
    pub fn paragraph_mark_del(&self, paragraph: NodeId) -> Option<NodeId> {
        let ppr = self.child_w(paragraph, "pPr")?;
        let rpr = self.child_w(ppr, "rPr")?;
        self.child_w(rpr, "del")
            .or_else(|| self.child_w(rpr, "moveFrom"))
    }

    /// Add a paragraph-mark revision marker (w:pPr/w:rPr/w:ins|w:del), creating
    /// pPr/rPr as needed. `kind` is "ins" or "del".
    pub fn add_paragraph_mark_marker(
        &mut self,
        paragraph: NodeId,
        kind: &str,
        id: u64,
        author: &str,
        date: &str,
    ) -> NodeId {
        let ppr = match self.child_w(paragraph, "pPr") {
            Some(ppr) => ppr,
            None => {
                let ppr = self.doc.create_element("w:pPr");
                self.doc.prepend_child(paragraph, ppr);
                ppr
            }
        };
        let rpr = match self.child_w(ppr, "rPr") {
            Some(rpr) => rpr,
            None => {
                let rpr = self.doc.create_element("w:rPr");
                self.doc.append_child(ppr, rpr);
                rpr
            }
        };
        let marker = self.doc.create_element(&format!("w:{kind}"));
        self.doc.set_attribute(marker, "w:id", &id.to_string());
        self.doc.set_attribute(marker, "w:author", author);
        self.doc.set_attribute(marker, "w:date", date);
        self.doc.append_child(rpr, marker);
        marker
    }

    pub fn paragraph_text(&self, paragraph: NodeId, view: View) -> String {
        let mut out = String::new();
        self.collect_text(paragraph, view, false, false, &mut out);
        out
    }

    fn collect_text(&self, node: NodeId, view: View, in_ins: bool, in_del: bool, out: &mut String) {
        for child in self.doc.children(node) {
            if !self.doc.is_element(child) {
                continue;
            }
            if self.is_w(child, "p") {
                // A nested paragraph belongs to a separate textbox/drawing
                // story. Never fold it into the selected paragraph's text.
                continue;
            }
            if self.is_w(child, "pPr") {
                // Paragraph properties never contribute run text (the paragraph-mark
                // w:ins marker lives here; it has no text content).
                continue;
            }
            if self.is_rev_ins(child) {
                self.collect_text(child, view, true, in_del, out);
                continue;
            }
            if self.is_rev_del(child) {
                self.collect_text(child, view, in_ins, true, out);
                continue;
            }
            if self.is_w(child, "t") {
                let include = match view {
                    View::Current => !in_del,
                    View::Baseline => !in_ins,
                    View::All => true,
                };
                if include {
                    out.push_str(&self.doc.text_content(child));
                }
                continue;
            }
            if self.is_w(child, "delText") {
                let include = match view {
                    View::Current => false,
                    View::Baseline => !in_ins,
                    View::All => true,
                };
                if include {
                    out.push_str(&self.doc.text_content(child));
                }
                continue;
            }
            if self.is_local(child, "AlternateContent") {
                if let Some(branch) = self.alternate_content_branch(child) {
                    self.collect_text(branch, view, in_ins, in_del, out);
                }
                continue;
            }
            self.collect_text(child, view, in_ins, in_del, out);
        }
    }

    /// The next unused revision id in this part. Every element family that
    /// carries a revision w:id shares one numeric sequence (reference-engine
    /// parity), so the allocation must clear all of them, not just w:ins/w:del.
    /// Scans the whole part (body or hdr/ftr root) so header/footer parts work.
    pub fn next_revision_id(&self) -> u64 {
        self.max_revision_id().map(|m| m + 1).unwrap_or(1)
    }

    /// Highest revision id present in this part, if any.
    pub fn max_revision_id(&self) -> Option<u64> {
        const REVISION_ID_TAGS: &[&str] = &[
            "ins",
            "del",
            "cellIns",
            "cellDel",
            "cellMerge",
            "moveFrom",
            "moveTo",
            "moveFromRangeStart",
            "moveFromRangeEnd",
            "moveToRangeStart",
            "moveToRangeEnd",
            "rPrChange",
            "pPrChange",
            "tblPrChange",
            "trPrChange",
            "tcPrChange",
            "sectPrChange",
            "tblGridChange",
            "numberingChange",
        ];
        let root = self.body().or_else(|| self.doc.root_element())?;
        let mut max = 0u64;
        let mut found = false;
        for node in self.doc.descendants(root) {
            if REVISION_ID_TAGS.iter().any(|tag| self.is_w(node, tag)) {
                if let Some(id) = self.attr(node, "id").and_then(|v| v.parse::<u64>().ok()) {
                    max = max.max(id);
                    found = true;
                }
            }
        }
        found.then_some(max)
    }

    /// Create a run `<w:r><w:t>text</w:t></w:r>`; `rpr_from` deep-clones that run's
    /// properties onto the new run so a split preserves formatting.
    pub fn create_run(&mut self, text: &str, rpr_from: Option<NodeId>) -> NodeId {
        let run = self.doc.create_element("w:r");
        if let Some(source_rpr) = rpr_from {
            let clone = self.doc.clone_node(source_rpr, true);
            self.doc.append_child(run, clone);
        }
        let t = self.doc.create_element("w:t");
        if text.starts_with(' ') || text.ends_with(' ') {
            self.ensure_space_preserve(t);
        }
        let content = self.doc.create_text(text);
        self.doc.append_child(t, content);
        self.doc.append_child(run, t);
        run
    }

    /// Set xml:space="preserve" without duplicating the attribute across storage
    /// forms (parsed leaves carry name "space" + prefix "xml"; created ones carry
    /// the qualified "xml:space" name).
    pub fn ensure_space_preserve(&mut self, element: NodeId) {
        let existing = self
            .doc
            .attributes(element)
            .iter()
            .find(|a| a.name == "space" || a.name == "xml:space")
            .map(|a| a.name.clone());
        match existing {
            Some(name) => {
                self.doc.set_attribute(element, &name, "preserve");
            }
            None => {
                self.doc.set_attribute(element, "xml:space", "preserve");
            }
        }
    }

    /// The w:rPr child of a run, if any.
    pub fn run_rpr(&self, run: NodeId) -> Option<NodeId> {
        self.child_w(run, "rPr")
    }

    /// Set the string content of a w:t / w:delText element (replacing all children),
    /// keeping xml:space=preserve accurate for edge whitespace — added when the new
    /// text needs it, and dropped when it no longer does (an edited leaf mirrors
    /// what a fresh serialization of its content would carry).
    pub fn set_text(&mut self, text_element: NodeId, text: &str) {
        let children: Vec<NodeId> = self.doc.children(text_element).collect();
        for child in children {
            self.doc.remove_node(child);
        }
        let content = self.doc.create_text(text);
        self.doc.append_child(text_element, content);
        if text.starts_with(' ') || text.ends_with(' ') {
            self.ensure_space_preserve(text_element);
        } else {
            let existing: Vec<String> = self
                .doc
                .attributes(text_element)
                .iter()
                .filter(|a| a.name == "space" || a.name == "xml:space")
                .map(|a| a.name.clone())
                .collect();
            for name in existing {
                self.doc.remove_attribute(text_element, &name);
            }
        }
    }
}

pub fn escape_xml_text(text: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_body(body: &str) -> DocxXml {
        DocxXml::parse(
            format!(
                concat!(
                    "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
                    "xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\">",
                    "<w:body>{}</w:body></w:document>"
                ),
                body
            )
            .as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn story_paragraphs_exclude_nested_textbox_paragraphs_but_include_tables() {
        let xml = parse_body(concat!(
            "<w:p><w:r><w:t>outer</w:t><w:drawing><w:txbxContent>",
            "<w:p><w:r><w:t>textbox</w:t></w:r></w:p>",
            "</w:txbxContent></w:drawing></w:r></w:p>",
            "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>table</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
            "<mc:AlternateContent>",
            "<mc:Choice Requires=\"wps\"><w:p><w:r><w:t>choice paragraph</w:t></w:r></w:p></mc:Choice>",
            "<mc:Fallback><w:p><w:r><w:t>fallback paragraph</w:t></w:r></w:p></mc:Fallback>",
            "</mc:AlternateContent>",
            "<w:p><w:r><w:t>last</w:t></w:r></w:p>"
        ));

        let paragraphs = xml.paragraphs();
        assert_eq!(paragraphs.len(), 4);
        assert_eq!(
            paragraphs
                .iter()
                .map(|p| xml.paragraph_text(p.node, View::Current))
                .collect::<Vec<_>>(),
            ["outer", "table", "fallback paragraph", "last"]
        );
    }

    #[test]
    fn paragraph_text_selects_fallback_without_crossing_nested_story() {
        let xml = parse_body(concat!(
            "<w:p><w:r><w:t>before-</w:t>",
            "<mc:AlternateContent>",
            "<mc:Choice Requires=\"wps\"><w:r><w:t>choice</w:t></w:r></mc:Choice>",
            "<mc:Fallback><w:r><w:t>fallback</w:t><w:drawing><w:txbxContent>",
            "<w:p><w:r><w:t>16′</w:t></w:r></w:p>",
            "</w:txbxContent></w:drawing></w:r></mc:Fallback>",
            "</mc:AlternateContent><w:t>-after</w:t></w:r></w:p>"
        ));
        let paragraph = xml.paragraphs()[0].node;

        assert_eq!(
            xml.paragraph_text(paragraph, View::Current),
            "before-fallback-after"
        );
    }

    #[test]
    fn alternate_content_without_fallback_has_no_effective_text() {
        let xml = parse_body(concat!(
            "<w:p><w:r><w:t>before</w:t><mc:AlternateContent>",
            "<mc:Choice Requires=\"wps\"><w:t>unsupported</w:t></mc:Choice>",
            "</mc:AlternateContent><w:t>after</w:t></w:r></w:p>"
        ));
        let paragraph = xml.paragraphs()[0].node;

        assert_eq!(xml.paragraph_text(paragraph, View::Current), "beforeafter");
    }
}
