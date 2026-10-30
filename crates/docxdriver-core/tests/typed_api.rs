//! Conformance for the clean-sheet typed core boundary.
//!
//! These tests intentionally use `Request`/`Command`/`Plan` directly.  The
//! historical JSON dispatcher and its operation-list compatibility shapes are
//! not part of this surface.

use std::io::{Read, Write};

use docxdriver_core::{
    execute_command, execute_plan, execute_request, ChangeMode, ChromeKind, Command,
    CommandRequest, CommandResult, CommentStatus, CreateCommand, EditCommand, EditOp, FindCommand,
    InsertAnchor, InsertPosition, LineSpacing, ParagraphAddress, Plan, PlanRequest, PlanResult,
    Points, PreviewKey, ReadCommand, Request, RequestResult, RevisionAction, RevisionTarget,
    SourceHash, TextEdit,
};

const SOURCE_ID: &str = "11111111";
const SECOND_ID: &str = "22222222";

fn package_with_body(body: &str) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let parts = [
        (
            "[Content_Types].xml",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
                "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
                "<Default Extension=\"xml\" ContentType=\"application/xml\"/>",
                "<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>",
                "</Types>"
            ),
        ),
        (
            "_rels/.rels",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
                "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>",
                "</Relationships>"
            ),
        ),
        (
            "word/document.xml",
            body,
        ),
        (
            "word/_rels/document.xml.rels",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>"
            ),
        ),
    ];
    for (name, contents) in parts {
        writer.start_file(name, options).unwrap();
        writer.write_all(contents.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn document(paragraphs: &[(&str, &str)]) -> Vec<u8> {
    let body = paragraphs
        .iter()
        .map(|(id, text)| format!("<w:p w14:paraId=\"{id}\"><w:r><w:t>{text}</w:t></w:r></w:p>"))
        .collect::<String>();
    package_with_body(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\"><w:body>{body}</w:body></w:document>"
    ))
}

fn document_with_body_table() -> Vec<u8> {
    let body = format!(
        "<w:p w14:paraId=\"{SOURCE_ID}\"><w:r><w:t>before</w:t></w:r></w:p>\
         <w:tbl><w:tr><w:tc><w:p w14:paraId=\"33333333\"><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
         <w:p w14:paraId=\"{SECOND_ID}\"><w:r><w:t>after</w:t></w:r></w:p>"
    );
    package_with_body(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\"><w:body>{body}</w:body></w:document>"
    ))
}

fn document_table_first() -> Vec<u8> {
    let body = format!(
        "<w:tbl><w:tr><w:tc><w:p w14:paraId=\"33333333\"><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>\
         <w:p w14:paraId=\"{SOURCE_ID}\"><w:r><w:t>after</w:t></w:r></w:p>"
    );
    package_with_body(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\"><w:body>{body}</w:body></w:document>"
    ))
}

fn document_adjacent_tables() -> Vec<u8> {
    let table = "<w:tbl><w:tr><w:tc><w:p w14:paraId=\"33333333\"><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
    let body = format!("{table}{table}");
    package_with_body(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\"><w:body>{body}</w:body></w:document>"
    ))
}

fn document_tables_in_two_sections() -> Vec<u8> {
    let table = |id: &str| {
        format!(
            "<w:tbl><w:tr><w:tc><w:p w14:paraId=\"{id}\"><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"
        )
    };
    let body = format!(
        "{}<w:p w14:paraId=\"{SOURCE_ID}\"><w:pPr><w:sectPr/></w:pPr></w:p>{}<w:sectPr/>",
        table("33333333"),
        table("44444444"),
    );
    package_with_body(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\"><w:body>{body}</w:body></w:document>"
    ))
}

fn read_markup(bytes: &[u8]) -> String {
    read_markup_view(bytes, None)
}

fn read_markup_view(bytes: &[u8], view: Option<&str>) -> String {
    let read = execute_request(
        Some(bytes),
        &command(Command::Read(ReadCommand {
            read_kind: None,
            view: view.map(str::to_string),
        })),
    );
    match read {
        RequestResult::Command(CommandResult::Completed {
            result: Some(value),
            ..
        }) => value["markup"].as_str().unwrap().to_string(),
        other => panic!("read failed: {other:?}"),
    }
}

fn commit_plan(bytes: &[u8], ops: Vec<EditOp>) -> Vec<u8> {
    let p = plan(bytes, ops);
    let preview = execute_plan(
        bytes,
        &PlanRequest {
            plan: p.clone(),
            preview_key: None,
        },
    );
    let key = match preview {
        PlanResult::Previewed { preview_key, .. } => preview_key,
        other => panic!("preview rejected: {other:?}"),
    };
    match execute_plan(
        bytes,
        &PlanRequest {
            plan: p,
            preview_key: Some(key),
        },
    ) {
        PlanResult::Committed { bytes, .. } => bytes,
        other => panic!("commit rejected: {other:?}"),
    }
}

fn document_xml(bytes: &[u8]) -> String {
    part_xml(bytes, "word/document.xml")
}

fn part_xml(bytes: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("valid DOCX zip");
    let mut xml = String::new();
    archive
        .by_name(name)
        .unwrap_or_else(|_| panic!("missing part {name}"))
        .read_to_string(&mut xml)
        .expect("UTF-8 part");
    xml
}

fn overwrite_part(bytes: &[u8], name: &str, contents: &[u8]) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("valid DOCX zip");
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).expect("zip entry");
        let entry_name = file.name().to_string();
        writer.start_file(&entry_name, options).unwrap();
        if entry_name == name {
            writer.write_all(contents).unwrap();
        } else {
            let mut data = Vec::new();
            file.read_to_end(&mut data).unwrap();
            writer.write_all(&data).unwrap();
        }
    }
    writer.finish().unwrap().into_inner()
}

fn read_listing(bytes: &[u8], kind: &str, view: Option<&str>) -> serde_json::Value {
    match execute_request(
        Some(bytes),
        &command(Command::Read(ReadCommand {
            read_kind: Some(
                if kind == "document" {
                    "document_ui"
                } else {
                    kind
                }
                .into(),
            ),
            view: view.map(str::to_owned),
        })),
    ) {
        RequestResult::Command(CommandResult::Completed {
            result: Some(value),
            ..
        }) => value,
        other => panic!("{kind} read failed: {other:?}"),
    }
}

