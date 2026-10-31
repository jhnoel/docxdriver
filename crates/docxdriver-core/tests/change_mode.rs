//! Change-mode correctness: a direct edit leaves no revision, a tracked edit
//! leaves nothing unmarked, and settling a tracked edit gives what the direct
//! edit (accept-all) or the input (reject-all) is.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

use docxdriver_core::integrity::{check, introduced};
use docxdriver_core::{create, Document, Source, View};

const AUTHOR: &str = "Ann";

fn run(bytes: &[u8], mode: &str, author: &str, ops: &str) -> Result<Vec<u8>, String> {
    let doc = Document::open(bytes);
    let json = format!(r#"{{"author":"{author}","change_mode":"{mode}","ops":{ops}}}"#);
    let plan = doc.plan(&json).map_err(|d| d.message)?;
    let review = doc.review(&plan).map_err(|d| d.message)?;
    doc.commit(&plan, &review.key).map_err(|d| d.message)
}

fn settle(bytes: &[u8], accept: bool) -> Vec<u8> {
    let action = if accept { "accept" } else { "reject" };
    let ops = format!(r#"[{{"op":"revision_settle","target":"all","action":"{action}"}}]"#);
    match run(bytes, "direct", "settler", &ops) {
        Ok(out) => out,
        // Nothing to settle: the plan is a no-op.
        Err(message) if message.contains("no ") || message.contains("0 revisions") => {
            bytes.to_vec()
        }
        Err(message) => panic!("settle {action}: {message}"),
    }
}

fn parts(bytes: &[u8]) -> BTreeMap<String, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let name = file.name().to_string();
        if !name.ends_with(".xml") && !name.ends_with(".rels") {
            continue;
        }
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        out.insert(name, text);
    }
    out
}

fn part(bytes: &[u8], name: &str) -> String {
    parts(bytes).remove(name).unwrap_or_default()
}

const REVISION_TAGS: &[&str] = &[
    "ins",
    "del",
    "moveFrom",
    "moveTo",
    "rPrChange",
    "pPrChange",
    "sectPrChange",
    "tblPrChange",
    "trPrChange",
    "tcPrChange",
    "tblGridChange",
    "tblPrExChange",
    "numberingChange",
    "cellIns",
    "cellDel",
    "cellMerge",
];

/// Every revision element in the package's WordprocessingML parts, as
/// `part: tag`.
fn revisions(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, xml) in parts(bytes) {
        if !name.starts_with("word/") || name.contains("_rels") {
            continue;
        }
        for tag in REVISION_TAGS {
            for suffix in [" ", ">", "/"] {
                let needle = format!("<w:{tag}{suffix}");
                for _ in xml.matches(&needle) {
                    out.push(format!("{name}: {tag}"));
                }
            }
        }
    }
    out.sort();
    out
}

/// Revision elements written by `author`.
fn authored(bytes: &[u8], author: &str) -> usize {
    let mut count = 0;
    for xml in parts(bytes).values() {
        for tag in REVISION_TAGS {
            let needle = format!("<w:{tag} ");
            for (at, _) in xml.match_indices(&needle) {
                let head = &xml[at..at + xml[at..].find('>').unwrap()];
                count += usize::from(head.contains(&format!("w:author=\"{author}\"")));
            }
        }
    }
    count
}

/// Every `w14:paraId` value in the package.
fn para_ids(xml: &str) -> Vec<String> {
    xml.match_indices("w14:paraId=\"")
        .map(|(at, tag)| xml[at + tag.len()..at + tag.len() + 8].to_string())
        .collect()
}

/// The package's parts with ids a plan allocates (absent from `input`)
/// numbered by first appearance, so a direct and a settled tracked output
/// compare equal when they hold the same content. Ids that `input` has stay.
fn canonical(bytes: &[u8], input: &[u8]) -> BTreeMap<String, String> {
    let known: BTreeSet<String> = parts(input)
        .values()
        .flat_map(|xml| para_ids(xml))
        .collect();
    let mut fresh: BTreeMap<String, String> = BTreeMap::new();
    let mut out = BTreeMap::new();
    for (name, xml) in parts(bytes) {
        // The declaration's line break is serializer style, not content.
        let mut text = xml.replacen("?>\n<", "?><", 1).trim_end().to_string();
        for id in para_ids(&xml) {
            if known.contains(&id) {
                continue;
            }
            let n = fresh.len();
            let label = fresh
                .entry(id.clone())
                .or_insert_with(|| format!("NEW{n:05}"))
                .clone();
            text = text.replace(&format!("\"{id}\""), &format!("\"{label}\""));
        }
        out.insert(name, merge_runs(&strip_empty_properties(&text)));
    }
    out
}

