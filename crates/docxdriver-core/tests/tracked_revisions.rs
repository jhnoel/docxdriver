//! Tracked edits over the author's own pending revisions and paragraph-mark
//! settlement, with Word semantics: deleting your own insertion removes it
//! outright, and the paragraph mark that survives keeps its properties.

use std::io::Read;

use docxdriver_core::{create, Document, Source, View};

const ANN: &str = "Ann";
const BOB: &str = "Bob";
const FOX: &str = "<p>The quick <b>brown</b> fox jumps.</p>";

// Ids of every `<p>` in a markup projection, in order.
fn paragraph_ids(bytes: &[u8]) -> Vec<String> {
    let markup = Document::open(bytes).markup(View::Markup).unwrap();
    markup
        .match_indices(" id=\"")
        .map(|(at, tag)| markup[at + tag.len()..at + tag.len() + 8].to_string())
        .collect()
}

// Previews then commits `ops` (a JSON array) by `author` in track mode.
fn track(bytes: &[u8], author: &str, ops: &str) -> Result<Vec<u8>, String> {
    let doc = Document::open(bytes);
    let json = format!(r#"{{"author":"{author}","change_mode":"track","ops":{ops}}}"#);
    let plan = doc.plan(&json).map_err(|d| d.message)?;
    let review = doc.review(&plan).map_err(|d| d.message)?;
    doc.commit(&plan, &review.key).map_err(|d| d.message)
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

// Visible body text of a view: one line per non-empty paragraph.
fn text(bytes: &[u8], view: View) -> String {
    let markup = Document::open(bytes).markup(view).unwrap();
    let mut out = String::new();
    let mut in_tag = false;
    for ch in markup.replace("</p>", "</p>\n").chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let lines: Vec<&str> = out.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines.join("\n")
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// `w:ins` never nests inside another `w:ins`: every opening tag is closed
/// before the next one opens.
fn assert_no_nested_ins(xml: &str) {
    let opens: Vec<usize> = xml.match_indices("<w:ins ").map(|(at, _)| at).collect();
    let closes: Vec<usize> = xml.match_indices("</w:ins>").map(|(at, _)| at).collect();
    for pair in opens.windows(2) {
        let closed = closes.iter().any(|&c| c > pair[0] && c < pair[1]);
        assert!(closed, "nested w:ins in {xml}");
    }
}

// "The quick<ins Ann> very</ins> brown fox jumps."
fn quick_very() -> Vec<u8> {
    let bytes = create(Source::Html(FOX.into())).unwrap();
    let id = &paragraph_ids(&bytes)[0];
    let ops = format!(
        r#"[{{"op":"revise_text","at":"{id}","edits":[{{"select":"quick","with":"quick very"}}]}}]"#
    );
    track(&bytes, ANN, &ops).unwrap()
}

// Applies one `revise_text` edit by `author` to the first paragraph.
fn revise(bytes: &[u8], author: &str, select: &str, with: &str) -> Result<Vec<u8>, String> {
    let id = &paragraph_ids(bytes)[0];
    let ops = format!(
        r#"[{{"op":"revise_text","at":"{id}","edits":[{{"select":"{select}","with":"{with}"}}]}}]"#
    );
    track(bytes, author, &ops)
}

const ORIGINAL: &str = "The quick brown fox jumps.";

#[test]
fn own_insertion_extends_in_place() {
    let edited = revise(&quick_very(), ANN, "very", "very very").unwrap();
    let xml = document_xml(&edited);

    assert_eq!(text(&edited, View::Final), "The quick very very brown fox jumps.");
    assert_eq!(text(&edited, View::Original), ORIGINAL);
    assert_eq!(count(&xml, "<w:del "), 0, "{xml}");
    assert_eq!(count(&xml, "<w:ins "), 1, "{xml}");
    assert_no_nested_ins(&xml);
}

#[test]
fn own_insertion_shrinks_away() {
    let edited = revise(&quick_very(), ANN, " very", "").unwrap();
    let xml = document_xml(&edited);

    assert_eq!(text(&edited, View::Final), ORIGINAL);
    assert_eq!(text(&edited, View::Original), ORIGINAL);
    assert_eq!(count(&xml, "<w:del "), 0, "{xml}");
    assert_eq!(count(&xml, "<w:ins "), 0, "{xml}");
}

#[test]
fn own_insertion_edits_inside() {
    let edited = revise(&quick_very(), ANN, "v", "vX").unwrap();
    let xml = document_xml(&edited);

    assert_eq!(text(&edited, View::Final), "The quick vXery brown fox jumps.");
    assert_eq!(text(&edited, View::Original), ORIGINAL);
    assert_eq!(count(&xml, "<w:del "), 0, "{xml}");
    assert_eq!(count(&xml, "<w:ins "), 1, "{xml}");
    assert_no_nested_ins(&xml);
}

#[test]
fn own_insertion_replaced_by_replace_text() {
    let bytes = quick_very();
    let id = &paragraph_ids(&bytes)[0];
    let ops =
        format!(r#"[{{"op":"replace_text","at":"{id}","select":"very","with":"much"}}]"#);
    let edited = track(&bytes, ANN, &ops).unwrap();
    let xml = document_xml(&edited);

    assert_eq!(text(&edited, View::Final), "The quick much brown fox jumps.");
    assert_eq!(text(&edited, View::Original), ORIGINAL);
    assert_eq!(count(&xml, "<w:del "), 0, "{xml}");
    assert_eq!(count(&xml, "<w:ins "), 1, "{xml}");
    assert_no_nested_ins(&xml);
}

#[test]
fn own_insertion_edited_twice_in_one_plan() {
    let bytes = create(Source::Html("<p>aaaa bb aaaa</p>".into())).unwrap();
    let id = &paragraph_ids(&bytes)[0];
    let ops = format!(
        r#"[{{"op":"replace_text","at":"{id}","select":"bb","with":"cc"}},
            {{"op":"replace_text","at":"{id}","select":"cc","with":"dd"}}]"#
    );
    let edited = track(&bytes, ANN, &ops).unwrap();
    let xml = document_xml(&edited);

    assert_eq!(text(&edited, View::Final), "aaaa dd aaaa");
    assert_eq!(text(&edited, View::Original), "aaaa bb aaaa");
    assert_eq!(count(&xml, "<w:del "), 1, "{xml}");
    assert_eq!(count(&xml, "<w:ins "), 1, "{xml}");
}

#[test]
fn own_insertion_with_plain_neighbor_stays_minimal() {
    let edited = revise(&quick_very(), ANN, "quick very", "quick very much").unwrap();
    let xml = document_xml(&edited);

    assert_eq!(text(&edited, View::Final), "The quick very much brown fox jumps.");
    assert_eq!(text(&edited, View::Original), ORIGINAL);
    assert_eq!(count(&xml, "<w:del "), 0, "{xml}");
    assert_eq!(count(&xml, "<w:ins "), 1, "{xml}");
    assert_no_nested_ins(&xml);
}

#[test]
fn foreign_insertion_gets_a_nested_deletion() {
    let edited = revise(&quick_very(), BOB, "very", "much").unwrap();
    let xml = document_xml(&edited);

    assert_eq!(text(&edited, View::Final), "The quick much brown fox jumps.");
    assert_eq!(text(&edited, View::Original), ORIGINAL);
    let del = xml.find("<w:del ").expect("a deletion");
    let open = xml[..del].rfind("<w:ins ").expect("Ann's insertion");
    assert!(xml[open..del].contains(ANN), "{xml}");
    assert!(!xml[open..del].contains("</w:ins>"), "del not nested: {xml}");
    assert!(xml[del..].contains(BOB), "{xml}");
}

#[test]
fn tracked_replace_paragraph_fills_an_empty_paragraph() {
    let bytes = create(Source::Html("<p>One.</p><p></p>".into())).unwrap();
    let id = &paragraph_ids(&bytes)[1];
    let ops = format!(r#"[{{"op":"replace_paragraph","at":"{id}","with":"<b>Two.</b>"}}]"#);
    let edited = track(&bytes, ANN, &ops).unwrap();

    assert_eq!(text(&edited, View::Final), "One.\nTwo.");
    assert_eq!(text(&edited, View::Original), "One.");
}

const LIST: &str = "<p>One.</p><ul><li>item one</li><li>item two</li></ul><p>End.</p>";

fn markup(bytes: &[u8]) -> String {
    Document::open(bytes).markup(View::Markup).unwrap()
}

// The opening tag of paragraph `id` in `markup`, e.g. `<p id="…" …>`.
fn opening_tag(markup: &str, id: &str) -> String {
    let start = markup.find(&format!("<p id=\"{id}\"")).expect("paragraph present");
    let end = start + markup[start..].find('>').unwrap() + 1;
    markup[start..end].to_string()
}

// LIST plus Ann's tracked paragraph "Mine." after "One." (text 1, mark 2).
fn list_with_mine() -> (Vec<u8>, Vec<u8>) {
    let bytes = create(Source::Html(LIST.into())).unwrap();
    let one = &paragraph_ids(&bytes)[0];
    let ops = format!(
        r#"[{{"op":"insert_paragraph","at":"{one}","position":"after","with":"Mine."}}]"#
    );
    let inserted = track(&bytes, ANN, &ops).unwrap();
    (bytes, inserted)
}

fn settle(bytes: &[u8], targets: &[&str], action: &str) -> Vec<u8> {
    let ops: Vec<String> = targets
        .iter()
        .map(|t| format!(r#"{{"op":"revision_settle","target":"{t}","action":"{action}"}}"#))
        .collect();
    track(bytes, ANN, &format!("[{}]", ops.join(","))).unwrap()
}

#[test]
fn rejecting_inserted_paragraph_restores_the_list() {
    let (original, inserted) = list_with_mine();
    let rejected = settle(&inserted, &["1", "2"], "reject");

    assert_eq!(markup(&rejected), markup(&original));
}

#[test]
fn rejecting_inserted_mark_joins_into_the_surviving_paragraph() {
    let (original, inserted) = list_with_mine();
    let ids = paragraph_ids(&inserted);
    let (mine, item_one) = (&ids[1], &ids[2]);
    let rejected = markup(&settle(&inserted, &["2"], "reject"));

    // "Mine." joins "item one", which keeps its list item, style and id.
    let tag = opening_tag(&markup(&original), item_one);
    let joined = format!(
        r#"<li data-docx-marker="•">{tag}<ins data-docx-revision="1" data-docx-author="Ann">Mine.</ins>item one</p>"#
    );
    assert!(rejected.contains(&joined), "{rejected}");
    assert!(!rejected.contains(mine.as_str()), "{rejected}");
}

#[test]
fn accepting_deleted_paragraph_keeps_the_next_paragraph() {
    let bytes = create(Source::Html(LIST.into())).unwrap();
    let one = paragraph_ids(&bytes)[0].clone();
    let ops = format!(r#"[{{"op":"delete_paragraphs","at":["{one}"]}}]"#);
    let deleted = track(&bytes, ANN, &ops).unwrap();

    let original = markup(&bytes);
    let without_one: String = original
        .lines()
        .filter(|line| !line.contains(&one))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(markup(&settle(&deleted, &["all"], "accept")), without_one.trim_end());
    assert_eq!(markup(&settle(&deleted, &["all"], "reject")), original);
}

// Tracked `delete_paragraphs` of the paragraph Ann inserted, by `author`.
fn delete_mine(author: &str) -> (Vec<u8>, Vec<u8>) {
    let (original, inserted) = list_with_mine();
    let mine = &paragraph_ids(&inserted)[1];
    let ops = format!(r#"[{{"op":"delete_paragraphs","at":["{mine}"]}}]"#);
    (original, track(&inserted, author, &ops).unwrap())
}

#[test]
fn deleting_own_inserted_paragraph_removes_it() {
    let (original, deleted) = delete_mine(ANN);
    let xml = document_xml(&deleted);

    assert_eq!(markup(&deleted), markup(&original));
    assert_eq!(count(&xml, "<w:ins "), 0, "{xml}");
    assert_eq!(count(&xml, "<w:del "), 0, "{xml}");
}

#[test]
fn deleting_foreign_inserted_paragraph_stays_tracked() {
    let (original, deleted) = delete_mine(BOB);
    let xml = document_xml(&deleted);

    assert!(count(&xml, "<w:del ") > 0, "{xml}");
    assert_eq!(text(&deleted, View::Final), text(&original, View::Final));
    assert_eq!(text(&deleted, View::Original), text(&original, View::Original));
}

#[test]
fn typed_text_takes_its_neighbors_formatting() {
    let bytes = create(Source::Html("<p>Plain <b>bold text</b> end.</p>".into())).unwrap();

    // A replacement wears the replaced text's formatting.
    let replaced = markup(&revise(&bytes, ANN, "text", "words").unwrap());
    assert!(replaced.contains(r#"<ins data-docx-revision="2" data-docx-author="Ann"><b>words</b></ins>"#), "{replaced}");

    // An insertion wears the formatting of the text before it.
    let inserted = markup(&revise(&bytes, ANN, "bold text", "bold new text").unwrap());
    assert!(inserted.contains(r#"<ins data-docx-revision="1" data-docx-author="Ann"><b>new </b></ins>"#), "{inserted}");

    // After plain text, typing stays plain.
    let plain = markup(&revise(&bytes, ANN, "Plain", "Plain old").unwrap());
    assert!(plain.contains(r#"<ins data-docx-revision="1" data-docx-author="Ann"> old</ins>"#), "{plain}");
}
