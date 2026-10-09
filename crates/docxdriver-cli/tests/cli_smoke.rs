use std::path::PathBuf;
use std::process::Command;

fn bin() -> PathBuf {
    env!("CARGO_BIN_EXE_docxdriver").into()
}
fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(bin())
        .args(args)
        .output()
        .expect("spawn docxdriver");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}
fn dir() -> tempfile::TempDir {
    tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap()
}
fn create_doc(dir: &tempfile::TempDir) -> PathBuf {
    let path = dir.path().join("doc.docx");
    let (code, out, err) = run(&[
        "create",
        path.to_str().unwrap(),
        "--html",
        "<p>Hello world</p>",
    ]);
    assert_eq!(code, 0, "{err}\n{out}");
    path
}
fn read_meta(path: &PathBuf) -> (String, String) {
    let (code, out, err) = run(&["--json", "read", path.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}\n{out}");
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    (
        v["result"]["markup"].as_str().unwrap().split_once("id=\"").unwrap().1.split_once('"').unwrap().0.into(),
        v["source"].as_str().unwrap().into(),
    )
}

#[test]
fn root_surface_is_typed_only() {
    let (code, out, err) = run(&["--help"]);
    assert_eq!(code, 0, "{err}");
    for command in [
        "create",
        "read",
        "find",
        "plan",
        "replace-text",
        "replace-paragraph",
        "format-text",
        "format-paragraph",
        "insert-paragraph",
        "delete-paragraphs",
        "set-page-margins",
        "set-even-and-odd-headers",
        "comment-add",
        "comment-reply",
        "comment-set-status",
        "comment-delete",
        "revision-settle",
    ] {
        assert!(
            out.lines()
                .any(|line| line.trim_start().starts_with(command)),
            "{command}: {out}"
        );
    }
    for removed in [
        "edit",
        "batch",
        "comment ",
        "revision ",
        "session",
        "format ",
        "style",
    ] {
        assert!(
            !out.lines()
                .any(|line| line.trim_start().starts_with(removed)),
            "{removed}: {out}"
        );
    }
}

#[test]
fn create_read_find_and_command_json_parity() {
    let d = dir();
    let path = create_doc(&d);
    let (id, source) = read_meta(&path);
    let before = std::fs::read(&path).unwrap();
    let (code, out, err) = run(&["--json", "find", path.to_str().unwrap(), "world"]);
    assert_eq!(code, 0, "{err}\n{out}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(out.trim()).unwrap()["outcome"],
        "completed"
    );
    let (code, out, err) = run(&[
        "--json",
        "replace-text",
        path.to_str().unwrap(),
        "--at",
        &id,
        "--select",
        "world",
        "--with",
        "globe",
        "--expect-source",
        &source,
        "--dry-run",
    ]);
    assert_eq!(code, 0, "{err}\n{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(value["outcome"], "completed");
    assert!(value.get("bytes").is_some());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let (code, out, err) = run(&[
        "--json",
        "replace-text",
        path.to_str().unwrap(),
        "--at",
        &id,
        "--select",
        "world",
        "--with",
        "globe",
        "--expect-source",
        "sha256:0000000000000000000000000000000000000000000000000000000000000000",
    ]);
    assert_eq!(code, 2, "{err}\n{out}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(out.trim()).unwrap()["outcome"],
        "rejected"
    );
}

#[test]
fn human_output_contains_typed_read_find_and_plan_fields() {
    let d = dir();
    let path = create_doc(&d);
    let (id, source) = read_meta(&path);

    let (code, out, err) = run(&["read", path.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}\n{out}");
    assert!(out.contains("\"kind\": \"document\""), "{out}");
    assert!(out.contains("Hello world"), "{out}");

    for kind in ["styles", "comments", "revisions"] {
        let (code, out, err) = run(&["read", path.to_str().unwrap(), "--kind", kind]);
        assert_eq!(code, 0, "{kind}: {err}\n{out}");
        assert!(out.contains(&format!("\"kind\": \"{kind}\"")), "{out}");
    }
    assert!(!out.contains("\"page\":"), "{out}");
    assert!(!out.contains("\"pages\":"), "{out}");

    let plan_path = d.path().join("human-plan.toml");
    std::fs::write(
        &plan_path,
        format!(
            "base = \"{source}\"\nauthor = \"docxdriver\"\nchange_mode = \"track\"\n\n[[ops]]\nop = \"replace_text\"\nat = \"{id}\"\nselect = \"world\"\nwith = \"globe\"\n"
        ),
    )
    .unwrap();
    let (code, out, err) = run(&["plan", path.to_str().unwrap(), plan_path.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}\n{out}");
    assert!(out.contains("\"outcome\": \"previewed\""), "{out}");
    assert!(out.contains("\"preview_key\""), "{out}");
    assert!(out.contains("replace_text"), "{out}");
}

#[test]
fn invalid_cli_values_are_typed_rejections() {
    let d = dir();
    let path = create_doc(&d);
    let cases: &[(&[&str], &str, &str)] = &[
        (
            &[
                "replace-text",
                "--at",
                "not-an-address",
                "--select",
                "x",
                "--with",
                "y",
            ],
            "invalid_address",
            "at",
        ),
        (
            &["set-page-margins", "--margin-top", "not-points"],
            "invalid_points",
            "margin-top",
        ),
        (
            &[
                "format-paragraph",
                "--at",
                "12345678",
                "--line-spacing",
                "exact:nope",
            ],
            "invalid_line_spacing",
            "line_spacing",
        ),
        (
            &["insert-paragraph", "--with", "x"],
            "invalid_insert_position",
            "before|after",
        ),
    ];
    for (tail, code_name, path_name) in cases {
        let mut args = vec!["--json", tail[0], path.to_str().unwrap()];
        args.extend_from_slice(&tail[1..]);
        let (code, out, err) = run(&args);
        assert_eq!(code, 2, "{tail:?}: {err}\n{out}");
        let value: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(value["outcome"], "rejected", "{value}");
        assert_eq!(value["diagnostic"]["code"], *code_name, "{value}");
        assert_eq!(value["diagnostic"]["path"], *path_name, "{value}");
    }

    let (code, out, err) = run(&[
        "--json",
        "replace-text",
        path.to_str().unwrap(),
        "--at",
        "12345678",
        "--select",
        "x",
        "--with",
        "y",
        "--expect-source",
        "not-a-hash",
    ]);
    assert_eq!(code, 2, "{err}\n{out}");
    let value: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(value["outcome"], "rejected");
    assert_eq!(value["diagnostic"]["code"], "invalid_source");
    assert_eq!(value["diagnostic"]["path"], "expected_source");
}

#[cfg(unix)]
#[test]
fn symlink_input_defaults_to_canonical_document_output() {
    use std::os::unix::fs::symlink;

    let d = dir();
    let target = create_doc(&d);
    let link = d.path().join("alias.docx");
    symlink(&target, &link).unwrap();
    let (id, _) = read_meta(&target);
    let (code, out, err) = run(&[
        "--json",
        "replace-text",
        link.to_str().unwrap(),
        "--at",
        &id,
        "--select",
        "world",
        "--with",
        "globe",
    ]);
    assert_eq!(code, 0, "{err}\n{out}");
    assert!(std::fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    let (code, out, err) = run(&["find", target.to_str().unwrap(), "globe"]);
    assert_eq!(code, 0, "{err}\n{out}");
}

#[test]
fn plan_preview_commit_and_stale_refusals() {
    let d = dir();
    let path = create_doc(&d);
    let (id, source) = read_meta(&path);
    let plan_path = d.path().join("plan.toml");
    std::fs::write(&plan_path, format!("base = \"{source}\"\nauthor = \"docxdriver\"\nchange_mode = \"track\"\n\n[[ops]]\nop = \"replace_text\"\nat = \"{id}\"\nselect = \"world\"\nwith = \"globe\"\n")).unwrap();
    let ps = plan_path.to_str().unwrap();
    let (code, out, err) = run(&["--json", "plan", path.to_str().unwrap(), ps]);
    assert_eq!(code, 0, "{err}\n{out}");
    let preview: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(preview["outcome"], "previewed");
    let key = preview["preview_key"].as_str().unwrap();
    let (code, out, err) = run(&[
        "--json",
        "plan",
        path.to_str().unwrap(),
        ps,
        "--commit",
        key,
    ]);
    assert_eq!(code, 0, "{err}\n{out}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(out.trim()).unwrap()["outcome"],
        "committed"
    );
    let (code, _, _) = run(&[
        "--json",
        "plan",
        path.to_str().unwrap(),
        ps,
        "--commit",
        "p1:sha256:bad",
    ]);
    assert_eq!(code, 2);
}

#[test]
fn path_containment_rejects_escape() {
    let outside = PathBuf::from("/tmp/docxdriver-cli-escape.docx");
    let (code, _, err) = run(&["read", outside.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(err.contains("escapes cwd"), "{err}");
}
