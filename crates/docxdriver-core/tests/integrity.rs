//! The commit gate: an edit may not introduce a fault its input lacked, and
//! faults already in the input never block an edit.

use std::io::{Read, Write};

use docxdriver_core::integrity::{check, gate};
use docxdriver_core::{
    execute_plan, ChangeMode, EditOp, ParagraphAddress, Plan, PlanRequest, PlanResult, SourceHash,
};

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W14: &str = "http://schemas.microsoft.com/office/word/2010/wordml";

fn package(body: &str, extra_attrs: &str) -> Vec<u8> {
    package_ns(W, body, extra_attrs)
}

fn package_ns(w: &str, body: &str, extra_attrs: &str) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    let parts = [
        (
            "[Content_Types].xml".to_string(),
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
                "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
                "<Default Extension=\"xml\" ContentType=\"application/xml\"/>",
                "<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>",
                "</Types>"
            )
            .to_string(),
        ),
        (
            "_rels/.rels".to_string(),
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
                "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>",
                "</Relationships>"
            )
            .to_string(),
        ),
        (
            "word/document.xml".to_string(),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"{w}\" xmlns:w14=\"{W14}\"{extra_attrs}><w:body>{body}</w:body></w:document>"
            ),
        ),
    ];
    for (name, content) in parts {
        writer.start_file(name, options).unwrap();
        writer.write_all(content.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn messages(bytes: &[u8]) -> Vec<String> {
    check(bytes).kinds().into_iter().map(|(_, message)| message).collect()
}

fn document_xml(bytes: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    archive.by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml
}

fn commit(bytes: &[u8], ops: Vec<EditOp>) -> Result<Vec<u8>, String> {
    let plan = Plan {
        base: SourceHash::from_bytes(bytes),
        author: "gate-test".into(),
        change_mode: ChangeMode::Track,
        ops,
    };
    let key = match execute_plan(bytes, &PlanRequest { plan: plan.clone(), preview_key: None }) {
        PlanResult::Previewed { preview_key, .. } => preview_key,
        PlanResult::Rejected { diagnostic, .. } => return Err(format!("{}: {}", diagnostic.code, diagnostic.message)),
        other => panic!("unexpected preview {other:?}"),
    };
    match execute_plan(bytes, &PlanRequest { plan, preview_key: Some(key) }) {
        PlanResult::Committed { bytes, .. } => Ok(bytes),
        PlanResult::Rejected { diagnostic, .. } => Err(format!("{}: {}", diagnostic.code, diagnostic.message)),
        other => panic!("unexpected commit {other:?}"),
    }
}

const PARAGRAPH: &str = "<w:p w14:paraId=\"11111111\"><w:r><w:t>Rent is due in thirty days.</w:t></w:r></w:p>";

#[test]
fn a_clean_package_has_no_findings() {
    assert_eq!(messages(&package(PARAGRAPH, "")), Vec::<String>::new());
}

#[test]
fn gate_refuses_an_introduced_fault_and_names_it() {
    let input = package(PARAGRAPH, "");
    let broken = package(
        "<w:p w14:paraId=\"11111111\"><w:r><w:fldChar w:fldCharType=\"begin\"/><w:t>x</w:t></w:r></w:p>",
        "",
    );
    let failure = gate(&input, &broken).unwrap_err();
    assert_eq!(
        failure.introduced,
        [("word/document.xml".to_string(), "integrity: fldChar begin without an end".to_string())]
    );
    assert!(failure.summary().contains("nothing was written"), "{}", failure.summary());
    assert!(gate(&input, &input).is_ok());
}

#[test]
fn faults_already_in_the_source_do_not_block_edits() {
    // Unbalanced bookmark, an unknown w: element, and a field code without
    // significant-space preservation: all pre-existing, all left alone.
    let body = concat!(
        "<w:p w14:paraId=\"11111111\"><w:bookmarkStart w:id=\"1\" w:name=\"open\"/><w:pPr><w:bogus/></w:pPr>",
        "<w:r><w:t>Rent is due in thirty days.</w:t></w:r></w:p>",
        "<w:p w14:paraId=\"22222222\"><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r>",
        "<w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>"
    );
    let input = package(body, "");
    let before = messages(&input);
    assert!(before.iter().any(|m| m.contains("bookmarkStart without a bookmarkEnd")), "{before:?}");
    assert!(before.iter().any(|m| m.contains("w:bogus is not allowed")), "{before:?}");
    let out = commit(
        &input,
        vec![EditOp::ReplaceText {
            at: ParagraphAddress::id("11111111"),
            select: "thirty".into(),
            occurrence: None,
            with: "sixty".into(),
        }],
    )
    .expect("pre-existing faults must not block the edit");
    assert!(document_xml(&out).contains("sixty"));
    assert_eq!(messages(&out), before);
}

#[test]
fn literal_line_breaks_in_attributes_survive_a_rewrite() {
    // XML reads a literal CRLF in an attribute as one space. xmloxide read it
    // as two; the engine normalizes line ends first, so the rewrite of this
    // part leaves the attribute as any conforming reader saw it.
    let body = format!("{PARAGRAPH}<w:p w14:paraId=\"22222222\"><w:r><w:t>keep</w:t></w:r></w:p>");
    let input = package(&body, " w:conformance=\"a\r\nb\"");
    let out = commit(
        &input,
        vec![EditOp::ReplaceText {
            at: ParagraphAddress::id("11111111"),
            select: "thirty".into(),
            occurrence: None,
            with: "sixty".into(),
        }],
    )
    .unwrap();
    assert!(document_xml(&out).contains("w:conformance=\"a b\""), "{}", document_xml(&out));
}

#[test]
fn package_and_xml_faults_are_reported() {
    let doctype = package(PARAGRAPH, "").to_vec();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&doctype)).unwrap();
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();
        let name = file.name().to_string();
        if name == "word/document.xml" {
            content = content.replace("<?xml version=\"1.0\" encoding=\"UTF-8\"?>", "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE w:document>");
        }
        writer.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        writer.write_all(content.as_bytes()).unwrap();
    }
    writer.start_file("word/media/orphan.bin", zip::write::SimpleFileOptions::default()).unwrap();
    writer.write_all(b"x").unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let found = messages(&bytes);
    assert!(found.contains(&"xml: contains a DTD".to_string()), "{found:?}");
    assert!(found.contains(&"integrity: part has no content type".to_string()), "{found:?}");
    assert_eq!(messages(b"not a zip"), ["package: not a readable zip archive"]);
}

