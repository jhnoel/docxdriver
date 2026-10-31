//! `convert_revisions_to_formatting`: tracked changes become amendment
//! formatting (37 CFR 1.121): inserted text underlined, deleted text struck
//! through, no revision left.

use std::io::Read;

use docxdriver_core::integrity::{check, introduced};
use docxdriver_core::{create, Case, Document, Source};

const NOW: &str = "2026-01-01T00:00:00Z";

fn run(bytes: &[u8], mode: &str, author: &str, ops: &str) -> Result<Vec<u8>, String> {
    let doc = Document::open(bytes);
    let json = format!(r#"{{"author":"{author}","change_mode":"{mode}","ops":{ops}}}"#);
    let plan = doc.plan(&json).map_err(|d| d.message)?;
    let review = doc.review(&plan).map_err(|d| d.message)?;
    doc.commit_at(&plan, &review.key, Some(NOW))
        .map_err(|d| d.message)
}

fn convert(bytes: &[u8], mode: &str, target: &str) -> Vec<u8> {
    let ops = format!(r#"[{{"op":"convert_revisions_to_formatting","target":{target}}}]"#);
    let out = run(bytes, mode, "Converter", &ops).unwrap();
    let new = introduced(&check(bytes), &check(&out));
    assert!(new.is_empty(), "introduced {new:?}");
    out
}

fn document_xml(bytes: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

const REVISIONS: &[&str] = &[
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
];

fn revision_count(xml: &str) -> usize {
    REVISIONS
        .iter()
        .map(|tag| {
            xml.matches(&format!("<w:{tag} ")).count() + xml.matches(&format!("<w:{tag}>")).count()
        })
        .sum()
}

/// The paragraph's runs as (text, underlined, struck), joining adjacent
/// runs with the same marks.
fn runs(xml: &str, contains: &str) -> Vec<(String, bool, bool)> {
    let paragraph = regex_lite::Regex::new(r"(?s)<w:p[ >].*?</w:p>").unwrap();
    let run = regex_lite::Regex::new(r"(?s)<w:r>(.*?)</w:r>").unwrap();
    let text =
        regex_lite::Regex::new(r"<w:(?:t|delText)(?: [^>]*)?>([^<]*)</w:(?:t|delText)>").unwrap();
    let p = paragraph
        .find_iter(xml)
        .map(|m| m.as_str())
        .find(|p| {
            text.captures_iter(p)
                .map(|c| c[1].to_string())
                .collect::<String>()
                .contains(contains)
        })
        .unwrap_or_else(|| panic!("{contains} in {xml}"));
    let mut out: Vec<(String, bool, bool)> = Vec::new();
    for r in run.captures_iter(p) {
        let body = &r[1];
        let t: String = text.captures_iter(body).map(|c| c[1].to_string()).collect();
        if t.is_empty() {
            continue;
        }
        let underlined =
            body.contains("<w:u w:val=\"single\"/>") || body.contains("<w:u w:val=\"double\"/>");
        let struck = body.contains("<w:strike/>") || body.contains("<w:dstrike/>");
        match out.last_mut() {
            Some(last) if last.1 == underlined && last.2 == struck => last.0.push_str(&t),
            _ => out.push((t, underlined, struck)),
        }
    }
    out
}

fn plain(text: &str) -> (String, bool, bool) {
    (text.into(), false, false)
}
fn under(text: &str) -> (String, bool, bool) {
    (text.into(), true, false)
}
fn struck(text: &str) -> (String, bool, bool) {
    (text.into(), false, true)
}

fn id_of(bytes: &[u8], needle: &str) -> String {
    let found = Document::open(bytes).find(needle, Case::Sensitive).unwrap();
    found[0]["at"]
        .as_str()
        .or_else(|| found[0]["paraId"].as_str())
        .or_else(|| found[0]["id"].as_str())
        .unwrap()
        .to_string()
}

fn source() -> Vec<u8> {
    create(Source::Html(
        "<p>Rent is due in thirty days.</p><p>The tenant shall pay <b>all fees</b> on time.</p><p>Keep this paragraph.</p><p>Remove this paragraph.</p><p>Closing words.</p>".into(),
    ))
    .unwrap()
}

#[test]
fn a_tracked_replacement_becomes_struck_then_underlined_text() {
    let base = source();
    let rent = id_of(&base, "Rent");
    for mode in ["track", "direct"] {
        let tracked = run(
            &base,
            "track",
            "Ann",
            &format!(r#"[{{"op":"replace_text","at":"{rent}","select":"thirty","with":"sixty"}}]"#),
        )
        .unwrap();
        let out = convert(&tracked, mode, r#""all""#);
        let xml = document_xml(&out);
        assert_eq!(revision_count(&xml), 0, "{mode}: {xml}");
        assert_eq!(
            runs(&xml, "Rent"),
            [
                plain("Rent is due in "),
                struck("thirty"),
                under("sixty"),
                plain(" days.")
            ],
            "{mode}"
        );
    }
}

#[test]
fn insertions_and_deletions_inside_a_paragraph() {
    let base = source();
    let rent = id_of(&base, "Rent");
    let ops = format!(
        r#"[{{"op":"revise_text","at":"{rent}","edits":[{{"select":"Rent","with":"Monthly rent"}},{{"select":" in thirty days","with":""}}]}}]"#
    );
    let tracked = run(&base, "track", "Ann", &ops).unwrap();
    let xml = document_xml(&convert(&tracked, "track", r#""all""#));
    assert_eq!(revision_count(&xml), 0, "{xml}");
    // The engine's word-level redline replaces the capitalised word.
    assert_eq!(
        runs(&xml, "Rent"),
        [
            struck("Rent"),
            under("Monthly rent"),
            plain(" is due"),
            struck(" in thirty days"),
            plain(".")
        ]
    );
}

#[test]
fn whole_paragraphs_inserted_and_deleted_stay_as_paragraphs() {
    let base = source();
    let keep = id_of(&base, "Keep this");
    let remove = id_of(&base, "Remove this");
    let ops = format!(
        r#"[{{"op":"insert_paragraph","at":"{keep}","position":"after","with":"A new paragraph."}},{{"op":"delete_paragraphs","at":["{remove}"]}}]"#
    );
    let tracked = run(&base, "track", "Ann", &ops).unwrap();
    let xml = document_xml(&convert(&tracked, "track", r#""all""#));
    assert_eq!(revision_count(&xml), 0, "{xml}");
    assert_eq!(runs(&xml, "A new paragraph."), [under("A new paragraph.")]);
    assert_eq!(
        runs(&xml, "Remove this"),
        [struck("Remove this paragraph.")]
    );
    // Every paragraph is still its own paragraph, in order.
    let order = [
        "Keep this",
        "A new paragraph.",
        "Remove this",
        "Closing words.",
    ]
    .map(|t| xml.find(t).unwrap());
    assert!(order.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(
        xml.matches("<w:p ").count() + xml.matches("<w:p>").count(),
        6,
        "{xml}"
    );
}

#[test]
fn existing_run_formatting_is_kept() {
    let base = source();
    let tenant = id_of(&base, "tenant");
    // "all fees" is bold; replacing "all" splits the bold run.
    let tracked = run(
        &base,
        "track",
        "Ann",
        &format!(
            r#"[{{"op":"replace_text","at":"{tenant}","select":"all fees","with":"any fees"}}]"#
        ),
    )
    .unwrap();
    let xml = document_xml(&convert(&tracked, "track", r#""all""#));
    let paragraph = &xml[xml.find("The tenant").unwrap()..];
    let paragraph = &paragraph[..paragraph.find("</w:p>").unwrap()];
    for piece in ["all", "any", " fees"] {
        let at = paragraph
            .find(&format!(">{piece}<"))
            .unwrap_or_else(|| panic!("{piece} in {paragraph}"));
        let run = &paragraph[paragraph[..at].rfind("<w:r>").unwrap()..at];
        assert!(run.contains("<w:b/>"), "{piece} lost bold: {run}");
    }
    assert_eq!(
        runs(&xml, "tenant"),
        [
            plain("The tenant shall pay "),
            struck("all"),
            under("any"),
            plain(" fees on time.")
        ]
    );
    // An underline or strike already there stays as it was.
    let underlined = create(Source::Html("<p>Pay <u>thirty</u> days.</p>".into())).unwrap();
    let at = id_of(&underlined, "Pay");
    let tracked = run(
        &underlined,
        "track",
        "Ann",
        &format!(r#"[{{"op":"delete_paragraphs","at":["{at}"]}}]"#),
    )
    .unwrap();
    let xml = document_xml(&convert(&tracked, "track", r#""all""#));
    assert_eq!(xml.matches("<w:u ").count(), 1, "{xml}");
    assert_eq!(xml.matches("<w:strike/>").count(), 3, "{xml}");
}

#[test]
fn other_authors_revisions_are_left_unless_targeted() {
    let base = source();
    let rent = id_of(&base, "Rent");
    let closing = id_of(&base, "Closing");
    let ann = run(
        &base,
        "track",
        "Ann",
        &format!(r#"[{{"op":"replace_text","at":"{rent}","select":"thirty","with":"sixty"}}]"#),
    )
    .unwrap();
    let both = run(
        &ann,
        "track",
        "Bob",
        &format!(r#"[{{"op":"replace_text","at":"{closing}","select":"Closing","with":"Final"}}]"#),
    )
    .unwrap();
    let out = convert(&both, "track", r#"{"author":"Ann"}"#);
    let xml = document_xml(&out);
    assert!(!xml.contains("w:author=\"Ann\""), "{xml}");
    assert_eq!(
        xml.matches("w:author=\"Bob\"").count(),
        document_xml(&both).matches("w:author=\"Bob\"").count(),
        "{xml}"
    );
    assert_eq!(
        runs(&xml, "Rent"),
        [
            plain("Rent is due in "),
            struck("thirty"),
            under("sixty"),
            plain(" days.")
        ]
    );
    // By id: one revision only.
    let bob_ids: Vec<String> = Document::open(&out)
        .revisions()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(bob_ids.len(), 2, "{bob_ids:?}");
    let one = convert(&out, "direct", &format!(r#"{{"id":"{}"}}"#, bob_ids[0]));
    assert_eq!(Document::open(&one).revisions().unwrap().len(), 1);
}

#[test]
fn at_limits_the_conversion_to_its_paragraphs() {
    let base = source();
    let rent = id_of(&base, "Rent");
    let closing = id_of(&base, "Closing");
    let ops = format!(
        r#"[{{"op":"replace_text","at":"{rent}","select":"thirty","with":"sixty"}},{{"op":"replace_text","at":"{closing}","select":"Closing","with":"Final"}}]"#
    );
    let tracked = run(&base, "track", "Ann", &ops).unwrap();
    let out = run(
        &tracked,
        "track",
        "Ann",
        &format!(r#"[{{"op":"convert_revisions_to_formatting","target":"all","at":["{rent}"]}}]"#),
    )
    .unwrap();
    let xml = document_xml(&out);
    assert_eq!(
        runs(&xml, "Rent"),
        [
            plain("Rent is due in "),
            struck("thirty"),
            under("sixty"),
            plain(" days.")
        ]
    );
    assert_eq!(Document::open(&out).revisions().unwrap().len(), 2, "{xml}");
}

#[test]
fn converting_is_idempotent_and_reports_what_it_did() {
    let base = source();
    let rent = id_of(&base, "Rent");
    let tracked = run(
        &base,
        "track",
        "Ann",
        &format!(r#"[{{"op":"replace_text","at":"{rent}","select":"thirty","with":"sixty"}}]"#),
    )
    .unwrap();
    // Converting in the same plan as the edit, and in a later commit.
    let same_plan = run(
        &base,
        "track",
        "Ann",
        &format!(r#"[{{"op":"replace_text","at":"{rent}","select":"thirty","with":"sixty"}},{{"op":"convert_revisions_to_formatting","target":"all"}}]"#),
    )
    .unwrap();
    let once = convert(&tracked, "track", r#""all""#);
    assert_eq!(document_xml(&same_plan), document_xml(&once));
    let twice = convert(&once, "track", r#""all""#);
    assert_eq!(document_xml(&once), document_xml(&twice));

    let doc = Document::open(&tracked);
    let plan = doc.plan(r#"{"author":"Ann","change_mode":"track","ops":[{"op":"convert_revisions_to_formatting","target":"all"}]}"#).unwrap();
    let review = doc.review(&plan).unwrap();
    let summary = &review.report.ops[0].summary;
    assert!(
        summary.contains("converted 2 revisions")
            && summary.contains("1 insertions underlined")
            && summary.contains("1 deletions struck"),
        "{summary}"
    );
}

#[test]
fn bad_targets_are_refused() {
    let base = source();
    for target in [
        r#""some""#,
        r#"{"id":"x"}"#,
        r#"{"author":""}"#,
        r#"{"id":"1","author":"a"}"#,
    ] {
        let ops = format!(r#"[{{"op":"convert_revisions_to_formatting","target":{target}}}]"#);
        assert!(run(&base, "track", "Ann", &ops).is_err(), "{target}");
    }
}

#[test]
fn rows_headers_and_formatting_changes_convert_too() {
    let base = create(Source::Html(
        "<p>Fees follow.</p><table><tr><td>Rent</td><td>100</td></tr><tr><td>Water</td><td>20</td></tr></table><p>End.</p>".into(),
    ))
    .unwrap();
    let base = run(
        &base,
        "direct",
        "setup",
        r#"[{"op":"set_header","with":"<p>Old header</p>"}]"#,
    )
    .unwrap();
    let fees = id_of(&base, "Fees");
    let ops = format!(
        r#"[{{"op":"edit_table","at":"table:1","action":{{"kind":"delete_rows","row":2,"count":1}}}},
            {{"op":"format_paragraph","at":"{fees}","alignment":"center"}},
            {{"op":"set_header","with":"<p>New header</p>"}}]"#
    );
    let tracked = run(&base, "track", "Ann", &ops).unwrap();
    let out = convert(&tracked, "track", r#""all""#);
    let xml = document_xml(&out);
    assert_eq!(revision_count(&xml), 0, "{xml}");
    assert!(Document::open(&out).revisions().unwrap().is_empty());
    // The deleted row stays, its text struck; the alignment is accepted.
    assert_eq!(runs(&xml, "Water"), [struck("Water")]);
    assert!(xml.contains(r#"<w:jc w:val="center"/>"#), "{xml}");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&out)).unwrap();
    let mut header = String::new();
    archive
        .by_name("word/header1.xml")
        .unwrap()
        .read_to_string(&mut header)
        .unwrap();
    assert_eq!(revision_count(&header), 0, "{header}");
    assert!(
        header.contains("Old header")
            && header.contains("New header")
            && header.contains("<w:strike/>")
            && header.contains("<w:u w:val=\"single\"/>"),
        "{header}"
    );
}
