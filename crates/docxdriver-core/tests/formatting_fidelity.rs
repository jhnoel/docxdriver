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
    commit_mode(input, ops, ChangeMode::Track)
}
fn commit_mode(input: &[u8], ops: Vec<EditOp>, mode: ChangeMode) -> Vec<u8> {
    let plan = Plan {
        base: SourceHash::from_bytes(input),
        author: "fidelity".into(),
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
            (json!({"highlight":"cyan"}), "highlight"),
            (json!({"highlight":"none"}), "highlight"),
            (json!({"clear":["highlight"]}), "highlight"),
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

#[test]
fn first_page_mode_tracks_properties_and_retains_first_stories_on_reject() {
    let input = fixture("", "");
    let input = commit(&input, vec![EditOp::SetHeader {
        section: None, kind: Some(ChromeKind::First), with: "<p>First story</p>".into(),
    }]);
    let before = xml(&input);
    let changed = commit(&input, vec![EditOp::SetFirstPageMode { section: None, enabled: false }]);
    let changed_xml = String::from_utf8(parts(&changed)["word/document.xml"].clone()).unwrap();
    assert!(changed_xml.contains("sectPrChange"));
    assert!(changed_xml.contains("headerReference"));
    assert!(changed_xml.contains("titlePg w:val=\"0\""));
    let restored = settle(&changed, RevisionAction::Reject);
    let restored_xml = xml(&restored);
    assert_eq!(signature(&restored_xml, restored_xml.root()), signature(&before, before.root()));
    assert!(parts(&restored).iter().any(|(name, bytes)| name.contains("header") && String::from_utf8_lossy(bytes).contains("First story")));
    let accepted = settle(&changed, RevisionAction::Accept);
    let enabled = commit(&accepted, vec![EditOp::SetFirstPageMode { section: None, enabled: true }]);
    assert!(String::from_utf8_lossy(&parts(&enabled)["word/document.xml"]).contains("titlePg w:val=\"1\""));
}

#[test]
fn highlight_palette_html_and_inheritance() {
    for (value, css) in [("yellow", "#FFFF00"), ("darkBlue", "#000080"), ("none", "transparent")] {
        let input = create(&format!("<p><mark data-highlight=\"{value}\">sample</mark></p>"));
        let raw = String::from_utf8(parts(&input)["word/document.xml"].clone()).unwrap();
        assert!(raw.contains(&format!("w:highlight w:val=\"{value}\"")), "{raw}");
        let rendered = read(&input, "final").to_string();
        assert!(rendered.contains("data-docx-highlight"), "{rendered}");
        assert!(rendered.contains(css), "{rendered}");
    }
    let input = create("<p><mark>default yellow</mark></p>");
    assert!(read(&input, "final").to_string().contains("#FFFF00"));
    let input = fixture("", "");
    let mut entries = parts(&input);
    entries.insert("word/styles.xml".into(), format!(r#"<w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr><w:highlight w:val="green"/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>"#).into_bytes());
    let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
    for (name, bytes) in entries { zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap(); zip.write_all(&bytes).unwrap(); }
    let inherited = zip.finish().unwrap().into_inner();
    assert!(read(&inherited, "final").to_string().contains("#00FF00"));
    let disabled = commit(&inherited, vec![format(json!({"highlight":"none"}))]);
    assert!(read(&disabled, "final").to_string().contains("transparent"));
    let restored = settle(&disabled, RevisionAction::Reject);
    assert!(read(&restored, "final").to_string().contains("#00FF00"));
}

#[test]
fn highlight_rejects_unknown_palette_and_rgb_shading() {
    assert!(serde_json::from_value::<EditOp>(json!({"op":"format_text","at":AT,"select":"selected","highlight":"#FFFF00"})).is_err());
    for html in ["<p><mark data-highlight=\"purple\">bad</mark></p>", "<p><span style=\"background-color:#FFFF00\">bad</span></p>"] {
        assert!(!matches!(command(None, Command::Create(CreateCommand { html: Some(html.into()), paragraphs: vec![] })), CommandResult::Completed { bytes: Some(_), .. }));
    }
}

#[test]
fn list_membership_preserves_rich_properties_and_reject_restores_reference() {
    let input = fixture(RICH_RUN, RICH_PARAGRAPH);
    let op = |extra: Value| { let mut v = json!({"op":"set_list","at":[AT]}); v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone()); serde_json::from_value::<EditOp>(v).unwrap() };
    let edited = commit(&input, vec![op(json!({"format":"decimal","start":4,"level":2}))]);
    let edited_xml = String::from_utf8(parts(&edited)["word/document.xml"].clone()).unwrap();
    assert!(edited_xml.contains("w:numPr"));
    assert!(edited_xml.contains("w:pPrChange"));
    assert!(edited_xml.contains("w:keepNext"));
    assert!(edited_xml.contains("w:hanging=\"180\""));
    assert!(edited_xml.contains("before selected after"));
    let rejected = settle(&edited, RevisionAction::Reject);
    let rejected_xml = String::from_utf8(parts(&rejected)["word/document.xml"].clone()).unwrap();
    assert!(!rejected_xml.contains("w:numPr"));
    let accepted = settle(&edited, RevisionAction::Accept);
    let restarted = commit(&accepted,vec![op(json!({"num_id":1,"start":7,"level":2}))]);
    let ns = String::from_utf8(parts(&restarted)["word/numbering.xml"].clone()).unwrap();
    assert!(ns.contains("startOverride"));
    assert!(ns.contains("w:numId=\"2\""));
    let accepted = settle(&restarted, RevisionAction::Accept);
    let removed = commit(&accepted,vec![op(json!({"remove":true}))]);
    let removed_xml = String::from_utf8(parts(&removed)["word/document.xml"].clone()).unwrap();
    assert!(removed_xml.contains("<w:numId w:val=\"0\""));
}

#[test]
fn section_layout_snapshots_preserve_stories_and_unrequested_properties() {
    let input = fixture("", "");
    let input = commit(&input, vec![EditOp::SetFooter { section: None, kind: None, with: "<p>Footer</p>".into() }]);
    let before = xml(&input);
    let op: EditOp = serde_json::from_value(json!({"op":"set_section_layout", "page_size":"a4", "orientation":"landscape", "columns":2, "column_spacing":18, "column_separator":true, "break_type":"continuous", "page_number_format":"lowerRoman", "page_number_start":3})).unwrap();
    let changed = commit(&input, vec![op]);
    let data = String::from_utf8(parts(&changed)["word/document.xml"].clone()).unwrap();
    for expected in ["w:w=\"16838\"", "w:h=\"11906\"", "w:orient=\"landscape\"", "w:num=\"2\"", "w:space=\"360\"", "w:start=\"3\""] { assert!(data.contains(expected), "{data}"); }
    let projected = read(&changed, "final").to_string();
    assert!(projected.contains("data-docx-orientation"));
    let rejected = settle(&changed, RevisionAction::Reject);
    let restored = xml(&rejected);
    assert_eq!(signature(&restored, restored.root()), signature(&before, before.root()));
    let accepted = settle(&changed, RevisionAction::Accept);
    let op: EditOp = serde_json::from_value(json!({"op":"set_section_layout", "column_widths":[144,216], "column_spacing":12, "page_number_continue":true})).unwrap();
    let unequal = commit(&accepted, vec![op]);
    let data = String::from_utf8(parts(&unequal)["word/document.xml"].clone()).unwrap();
    assert!(data.contains("equalWidth=\"0\""));
    let accepted = settle(&unequal, RevisionAction::Accept);
    let accepted_doc = xml(&accepted);
    let pg_num = accepted_doc.descendants(accepted_doc.root()).find(|&n|local(&accepted_doc,n)=="pgNumType").unwrap();
    assert!(!accepted_doc.attributes(pg_num).iter().any(|a| a.name=="start"));
}

#[test]
fn table_formats_preserve_content_merge_grid_and_reject_snapshots() {
    let input=create("<table><tr><td rowspan=\"2\"><p>merged</p></td><td><p>neighbor</p><table><tr><td>nested</td></tr></table></td></tr><tr><td>last</td></tr></table>");
    let ops=vec![
        json!({"op":"format_table","at":"table:1","format":{"width":{"unit":"percent","value":80},"column_widths":[120,180],"layout":"fixed","alignment":"center","padding":{"top":3},"borders":{"top":{"style":"double","color":"#123456","width":1}}}}),
        json!({"op":"format_row","at":"table:1/row:1","format":{"height":24,"height_rule":"at_least","repeating_header":true,"allow_split":false}}),
        json!({"op":"format_cell","at":"table:1/row:1/cell:1","format":{"fill":"#ABCDEF","vertical_alignment":"center","padding":{"left":6},"borders":{"bottom":{"style":"single","width":0.5}}}}),
    ].into_iter().map(|v|serde_json::from_value(v).unwrap()).collect();
    let output=commit(&input,ops);let raw=String::from_utf8(parts(&output)["word/document.xml"].clone()).unwrap();
    assert_eq!(raw.matches("w:fill=\"ABCDEF\"").count(),2,"{raw}");
    assert!(raw.contains("w:tblPrChange")&&raw.contains("w:trPrChange")&&raw.contains("w:tcPrChange")&&raw.contains("w:tblGridChange"),"{raw}");
    let final_view=read(&output,"final").to_string();
    assert!(final_view.contains("#ABCDEF")&&final_view.contains("width:80%")&&final_view.contains("nested"),"{final_view}");
    let original_view=read(&output,"original").to_string();assert!(!original_view.contains("#ABCDEF")&&!original_view.contains("width:80%"),"{original_view}");
    let inventory=read(&output,"final");assert_eq!(inventory["tables"].as_array().unwrap().len(),2);assert_eq!(inventory["tables"][0]["rows"][1]["cells"][0]["logical"],false);
    let rejected=settle(&output,RevisionAction::Reject);
    let src=xml(&input);let rejected=xml(&rejected);let table=|d:&Document| d.descendants(d.root()).find(|&n|local(d,n)=="tbl").unwrap();
    assert_eq!(signature(&src,table(&src)),signature(&rejected,table(&rejected)));
    let accepted=settle(&output,RevisionAction::Accept);assert!(read(&accepted,"final").to_string().contains("#ABCDEF"));
}

#[test]
fn table_html_data_format_uses_the_typed_contract() {
    let input=create(r##"<table data-format='{"width":{"unit":"percent","value":75},"layout":"fixed"}'><tr data-format='{"repeating_header":true}'><td data-format='{"fill":"#112233","padding":{"top":4}}'>sample</td></tr></table>"##);
    let rendered=read(&input,"final").to_string();assert!(rendered.contains("width:75%")&&rendered.contains("#112233")&&rendered.contains("padding-top:4pt"),"{rendered}");
    let raw=String::from_utf8(parts(&input)["word/document.xml"].clone()).unwrap();assert!(raw.contains("w:tblHeader")&&!raw.contains("PrChange"),"{raw}");
}

#[test]
fn table_source_handles_and_inserted_aliases_survive_shifted_positions() {
    let input = fixture("", "");
    let input = commit_mode(&input, vec![serde_json::from_value(json!({"op":"insert_table","at":AT,"position":"after","rows":2,"columns":2})).unwrap()], ChangeMode::Direct);
    let ops = vec![
        json!({"op":"insert_table","at":AT,"position":"before","rows":1,"columns":1,"as":"new"}),
        json!({"op":"format_cell","at":"$new/row:1/cell:1","format":{"fill":"#FF0000"}}),
        json!({"op":"format_cell","at":"table:1/row:2/cell:2","format":{"fill":"#00FF00"}}),
        json!({"op":"edit_table","at":"table:1","action":{"kind":"insert_rows","row":1,"position":"before","count":1}}),
        json!({"op":"format_cell","at":"table:1/row:1/cell:1","format":{"fill":"#0000FF"}}),
    ];
    let changed = commit_mode(&input, ops.into_iter().map(|v|serde_json::from_value(v).unwrap()).collect(), ChangeMode::Direct);
    let doc = xml(&changed);
    let tables: Vec<_> = doc.descendants(doc.root()).filter(|&n|local(&doc,n)=="tbl").collect();
    assert_eq!(tables.len(),2);
    let fills = |t| doc.descendants(t).filter(|&n|local(&doc,n)=="shd").flat_map(|n|doc.attributes(n).iter().filter(|a|a.name=="fill").map(|a|a.value.clone())).collect::<Vec<_>>();
    assert_eq!(fills(tables[0]),vec!["FF0000"]);
    assert_eq!(fills(tables[1]),vec!["0000FF","00FF00"]);
}

fn direct_commit(input: &[u8], ops: Vec<EditOp>) -> Vec<u8> {
    let plan = Plan { base: SourceHash::from_bytes(input), author:"grid".into(), change_mode: ChangeMode::Direct, ops };
    let request=PlanRequest { plan:plan.clone(), preview_key:None };
    let preview=execute_plan(input,&request);
    let key=match preview {PlanResult::Previewed{preview_key,..}=>preview_key,other=>panic!("{other:?}")};
    match execute_plan(input,&PlanRequest{plan,preview_key:Some(key)}){PlanResult::Committed{bytes,..}=>bytes,other=>panic!("{other:?}")}
}
#[test]
fn table_grid_insert_merge_split_and_delete_preserve_required_structure() {
    let input=fixture("", "");
    let operation = |v:Value| serde_json::from_value::<EditOp>(v).unwrap();
    let inserted=direct_commit(&input,vec![operation(json!({"op":"insert_table","at":AT,"position":"before","rows":2,"columns":2}))]);
    let merged=direct_commit(&inserted,vec![operation(json!({"op":"edit_table","at":"table:1","action":{"kind":"merge_cells","row":1,"column":1,"row_span":2,"column_span":2}}))]);
    let raw=String::from_utf8(parts(&merged)["word/document.xml"].clone()).unwrap();
    assert_eq!(raw.matches("<w:tc>").count(),2);
    assert_eq!(raw.matches("w:gridSpan").count(),2);
    assert!(raw.contains("w:vMerge w:val=\"restart\""));
    let split=direct_commit(&merged,vec![operation(json!({"op":"edit_table","at":"table:1","action":{"kind":"split_cell","row":1,"column":1}}))]);
    let raw=String::from_utf8(parts(&split)["word/document.xml"].clone()).unwrap();
    assert_eq!(raw.matches("<w:tc>").count(),4);
    assert!(!raw.contains("w:vMerge"));
    let expanded=direct_commit(&split,vec![operation(json!({"op":"edit_table","at":"table:1","action":{"kind":"insert_columns","column":1,"position":"after","count":1}})),operation(json!({"op":"edit_table","at":"table:1","action":{"kind":"insert_rows","row":1,"position":"after","count":1}}))]);
    let raw=String::from_utf8(parts(&expanded)["word/document.xml"].clone()).unwrap();
    assert_eq!(raw.matches("<w:tc>").count(),9);
    assert_eq!(raw.matches("<w:gridCol ").count(),3);
    let shrunk=direct_commit(&expanded,vec![operation(json!({"op":"edit_table","at":"table:1","action":{"kind":"delete_columns","column":2,"count":1}})),operation(json!({"op":"edit_table","at":"table:1","action":{"kind":"delete_rows","row":2,"count":1}}))]);
    let raw=String::from_utf8(parts(&shrunk)["word/document.xml"].clone()).unwrap();
    assert_eq!(raw.matches("<w:tc>").count(),4);
    let deleted=direct_commit(&shrunk,vec![operation(json!({"op":"edit_table","at":"table:1","action":{"kind":"delete"}}))]);
    let raw=String::from_utf8(parts(&deleted)["word/document.xml"].clone()).unwrap();
    assert!(!raw.contains("<w:tbl>"));
    assert!(raw.contains("before selected after"));
}