/// A minimal package whose document has one paragraph and whose comments part
/// mirrors a foreign (non-docxdriver) author: the root declares only `w` and
/// `w15`, and the existing comment paragraph uses `w15:paraId` — the shape
/// that masks the engine's `w14` prefix when it appends a new comment.
fn package_with_foreign_comments() -> Vec<u8> {
    let body = format!("<w:p w14:paraId=\"{SOURCE_ID}\"><w:r><w:t>text</w:t></w:r></w:p>");
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let document_xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\"><w:body>{body}</w:body></w:document>"
    );
    let parts = [
        (
            "[Content_Types].xml",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
                "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
                "<Default Extension=\"xml\" ContentType=\"application/xml\"/>",
                "<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>",
                "<Override PartName=\"/word/comments.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml\"/>",
                "<Override PartName=\"/word/commentsExtended.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtended+xml\"/>",
                "<Override PartName=\"/word/commentsIds.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.commentsIds+xml\"/>",
                "</Types>"
            ),
        ),
        (
            "_rels/.rels",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
                "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>",
                "</Relationships>"
            ),
        ),
        ("word/document.xml", document_xml.as_str()),
        (
            "word/comments.xml",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
                "<w:comments xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" ",
                "xmlns:w15=\"http://schemas.microsoft.com/office/word/2012/wordml\">",
                "<w:comment w:id=\"0\" w:author=\"Rev\" w:date=\"2026-07-05T02:35:50.205Z\">",
                "<w:p w15:paraId=\"00000000\"><w:r><w:t>Check this</w:t></w:r></w:p>",
                "</w:comment>",
                "</w:comments>"
            ),
        ),
        (
            "word/_rels/document.xml.rels",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>"
            ),
        ),
    ];
    for (name, contents) in parts {
        writer.start_file(name, options).unwrap();
        writer.write_all(contents.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn plan(bytes: &[u8], ops: Vec<EditOp>) -> Plan {
    Plan {
        base: SourceHash::from_bytes(bytes),
        author: "test-agent".into(),
        change_mode: ChangeMode::Direct,
        ops,
    }
}

fn command(command: Command) -> Request {
    Request::Command(CommandRequest {
        command,
        expected_source: None,
    })
}

fn completed_bytes(result: CommandResult) -> Vec<u8> {
    match result {
        CommandResult::Completed { bytes, .. } => bytes.expect("command output bytes"),
        CommandResult::Rejected { diagnostic, .. } => {
            panic!(
                "command rejected: {}: {}",
                diagnostic.code, diagnostic.message
            )
        }
    }
}

#[test]
fn command_create_read_find_and_source_cas_use_typed_results() {
    let create = command(Command::Create(CreateCommand {
        paragraphs: vec!["Alpha".into(), "Beta".into()],
        html: None,
    }));
    let created = match execute_request(None, &create) {
        RequestResult::Command(result) => completed_bytes(result),
        other => panic!("expected command result, got {other:?}"),
    };

    let read = command(Command::Read(ReadCommand {
        read_kind: None,
        view: None,
    }));
    let read_result = match execute_request(Some(&created), &read) {
        RequestResult::Command(CommandResult::Completed {
            result: Some(value),
            ..
        }) => value,
        other => panic!("read failed: {other:?}"),
    };
    assert_eq!(read_result["kind"], "document");
    assert_eq!(read_result["view"], "markup");
    assert!(read_result.get("page").is_none());
    assert!(read_result.get("pages").is_none());
    assert_eq!(read_result["source"].as_str().unwrap().len(), 71);

    let find = command(Command::Find(FindCommand {
        query: "alpha".into(),
        ignore_case: true,
        view: None,
    }));
    let find_result = match execute_request(Some(&created), &find) {
        RequestResult::Command(CommandResult::Completed {
            result: Some(value),
            ..
        }) => value,
        other => panic!("find failed: {other:?}"),
    };
    assert_eq!(find_result["matches"].as_array().unwrap().len(), 1);

    let mismatch = CommandRequest {
        command: Command::Find(FindCommand {
            query: "Alpha".into(),
            ignore_case: false,
            view: None,
        }),
        expected_source: Some(SourceHash::from_bytes(b"different")),
    };
    match execute_command(Some(&created), &mismatch) {
        CommandResult::Rejected { diagnostic, .. } => {
            assert_eq!(diagnostic.code, "source_mismatch")
        }
        other => panic!("expected source rejection, got {other:?}"),
    }
}

#[test]
fn plan_preview_commit_is_keyed_and_binds_insert_aliases() {
    let source = document(&[(SOURCE_ID, "first"), (SECOND_ID, "last")]);
    let plan = plan(
        &source,
        vec![
            EditOp::InsertParagraph {
                at: InsertAnchor::paragraph(SOURCE_ID),
                position: InsertPosition::After,
                with: "inserted".into(),
                style: None,
                alias: Some("new_para".into()),
            },
            EditOp::ReplaceParagraph {
                at: ParagraphAddress::alias("new_para"),
                with: "changed".into(),
            },
        ],
    );
    let preview = execute_plan(
        &source,
        &PlanRequest {
            plan: plan.clone(),
            preview_key: None,
        },
    );
    let preview_json = serde_json::to_value(&preview).unwrap();
    assert!(
        preview_json.get("bytes").is_none(),
        "preview must not emit bytes"
    );
    let key = match &preview {
        PlanResult::Previewed {
            preview_key,
            report,
            ..
        } => {
            assert_eq!(report.completed, 2);
            assert_eq!(report.ops.len(), 2);
            assert_eq!(report.aliases["new_para"].as_str().unwrap().len(), 8);
            assert!(report.allocated_ids.len() == 1);
            preview_key.clone()
        }
        other => panic!("expected preview, got {other:?}"),
    };
    assert_eq!(key, plan.preview_key());
    let committed = execute_plan(
        &source,
        &PlanRequest {
            plan,
            preview_key: Some(key),
        },
    );
    let output = match committed {
        PlanResult::Committed { bytes, report, .. } => {
            assert_eq!(report.completed, 2);
            bytes
        }
        other => panic!("expected commit, got {other:?}"),
    };
    let read = execute_request(
        Some(&output),
        &command(Command::Read(ReadCommand {
            read_kind: None,
            view: None,
        })),
    );
    let value = match read {
        RequestResult::Command(CommandResult::Completed {
            result: Some(value),
            ..
        }) => value,
        other => panic!("read committed output failed: {other:?}"),
    };
    let markup = value["markup"].as_str().unwrap();
    assert!(markup.contains("changed"), "{markup}");
}

#[test]
fn plan_rejects_stale_key_and_stops_atomically_on_unknown_alias() {
    let source = document(&[(SOURCE_ID, "first"), (SECOND_ID, "last")]);
    let atomic = plan(
        &source,
        vec![
            EditOp::ReplaceParagraph {
                at: ParagraphAddress::id(SOURCE_ID),
                with: "changed".into(),
            },
            EditOp::ReplaceParagraph {
                at: ParagraphAddress::alias("missing"),
                with: "never applied".into(),
            },
        ],
    );
    let rejected = execute_plan(
        &source,
        &PlanRequest {
            plan: atomic.clone(),
            preview_key: None,
        },
    );
    let rejected_json = serde_json::to_value(&rejected).unwrap();
    assert!(
        rejected_json.get("bytes").is_none(),
        "rejected plans must not emit bytes"
    );
    match &rejected {
        PlanResult::Rejected {
            diagnostic,
            report: Some(report),
            ..
        } => {
            assert_eq!(diagnostic.code, "unknown_alias");
            assert_eq!(report.completed, 1);
            assert_eq!(report.stopped_at, Some(1));
            assert_eq!(report.ops.len(), 1);
        }
        other => panic!("expected atomic rejection, got {other:?}"),
    }
    let key = atomic.preview_key();
    let stale = execute_plan(
        &document(&[(SOURCE_ID, "changed"), (SECOND_ID, "last")]),
        &PlanRequest {
            plan: atomic,
            preview_key: Some(key),
        },
    );
    match stale {
        PlanResult::Rejected { diagnostic, .. } => assert_eq!(diagnostic.code, "source_mismatch"),
        other => panic!("expected stale source rejection, got {other:?}"),
    }
}

#[test]
fn every_edit_op_and_command_variant_has_strict_stable_json_and_toml() {
    let id = ParagraphAddress::id(SOURCE_ID);
    let ops = vec![
        EditOp::ReplaceText {
            at: id.clone(),
            select: "old".into(),
            with: "new".into(),
            occurrence: Some(1),
        },
        EditOp::ReviseText {
            at: Some(id.clone()),
            within: Some("old text".into()),
            occurrence: Some(1),
            edits: vec![
                TextEdit {
                    select: "old".into(),
                    with: "new".into(),
                    occurrence: None,
                    all: true,
                },
                TextEdit {
                    select: "text".into(),
                    with: "words".into(),
                    occurrence: Some(1),
                    all: false,
                },
            ],
        },
        EditOp::ReviseText {
            at: None,
            within: None,
            occurrence: None,
            edits: vec![TextEdit {
                select: "a".into(),
                with: "b".into(),
                occurrence: None,
                all: true,
            }],
        },
        EditOp::ReplaceParagraph {
            at: id.clone(),
            with: "paragraph".into(),
        },
        EditOp::FormatText {
            at: id.clone(),
            select: "text".into(),
            occurrence: None,
            bold: Some(true),
            italic: Some(false),
            underline: Some(true),
            strike: Some(false),
            superscript: Some(false),
            subscript: Some(false),
                highlight: None,
            color: Some("#AABBCC".into()),
            font_size: Some(Points::from_points(12.5).unwrap()),
            clear: vec!["italic".into()],
        },
        EditOp::FormatParagraph {
            at: id.clone(),
            style: Some("Normal".into()),
            alignment: Some("justify".into()),
            indent_left: Some(docxdriver_core::SignedPoints::from_points(1.25).unwrap()),
            indent_right: None,
            first_line_indent: None,
            space_before: None,
            space_after: Some(Points::from_points(2.0).unwrap()),
            line_spacing: Some(LineSpacing::Multiple(1.15)),
            clear: vec![],
        },
        EditOp::InsertParagraph {
            at: id.clone().into(),
            position: InsertPosition::Before,
            with: "new paragraph".into(),
            style: Some("Heading 1".into()),
            alias: Some("inserted".into()),
        },
        EditOp::InsertParagraph {
            at: InsertAnchor::table(1),
            position: InsertPosition::After,
            with: "after table anchor".into(),
            style: None,
            alias: None,
        },
        EditOp::DeleteParagraphs {
            at: vec![id.clone()],
        },
        EditOp::SetPageMargins {
            section: Some(1),
            top: Some(Points::from_points(72.0).unwrap()),
            right: None,
            bottom: None,
            left: Some(Points::from_points(72.0).unwrap()),
            header: None,
            footer: None,
            gutter: None,
        },
        EditOp::SetEvenAndOddHeaders { even_and_odd: true },
        EditOp::SetHeader {
            section: Some(1),
            kind: Some(ChromeKind::Default),
            with: "<p>Header</p>".into(),
        },
        EditOp::SetFooter {
            section: None,
            kind: None,
            with: "<p>Footer</p>".into(),
        },
        EditOp::ClearHeader {
            section: Some(1),
            kind: None,
        },
        EditOp::ClearFooter {
            section: None,
            kind: Some(ChromeKind::Default),
        },
        EditOp::CommentAdd {
            at: id.clone(),
            select: Some("text".into()),
            occurrence: Some(1),
            text: "review".into(),
        },
        EditOp::CommentReply {
            comment_id: "0".into(),
            text: "reply".into(),
        },
        EditOp::CommentSetStatus {
            comment_id: "0".into(),
            status: CommentStatus::Resolved,
        },
        EditOp::CommentDelete {
            comment_id: "0".into(),
        },
        EditOp::BookmarkAdd {
            at: id.clone(),
            name: "Clause".into(),
            select: Some("text".into()),
            occurrence: Some(1),
        },
        EditOp::BookmarkRename {
            name: "Clause".into(),
            new_name: "Clause_2".into(),
        },
        EditOp::BookmarkMove {
            name: "Clause_2".into(),
            at: id.clone(),
            select: None,
            occurrence: None,
        },
        EditOp::BookmarkDelete {
            name: "Clause_2".into(),
        },
        EditOp::RevisionSettle {
            target: RevisionTarget::All,
            action: RevisionAction::Accept,
        },
    ];
    let source = document(&[(SOURCE_ID, "text")]);
    let p = plan(&source, ops);
    let json = p.canonical_json().expect("canonical JSON");
    let from_json = Plan::from_json(&json).expect("JSON roundtrip");
    assert_eq!(from_json, p);
    let toml = p.canonical_toml().expect("canonical TOML");
    let from_toml = Plan::from_toml(&toml).expect("TOML roundtrip");
    assert_eq!(from_toml, p);

    let commands = [
        Command::Create(CreateCommand {
            paragraphs: vec!["x".into()],
            html: None,
        }),
        Command::Read(ReadCommand {
            read_kind: Some("styles".into()),
            view: None,
        }),
        Command::Find(FindCommand {
            query: "x".into(),
            ignore_case: true,
            view: Some(docxdriver_core::FindView::Final),
        }),
        Command::Edit(EditCommand {
            author: "a".into(),
            change_mode: ChangeMode::Track,
            op: EditOp::ReplaceParagraph {
                at: ParagraphAddress::id(SOURCE_ID),
                with: "y".into(),
            },
        }),
    ];
    for c in commands {
        let encoded = serde_json::to_string(&c).unwrap();
        let decoded: Command = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, c);
    }
}

#[test]
fn strict_validation_reports_unknown_fields_invalid_units_and_toml_spans() {
    let source = SourceHash::from_bytes(b"source").as_str();
    let unknown = format!(
        r#"{{"base":"{source}","author":"a","change_mode":"track","ops":[{{"op":"replace_text","at":"{SOURCE_ID}","select":"x","with":"y","future":true}}]}}"#
    );
    let error = Plan::from_json(&unknown).unwrap_err();
    assert_eq!(error.code, "invalid_plan");

    let invalid_color = format!(
        r##"{{
  "base": "{source}",
  "author": "a",
  "change_mode": "track",
  "ops": [{{
    "op": "format_text",
    "at": "{SOURCE_ID}",
    "select": "x",
    "color": "#aabbcc"
  }}]
}}"##
    );
    let error = Plan::from_json(&invalid_color).unwrap_err();
    assert_eq!(error.code, "invalid_operation");
    assert_eq!(error.path.as_deref(), Some("ops[0].color"));
    let span = error.span.expect("field diagnostics retain source spans");
    assert!(span.line > 1 && span.column > 1);
    assert!(invalid_color
        .lines()
        .nth(span.line - 1)
        .unwrap()
        .contains("color"));

    let toml = format!(
        "base = \"{source}\"\nauthor = \"a\"\nchange_mode = \"track\"\n\n[[ops]]\nop = \"replace_text\"\nat = \"{SOURCE_ID}\"\nselect = \"x\"\nwith = \"y\"\nunknown = true\n"
    );
    let error = Plan::from_toml(&toml).unwrap_err();
    assert_eq!(error.code, "invalid_plan");
    let span = error.span.expect("TOML diagnostics retain source spans");
    assert!(toml.lines().nth(span.line - 1).unwrap().contains("unknown"));

    assert!(Points::from_points(0.01).is_err());
    assert!(serde_json::from_str::<ParagraphAddress>(r#""$bad-name""#).is_err());
    assert!(serde_json::from_str::<ParagraphAddress>(r#""1111111a""#).is_err());
    assert!(serde_json::from_str::<RevisionTarget>(r#""x""#).is_err());
}

#[test]
fn typed_read_kinds_and_full_document_inspection_have_structured_results() {
    let source = document(&[(SOURCE_ID, &"x".repeat(40_000)), (SECOND_ID, "tail")]);
    for kind in ["styles", "comments", "revisions"] {
        let result = execute_request(
            Some(&source),
            &command(Command::Read(ReadCommand {
                read_kind: Some(
                    if kind == "document" {
                        "document_ui"
                    } else {
                        kind
                    }
                    .into(),
                ),
                view: None,
            })),
        );
        match result {
            RequestResult::Command(CommandResult::Completed {
                result: Some(value),
                ..
            }) => {
                assert_eq!(value["kind"], kind);
                assert!(value["source"].as_str().is_some());
            }
            other => panic!("{kind} read failed: {other:?}"),
        }
    }
    let document = execute_request(
        Some(&source),
        &command(Command::Read(ReadCommand {
            read_kind: None,
            view: None,
        })),
    );
    match document {
        RequestResult::Command(CommandResult::Completed {
            result: Some(value),
            ..
        }) => {
            assert!(value["markup"].as_str().unwrap().contains("tail"));
            assert!(value.get("page").is_none());
            assert!(value.get("pages").is_none());
        }
        other => panic!("document inspection failed: {other:?}"),
    }
    let listing_with_view = read_listing(&source, "styles", Some("markup"));
    assert_eq!(listing_with_view["kind"], "styles");
}

#[test]
fn comment_add_then_read_lists_the_thread() {
    let source = document(&[(SOURCE_ID, "Hello world")]);
    let commented = commit_plan(
        &source,
        vec![EditOp::CommentAdd {
            at: ParagraphAddress::id(SOURCE_ID.to_string()),
            select: None,
            occurrence: None,
            text: "TODO review this".into(),
        }],
    );
    let listing = read_listing(&commented, "comments", None);
    assert_eq!(listing["kind"], "comments");
    let threads = listing["comments"].as_array().expect("comments array");
    assert_eq!(threads.len(), 1, "{listing}");
    assert_eq!(threads[0]["id"], "0");
    assert_eq!(threads[0]["status"], "open");
    assert_eq!(threads[0]["root"]["text"], "TODO review this");
    assert_eq!(threads[0]["root"]["author"], "test-agent");
    assert_eq!(threads[0]["anchor"]["ranges"][0]["locator"], "p1:0-11");

    let markup = read_markup(&commented);
    assert!(
        markup.contains("<span data-docx-comment-start=\"0\" hidden></span>")
            && markup.contains("<span data-docx-comment-end=\"0\" hidden></span>"),
        "document markup should carry comment range milestones: {markup}"
    );

    let with_view = read_listing(&commented, "comments", Some("markup"));
    assert_eq!(with_view["comments"][0]["root"]["text"], "TODO review this");
}

#[test]
fn document_read_survives_unreadable_comments_part() {
    let source = document(&[(SOURCE_ID, "Hello world")]);
    let commented = commit_plan(
        &source,
        vec![EditOp::CommentAdd {
            at: ParagraphAddress::id(SOURCE_ID.to_string()),
            select: None,
            occurrence: None,
            text: "TODO".into(),
        }],
    );
    let broken = overwrite_part(&commented, "word/comments.xml", b"<not-xml");
    let markup = read_markup(&broken);
    assert!(
        markup.contains("Hello world"),
        "document read must still render when comments.xml is unreadable: {markup}"
    );
    match execute_request(
        Some(&broken),
        &command(Command::Read(ReadCommand {
            read_kind: Some("comments".into()),
            view: None,
        })),
    ) {
        RequestResult::Command(CommandResult::Rejected { diagnostic, .. }) => {
            assert_eq!(diagnostic.code, "command_rejected");
            assert!(
                diagnostic.message.contains("word/comments.xml"),
                "{}",
                diagnostic.message
            );
        }
        other => panic!("expected comments-kind rejection, got {other:?}"),
    }
}

#[test]
fn preview_key_is_stable_and_wrong_key_rejects_before_fold() {
    let source = document(&[(SOURCE_ID, "old")]);
    let p = plan(
        &source,
        vec![EditOp::ReplaceText {
            at: ParagraphAddress::id(SOURCE_ID),
            select: "old".into(),
            with: "new".into(),
            occurrence: None,
        }],
    );
    let a = PreviewKey::new(&p);
    let b = PreviewKey::new(&Plan::from_json(&p.canonical_json().unwrap()).unwrap());
    assert_eq!(a, b);
    let wrong = execute_plan(
        &source,
        &PlanRequest {
            plan: p,
            preview_key: Some(PreviewKey(
                "p1:sha256:0000000000000000000000000000000000000000000000000000000000000000".into(),
            )),
        },
    );
    match wrong {
        PlanResult::Rejected {
            diagnostic, report, ..
        } => {
            assert_eq!(diagnostic.code, "preview_key_mismatch");
            assert!(report.is_none());
        }
        other => panic!("expected key rejection, got {other:?}"),
    }
}

#[test]
fn every_edit_op_executes_through_an_individual_typed_command() {
    let fresh = || document(&[(SOURCE_ID, "text")]);
    let run_mode = |bytes: &[u8], op: EditOp, change_mode: ChangeMode| {
        let request = CommandRequest {
            command: Command::Edit(EditCommand {
                author: "typed-test".into(),
                change_mode,
                op,
            }),
            expected_source: None,
        };
        match execute_command(Some(bytes), &request) {
            CommandResult::Completed {
                bytes: Some(out), ..
            } => out,
            CommandResult::Completed { bytes: None, .. } => panic!("edit returned no bytes"),
            CommandResult::Rejected { diagnostic, .. } => {
                panic!(
                    "typed edit rejected: {}: {}",
                    diagnostic.code, diagnostic.message
                )
            }
        }
    };
    let run = |bytes: &[u8], op: EditOp| run_mode(bytes, op, ChangeMode::Direct);
    let id = ParagraphAddress::id(SOURCE_ID);
    let _ = run(
        &fresh(),
        EditOp::ReplaceText {
            at: id.clone(),
            select: "text".into(),
            with: "changed".into(),
            occurrence: None,
        },
    );
    let _ = run(
        &fresh(),
        EditOp::ReplaceParagraph {
            at: id.clone(),
            with: "changed paragraph".into(),
        },
    );
    let _ = run(
        &fresh(),
        EditOp::FormatText {
            at: id.clone(),
            select: "text".into(),
            occurrence: None,
            bold: Some(true),
            italic: None,
            underline: None,
            strike: None,
            superscript: None,
            subscript: None,
                highlight: None,
            color: None,
            font_size: None,
            clear: vec![],
        },
    );
    let _ = run(
        &fresh(),
        EditOp::FormatParagraph {
            at: id.clone(),
            style: None,
            alignment: Some("center".into()),
            indent_left: None,
            indent_right: None,
            first_line_indent: None,
            space_before: None,
            space_after: None,
            line_spacing: Some(LineSpacing::Exact(Points::from_points(12.0).unwrap())),
            clear: vec![],
        },
    );
    let _ = run(
        &fresh(),
        EditOp::InsertParagraph {
            at: id.clone().into(),
            position: InsertPosition::After,
            with: "inserted".into(),
            style: None,
            alias: None,
        },
    );
    let _ = run(
        &document(&[(SOURCE_ID, "first"), (SECOND_ID, "second")]),
        EditOp::DeleteParagraphs {
            at: vec![ParagraphAddress::id(SECOND_ID)],
        },
    );
    let _ = run(
        &fresh(),
        EditOp::SetPageMargins {
            section: None,
            top: Some(Points::from_points(72.0).unwrap()),
            right: None,
            bottom: None,
            left: None,
            header: None,
            footer: None,
            gutter: None,
        },
    );
    let _ = run(
        &fresh(),
        EditOp::SetEvenAndOddHeaders { even_and_odd: true },
    );
    let with_header = run(
        &fresh(),
        EditOp::SetHeader {
            section: None,
            kind: None,
            with: "<p>Acme Inc</p>".into(),
        },
    );
    let markup = read_markup(&with_header);
    assert!(markup.contains("<header>") && markup.contains("Acme Inc"));
    let with_footer = run(
        &with_header,
        EditOp::SetFooter {
            section: None,
            kind: None,
            with: "<p>Page 1</p>".into(),
        },
    );
    let markup = read_markup(&with_footer);
    assert!(markup.contains("<footer>") && markup.contains("Page 1"));
    let with_first = run(
        &fresh(),
        EditOp::SetHeader {
            section: None,
            kind: Some(ChromeKind::First),
            with: "<p>First page</p>".into(),
        },
    );
    let markup = read_markup(&with_first);
    assert!(markup.contains("<header data-docx-kind=\"first\">"));
    assert!(markup.contains("data-docx-first-page=\"true\""));
    let replaced = run(
        &with_header,
        EditOp::SetHeader {
            section: None,
            kind: None,
            with: "<p>Acme Corp</p>".into(),
        },
    );
    let markup = read_markup(&replaced);
    assert!(markup.contains("Acme Corp"));
    let cleared = run(
        &with_header,
        EditOp::ClearHeader {
            section: None,
            kind: None,
        },
    );
    let markup = read_markup(&cleared);
    assert!(!markup.contains("<header>"));

    let commented = run(
        &fresh(),
        EditOp::CommentAdd {
            at: id.clone(),
            select: Some("text".into()),
            occurrence: None,
            text: "review".into(),
        },
    );
    let replied = run(
        &commented,
        EditOp::CommentReply {
            comment_id: "0".into(),
            text: "reply".into(),
        },
    );
    let resolved = run(
        &replied,
        EditOp::CommentSetStatus {
            comment_id: "0".into(),
            status: CommentStatus::Resolved,
        },
    );
    let _ = run(
        &resolved,
        EditOp::CommentDelete {
            comment_id: "0".into(),
        },
    );
    for mode in [ChangeMode::Direct, ChangeMode::Track] {
        let added = run_mode(
            &fresh(),
            EditOp::BookmarkAdd {
                at: id.clone(),
                name: "Mark".into(),
                select: Some("ex".into()),
                occurrence: None,
            },
            mode,
        );
        let renamed = run_mode(
            &added,
            EditOp::BookmarkRename {
                name: "Mark".into(),
                new_name: "Mark_2".into(),
            },
            mode,
        );
        let moved = run_mode(
            &renamed,
            EditOp::BookmarkMove {
                name: "Mark_2".into(),
                at: id.clone(),
                select: None,
                occurrence: None,
            },
            mode,
        );
        let deleted = run_mode(
            &moved,
            EditOp::BookmarkDelete {
                name: "mark_2".into(),
            },
            mode,
        );
        assert!(!document_xml(&deleted).contains("bookmark"), "{}", document_xml(&deleted));
    }

    let tracked = run_mode(
        &fresh(),
        EditOp::ReplaceText {
            at: id.clone(),
            select: "text".into(),
            with: "tracked".into(),
            occurrence: None,
        },
        ChangeMode::Track,
    );
    let settled = run(
        &tracked,
        EditOp::RevisionSettle {
            target: RevisionTarget::All,
            action: RevisionAction::Accept,
        },
    );
    let settled_xml = document_xml(&settled);
    assert!(
        !settled_xml.contains("<w:ins"),
        "accepted revisions remain: {settled_xml}"
    );
    assert!(
        !settled_xml.contains("<w:del"),
        "rejected revisions remain: {settled_xml}"
    );
}

#[test]
fn every_edit_op_also_executes_as_a_single_plan_entry() {
    let fresh = || document(&[(SOURCE_ID, "text")]);
    let run_plan_mode = |bytes: &[u8], op: EditOp, change_mode: ChangeMode| {
        let mut p = plan(bytes, vec![op]);
        p.change_mode = change_mode;
        let key = match execute_plan(bytes, &PlanRequest { plan: p.clone(), preview_key: None }) {
            PlanResult::Previewed { preview_key, .. } => preview_key,
            other => panic!("plan preview rejected: {other:?}"),
        };
        match execute_plan(bytes, &PlanRequest { plan: p, preview_key: Some(key) }) {
            PlanResult::Committed { bytes, .. } => bytes,
            other => panic!("plan commit rejected: {other:?}"),
        }
    };
    let run_plan = |bytes: &[u8], op: EditOp| {
        let p = plan(bytes, vec![op]);
        let preview = execute_plan(
            bytes,
            &PlanRequest {
                plan: p.clone(),
                preview_key: None,
            },
        );
        let key = match preview {
            PlanResult::Previewed { preview_key, .. } => preview_key,
            other => panic!("plan preview rejected: {other:?}"),
        };
        match execute_plan(
            bytes,
            &PlanRequest {
                plan: p,
                preview_key: Some(key),
            },
        ) {
            PlanResult::Committed { bytes, .. } => bytes,
            other => panic!("plan commit rejected: {other:?}"),
        }
    };
    let id = ParagraphAddress::id(SOURCE_ID);
    let _ = run_plan(
        &fresh(),
        EditOp::ReplaceText {
            at: id.clone(),
            select: "text".into(),
            with: "changed".into(),
            occurrence: None,
        },
    );
    let _ = run_plan(
        &fresh(),
        EditOp::ReplaceParagraph {
            at: id.clone(),
            with: "changed paragraph".into(),
        },
    );
    let _ = run_plan(
        &fresh(),
        EditOp::FormatText {
            at: id.clone(),
            select: "text".into(),
            occurrence: None,
            bold: Some(true),
            italic: None,
            underline: None,
            strike: None,
            superscript: None,
            subscript: None,
                highlight: None,
            color: None,
            font_size: None,
            clear: vec![],
        },
    );
    let _ = run_plan(
        &fresh(),
        EditOp::FormatParagraph {
            at: id.clone(),
            style: None,
            alignment: Some("justify".into()),
            indent_left: None,
            indent_right: None,
            first_line_indent: None,
            space_before: None,
            space_after: None,
            line_spacing: Some(LineSpacing::Multiple(1.15)),
            clear: vec![],
        },
    );
    let _ = run_plan(
        &fresh(),
        EditOp::InsertParagraph {
            at: id.clone().into(),
            position: InsertPosition::Before,
            with: "inserted".into(),
            style: None,
            alias: None,
        },
    );
    let _ = run_plan(
        &document(&[(SOURCE_ID, "first"), (SECOND_ID, "second")]),
        EditOp::DeleteParagraphs {
            at: vec![ParagraphAddress::id(SECOND_ID)],
        },
    );
    let _ = run_plan(
        &fresh(),
        EditOp::SetPageMargins {
            section: None,
            top: Some(Points::from_points(72.0).unwrap()),
            right: None,
            bottom: None,
            left: None,
            header: None,
            footer: None,
            gutter: None,
        },
    );
    let _ = run_plan(
        &fresh(),
        EditOp::SetEvenAndOddHeaders { even_and_odd: true },
    );
    let _ = run_plan(
        &fresh(),
        EditOp::SetHeader {
            section: None,
            kind: None,
            with: "<p>Acme Inc</p>".into(),
        },
    );
    let with_header = run_plan(
        &fresh(),
        EditOp::SetHeader {
            section: None,
            kind: None,
            with: "<p>Acme Inc</p>".into(),
        },
    );
    let _ = run_plan(
        &with_header,
        EditOp::SetFooter {
            section: None,
            kind: None,
            with: "<p>Page 1</p>".into(),
        },
    );
    let _ = run_plan(
        &with_header,
        EditOp::ClearHeader {
            section: None,
            kind: None,
        },
    );
    let with_footer = run_plan(
        &with_header,
        EditOp::SetFooter {
            section: None,
            kind: None,
            with: "<p>Page 1</p>".into(),
        },
    );
    let _ = run_plan(
        &with_footer,
        EditOp::ClearFooter {
            section: None,
            kind: None,
        },
    );
    let commented = run_plan(
        &fresh(),
        EditOp::CommentAdd {
            at: id.clone(),
            select: Some("text".into()),
            occurrence: None,
            text: "review".into(),
        },
    );
    let replied = run_plan(
        &commented,
        EditOp::CommentReply {
            comment_id: "0".into(),
            text: "reply".into(),
        },
    );
    let resolved = run_plan(
        &replied,
        EditOp::CommentSetStatus {
            comment_id: "0".into(),
            status: CommentStatus::Resolved,
        },
    );
    let _ = run_plan(
        &resolved,
        EditOp::CommentDelete {
            comment_id: "0".into(),
        },
    );
    for mode in [ChangeMode::Direct, ChangeMode::Track] {
        let added = run_plan_mode(
            &fresh(),
            EditOp::BookmarkAdd {
                at: id.clone(),
                name: "Mark".into(),
                select: Some("ex".into()),
                occurrence: None,
            },
            mode,
        );
        let renamed = run_plan_mode(
            &added,
            EditOp::BookmarkRename {
                name: "Mark".into(),
                new_name: "Mark_2".into(),
            },
            mode,
        );
        let moved = run_plan_mode(
            &renamed,
            EditOp::BookmarkMove {
                name: "Mark_2".into(),
                at: id.clone(),
                select: None,
                occurrence: None,
            },
            mode,
        );
        let deleted = run_plan_mode(
            &moved,
            EditOp::BookmarkDelete {
                name: "mark_2".into(),
            },
            mode,
        );
        assert!(!document_xml(&deleted).contains("bookmark"), "{}", document_xml(&deleted));
    }
    let tracked = {
        let request = CommandRequest {
            command: Command::Edit(EditCommand {
                author: "typed-test".into(),
                change_mode: ChangeMode::Track,
                op: EditOp::ReplaceText {
                    at: id.clone(),
                    select: "text".into(),
                    with: "tracked".into(),
                    occurrence: None,
                },
            }),
            expected_source: None,
        };
        match execute_command(Some(&fresh()), &request) {
            CommandResult::Completed {
                bytes: Some(bytes), ..
            } => bytes,
            other => panic!("tracked setup failed: {other:?}"),
        }
    };
    let _ = run_plan(
        &tracked,
        EditOp::RevisionSettle {
            target: RevisionTarget::All,
            action: RevisionAction::Accept,
        },
    );
}

#[test]
fn insert_paragraph_table_anchor_numbers_body_tables_and_inserts_siblings() {
    let source = document_with_body_table();
    let markup = read_markup(&source);
    assert!(markup.contains("<table data-docx-table=\"1\" data-docx-handle=\"table:1\">"));
    assert_eq!(markup.matches(" data-docx-table=\"").count(), 1);

    let after = commit_plan(
        &source,
        vec![EditOp::InsertParagraph {
            at: InsertAnchor::table(1),
            position: InsertPosition::After,
            with: "after table".into(),
            style: None,
            alias: None,
        }],
    );
    let xml = document_xml(&after);
    assert!(
        xml.contains("</w:tbl><w:p"),
        "expected paragraph sibling after table: {xml}"
    );
    assert!(!xml.contains("</w:tc><w:p w14:paraId"));
    let markup = read_markup(&after);
    assert!(markup.contains("after table"));

    let before = commit_plan(
        &source,
        vec![EditOp::InsertParagraph {
            at: InsertAnchor::table(1),
            position: InsertPosition::Before,
            with: "before table".into(),
            style: None,
            alias: None,
        }],
    );
    let xml = document_xml(&before);
    assert!(
        xml.contains("</w:p><w:tbl"),
        "expected paragraph sibling before table: {xml}"
    );

    let leading = commit_plan(
        &document_table_first(),
        vec![EditOp::InsertParagraph {
            at: InsertAnchor::table(1),
            position: InsertPosition::Before,
            with: "leading".into(),
            style: None,
            alias: None,
        }],
    );
    let xml = document_xml(&leading);
    let body_start = xml.find("<w:body>").expect("body");
    let body = &xml[body_start..];
    assert!(
        body.starts_with("<w:body><w:p"),
        "expected leading paragraph before first table: {xml}"
    );
    assert!(body.contains("<w:tbl>"));

    let between = commit_plan(
        &document_adjacent_tables(),
        vec![EditOp::InsertParagraph {
            at: InsertAnchor::table(1),
            position: InsertPosition::After,
            with: "between".into(),
            style: None,
            alias: None,
        }],
    );
    let xml = document_xml(&between);
    let between_pos = xml.find("between").expect("inserted text");
    let first_tbl_end = xml.find("</w:tbl>").expect("first table");
    let second_tbl = xml[first_tbl_end + "</w:tbl>".len()..]
        .find("<w:tbl>")
        .expect("second table");
    assert!(
        between_pos > first_tbl_end && between_pos < first_tbl_end + second_tbl,
        "paragraph should sit between tables: {xml}"
    );

    let rejected = execute_plan(
        &source,
        &PlanRequest {
            plan: plan(
                &source,
                vec![EditOp::InsertParagraph {
                    at: InsertAnchor::table(9),
                    position: InsertPosition::After,
                    with: "nope".into(),
                    style: None,
                    alias: None,
                }],
            ),
            preview_key: None,
        },
    );
    match rejected {
        PlanResult::Rejected { diagnostic, .. } => {
            assert_eq!(diagnostic.code, "operation_rejected");
            assert!(diagnostic.message.contains("no body-level table 9"));
        }
        other => panic!("expected rejection, got {other:?}"),
    }
}

#[test]
fn first_comment_gets_a_valid_nonzero_para_id() {
    let source = document(&[(SOURCE_ID, "text")]);
    let commented = commit_plan(
        &source,
        vec![EditOp::CommentAdd {
            at: ParagraphAddress::id(SOURCE_ID.to_string()),
            select: None,
            occurrence: None,
            text: "review".into(),
        }],
    );
    let comments = part_xml(&commented, "word/comments.xml");
    let at = comments
        .find("w14:paraId=\"")
        .expect("comment paragraph must carry w14:paraId");
    let value = &comments[at + "w14:paraId=\"".len()..][..8];
    let parsed = u32::from_str_radix(value, 16).expect("hex paraId");
    assert!(
        parsed > 0 && parsed < 0x8000_0000,
        "first comment paraId must be in (0, 0x80000000), got {value:?}: {comments}"
    );
}

#[test]
fn comment_add_declares_w14_on_pre_existing_foreign_comments_part() {
    let source = package_with_foreign_comments();
    let commented = commit_plan(
        &source,
        vec![EditOp::CommentAdd {
            at: ParagraphAddress::id(SOURCE_ID.to_string()),
            select: Some("text".into()),
            occurrence: None,
            text: "new comment".into(),
        }],
    );
    let comments = part_xml(&commented, "word/comments.xml");
    assert!(
        comments.contains("xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\""),
        "comments part must declare the w14 namespace before using w14:paraId: {comments}"
    );
    let extended = part_xml(&commented, "word/commentsExtended.xml");
    assert!(
        extended.contains("xmlns:w15=\"http://schemas.microsoft.com/office/word/2012/wordml\""),
        "commentsExtended part must declare w15: {extended}"
    );
    assert!(
        !extended.contains("mc:Ignorable"),
        "Word recovers commentsExtended as empty if its own w15 prefix is ignorable: {extended}"
    );
    let ids = part_xml(&commented, "word/commentsIds.xml");
    assert!(
        ids.contains("xmlns:w16cid=\"http://schemas.microsoft.com/office/word/2016/wordml/cid\""),
        "commentsIds part must declare w16cid: {ids}"
    );
    assert!(
        !ids.contains("mc:Ignorable"),
        "Word recovers commentsIds as empty if its own w16cid prefix is ignorable: {ids}"
    );
}

#[test]
fn comment_reference_run_follows_the_comment_range() {
    let source = document(&[(SOURCE_ID, "text")]);
    let commented = commit_plan(
        &source,
        vec![EditOp::CommentAdd {
            at: ParagraphAddress::id(SOURCE_ID.to_string()),
            select: None,
            occurrence: None,
            text: "review".into(),
        }],
    );
    let xml = document_xml(&commented);
    let start = xml.find("commentRangeStart").expect("range start");
    let end = xml.find("commentRangeEnd").expect("range end");
    let reference = xml.find("commentReference").expect("reference run");
    assert!(
        start < end && end < reference,
        "Word places commentReference after commentRangeEnd (start {start}, end {end}, reference {reference}): {xml}"
    );
}

#[test]
fn comment_add_writes_word_compatible_comment_parts() {
    let source = document(&[(SOURCE_ID, "text")]);
    let commented = commit_plan(
        &source,
        vec![EditOp::CommentAdd {
            at: ParagraphAddress::id(SOURCE_ID.to_string()),
            select: None,
            occurrence: None,
            text: "TODO".into(),
        }],
    );

    let comments = part_xml(&commented, "word/comments.xml");
    assert!(
        comments.contains("w:initials="),
        "Word-shaped comments carry w:initials: {comments}"
    );
    assert!(
        comments.contains("<w:annotationRef"),
        "Word-shaped comments carry w:annotationRef: {comments}"
    );
    assert!(
        comments.contains("w:val=\"CommentText\""),
        "comment paragraph should use CommentText: {comments}"
    );
    let para_at = comments
        .find("w14:paraId=\"")
        .expect("comment paragraph must carry w14:paraId");
    let para_id = &comments[para_at + "w14:paraId=\"".len()..][..8];
    assert_ne!(
        para_id, SOURCE_ID,
        "comment paraId must not collide with a document paragraph id: {comments}"
    );
    assert!(
        comments.contains("mc:Ignorable=\"w14\""),
        "comments.xml may ignorable w14, but not the w: root: {comments}"
    );
    let extended = part_xml(&commented, "word/commentsExtended.xml");
    assert!(
        !extended.contains("mc:Ignorable"),
        "Word recovers commentsExtended as empty if its own w15 prefix is ignorable: {extended}"
    );

    let ids = part_xml(&commented, "word/commentsIds.xml");
    assert!(
        !ids.contains("mc:Ignorable"),
        "Word recovers commentsIds as empty if its own w16cid prefix is ignorable: {ids}"
    );
    let durable_at = ids
        .find("w16cid:durableId=\"")
        .expect("commentsIds must carry durableId");
    let durable = &ids[durable_at + "w16cid:durableId=\"".len()..][..8];
    let durable_value = u32::from_str_radix(durable, 16).expect("hex durableId");
    assert!(
        durable_value > 0 && durable_value < 0x7FFF_FFFF,
        "Word rejects durableId >= 0x7FFFFFFF (got {durable}): {ids}"
    );

    let extensible = part_xml(&commented, "word/commentsExtensible.xml");
    assert!(
        extensible.contains(&format!("w16cex:durableId=\"{durable}\"")),
        "commentsExtensible must repeat the commentsIds durableId {durable}: {extensible}"
    );
    assert!(
        extensible
            .contains("xmlns:w16cex=\"http://schemas.microsoft.com/office/word/2018/wordml/cex\""),
        "commentsExtensible part must declare w16cex: {extensible}"
    );
    assert!(
        !extensible.contains("mc:Ignorable"),
        "Word recovers commentsExtensible as empty if its own w16cex prefix is ignorable: {extensible}"
    );

    let types = part_xml(&commented, "[Content_Types].xml");
    for (part, content_type) in [
        (
            "/word/commentsExtended.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtended+xml",
        ),
        (
            "/word/commentsIds.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.commentsIds+xml",
        ),
        (
            "/word/commentsExtensible.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtensible+xml",
        ),
    ] {
        let needle = format!("PartName=\"{part}\" ContentType=\"{content_type}\"");
        assert!(
            types.contains(&needle),
            "expected {needle} in [Content_Types].xml: {types}"
        );
    }
    let rels = part_xml(&commented, "word/_rels/document.xml.rels");
    assert!(
        rels.contains("commentsExtensible.xml"),
        "document rels must target commentsExtensible.xml: {rels}"
    );
}

#[test]
fn comment_add_repairs_short_ms_word_content_types() {
    // docxdriver 0.3.0 registered the extended comment parts under the short
    // vnd.ms-word types, which Word reports as unreadable content.
    let source = package_with_foreign_comments();
    let types = part_xml(&source, "[Content_Types].xml").replace(
        "application/vnd.openxmlformats-officedocument.wordprocessingml.comments",
        "application/vnd.ms-word.comments",
    );
    let source = overwrite_part(&source, "[Content_Types].xml", types.as_bytes());
    let commented = commit_plan(
        &source,
        vec![EditOp::CommentAdd {
            at: ParagraphAddress::id(SOURCE_ID.to_string()),
            select: Some("text".into()),
            occurrence: None,
            text: "new comment".into(),
        }],
    );
    let types = part_xml(&commented, "[Content_Types].xml");
    for part in ["Extended", "Ids", "Extensible"] {
        assert!(
            types.contains(&format!(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.comments{part}+xml"
            )),
            "comments{part} must use the type Word writes: {types}"
        );
    }
    assert!(
        !types.contains("application/vnd.ms-word.comments"),
        "the short vnd.ms-word comment types must not remain: {types}"
    );
}

#[test]
fn body_table_numbers_are_document_wide_across_sections() {
    let source = document_tables_in_two_sections();
    let markup = read_markup(&source);
    assert!(
        markup.contains("<table data-docx-table=\"1\" data-docx-handle=\"table:1\">")
            && markup.contains("<table data-docx-table=\"2\" data-docx-handle=\"table:2\">"),
        "expected consecutive table numbers across sections: {markup}"
    );
    let after = commit_plan(
        &source,
        vec![EditOp::InsertParagraph {
            at: InsertAnchor::table(2),
            position: InsertPosition::After,
            with: "after second table".into(),
            style: None,
            alias: None,
        }],
    );
    let xml = document_xml(&after);
    assert!(
        xml.contains("after second table"),
        "expected insert after table:2: {xml}"
    );
}

fn equation_document() -> Vec<u8> {
    package_with_body(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"><w:body><w:p w14:paraId="11111111"><w:r><w:t>before</w:t></w:r><m:oMath><m:r><m:t>x</m:t></m:r></m:oMath><w:r><w:t>between</w:t></w:r><m:oMathPara><m:oMath><m:r><m:t>x</m:t></m:r></m:oMath></m:oMathPara><w:r><w:t>after</w:t></w:r></w:p></w:body></w:document>"#,
    )
}

#[test]
fn equation_markup_is_content_only_and_paragraph_context_controls_omml() {
    let html = r#"<p>Value <equation>x^{2}</equation>.</p><p><equation>\frac{a}{b}</equation></p>"#;
    let created = match execute_request(
        None,
        &command(Command::Create(CreateCommand {
            paragraphs: vec![],
            html: Some(html.into()),
        })),
    ) {
        RequestResult::Command(result) => completed_bytes(result),
        other => panic!("create failed: {other:?}"),
    };
    let markup = read_markup(&created);
    assert!(markup.contains("<msup>"), "{markup}");
    assert!(markup.contains("<mfrac>"), "{markup}");
    assert!(
        markup.contains("<math ") && !markup.contains("latex=\"") && !markup.contains("<equation")
    );
    let xml = document_xml(&created);
    assert!(xml.contains("<m:oMathPara"), "{xml}");
    let listing = read_listing(&created, "document", None);
    assert_eq!(listing["equations"][0]["display"], false);
    assert_eq!(listing["equations"][1]["display"], true);
}

#[test]
fn large_operator_operand_is_nested_in_nary_on_create_and_replace() {
    fn nary_body(xml: &str) -> String {
        let doc = xmloxide::tree::Document::parse_bytes(xml.as_bytes()).unwrap();
        let root = doc.root_element().unwrap();
        let nary = doc
            .descendants(root)
            .find(|&node| {
                doc.node_name(node)
                    .is_some_and(|name| name.rsplit(':').next() == Some("nary"))
            })
            .unwrap();
        let body = doc
            .children(nary)
            .find(|&node| {
                doc.node_name(node)
                    .is_some_and(|name| name.rsplit(':').next() == Some("e"))
            })
            .unwrap();
        doc.text_content(body)
    }

    let created = match execute_request(
        None,
        &command(Command::Create(CreateCommand {
            paragraphs: vec![],
            html: Some(r"<p><equation>\sum_{i=1}^{n} x_{i}+y</equation></p>".into()),
        })),
    ) {
        RequestResult::Command(result) => completed_bytes(result),
        other => panic!("create failed: {other:?}"),
    };
    assert_eq!(nary_body(&document_xml(&created)), "xi");

    let id = read_listing(&created, "document", None)["equations"][0]["at"]
        .as_str()
        .unwrap()
        .to_string();
    let replaced = commit_plan(
        &created,
        vec![EditOp::ReplaceEquation {
            at: ParagraphAddress::id(&id),
            equation: None,
            mathml: latex2mathml::latex_to_mathml(
                r"\prod_{k=1}^{m} (k+1)",
                latex2mathml::DisplayStyle::Inline,
            )
            .unwrap(),
            display: None,
        }],
    );
    assert_eq!(nary_body(&document_xml(&replaced)), "(k+1)");
}
fn equation_op(equation: Option<u32>, latex: &str) -> EditOp {
    EditOp::ReplaceEquation {
        at: ParagraphAddress::id(SOURCE_ID),
        equation,
        mathml: latex2mathml::latex_to_mathml(latex, latex2mathml::DisplayStyle::Inline)
            .unwrap_or_else(|_| "<math><mfrac></math>".into()),
        display: None,
    }
}
fn delete_equation_op(equation: Option<u32>) -> EditOp {
    EditOp::DeleteEquation {
        at: ParagraphAddress::id(SOURCE_ID),
        equation,
    }
}
#[test]
fn equations_are_discoverable_and_ambiguous_edits_are_rejected() {
    let bytes = equation_document();
    let canonical = read_markup(&bytes);
    assert_eq!(canonical.matches("<math ").count(), 2);
    assert!(!canonical.contains("latex=\"") && canonical.contains("<math"));
    let listing = read_listing(&bytes, "document", None);
    assert_eq!(listing["equations"][1]["equation"], 2);
    assert_eq!(listing["equations"][1]["display"], true);
    for op in [
        equation_op(None, "y"),
        equation_op(Some(3), "y"),
        equation_op(Some(1), r"\frac{"),
    ] {
        assert!(matches!(
            execute_plan(
                &bytes,
                &PlanRequest {
                    plan: plan(&bytes, vec![op]),
                    preview_key: None
                }
            ),
            PlanResult::Rejected { .. }
        ));
    }
    let edited = commit_plan(&bytes, vec![equation_op(Some(2), r"\frac{a}{b}")]);
    let listing = read_listing(&edited, "document", None);
    let eq = &listing["equations"];
    assert!(eq[0]["mathml"].as_str().unwrap().contains("<mi>x</mi>"));
    assert_eq!(eq[1]["display"], true);
    assert!(eq[1]["mathml"].as_str().unwrap().contains("mfrac"));
    let markup = read_markup(&edited);
    for text in ["before", "between", "after"] {
        assert!(markup.contains(text));
    }
}
#[test]
fn equation_replacement_tracks_and_can_be_rejected_without_losing_original_math() {
    let bytes = equation_document();
    let mut p = plan(&bytes, vec![equation_op(Some(1), "y^{2}")]);
    p.change_mode = ChangeMode::Track;
    let key = match execute_plan(
        &bytes,
        &PlanRequest {
            plan: p.clone(),
            preview_key: None,
        },
    ) {
        PlanResult::Previewed { preview_key, .. } => preview_key,
        other => panic!("{other:?}"),
    };
    let edited = match execute_plan(
        &bytes,
        &PlanRequest {
            plan: p,
            preview_key: Some(key),
        },
    ) {
        PlanResult::Committed { bytes, .. } => bytes,
        other => panic!("{other:?}"),
    };
    let listing = read_listing(&edited, "document", None);
    assert_eq!(listing["equations"].as_array().unwrap().len(), 2);
    assert_eq!(listing["equations"][0]["editable"], false);
    let restored = commit_plan(
        &edited,
        vec![EditOp::RevisionSettle {
            target: RevisionTarget::All,
            action: RevisionAction::Reject,
        }],
    );
    assert_eq!(read_markup(&restored), read_markup(&bytes));
    let accepted = commit_plan(
        &edited,
        vec![EditOp::RevisionSettle {
            target: RevisionTarget::All,
            action: RevisionAction::Accept,
        }],
    );
    let listing = read_listing(&accepted, "document", None);
    assert!(listing["equations"][0]["mathml"]
        .as_str()
        .unwrap()
        .contains('y'));
}

#[test]
fn equations_can_be_deleted_directly_or_as_recoverable_tracked_changes() {
    let bytes = equation_document();

    let direct = commit_plan(&bytes, vec![delete_equation_op(Some(1))]);
    let listing = read_listing(&direct, "document", None);
    assert_eq!(listing["equations"].as_array().unwrap().len(), 1);
    assert_eq!(listing["equations"][0]["display"], true);
    assert_eq!(listing["equations"][0]["at"], SOURCE_ID);

    let mut tracked_plan = plan(&bytes, vec![delete_equation_op(Some(1))]);
    tracked_plan.change_mode = ChangeMode::Track;
    let key = match execute_plan(
        &bytes,
        &PlanRequest {
            plan: tracked_plan.clone(),
            preview_key: None,
        },
    ) {
        PlanResult::Previewed { preview_key, .. } => preview_key,
        other => panic!("{other:?}"),
    };
    let tracked = match execute_plan(
        &bytes,
        &PlanRequest {
            plan: tracked_plan,
            preview_key: Some(key),
        },
    ) {
        PlanResult::Committed { bytes, .. } => bytes,
        other => panic!("{other:?}"),
    };
    let final_listing = read_listing(&tracked, "document", Some("final"));
    assert_eq!(final_listing["equations"].as_array().unwrap().len(), 1);
    assert!(read_markup_view(&tracked, Some("original")).contains("<math"));

    let restored = commit_plan(
        &tracked,
        vec![EditOp::RevisionSettle {
            target: RevisionTarget::All,
            action: RevisionAction::Reject,
        }],
    );
    assert_eq!(
        read_listing(&restored, "document", None)["equations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let accepted = commit_plan(
        &tracked,
        vec![EditOp::RevisionSettle {
            target: RevisionTarget::All,
            action: RevisionAction::Accept,
        }],
    );
    assert_eq!(
        read_listing(&accepted, "document", None)["equations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn deleting_sole_equation_removes_the_complete_math_object() {
    for html in [
        "<p><equation>x^{2}</equation></p>",
        "<p>Value <equation>x^{2}</equation>.</p>",
    ] {
        let source = match execute_request(
            None,
            &command(Command::Create(CreateCommand {
                paragraphs: vec![],
                html: Some(html.into()),
            })),
        ) {
            RequestResult::Command(result) => completed_bytes(result),
            other => panic!("create failed: {other:?}"),
        };
        let id = read_listing(&source, "document", None)["equations"][0]["at"]
            .as_str()
            .unwrap()
            .to_string();
        let delete = EditOp::DeleteEquation {
            at: ParagraphAddress::id(&id),
            equation: None,
        };

        let direct = commit_plan(&source, vec![delete.clone()]);
        let xml = document_xml(&direct);
        assert!(!xml.contains("<m:oMath"), "{xml}");
        assert!(!read_markup(&direct).contains("<math"));
        assert!(read_listing(&direct, "document", None)["equations"].is_null());

        let mut tracked_plan = plan(&source, vec![delete]);
        tracked_plan.change_mode = ChangeMode::Track;
        let key = match execute_plan(
            &source,
            &PlanRequest {
                plan: tracked_plan.clone(),
                preview_key: None,
            },
        ) {
            PlanResult::Previewed { preview_key, .. } => preview_key,
            other => panic!("{other:?}"),
        };
        let tracked = match execute_plan(
            &source,
            &PlanRequest {
                plan: tracked_plan,
                preview_key: Some(key),
            },
        ) {
            PlanResult::Committed { bytes, .. } => bytes,
            other => panic!("{other:?}"),
        };
        assert!(!read_markup_view(&tracked, Some("final")).contains("<math"));
        let accepted = commit_plan(
            &tracked,
            vec![EditOp::RevisionSettle {
                target: RevisionTarget::All,
                action: RevisionAction::Accept,
            }],
        );
        let xml = document_xml(&accepted);
        assert!(!xml.contains("<m:oMath"), "{xml}");
        assert!(!read_markup(&accepted).contains("<math"));
    }
}

#[test]
fn tracked_plan_allocates_unique_revision_ids_and_reports_final_markup() {
    let bytes = document(&[(SOURCE_ID, "one two three"), (SECOND_ID, "four five")]);
    let mut p = plan(
        &bytes,
        vec![
            EditOp::ReplaceText {
                at: ParagraphAddress::id(SOURCE_ID),
                select: "one".into(),
                with: "uno".into(),
                occurrence: None,
            },
            EditOp::ReplaceText {
                at: ParagraphAddress::id(SECOND_ID),
                select: "four".into(),
                with: "cuatro".into(),
                occurrence: None,
            },
            EditOp::ReplaceText {
                at: ParagraphAddress::id(SOURCE_ID),
                select: "three".into(),
                with: "tres".into(),
                occurrence: None,
            },
        ],
    );
    p.change_mode = ChangeMode::Track;
    let key = PreviewKey::new(&p);
    let PlanResult::Committed { bytes: out, report, .. } = execute_plan(
        &bytes,
        &PlanRequest {
            plan: p,
            preview_key: Some(key),
        },
    ) else {
        panic!("plan did not commit");
    };
    let xml = document_xml(&out);
    let mut ids: Vec<&str> = xml
        .split("<w:")
        .filter(|tag| tag.starts_with("ins ") || tag.starts_with("del "))
        .filter_map(|tag| tag.split("w:id=\"").nth(1)?.split('"').next())
        .collect();
    assert_eq!(ids.len(), 6, "{xml}");
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 6, "revision ids must be unique: {xml}");
    // Affected markup comes from one render of the plan's resulting state, so
    // the first edit's paragraph already shows the third edit.
    let first = serde_json::to_string(&report.ops[0].affected).unwrap();
    assert!(first.contains("uno") && first.contains("tres"), "{first}");
    assert_eq!(report.ops[0].affected, report.ops[2].affected);
}

fn edit(select: &str, with: &str) -> TextEdit {
    TextEdit {
        select: select.into(),
        with: with.into(),
        occurrence: None,
        all: false,
    }
}

fn revise(at: Option<&str>, within: Option<&str>, edits: Vec<TextEdit>) -> EditOp {
    EditOp::ReviseText {
        at: at.map(ParagraphAddress::id),
        within: within.map(str::to_string),
        occurrence: None,
        edits,
    }
}

/// Commit in the given mode, returning the final-view text of the document.
fn revise_final(bytes: &[u8], mode: ChangeMode, ops: Vec<EditOp>) -> Result<String, String> {
    let mut p = plan(bytes, ops);
    p.change_mode = mode;
    let key = PreviewKey::new(&p);
    match execute_plan(bytes, &PlanRequest { plan: p, preview_key: Some(key) }) {
        PlanResult::Committed { bytes, .. } => {
            let markup = read_markup_view(&bytes, Some("final"));
            let text = markup
                .split('<')
                .map(|piece| piece.split_once('>').map_or(piece, |(_, text)| text))
                .collect::<String>();
            Ok(text)
        }
        PlanResult::Rejected { diagnostic, .. } => Err(diagnostic.message),
        other => panic!("unexpected {other:?}"),
    }
}

const CLAUSES: &[(&str, &str)] = &[
    (SOURCE_ID, "The tenant shall pay the rent within thirty days of the notice."),
    (SECOND_ID, "The landlord shall repair the roof within thirty days of the request."),
];

#[test]
fn revise_text_scopes_short_selectors_to_a_unique_phrase_without_paragraph_ids() {
    let bytes = document(CLAUSES);
    for mode in [ChangeMode::Track, ChangeMode::Direct] {
        // "the" and "thirty" are ambiguous document-wide; the phrase is not.
        let text = revise_final(
            &bytes,
            mode,
            vec![revise(
                None,
                Some("repair the roof within thirty days"),
                vec![edit("thirty", "sixty"), edit("the roof", "the <b>roof</b> and gutters")],
            )],
        )
        .unwrap();
        assert!(
            text.contains("The landlord shall repair the roof and gutters within sixty days of the request."),
            "{mode:?}: {text}"
        );
        assert!(text.contains("pay the rent within thirty days"), "{mode:?}: {text}");
    }
}

#[test]
fn revise_text_edits_resolve_against_one_snapshot_and_apply_adjacent_spans() {
    let bytes = document(CLAUSES);
    // The second edit's select would not exist if the first applied first.
    let text = revise_final(
        &bytes,
        ChangeMode::Track,
        vec![revise(
            Some(SOURCE_ID),
            None,
            vec![edit("tenant", "lessee"), edit(" shall", " must"), edit("lessee", "never-present")],
        )],
    );
    assert!(text.unwrap_err().contains("edits[2].select \"lessee\" matched nothing"));
    let text = revise_final(
        &bytes,
        ChangeMode::Track,
        vec![revise(Some(SOURCE_ID), None, vec![edit("tenant", "lessee"), edit(" shall", " must")])],
    )
    .unwrap();
    assert!(text.contains("The lessee must pay the rent"), "{text}");
}

#[test]
fn revise_text_all_replaces_every_match_in_scope_and_document() {
    let bytes = document(CLAUSES);
    let mut all = edit("thirty", "forty-five");
    all.all = true;
    let text = revise_final(&bytes, ChangeMode::Track, vec![revise(None, None, vec![all.clone()])]).unwrap();
    assert_eq!(text.matches("forty-five days").count(), 2, "{text}");
    let mut the = edit("the", "a");
    the.all = true;
    let text = revise_final(&bytes, ChangeMode::Direct, vec![revise(Some(SECOND_ID), None, vec![the])]).unwrap();
    assert!(text.contains("repair a roof within thirty days of a request"), "{text}");
    assert!(text.contains("pay the rent"), "{text}");
}

#[test]
fn revise_text_rejects_ambiguity_overlap_and_explains_scope() {
    let bytes = document(CLAUSES);
    let error = revise_final(&bytes, ChangeMode::Track, vec![revise(None, Some("within thirty days"), vec![edit("thirty", "sixty")])])
        .unwrap_err();
    assert!(error.contains("matched 2 times in the document"), "{error}");
    assert!(error.contains(SOURCE_ID) && error.contains(SECOND_ID), "{error}");

    // "tenant" is in the paragraph but outside the scope.
    let error = revise_final(&bytes, ChangeMode::Track, vec![revise(Some(SOURCE_ID), Some("the rent within"), vec![edit("tenant", "lessee")])])
        .unwrap_err();
    assert!(error.contains("edits[0].select \"tenant\" matched nothing in within \"the rent within\""), "{error}");

    let error = revise_final(&bytes, ChangeMode::Track, vec![revise(Some(SOURCE_ID), None, vec![edit("the", "a")])]).unwrap_err();
    assert!(error.contains("matched 2 times in this paragraph"), "{error}");

    let error = revise_final(
        &bytes,
        ChangeMode::Track,
        vec![revise(Some(SOURCE_ID), None, vec![edit("pay the rent", "remit"), edit("the rent within", "rent in")])],
    )
    .unwrap_err();
    assert!(error.contains("overlapping"), "{error}");

    let mut within = revise(None, Some("within thirty days"), vec![edit("thirty", "sixty")]);
    if let EditOp::ReviseText { occurrence, .. } = &mut within {
        *occurrence = Some(2);
    }
    let text = revise_final(&bytes, ChangeMode::Track, vec![within]).unwrap();
    assert!(text.contains("pay the rent within thirty days"), "{text}");
    assert!(text.contains("repair the roof within sixty days"), "{text}");
}

#[test]
fn stale_selector_after_an_earlier_op_names_the_operations_that_changed_the_text() {
    let bytes = document(CLAUSES);
    let error = revise_final(
        &bytes,
        ChangeMode::Track,
        vec![
            EditOp::ReplaceText {
                at: ParagraphAddress::id(SOURCE_ID),
                select: "thirty days".into(),
                with: "sixty days".into(),
                occurrence: None,
            },
            EditOp::RevisionSettle {
                target: RevisionTarget::All,
                action: RevisionAction::Accept,
            },
            EditOp::FormatText {
                at: ParagraphAddress::id(SOURCE_ID),
                select: "thirty days".into(),
                occurrence: None,
                bold: Some(true),
                italic: None,
                underline: None,
                strike: None,
                superscript: None,
                subscript: None,
                highlight: None,
                color: None,
                font_size: None,
                clear: Vec::new(),
            },
        ],
    )
    .unwrap_err();
    assert!(error.contains("matched nothing"), "{error}");
    assert!(
        error.contains("ops[0] replace_text, ops[1] revision_settle changed the text first"),
        "{error}"
    );
}

/// Element order the ECMA-376 sequences require, for edits that create
/// structure (see scripts/ooxml-conformance.py for the schema gate).
#[test]
fn created_structure_follows_schema_sequence_order() {
    let body = format!(
        "<w:p w14:paraId=\"{SOURCE_ID}\"><w:pPr><w:rPr><w:b/></w:rPr></w:pPr><w:hyperlink r:id=\"rId9\"><w:r><w:t>linked</w:t></w:r></w:hyperlink><w:r><w:t> text</w:t></w:r></w:p>\
         <w:p w14:paraId=\"{SECOND_ID}\"><w:r><w:t>next</w:t></w:r></w:p>\
         <w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:docGrid w:linePitch=\"360\"/></w:sectPr>"
    );
    let bytes = package_with_body(&format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body>{body}</w:body></w:document>"
    ));
    let mut deleted = plan(&bytes, vec![EditOp::DeleteParagraphs { at: vec![ParagraphAddress::id(SOURCE_ID)] }]);
    deleted.change_mode = ChangeMode::Track;
    let key = PreviewKey::new(&deleted);
    let PlanResult::Committed { bytes: out, .. } = execute_plan(&bytes, &PlanRequest { plan: deleted, preview_key: Some(key) }) else {
        panic!("delete did not commit");
    };
    let xml = document_xml(&out);
    // The paragraph-mark deletion leads CT_ParaRPr; a hyperlink is never inside w:del.
    assert!(xml.contains("<w:rPr><w:del "), "{xml}");
    assert!(!xml.contains("<w:del w:id=\"1\" w:author=\"test-agent\" w:date=\"2000-01-01T00:00:00Z\"><w:hyperlink"), "{xml}");
    assert!(xml.contains("<w:hyperlink r:id=\"rId9\"><w:del "), "{xml}");

    let chrome = commit_plan(
        &bytes,
        vec![
            EditOp::SetPageMargins { section: None, top: Some(Points::from_points(36.0).unwrap()), right: None, bottom: None, left: None, header: None, footer: None, gutter: None },
            EditOp::SetHeader { section: None, kind: Some(ChromeKind::First), with: "<p>First</p>".into() },
            EditOp::SetFooter { section: None, kind: None, with: "<p>Footer</p>".into() },
            EditOp::SetEvenAndOddHeaders { even_and_odd: true },
        ],
    );
    let xml = document_xml(&chrome);
    let sect = &xml[xml.find("<w:sectPr").unwrap()..];
    let at = |tag: &str| sect.find(tag).unwrap_or_else(|| panic!("{tag} missing: {sect}"));
    assert!(at("<w:headerReference") < at("<w:pgSz"), "{sect}");
    assert!(at("<w:footerReference") < at("<w:pgSz"), "{sect}");
    assert!(at("<w:pgSz") < at("<w:pgMar") && at("<w:pgMar") < at("<w:titlePg"), "{sect}");
    assert!(at("<w:titlePg") < at("<w:docGrid"), "{sect}");
    for attr in ["w:top=\"720\"", "w:right=", "w:bottom=", "w:left=", "w:header=", "w:footer=", "w:gutter="] {
        assert!(sect.contains(attr), "pgMar lacks {attr}: {sect}");
    }

    let formatted = commit_plan(
        &bytes,
        vec![EditOp::FormatParagraph {
            at: ParagraphAddress::id(SOURCE_ID),
            style: None,
            alignment: Some("center".into()),
            indent_left: None,
            indent_right: None,
            first_line_indent: None,
            space_before: None,
            space_after: Some(Points::from_points(6.0).unwrap()),
            line_spacing: None,
            clear: vec![],
        }],
    );
    let xml = document_xml(&formatted);
    let ppr = &xml[xml.find("<w:pPr>").unwrap()..xml.find("</w:pPr>").unwrap()];
    let at = |tag: &str| ppr.find(tag).unwrap_or_else(|| panic!("{tag} missing: {ppr}"));
    // CT_PPr: spacing, jc, rPr, pPrChange; the snapshot is CT_PPrBase (no rPr).
    assert!(at("<w:spacing") < at("<w:jc") && at("<w:jc") < at("<w:rPr>"), "{ppr}");
    assert!(at("<w:rPr>") < at("<w:pPrChange"), "{ppr}");
    assert!(xml.contains("<w:pPrChange") && !xml.contains("<w:pPr><w:rPr><w:b/></w:rPr></w:pPr></w:pPrChange>"), "{xml}");
}

#[test]
fn signed_paragraph_indentation_is_selective_and_rejectable() {
    let body = format!("<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\"><w:body><w:p w14:paraId=\"{SOURCE_ID}\"><w:pPr><w:ind w:left=\"120\" w:firstLine=\"240\" w:leftChars=\"100\" w:start=\"80\"/><w:jc w:val=\"right\"/></w:pPr><w:r><w:t>Indent me</w:t></w:r></w:p><w:p w14:paraId=\"{SECOND_ID}\"><w:r><w:t>Neighbor</w:t></w:r></w:p><w:sectPr/></w:body></w:document>");
    let source = package_with_body(&body);
    for offset in [-18.0, 0.0, 12.0] {
        let op: EditOp = serde_json::from_value(serde_json::json!({"op":"format_paragraph", "at":SOURCE_ID,"indent_left":-6,"indent_right":-3,"first_line_indent":offset})).unwrap();
        let result = commit_plan(&source, vec![op]);
        let xml = document_xml(&result);
        let direct = xml.split("<w:pPrChange").next().unwrap();
        assert!(direct.contains("w:left=\"-120\""), "{xml}");
        assert!(direct.contains("w:right=\"-60\""), "{xml}");
        assert!(direct.contains("w:leftChars=\"100\"") && direct.contains("w:start=\"80\""), "{xml}");
        assert!(direct.contains("<w:jc w:val=\"right\""), "{xml}");
        if offset < 0.0 { assert!(direct.contains("w:hanging=\"360\"") && !direct.contains("w:firstLine="), "{xml}"); }
        else { assert!(direct.contains(&format!("w:firstLine=\"{}\"", (offset * 20.0) as i64)) && !direct.contains("w:hanging="), "{xml}"); }
        let rejected = commit_plan(&result, vec![EditOp::RevisionSettle {target:RevisionTarget::All,action:RevisionAction::Reject}]);
        let restored = document_xml(&rejected);
        assert!(restored.contains("w:firstLine=\"240\"") && restored.contains("w:left=\"120\""), "{restored}");
        assert!(restored.contains("Indent me") && restored.contains("Neighbor") && restored.contains(SECOND_ID));
        let accepted = commit_plan(&result, vec![EditOp::RevisionSettle {target:RevisionTarget::All,action:RevisionAction::Accept}]);
        let clear: EditOp = serde_json::from_value(serde_json::json!({"op":"format_paragraph","at":SOURCE_ID,"clear":["first_line_indent"]})).unwrap();
        let cleared = document_xml(&commit_plan(&accepted, vec![clear]));
        let direct = cleared.split("<w:pPrChange").next().unwrap();
        assert!(!direct.contains("w:firstLine=") && !direct.contains("w:hanging="), "{cleared}");
    }
    assert!(serde_json::from_value::<Points>(serde_json::json!(-6)).is_err());
}

#[test]
fn multilevel_list_definitions_have_isolated_formatting_and_restarts() {
    let source = document(&[(SOURCE_ID, "list item"), (SECOND_ID, "ordinary prose")]);
    let operation: EditOp = serde_json::from_value(serde_json::json!({
        "op":"set_list","at":[SOURCE_ID],"level":2,"levels":[
            {"format":"decimal","label":"%1.","start":3,"indent_left":18,"first_line_indent":-9,"marker_bold":true},
            {"format":"lowerLetter","label":"%1.%2)","restart_after":0,"marker_color":"123456"},
            {"format":"lowerRoman","label":"%1.%2.%3","restart_after":1,"marker_font":"Arial","indent_left":-6}
        ]
    })).unwrap();
    let result = commit_plan(&source, vec![operation]);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&result)).unwrap();
    let mut numbering = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("word/numbering.xml").unwrap(), &mut numbering).unwrap();
    for value in ["w:val=\"%1.%2.%3\"", "w:lvlRestart w:val=\"2\"", "w:hanging=\"180\"", "w:left=\"-120\"", "w:val=\"123456\"", "w:ascii=\"Arial\""] { assert!(numbering.contains(value), "missing {value}: {numbering}"); }
    let rejected = commit_plan(&result, vec![EditOp::RevisionSettle {target:RevisionTarget::All,action:RevisionAction::Reject}]);
    assert!(!document_xml(&rejected).contains("<w:numPr>"));
}

fn ops(values: serde_json::Value) -> Vec<EditOp> {
    serde_json::from_value(values).expect("valid ops")
}

fn ids_of(xml: &str, tag: &str) -> Vec<String> {
    let needle = format!("<w:{tag} ");
    xml.match_indices(&needle)
        .filter_map(|(at, _)| {
            let rest = &xml[at..];
            let rest = &rest[..rest.find('>').unwrap_or(rest.len())];
            let value = &rest[rest.find("w:id=\"")? + 6..];
            Some(value[..value.find('"')?].to_string())
        })
        .collect()
}

#[test]
fn comment_reply_is_anchored_beside_its_parent_and_deleted_with_the_thread() {
    let source = document(&[(SOURCE_ID, "alpha beta")]);
    let commented = commit_plan(
        &source,
        ops(serde_json::json!([{"op": "comment_add", "at": SOURCE_ID, "select": "beta", "text": "root"}])),
    );
    let root = ids_of(&document_xml(&commented), "commentReference")[0].clone();
    let replied = commit_plan(
        &commented,
        ops(serde_json::json!([{"op": "comment_reply", "comment_id": root, "text": "reply"}])),
    );
    let document = document_xml(&replied);
    let references = ids_of(&document, "commentReference");
    assert_eq!(references.len(), 2, "Word anchors every reply: {document}");
    let reply = references.iter().find(|id| **id != root).expect("reply reference").clone();
    for tag in ["commentRangeStart", "commentRangeEnd"] {
        assert_eq!(ids_of(&document, tag), vec![root.clone(), reply.clone()], "{tag}: {document}");
    }

    let deleted = commit_plan(&replied, ops(serde_json::json!([{"op": "comment_delete", "comment_id": root}])));
    let document = document_xml(&deleted);
    assert!(!document.contains("commentRange") && !document.contains("commentReference"), "{document}");
}

#[test]
fn revision_ids_are_unique_per_element() {
    let source = document(&[(SOURCE_ID, "first paragraph"), (SECOND_ID, "second paragraph")]);
    let deleted = commit_plan(&source, ops(serde_json::json!([{"op": "delete_paragraphs", "at": [SOURCE_ID]}])));
    let document = document_xml(&deleted);
    let ids = ids_of(&document, "del");
    assert!(ids.len() > 1, "expected a run deletion and a paragraph-mark deletion: {document}");
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "Word writes one revision id per element: {document}");
}

#[test]
fn run_properties_are_written_in_sequence_order() {
    let body = format!(
        "<w:p w14:paraId=\"{SOURCE_ID}\"><w:r><w:rPr><w:i/><w:lang w:val=\"en-US\"/></w:rPr><w:t>text</w:t></w:r></w:p>"
    );
    let formatted = commit_plan(
        &package_with_body(&format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\"><w:body>{body}</w:body></w:document>"
        )),
        ops(serde_json::json!([{"op": "format_text", "at": SOURCE_ID, "select": "text", "bold": true, "font_size": 14, "strike": true, "underline": true}])),
    );
    let document = document_xml(&formatted);
    let rpr = &document[document.find("<w:rPr>").expect("run properties")..];
    let order: Vec<&str> = ["<w:b", "<w:i", "<w:strike", "<w:sz", "<w:u ", "<w:lang", "<w:rPrChange"]
        .into_iter()
        .map(|tag| (tag, rpr.find(tag).unwrap_or_else(|| panic!("{tag} missing: {rpr}"))))
        .collect::<Vec<_>>()
        .windows(2)
        .filter(|pair| pair[0].1 > pair[1].1)
        .map(|pair| pair[1].0)
        .collect();
    assert!(order.is_empty(), "out of CT_RPr sequence order at {order:?}: {rpr}");
}

#[test]
fn section_boundary_replaces_existing_break_type_without_duplicate_attributes() {
    let source = package_with_body(&format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"><w:body><w:p w14:paraId="{SOURCE_ID}"><w:r><w:t>First</w:t></w:r></w:p><w:sectPr><w:type w:val="nextPage"/></w:sectPr></w:body></w:document>"#
    ));
    let insert: EditOp = serde_json::from_value(serde_json::json!({
        "op": "set_section_boundary", "at": SOURCE_ID, "break_type": "continuous"
    })).unwrap();
    let output = commit_plan(&source, vec![insert]);
    let xml = document_xml(&output);
    xmloxide::tree::Document::parse_str(&xml).expect("boundary output must be well-formed XML");
    let read = read_listing(&output, "document", None);
    assert_eq!(read["sections"][0]["break_type"], "continuous");
    assert_eq!(read["sections"][1]["break_type"], "nextPage");
}

