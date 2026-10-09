# DOCX Block Insert + Inline Formatting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the typed plan surface so a document can be *created* paragraph-by-paragraph through the same validated plan path used for edits: add `insert_table`, `insert_list`, and `insert_image` ops, inline `color`/`font`/`font_size` on the `with` dialect (with read round-trip), and expose the already-implemented `comment_*` ops through the docxdriver-pi Python REPL.

**Architecture:** The Rust core (`crates/docxdriver-core`) already owns a validated plan pipeline: `EditOp` enum (schema-generated) → `validate_plan` → `run_typed_edit` → typed `*_typed` functions that mutate a `WorkState` package. New ops are new `EditOp` variants plus typed functions in `commands/typed.rs`; block content (tables/lists) reuses the HTML builder in `html/build.rs`; inline color/font/size extend the shared `Fmt` struct used by parse/render/insert. The docxdriver-pi surface is purely declarative: a capability manifest + a generated Python prelude.

**Tech Stack:** Rust (docxdriver-core, schemars), OOXML, TypeScript (docxdriver-pi), Monty Python sandbox, Node scripts.

## Global Constraints

- New ops follow the existing anchor model: `at: ParagraphAddress` (id or `$alias`) + `position: before|after`, mirroring `insert_paragraph`.
- `with` values are parsed by `parse_with` in `commands/typed.rs` and reduce to `Vec<InsPiece>`; the safe inline dialect must stay a strict subset (no tables/fields/notes/revision tags/newlines).
- Revisions: **tracked everywhere** (decision B). `tracked = matches!(mode, ChangeMode::Track)` is already the dispatch rule; block ops must author `<w:ins>` when tracked.
- Determinism: allocated paragraph ids, `numId`s, and revision ids must derive from `plan_seed` (sha256 of canonical plan) + `op_index`, never from a counter (see `allocate_insert_para_id_typed`).
- Schema: `EditOp` is the source of truth. After every `EditOp` change, regenerate with `cargo run -p docxdriver-core --bin generate_typed_schema` and copy `packages/docxdriver/schema/typed-request.schema.json` → `packages/docxdriver-pi/schema/typed-request.schema.json`.
- Colors are uppercase `#RRGGBB`; measurements are points (`Points(f32)`); `font_size` in the op schema is `Points`.
- `Fmt` gains `Clone` (it is currently `Copy`); every `*fmt`/`*piece_fmt` deref must become `.clone()`/`== &fmt`.

---

## File Structure

**Modify (Rust core):**
- `crates/docxdriver-core/src/html/mod.rs` — `Fmt` struct: add `color`/`font`/`size`, drop `Copy`, keep `PartialEq, Eq, Default`.
- `crates/docxdriver-core/src/html/parse.rs` — parse `<span style="…">` into `Fmt.color/font/size`; keep the existing b/i/u/s/sup/sub arms untouched.
- `crates/docxdriver-core/src/html/render.rs` — `run_fmt` reads `w:color`/`w:rFonts`/`w:sz`; `fmt_stack`/`transition_fmt`/`open_fmt_tag` emit `<span style="…">`; `run_fmt_pending` untouched.
- `crates/docxdriver-core/src/segments.rs` — `ensure_run` writes `w:color`/`w:rFonts`/`w:sz` into `w:rPr`.
- `crates/docxdriver-core/src/commands/typed.rs` — unblock `<image>` in `parse_with`; add `InsertTableArgs`/`InsertListArgs`/`InsertImageArgs` + `insert_table_typed`/`insert_list_typed`/`insert_image_typed`; add `font` to `FormatTextArgs`.
- `crates/docxdriver-core/src/commands/mod.rs` — dispatch arms in `run_typed_edit`.
- `crates/docxdriver-core/src/api.rs` — `EditOp` variants, `validate_plan` arms, `operation_name` arms.
- `crates/docxdriver-core/src/html/build.rs` — list parse (`ul`/`ol`/`li`) + `table_xml` tracked-row mode.
- `crates/docxdriver-core/src/html/chrome.rs` (or new `media.rs` helper) — `ensure_image_media(package, src) -> rid`.
- `crates/docxdriver-core/tests/typed_api.rs` — extend the "every edit op" tests.

**Modify (docxdriver-pi):**
- `packages/docxdriver-pi/scripts/plan-capability-manifest.mjs` — add `insert_table`, `insert_list`, `insert_image`, `comment_add`, `comment_reply`, `comment_set_status`, `comment_delete` + docs.
- Regenerate `packages/docxdriver-pi/src/python-plan-prelude.ts` regions via `generate-python-dataclasses.mjs`.
- `packages/docxdriver-pi/skills/docx-pi/SKILL.md` — document `with` inline dialect + new ops.

