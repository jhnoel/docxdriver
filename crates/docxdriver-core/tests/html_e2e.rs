//! End-to-end coverage through the public source-bound API, not renderer internals.
use docxdriver_core::*;
use serde_json::{json, Value};
use std::io::{Cursor, Read, Write};

pub const MATH: &str = r#"<math display="block"><mfrac><msup><mi>x</mi><mn>2</mn></msup><msqrt><mi>y</mi></msqrt></mfrac></math>"#;
const PNG: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jV1kAAAAASUVORK5CYII=";
fn command(input: Option<&[u8]>, command: Command) -> CommandResult {
    execute_command(
        input,
        &CommandRequest {
            command,
            expected_source: None,
        },
    )
}
fn bytes(result: CommandResult) -> Vec<u8> {
    match result {
        CommandResult::Completed {
            bytes: Some(bytes), ..
        } => bytes,
        other => panic!("{other:?}"),
    }
}
fn create(html: &str) -> Vec<u8> {
    bytes(command(
        None,
        Command::Create(CreateCommand {
            html: Some(html.into()),
            paragraphs: vec![],
        }),
    ))
}
fn read(input: &[u8], kind: &str, view: &str) -> Value {
    match command(
        Some(input),
        Command::Read(ReadCommand {
            read_kind: Some(
                if kind == "document" {
                    "document_ui"
                } else {
                    kind
                }
                .into(),
            ),
            view: Some(view.into()),
        }),
    ) {
        CommandResult::Completed {
            result: Some(value),
            ..
        } => value,
        other => panic!("{other:?}"),
    }
}
fn source(input: &[u8]) -> SourceHash {
    SourceHash::from_bytes(input)
}
fn commit(input: &[u8], ops: Vec<EditOp>, mode: ChangeMode) -> Vec<u8> {
    let plan = Plan {
        base: source(input),
        author: "HTML e2e".into(),
        change_mode: mode,
        ops,
    };
    let preview = execute_plan(
        input,
        &PlanRequest {
            plan: plan.clone(),
            preview_key: None,
        },
    );
    let key = match preview {
        PlanResult::Previewed { preview_key, .. } => preview_key,
        other => panic!("{other:?}"),
    };
    match execute_plan(
        input,
        &PlanRequest {
            plan,
            preview_key: Some(key),
        },
    ) {
        PlanResult::Committed { bytes, .. } => bytes,
        other => panic!("{other:?}"),
    }
}
fn part(input: &[u8], name: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(input)).unwrap();
    let mut out = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}
fn html(doc: &Value) -> &str {
    doc["html"].as_str().unwrap()
}
fn assert_html(doc: &Value) {
    assert_eq!(doc["projection_version"], 2);
    assert_eq!(doc["content_type"], "text/html");
    assert!(doc.get("markup").is_none());
    let parsed = xmloxide::html5::parse_html5_full(&format!(
        "<!doctype html><html><head><title>e2e</title></head><body>{}</body></html>",
        html(doc)
    ));
    assert!(
        parsed.errors.is_empty(),
        "HTML tree repair: {:?}\n{}",
        parsed.errors,
        html(doc)
    );
    for region in doc["source_map"]["regions"].as_array().unwrap() {
        let fragment = &html(doc)[region["html_start"].as_u64().unwrap() as usize
            ..region["html_end"].as_u64().unwrap() as usize];
        for id in region["paragraphs"].as_array().unwrap() {
            let id = id.as_str().unwrap();
            assert!(
                fragment.contains(&format!("id=\"{id}\""))
                    || fragment.contains(&format!("data-docx-paragraph=\"{id}\"")),
                "source map ID missing from its region: {id}: {fragment}"
            );
        }
    }
    for forbidden in [
        "<equation",
        "<image",
        "<field",
        "<format",
        "<comment-start",
        "<comment-end",
        "<footnote>",
        "<section/>",
    ] {
        assert!(!html(doc).contains(forbidden), "non-HTML {forbidden}");
    }
}