/// Drops empty property containers: settling a revision can leave an
/// empty `w:rPr`/`w:pPr` where the direct edit writes none.
fn strip_empty_properties(xml: &str) -> String {
    let mut text = xml.to_string();
    loop {
        let next = [
            "<w:pPr><w:rPr/></w:pPr>",
            "<w:rPr/>",
            "<w:pPr/>",
            "<w:tcPr/>",
            "<w:trPr/>",
            "<w:rPr></w:rPr>",
            "<w:pPr></w:pPr>",
        ]
        .iter()
        .fold(text.clone(), |acc, empty| acc.replace(empty, ""));
        if next == text {
            return text;
        }
        text = next;
    }
}

/// Joins adjacent plain-text runs with the same properties, the way a
/// settled tracked edit and a direct edit differ only in run boundaries.
fn merge_runs(xml: &str) -> String {
    let run = regex_lite::Regex::new(
        r#"<w:r>(<w:rPr>(?:[^<]|<[^/]|</[^w]|</w:[^r]|</w:r[^P])*</w:rPr>)?<w:t(?: xml:space="preserve")?>([^<]*)</w:t></w:r>"#,
    )
    .unwrap();
    let mut out = String::new();
    let mut pending: Option<(String, String)> = None;
    let mut last = 0;
    let flush = |out: &mut String, pending: &mut Option<(String, String)>| {
        if let Some((rpr, text)) = pending.take() {
            out.push_str(&format!("<w:r>{rpr}<w:t>{text}</w:t></w:r>"));
        }
    };
    for caps in run.captures_iter(xml) {
        let whole = caps.get(0).unwrap();
        let rpr = caps.get(1).map_or("", |m| m.as_str()).to_string();
        let text = caps[2].to_string();
        if whole.start() != last {
            flush(&mut out, &mut pending);
            out.push_str(&xml[last..whole.start()]);
        }
        match &mut pending {
            Some((held, body)) if *held == rpr => body.push_str(&text),
            _ => {
                flush(&mut out, &mut pending);
                pending = Some((rpr, text));
            }
        }
        last = whole.end();
    }
    flush(&mut out, &mut pending);
    out.push_str(&xml[last..]);
    out
}

fn assert_same(label: &str, a: &BTreeMap<String, String>, b: &BTreeMap<String, String>) {
    let names: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    for name in names {
        let (x, y) = (a.get(name), b.get(name));
        if x != y {
            let (x, y) = (
                x.cloned().unwrap_or_default(),
                y.cloned().unwrap_or_default(),
            );
            let at = x.chars().zip(y.chars()).take_while(|(p, q)| p == q).count();
            let from = at.saturating_sub(200);
            panic!(
                "{label}: {name} differs at {at}\n left: {}\n right: {}",
                x.chars().skip(from).take(500).collect::<String>(),
                y.chars().skip(from).take(500).collect::<String>()
            );
        }
    }
}

fn assert_no_new_faults(label: &str, input: &[u8], output: &[u8]) {
    let new = introduced(&check(input), &check(output));
    assert!(new.is_empty(), "{label}: introduced {new:?}");
}

/// Paragraph (`<p>`, `<hN>`) and list item tags of a view: id, style,
/// number or marker.
fn view_tags(bytes: &[u8], view: View) -> Vec<String> {
    let markup = Document::open(bytes).markup(view).unwrap();
    let mut out = Vec::new();
    for (at, _) in markup.match_indices('<') {
        let rest = &markup[at + 1..];
        let tag: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if !(tag == "p" || tag == "li" || (tag.len() == 2 && tag.starts_with('h'))) {
            continue;
        }
        let end = rest.find('>').unwrap();
        let head = &rest[..end];
        let attr = |name: &str| {
            head.find(&format!(" {name}=\"")).map(|i| {
                let v = &head[i + name.len() + 3..];
                v[..v.find('"').unwrap()].to_string()
            })
        };
        out.push(format!(
            "{tag} {} {} {}",
            attr("id").unwrap_or_default(),
            attr("data-docx-style").unwrap_or_default(),
            attr("data-docx-number")
                .or_else(|| attr("data-docx-marker"))
                .unwrap_or_default()
        ));
    }
    out
}

