//! Revisions and comments carry the time of the edit: the current UTC time
//! by default, or the time a caller fixes for byte-stable output.

use docxdriver_core::{
    create, execute_plan, revision_time, Document, PlanRequest, PlanResult, Source,
};

const OPS: &str = r#"[{"op":"replace_text","at":"ID","select":"thirty","with":"sixty"},
    {"op":"comment_add","at":"ID","select":"days","text":"Check"}]"#;

fn source() -> (Vec<u8>, String) {
    let bytes = create(Source::Html("<p>Rent is due in thirty days.</p>".into())).unwrap();
    let markup = Document::open(&bytes)
        .markup(docxdriver_core::View::Markup)
        .unwrap();
    let at = markup.find(" id=\"").unwrap() + 5;
    let id = markup[at..at + 8].to_string();
    (bytes, id)
}

fn commit(now: Option<&str>) -> Vec<u8> {
    let (bytes, id) = source();
    let doc = Document::open(&bytes);
    let json = format!(
        r#"{{"author":"Ann","change_mode":"track","ops":{}}}"#,
        OPS.replace("ID", &id)
    );
    let plan = doc.plan(&json).unwrap();
    let review = doc.review(&plan).unwrap();
    doc.commit_at(&plan, &review.key, now).unwrap()
}

fn dates(bytes: &[u8]) -> Vec<String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let mut text = String::new();
        if std::io::Read::read_to_string(&mut file, &mut text).is_err() {
            continue;
        }
        for (at, tag) in text.match_indices("w:date=\"") {
            out.push(text[at + tag.len()..at + tag.len() + 20].to_string());
        }
    }
    out
}

#[test]
fn revisions_carry_the_current_time_by_default() {
    let before = revision_time(None).unwrap();
    let stamped = dates(&commit(None));
    let after = revision_time(None).unwrap();
    assert!(stamped.len() >= 3, "{stamped:?}");
    for date in &stamped {
        assert!(
            before.as_str() <= date.as_str() && date.as_str() <= after.as_str(),
            "{date} not in {before}..{after}"
        );
    }
}

#[test]
fn a_fixed_time_gives_byte_stable_output() {
    let first = commit(Some("2026-10-31T12:34:56Z"));
    let second = commit(Some("2026-10-31T12:34:56Z"));
    assert_eq!(first, second);
    assert!(
        dates(&first).iter().all(|d| d == "2026-10-31T12:34:56Z"),
        "{:?}",
        dates(&first)
    );
}

#[test]
fn a_malformed_time_is_refused() {
    let (bytes, id) = source();
    let doc = Document::open(&bytes);
    let plan = doc
        .plan(&format!(
            r#"{{"author":"Ann","change_mode":"track","ops":{}}}"#,
            OPS.replace("ID", &id)
        ))
        .unwrap();
    let key = doc.review(&plan).unwrap().key;
    for bad in [
        "2026-10-31 12:34:56",
        "2026-10-31T12:34:56.123Z",
        "2026-10-31T12:34:56+00:00",
        "2026-13-45T25:61:61Z",
        "2025-02-29T12:00:00Z",
    ] {
        let request = PlanRequest {
            plan: plan.clone(),
            preview_key: Some(key.clone()),
            now: Some(bad.into()),
        };
        match execute_plan(&bytes, &request) {
            PlanResult::Rejected { diagnostic, .. } => assert_eq!(diagnostic.code, "invalid_now"),
            other => panic!("{bad}: {other:?}"),
        }
    }
}

#[test]
fn utc_times_are_formatted_as_word_writes_them() {
    let now = revision_time(None).unwrap();
    assert_eq!(now.len(), 20);
    assert!(now.ends_with('Z') && now.as_bytes()[10] == b'T', "{now}");
    assert!(now.as_str() > "2026-01-01T00:00:00Z", "{now}");
}
