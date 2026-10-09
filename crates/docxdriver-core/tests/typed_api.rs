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
    SourceHash,
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
            color: Some("#AABBCC".into()),
            font_size: Some(Points::from_points(12.5).unwrap()),
            clear: vec!["italic".into()],
        },
        EditOp::FormatParagraph {
            at: id.clone(),
            style: Some("Normal".into()),
            alignment: Some("justify".into()),
            indent_left: Some(Points::from_points(1.25).unwrap()),
            indent_right: None,
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
    assert!(markup.contains("<table data-docx-table=\"1\">"));
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
            "application/vnd.ms-word.commentsExtended+xml",
        ),
        (
            "/word/commentsIds.xml",
            "application/vnd.ms-word.commentsIds+xml",
        ),
        (
            "/word/commentsExtensible.xml",
            "application/vnd.ms-word.commentsExtensible+xml",
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
fn comment_add_repairs_legacy_extended_content_types() {
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
    let types = part_xml(&commented, "[Content_Types].xml");
    assert!(
        types.contains("application/vnd.ms-word.commentsExtended+xml"),
        "legacy openxmlformats commentsExtended content type must be rewritten: {types}"
    );
    assert!(
        !types.contains(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.commentsExtended+xml"
        ),
        "the long openxmlformats commentsExtended type must not remain: {types}"
    );
}

#[test]
fn body_table_numbers_are_document_wide_across_sections() {
    let source = document_tables_in_two_sections();
    let markup = read_markup(&source);
    assert!(
        markup.contains("<table data-docx-table=\"1\">")
            && markup.contains("<table data-docx-table=\"2\">"),
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
