//! Formatting contract regressions through public preview/commit operations.
use docxdriver_core::*;
use serde_json::{json, Value};
use std::io::{Cursor, Read, Write};
use xmloxide::tree::{Document, NodeId};

const AT: &str = "11111111";
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const STYLES: &str = r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:pPr><w:jc w:val="right"/></w:pPr><w:rPr><w:b/><w:i/><w:strike/><w:color w:val="112233"/><w:sz w:val="28"/><w:u w:val="double"/><w:vertAlign w:val="superscript"/></w:rPr></w:style>"#;
const RICH_RUN: &str = r#"<w:rFonts w:ascii="Arial" w:hAnsi="Arial" w:eastAsia="MS Mincho"/><w:b w:val="0"/><w:i w:val="false"/><w:smallCaps/><w:color w:val="auto" w:themeColor="accent1" w:themeTint="80"/><w:sz w:val="24"/><w:highlight w:val="yellow"/><w:u w:val="double" w:color="FF0000" w:themeColor="accent2"/><w:lang w:val="en-US"/>"#;
const RICH_PARAGRAPH: &str = r#"<w:keepNext/><w:keepLines/><w:widowControl w:val="0"/><w:pBdr><w:bottom w:val="single" w:sz="4" w:color="AABBCC"/></w:pBdr><w:shd w:val="clear" w:fill="EEEEEE"/><w:tabs><w:tab w:val="right" w:leader="dot" w:pos="1440"/></w:tabs><w:spacing w:before="120" w:after="240" w:line="276" w:lineRule="auto" w:beforeAutospacing="0"/><w:ind w:left="360" w:right="180" w:hanging="180"/><w:jc w:val="left"/><w:rPr><w:lang w:val="en-US"/></w:rPr><w:sectPr><w:pgSz w:w="12240" w:h="15840"/></w:sectPr>"#;