#[test]
fn story_rules() {
    let cases = [
        ("<w:p><w:r><w:t> lead</w:t></w:r></w:p>", "integrity: run text has leading or trailing space without xml:space=\"preserve\""),
        ("<w:p><w:r><w:delText>x</w:delText></w:r></w:p>", "integrity: w:delText outside a deletion"),
        ("<w:p><w:r><w:instrText>PAGE</w:instrText></w:r></w:p>", "integrity: instrText outside a field code"),
        ("<w:tbl><w:tblPr/><w:tblGrid/></w:tbl>", "integrity: table has no rows"),
        ("<w:p><w:bookmarkStart w:id=\"1\" w:name=\"a\"/><w:bookmarkEnd w:id=\"1\"/><w:bookmarkStart w:id=\"2\" w:name=\"a\"/><w:bookmarkEnd w:id=\"2\"/></w:p>", "integrity: duplicate bookmark name"),
        ("<w:p w14:paraId=\"90000000\"/>", "integrity: w14:paraId is not eight hex digits below 80000000"),
        ("<w:p><w:pPr><w:jc w:val=\"justify\"/></w:pPr></w:p>", "schema: w:jc/@w:val is not a valid ST_Jc"),
        ("<w:sectPr/><w:p/>", "schema: w:p is out of order in w:body"),
    ];
    for (body, expected) in cases {
        let found = messages(&package(body, ""));
        assert!(found.contains(&expected.to_string()), "{body}: {found:?}");
    }
}

#[test]
fn justify_is_written_as_the_schema_value() {
    let out = commit(
        &package(PARAGRAPH, ""),
        vec![EditOp::FormatParagraph {
            at: ParagraphAddress::id("11111111"),
            style: None,
            alignment: Some("justify".into()),
            indent_left: None,
            indent_right: None,
            first_line_indent: None,
            space_before: None,
            space_after: None,
            line_spacing: None,
            clear: Vec::new(),
        }],
    )
    .unwrap();
    assert!(document_xml(&out).contains("<w:jc w:val=\"both\"/>"), "{}", document_xml(&out));
}