#[test]
fn html5_creation_lists_tables_media_and_mathml_are_renderable() {
    let input = create(&format!(
        r#"<section><header><p>Example</p></header><h1>HTML &amp; MathML</h1>
        <p style='text-align:center;color:#123456;font-size:14pt'>Hello&nbsp;<strong>world</strong> &#x1F600;<br>Again.</p>
        <ol start=3><li>First<ul><li>Nested</li></ul></li><li>Second</li></ol>
        <table><tr><th colspan=2>Heading</th></tr><tr><td rowspan=2>Left</td><td>Top</td></tr><tr><td><table><tr><td>Nested cell</td></tr></table></td></tr></table>
        <p><img src='data:image/png;base64,{PNG}' width=32 height=24 alt='one pixel'></p><p>{MATH}</p></section>"#
    ));
    let doc = read(&input, "document", "markup");
    assert_html(&doc);
    for tag in [
        "<ol ",
        "<ul ",
        "<li ",
        "<tbody>",
        "colspan=\"2\"",
        "rowspan=\"2\"",
        "<img src=\"assets/",
        "width=\"32\"",
        "height=\"24\"",
        "<math xmlns=",
        "<mfrac>",
        "<msup>",
        "<msqrt>",
        "text-align:center",
        "color:#123456",
    ] {
        assert!(html(&doc).contains(tag), "missing {tag}: {}", html(&doc));
    }
    assert!(doc["css"]
        .as_str()
        .unwrap()
        .contains("white-space: pre-wrap"));
    assert_eq!(doc["assets"].as_array().unwrap().len(), 1);
    assert!(doc["assets"][0]["data_url"].is_null());
    let assets = read(&input, "assets", "final");
    assert!(assets["assets"][0]["data_url"]
        .as_str()
        .unwrap()
        .starts_with("data:image/png;base64,"));
    let equation = &doc["equations"][0];
    assert!(equation["latex"].is_null());
    assert!(equation["mathml"].as_str().unwrap().contains("<mfrac>"));
    let ids: Vec<&str> = doc["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    for block in doc["blocks"].as_array().unwrap() {
        for id in block["paragraphs"].as_array().unwrap() {
            assert!(ids.contains(&id.as_str().unwrap()));
        }
    }
    for region in doc["source_map"]["regions"].as_array().unwrap() {
        let start = region["html_start"].as_u64().unwrap() as usize;
        let end = region["html_end"].as_u64().unwrap() as usize;
        assert!(html(&doc).get(start..end).is_some());
    }
}

#[test]
fn html_replacements_track_reject_and_preserve_package_parts() {
    let input=create("<!doctype html><html><head><title>Contract</title><meta charset=utf-8></head><body><p>Pay within thirty days.</p><p>Untouched <em>paragraph</em>.</p></body></html>");
    let doc = read(&input, "document", "markup");
    let at = doc["paragraphs"][0]["id"].as_str().unwrap();
    let edited=commit(&input,vec![EditOp::ReplaceText{at:ParagraphAddress::id(at),select:"thirty".into(),with:"<span style='color:#AABBCC;font-size:15pt;font-family:Arial'><strong>sixty</strong></span>".into(),occurrence:None}],ChangeMode::Track);
    assert_eq!(
        part(&input, "word/styles.xml"),
        part(&edited, "word/styles.xml")
    );
    let markup = read(&edited, "document", "markup");
    assert_html(&markup);
    assert!(html(&markup).contains("data-docx-revision="));
    assert!(html(&markup).contains("color:#AABBCC"));
    assert_eq!(
        read(&edited, "document", "final")["paragraphs"][0]["text"],
        "Pay within sixty days."
    );
    assert_eq!(
        read(&edited, "document", "original")["paragraphs"][0]["text"],
        "Pay within thirty days."
    );
    let restored = commit(
        &edited,
        vec![EditOp::RevisionSettle {
            target: RevisionTarget::All,
            action: RevisionAction::Reject,
        }],
        ChangeMode::Direct,
    );
    assert_eq!(
        html(&read(&restored, "document", "final")),
        html(&read(&input, "document", "final"))
    );
}