#[test]
fn section_handles_survive_boundaries_and_merging_preserves_final_layout() {
    let source = document(&[(SOURCE_ID,"First"),(SECOND_ID,"Second")]);
    let insert:EditOp=serde_json::from_value(serde_json::json!({"op":"set_section_boundary","at":SOURCE_ID,"break_type":"continuous"})).unwrap();
    let mut request=plan(&source,vec![insert.clone()]);request.change_mode=ChangeMode::Track;
    assert!(matches!(execute_plan(&source,&PlanRequest{plan:request,preview_key:None}),PlanResult::Rejected{..}));
    let mut request=plan(&source,vec![insert]);request.change_mode=ChangeMode::Direct;
    let key=PreviewKey::new(&request);
    let PlanResult::Committed {bytes:split,..}=execute_plan(&source,&PlanRequest{plan:request,preview_key:Some(key)})else{panic!("insert failed")};
    let read=read_listing(&split,"document",None);
    assert_eq!(read["sections"].as_array().unwrap().len(),2,"{read}");
    assert_eq!(read["sections"][0]["handle"],format!("section:{SOURCE_ID}"));
    assert_eq!(read["sections"][1]["handle"],"section:final");
    let layout:EditOp=serde_json::from_value(serde_json::json!({"op":"set_section_layout","section_handle":"section:final","orientation":"landscape"})).unwrap();
    let mut request=plan(&split,vec![layout]);request.change_mode=ChangeMode::Direct;let key=PreviewKey::new(&request);
    let PlanResult::Committed {bytes:landscape,..}=execute_plan(&split,&PlanRequest{plan:request,preview_key:Some(key)})else{panic!("layout failed")};
    let remove:EditOp=serde_json::from_value(serde_json::json!({"op":"set_section_boundary","at":SOURCE_ID,"remove":true})).unwrap();
    let mut request=plan(&landscape,vec![remove]);request.change_mode=ChangeMode::Direct;let key=PreviewKey::new(&request);
    let PlanResult::Committed {bytes:merged,..}=execute_plan(&landscape,&PlanRequest{plan:request,preview_key:Some(key)})else{panic!("remove failed")};
    let xml=document_xml(&merged);assert_eq!(xml.matches("<w:sectPr").count(),1,"{xml}");
    assert!(xml.contains("w:orient=\"landscape\"")&&xml.contains("First")&&xml.contains("Second"),"{xml}");
}