---

## Task 1: Extend `Fmt` with color/font/size (Clone, no Copy)

**Files:**
- Modify: `crates/docxdriver-core/src/html/mod.rs:53-60`
- Modify (ripple): `crates/docxdriver-core/src/html/parse.rs`, `render.rs`, `segments.rs` — only the `*fmt`/`*piece_fmt` deref sites compile-fix here; behavior lands in Task 2.

**Interfaces:**
- Consumes: nothing.
- Produces: `Fmt { bold, italic, underline, strike, vert_align, color: Option<String>, font: Option<String>, size: Option<f32> }` with `#[derive(Debug, Clone, PartialEq)]` (no `Copy`, no `Eq` — `f32` is not `Eq`).

- [ ] **Step 1: Write the failing test**

Add to `crates/docxdriver-core/tests/typed_api.rs` (a compile-level assertion: `Fmt` is `Clone + PartialEq`, fields present):

```rust
#[test]
fn fmt_carries_color_font_size_and_is_clone() {
    let f = docxdriver_core::html::Fmt {
        bold: true,
        color: Some("#FF0000".to_string()),
        font: Some("Calibri".to_string()),
        size: Some(12.0),
        ..Default::default()
    };
    let g = f.clone();
    assert_eq!(f, g);
    assert_eq!(f.size, Some(12.0));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p docxdriver-core --test typed_api fmt_carries_color_font_size_and_is_clone`
Expected: FAIL — `Fmt` has no `color`/`font`/`size` fields, and `Fmt` cannot be cloned into a binding typed as non-`Copy` in this shape (compile error on missing fields).

- [ ] **Step 3: Change `Fmt`**

In `crates/docxdriver-core/src/html/mod.rs`, replace the `Fmt` definition:

```rust
/// Bold/italic/underline/strike/super/sub + color/font/size formatting state.
/// Canonical tag nesting order is b → i → u → s → (sup|sub) → span(color/font/size).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Fmt {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub vert_align: Option<VertAlign>,
    /// `w:color` val, uppercase `#RRGGBB`.
    pub color: Option<String>,
    /// `w:rFonts w:ascii` (and `w:hAnsi`/`w:cs`) family name.
    pub font: Option<String>,
    /// `w:sz` half-points, stored as points (12.0 → w:sz val "24").
    pub size: Option<f32>,
}
```

- [ ] **Step 4: Fix compile ripple**

`cargo build -p docxdriver-core` will surface every deref site. Fix each mechanically:
- `segments.rs` `insert_ins_pieces_at`: `Some(InsPiece::Text(text, piece_fmt)) if *piece_fmt == fmt` → `if piece_fmt == &fmt`; `if run.is_none() || *run_fmt != fmt` → `*run_fmt != fmt` still works (`Fmt: PartialEq`); `*run_fmt = fmt` → `*run_fmt = fmt.clone()` (only if the surrounding borrows require it — if it compiles as `*run_fmt = fmt` with `run_fmt: &mut Fmt`, leave it).
- `parse.rs` `if fmt != Fmt::default()` — unchanged (works with `PartialEq`).
- `render.rs` `self.fmt == fmt`, `self.fmt = fmt` — unchanged; `Self::fmt_stack(fmt)` must become `Self::fmt_stack(&fmt)` (change the signature in Task 2, or now: `fn fmt_stack(fmt: &Fmt)`).

- [ ] **Step 5: Run tests**

Run: `cargo test -p docxdriver-core`
Expected: PASS (existing suite green; the new `Fmt` fields are defaulted so no behavioral change).

- [ ] **Step 6: Commit**

```bash
git add crates/docxdriver-core/src/html/mod.rs crates/docxdriver-core/src/html/parse.rs crates/docxdriver-core/src/html/render.rs crates/docxdriver-core/src/segments.rs crates/docxdriver-core/tests/typed_api.rs
git commit -m "feat(docxdriver-core): extend Fmt with color/font/size (Clone)"
```

---

## Task 2: `<span style>` in the `with` dialect + render round-trip

**Files:**
- Modify: `crates/docxdriver-core/src/html/parse.rs` (span arm in `parse_fragment`)
- Modify: `crates/docxdriver-core/src/html/render.rs` (`run_fmt`, `fmt_stack`, `transition_fmt`, `open_fmt_tag`)
- Modify: `crates/docxdriver-core/src/segments.rs` (`ensure_run` rPr writes)

**Interfaces:**
- Consumes: `Fmt { color, font, size, .. }` from Task 1.
- Produces: canonical span token `<span style="color:#RRGGBB;font-family:NAME;font-size:Npt">` with fixed attribute order; `parse_fragment` accepts exactly that (and a permissive subset: any order, quoted values, `font-size` as `Npt` or `N`).

- [ ] **Step 1: Write the failing tests**

In `crates/docxdriver-core/tests/typed_api.rs`:

```rust
#[test]
fn with_span_color_font_size_round_trips_through_insert_and_read() {
    let bytes = document(&[("A", "start"), ("B", "end")]);
    let ops = vec![EditOp::InsertParagraph {
        at: ParagraphAddress::Id("00000001".into()), // adjust to a real paraId from `document`
        position: InsertPosition::After,
        with: "plain <span style=\"color:#FF0000;font-family:Calibri;font-size:12pt\">styled</span> tail".into(),
        style: None,
        alias: None,
    }];
    let committed = /* run plan preview+commit, then read markup */;
    // Assert read markup contains the canonical span token:
    assert!(markup.contains("<span style=\"color:#FF0000;font-family:Calibri;font-size:12pt\">styled</span>"));
}
```

> Note: use the existing `plan(bytes, ops)` / `document(..)` helpers already in `typed_api.rs`; copy the `plan_preview_commit_is_keyed_and_binds_insert_aliases` test's preview→commit→read plumbing. Use a real paragraph id from the helper's output (call `docx_read` on the built doc first, or read `document`'s body — the helper assigns deterministic ids; inspect with a temporary `println!`).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p docxdriver-core --test typed_api with_span_color_font_size_round_trips_through_insert_and_read`
Expected: FAIL — `parse_fragment` rejects `<span>` (unknown tag), so `parse_with` returns the "does not parse as safe inline content" error.

