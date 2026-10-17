//! PLAN runs in CPython, its builders emit plans core accepts, and every
//! published copy carries the same text.

use docxdriver_core::parse_plan_json;
use docxdriver_monty_repl::{HOST_WRAPPERS, PLAN, PRELUDE};
use std::path::Path;
use std::process::{Command, Output};

const PYTHON_CANDIDATES: [&str; 2] = ["python3", "python"];
const BASE: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

/// Builds one plan with every builder kind and prints its JSON with `base`.
const SAMPLE: &str = r#"
import json, sys
namespace = {}
exec(sys.stdin.read(), namespace)
globals().update(namespace)
plan = Plan(author="Tester", operations=[
    AcceptRevision("all"),
    ReplaceText(at="0F537164", select="thirty", with_="sixty", occurrence=1),
    Revise("rent within thirty days").replace("thirty", "sixty").replace("rent", "the rent"),
    FormatText(at="0F537164", select="sixty", bold=True),
    InsertParagraph(at="0F537164", position="after", with_="New", as_="new"),
    CommentAdd(at="$new", text="Check"),
    DeleteParagraphs(at=["0F537165"]),
])
value = plan.to_dict()
value["base"] = sys.argv[1]
print(json.dumps(value))
"#;

/// Runs `args` with the first Python interpreter found, feeding `stdin`.
fn python(args: &[&str], stdin: &str) -> Output {
    use std::io::Write;
    use std::process::Stdio;

    for candidate in PYTHON_CANDIDATES {
        let Ok(mut child) = Command::new(candidate)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        else {
            continue;
        };
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        return child.wait_with_output().unwrap();
    }
    panic!("no Python interpreter found (tried {PYTHON_CANDIDATES:?})");
}

#[test]
fn builders_emit_core_plans() {
    let output = python(&["-I", "-c", SAMPLE, BASE], PLAN);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    // Core parses the emitted JSON: key "ops", schema names ("with", "as").
    let json = String::from_utf8(output.stdout).unwrap();
    let plan = parse_plan_json(&json).unwrap_or_else(|d| panic!("{d:?}\n{json}"));
    assert_eq!(plan.author, "Tester");
    assert_eq!(plan.ops.len(), 7);
}

#[test]
fn prelude_runs_and_sdk_copy_matches() {
    // PyPI docxdriver ships docxdriver/plan.py; it must be PLAN verbatim.
    let sdk = include_str!("../../docxdriver-python/python/docxdriver/plan.py");
    assert_eq!(sdk, PLAN);
    assert_eq!(PRELUDE, format!("{PLAN}\n{HOST_WRAPPERS}"));

    let output = python(&["-I", "-c", "exec(__import__('sys').stdin.read(), {})"], PRELUDE);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn generated_python_is_current() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/generate-python-plan.py");
    let output = python(&[script.to_str().unwrap(), "--check"], "");
    assert!(
        output.status.success(),
        "run `python scripts/generate-python-plan.py`: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
