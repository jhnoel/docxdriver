//! The `Document` facade: create, read, find, review, commit on bytes.

use docxdriver_core::{create, Case, Document, Source, View};

const HTML: &str = "<h1>Title</h1><p>Rent is due in thirty days.</p><p>Second.</p>";

// The id of the first `<p>` in a markup projection.
fn first_paragraph(markup: &str) -> String {
    let start = markup.find("<p id=\"").expect("a paragraph") + "<p id=\"".len();
    markup[start..start + 8].to_string()
}

#[test]
fn creates_reads_and_finds() {
    let bytes = create(Source::Paragraphs(vec!["Alpha".into(), "Beta".into()])).unwrap();
    let doc = Document::open(&bytes);

    assert!(doc.markup(View::Markup).unwrap().contains("Beta"));
    assert!(doc.comments().unwrap().is_empty());
    assert!(doc.revisions().unwrap().is_empty());
    assert!(!doc.styles().unwrap().is_empty());

    let matches = doc.find("alpha", Case::Insensitive).unwrap();
    assert_eq!(matches.len(), 1);
    assert!(doc.find("alpha", Case::Sensitive).unwrap().is_empty());
}

#[test]
fn reviews_then_commits() {
    let bytes = create(Source::Html(HTML.into())).unwrap();
    let doc = Document::open(&bytes);
    let id = first_paragraph(&doc.markup(View::Markup).unwrap());

    // A plan without base: the document binds it.
    let json = format!(
        r#"{{"author":"T","change_mode":"track","ops":[
            {{"op":"replace_text","at":"{id}","select":"thirty","with":"sixty"}},
            {{"op":"comment_add","at":"{id}","text":"Check"}}]}}"#
    );
    let plan = doc.plan(&json).unwrap();
    let review = doc.review(&plan).unwrap();
    assert_eq!(review.report.completed, 2);

    let edited = doc.commit(&plan, &review.key).unwrap();
    let edited = Document::open(&edited);
    assert!(edited.markup(View::Final).unwrap().contains("sixty days"));
    assert!(edited.markup(View::Original).unwrap().contains("thirty days"));
    assert_eq!(edited.revisions().unwrap().len(), 2);
    assert_eq!(edited.comments().unwrap().len(), 1);
}

#[test]
fn lists_equations_by_paragraph() {
    let html = "<p>Area <math display=\"inline\"><mi>x</mi><mo>+</mo><mn>1</mn></math> here.</p>";
    let bytes = create(Source::Html(html.into())).unwrap();
    let equations = Document::open(&bytes).equations().unwrap();
    assert_eq!(equations.len(), 1, "{equations:?}");
    assert!(equations[0]["at"].is_string(), "{equations:?}");
    let plain = create(Source::Paragraphs(vec!["None".into()])).unwrap();
    assert!(Document::open(&plain).equations().unwrap().is_empty());
}

#[test]
fn refuses_stale_and_invalid_plans() {
    let bytes = create(Source::Html(HTML.into())).unwrap();
    let doc = Document::open(&bytes);
    let id = first_paragraph(&doc.markup(View::Markup).unwrap());
    let json = format!(r#"{{"author":"T","change_mode":"direct","ops":[{{"op":"replace_text","at":"{id}","select":"thirty","with":"sixty"}}]}}"#);
    let plan = doc.plan(&json).unwrap();
    let review = doc.review(&plan).unwrap();

    // Another document's bytes: the key no longer matches.
    let other = create(Source::Paragraphs(vec!["Other".into()])).unwrap();
    assert!(Document::open(&other).commit(&plan, &review.key).is_err());

    assert!(doc.plan("[]").is_err());
    assert!(doc.plan(r#"{"author":"T","change_mode":"track","ops":[{"op":"nope"}]}"#).is_err());
}