#[test]
fn section_boundary_handles_follow_inserted_paragraph_aliases() {
    let source=document(&[(SOURCE_ID,"First"),(SECOND_ID,"Second")]);
    let ops:Vec<EditOp>=serde_json::from_value(serde_json::json!([
        {"op":"insert_paragraph","at":SOURCE_ID,"position":"after","with":"Middle","as":"middle"},
        {"op":"set_section_boundary","at":"$middle","break_type":"continuous"},
        {"op":"set_section_layout","section_handle":"section:$middle","columns":2}
    ])).unwrap();
    let result=commit_plan(&source,ops);
    let doc=read_listing(&result,"document",None);
    assert_eq!(doc["sections"].as_array().unwrap().len(),2,"{doc}");
    assert_eq!(doc["sections"][0]["columns"]["num"],"2","{doc}");
    assert!(doc["sections"][0]["handle"].as_str().unwrap().starts_with("section:"));
}

fn commit_auto_diff(bytes: &[u8], op: EditOp) -> Vec<u8> {
    let mut p = plan(bytes, vec![op]);
    p.change_mode = ChangeMode::Track;
    let key = match execute_plan(bytes, &PlanRequest { plan: p.clone(), preview_key: None }) {
        PlanResult::Previewed { preview_key, .. } => preview_key,
        other => panic!("auto_diff review failed: {other:?}"),
    };
    match execute_plan(bytes, &PlanRequest { plan: p, preview_key: Some(key) }) {
        PlanResult::Committed { bytes, .. } => bytes,
        other => panic!("auto_diff commit failed: {other:?}"),
    }
}