- [ ] **Step 3: Parse `<span style>`**

In `parse.rs`, add an arm in the tag match (next to the `sup`/`sub` arms) for `("span", false)` / `("span", true)`. Add a helper:

```rust
/// Parse the small CSS subset used by the dialect span token.
fn parse_span_style(style: &str) -> Result<(Option<String>, Option<String>, Option<f32>), String> {
    let mut color = None; let mut font = None; let mut size = None;
    for decl in style.split(';') {
        let decl = decl.trim();
        if decl.is_empty() { continue; }
        let (key, value) = decl.split_once(':')
            .ok_or_else(|| format!("bad span style declaration {decl:?}"))?;
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        match key.trim() {
            "color" => {
                let v = value.to_uppercase();
                if !v.starts_with('#') || v.len() != 7 {
                    return Err(format!("color must be #RRGGBB, got {value:?}"));
                }
                color = Some(v);
            }
            "font-family" => font = Some(value.to_string()),
            "font-size" => {
                let n = value.trim_end_matches("pt").parse::<f32>()
                    .map_err(|_| format!("font-size must be a number of points, got {value:?}"))?;
                size = Some(n);
            }
            other => return Err(format!("unknown span style property {other:?}")),
        }
    }
    Ok((color, font, size))
}
```

Open arm sets `fmt.color/font/size` (error if already set — span does not nest); close arm clears them.

- [ ] **Step 4: Render the span token**

In `render.rs`, change `fmt_stack` to append a span entry and emit it in `open_fmt_tag`:

```rust
fn fmt_stack(fmt: &Fmt) -> Vec<FmtToken> { /* b,i,u,s,(sup|sub), then Span if any of color/font/size */ }

fn span_style(fmt: &Fmt) -> String {
    let mut parts = Vec::new();
    if let Some(c) = &fmt.color { parts.push(format!("color:{}", c)); }
    if let Some(f) = &fmt.font { parts.push(format!("font-family:{}", f)); }
    if let Some(s) = fmt.size { parts.push(format!("font-size:{}pt", s)); }
    parts.join(";")
}
```

