//! A checked-in DOCX emitted by the reference engine remains readable through
//! the typed API. This guards the byte-level interchange contract without
//! exercising only the public typed API.

use docxdriver_core::{
    execute_plan, execute_request, ChangeMode, Command, CommandRequest, CommandResult, EditOp,
    ParagraphAddress, Plan, PlanRequest, PlanResult, ReadCommand, Request, RequestResult,
    SourceHash,
};

const TS_REFERENCE: &[u8] = include_bytes!("fixtures/ts-reference.docx");

fn read(bytes: &[u8], kind: Option<&str>, view: Option<&str>) -> serde_json::Value {
    match execute_request(
        Some(bytes),
        &Request::Command(CommandRequest {
            command: Command::Read(ReadCommand {
                read_kind: kind.map(str::to_owned),
                view: view.map(str::to_owned),
            }),
            expected_source: None,
        }),
    ) {
        RequestResult::Command(CommandResult::Completed {
            result: Some(value),
            ..
        }) => value,
        other => panic!("typed read failed: {other:?}"),
    }
}

fn markup(value: &serde_json::Value) -> &str {
    value["markup"].as_str().expect("typed document markup")
}

fn first_markup_id(value: &serde_json::Value) -> String {
    let block = markup(value);
    block
        .split_once("id=\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(id, _)| id.to_owned())
        .expect("paragraph id in typed markup")
}

#[test]
fn reads_reference_bytes_and_settles_revisions_through_plan() {
    let current = read(TS_REFERENCE, None, Some("final"));
    assert!(markup(&current).contains("brave world"));
    let baseline = read(TS_REFERENCE, None, Some("original"));
    assert!(markup(&baseline).contains("Hello world"));

    let revisions = read(TS_REFERENCE, Some("revisions"), None);
    assert!(revisions["revisions"].as_array().unwrap().len() >= 2);
    let comments = read(TS_REFERENCE, Some("comments"), None);
    assert_eq!(comments["comments"].as_array().unwrap().len(), 1);

    let doc = read(TS_REFERENCE, None, None);
    let id = first_markup_id(&doc);
    let p = Plan {
        base: SourceHash::from_bytes(TS_REFERENCE),
        author: "typed-test".into(),
        change_mode: ChangeMode::Direct,
        ops: vec![
            EditOp::RevisionSettle {
                target: docxdriver_core::RevisionTarget::All,
                action: docxdriver_core::RevisionAction::Accept,
            },
            EditOp::ReplaceText {
                at: ParagraphAddress::id(id),
                select: "brave world".into(),
                with: "brave world".into(),
                occurrence: None,
            },
        ],
    };
    let preview = match execute_plan(
        TS_REFERENCE,
        &PlanRequest {
            plan: p.clone(),
            preview_key: None,
        },
    ) {
        PlanResult::Previewed { preview_key, .. } => preview_key,
        other => panic!("reference preview failed: {other:?}"),
    };
    match execute_plan(
        TS_REFERENCE,
        &PlanRequest {
            plan: p,
            preview_key: Some(preview),
        },
    ) {
        PlanResult::Committed { bytes, .. } => {
            let accepted = read(&bytes, None, Some("original"));
            assert!(markup(&accepted).contains("brave world"));
        }
        other => panic!("reference commit failed: {other:?}"),
    }
}