#[test]
fn auto_diff_tracks_only_changed_words_and_roundtrips_both_views() {
    for (old, new) in [
        ("Payment is due within thirty days.", "Payment is due within sixty days."),
        ("one two three four five", "one TWO three FOUR five"),
        ("hello world", "hello brave world"),
        ("hello brave world", "hello world"),
        ("hello", "hello!"),
        ("hello!", "hello"),
        ("café naïve résumé", "café wise résumé"),
        ("a a b a", "a b a a"),
        ("one  two", "one two"),
        ("same words", "same words"),
        ("delete everything", ""),
        ("", "new text"),
    ] {
        for op in [
            EditOp::ReplaceParagraph { at: ParagraphAddress::id(SOURCE_ID), with: new.into() },
            revise(Some(SOURCE_ID), None, vec![edit(old, new)]),
            EditOp::ReplaceText { at: ParagraphAddress::id(SOURCE_ID), select: old.into(), with: new.into(), occurrence: None },
        ].into_iter().take(if old.is_empty() { 1 } else { 3 }) {
            let source = document(&[(SOURCE_ID, old)]);
            let changed = commit_auto_diff(&source, op);
            for (action, expected) in [(RevisionAction::Accept, new), (RevisionAction::Reject, old)] {
                let settled = commit_plan(&changed, vec![EditOp::RevisionSettle { target: RevisionTarget::All, action }]);
                let text = read_markup_view(&settled, Some("final"));
                let text: String = text.split('<').map(|s| s.split_once('>').map_or(s, |(_, t)| t)).collect();
                assert_eq!(text.trim(), expected, "{old:?} -> {new:?}");
            }
            let xml = document_xml(&changed);
            if old == new { assert!(!xml.contains("<w:ins") && !xml.contains("<w:del")); }
            if old.contains("thirty") {
                assert_eq!(xml.matches("<w:ins ").count(), 1);
                assert_eq!(xml.matches("<w:del ").count(), 1);
                assert!(xml.contains(">thirty</w:delText>"), "{xml}");
                assert!(xml.contains(">sixty</w:t>"), "{xml}");
            }
        }
    }
}