fn first_id(bytes: &[u8], needle: &str) -> String {
    let found = Document::open(bytes)
        .find(needle, docxdriver_core::Case::Sensitive)
        .unwrap();
    found
        .first()
        .and_then(|m| {
            m.get("at")
                .or_else(|| m.get("paraId"))
                .or_else(|| m.get("id"))
        })
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no paragraph with {needle:?}: {found:?}"))
        .to_string()
}

const BODY: &str = concat!(
    "<h1>Agreement</h1>",
    "<p>The Supplier shall deliver the Services within thirty days.</p>",
    "<ol><li>First claim of the parties.</li><li>Second claim of the parties.</li><li>Third claim of the parties.</li></ol>",
    "<h2>Abstract</h2>",
    "<p>Either party may terminate this agreement with notice.</p>",
    "<table><tr><td>Fee schedule</td><td>Amount due</td></tr><tr><td>Initial fee</td><td>5000 dollars</td></tr></table>",
    "<p>The area is <math display=\"inline\"><msup><mi>r</mi><mn>2</mn></msup></math> per region.</p>",
    "<p>Closing paragraph of the agreement.</p>",
);

/// The fixture: headings, a numbered list, a table, a header and a footer.
fn fixture() -> Vec<u8> {
    let bytes = create(Source::Html(BODY.into())).unwrap();
    run(
        &bytes,
        "direct",
        "setup",
        r#"[{"op":"set_header","with":"<p>Running header</p><p>Second header line</p>"},{"op":"set_footer","with":"<p>Running footer</p>"}]"#,
    )
    .unwrap()
}

/// The fixture with another author's pending insertion and deletion.
fn foreign_fixture() -> Vec<u8> {
    let bytes = fixture();
    let supplier = first_id(&bytes, "Supplier");
    let either = first_id(&bytes, "Either party");
    let ops = format!(
        r#"[{{"op":"replace_text","at":"{supplier}","select":"thirty","with":"sixty"}},
            {{"op":"replace_text","at":"{either}","select":"notice","with":"written notice"}}]"#
    );
    run(&bytes, "track", "Bob", &ops).unwrap()
}