fn commit_mode(bytes: &[u8], mode: ChangeMode, ops: Vec<EditOp>) -> Vec<u8> {
    let plan = Plan { base: SourceHash::from_bytes(bytes), author: "gate-test".into(), change_mode: mode, ops };
    let key = match execute_plan(bytes, &PlanRequest { plan: plan.clone(), preview_key: None }) {
        PlanResult::Previewed { preview_key, .. } => preview_key,
        other => panic!("preview rejected: {other:?}"),
    };
    match execute_plan(bytes, &PlanRequest { plan, preview_key: Some(key) }) {
        PlanResult::Committed { bytes, .. } => bytes,
        other => panic!("commit rejected: {other:?}"),
    }
}

/// Text in document order, `|` for breaks, `~text~` for deleted text.
fn reading(bytes: &[u8]) -> String {
    let xml = document_xml(bytes);
    let mut out = String::new();
    let mut rest = xml.as_str();
    while let Some(start) = rest.find('<') {
        let end = rest[start..].find('>').unwrap() + start;
        let tag = &rest[start + 1..end];
        let after = &rest[end + 1..];
        let text = &after[..after.find('<').unwrap_or(after.len())];
        if tag.starts_with("w:t") && !tag.starts_with("w:tbl") {
            out.push_str(text);
        } else if tag.starts_with("w:delText") {
            out.push_str(&format!("~{text}~"));
        } else if tag.starts_with("w:br") {
            out.push('|');
        }
        rest = after;
    }
    out
}

#[test]
fn editing_a_run_with_several_leaves_keeps_their_order() {
    // Word writes line breaks inside one run between two w:t leaves.
    let body = "<w:p w14:paraId=\"11111111\"><w:r><w:rPr><w:b/></w:rPr><w:t>Lorem ipsum dolor.</w:t><w:br/><w:t>Second line.</w:t></w:r></w:p>";
    let replace = || vec![EditOp::ReplaceText {
        at: ParagraphAddress::id("11111111"),
        select: "Lorem".into(),
        occurrence: None,
        with: "Hello".into(),
    }];
    let direct = commit_mode(&package(body, ""), ChangeMode::Direct, replace());
    assert_eq!(reading(&direct), "Hello ipsum dolor.|Second line.");
    let tracked = commit_mode(&package(body, ""), ChangeMode::Track, replace());
    assert_eq!(reading(&tracked), "~Lorem~Hello ipsum dolor.|Second line.");
    // Selecting a whole leaf leaves the break and the next leaf alone.
    let whole = commit_mode(
        &package(body, ""),
        ChangeMode::Track,
        vec![EditOp::ReplaceText {
            at: ParagraphAddress::id("11111111"),
            select: "Second line.".into(),
            occurrence: None,
            with: "Other.".into(),
        }],
    );
    // The diff keeps the shared trailing period.
    assert_eq!(reading(&whole), "Lorem ipsum dolor.|~Second line~Other.");
    assert!(document_xml(&whole).matches("<w:b/>").count() >= 3, "formatting kept on every split run");
}

#[test]
fn tracked_deletion_marks_field_codes_deleted_and_rejection_restores_them() {
    let body = concat!(
        "<w:p w14:paraId=\"11111111\"><w:r><w:t xml:space=\"preserve\">See page </w:t></w:r>",
        "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> PAGE </w:instrText></w:r>",
        "<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>",
        "<w:p w14:paraId=\"22222222\"><w:r><w:t>Next.</w:t></w:r></w:p>"
    );
    let deleted = commit_mode(
        &package(body, ""),
        ChangeMode::Track,
        vec![EditOp::DeleteParagraphs { at: vec![ParagraphAddress::id("11111111")] }],
    );
    let xml = document_xml(&deleted);
    assert!(xml.contains("<w:delInstrText xml:space=\"preserve\"> PAGE </w:delInstrText>"), "{xml}");
    assert!(!xml.contains("<w:instrText"), "{xml}");
    let restored = commit_mode(
        &deleted,
        ChangeMode::Direct,
        vec![EditOp::RevisionSettle {
            target: docxdriver_core::RevisionTarget::All,
            action: docxdriver_core::RevisionAction::Reject,
        }],
    );
    let xml = document_xml(&restored);
    assert!(xml.contains("<w:instrText xml:space=\"preserve\"> PAGE </w:instrText>"), "{xml}");
    assert!(!xml.contains("delInstrText") && !xml.contains("delText"), "{xml}");
}