#[test]
fn mathml_equation_edits_preview_commit_and_restore_original_omml() {
    let input = create(&format!("<p>{MATH}</p>"));
    let doc = read(&input, "document", "markup");
    let at = doc["equations"][0]["at"].as_str().unwrap();
    let replacement = "<math><mroot><mi>z</mi><mn>3</mn></mroot></math>";
    let edited = commit(
        &input,
        vec![EditOp::ReplaceEquation {
            at: ParagraphAddress::id(at),
            mathml: replacement.into(),
            equation: None,
            display: None,
        }],
        ChangeMode::Track,
    );
    let current = read(&edited, "document", "final");
    assert_html(&current);
    assert!(html(&current).contains("<mroot>"));
    assert!(!html(&current).contains("<mfrac>"));
    let restored = commit(
        &edited,
        vec![EditOp::RevisionSettle {
            target: RevisionTarget::All,
            action: RevisionAction::Reject,
        }],
        ChangeMode::Direct,
    );
    let original = String::from_utf8(part(&input, "word/document.xml")).unwrap();
    let restored_xml = String::from_utf8(part(&restored, "word/document.xml")).unwrap();
    let omml = |xml: String| -> String {
        let start = xml.find("<m:oMath").unwrap();
        let end = xml.find("</m:oMathPara>").unwrap() + "</m:oMathPara>".len();
        xml[start..end].into()
    };
    assert_eq!(omml(original), omml(restored_xml));
    for bad in [
        "<math><mfrac><mi>x</mi></mfrac></math>",
        "<math><script>bad</script></math>",
        "<math><mi onclick='bad()'>x</mi></math>",
    ] {
        let plan = Plan {
            base: source(&input),
            author: "e2e".into(),
            change_mode: ChangeMode::Track,
            ops: vec![EditOp::ReplaceEquation {
                at: ParagraphAddress::id(at),
                mathml: bad.into(),
                equation: None,
                display: None,
            }],
        };
        assert!(matches!(
            execute_plan(
                &input,
                &PlanRequest {
                    plan,
                    preview_key: None
                }
            ),
            PlanResult::Rejected { .. }
        ));
    }
}

#[test]
fn unsafe_or_unrepresentable_html_is_rejected_without_bytes() {
    for bad in [
        "<p><script>alert(1)</script></p>",
        "<p><img src='https://example.com/a.png'></p>",
        "<p><a href='javascript:alert(1)'>x</a></p>",
        "<p><span onclick='bad()'>x</span></p>",
        "<p><span style='position:fixed'>x</span></p>",
        "<p><math><unknown>x</unknown></math></p>",
    ] {
        assert!(
            matches!(
                command(
                    None,
                    Command::Create(CreateCommand {
                        html: Some(bad.into()),
                        paragraphs: vec![]
                    })
                ),
                CommandResult::Rejected { .. }
            ),
            "accepted {bad}"
        );
    }
    let input = create("<p>Original</p>");
    let doc = read(&input, "document", "markup");
    let at = doc["paragraphs"][0]["id"].as_str().unwrap();
    let result = command(
        Some(&input),
        Command::Edit(EditCommand {
            author: "e2e".into(),
            change_mode: ChangeMode::Track,
            op: EditOp::ReplaceText {
                at: ParagraphAddress::id(at),
                select: "Original".into(),
                with: "<p>New block</p>".into(),
                occurrence: None,
            },
        }),
    );
    assert!(matches!(result, CommandResult::Rejected { .. }));
}