/// One op per row: (label, ops JSON built against `bytes`).
fn op_matrix(bytes: &[u8]) -> Vec<(&'static str, String)> {
    let supplier = first_id(bytes, "Supplier");
    let claim1 = first_id(bytes, "First claim");
    let claim3 = first_id(bytes, "Third claim");
    let either = first_id(bytes, "Either party");
    let closing = first_id(bytes, "Closing paragraph");
    let area = first_id(bytes, "The area is");
    vec![
        ("replace_text", format!(r#"[{{"op":"replace_text","at":"{supplier}","select":"Services","with":"Goods"}}]"#)),
        ("revise_text", format!(r#"[{{"op":"revise_text","at":"{supplier}","edits":[{{"select":"deliver","with":"ship"}}]}}]"#)),
        ("replace_paragraph", format!(r#"[{{"op":"replace_paragraph","at":"{either}","with":"Neither party may <b>terminate</b>."}}]"#)),
        ("format_text", format!(r##"[{{"op":"format_text","at":"{supplier}","select":"Supplier","bold":true,"color":"#AA0000"}}]"##)),
        ("format_paragraph", format!(r#"[{{"op":"format_paragraph","at":"{supplier}","alignment":"center"}}]"#)),
        ("set_list", format!(r#"[{{"op":"set_list","at":["{supplier}"],"format":"bullet"}}]"#)),
        ("set_list_remove", format!(r#"[{{"op":"set_list","at":["{claim1}"],"remove":true}}]"#)),
        ("insert_paragraph", format!(r#"[{{"op":"insert_paragraph","at":"{supplier}","position":"after","with":"Inserted <b>text</b>"}}]"#)),
        ("insert_paragraph_last", format!(r#"[{{"op":"insert_paragraph","at":"{closing}","position":"after","with":"After the end"}}]"#)),
        ("delete_paragraphs", format!(r#"[{{"op":"delete_paragraphs","at":["{supplier}"]}}]"#)),
        ("delete_last_claim", format!(r#"[{{"op":"delete_paragraphs","at":["{claim3}"]}}]"#)),
        ("delete_last_paragraph", format!(r#"[{{"op":"delete_paragraphs","at":["{closing}"]}}]"#)),
        ("insert_table", format!(r#"[{{"op":"insert_table","at":"{supplier}","position":"after","rows":2,"columns":2,"cells":[["a","b"],["c","d"]]}}]"#)),
        ("edit_table_insert_rows", r#"[{"op":"edit_table","at":"table:1","action":{"kind":"insert_rows","row":1,"position":"after","count":1}}]"#.into()),
        ("edit_table_delete_rows", r#"[{"op":"edit_table","at":"table:1","action":{"kind":"delete_rows","row":2,"count":1}}]"#.into()),
        ("edit_table_delete", r#"[{"op":"edit_table","at":"table:1","action":{"kind":"delete"}}]"#.into()),
        ("format_table", r#"[{"op":"format_table","at":"table:1","format":{"alignment":"center"}}]"#.into()),
        ("format_row", r#"[{"op":"format_row","at":"table:1/row:1","format":{"repeating_header":true}}]"#.into()),
        ("format_cell", r##"[{"op":"format_cell","at":"table:1/row:1/cell:1","format":{"fill":"#ABCDEF"}}]"##.into()),
        ("edit_table_insert_rows_before", r#"[{"op":"edit_table","at":"table:1","action":{"kind":"insert_rows","row":1,"position":"before","count":2}}]"#.into()),
        ("edit_table_insert_columns", r#"[{"op":"edit_table","at":"table:1","action":{"kind":"insert_columns","column":1,"position":"after","count":1}}]"#.into()),
        ("edit_table_delete_columns", r#"[{"op":"edit_table","at":"table:1","action":{"kind":"delete_columns","column":1,"count":1}}]"#.into()),
        ("edit_table_merge_cells", r#"[{"op":"edit_table","at":"table:1","action":{"kind":"merge_cells","row":1,"column":1,"row_span":1,"column_span":2}}]"#.into()),
        ("insert_table_in_plan", format!(r#"[{{"op":"insert_table","at":"{closing}","position":"before","rows":1,"columns":2,"as":"t"}},{{"op":"replace_paragraph","at":"$t_r1c1","with":"cell"}},{{"op":"edit_table","at":"$t","action":{{"kind":"insert_rows","row":1,"position":"after","count":1}}}}]"#)),
        ("replace_equation", format!(r#"[{{"op":"replace_equation","at":"{area}","equation":1,"mathml":"<math display=\"inline\"><mfrac><mi>a</mi><mi>b</mi></mfrac></math>"}}]"#)),
        ("delete_equation", format!(r#"[{{"op":"delete_equation","at":"{area}","equation":1}}]"#)),
        ("set_header", r#"[{"op":"set_header","with":"<p>New header</p>"}]"#.into()),
        ("set_header_lines", r#"[{"op":"set_header","with":"<p>One</p><p style=\"text-align:center\">Two</p><p>Three</p>"}]"#.into()),
        ("set_header_first", r#"[{"op":"set_header","kind":"first","with":"<p>Cover header</p>"}]"#.into()),
        ("set_header_even", r#"[{"op":"set_header","kind":"even","with":"<p>Even header</p>"}]"#.into()),
        ("set_footer_page", r#"[{"op":"set_footer","with":"<p>Page <page-number/> of <page-count/></p>"}]"#.into()),
        ("set_footer", r#"[{"op":"set_footer","with":"<p>New footer</p>"}]"#.into()),
        ("clear_header", r#"[{"op":"clear_header"}]"#.into()),
        ("clear_footer", r#"[{"op":"clear_footer"}]"#.into()),
        ("set_page_margins", r#"[{"op":"set_page_margins","top":72.0}]"#.into()),
        ("set_section_layout", r#"[{"op":"set_section_layout","orientation":"landscape"}]"#.into()),
        ("set_first_page_mode", r#"[{"op":"set_first_page_mode","enabled":true}]"#.into()),
        ("set_even_and_odd_headers", r#"[{"op":"set_even_and_odd_headers","even_and_odd":true}]"#.into()),
        ("set_section_boundary", format!(r#"[{{"op":"set_section_boundary","at":"{supplier}"}}]"#)),
        ("comment_add", format!(r#"[{{"op":"comment_add","at":"{supplier}","select":"Supplier","text":"Check"}}]"#)),
        ("bookmark_add", format!(r#"[{{"op":"bookmark_add","at":"{supplier}","name":"Mark"}}]"#)),
        ("revision_settle", r#"[{"op":"revision_settle","target":"all","action":"accept"}]"#.into()),
    ]
}

fn both_fixtures() -> [(&'static str, Vec<u8>); 2] {
    [("clean", fixture()), ("foreign", foreign_fixture())]
}

/// Other authors' revision elements, by tag.
fn foreign_revisions(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, xml) in parts(bytes) {
        for tag in REVISION_TAGS {
            for (at, _) in xml.match_indices(&format!("<w:{tag} ")) {
                let head = &xml[at..at + xml[at..].find('>').unwrap()];
                if head.contains("w:author=\"Bob\"") {
                    out.push(format!("{name}: {tag}"));
                }
            }
        }
    }
    out.sort();
    out
}

// Finding 1: format_text, format_paragraph, set_list and delete_paragraphs
// wrote rPrChange / pPrChange / w:del in direct mode.
#[test]
fn direct_edits_leave_no_revision() {
    for (label, base) in both_fixtures() {
        for (op, ops) in op_matrix(&base) {
            let Ok(out) = run(&base, "direct", AUTHOR, &ops) else {
                continue;
            };
            assert_eq!(
                authored(&out, AUTHOR),
                0,
                "{label} {op}: {:?}",
                revisions(&out)
            );
            if op == "revision_settle" {
                continue; // settles everyone's revisions by definition
            }
            // Other authors' revisions are neither settled nor re-authored
            // (a direct deletion may take one away with its content).
            let before = foreign_revisions(&base);
            let after = foreign_revisions(&out);
            assert!(
                after.iter().all(|r| before.contains(r)),
                "{label} {op}: {after:?} vs {before:?}"
            );
            if !op.starts_with("delete") && op != "replace_paragraph" {
                assert_eq!(after, before, "{label} {op}");
            }
            assert_no_new_faults(&format!("{label} {op}"), &base, &out);
        }
    }
}

// Finding 4: deleting a paragraph handed its id and properties to the
// paragraph that merged into its place.
#[test]
fn deleting_a_paragraph_keeps_the_next_paragraph_intact() {
    let base = fixture();
    let claim2 = first_id(&base, "Second claim");
    let claim3 = first_id(&base, "Third claim");
    let abstract_heading = first_id(&base, "Abstract");
    let ops = format!(r#"[{{"op":"delete_paragraphs","at":["{claim3}"]}}]"#);
    let direct = run(&base, "direct", AUTHOR, &ops).unwrap();
    let tracked = run(&base, "track", AUTHOR, &ops).unwrap();
    for (label, bytes, view) in [
        ("direct", direct.clone(), View::Markup),
        ("accepted", settle(&tracked, true), View::Markup),
        ("final view", tracked.clone(), View::Final),
    ] {
        let tags = view_tags(&bytes, view);
        assert!(
            tags.contains(&format!("h2 {abstract_heading} Heading2 ")),
            "{label}: {tags:?}"
        );
        assert!(
            !tags.iter().any(|t| t.contains(&claim3)),
            "{label}: {tags:?}"
        );
        assert!(tags.contains(&format!("p {claim2}  ")), "{label}: {tags:?}");
        let markup = Document::open(&bytes).markup(view).unwrap();
        assert!(!markup.contains("Third claim"), "{label}: {markup}");
        assert!(
            markup.contains("<h2 id=\"") && !markup.contains("<li data-docx-marker=\"3.\""),
            "{label}: {markup}"
        );
    }
    let xml = part(&direct, "word/document.xml");
    let heading = &xml[xml
        .find(&format!("w14:paraId=\"{abstract_heading}\""))
        .unwrap()..];
    assert!(
        heading[..heading.find("</w:p>").unwrap()].contains("<w:pStyle w:val=\"Heading2\"/>"),
        "{xml}"
    );
    assert!(
        !heading[..heading.find("</w:p>").unwrap()].contains("numPr"),
        "{xml}"
    );

    // Deleting the paragraph before a list item keeps the item's id and numbering.
    let supplier = first_id(&base, "Supplier");
    let claim1 = first_id(&base, "First claim");
    let ops = format!(r#"[{{"op":"delete_paragraphs","at":["{supplier}"]}}]"#);
    let tracked = run(&base, "track", AUTHOR, &ops).unwrap();
    for (label, bytes, view) in [
        (
            "direct",
            run(&base, "direct", AUTHOR, &ops).unwrap(),
            View::Markup,
        ),
        ("accepted", settle(&tracked, true), View::Markup),
        ("final view", tracked.clone(), View::Final),
    ] {
        let tags = view_tags(&bytes, view);
        let at = tags
            .iter()
            .position(|t| t.contains(&claim1))
            .unwrap_or_else(|| panic!("{label}: {tags:?}"));
        assert_eq!(tags[at - 1], "li   1.", "{label}: {tags:?}");
        assert!(
            !tags.iter().any(|t| t.contains(&supplier)),
            "{label}: {tags:?}"
        );
    }
}

// Finding 5: the final and original views showed a joined paragraph with
// the wrong id, style and list label.
#[test]
fn views_are_the_settled_documents() {
    let base = fixture();
    let claim1 = first_id(&base, "First claim");
    let plans = [
        format!(
            r#"[{{"op":"insert_paragraph","at":"{claim1}","position":"after","with":"Inserted claim"}}]"#
        ),
        format!(
            r#"[{{"op":"insert_paragraph","at":"{claim1}","position":"after","with":"Inserted claim","as":"n"}},{{"op":"set_list","at":["$n"],"num_id":1}}]"#
        ),
        format!(r#"[{{"op":"set_list","at":["{claim1}"],"remove":true}}]"#),
        format!(r#"[{{"op":"format_paragraph","at":"{claim1}","style":"Heading1"}}]"#),
    ];
    for (label, base) in both_fixtures() {
        let plans = plans
            .iter()
            .cloned()
            .chain(op_matrix(&base).into_iter().map(|(_, ops)| ops));
        for ops in plans {
            let Ok(tracked) = run(&base, "track", AUTHOR, &ops) else {
                continue;
            };
            assert_eq!(
                view_tags(&tracked, View::Final),
                view_tags(&settle(&tracked, true), View::Markup),
                "{label} final {ops}"
            );
            assert_eq!(
                view_tags(&tracked, View::Original),
                view_tags(&settle(&tracked, false), View::Markup),
                "{label} original {ops}"
            );
        }
    }
    // A list paragraph replaced by a tracked insertion and a tracked
    // deletion keeps its list label in the original view (the bench's
    // sloppy oracle: claim 3 lost "3.").
    let claim3 = first_id(&base, "Third claim");
    for position in ["before", "after"] {
        let ops = format!(
            r#"[{{"op":"insert_paragraph","at":"{claim3}","position":"{position}","with":"Third claim, revised."}},{{"op":"delete_paragraphs","at":["{claim3}"]}}]"#
        );
        let tracked = run(&base, "track", AUTHOR, &ops).unwrap();
        let original = view_tags(&tracked, View::Original);
        assert_eq!(original, view_tags(&base, View::Markup), "{position}");
        assert_eq!(
            original,
            view_tags(&settle(&tracked, false), View::Markup),
            "{position}"
        );
        assert_eq!(
            view_tags(&tracked, View::Final),
            view_tags(&settle(&tracked, true), View::Markup),
            "{position}"
        );
    }
    // The original view numbers the claims 1-3, without the rejected item.
    let ops = &plans[1];
    let tracked = run(&base, "track", AUTHOR, ops).unwrap();
    let markers: Vec<String> = view_tags(&tracked, View::Original)
        .into_iter()
        .filter(|t| t.starts_with("li"))
        .collect();
    assert_eq!(markers, ["li   1.", "li   2.", "li   3."]);
}

// Finding 6: rejecting an insertion after the last body paragraph left an
// empty paragraph.
#[test]
fn rejecting_an_insertion_after_the_last_paragraph_restores_the_input() {
    let base = fixture();
    let closing = first_id(&base, "Closing paragraph");
    let ops = format!(
        r#"[{{"op":"insert_paragraph","at":"{closing}","position":"after","with":["After the end","And more"]}}]"#
    );
    let tracked = run(&base, "track", AUTHOR, &ops).unwrap();
    let rejected = settle(&tracked, false);
    assert_same(
        "reject",
        &canonical(&rejected, &base),
        &canonical(&base, &base),
    );
    assert_eq!(
        view_tags(&tracked, View::Original),
        view_tags(&base, View::Markup)
    );
    // Rejecting the insertion one revision at a time ends the same way.
    let mut bytes = tracked.clone();
    while let Some(id) = Document::open(&bytes)
        .revisions()
        .unwrap()
        .first()
        .and_then(|r| r["id"].as_str().map(str::to_owned))
    {
        let ops = format!(r#"[{{"op":"revision_settle","target":"{id}","action":"reject"}}]"#);
        bytes = run(&bytes, "direct", "settler", &ops).unwrap();
    }
    assert_same(
        "reject one by one",
        &canonical(&bytes, &base),
        &canonical(&base, &base),
    );
    // Before a table, too.
    let either = first_id(&base, "Either party");
    let ops = format!(
        r#"[{{"op":"insert_paragraph","at":"{either}","position":"after","with":"Before the table"}}]"#
    );
    let tracked = run(&base, "track", AUTHOR, &ops).unwrap();
    assert_same(
        "reject before table",
        &canonical(&settle(&tracked, false), &base),
        &canonical(&base, &base),
    );
    assert_no_new_faults("insert after last", &base, &tracked);
}

/// What a tracked edit of each op must do.
#[derive(Debug, PartialEq)]
enum Track {
    /// Revisions only: accept-all gives the direct output, reject-all the input.
    Exact,
    /// As `Exact`, but reject-all keeps definitions or empty stories the op
    /// added (list definitions, a new header part); its views still equal
    /// the input's.
    KeepsAdditions,
    /// Accept-all leaves an empty story where the direct edit removes its
    /// reference (same rendering in the first section); reject-all gives the
    /// input.
    EmptiesStory,
    /// As `Exact`, but a simple field is written as the complex field
    /// runs a tracked change holds; the stories' text is the same.
    ComplexField,
    /// Refused with an error naming the op.
    Refused,
    /// Applied directly in both modes: Word records no revision for it.
    Direct,
}

fn expectation(op: &str) -> Track {
    match op {
        "set_list" | "set_header_first" => Track::KeepsAdditions,
        "set_footer_page" => Track::ComplexField,
        "clear_header" | "clear_footer" => Track::EmptiesStory,
        "set_even_and_odd_headers"
        | "set_header_even"
        | "set_section_boundary"
        | "edit_table_insert_columns"
        | "edit_table_delete_columns"
        | "edit_table_merge_cells" => Track::Refused,
        "comment_add" | "bookmark_add" | "revision_settle" => Track::Direct,
        _ => Track::Exact,
    }
}

/// Visible text of every part's paragraphs (body, headers, footers).
fn story_text(bytes: &[u8]) -> BTreeMap<String, String> {
    let tag = regex_lite::Regex::new(r#"<w:t(?: [^>]*)?>([^<]*)</w:t>|<w:instrText[^>]*>([^<]*)</w:instrText>|<w:fldSimple w:instr="([^"]*)"|</w:p>"#).unwrap();
    parts(bytes)
        .into_iter()
        .filter(|(name, _)| name.starts_with("word/header") || name.starts_with("word/footer"))
        .map(|(name, xml)| {
            let text: String = tag
                .captures_iter(&xml)
                .map(|c| match (c.get(1), c.get(2).or(c.get(3))) {
                    (Some(text), _) => text.as_str().to_string(),
                    (None, Some(field)) => format!("{{{}}}", field.as_str().trim()),
                    (None, None) => "\n".to_string(),
                })
                .collect();
            (name, text.trim_end_matches('\n').to_string())
        })
        .collect()
}

// Findings 2 and 3: every op in track mode produces Word's tracked form or
// is refused; settling it gives the direct edit or the input; other
// authors' revisions are left alone.
#[test]
fn tracked_edits_settle_to_the_direct_edit_and_the_input() {
    let mut failures = Vec::new();
    for (label, base) in both_fixtures() {
        let input_rejected = settle(&base, false);
        for (op, ops) in op_matrix(&base) {
            let outcome =
                std::panic::catch_unwind(|| check_tracked(label, &base, &input_rejected, op, &ops));
            if let Err(panic) = outcome {
                let message = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()));
                failures.push(format!(
                    "{label} {op}: {}",
                    message
                        .unwrap_or_default()
                        .chars()
                        .take(1500)
                        .collect::<String>()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

fn check_tracked(label: &str, base: &[u8], input_rejected: &[u8], op: &str, ops: &str) {
    let (base, input_rejected) = (base.to_vec(), input_rejected.to_vec());
    {
        {
            let expect = expectation(op);
            let tracked = run(&base, "track", AUTHOR, ops);
            let context = format!("{label} {op}");
            if expect == Track::Refused {
                let message = tracked.expect_err(&context);
                let name = ops
                    .split("\"op\":\"")
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap();
                assert!(message.contains(name), "{context}: {message}");
                assert!(message.contains("direct"), "{context}: {message}");
                return;
            }
            let tracked = match tracked {
                Ok(bytes) => bytes,
                // Refusing an edit over another author's revision is correct.
                Err(message) if label == "foreign" && message.contains("tracked change") => return,
                Err(message) => panic!("{context}: {message}"),
            };
            let direct = run(&base, "direct", AUTHOR, ops)
                .unwrap_or_else(|e| panic!("{context} direct: {e}"));
            assert_no_new_faults(&context, &base, &tracked);
            assert_no_new_faults(&context, &base, &direct);
            assert_no_new_faults(
                &format!("{context} accepted"),
                &base,
                &settle(&tracked, true),
            );
            assert_no_new_faults(
                &format!("{context} rejected"),
                &base,
                &settle(&tracked, false),
            );
            if expect == Track::Direct {
                assert_same(
                    &context,
                    &canonical(&tracked, &base),
                    &canonical(&direct, &base),
                );
                return;
            }
            assert!(
                authored(&tracked, AUTHOR) > 0,
                "{context}: no revision written"
            );
            assert_eq!(
                foreign_revisions(&tracked),
                foreign_revisions(&base),
                "{context}: other authors' revisions changed"
            );
            let accepted = canonical(&settle(&tracked, true), &base);
            let direct_settled = canonical(&settle(&direct, true), &base);
            let rejected = settle(&tracked, false);
            match expect {
                Track::Exact | Track::KeepsAdditions => {
                    assert_same(&format!("{context} accept"), &accepted, &direct_settled)
                }
                Track::ComplexField => {
                    let body = |m: &BTreeMap<String, String>| m.get("word/document.xml").cloned();
                    assert_eq!(body(&accepted), body(&direct_settled), "{context}");
                    assert_eq!(
                        story_text(&settle(&tracked, true)),
                        story_text(&direct),
                        "{context}"
                    );
                }
                _ => {
                    let mut without_story = accepted.clone();
                    without_story.retain(|name, _| {
                        !name.starts_with("word/header") && !name.starts_with("word/footer")
                    });
                    let doc = |m: &BTreeMap<String, String>| m.get("word/document.xml").cloned();
                    assert_ne!(doc(&without_story), None);
                    let texts = story_text(&settle(&tracked, true));
                    assert!(texts.values().any(|t| t.is_empty()), "{context}: {texts:?}");
                }
            }
            match expect {
                Track::KeepsAdditions => {
                    let mut left = canonical(&rejected, &base);
                    let mut right = canonical(&input_rejected, &base);
                    for map in [&mut left, &mut right] {
                        map.retain(|name, _| name == "word/document.xml");
                    }
                    let strip_section =
                        regex_lite::Regex::new(r"<w:(header|footer)Reference [^>]*/>").unwrap();
                    for map in [&mut left, &mut right] {
                        for xml in map.values_mut() {
                            *xml = strip_section.replace_all(xml, "").into_owned();
                        }
                    }
                    assert_same(&format!("{context} reject"), &left, &right);
                    let texts = story_text(&rejected);
                    let before = story_text(&input_rejected);
                    for (name, text) in &texts {
                        assert_eq!(
                            before.get(name).map(String::as_str).unwrap_or(""),
                            text,
                            "{context}: {name}"
                        );
                    }
                }
                _ => assert_same(
                    &format!("{context} reject"),
                    &canonical(&rejected, &base),
                    &canonical(&input_rejected, &base),
                ),
            }
            assert_eq!(
                view_tags(&tracked, View::Final),
                view_tags(&settle(&tracked, true), View::Markup),
                "{context} final view"
            );
            assert_eq!(
                view_tags(&tracked, View::Original),
                view_tags(&settle(&tracked, false), View::Markup),
                "{context} original view"
            );
        }
    }
}
