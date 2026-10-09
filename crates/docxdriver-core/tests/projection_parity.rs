//! Semantic parity against the actual output of the prior committed engine.
//! The legacy snapshot is generated from 0e5a0f9c, not from this implementation.
use docxdriver_core::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use xmloxide::tree::{Document, NodeId, NodeKind};
const INPUT: &[u8] = include_bytes!("fixtures/projection-parity/parity.docx");
const PRIOR: &str = include_str!("fixtures/projection-parity/prior-projection.json");
fn read(view: &str) -> Value {
    match execute_command(
        Some(INPUT),
        &CommandRequest {
            command: Command::Read(ReadCommand {
                read_kind: Some("document_ui".into()),
                view: Some(view.into()),
            }),
            expected_source: None,
        },
    ) {
        CommandResult::Completed {
            result: Some(v), ..
        } => v,
        other => panic!("{other:?}"),
    }
}
fn name(d: &Document, n: NodeId) -> &str {
    d.node_name(n)
        .unwrap_or("")
        .rsplit(':')
        .next()
        .unwrap_or("")
}
fn attr<'a>(d: &'a Document, n: NodeId, a: &str, b: &str) -> Option<&'a str> {
    d.attribute(n, a).or_else(|| d.attribute(n, b))
}
fn parse(markup: &str, modern: bool) -> Document {
    let mut s = markup.to_string();
    if modern {
        let re = regex_lite::Regex::new(r"<(br|img)(\s[^<>]*?)?>").unwrap();
        s = re.replace_all(&s, "<$1$2/>").into_owned();
        s = s.replace(" hidden>", " hidden=\"true\">");
    } else {
        for flag in ["pending", "first-page", "first", "even"] {
            s = s.replace(&format!(" {flag}>"), &format!(" {flag}=\"true\">"));
            s = s.replace(&format!(" {flag}/>"), &format!(" {flag}=\"true\"/>"));
            s = s.replace(&format!(" {flag} "), &format!(" {flag}=\"true\" "));
        }
    }
    Document::parse_str(&format!("<root>{s}</root>")).unwrap_or_else(|e| panic!("{e}: {s}"))
}
fn text(d: &Document, n: NodeId) -> String {
    if matches!(
        name(d, n),
        "equation" | "math" | "footnote" | "endnote" | "image" | "img"
    ) || d.attribute(n, "data-docx-note").is_some()
        || d.attribute(n, "data-docx-image-unavailable").is_some()
        || d.attribute(n, "hidden").is_some()
    {
        return String::new();
    }
    if matches!(
        d.node(n).kind,
        NodeKind::Text { .. } | NodeKind::CData { .. }
    ) {
        return d.node_text(n).unwrap_or("").into();
    }
    d.children(n).map(|c| text(d, c)).collect()
}
fn normalized(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn element_nodes(d: &Document) -> impl Iterator<Item = NodeId> + '_ {
    d.descendants(d.root()).filter(|&n| d.is_element(n))
}
fn collect(markup: &str, modern: bool) -> Value {
    let d = parse(markup, modern);
    let mut paragraphs = Vec::new();
    let mut links = Vec::new();
    let mut images = Vec::new();
    let mut breaks = Vec::new();
    let mut comments = Vec::new();
    let mut notes = Vec::new();
    let mut chrome = Vec::new();
    let mut tables = Vec::new();
    let mut sections = Vec::new();
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    let mut revisions: BTreeMap<(String, String, String), String> = BTreeMap::new();
    let mut pending: BTreeMap<(String, String), String> = BTreeMap::new();
    for n in element_nodes(&d) {
        let tag = name(&d, n);
        if matches!(tag, "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
            && d.attribute(n, "id").is_some()
        {
            paragraphs.push(json!({"id":d.attribute(n,"id"),"ord":attr(&d,n,"data-docx-ord","ord"),"tag":tag,"style":attr(&d,n,"data-docx-style","class").map(str::to_owned).or_else(|| tag.strip_prefix('h').filter(|v| v.len() == 1 && matches!(*v, "1" | "2" | "3" | "4" | "5" | "6")).map(|v| format!("Heading{v}"))),"number":attr(&d,n,"data-docx-number","num"),"break_ins":attr(&d,n,"data-docx-break-ins","break-ins"),"break_del":attr(&d,n,"data-docx-break-del","break-del"),"text":text(&d,n)}));
        }
        if tag == "a"
            && !d
                .parent(n)
                .is_some_and(|p| d.attribute(p, "data-docx-note").is_some())
        {
            links.push(json!({"href":attr(&d,n,"data-docx-href","href"),"text":text(&d,n)}));
        }
        if tag == "field" || d.attribute(n, "data-docx-field").is_some() {
            let key = attr(&d, n, "data-docx-field", "instr")
                .unwrap_or("")
                .to_owned();
            fields.entry(key).or_default().push_str(&text(&d, n));
        }
        if matches!(tag, "ins" | "del") {
            let key = (
                tag.to_owned(),
                attr(&d, n, "data-docx-revision", "id")
                    .unwrap_or("")
                    .to_owned(),
                attr(&d, n, "data-docx-author", "author")
                    .unwrap_or("")
                    .to_owned(),
            );
            revisions.entry(key).or_default().push_str(&text(&d, n));
        }
        if d.attribute(n, "pending").is_some()
            || d.attribute(n, "data-docx-format-change").is_some()
        {
            let key = (
                attr(&d, n, "data-docx-revision", "id")
                    .unwrap_or("")
                    .to_owned(),
                attr(&d, n, "data-docx-author", "author")
                    .unwrap_or("")
                    .to_owned(),
            );
            pending.entry(key).or_default().push_str(&text(&d, n));
        }
        if matches!(tag, "image" | "img") || d.attribute(n, "data-docx-image-unavailable").is_some()
        {
            images.push(json!({"width":attr(&d,n,"width","w").or_else(||d.attribute(n,"data-docx-image-width")),"height":attr(&d,n,"height","h").or_else(||d.attribute(n,"data-docx-image-height")),"alt":d.attribute(n,"alt").or_else(||d.attribute(n,"data-docx-image-alt"))}));
        }
        if tag == "br" {
            breaks.push(
                attr(&d, n, "data-docx-break", "type")
                    .unwrap_or("line")
                    .to_owned(),
            );
        }
        for (legacy, attribute) in [
            ("comment-start", "data-docx-comment-start"),
            ("comment-end", "data-docx-comment-end"),
        ] {
            if tag == legacy || d.attribute(n, attribute).is_some() {
                comments.push(json!({"kind":legacy,"id":attr(&d,n,attribute,"id")}));
            }
        }
        if matches!(tag, "footnote" | "endnote") || d.attribute(n, "data-docx-note-body").is_some()
        {
            let kind = d.attribute(n, "data-docx-note-body").unwrap_or(tag);
            // Ignore the reference-site representation and compare note bodies.
            let body = d.children(n).map(|c| text(&d, c)).collect::<String>();
            notes.push(json!({"kind":kind,"text":normalized(&body)}));
        }
        if matches!(tag, "header" | "footer") {
            chrome.push(json!({"kind":tag,"slot":d.attribute(n,"data-docx-kind").unwrap_or(if d.attribute(n,"first").is_some(){"first"}else if d.attribute(n,"even").is_some(){"even"}else{"default"}),"text":normalized(&text(&d,n))}));
        }
        if tag == "section" {
            sections.push(
                d.attribute(n, "first-page").is_some()
                    || d.attribute(n, "data-docx-first-page") == Some("true"),
            );
        }
        if tag == "table" {
            let cells:Vec<_>=d.descendants(n).filter(|&c|name(&d,c)=="td" && nearest_table(&d,c)==Some(n)).map(|c|json!({"colspan":d.attribute(c,"colspan"),"rowspan":d.attribute(c,"rowspan"),"text":normalized(&text(&d,c))})).collect();
            tables.push(json!({"number":attr(&d,n,"data-docx-table","n"),"cells":cells}));
        }
    }
    notes.sort_by_key(Value::to_string);
    // Notes moved from reference-site expansions to linked appendices.
    // Compare table inventory, while preserving cell order and body anchors.
    tables.sort_by_key(Value::to_string);
    let revisions: Vec<_> = revisions
        .into_iter()
        .map(|((kind, id, author), text)| json!({"kind":kind,"id":id,"author":author,"text":text}))
        .collect();
    let pending: Vec<_> = pending
        .into_iter()
        .map(|((id, author), text)| json!({"id":id,"author":author,"text":text}))
        .collect();
    json!({"paragraphs":paragraphs,"links":links,"fields":fields,"revisions":revisions,"pending_format":pending,"images":images,"breaks":breaks,"comments":comments,"notes":notes,"chrome":chrome,"tables":tables,"sections":sections})
}
fn nearest_table(d: &Document, n: NodeId) -> Option<NodeId> {
    let mut p = d.parent(n);
    while let Some(n) = p {
        if name(d, n) == "table" {
            return Some(n);
        }
        p = d.parent(n);
    }
    None
}

#[test]
fn compact_default_preserves_the_entire_document_in_every_view() {
    for view in ["markup", "final", "original"] {
        let ui = read(view);
        let model = match execute_command(
            Some(INPUT),
            &CommandRequest {
                command: Command::Read(ReadCommand {
                    read_kind: None,
                    view: Some(view.into()),
                }),
                expected_source: None,
            },
        ) {
            CommandResult::Completed {
                result: Some(v), ..
            } => v,
            other => panic!("{other:?}"),
        };
        for field in ["html", "source_map", "blocks", "paragraphs"] {
            assert!(model.get(field).is_none(), "model response repeats {field}");
        }
        assert!(model["css"].is_object());
        assert!(model["styles"].is_object());
        assert!(!model["markup"].as_str().unwrap().contains(" style="));
        assert!(!model["markup"].as_str().unwrap().contains("<style"));
        let mut expected = collect(ui["html"].as_str().unwrap(), true);
        let mut actual = collect(model["markup"].as_str().unwrap(), true);
        // Position is implicit in document order; IDs remain the edit addresses.
        for inventory in [&mut expected, &mut actual] {
            for p in inventory["paragraphs"].as_array_mut().unwrap() {
                p.as_object_mut().unwrap().remove("ord");
            }
        }
        assert_eq!(expected, actual, "{view}: lost complete-document context");
        let descriptors: Vec<Value> = ui["equations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let mut e = e.clone();
                e.as_object_mut().unwrap().remove("mathml");
                e
            })
            .collect();
        assert_eq!(model["equations"], json!(descriptors));
        assert!(model["markup"].as_str().unwrap().len() < ui["html"].as_str().unwrap().len());
        assert!(serde_json::to_vec(&model).unwrap().len() < serde_json::to_vec(&ui).unwrap().len());
        assert_eq!(model["source"], ui["source"]);
        let selections = model["selections"]["paragraphs"].as_array().unwrap();
        assert!(!selections.is_empty());
        for selection in selections {
            let paragraph = ui["paragraphs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["id"] == selection["at"])
                .unwrap();
            assert_eq!(selection["selectable_text"], paragraph["selectable_text"]);
        }
        assert!(!model["comments"].as_array().unwrap().is_empty());
        for asset in model["assets"].as_array().unwrap() {
            assert!(model["markup"]
                .as_str()
                .unwrap()
                .contains(asset["url"].as_str().unwrap()));
            assert!(!asset["parts"].as_array().unwrap().is_empty());
            assert!(asset.get("base64").is_none());
        }
    }
}
#[test]
fn prior_projection_semantics_survive_all_revision_views() {
    let prior: Value = serde_json::from_str(PRIOR).unwrap();
    for view in ["markup", "final", "original"] {
        let old = &prior[view];
        let new = read(view);
        for field in ["source", "view", "kind", "idRepairsNeeded"] {
            assert_eq!(old[field], new[field], "{view}: {field}");
        }
        // New block boundaries may group a list, but every old source paragraph
        // remains represented and every emitted address resolves canonically.
        let block_ids = |v: &Value| {
            v["blocks"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|b| b["paragraphs"].as_array().unwrap().iter())
                .cloned()
                .collect::<Vec<_>>()
        };
        assert!(block_ids(&new).len() >= block_ids(old).len());
        for id in block_ids(&new) {
            assert!(
                new["paragraphs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["id"] == id),
                "unresolvable block paragraph: {id}"
            );
        }
        let before = collect(old["markup"].as_str().unwrap(), false);
        let after = collect(new["html"].as_str().unwrap(), true);
        for feature in [
            "paragraphs",
            "links",
            "fields",
            "revisions",
            "pending_format",
            "images",
            "breaks",
            "comments",
            "notes",
            "chrome",
            "tables",
            "sections",
        ] {
            if matches!(feature, "notes" | "tables") {
                // The old final/original views suppressed notes entirely;
                // retaining their bodies (and note tables) is an improvement.
                let mut remaining = after[feature].as_array().unwrap().clone();
                for old in before[feature].as_array().unwrap() {
                    let at = remaining
                        .iter()
                        .position(|v| v == old)
                        .unwrap_or_else(|| panic!("{view}: missing {feature}: {old}"));
                    remaining.remove(at);
                }
            } else {
                assert_eq!(before[feature], after[feature], "{view}: {feature}");
            }
        }
        // Existing structured equation selectors retain addressing and layout.
        let descriptors = |v: &Value| {
            v["equations"].as_array().unwrap().iter().map(|e|json!({"at":e["at"],"equation":e["equation"],"display":e["display"],"editable":e["editable"]})).collect::<Vec<_>>()
        };
        assert_eq!(
            descriptors(old),
            descriptors(&new),
            "{view}: equation descriptors"
        );
    }
}
#[test]
fn every_prior_equation_structure_has_native_mathml_without_placeholder_loss() {
    let v = read("markup");
    let equations = v["equations"].as_array().unwrap();
    let manifest: Value =
        serde_json::from_str(include_str!("fixtures/projection-parity/features.json")).unwrap();
    assert_eq!(
        equations.len(),
        manifest["equations"].as_array().unwrap().len()
    );
    let expected = [
        ("sup", "<msup>"),
        ("sub", "<msub>"),
        ("subsup", "<msubsup>"),
        ("prescripts", "<mmultiscripts>"),
        ("fraction", "<mfrac>"),
        ("nobar", "linethickness=\"0\""),
        ("skew", "bevelled=\"true\""),
        ("sqrt", "<msqrt>"),
        ("root", "<mroot>"),
        ("nary", "<mover>"),
        ("limlow", "<munder accentunder=\"false\">"),
        ("limupp", "<mover accent=\"false\">"),
        ("function", "\u{2061}"),
        ("accent", ">→<"),
        ("bar", ">¯<"),
        ("underbar", "accentunder=\"true\""),
        ("group-default", ">⏞<"),
        ("underbrace", ">⏟<"),
        ("delimiters", ">;<"),
        ("matrix", "<mtable>"),
        ("equation-array", "<mtable>"),
        ("box", ">x<"),
        ("borderbox", "<menclose"),
        ("phantom", "<mphantom>"),
        ("deleted-run", ">Old math<"),
        ("unknown", "data-docx-unsupported=\"unknown\""),
        ("display", "display=\"block\""),
    ];
    for (index, label) in manifest["equations"].as_array().unwrap().iter().enumerate() {
        let label = label.as_str().unwrap();
        let math = equations[index]["mathml"].as_str().unwrap();
        if label != "unknown" {
            assert!(!math.contains("data-docx-unsupported"), "{label}: {math}");
        }
        if let Some((_, fragment)) = expected.iter().find(|(name, _)| *name == label) {
            assert!(math.contains(fragment), "{label}: {math}");
        }
        if label == "runs" {
            for token in [
                "<mi>α</mi>",
                "<mi>β</mi>",
                "<mo>+</mo>",
                "<mo mathvariant=\"normal\">sin</mo>",
            ] {
                assert!(math.contains(token), "{math}");
            }
        }
        if label == "limlow" {
            assert!(
                math.contains("<mo mathvariant=\"normal\">lim</mo>"),
                "{math}"
            );
        }
        if label == "nary" {
            assert!(!math.contains("HIDDEN"), "{math}");
        }
    }
}
#[test]
fn permanent_inline_formatting_retains_every_prior_character_format() {
    let prior: Value = serde_json::from_str(PRIOR).unwrap();
    let new = read("markup");
    fn bits(d: &Document, n: NodeId, mut format: u8, out: &mut Vec<(char, u8)>) {
        format |= match name(d, n) {
            "b" => 1,
            "i" => 2,
            "u" => 4,
            "s" => 8,
            "sup" => 16,
            "sub" => 32,
            _ => 0,
        };
        if matches!(d.node(n).kind, NodeKind::Text { .. }) {
            out.extend(d.node_text(n).unwrap().chars().map(|c| (c, format)));
        }
        for c in d.children(n) {
            bits(d, c, format, out);
        }
    }
    let formats = |markup: &str, modern| {
        let d = parse(markup, modern);
        let p = element_nodes(&d)
            .find(|&n| d.attribute(n, "id") == Some("11111111"))
            .unwrap();
        let mut out = Vec::new();
        bits(&d, p, 0, &mut out);
        out
    };
    assert_eq!(
        formats(prior["markup"]["markup"].as_str().unwrap(), false),
        formats(new["html"].as_str().unwrap(), true)
    );
}

#[test]
fn compact_comment_records_preserve_thread_content_and_ready_to_use_selectors() {
    let command = |kind: &str| match execute_command(
        Some(INPUT),
        &CommandRequest {
            command: Command::Read(ReadCommand {
                read_kind: Some(kind.into()),
                view: Some("markup".into()),
            }),
            expected_source: None,
        },
    ) {
        CommandResult::Completed {
            result: Some(v), ..
        } => v,
        other => panic!("{other:?}"),
    };
    let detailed = command("comments");
    let compact = command("document");
    let ui = read("final");
    assert_eq!(
        compact["selection_space"],
        json!({"view":"final","offset_unit":"unicode_scalar"})
    );
    assert_eq!(
        compact["comments"].as_array().unwrap().len(),
        detailed["comments"].as_array().unwrap().len()
    );
    for thread in compact["comments"].as_array().unwrap() {
        let original = detailed["comments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == thread["id"])
            .unwrap();
        for key in ["id", "status", "durableId"] {
            assert_eq!(thread[key], original[key]);
        }
        for key in ["text", "author", "date", "paraId"] {
            assert_eq!(thread["root"][key], original["root"][key]);
        }
        assert_eq!(
            thread["replies"].as_array().unwrap().len(),
            original["replies"].as_array().unwrap().len()
        );
        for (reply, prior) in thread["replies"]
            .as_array()
            .unwrap()
            .iter()
            .zip(original["replies"].as_array().unwrap())
        {
            for key in ["id", "text", "author", "date", "paraId", "durableId"] {
                assert_eq!(reply[key], prior[key]);
            }
        }
        assert!(thread.get("createdAt").is_none() && thread["root"].get("threadId").is_none());
        for range in thread["anchor"]["ranges"].as_array().unwrap() {
            let p = ui["paragraphs"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["id"] == range["at"])
                .unwrap();
            let text = p["selectable_text"].as_str().unwrap();
            let start = range["start"].as_u64().unwrap() as usize;
            let end = range["end"].as_u64().unwrap() as usize;
            assert_eq!(
                text.chars()
                    .skip(start)
                    .take(end - start)
                    .collect::<String>(),
                range["select"].as_str().unwrap()
            );
            if let Some(occurrence) = range["occurrence"].as_u64() {
                let (byte, _) = text
                    .match_indices(range["select"].as_str().unwrap())
                    .nth(occurrence as usize - 1)
                    .unwrap();
                assert_eq!(text[..byte].chars().count(), start);
            }
            assert!(range.get("locator").is_none());
        }
    }
}