#[test]
fn auto_diff_preserves_unchanged_run_formatting_and_supports_rich_replacements() {
    let source = document(&[(SOURCE_ID, "The bold word stays old.")]);
    let source = commit_plan(&source, vec![serde_json::from_value(serde_json::json!({
        "op":"format_text", "at":SOURCE_ID,"select":"bold", "bold":true
    })).unwrap()]);
    let changed = commit_auto_diff(&source, EditOp::ReplaceParagraph {
        at: ParagraphAddress::id(SOURCE_ID), with: "The bold word stays new.".into(),
    });
    let xml = document_xml(&changed);
    assert!(xml.contains("<w:b"));
    assert!(xml.contains(">bold</w:t>"));
    let text = revise_final(&source, ChangeMode::Track, vec![revise(Some(SOURCE_ID), None, vec![edit("old.", "<b>new.</b>")])]).unwrap();
    assert!(text.contains("stays new."), "{text}");
}

#[test]
fn auto_diff_groups_rewritten_phrases_but_keeps_unchanged_words() {
    for (old, new, groups) in [
        ("old red car", "new blue truck", 1),
        ("old, red car", "new, blue truck", 1),
        ("old red car stays here", "new blue truck stays there", 2),
    ] {
        let source = document(&[(SOURCE_ID, old)]);
        let output = commit_auto_diff(&source, EditOp::ReplaceParagraph {
            at: ParagraphAddress::id(SOURCE_ID), with: new.into(),
        });
        let xml = document_xml(&output);
        // Each contiguous changed phrase has at most one revision of each type.
        assert_eq!(xml.matches("<w:del ").count(), groups, "{xml}");
        assert_eq!(xml.matches("<w:ins ").count(), groups, "{xml}");
    }
}

