//! The OPC package discovers its main part through package relationships; the
//! conventional `word/document.xml` name is not a contract.

use std::io::{Read, Write};

use docxdriver_core::{
    execute_plan, execute_request, ChangeMode, Command, CommandRequest, CommandResult, EditOp,
    ParagraphAddress, Plan, PlanRequest, PlanResult, ReadCommand, Request, RequestResult,
    SourceHash,
};

fn renamed_main_part() -> Vec<u8> {
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
                "<Override PartName=\"/word/trial.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>",
                "</Types>"
            ),
        ),
        (
            "_rels/.rels",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
                "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/trial.xml\"/>",
                "</Relationships>"
            ),
        ),
        (
            "word/trial.xml",
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:w14=\"http://schemas.microsoft.com/office/word/2010/wordml\">",
                "<w:body><w:p w14:paraId=\"11111111\"><w:r><w:t>trial text</w:t></w:r></w:p></w:body></w:document>"
            ),
        ),
        (
            "word/_rels/trial.xml.rels",
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>",
        ),
    ];
    for (name, content) in parts {
        writer.start_file(name, options).unwrap();
        writer.write_all(content.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn read_document(bytes: &[u8]) -> serde_json::Value {
    match execute_request(
        Some(bytes),
        &Request::Command(CommandRequest {
            command: Command::Read(ReadCommand {
                read_kind: Some("document_ui".into()),
                view: None,
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

#[test]
fn main_part_resolves_via_package_rels_for_typed_read_and_plan_write() {
    let bytes = renamed_main_part();
    let value = read_document(&bytes);
    assert!(value["html"].as_str().unwrap().contains("trial text"));
    let id = value["paragraphs"][0]["id"].as_str().unwrap_or("11111111");

    let p = Plan {
        base: SourceHash::from_bytes(&bytes),
        author: "typed-test".into(),
        change_mode: ChangeMode::Direct,
        ops: vec![EditOp::ReplaceParagraph {
            at: ParagraphAddress::id(id),
            with: "real text".into(),
        }],
    };
    let preview = execute_plan(
        &bytes,
        &PlanRequest {
            plan: p.clone(),
            preview_key: None,
        },
    );
    let key = match preview {
        PlanResult::Previewed { preview_key, .. } => preview_key,
        other => panic!("typed plan preview failed: {other:?}"),
    };
    let out = match execute_plan(
        &bytes,
        &PlanRequest {
            plan: p,
            preview_key: Some(key),
        },
    ) {
        PlanResult::Committed { bytes, .. } => bytes,
        other => panic!("typed plan commit failed: {other:?}"),
    };
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&out)).unwrap();
    assert!(archive.by_name("word/document.xml").is_err());
    let mut xml = String::new();
    archive
        .by_name("word/trial.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("real text"), "{xml}");
    assert!(read_document(&out)["html"]
        .as_str()
        .unwrap()
        .contains("real text"));
}