In `open_fmt_tag`, when the token is `Span`, emit `<span style="{escaped}">`; in `close_fmt_to` emit `</span>`. Keep `span` the **innermost** layer (after sup/sub) so `transition_fmt`'s prefix-match diff logic stays correct. Update `FmtToken` to be an enum `Named(&'static str) | Span` and adjust `keep`/`zip` comparisons accordingly (compare by token equality).

- [ ] **Step 5: Read + write rPr color/font/size**

`render.rs` `run_fmt`: add three `else if` arms reading `w:color` (`fmt.color = xml.attr(child, "val").map(str::to_uppercase)`), `w:rFonts` (`w:ascii` attr → `fmt.font`), `w:sz` (`w:val` half-points → `fmt.size = Some(halfpoints / 2.0)`).

`segments.rs` `ensure_run`: after the `vert_align` block, write:

```rust
if let Some(color) = &fmt.color {
    let c = xml.doc.create_element("w:color");
    xml.doc.set_attribute(c, "w:val", color);
    xml.doc.append_child(rpr, c);
}
if let Some(font) = &fmt.font {
    let rfonts = xml.doc.create_element("w:rFonts");
    xml.doc.set_attribute(rfonts, "w:ascii", font);
    xml.doc.set_attribute(rfonts, "w:hAnsi", font);
    xml.doc.set_attribute(rfonts, "w:cs", font);
    xml.doc.append_child(rpr, rfonts);
}
if let Some(size) = fmt.size {
    let sz = xml.doc.create_element("w:sz");
    xml.doc.set_attribute(sz, "w:val", &format!("{}", (size * 2.0).round() as i64));
    xml.doc.append_child(rpr, sz);
}
```

(`fmt` here is the `crate::html::Fmt` value already in scope; convert `*fmt` uses to `fmt` as needed.)

- [ ] **Step 6: Run tests**

Run: `cargo test -p docxdriver-core --test typed_api with_span_color_font_size_round_trips_through_insert_and_read`
Expected: PASS. Then `cargo test -p docxdriver-core` — full suite green.

- [ ] **Step 7: Commit**

```bash
git add crates/docxdriver-core/src/html/parse.rs crates/docxdriver-core/src/html/render.rs crates/docxdriver-core/src/segments.rs crates/docxdriver-core/tests/typed_api.rs
git commit -m "feat(docxdriver-core): span style color/font/size in with dialect + read round-trip"
```

---

## Task 3: `format_text` gains `font` (family name)

**Files:**
- Modify: `crates/docxdriver-core/src/api.rs` (`EditOp::FormatText` — add `font: Option<String>`)
- Modify: `crates/docxdriver-core/src/commands/typed.rs` (`FormatTextArgs` + `format_text_typed`)
- Modify: `crates/docxdriver-core/src/commands/mod.rs` (dispatch arm)
- Regenerate schema (see Step 4).

**Interfaces:**
- Consumes: `Fmt` rPr write helper from Task 2 (format_text writes `w:rFonts` via the same `set` path — reuse the existing per-property `set_child_val` style used for color/font_size).
- Produces: `FormatText { .., font: Option<String> }`; `FormatTextArgs { .., font: Option<String> }`.

- [ ] **Step 1: Write the failing test**

In `typed_api.rs`, add a `format_text` case to `every_edit_op_executes_through_an_individual_typed_command` (the existing enumerating test) — add an op with `font: Some("Courier New".into())` and assert the committed document's `w:rFonts w:ascii` equals `Courier New` (reuse the `document_xml(bytes)` helper).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p docxdriver-core --test typed_api every_edit_op_executes_through_an_individual_typed_command`
Expected: FAIL — `EditOp::FormatText` has no `font` field (compile error in the test literal).

- [ ] **Step 3: Add the field**

