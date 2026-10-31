//! Markup copied from a document read is valid edit input: its inline styles
//! are the same CSS edits accept, so formatting round-trips.

use docxdriver_core::{
    execute_command, execute_plan, ChangeMode, Command, CommandRequest, CommandResult,
    CreateCommand, EditOp, InsertAnchor, InsertPosition, ParagraphAddress, Plan, PlanRequest,
    PlanResult, ReadCommand, SourceHash,
};
use serde_json::Value;

fn run(input: Option<&[u8]>, command: Command) -> CommandResult {
    execute_command(
        input,
        &CommandRequest {
            command,
            expected_source: None, now: None
        },
    )
}

fn create(html: &str) -> Vec<u8> {
    match run(
        None,
        Command::Create(CreateCommand {
            html: Some(html.into()),
            paragraphs: vec![],
        }),
    ) {
        CommandResult::Completed { bytes: Some(b), .. } => b,
        other => panic!("{other:?}"),
    }
}

fn read(bytes: &[u8]) -> Value {
    read_view(bytes, None)
}

fn read_view(bytes: &[u8], view: Option<&str>) -> Value {
    match run(
        Some(bytes),
        Command::Read(ReadCommand {
            read_kind: None,
            view: view.map(str::to_string),
        }),
    ) {
        CommandResult::Completed { result: Some(v), .. } => v,
        other => panic!("{other:?}"),
    }
}

/// The inner HTML of paragraph `id` in a read's markup.
fn inner(markup: &str, id: &str) -> String {
    let open = markup.find(&format!("id=\"{id}\"")).unwrap();
    let start = open + markup[open..].find('>').unwrap() + 1;
    let end = start + markup[start..].find("</p>").unwrap();
    markup[start..end].to_string()
}

/// A paragraph's start tag and content.
fn paragraph(read: &Value, id: &str) -> String {
    let markup = read["markup"].as_str().unwrap();
    let open = markup.find(&format!("<p id=\"{id}\"")).unwrap();
    let end = open + markup[open..].find("</p>").unwrap();
    markup[open..end].to_string()
}

fn plan(bytes: &[u8], ops: Vec<EditOp>) -> Plan {
    Plan {
        base: SourceHash::from_bytes(bytes),
        author: "test-agent".into(),
        change_mode: ChangeMode::Direct,
        ops,
    }
}

fn commit(bytes: &[u8], ops: Vec<EditOp>) -> Vec<u8> {
    commit_in(bytes, ops, ChangeMode::Direct)
}

fn commit_in(bytes: &[u8], ops: Vec<EditOp>, change_mode: ChangeMode) -> Vec<u8> {
    let p = Plan { change_mode, ..plan(bytes, ops) };
    let key = match execute_plan(bytes, &PlanRequest { plan: p.clone(), preview_key: None, now: None }) {
        PlanResult::Previewed { preview_key, .. } => preview_key,
        other => panic!("preview rejected: {other:?}"),
    };
    match execute_plan(bytes, &PlanRequest { plan: p, preview_key: Some(key), now: None }) {
        PlanResult::Committed { bytes, .. } => bytes,
        other => panic!("commit rejected: {other:?}"),
    }
}

fn rejection(bytes: &[u8], ops: Vec<EditOp>) -> (String, String) {
    match execute_plan(bytes, &PlanRequest { plan: plan(bytes, ops), preview_key: None, now: None }) {
        PlanResult::Rejected { diagnostic, .. } => (diagnostic.code, diagnostic.message),
        other => panic!("expected rejection: {other:?}"),
    }
}

const SOURCE: &str = "<p style=\"text-align:center\">Lead <span style=\"color:#123456;font-family:Arial\">blue</span> and <b>bold</b> tail</p><p>Plain <span style=\"color:#AA0000\">red</span> <span style=\"--docx-highlight:yellow\">marked</span></p><p><span style=\"font-size:14pt;color:#0000AA\">All runs share this</span></p>";