fn with_parts(input: &[u8], replacements: &[(&str, String)]) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(input)).unwrap();
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        if replacements.iter().any(|(name, _)| *name == f.name()) {
            continue;
        }
        let mut b = Vec::new();
        f.read_to_end(&mut b).unwrap();
        writer.start_file(f.name(), options).unwrap();
        writer.write_all(&b).unwrap();
    }
    for (name, text) in replacements {
        writer.start_file(*name, options).unwrap();
        writer.write_all(text.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn fixture() -> Vec<u8> {
    let input = create(&format!(
        r#"<section><header><p>Renderable document</p></header><h1>HTML document projection</h1>
    <p>Repeat 😀 target, then target.</p><p>Equation {MATH} after math.</p>
    <ol start=3><li>Numbered item<ul><li>Nested bullet</li></ul></li><li>Next item</li></ol>
    <table><tr><td rowspan=2>Spanning cell</td><td>Top right</td></tr><tr><td>Bottom right</td></tr></table>
    <p><img src='data:image/png;base64,{PNG}' width=32 height=24 alt='Embedded pixel'> Note here.</p>
    <p style='text-align:right;color:#123456;font-size:14pt'>Styled final paragraph</p></section>"#
    ));
    let xml = String::from_utf8(part(&input, "word/document.xml"))
        .unwrap()
        .replace(
            "Note here.</w:t>",
            "Note here.</w:t><w:footnoteReference w:id=\"7\"/>",
        );
    let rels=String::from_utf8(part(&input,"word/_rels/document.xml.rels")).unwrap().replace("</Relationships>","<Relationship Id=\"rIdNote\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes\" Target=\"footnotes.xml\"/></Relationships>");
    let types=String::from_utf8(part(&input,"[Content_Types].xml")).unwrap().replace("</Types>","<Override PartName=\"/word/footnotes.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/></Types>");
    let notes="<w:footnotes xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:footnote w:id=\"7\"><w:p><w:r><w:t>Footnote body with </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>formatting</w:t></w:r></w:p></w:footnote></w:footnotes>";
    with_parts(
        &input,
        &[
            ("word/document.xml", xml),
            ("word/_rels/document.xml.rels", rels),
            ("[Content_Types].xml", types),
            ("word/footnotes.xml", notes.into()),
            (
                "customXml/untouched.xml",
                "<opaque>Preserve this</opaque>".into(),
            ),
        ],
    )
}

#[test]
fn e2e_artifact_fixture() {
    let input = fixture();
    let doc = read(&input, "document", "final");
    assert_html(&doc);
    assert!(html(&doc).contains("-footnote-7\">1</a>"));
    assert!(html(&doc).contains("<aside data-docx-notes=\"footnote\">"));
    let paragraph = doc["paragraphs"][0]["id"].as_str().unwrap();
    let edited = commit(
        &input,
        vec![EditOp::ReplaceText {
            at: ParagraphAddress::id(paragraph),
            select: "HTML".into(),
            with: "Web".into(),
            occurrence: None,
        }],
        ChangeMode::Track,
    );
    assert_eq!(
        part(&input, "customXml/untouched.xml"),
        part(&edited, "customXml/untouched.xml")
    );
    assert_eq!(
        part(&input, "word/footnotes.xml"),
        part(&edited, "word/footnotes.xml")
    );
    if let Ok(dir) = std::env::var("DOCXDRIVER_HTML_E2E_OUT") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/source.docx"), input).unwrap();
        std::fs::write(
            format!("{dir}/native.json"),
            serde_json::to_vec_pretty(&json!(doc)).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn mathml_display_attribute_controls_native_creation() {
    let input = create(
        "<p><math display=inline><mi>x</mi></math></p><p><math display=block><mi>y</mi></math></p>",
    );
    let doc = read(&input, "document", "markup");
    assert_eq!(doc["equations"][0]["display"], false);
    assert_eq!(doc["equations"][1]["display"], true);
}

#[test]
fn html_format_cascade_and_native_math_structures_round_trip() {
    let input = create(
        r#"<p><strong>outer <strong>inner</strong> outer</strong> plain <span style="font-weight:bold"><b>nested</b></span><em style="color:#336699">colored</em></p><p><math><mrow><mi mathvariant="bold">A</mi><mo>+</mo><mfrac bevelled="true"><mn>1</mn><mn>2</mn></mfrac><mmultiscripts><mi>x</mi><mn>1</mn><mn>2</mn><mprescripts></mprescripts><mn>3</mn><mn>4</mn></mmultiscripts></mrow></math></p>"#,
    );
    let doc = read(&input, "document", "final");
    assert_html(&doc);
    let output = html(&doc);
    assert!(output.contains("outer inner outer"), "{output}");
    assert!(output.contains("color:#336699"), "{output}");
    assert!(output.contains("mathvariant=\"bold\""), "{output}");
    assert!(output.contains("bevelled=\"true\""), "{output}");
    assert!(output.contains("<mmultiscripts>"), "{output}");
    for token in [">1<", ">2<", ">3<", ">4<"] {
        assert!(output.contains(token), "{output}");
    }
    // The public HTML/MathML surface can be created again without a second format.
    let recreated = create(output);
    let again = read(&recreated, "document", "final");
    assert_html(&again);
    assert!(html(&again).contains("<mmultiscripts>"));
}

#[test]
fn real_docx_html_views_and_edit_preserve_unrelated_parts() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test-docs/ctnf-18690238-data-stream.docx");
    let input = std::fs::read(path).unwrap();
    for view in ["markup", "final", "original"] {
        assert_html(&read(&input, "document", view));
    }
    let doc = read(&input, "document", "markup");
    let paragraph = doc["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["selectable_text"].as_str().is_some_and(|t| t.len() > 20))
        .unwrap();
    let at = ParagraphAddress::id(paragraph["id"].as_str().unwrap());
    let changed = commit(
        &input,
        vec![EditOp::ReplaceText {
            at,
            select: paragraph["selectable_text"].as_str().unwrap().into(),
            occurrence: None,
            with: "<strong>Real document verification</strong>".into(),
        }],
        ChangeMode::Direct,
    );
    assert_html(&read(&changed, "document", "final"));
    let mut archive = zip::ZipArchive::new(Cursor::new(&input)).unwrap();
    for i in 0..archive.len() {
        let entry = archive.by_index(i).unwrap();
        let name = entry.name().to_string();
        if entry.is_dir() || name == "word/document.xml" {
            continue;
        }
        drop(entry);
        assert_eq!(
            part(&input, &name),
            part(&changed, &name),
            "unrelated part changed: {name}"
        );
    }
}

#[test]
fn html_and_math_comments_are_invisible_on_create_and_edit() {
    let input = create(
        "<!--leading--><p>A<!--hidden-->B<math><!--math--><mi>x</mi></math></p><!--trailing-->",
    );
    let doc = read(&input, "document", "final");
    assert_eq!(doc["paragraphs"][0]["text"], "AB");
    for comment in ["leading", "hidden", "math--", "trailing"] {
        assert!(!html(&doc).contains(comment));
    }
    let changed = commit(
        &input,
        vec![EditOp::ReplaceText {
            at: ParagraphAddress::id(doc["paragraphs"][0]["id"].as_str().unwrap()),
            select: "AB".into(),
            occurrence: None,
            with: "C<!-- invisible -->D".into(),
        }],
        ChangeMode::Direct,
    );
    assert_eq!(
        read(&changed, "document", "final")["paragraphs"][0]["text"],
        "CD"
    );
}

#[test]
fn default_paragraph_style_and_its_base_apply_without_pstyle() {
    let input = create("<p>Default paragraph</p><p data-docx-style='Other'>Explicit paragraph</p>");
    let styles = r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Base"><w:pPr><w:jc w:val="right"/></w:pPr><w:rPr><w:color w:val="CC0000"/></w:rPr></w:style><w:style w:type="paragraph" w:default="1" w:styleId="DefaultParagraph"><w:basedOn w:val="Base"/><w:rPr><w:b/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Other"><w:rPr><w:i/></w:rPr></w:style></w:styles>"#;
    let patched = with_parts(&input, &[("word/styles.xml", styles.into())]);
    for view in ["markup", "final", "original"] {
        let doc = read(&patched, "document", view);
        assert_html(&doc);
        let blocks = doc["blocks"].as_array().unwrap();
        let default = &html(&doc)[blocks[0]["html_start"].as_u64().unwrap() as usize
            ..blocks[0]["html_end"].as_u64().unwrap() as usize];
        assert!(
            default.contains("text-align:right")
                && default.contains("color:#CC0000")
                && default.contains("<b>Default paragraph</b>"),
            "{default}"
        );
        let explicit = &html(&doc)[blocks[1]["html_start"].as_u64().unwrap() as usize
            ..blocks[1]["html_end"].as_u64().unwrap() as usize];
        assert!(
            explicit.contains("<i>Explicit paragraph</i>")
                && !explicit.contains("text-align:right"),
            "{explicit}"
        );
    }
}

const RESETS:&str="<span style='font-weight:normal;font-style:normal;text-decoration:none;vertical-align:baseline'>Reset text</span>";
fn reset_styles() -> String {
    r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:rPr><w:b/><w:i/><w:u w:val="single"/><w:strike/><w:vertAlign w:val="superscript"/></w:rPr></w:style></w:styles>"#.into()
}
fn assert_resets(input: &[u8]) {
    let xml = String::from_utf8(part(input, "word/document.xml")).unwrap();
    for prop in [
        "<w:b w:val=\"0\"",
        "<w:i w:val=\"0\"",
        "<w:u w:val=\"none\"",
        "<w:strike w:val=\"0\"",
        "<w:vertAlign w:val=\"baseline\"",
    ] {
        assert!(xml.contains(prop), "{prop}: {xml}");
    }
    let doc = read(input, "document", "final");
    for css in [
        "font-weight:normal",
        "font-style:normal",
        "text-decoration:none",
        "vertical-align:baseline",
    ] {
        assert!(html(&doc).contains(css), "{css}: {}", html(&doc));
    }
    for tag in ["<b>", "<i>", "<u>", "<s>", "<sup>"] {
        assert!(!html(&doc).contains(tag), "{tag}: {}", html(&doc));
    }
}
#[test]
fn explicit_format_resets_survive_creation_direct_and_tracked_edits() {
    let created = create(&format!("<h1>{RESETS}</h1>"));
    assert_resets(&with_parts(
        &created,
        &[("word/styles.xml", reset_styles())],
    ));
    let input = with_parts(
        &create("<h1>Old text</h1>"),
        &[("word/styles.xml", reset_styles())],
    );
    let doc = read(&input, "document", "final");
    for mode in [ChangeMode::Direct, ChangeMode::Track] {
        let changed = commit(
            &input,
            vec![EditOp::ReplaceText {
                at: ParagraphAddress::id(doc["paragraphs"][0]["id"].as_str().unwrap()),
                select: "Old text".into(),
                occurrence: None,
                with: RESETS.into(),
            }],
            mode,
        );
        assert_resets(&changed);
        if mode == ChangeMode::Track {
            assert!(html(&read(&changed, "document", "original")).contains("<b>"));
        }
    }
}

#[test]
fn inherited_math_variants_and_operator_variants_survive_and_unsupported_attrs_fail() {
    let input=create("<p><math><mstyle mathvariant='bold'><mi>x</mi><mo>+</mo><mn>2</mn><mi mathvariant='normal'>y</mi></mstyle></math></p>");
    let doc = read(&input, "document", "final");
    for token in [
        "<mi mathvariant=\"bold\">x</mi>",
        "<mo mathvariant=\"bold\">+</mo>",
        "<mn mathvariant=\"bold\">2</mn>",
        "<mi mathvariant=\"normal\">y</mi>",
    ] {
        assert!(html(&doc).contains(token), "{token}: {}", html(&doc));
    }
    for formula in [
        "<msubsup><mo mathvariant='bold'>∫</mo><mn>0</mn><mn>1</mn></msubsup><mi>x</mi>",
        "<mstyle mathvariant='bold'><mo>∑</mo><mi>x</mi></mstyle>",
    ] {
        let input = create(&format!("<p><math>{formula}</math></p>"));
        let doc = read(&input, "document", "final");
        let operator = if formula.contains('∫') {
            '∫'
        } else {
            '∑'
        };
        assert!(
            html(&doc).contains(&format!("<mo mathvariant=\"bold\">{operator}</mo>")),
            "{}",
            html(&doc)
        );
    }
    for attribute in [
        "mathcolor='red'",
        "mathsize='20px'",
        "style='color:red'",
        "unknown='discard'",
    ] {
        let result = command(
            None,
            Command::Create(CreateCommand {
                html: Some(format!("<p><math><mi {attribute}>x</mi></math></p>")),
                paragraphs: vec![],
            }),
        );
        assert!(
            matches!(result, CommandResult::Rejected { .. }),
            "{result:?}"
        );
    }
}

#[test]
fn multi_paragraph_list_items_fail_instead_of_changing_numbering() {
    for html in [
        "<ol><li><p>A</p><p>B</p></li><li>C</li></ol>",
        "<ul><li>A<p>B</p></li></ul>",
    ] {
        let result = command(
            None,
            Command::Create(CreateCommand {
                html: Some(html.into()),
                paragraphs: vec![],
            }),
        );
        match result {
            CommandResult::Rejected { diagnostic, .. } => assert!(
                diagnostic.message.contains("multiple paragraphs"),
                "{diagnostic:?}"
            ),
            other => panic!("{other:?}"),
        }
    }
    let input = create("<ol><li>A<br>B<ul><li>Nested</li></ul></li><li>C</li></ol>");
    let doc = read(&input, "document", "final");
    assert_html(&doc);
    assert_eq!(html(&doc).matches("<li ").count(), 3);
}

#[test]
fn nary_limits_preserve_explicit_placement_and_document_defaults() {
    for (tag, location) in [("msubsup", "subSup"), ("munderover", "undOvr")] {
        let input = create(&format!(
            "<p><math><{tag}><mo>∫</mo><mn>0</mn><mn>1</mn></{tag}><mi>x</mi></math></p>"
        ));
        let doc = read(&input, "document", "final");
        assert!(html(&doc).contains(&format!("<{tag}>")), "{}", html(&doc));
        assert!(doc["equations"][0]["mathml"]
            .as_str()
            .unwrap()
            .contains(&format!("<{tag}>")));
        let xml = String::from_utf8(part(&input, "word/document.xml")).unwrap();
        assert!(
            xml.contains(&format!("<m:limLoc m:val=\"{location}\"")),
            "{xml}"
        );
        let no_location = xml
            .replace(&format!("<m:limLoc m:val=\"{location}\" />"), "")
            .replace(&format!("<m:limLoc m:val=\"{location}\"/>"), "");
        assert!(!no_location.contains("limLoc"));
        let settings = r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:mathPr><m:intLim m:val="undOvr"/></m:mathPr></w:settings>"#;
        let patched = with_parts(
            &input,
            &[
                ("word/document.xml", no_location),
                ("word/settings.xml", settings.into()),
            ],
        );
        assert!(html(&read(&patched, "document", "final")).contains("<munderover>"));
    }
}

fn controls_fixture() -> Vec<u8> {
    let input = create("<p>Base</p>");
    let document = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"><w:body>
<w:p w14:paraId="11111111"><w:r><w:t>Before control.</w:t></w:r></w:p>
<w:sdt><w:sdtPr><w:alias w:val="Control metadata must not become text"/></w:sdtPr><w:sdtContent><w:customXml>
<w:sdt><w:sdtContent><w:p w14:paraId="22222222"><w:pPr><w:sectPr/></w:pPr><w:r><w:t>Critical control context.</w:t></w:r></w:p></w:sdtContent></w:sdt>
<w:sdt><w:sdtContent><w:tbl><w:sdt><w:sdtContent><w:tr><w:sdt><w:sdtContent><w:tc><w:sdt><w:sdtContent>
<w:p w14:paraId="33333333"><w:r><w:t>Controlled cell.</w:t></w:r></w:p>
<w:tbl><w:tr><w:tc><w:p w14:paraId="44444444"><w:r><w:t>Nested table context.</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
</w:sdtContent></w:sdt></w:tc></w:sdtContent></w:sdt></w:tr></w:sdtContent></w:sdt></w:tbl></w:sdtContent></w:sdt>
</w:customXml></w:sdtContent></w:sdt>
<w:p w14:paraId="55555555"><w:r><w:t>After control.</w:t></w:r></w:p><w:sectPr/>
</w:body></w:document>"#;
    with_parts(&input, &[("word/document.xml", document.into())])
}

#[test]
fn content_controls_read_edit_and_table_anchors_keep_full_context() {
    let input = controls_fixture();
    let expected = [
        "Before control.",
        "Critical control context.",
        "Controlled cell.",
        "Nested table context.",
        "After control.",
    ];
    for view in ["markup", "final", "original"] {
        let ui = read(&input, "document", view);
        assert_html(&ui);
        let model = match command(
            Some(&input),
            Command::Read(ReadCommand {
                read_kind: None,
                view: Some(view.into()),
            }),
        ) {
            CommandResult::Completed {
                result: Some(result),
                ..
            } => result,
            other => panic!("{other:?}"),
        };
        for html in [html(&ui), model["markup"].as_str().unwrap()] {
            let positions: Vec<_> = expected
                .iter()
                .map(|text| html.find(text).expect("all context rendered"))
                .collect();
            assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
            assert_eq!(html.matches("<section ").count(), 2);
            assert_eq!(html.matches("<table").count(), 2);
            assert!(!html.contains("Control metadata must not become text"));
        }
        assert_eq!(ui["paragraphs"].as_array().unwrap().len(), 5);
    }
    let edited = commit(
        &input,
        vec![
            EditOp::ReplaceText {
                at: ParagraphAddress::id("22222222"),
                select: "Critical".into(),
                with: "Preserved".into(),
                occurrence: None,
            },
            EditOp::InsertParagraph {
                at: InsertAnchor::Table(1),
                position: InsertPosition::After,
                with: "After controlled table.".into(),
                style: None,
                alias: None,
            },
        ],
        ChangeMode::Direct,
    );
    let ui = read(&edited, "document", "final");
    assert!(html(&ui).contains("Preserved control context."));
    let appended = html(&ui).find("After controlled table.").unwrap();
    assert!(appended > html(&ui).find("Nested table context.").unwrap());
    assert!(appended < html(&ui).find("After control.").unwrap());
    assert_eq!(
        part(&input, "word/styles.xml"),
        part(&edited, "word/styles.xml")
    );
    let xml = String::from_utf8(part(&edited, "word/document.xml")).unwrap();
    assert_eq!(xml.matches(":sdt>").count(), 12);
    assert!(xml.contains("Control metadata must not become text"));
    if let Ok(dir) = std::env::var("DOCXDRIVER_HTML_E2E_OUT") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(format!("{dir}/controls.docx"), input).unwrap();
    }
}

#[test]
fn mathml_accent_writes_preserve_explicit_semantics() {
    for tag in ["mover", "munder"] {
        let attribute = if tag == "mover" {
            "accent"
        } else {
            "accentunder"
        };
        for value in ["true", "false"] {
            for decoration in ["¯", "ˆ", "⏞", "⏟"] {
                let math=format!("<math><{tag} {attribute}=\"{value}\"><mi>x</mi><mo>{decoration}</mo></{tag}></math>");
                let input = create(&format!("<p>{math}</p>"));
                let ui = read(&input, "document", "final");
                assert_html(&ui);
                let parsed = xmloxide::html5::parse_html5_full(html(&ui));
                let doc = &parsed.document;
                let node = doc
                    .descendants(doc.root())
                    .find(|&n| doc.node_name(n) == Some(tag))
                    .unwrap();
                assert_eq!(
                    doc.attribute(node, attribute),
                    Some(value),
                    "{math}: {}",
                    html(&ui)
                );
                assert!(doc.text_content(node).contains(decoration));
                // Editing an existing equation uses the same supported public input.
                let base = create("<p><math><mi>y</mi></math></p>");
                let at = read(&base, "document", "markup")["equations"][0]["at"]
                    .as_str()
                    .unwrap()
                    .to_string();
                let edited = commit(
                    &base,
                    vec![EditOp::ReplaceEquation {
                        at: ParagraphAddress::id(at),
                        mathml: math,
                        equation: None,
                        display: None,
                    }],
                    ChangeMode::Track,
                );
                let projection = read(&edited, "document", "final");
                let parsed = xmloxide::html5::parse_html5_full(html(&projection));
                let node = parsed
                    .document
                    .descendants(parsed.document.root())
                    .find(|&n| parsed.document.node_name(n) == Some(tag))
                    .unwrap();
                assert_eq!(parsed.document.attribute(node, attribute), Some(value));
                assert!(html(&read(&edited, "document", "original")).contains(">y</mi>"));
            }
        }
    }
    for math in [
        "<math><mover accent='true'><mi>x</mi><mo stretchy='false'>ˆ</mo></mover></math>",
        "<math><munder accentunder='true'><mi>x</mi><mrow><mi>a</mi><mi>b</mi></mrow></munder></math>",
        "<math><mover accent='invalid'><mi>x</mi><mo>ˆ</mo></mover></math>",
        "<math><munderover accentunder='true' accent='true'><mi>x</mi><mo>¯</mo><mo>ˆ</mo></munderover></math>",
    ] {
        assert!(matches!(command(None,Command::Create(CreateCommand{html:Some(format!("<p>{math}</p>")),paragraphs:vec![]})),CommandResult::Rejected{..}));
    }
}

#[test]
fn agent_read_separates_css_and_resolves_named_paragraph_styles() {
    let input = create("<h1>Title</h1><p>Body</p>");
    let styles = r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Base"><w:rPr><w:color w:val="123456"/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:basedOn w:val="Base"/><w:rPr><w:sz w:val="32"/></w:rPr></w:style><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:rPr><w:sz w:val="24"/></w:rPr></w:style></w:styles>"#;
    let input = with_parts(&input, &[("word/styles.xml", styles.into())]);
    for view in ["markup", "final", "original"] {
        let result = match command(Some(&input), Command::Read(ReadCommand {read_kind: None, view: Some(view.into())})) {
            CommandResult::Completed {result: Some(v), ..} => v,
            other => panic!("{other:?}"),
        };
        let markup = result["markup"].as_str().unwrap();
        assert!(markup.contains("data-docx-style=\"Heading1\""));
        assert!(markup.contains("data-docx-style=\"Normal\""));
        assert!(!markup.contains(" style="));
        assert!(!markup.contains("<style"));
        assert!(result["styles"]["Heading1"].as_str().unwrap().contains("color:#123456"));
        assert!(result["styles"]["Heading1"].as_str().unwrap().contains("font-size:16pt"));
        assert!(!result["css"].as_object().unwrap().is_empty());
    }
}