- `api.rs` `EditOp::FormatText`: add `#[serde(default)] font: Option<String>,`.
- `commands/typed.rs` `FormatTextArgs`: add `pub font: Option<String>,`; in `format_text_typed`, write `w:rFonts` (`w:ascii`/`w:hAnsi`/`w:cs`) when `font` is `Some`, and add `"font"` to the `clear`-able property set if the existing `clear` logic enumerates properties (check `validate_plan`'s `format_text clear contains an unknown property` branch and add `"font"`).
- `commands/mod.rs`: pass `font: font.clone()` into `FormatTextArgs`.

- [ ] **Step 4: Regenerate + sync schema**

Run:

```bash
cargo run -p docxdriver-core --bin generate_typed_schema
cp packages/docxdriver/schema/typed-request.schema.json packages/docxdriver-pi/schema/typed-request.schema.json
```

Verify `format_text` in the JSON schema now lists `font`.

- [ ] **Step 5: Run tests**

Run: `cargo test -p docxdriver-core`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/docxdriver-core/src/api.rs crates/docxdriver-core/src/commands/typed.rs crates/docxdriver-core/src/commands/mod.rs packages/docxdriver/schema/typed-request.schema.json packages/docxdriver-pi/schema/typed-request.schema.json crates/docxdriver-core/tests/typed_api.rs
git commit -m "feat(docxdriver-core): format_text font (family name) field"
```

---

## Task 4: Unblock inline `<image>` in `with` + media allocation in the insert path

**Files:**
- Modify: `crates/docxdriver-core/src/commands/typed.rs` (`parse_with` — remove the `Atom::Image` rejection)
- Modify: `crates/docxdriver-core/src/html/chrome.rs` or new `crates/docxdriver-core/src/media.rs` — add `ensure_image_media(package, src) -> Result<String, Outcome>` (rid allocator)
- Modify: `crates/docxdriver-core/src/segments.rs` (`insert_ins_pieces_at` already handles `InsPiece::Image`; no change unless rid binding needs a hook)

**Interfaces:**
- Consumes: `crate::media::parse_data_url(src)` (exists at `media.rs:287`); `Package::set`.
- Produces: `parse_with` accepts `<image src="data:…"/>` and yields `InsPiece::Image { rid }` (rid allocated via `ensure_image_media`); `insert_plain_pieces` (untracked) must also allocate media — extend it the same way.

- [ ] **Step 1: Write the failing test**

In `typed_api.rs`: `InsertParagraph` with `with: "<image src=\"data:image/png;base64,iVBORw0KGgo=\"/>"`, commit, assert the document now contains `word/media/image1.png` (via `document_xml` + package listing) and `w:drawing` in the inserted run.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p docxdriver-core --test typed_api <new_test_name>`
Expected: FAIL — `parse_with` returns "with contains an image — images are not in the safe inline set".

- [ ] **Step 3: Remove the rejection + bind rid**

In `parse_with` (typed.rs), replace the `Atom::Image { .. } => return Err(…)` arm with logic that parses `src` via `crate::media::parse_data_url` and emits `InsPiece::Image { rid }`. Because `parse_with` returns `(Vec<InsPiece>, Option<String>)` and currently has no `&mut Package`, change its signature to `parse_with(with: &str, package: &mut Package, author: &str, now: &str)` so it can call `ensure_image_media`. Update the three call sites (`replace_text_typed`, `replace_paragraph_typed`, `insert_paragraph_typed`) to pass `package`.

- [ ] **Step 4: Implement `ensure_image_media`**

Mirror `build_package`'s media wiring in `readonly.rs` (next_rid/overrides/content_type_defaults): allocate the next free relationship id in `word/_rels/document.xml.rels`, append a `<Relationship Type="…/image" Target="media/imageN.ext">`, add `media/imageN.ext` via `package.set`, and add the `<Default Extension="png" ContentType="image/png"/>` to `[Content_Types].xml`. Return the rid string.

- [ ] **Step 5: Run tests**

Run: `cargo test -p docxdriver-core`
Expected: PASS (new test green; existing `parse_with` rejection tests updated).

- [ ] **Step 6: Commit**

```bash
git add crates/docxdriver-core/src/commands/typed.rs crates/docxdriver-core/src/html/chrome.rs crates/docxdriver-core/tests/typed_api.rs
git commit -m "feat(docxdriver-core): allow inline <image> in with dialect"
```

---

## Task 5: `insert_image` op (block-level, own paragraph)

**Files:**
- Modify: `crates/docxdriver-core/src/api.rs` (`EditOp::InsertImage` variant, `validate_plan`, `operation_name`)
- Modify: `crates/docxdriver-core/src/commands/typed.rs` (`InsertImageArgs` + `insert_image_typed`)
- Modify: `crates/docxdriver-core/src/commands/mod.rs` (dispatch arm)
- Modify: `crates/docxdriver-core/tests/typed_api.rs`
- Regenerate + sync schema.

**Interfaces:**
- Consumes: `ensure_image_media` (Task 4); `insert_paragraph_typed`'s anchor/paraId allocation (reuse `allocate_insert_para_id_typed`, `resolve_paragraph_by_para_id`, `insert_before`).
- Produces: `InsertImage { at: ParagraphAddress, position: InsertPosition, src: String, width: Option<Points>, height: Option<Points>, alt: Option<String>, #[serde(rename="as")] alias: Option<String> }`; `InsertImageArgs { at, insert_before, src, width, height, alt, tracked, author, plan_seed, op_index }`.

- [ ] **Step 1: Write the failing test**

`typed_api.rs`: `InsertImage { at, position: After, src: data-url, width: Some(Points(100.0)), height: Some(Points(100.0)), alt: None, alias: None }` through a plan; assert the committed doc has a new paragraph whose run contains `w:drawing` and `wp:extent` with `cx`/`cy` = 914400 EMU (100pt).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p docxdriver-core --test typed_api <new_test_name>`
Expected: FAIL — `EditOp::InsertImage` does not exist.

- [ ] **Step 3: Add the variant + validation + name**

`api.rs`: add the `InsertImage { .. }` variant (fields exactly as in Interfaces). In `validate_plan`, validate `src` starts with `data:` (error "insert_image src must be a data: URL"); in `operation_name` return `"insert_image"`.

- [ ] **Step 4: Implement `insert_image_typed`**

Create a new `<w:p>`, insert before/after anchor, allocate paraId (reuse `allocate_insert_para_id_typed(xml, source_hash, &args.plan_seed, args.op_index)`), then write a single run: if `tracked`, wrap run in `<w:ins>` (reuse `insert_ins_pieces_at` with `pieces = vec![InsPiece::Image { rid }]`); else `insert_plain_pieces` with the same piece. Extent cx/cy come from `width`/`height` (points → EMU: `(pt * 12700.0).round()`); default 96pt square when absent. `alt` writes `wp:docPr descr`.

- [ ] **Step 5: Dispatch + schema + manifest + prelude + docs**

- `commands/mod.rs` `run_typed_edit`: add the `EditOp::InsertImage` arm building `InsertImageArgs` (same pattern as `InsertParagraph`).
- Regenerate + sync schema (Task 3 Step 4 commands).
- `plan-capability-manifest.mjs`: add `'insert_image'` to `ops` + a `docs.insert_image` prose entry.
- Run `node packages/docxdriver-pi/scripts/generate-python-dataclasses.mjs` and confirm the `InsertImage` dataclass appears in `src/python-plan-prelude.ts`.
- Add `insert_image` to `crates/docxdriver-core/tests/typed_api.rs` enumerating tests.

- [ ] **Step 6: Run tests**

Run: `cargo test -p docxdriver-core && (cd packages/docxdriver-pi && npm test)`
Expected: PASS (drift tests confirm prelude matches schema).

- [ ] **Step 7: Commit**

```bash
git add crates/docxdriver-core/src/api.rs crates/docxdriver-core/src/commands/typed.rs crates/docxdriver-core/src/commands/mod.rs packages/docxdriver/schema/typed-request.schema.json packages/docxdriver-pi/schema/typed-request.schema.json packages/docxdriver-pi/src/python-plan-prelude.ts packages/docxdriver-pi/scripts/plan-capability-manifest.mjs crates/docxdriver-core/tests/typed_api.rs
git commit -m "feat: insert_image plan op (block-level)"
```

---

## Task 6: `insert_table` op (tracked rows)

**Files:**
- Modify: `crates/docxdriver-core/src/api.rs` (`EditOp::InsertTable`, validate, name)
- Modify: `crates/docxdriver-core/src/commands/typed.rs` (`InsertTableArgs` + `insert_table_typed`)
- Modify: `crates/docxdriver-core/src/html/build.rs` (`table_xml` → accept a `tracked` flag emitting row-level `<w:ins>`)
- Modify: `crates/docxdriver-core/src/html/render.rs` (`table_rows` see through `w:ins`/`w:del`; emit `<ins>`/`<del>` around rows in markup)
- Modify: `crates/docxdriver-core/src/commands/mod.rs`
- Modify: tests; regenerate schema; manifest/prelude/docs.

**Interfaces:**
- Consumes: the create table builder's `<table>/<tr>/<td>` parsing (already in `build.rs`); `resolve_paragraph_by_para_id`, `allocate_insert_para_id_typed`.
- Produces: `InsertTable { at, position, html: String, #[serde(rename="as")] alias: Option<String> }`; `InsertTableArgs { at, insert_before, html, tracked, author, plan_seed, op_index }`.

- [ ] **Step 1: Write the failing test**

`typed_api.rs`: `InsertTable { at, position: After, html: "<table><tr><td>A</td><td>B</td></tr></table>", alias: None }` (tracked mode); commit; assert `document_xml` contains `<w:tbl>` with each `<w:tr>` wrapped in `<w:ins>`, and that read markup emits the table with `<ins>` markers.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p docxdriver-core --test typed_api <new_test_name>`
Expected: FAIL — no `InsertTable` op.

- [ ] **Step 3: Table XML with tracked rows**

In `build.rs`, change `table_xml(rows)` to `table_xml(rows, tracked: bool, rev_id: u64, author: &str, date: &str)`; when `tracked`, wrap each `<w:tr>` in `<w:ins w:id="{rev_id}" w:author="{author}" w:date="{date}">…</w:ins>`. Reuse the existing `<td>` paragraph builder (cells currently get bare `<w:p>`; give each cell paragraph a fresh `paraId` via `allocate_insert_para_id_typed` so they are addressable).

- [ ] **Step 4: Render table revisions**

`render.rs` `table_rows`: iterate children of `w:tbl`, and when a child is `w:ins`/`w:del`, descend into it and record the wrapper so `render_table` emits `<ins id author>`/`<del id author>` around that row's cells in the markup projection. (The markup `<table>` is a protected opaque region; the revision markers wrap row content only.)

- [ ] **Step 5: `insert_table_typed` + dispatch + validate + surface**

Mirror `insert_paragraph_typed`: build `<w:tbl>` from `html` via a new small `build_table_block(html) -> Result<String, String>` wrapper around the existing `<table>` parse path (extract it from `body_and_chrome_from_html` or call the table branch directly), insert the `<w:tbl>` before/after anchor, and pass `tracked` through. Wire `validate_plan` (reject nested tables is already in the builder) + `operation_name` + dispatch + schema/manifest/prelude/docs exactly as Task 5 Steps 3–5.

- [ ] **Step 6: Run tests**

Run: `cargo test -p docxdriver-core && (cd packages/docxdriver-pi && npm test)`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/docxdriver-core/src/api.rs crates/docxdriver-core/src/commands/typed.rs crates/docxdriver-core/src/commands/mod.rs crates/docxdriver-core/src/html/build.rs crates/docxdriver-core/src/html/render.rs packages/docxdriver/schema/typed-request.schema.json packages/docxdriver-pi/schema/typed-request.schema.json packages/docxdriver-pi/src/python-plan-prelude.ts packages/docxdriver-pi/scripts/plan-capability-manifest.mjs crates/docxdriver-core/tests/typed_api.rs
git commit -m "feat: insert_table plan op with tracked rows"
```

---

## Task 7: `insert_list` op (numbering part + ul/ol parse)

**Files:**
- Modify: `crates/docxdriver-core/src/api.rs`, `commands/typed.rs`, `commands/mod.rs`
- Modify: `crates/docxdriver-core/src/html/build.rs` (parse `ul`/`ol`/`li`, build list paragraphs with `numPr`)
- New helper in `build.rs` (or `package.rs`): `ensure_numbering_part(package, seed) -> numId` (creates `word/numbering.xml` with bullet + decimal `abstractNum`s)
- Modify: `crates/docxdriver-core/src/html/render.rs` (reconstruct list markers from `numPr` — verify/extend the existing `num` annotation path)
- Tests; schema; manifest/prelude/docs.

**Interfaces:**
- Consumes: `allocate_insert_para_id_typed`; `Package::set`.
- Produces: `InsertList { at, position, html: String, #[serde(rename="as")] alias: Option<String> }`; `InsertListArgs { at, insert_before, html, tracked, author, plan_seed, op_index }`.

- [ ] **Step 1: Write the failing test**

`typed_api.rs`: `InsertList { at, position: After, html: "<ul><li>one</li><li>two</li></ul>", alias: None }`; commit; assert `document_xml` contains `w:numPr` on the two inserted paragraphs and that `word/numbering.xml` exists in the package.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p docxdriver-core --test typed_api <new_test_name>`
Expected: FAIL — no `InsertList`; also `<ul>` is unparsable in the builder.

- [ ] **Step 3: Numbering part**

Add `ensure_numbering_part(package: &mut Package, seed: &[u8; 32]) -> Result<u32, Outcome>`: if `word/numbering.xml` exists, parse and append a `num`; else emit a minimal numbering.xml with two `abstractNum`s (bullet `decimal`-less `<w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/>…`, and `decimal` `%1.`) and two `num` instances referencing them; allocate `numId` from `seed` deterministically.

- [ ] **Step 4: List parse + build**

In `build.rs`, add `ul`/`ol`/`li` arms (reject `li` outside a list; allow the inline `with` dialect inside `li` text). Build each item as a `<w:p>` with `<w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="{numId}"/></w:numPr></w:pPr>` plus the parsed runs.

- [ ] **Step 5: `insert_list_typed` + dispatch + render + surface**

Mirror `insert_paragraph_typed` for each item (tracked = runs in `<w:ins>` + paragraph-mark marker; paraId per item). Render: reconstruct the computed list marker into the `num` annotation of each item's `<p>` (the `ParaAttrs.num` field already exists for this). Wire validate/name/dispatch/schema/manifest/prelude/docs as in Task 5.

- [ ] **Step 6: Run tests**

Run: `cargo test -p docxdriver-core && (cd packages/docxdriver-pi && npm test)`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/docxdriver-core/src/api.rs crates/docxdriver-core/src/commands/typed.rs crates/docxdriver-core/src/commands/mod.rs crates/docxdriver-core/src/html/build.rs crates/docxdriver-core/src/html/render.rs packages/docxdriver/schema/typed-request.schema.json packages/docxdriver-pi/schema/typed-request.schema.json packages/docxdriver-pi/src/python-plan-prelude.ts packages/docxdriver-pi/scripts/plan-capability-manifest.mjs crates/docxdriver-core/tests/typed_api.rs
git commit -m "feat: insert_list plan op with numbering part"
```

---

## Task 8: Expose `comment_*` ops + document the `with` dialect

**Files:**
- Modify: `packages/docxdriver-pi/scripts/plan-capability-manifest.mjs`
- Regenerate: `packages/docxdriver-pi/src/python-plan-prelude.ts`
- Modify: `packages/docxdriver-pi/skills/docx-pi/SKILL.md`

**Interfaces:**
- Consumes: `comment_add`/`comment_reply`/`comment_set_status`/`comment_delete` already in `EditOp` + `run_typed_edit` (verify the dispatch arms exist in `commands/mod.rs`; they are already implemented in core).
- Produces: these four ops callable from the Python REPL as `CommentAdd`/`CommentReply`/`CommentSetStatus`/`CommentDelete` dataclasses.

- [ ] **Step 1: Add ops + docs to the manifest**

In `plan-capability-manifest.mjs`, extend `ops` with `'comment_add', 'comment_reply', 'comment_set_status', 'comment_delete'` and add concise `docs` prose for each (anchor model: `at` + optional `select`/`occurrence`; `comment_id` for the other three).

- [ ] **Step 2: Regenerate the prelude**

Run: `node packages/docxdriver-pi/scripts/generate-python-dataclasses.mjs`
Expected: the four `Comment*` dataclasses and the operation reference now include the comment ops.

- [ ] **Step 3: Verify end-to-end + drift tests**

Run: `(cd packages/docxdriver-pi && npm test)`
Expected: PASS — drift tests confirm the prelude matches the schema, and `docx_help` lists the comment ops.

- [ ] **Step 4: Document the `with` inline dialect + new ops in the skill**

In `packages/docxdriver-pi/skills/docx-pi/SKILL.md`, add a section documenting:
- the `with` inline dialect: `<b> <i> <u> <s> <sup> <sub> <span style="color:#RRGGBB;font-family:NAME;font-size:Npt"> <a href="…"> <br/> <equation>…</equation> <image src="data:…"/>`
- the new block ops `insert_table` / `insert_list` / `insert_image` with their `html`/`src` fields and anchor semantics
- the comment ops and their anchor/status model.

- [ ] **Step 5: Commit**

```bash
git add packages/docxdriver-pi/scripts/plan-capability-manifest.mjs packages/docxdriver-pi/src/python-plan-prelude.ts packages/docxdriver-pi/skills/docx-pi/SKILL.md
git commit -m "feat(docxdriver-pi): expose comment ops + document with dialect and block ops"
```

---

## Self-Review Notes (run before handoff)

1. **Spec coverage:** color/font/size inline (Tasks 1–3), `insert_image` (Tasks 4–5), `insert_table` (Task 6), `insert_list` (Task 7), commenting exposure + docs (Task 8). ✓
2. **Type consistency:** `InsertImageArgs`/`InsertTableArgs`/`InsertListArgs` all carry `plan_seed: [u8; 32]` and `op_index: usize` exactly like `InsertParagraphArgs`; `paragraph` anchor helpers keep the `source_hash: Option<[u8;32]>` signature. ✓
3. **Determinism:** every new allocation (paraId, numId, rid) derives from `plan_seed`/`op_index` — no process-global counters. ✓
4. **Ripple check:** `Fmt` Copy→Clone touches `parse.rs`, `render.rs`, `segments.rs` only; those are all addressed in Tasks 1–2. ✓

## Execution Handoff

After saving, choose one execution path:
1. **Subagent-Driven (recommended)** — superpowers:subagent-driven-development
2. **Inline Execution** — superpowers:executing-plans