fn ids(read: &Value) -> Vec<String> {
    let markup = read["markup"].as_str().unwrap();
    markup
        .match_indices("<p id=\"")
        .map(|(i, _)| markup[i + 7..i + 15].to_string())
        .collect()
}

#[test]
fn pasted_read_markup_round_trips_through_replace_paragraph() {
    let source = create(SOURCE);
    let before = read(&source);
    let markup = before["markup"].as_str().unwrap();
    assert!(markup.contains("<span style="), "{markup}");
    for mode in [ChangeMode::Direct, ChangeMode::Track] {
        for id in ids(&before) {
            let with = inner(markup, &id);
            // Tracked replacements record the old and new content; compare
            // the new one.
            let after = read_view(
                &commit_in(
                    &source,
                    vec![EditOp::ReplaceParagraph {
                        at: ParagraphAddress::id(&id),
                        with,
                    }],
                    mode,
                ),
                Some("final"),
            );
            assert_eq!(paragraph(&after, &id), paragraph(&before, &id), "{mode:?}");
        }
    }
}

#[test]
fn pasted_runs_carry_their_formatting_into_other_paragraphs() {
    let source = create(SOURCE);
    let before = read(&source);
    let [first, second, ..] = &ids(&before)[..] else { panic!() };
    let red = inner(before["markup"].as_str().unwrap(), second);
    let after = read(&commit(
        &source,
        vec![
            EditOp::ReplaceParagraph {
                at: ParagraphAddress::id(first),
                with: red.clone(),
            },
            EditOp::InsertParagraph {
                at: InsertAnchor::Paragraph(ParagraphAddress::id(second)),
                position: InsertPosition::After,
                with: red.into(),
                style: None,
                alias: None,
            },
        ],
    ));
    let markup = after["markup"].as_str().unwrap();
    assert_eq!(
        markup.matches("<span style=\"color:#AA0000;\">red</span>").count(),
        3,
        "{markup}"
    );
}

#[test]
fn paragraph_unset_declarations_are_accepted_and_table_styles_point_to_table_ops() {
    let source = create(SOURCE);
    let before = read(&source);
    let [first, ..] = &ids(&before)[..] else { panic!() };
    let after = read(&commit(
        &source,
        vec![EditOp::InsertParagraph {
            at: InsertAnchor::Paragraph(ParagraphAddress::id(first)),
            position: InsertPosition::After,
            with: "<span style=\"color:unset;font-size:14pt\">sized</span>".into(),
            style: None,
            alias: None,
        }],
    ));
    assert!(
        // The new paragraph continues its anchor's centring, as Enter does.
        after["markup"].as_str().unwrap().contains("style=\"font-size:14pt;text-align:center;\">sized</p>"),
        "{}",
        after["markup"]
    );
    let (_, message) = rejection(
        &source,
        vec![EditOp::SetHeader {
            section: None,
            kind: None,
            with: "<table><tr><td style=\"width:72pt\"><p>x</p></td></tr></table>".into(),
        }],
    );
    assert!(message.contains("format_cell"), "{message}");
}

#[test]
fn inline_style_wins_over_inherited_paragraph_formatting() {
    let source = create(SOURCE);
    let before = read(&source);
    let [.., third] = &ids(&before)[..] else { panic!() };
    let after = read(&commit(
        &source,
        vec![EditOp::ReplaceParagraph {
            at: ParagraphAddress::id(third),
            with: "shared <span style=\"color:#00AA00\">green</span>".into(),
        }],
    ));
    let markup = paragraph(&after, third);
    assert!(
        markup.ends_with("style=\"font-size:14pt;\"><span style=\"color:#0000AA;\">shared </span><span style=\"color:#00AA00;\">green</span>"),
        "{markup}"
    );
}

#[test]
fn create_accepts_markup_from_a_read() {
    let before = read(&create(SOURCE));
    let copy = read(&create(before["markup"].as_str().unwrap()));
    let strip = |read: &Value| {
        ids(read)
            .iter()
            .map(|id| {
                let p = paragraph(read, id);
                p[p.find('>').unwrap()..].to_string()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(strip(&copy), strip(&before));
}