// ---------------------------------------------------------------------------
// Bookmarks
// ---------------------------------------------------------------------------

const W_DOC_OPEN: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\"><w:body>";

fn body_document(body: &str) -> Vec<u8> {
    package_with_body(&format!("{W_DOC_OPEN}{body}</w:body></w:document>"))
}

/// A document whose default header holds bookmark `InHeader` (w:id 7).
fn document_with_header_bookmark(body: &str) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let document = format!(
        "{W_DOC_OPEN}{body}<w:sectPr><w:headerReference xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" w:type=\"default\" r:id=\"rId1\"/></w:sectPr></w:body></w:document>"
    );
    let parts = [
        ("[Content_Types].xml", concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
            "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
            "<Default Extension=\"xml\" ContentType=\"application/xml\"/>",
            "<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>",
            "<Override PartName=\"/word/header1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>",
            "</Types>").to_string()),
        ("_rels/.rels", concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
            "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>",
            "</Relationships>").to_string()),
        ("word/document.xml", document),
        ("word/_rels/document.xml.rels", concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
            "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/header\" Target=\"header1.xml\"/>",
            "</Relationships>").to_string()),
        ("word/header1.xml", concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            "<w:hdr xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\">",
            "<w:p w14:paraId=\"55555555\"><w:bookmarkStart w:id=\"7\" w:name=\"InHeader\"/><w:r><w:t>Running head</w:t></w:r><w:bookmarkEnd w:id=\"7\"/></w:p>",
            "</w:hdr>").to_string()),
    ];
    for (name, contents) in parts {
        writer.start_file(name, options).unwrap();
        writer.write_all(contents.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn commit_mode(bytes: &[u8], ops: Vec<EditOp>, mode: ChangeMode) -> Vec<u8> {
    let mut p = plan(bytes, ops);
    p.change_mode = mode;
    let key = match execute_plan(bytes, &PlanRequest { plan: p.clone(), preview_key: None }) {
        PlanResult::Previewed { preview_key, .. } => preview_key,
        other => panic!("preview rejected: {other:?}"),
    };
    match execute_plan(bytes, &PlanRequest { plan: p, preview_key: Some(key) }) {
        PlanResult::Committed { bytes, .. } => bytes,
        other => panic!("commit rejected: {other:?}"),
    }
}

fn rejection(bytes: &[u8], ops: Vec<EditOp>) -> docxdriver_core::Diagnostic {
    match execute_plan(bytes, &PlanRequest { plan: plan(bytes, ops), preview_key: None }) {
        PlanResult::Rejected { diagnostic, .. } => diagnostic,
        other => panic!("plan was not rejected: {other:?}"),
    }
}

fn bookmark_list(bytes: &[u8]) -> Vec<serde_json::Value> {
    let listing = read_listing(bytes, "bookmarks", None);
    assert_eq!(listing["kind"], "bookmarks");
    listing["bookmarks"].as_array().expect("bookmarks array").clone()
}

fn bookmark<'a>(list: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
    list.iter()
        .find(|b| b["name"] == name)
        .unwrap_or_else(|| panic!("no bookmark {name} in {list:?}"))
}

/// Every bookmark start has exactly one end with its id, and ids are unique.
fn assert_paired(bytes: &[u8]) {
    let xml = document_xml(bytes);
    let starts = ids_of(&xml, "bookmarkStart");
    let ends = ids_of(&xml, "bookmarkEnd");
    let mut sorted_starts = starts.clone();
    sorted_starts.sort();
    sorted_starts.dedup();
    assert_eq!(sorted_starts.len(), starts.len(), "duplicate bookmark ids: {xml}");
    let mut sorted_ends = ends.clone();
    sorted_ends.sort();
    assert_eq!(sorted_starts, sorted_ends, "unpaired bookmarks: {xml}");
}

#[test]
fn bookmark_add_wraps_selection_or_whole_paragraph_and_reads_back() {
    let source = document(&[(SOURCE_ID, "alpha beta gamma"), (SECOND_ID, "second")]);
    let out = commit_plan(
        &source,
        ops(serde_json::json!([
            {"op": "bookmark_add", "at": SOURCE_ID, "name": "Beta", "select": "beta"},
            {"op": "bookmark_add", "at": SECOND_ID, "name": "Whole"},
        ])),
    );
    let xml = document_xml(&out);
    assert!(
        xml.contains("<w:bookmarkStart w:id=\"0\" w:name=\"Beta\"/><w:r><w:t>beta</w:t></w:r><w:bookmarkEnd w:id=\"0\"/>"),
        "{xml}"
    );
    assert!(xml.contains("<w:bookmarkStart w:id=\"1\" w:name=\"Whole\"/><w:r><w:t>second</w:t></w:r><w:bookmarkEnd w:id=\"1\"/>"), "{xml}");
    assert_paired(&out);
    let list = bookmark_list(&out);
    assert_eq!(
        list[0],
        serde_json::json!({"name": "Beta", "id": "0", "hidden": false, "start": SOURCE_ID, "end": SOURCE_ID, "text": "beta", "part": "word/document.xml"})
    );
    assert_eq!(bookmark(&list, "Whole")["text"], "second");
    // The markup projection is unchanged by bookmarks.
    assert_eq!(read_markup(&out), read_markup(&source));
}

#[test]
fn bookmark_add_is_never_tracked_and_reports_its_op() {
    let source = document(&[(SOURCE_ID, "alpha beta")]);
    let p = Plan { change_mode: ChangeMode::Track, ..plan(&source, ops(serde_json::json!([
        {"op": "bookmark_add", "at": SOURCE_ID, "name": "Beta", "select": "beta", "occurrence": 1},
    ]))) };
    let report = match execute_plan(&source, &PlanRequest { plan: p, preview_key: None }) {
        PlanResult::Previewed { report, .. } => report,
        other => panic!("preview rejected: {other:?}"),
    };
    assert_eq!(report.ops[0].op, "bookmark_add");
    assert!(report.ops[0].summary.contains("bookmark \"Beta\" (id 0) added around occurrence 1 of 1"), "{}", report.ops[0].summary);
    assert_eq!(report.ops[0].affected[0]["para_id"], SOURCE_ID);
    let out = commit_mode(&source, ops(serde_json::json!([
        {"op": "bookmark_add", "at": SOURCE_ID, "name": "Beta", "select": "beta"},
    ])), ChangeMode::Track);
    let xml = document_xml(&out);
    assert!(!xml.contains("<w:ins") && !xml.contains("<w:del"), "{xml}");
}

#[test]
fn bookmark_add_targets_a_plan_local_alias() {
    let source = document(&[(SOURCE_ID, "alpha")]);
    let out = commit_plan(
        &source,
        ops(serde_json::json!([
            {"op": "insert_paragraph", "at": SOURCE_ID, "position": "after", "with": "fresh words", "as": "new"},
            {"op": "bookmark_add", "at": "$new", "name": "Fresh", "select": "words"},
        ])),
    );
    let list = bookmark_list(&out);
    assert_eq!(bookmark(&list, "Fresh")["text"], "words");
    assert_ne!(bookmark(&list, "Fresh")["start"], SOURCE_ID);
}

#[test]
fn bookmark_names_follow_word_rules_with_hints() {
    let source = body_document(&format!(
        "<w:p w14:paraId=\"{SOURCE_ID}\"><w:bookmarkStart w:id=\"3\" w:name=\"Intro\"/><w:r><w:t>alpha</w:t></w:r><w:bookmarkEnd w:id=\"3\"/><w:bookmarkStart w:id=\"4\" w:name=\"_GoBack\"/><w:bookmarkEnd w:id=\"4\"/></w:p>"
    ));
    let add = |name: &str| ops(serde_json::json!([{"op": "bookmark_add", "at": SOURCE_ID, "name": name}]));
    let invalid = rejection(&source, add("2nd"));
    assert_eq!(invalid.code, "invalid_operation");
    assert_eq!(invalid.path.as_deref(), Some("ops[0].name"));
    assert!(invalid.message.contains("starts with a digit"), "{}", invalid.message);
    assert!(rejection(&source, add("two words")).message.contains("\"two_words\""));
    assert!(rejection(&source, add(&"x".repeat(41))).message.contains("41 characters"));
    let hidden = rejection(&source, add("_Mine"));
    assert!(hidden.message.contains("hidden or reserved"), "{}", hidden.message);
    let duplicate = rejection(&source, add("INTRO"));
    assert_eq!(duplicate.code, "operation_rejected");
    assert!(duplicate.message.contains("already used by bookmark \"Intro\""), "{}", duplicate.message);
    let rename_hidden = rejection(&source, ops(serde_json::json!([{"op": "bookmark_rename", "name": "Intro", "new_name": "_x"}])));
    assert_eq!(rename_hidden.path.as_deref(), Some("ops[0].new_name"));
    let unknown = rejection(&source, ops(serde_json::json!([{"op": "bookmark_delete", "name": "Intor"}])));
    assert!(unknown.message.contains("no bookmark named \"Intor\"; close matches: \"Intro\""), "{}", unknown.message);
    assert!(!unknown.message.contains("_GoBack"), "{}", unknown.message);
    // A case-only rename of the same bookmark is fine; hidden ones may be
    // renamed from, moved and deleted.
    let out = commit_plan(&source, ops(serde_json::json!([
        {"op": "bookmark_rename", "name": "intro", "new_name": "INTRO"},
        {"op": "bookmark_rename", "name": "_GoBack", "new_name": "Back"},
        {"op": "bookmark_move", "name": "Back", "at": SOURCE_ID, "select": "lph"},
    ])));
    let list = bookmark_list(&out);
    assert_eq!(bookmark(&list, "INTRO")["id"], "3");
    assert_eq!(bookmark(&list, "Back")["id"], "4");
    assert_eq!(bookmark(&list, "Back")["text"], "lph");
    assert_paired(&out);
    let hidden_source = body_document(&format!(
        "<w:p w14:paraId=\"{SOURCE_ID}\"><w:bookmarkStart w:id=\"4\" w:name=\"_GoBack\"/><w:bookmarkEnd w:id=\"4\"/><w:r><w:t>alpha</w:t></w:r></w:p>"
    ));
    let list = bookmark_list(&hidden_source);
    assert_eq!(list[0]["hidden"], true);
    assert_eq!(list[0]["text"], "");
    let out = commit_plan(&hidden_source, ops(serde_json::json!([{"op": "bookmark_delete", "name": "_GoBack"}])));
    assert!(!document_xml(&out).contains("bookmark"));
}

#[test]
fn bookmark_move_keeps_name_and_id_and_delete_keeps_text() {
    let source = body_document(&format!(
        "<w:p w14:paraId=\"{SOURCE_ID}\"><w:r><w:t xml:space=\"preserve\">one </w:t></w:r><w:bookmarkStart w:id=\"9\" w:name=\"Span\"/><w:r><w:t>two</w:t></w:r></w:p>\
         <w:p w14:paraId=\"{SECOND_ID}\"><w:r><w:t>three four</w:t></w:r><w:bookmarkEnd w:id=\"9\"/></w:p>"
    ));
    let list = bookmark_list(&source);
    assert_eq!(list[0]["start"], SOURCE_ID);
    assert_eq!(list[0]["end"], SECOND_ID);
    assert_eq!(list[0]["text"], "two\nthree four");
    let moved = commit_plan(&source, ops(serde_json::json!([
        {"op": "bookmark_move", "name": "span", "at": SECOND_ID, "select": "four"},
    ])));
    assert_paired(&moved);
    let list = bookmark_list(&moved);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "Span");
    assert_eq!(list[0]["id"], "9");
    assert_eq!(list[0]["start"], SECOND_ID);
    assert_eq!(list[0]["text"], "four");
    let deleted = commit_plan(&moved, ops(serde_json::json!([{"op": "bookmark_delete", "name": "Span"}])));
    assert!(!document_xml(&deleted).contains("bookmark"));
    assert_eq!(read_markup(&deleted), read_markup(&source));
}