fn command(input: Option<&[u8]>, cmd: Command) -> CommandResult {
    execute_command(
        input,
        &CommandRequest {
            command: cmd,
            expected_source: None,
        },
    )
}
fn create(html: &str) -> Vec<u8> {
    match command(
        None,
        Command::Create(CreateCommand {
            html: Some(html.into()),
            paragraphs: vec![],
        }),
    ) {
        CommandResult::Completed {
            bytes: Some(bytes), ..
        } => bytes,
        other => panic!("{other:?}"),
    }
}
fn parts(input: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(input)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut entry = zip.by_index(i).unwrap();
            let mut bytes = vec![];
            entry.read_to_end(&mut bytes).unwrap();
            (entry.name().to_owned(), bytes)
        })
        .collect()
}
fn fixture(run: &str, paragraph: &str) -> Vec<u8> {
    let mut entries = parts(&create("<p>seed</p>"));
    entries.insert("word/document.xml".into(), format!(r#"<w:document xmlns:w="{W}" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"><w:body><w:p w14:paraId="{AT}"><w:pPr>{paragraph}</w:pPr><w:r><w:rPr>{run}</w:rPr><w:t>before selected after</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#).into_bytes());
    entries.insert(
        "word/styles.xml".into(),
        format!(r#"<w:styles xmlns:w="{W}">{STYLES}</w:styles>"#).into_bytes(),
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
    for (name, bytes) in entries {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn commit(input: &[u8], ops: Vec<EditOp>) -> Vec<u8> {
    let plan = Plan {
        base: SourceHash::from_bytes(input),
        author: "fidelity".into(),
        change_mode: ChangeMode::Track,
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
fn format(properties: Value) -> EditOp {
    let mut op = json!({"op":"format_text", "at":AT, "select":"selected"});
    op.as_object_mut()
        .unwrap()
        .extend(properties.as_object().unwrap().clone());
    serde_json::from_value(op).unwrap()
}
fn para_format(properties: Value) -> EditOp {
    let mut op = json!({"op":"format_paragraph", "at":AT});
    op.as_object_mut()
        .unwrap()
        .extend(properties.as_object().unwrap().clone());
    serde_json::from_value(op).unwrap()
}
fn settle(input: &[u8], action: RevisionAction) -> Vec<u8> {
    commit(
        input,
        vec![EditOp::RevisionSettle {
            target: RevisionTarget::All,
            action,
        }],
    )
}
fn xml(input: &[u8]) -> Document {
    Document::parse_bytes(&parts(input)["word/document.xml"]).unwrap()
}
fn local(doc: &Document, n: NodeId) -> &str {
    doc.node_name(n).unwrap_or("").rsplit(':').next().unwrap()
}
fn child(doc: &Document, n: NodeId, name: &str) -> Option<NodeId> {
    doc.children(n).find(|&c| local(doc, c) == name)
}
fn paragraph(doc: &Document) -> NodeId {
    doc.descendants(doc.root())
        .find(|&n| local(doc, n) == "p")
        .unwrap()
}
fn selected_run(doc: &Document) -> NodeId {
    doc.descendants(doc.root())
        .find(|&n| {
            local(doc, n) == "r"
                && doc.children(n).any(|c| {
                    local(doc, c) == "t"
                        && doc
                            .children(c)
                            .any(|t| doc.node_text(t) == Some("selected"))
                })
        })
        .unwrap()
}
fn signature(doc: &Document, n: NodeId) -> Value {
    let attrs: std::collections::BTreeMap<_, _> = doc
        .attributes(n)
        .iter()
        .map(|a| {
            (
                format!("{}:{}", a.prefix.as_deref().unwrap_or(""), a.name),
                a.value.clone(),
            )
        })
        .collect();
    json!({"name":local(doc,n), "attrs":attrs, "text":doc.node_text(n), "children":doc.children(n).map(|c|signature(doc,c)).collect::<Vec<_>>()})
}
fn bag(doc: &Document, n: NodeId, excluded: &[&str]) -> Vec<Value> {
    doc.children(n)
        .filter(|&c| !excluded.contains(&local(doc, c)))
        .map(|c| signature(doc, c))
        .collect()
}
fn read(input: &[u8], view: &str) -> Value {
    match command(
        Some(input),
        Command::Read(ReadCommand {
            read_kind: Some("document_ui".into()),
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

#[test]
fn selection_edits_preserve_rich_run_properties_and_restore_complete_snapshots() {
    for underline in ["double", "dotted", "dash"] {
        let rich = RICH_RUN.replace("w:val=\"double\"", &format!("w:val=\"{underline}\""));
        let input = fixture(&rich, RICH_PARAGRAPH);
        let source = xml(&input);
        let original_run = source
            .descendants(source.root())
            .find(|&n| local(&source, n) == "r")
            .unwrap();
        let original_rpr = child(&source, original_run, "rPr").unwrap();
        for (requested, changed_property) in [
            (json!({"font_size":15.0}), "sz"),
            (json!({"color":"#123456"}), "color"),
        ] {
            let edited = commit(&input, vec![format(requested)]);
            let doc = xml(&edited);
            let rpr = child(&doc, selected_run(&doc), "rPr").unwrap();
            assert_eq!(
                bag(&doc, rpr, &[changed_property, "rPrChange"]),
                bag(&source, original_rpr, &[changed_property])
            );
            let snapshot = child(&doc, child(&doc, rpr, "rPrChange").unwrap(), "rPr").unwrap();
            assert_eq!(signature(&doc, snapshot), signature(&source, original_rpr));
            // Boundary splits copy the whole source property bag to untouched text.
            for run in doc
                .children(paragraph(&doc))
                .filter(|&n| local(&doc, n) == "r" && n != selected_run(&doc))
            {
                assert_eq!(
                    signature(&doc, child(&doc, run, "rPr").unwrap()),
                    signature(&source, original_rpr)
                );
            }
            for view in ["original", "final", "markup"] {
                assert!(read(&edited, view)["html"]
                    .as_str()
                    .unwrap()
                    .contains("selected"));
            }
            let rejected = xml(&settle(&edited, RevisionAction::Reject));
            for run in rejected
                .children(paragraph(&rejected))
                .filter(|&n| local(&rejected, n) == "r")
            {
                assert_eq!(
                    signature(&rejected, child(&rejected, run, "rPr").unwrap()),
                    signature(&source, original_rpr)
                );
            }
            let accepted = xml(&settle(&edited, RevisionAction::Accept));
            assert_eq!(
                bag(
                    &accepted,
                    child(&accepted, selected_run(&accepted), "rPr").unwrap(),
                    &[]
                ),
                bag(&doc, rpr, &["rPrChange"])
            );
            for (name, data) in parts(&input) {
                if name != "word/document.xml" {
                    assert_eq!(parts(&edited)[&name], data, "{name}");
                }
            }
        }
    }
}

#[test]
fn false_overrides_inherited_formatting_and_clear_restores_inheritance() {
    let input = fixture("", "");
    let props =
        json!({"bold":false,"italic":false,"underline":false,"strike":false,"superscript":false});
    let edited = commit(&input, vec![format(props)]);
    let doc = xml(&edited);
    let rpr = child(&doc, selected_run(&doc), "rPr").unwrap();
    for (name, value) in [
        ("b", "0"),
        ("i", "0"),
        ("u", "none"),
        ("strike", "0"),
        ("vertAlign", "baseline"),
    ] {
        let node = child(&doc, rpr, name).unwrap();
        assert_eq!(doc.attribute(node, "val"), Some(value));
    }
    let original = read(&edited, "original")["html"]
        .as_str()
        .unwrap()
        .to_owned();
    let final_html = read(&edited, "final")["html"].as_str().unwrap().to_owned();
    assert!(original.contains("<b>"), "{original}");
    assert!(final_html.contains("font-weight:normal"), "{final_html}");
    let accepted = settle(&edited, RevisionAction::Accept);
    // Clear wins over an explicit value, removes the local overrides, and
    // restores the effective default paragraph style on the selected text.
    let cleared = commit(
        &accepted,
        vec![format(
            json!({"bold":false,"clear":["bold","italic","underline","strike","subscript"]}),
        )],
    );
    let cleared_doc = xml(&cleared);
    let cleared_rpr = child(&cleared_doc, selected_run(&cleared_doc), "rPr").unwrap();
    for name in ["b", "i", "u", "strike", "vertAlign"] {
        assert!(child(&cleared_doc, cleared_rpr, name).is_none());
    }
    assert!(read(&cleared, "final")["html"]
        .as_str()
        .unwrap()
        .contains("<b>"));
    let restored = xml(&settle(&cleared, RevisionAction::Reject));
    let accepted_doc = xml(&accepted);
    assert_eq!(
        signature(
            &restored,
            child(&restored, selected_run(&restored), "rPr").unwrap()
        ),
        signature(
            &accepted_doc,
            child(&accepted_doc, selected_run(&accepted_doc), "rPr").unwrap()
        )
    );
    // Reapplying an explicit off value records no second revision.
    let noop = commit(&accepted, vec![format(json!({"bold":false}))]);
    assert!(
        !String::from_utf8(parts(&noop)["word/document.xml"].clone())
            .unwrap()
            .contains("rPrChange")
    );
}

#[test]
fn clear_removes_themed_color_and_size_and_inherits_style_values() {
    let input = fixture(RICH_RUN, "");
    let cleared = commit(&input, vec![format(json!({"clear":["color","font_size"]}))]);
    let doc = xml(&cleared);
    let rpr = child(&doc, selected_run(&doc), "rPr").unwrap();
    assert!(child(&doc, rpr, "color").is_none());
    assert!(child(&doc, rpr, "sz").is_none());
    let html = read(&cleared, "final")["html"].as_str().unwrap().to_owned();
    assert!(
        html.contains("color:#112233") && html.contains("font-size:14pt"),
        "{html}"
    );
    let rejected = xml(&settle(&cleared, RevisionAction::Reject));
    let source = xml(&input);
    let original_run = source
        .descendants(source.root())
        .find(|&n| local(&source, n) == "r")
        .unwrap();
    assert_eq!(
        signature(
            &rejected,
            child(&rejected, selected_run(&rejected), "rPr").unwrap()
        ),
        signature(&source, child(&source, original_run, "rPr").unwrap())
    );
}

#[test]
fn paragraph_edits_preserve_rich_properties_and_clear_inherits_alignment() {
    let input = fixture(RICH_RUN, RICH_PARAGRAPH);
    let source = xml(&input);
    let original = child(&source, paragraph(&source), "pPr").unwrap();
    let edited = commit(&input, vec![para_format(json!({"alignment":"center"}))]);
    let doc = xml(&edited);
    let ppr = child(&doc, paragraph(&doc), "pPr").unwrap();
    assert_eq!(
        bag(&doc, ppr, &["jc", "pPrChange"]),
        bag(&source, original, &["jc"])
    );
    let rejected = xml(&settle(&edited, RevisionAction::Reject));
    assert_eq!(
        signature(
            &rejected,
            child(&rejected, paragraph(&rejected), "pPr").unwrap()
        ),
        signature(&source, original)
    );
    let cleared = commit(&input, vec![para_format(json!({"clear":["alignment"]}))]);
    assert!(read(&cleared, "final")["html"]
        .as_str()
        .unwrap()
        .contains("text-align:right"));
    assert!(read(&cleared, "original")["html"]
        .as_str()
        .unwrap()
        .contains("text-align:left"));
    // Alter one attribute of a shared property element without losing the rest.
    let edited = commit(
        &input,
        vec![para_format(json!({"indent_left":36.0,"space_after":18.0}))],
    );
    let doc = xml(&edited);
    let ppr = child(&doc, paragraph(&doc), "pPr").unwrap();
    assert_eq!(
        doc.attribute(child(&doc, ppr, "ind").unwrap(), "hanging"),
        Some("180")
    );
    let spacing = child(&doc, ppr, "spacing").unwrap();
    for (name, value) in [
        ("before", "120"),
        ("line", "276"),
        ("lineRule", "auto"),
        ("beforeAutospacing", "0"),
    ] {
        assert_eq!(doc.attribute(spacing, name), Some(value));
    }
    assert_eq!(
        bag(&doc, ppr, &["ind", "spacing", "pPrChange"]),
        bag(&source, original, &["ind", "spacing"])
    );
}

#[test]
fn unsupported_table_formatting_is_rejected_without_output_and_structural_html_still_works() {
    for tag in ["table", "tbody", "thead", "tfoot", "tr", "td", "th"] {
        for attr in [
            "style='background-color:#FFFF00'",
            "style=''",
            "class='report'",
            "width='100'",
            "height='100'",
            "border='1'",
            "cellpadding='4'",
            "cellspacing='2'",
            "align='center'",
            "valign='middle'",
            "bgcolor='#FFFF00'",
        ] {
            let section = if ["thead", "tfoot"].contains(&tag) {
                tag
            } else {
                "tbody"
            };
            let cell = if tag == "th" { "th" } else { "td" };
            let attrs = |name: &str| {
                if name == tag {
                    format!(" {attr}")
                } else {
                    String::new()
                }
            };
            let html = format!(
                "<table{}><{section}{}><tr{}><{cell}{}>text</{cell}></tr></{section}></table>",
                attrs("table"),
                attrs(section),
                attrs("tr"),
                attrs(cell)
            );
            let result = command(
                None,
                Command::Create(CreateCommand {
                    html: Some(html.clone()),
                    paragraphs: vec![],
                }),
            );
            match result {
                CommandResult::Rejected { diagnostic, .. } => assert!(
                    diagnostic.message.contains("unsupported table formatting"),
                    "{html}: {diagnostic:?}"
                ),
                other => panic!("{html}: {other:?}"),
            }
        }
    }
    create("<table data-docx-table='1'><tr><td colspan='2'><p style='text-align:center'>styled cell text</p></td></tr><tr><td>one</td><td>two</td></tr></table>");
    // The diagnostic also survives the header plan route, which authors blocks.
    let input = fixture("", "");
    let plan = Plan {
        base: SourceHash::from_bytes(&input),
        author: "test".into(),
        change_mode: ChangeMode::Track,
        ops: vec![EditOp::SetHeader {
            section: None,
            kind: None,
            with: "<table style='width:100pt'><tr><td>text</td></tr></table>".into(),
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

#[test]
fn native_html_capabilities_match_the_published_matrix() {
    let matrix: Value = serde_json::from_str(include_str!(
        "../../../packages/docxdriver/schema/formatting-capabilities.json"
    ))
    .unwrap();
    for row in matrix["capabilities"].as_array().unwrap() {
        let Some(html) = row["html_example"].as_str() else {
            continue;
        };
        let result = command(
            None,
            Command::Create(CreateCommand {
                html: Some(html.into()),
                paragraphs: vec![],
            }),
        );
        if row["html_authoring"] == "unsupported" {
            match result {
                CommandResult::Rejected { diagnostic, .. } => assert!(
                    diagnostic.message.contains("unsupported")
                        || diagnostic.message.contains("out of range"),
                    "{}: {diagnostic:?}",
                    row["id"]
                ),
                other => panic!("{}: {other:?}", row["id"]),
            }
        } else {
            let bytes = match result {
                CommandResult::Completed {
                    bytes: Some(bytes), ..
                } => bytes,
                other => panic!("{}: {other:?}", row["id"]),
            };
            if let Some(css) = row["read_contains"].as_str() {
                let projected = read(&bytes, "final");
                assert!(
                    projected["html"].as_str().unwrap().contains(css),
                    "{}: {projected}",
                    row["id"]
                );
            }
        }
    }
}

#[test]
fn formatting_on_containers_without_a_style_mapping_is_explicitly_rejected() {
    for html in [
        "<div style='text-align:center'><p>text</p></div>",
        "<ol style='color:#123456'><li>text</li></ol>",
        "<ul><li style='font-weight:bold'>text</li></ul>",
        "<p><a href='https://example.com' style='color:#123456'>text</a></p>",
        "<p><span data-docx-field='PAGE' style='font-weight:bold'>1</span></p>",
        "<section style='margin-left:12pt'><p>text</p></section>",
    ] {
        match command(
            None,
            Command::Create(CreateCommand {
                html: Some(html.into()),
                paragraphs: vec![],
            }),
        ) {
            CommandResult::Rejected { diagnostic, .. } => assert!(
                diagnostic
                    .message
                    .contains("unsupported HTML style attribute"),
                "{html}: {diagnostic:?}"
            ),
            other => panic!("{html}: {other:?}"),
        }
    }
}

#[test]
fn equivalent_direct_boolean_values_and_absent_clears_record_no_revision() {
    for (properties, requested) in [
        ("<w:b/>", json!({"bold":true})),
        ("<w:b w:val='true'/>", json!({"bold":true})),
        ("<w:i w:val='false'/>", json!({"italic":false})),
        ("<w:strike w:val='off'/>", json!({"strike":false})),
        ("", json!({"clear":["bold","color"]})),
    ] {
        let input = fixture(properties, "");
        let edited = commit(&input, vec![format(requested)]);
        let doc = xml(&edited);
        assert!(!doc
            .descendants(doc.root())
            .any(|n| local(&doc, n) == "rPrChange"));
        let source = xml(&input);
        let original_run = source
            .descendants(source.root())
            .find(|&n| local(&source, n) == "r")
            .unwrap();
        for run in doc
            .children(paragraph(&doc))
            .filter(|&n| local(&doc, n) == "r")
        {
            assert_eq!(
                signature(&doc, child(&doc, run, "rPr").unwrap()),
                signature(&source, child(&source, original_run, "rPr").unwrap())
            );
        }
    }
}

#[test]
fn ooxml_off_values_override_inherited_bold_italic_and_strike_in_preview() {
    for off in ["0", "false", "off"] {
        let input = fixture(
            &format!("<w:b w:val='{off}'/><w:i w:val='{off}'/><w:strike w:val='{off}'/>"),
            "",
        );
        for view in ["final", "original", "markup"] {
            let projected = read(&input, view);
            let html = projected["html"].as_str().unwrap();
            for tag in ["<b>", "<i>", "<s>"] {
                assert!(!html.contains(tag), "{off}: {html}");
            }
            assert!(
                html.contains("font-weight:normal") && html.contains("font-style:normal"),
                "{off}: {html}"
            );
        }
    }
}

#[test]
fn paragraph_whitespace_hints_use_the_same_bounded_values_as_inline_input() {
    for value in ["nowrap", "invalid"] {
        let html = format!("<p style='white-space:{value}'>sample text</p>");
        match command(
            None,
            Command::Create(CreateCommand {
                html: Some(html),
                paragraphs: vec![],
            }),
        ) {
            CommandResult::Rejected { diagnostic, .. } => assert!(diagnostic
                .message
                .contains("unsupported paragraph CSS property white-space")),
            other => panic!("{other:?}"),
        }
    }
    for value in ["pre-wrap", "normal", "pre"] {
        create(&format!("<p style='white-space:{value}'>sample text</p>"));
    }
}

#[test]
fn rejecting_paragraph_formatting_restores_schema_order_on_imported_section_tails() {
    let section = "<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/></w:sectPr>";
    let imported = format!("{section}{}", RICH_PARAGRAPH.replace(section, ""));
    let input = fixture(RICH_RUN, &imported);
    let edited = commit(
        &input,
        vec![
            para_format(json!({"alignment":"center"})),
            EditOp::RevisionSettle {
                target: RevisionTarget::All,
                action: RevisionAction::Reject,
            },
        ],
    );
    let doc = xml(&edited);
    let ppr = child(&doc, paragraph(&doc), "pPr").unwrap();
    let names: Vec<_> = doc.children(ppr).map(|n| local(&doc, n)).collect();
    assert_eq!(&names[names.len() - 2..], &["rPr", "sectPr"]);
    let source = xml(&input);
    let original = child(&source, paragraph(&source), "pPr").unwrap();
    for name in ["rPr", "sectPr"] {
        assert_eq!(
            signature(&doc, child(&doc, ppr, name).unwrap()),
            signature(&source, child(&source, original, name).unwrap())
        );
    }
    assert_eq!(
        bag(&doc, ppr, &["rPr", "sectPr"]),
        bag(&source, original, &["rPr", "sectPr"])
    );
}