#[test]
fn tracked_deletion_leaves_text_box_stories_alone() {
    let body = concat!(
        "<w:p w14:paraId=\"11111111\"><w:r><w:t>Anchor</w:t></w:r><w:r><w:pict><v:shape xmlns:v=\"urn:schemas-microsoft-com:vml\"><v:textbox>",
        "<w:txbxContent><w:p><w:r><w:t>Inside the box</w:t></w:r></w:p></w:txbxContent>",
        "</v:textbox></v:shape></w:pict></w:r></w:p>",
        "<w:p w14:paraId=\"22222222\"><w:r><w:t>Next.</w:t></w:r></w:p>"
    );
    let deleted = commit_mode(
        &package(body, ""),
        ChangeMode::Track,
        vec![EditOp::DeleteParagraphs { at: vec![ParagraphAddress::id("11111111")] }],
    );
    let xml = document_xml(&deleted);
    assert!(xml.contains("<w:delText>Anchor</w:delText>"), "{xml}");
    assert!(xml.contains("<w:t>Inside the box</w:t>"), "{xml}");
}

fn settle(bytes: &[u8], action: docxdriver_core::RevisionAction) -> Vec<u8> {
    commit_mode(
        bytes,
        ChangeMode::Direct,
        vec![EditOp::RevisionSettle { target: docxdriver_core::RevisionTarget::All, action }],
    )
}

#[test]
fn accepting_a_deletion_keeps_anchors_and_never_cuts_a_field() {
    // The deletion holds a field's begin, code and separator, a bookmark
    // start and a comment reference; the field result, its end and the
    // bookmark end sit outside it.
    let body = concat!(
        "<w:p w14:paraId=\"11111111\"><w:del w:id=\"1\" w:author=\"a\" w:date=\"2020-01-01T00:00:00Z\">",
        "<w:r><w:delText xml:space=\"preserve\">Gone </w:delText></w:r><w:bookmarkStart w:id=\"5\" w:name=\"mark\"/>",
        "<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:delInstrText xml:space=\"preserve\"> PAGE </w:delInstrText></w:r>",
        "<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:commentReference w:id=\"0\"/></w:r></w:del>",
        "<w:r><w:t>7</w:t></w:r><w:bookmarkEnd w:id=\"5\"/><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>"
    );
    let input = package(body, "");
    let accepted = settle(&input, docxdriver_core::RevisionAction::Accept);
    let xml = document_xml(&accepted);
    assert_eq!(reading(&accepted), "7");
    assert!(xml.contains("<w:bookmarkStart w:id=\"5\" w:name=\"mark\"/>"), "{xml}");
    assert!(xml.contains("<w:commentReference w:id=\"0\"/>"), "{xml}");
    assert!(!xml.contains("fldChar") && !xml.contains("InstrText"), "{xml}");
    assert_eq!(messages(&accepted), messages(&input));
}

#[test]
fn settling_tracked_cells() {
    let row = |cells: &str| format!("<w:tr>{cells}</w:tr>");
    let cell = |props: &str, text: &str| {
        format!("<w:tc><w:tcPr><w:tcW w:w=\"100\" w:type=\"dxa\"/>{props}</w:tcPr><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>")
    };
    let inserted = "<w:cellIns w:id=\"1\" w:author=\"a\" w:date=\"2020-01-01T00:00:00Z\"/>";
    let body = format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid><w:gridCol w:w=\"100\"/><w:gridCol w:w=\"100\"/></w:tblGrid>{}{}</w:tbl><w:p/>",
        row(&(cell("", "A") + &cell("", "B"))),
        row(&(cell(inserted, "New") + &cell("", "C"))),
    );
    let input = package(&body, "");
    let rejected = settle(&input, docxdriver_core::RevisionAction::Reject);
    let xml = document_xml(&rejected);
    assert_eq!(reading(&rejected), "ABC");
    assert!(xml.contains("<w:trPr><w:gridAfter w:val=\"1\"/></w:trPr>"), "{xml}");
    assert!(!xml.contains("cellIns"), "{xml}");
    let accepted = settle(&input, docxdriver_core::RevisionAction::Accept);
    assert_eq!(reading(&accepted), "ABNewC");
    assert!(!document_xml(&accepted).contains("cellIns"));
    // A row whose only cell is rejected goes, and a table without rows
    // becomes an empty paragraph.
    let lone = format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr><w:tblGrid><w:gridCol w:w=\"100\"/></w:tblGrid>{}</w:tbl><w:p/>",
        row(&cell(inserted, "New"))
    );
    let gone = settle(&package(&lone, ""), docxdriver_core::RevisionAction::Reject);
    assert!(!document_xml(&gone).contains("<w:tbl>"), "{}", document_xml(&gone));
    assert_eq!(messages(&gone), Vec::<String>::new());
}