#[test]
fn header_bookmarks_are_read_and_refused_for_editing() {
    let source = document_with_header_bookmark(&format!(
        "<w:p w14:paraId=\"{SOURCE_ID}\"><w:r><w:t>alpha</w:t></w:r></w:p>"
    ));
    let list = bookmark_list(&source);
    assert_eq!(
        list[0],
        serde_json::json!({"name": "InHeader", "id": "7", "hidden": false, "start": "55555555", "end": "55555555", "text": "Running head", "part": "word/header1.xml"})
    );
    let refused = rejection(&source, ops(serde_json::json!([{"op": "bookmark_delete", "name": "InHeader"}])));
    assert!(refused.message.contains("is in word/header1.xml; bookmark editing is body-only"), "{}", refused.message);
    let duplicate = rejection(&source, ops(serde_json::json!([{"op": "bookmark_add", "at": SOURCE_ID, "name": "inheader"}])));
    assert!(duplicate.message.contains("in word/header1.xml"), "{}", duplicate.message);
    // New ids clear every story's ids.
    let out = commit_plan(&source, ops(serde_json::json!([{"op": "bookmark_add", "at": SOURCE_ID, "name": "Body"}])));
    assert_eq!(bookmark(&bookmark_list(&out), "Body")["id"], "8");
}

#[test]
fn bookmark_markers_climb_out_of_wrappers_only_when_the_range_is_unchanged() {
    let source = body_document(&format!(
        "<w:p w14:paraId=\"{SOURCE_ID}\"><w:hyperlink w:anchor=\"x\"><w:r><w:t>link text</w:t></w:r></w:hyperlink><w:r><w:t xml:space=\"preserve\"> after</w:t></w:r></w:p>"
    ));
    let out = commit_plan(&source, ops(serde_json::json!([
        {"op": "bookmark_add", "at": SOURCE_ID, "name": "Whole_link", "select": "link text"},
        {"op": "bookmark_add", "at": SOURCE_ID, "name": "Inner", "select": "text"},
    ])));
    let xml = document_xml(&out);
    assert!(xml.contains("<w:bookmarkStart w:id=\"0\" w:name=\"Whole_link\"/><w:hyperlink"), "{xml}");
    assert!(xml.contains("</w:hyperlink><w:bookmarkEnd w:id=\"1\"/><w:bookmarkEnd w:id=\"0\"/>"), "{xml}");
    // The inner start stays in the hyperlink (text precedes it there); its
    // end climbs out, since nothing follows the run inside the hyperlink.
    assert!(xml.contains("<w:bookmarkStart w:id=\"1\" w:name=\"Inner\"/><w:r><w:t>text</w:t></w:r></w:hyperlink><w:bookmarkEnd w:id=\"1\"/>"), "{xml}");
    let list = bookmark_list(&out);
    assert_eq!(bookmark(&list, "Inner")["text"], "text");
    assert_eq!(bookmark(&list, "Whole_link")["text"], "link text");
    assert_paired(&out);
}

#[test]
fn existing_edits_keep_bookmarks_paired() {
    let marked = |body: &str| body_document(body);
    let one = format!(
        "<w:p w14:paraId=\"{SOURCE_ID}\"><w:r><w:t xml:space=\"preserve\">alpha </w:t></w:r><w:bookmarkStart w:id=\"1\" w:name=\"Mid\"/><w:r><w:t>beta</w:t></w:r><w:bookmarkEnd w:id=\"1\"/><w:r><w:t xml:space=\"preserve\"> gamma</w:t></w:r></w:p>\
         <w:p w14:paraId=\"{SECOND_ID}\"><w:r><w:t>second</w:t></w:r></w:p>"
    );
    let cross = format!(
        "<w:p w14:paraId=\"{SOURCE_ID}\"><w:r><w:t xml:space=\"preserve\">alpha </w:t></w:r><w:bookmarkStart w:id=\"1\" w:name=\"Mid\"/><w:r><w:t>beta</w:t></w:r></w:p>\
         <w:p w14:paraId=\"{SECOND_ID}\"><w:r><w:t>second</w:t></w:r><w:bookmarkEnd w:id=\"1\"/></w:p>\
         <w:p w14:paraId=\"33333333\"><w:r><w:t>third</w:t></w:r></w:p>"
    );
    let cases = [
        serde_json::json!({"op": "replace_text", "at": SOURCE_ID, "select": "beta", "with": "BETA"}),
        serde_json::json!({"op": "replace_text", "at": SOURCE_ID, "select": "alpha beta", "with": "all new"}),
        serde_json::json!({"op": "replace_text", "at": SOURCE_ID, "select": "a b", "with": "a <i>b</i>"}),
        serde_json::json!({"op": "revise_text", "at": SOURCE_ID, "edits": [{"select": "a b", "with": "a-b"}, {"select": "lph", "with": "LPH"}]}),
        serde_json::json!({"op": "replace_paragraph", "at": SOURCE_ID, "with": "Replaced <b>text</b>"}),
        serde_json::json!({"op": "delete_paragraphs", "at": [SOURCE_ID]}),
    ];
    for body in [&one, &cross] {
        for case in &cases {
            for mode in [ChangeMode::Track, ChangeMode::Direct] {
                let source = marked(body);
                let out = commit_mode(&source, ops(serde_json::json!([case])), mode);
                assert_paired(&out);
                let list = bookmark_list(&out);
                assert!(!list.is_empty(), "{case} {mode:?} lost the bookmark: {}", document_xml(&out));
                let mid = bookmark(&list, "Mid");
                assert!(mid["start"].is_string() && mid["end"].is_string(), "{case} {mode:?}: {mid}");
                // The bookmark remains editable after the edit.
                let moved = commit_mode(&out, ops(serde_json::json!([{"op": "bookmark_move", "name": "Mid", "at": SECOND_ID}])), mode);
                assert_paired(&moved);
                assert_eq!(bookmark(&bookmark_list(&moved), "Mid")["text"], "second");
            }
        }
        // Deleting every bookmarked paragraph and accepting keeps the pair.
        let source = marked(body);
        let out = commit_mode(&source, ops(serde_json::json!([
            {"op": "delete_paragraphs", "at": [SOURCE_ID, SECOND_ID]},
            {"op": "revision_settle", "target": "all", "action": "accept"},
        ])), ChangeMode::Track);
        assert_paired(&out);
    }
}

fn preview_plan(bytes: &[u8], ops: Vec<EditOp>) -> PlanResult {
    execute_plan(bytes, &PlanRequest { plan: plan(bytes, ops), preview_key: None })
}

#[test]
fn insert_table_cells_fill_in_one_plan_and_bind_cell_aliases() {
    let source = document(&[(SOURCE_ID, "first"), (SECOND_ID, "last")]);
    let table = ops(serde_json::json!([
        {"op": "insert_table", "at": SOURCE_ID, "position": "after", "rows": 3, "columns": 2, "as": "t",
         "cells": [["<b>Name</b>", "<i>Role</i>"], ["Ada", ""], ["Grace"]]},
        {"op": "replace_paragraph", "at": "$t_r2c2", "with": "Analyst"},
        {"op": "insert_paragraph", "at": "$t_r3c1", "position": "after", "with": "Hopper"},
    ]));
    let report = match preview_plan(&source, table.clone()) {
        PlanResult::Previewed { report, .. } => report,
        other => panic!("preview rejected: {other:?}"),
    };
    assert_eq!(report.ops[0].summary, "inserted table table:1 with 4 filled cells");
    let cell = |name: &str| report.aliases[name].as_str().unwrap().to_string();
    let ids: Vec<_> = ["t_r1c1", "t_r1c2", "t_r2c1", "t_r2c2", "t_r3c1", "t_r3c2"].iter().map(|n| cell(n)).collect();
    // Six cell ids, then the inserted paragraph's.
    assert_eq!(report.allocated_ids.len(), 6, "{report:?}");
    assert_eq!(ids, report.allocated_ids);
    assert_eq!(report.aliases["t"], "$t");

    let out = commit_plan(&source, table.clone());
    // The same plan allocates the same ids at commit.
    let xml = document_xml(&out);
    for id in &ids {
        assert!(xml.contains(&format!("w14:paraId=\"{id}\"")), "{id} missing: {xml}");
    }
    let markup = read_markup(&out);
    for text in ["<b>Name</b>", "<i>Role</i>", ">Ada<", ">Analyst<", ">Grace<", ">Hopper<"] {
        assert!(markup.contains(text), "{text} missing: {markup}");
    }
    // Committing twice is deterministic.
    assert_eq!(commit_plan(&source, table), out);
}

#[test]
fn insert_table_without_cells_still_allocates_cell_ids() {
    let source = document(&[(SOURCE_ID, "first")]);
    let table = ops(serde_json::json!([
        {"op": "insert_table", "at": SOURCE_ID, "position": "before", "rows": 2, "columns": 2},
    ]));
    let report = match preview_plan(&source, table.clone()) {
        PlanResult::Previewed { report, .. } => report,
        other => panic!("preview rejected: {other:?}"),
    };
    assert_eq!(report.ops[0].summary, "inserted table table:1");
    assert_eq!(report.allocated_ids.len(), 4);
    assert!(report.aliases.is_empty());
    // Ids depend on the whole plan; the committed plan keeps the previewed ids.
    let xml = document_xml(&commit_plan(&source, table));
    for id in &report.allocated_ids {
        assert!(xml.contains(&format!("w14:paraId=\"{id}\"")), "{id} missing: {xml}");
    }
}

#[test]
fn insert_table_cells_reject_extra_rows_columns_and_bad_markup() {
    let source = document(&[(SOURCE_ID, "first")]);
    let rejected = |value: serde_json::Value| match preview_plan(&source, ops(serde_json::json!([value]))) {
        PlanResult::Rejected { diagnostic, .. } => diagnostic.message,
        other => panic!("expected rejection, got {other:?}"),
    };
    let rows = rejected(serde_json::json!({"op": "insert_table", "at": SOURCE_ID, "position": "after", "rows": 1, "columns": 2, "cells": [["a"], ["b"], ["c"]]}));
    assert!(rows.contains("cells has 3 rows but the table has 1") && rows.contains("row 2 to row 3"), "{rows}");
    let columns = rejected(serde_json::json!({"op": "insert_table", "at": SOURCE_ID, "position": "after", "rows": 2, "columns": 2, "cells": [["a", "b"], ["c", "d", "e"]]}));
    assert!(columns.contains("cells[1] (row 2) has 3 cells but the table has 2 columns") && columns.contains("(column 3)"), "{columns}");
    let markup = rejected(serde_json::json!({"op": "insert_table", "at": SOURCE_ID, "position": "after", "rows": 1, "columns": 2, "cells": [["a", "<table>x</table>"]]}));
    assert!(markup.contains("cells[0][1]"), "{markup}");
    // Tracked insertion stays unsupported, filled or not.
    let mut tracked = plan(&source, ops(serde_json::json!([
        {"op": "insert_table", "at": SOURCE_ID, "position": "after", "rows": 1, "columns": 1, "cells": [["a"]]},
    ])));
    tracked.change_mode = ChangeMode::Track;
    match execute_plan(&source, &PlanRequest { plan: tracked, preview_key: None }) {
        PlanResult::Rejected { diagnostic, .. } => assert!(diagnostic.message.contains("tracked table insertion"), "{}", diagnostic.message),
        other => panic!("expected rejection, got {other:?}"),
    }
}

#[test]
fn insert_table_cell_aliases_collide_with_existing_aliases() {
    let source = document(&[(SOURCE_ID, "first")]);
    match preview_plan(&source, ops(serde_json::json!([
        {"op": "insert_paragraph", "at": SOURCE_ID, "position": "after", "with": "x", "as": "t_r1c1"},
        {"op": "insert_table", "at": SOURCE_ID, "position": "after", "rows": 1, "columns": 1, "as": "t"},
    ]))) {
        PlanResult::Rejected { diagnostic, .. } => assert_eq!(diagnostic.code, "duplicate_alias"),
        other => panic!("expected rejection, got {other:?}"),
    }
}

#[test]
fn insert_table_link_cell_declares_the_relationships_prefix() {
    // `document` declares only w and w14, as some producers do.
    let source = document(&[(SOURCE_ID, "first")]);
    assert!(!document_xml(&source).contains("xmlns:r="));
    let out = commit_plan(&source, ops(serde_json::json!([
        {"op": "insert_table", "at": SOURCE_ID, "position": "after", "rows": 1, "columns": 1,
         "cells": [["<a href=\"https://example.com/\">site</a>"]]},
    ])));
    let xml = document_xml(&out);
    assert!(xml.contains("xmlns:r=") && xml.contains("<w:hyperlink r:id="), "{xml}");
}