#[test]
fn deleting_an_equation_keeps_comment_anchors() {
    let body = concat!(
        "<w:p w14:paraId=\"11111111\"><w:r><w:t>x</w:t></w:r><m:oMath>",
        "<m:r><m:t>a</m:t></m:r><w:r><w:commentReference w:id=\"0\"/></w:r></m:oMath></w:p>"
    );
    let input = package(body, " xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\"");
    let out = commit_mode(
        &input,
        ChangeMode::Direct,
        vec![EditOp::DeleteEquation { at: ParagraphAddress::id("11111111"), equation: None }],
    );
    let xml = document_xml(&out);
    assert!(!xml.contains("oMath"), "{xml}");
    assert!(xml.contains("<w:commentReference w:id=\"0\"/>"), "{xml}");
}

#[test]
fn strict_documents_are_refused() {
    let input = package_ns("http://purl.oclc.org/ooxml/wordprocessingml/main", PARAGRAPH, "");
    let error = commit(
        &input,
        vec![EditOp::ReplaceText {
            at: ParagraphAddress::id("11111111"),
            select: "thirty".into(),
            occurrence: None,
            with: "sixty".into(),
        }],
    )
    .unwrap_err();
    assert!(error.contains("Strict Open XML"), "{error}");
}

#[test]
fn single_instance_relationships_are_counted_across_strict_and_transitional() {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(package(PARAGRAPH, ""))).unwrap();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let name = entry.name().to_string();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).unwrap();
        writer.start_file(name, options).unwrap();
        writer.write_all(&content).unwrap();
    }
    let settings = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"/>";
    let rels = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        "<Relationship Id=\"rId1\" Type=\"http://purl.oclc.org/ooxml/officeDocument/relationships/settings\" Target=\"settings.xml\"/>",
        "<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings\" Target=\"settings.xml\"/>",
        "</Relationships>"
    );
    for (name, content) in [("word/settings.xml", settings), ("word/_rels/document.xml.rels", rels)] {
        writer.start_file(name, options).unwrap();
        writer.write_all(content.as_bytes()).unwrap();
    }
    let bytes = writer.finish().unwrap().into_inner();
    assert!(messages(&bytes).iter().any(|m| m.contains("more than one settings relationship")), "{:?}", messages(&bytes));
}

#[test]
fn run_properties_stay_ahead_of_office_text_effects() {
    let w14_ns = " xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" mc:Ignorable=\"w14\"";
    let body = |rpr: &str| {
        format!("<w:p w14:paraId=\"11111111\"><w:r><w:rPr>{rpr}</w:rPr><w:t>Glowing words</w:t></w:r></w:p>")
    };
    let glow = "<w14:glow w14:rad=\"63500\"><w14:srgbClr w14:val=\"00B0F0\"/></w14:glow>";
    let misplaced = package(&body(&format!("{glow}<w:b/>")), w14_ns);
    assert!(
        messages(&misplaced).iter().any(|m| m == "schema: w:b follows an Office extension element in w:rPr"),
        "{:?}",
        messages(&misplaced)
    );
    let input = package(&body(glow), w14_ns);
    for mode in [ChangeMode::Direct, ChangeMode::Track] {
        let out = commit_mode(
            &input,
            mode,
            vec![EditOp::FormatText {
                at: ParagraphAddress::id("11111111"),
                select: "Glowing".into(),
                occurrence: None,
                bold: Some(true),
                italic: None,
                underline: None,
                strike: None,
                superscript: None,
                subscript: None,
                highlight: None,
                color: Some("#AABBCC".into()),
                font_size: None,
                clear: vec![],
            }],
        );
        let xml = document_xml(&out);
        let run = &xml[xml.find("<w:rPr>").unwrap()..];
        let glow = run.find("<w14:glow").unwrap();
        assert!(run.find("<w:b").unwrap() < glow && run.find("<w:color").unwrap() < glow, "{xml}");
        assert_eq!(messages(&out), Vec::<String>::new());
    }
}
