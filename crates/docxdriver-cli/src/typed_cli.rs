use crate::{ChangeModeArg, Cli, Commands, MutArgs, ReadView};
use docxdriver_core::{
    execute_request, ChangeMode, ChromeKind, Command, CommandRequest, CommandResult, CommentStatus,
    CreateCommand as CoreCreate, EditCommand, EditOp, FindCommand, FindView, InsertAnchor,
    InsertPosition, LineSpacing, ParagraphAddress, PlanRequest, PlanResult, Points, PreviewKey,
    ReadCommand, Request, RevisionAction, RevisionTarget, SourceHash,
};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

pub struct Context {
    json: bool,
}

fn emit(ctx: &Context, value: &Value) -> Result<(), String> {
    if ctx.json {
        println!(
            "{}",
            serde_json::to_string(value).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
        );
    }
    Ok(())
}
fn core_json<T: serde::Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

fn canonical_cwd() -> Result<PathBuf, String> {
    std::env::current_dir()
        .and_then(|p| fs::canonicalize(p))
        .map_err(|e| format!("resolve cwd: {e}"))
}
fn lexical(path: &Path) -> Result<PathBuf, String> {
    // On Windows canonicalize adds a verbatim prefix (\\?\) while CLI paths
    // commonly use ordinary drive paths. Compare lexical paths in the same
    // spelling, then verify containment against canonical paths below.
    let root = std::env::current_dir().map_err(|e| format!("resolve cwd: {e}"))?;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let mut out = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::Prefix(_) | Component::RootDir => out.push(part.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return Err(format!("path escapes cwd: {}", path.display()));
                }
            }
            Component::Normal(x) => out.push(x),
        }
    }
    if !out.starts_with(&root) {
        return Err(format!("path escapes cwd: {}", path.display()));
    }
    Ok(out)
}
fn safe_existing(path: &Path) -> Result<PathBuf, String> {
    let lexical = lexical(path)?;
    let canonical =
        fs::canonicalize(&lexical).map_err(|e| format!("resolve {}: {e}", path.display()))?;
    if !canonical.starts_with(canonical_cwd()?) {
        return Err(format!(
            "path escapes cwd through symlink: {}",
            path.display()
        ));
    }
    Ok(canonical)
}
fn safe_new(path: &Path) -> Result<PathBuf, String> {
    let lexical = lexical(path)?;
    let parent = lexical.parent().unwrap_or_else(|| Path::new("."));
    let canonical_parent =
        fs::canonicalize(parent).map_err(|e| format!("resolve output directory: {e}"))?;
    if !canonical_parent.starts_with(canonical_cwd()?) {
        return Err(format!("output escapes cwd: {}", path.display()));
    }
    Ok(lexical)
}
fn read_doc(path: &Path) -> Result<(PathBuf, Vec<u8>, SourceHash), String> {
    let actual = safe_existing(path)?;
    let bytes = fs::read(&actual).map_err(|e| format!("read {}: {e}", path.display()))?;
    let hash = SourceHash::from_bytes(&bytes);
    Ok((actual, bytes, hash))
}
#[derive(Debug)]
struct ParseError {
    code: &'static str,
    path: &'static str,
    message: String,
}

fn parse_expected(value: Option<&str>) -> Result<Option<SourceHash>, ParseError> {
    value
        .map(SourceHash::parse)
        .transpose()
        .map_err(|e| ParseError {
            code: "invalid_source",
            path: "expected_source",
            message: format!("--expect-source: {e}"),
        })
}
fn rejected_at(code: &str, message: impl Into<String>, path: Option<String>) -> CommandResult {
    CommandResult::Rejected {
        source: None,
        result: None,
        diagnostic: docxdriver_core::Diagnostic {
            code: code.into(),
            message: message.into(),
            path,
            span: None,
        },
    }
}
fn invalid_input(ctx: &Context, error: ParseError) -> Result<ExitCode, String> {
    let result = rejected_at(error.code, error.message, Some(error.path.into()));
    emit(ctx, &result_value(&result)?)?;
    Ok(ExitCode::from(2))
}
fn mode(mode: ChangeModeArg) -> ChangeMode {
    if matches!(mode, ChangeModeArg::Track) {
        ChangeMode::Track
    } else {
        ChangeMode::Direct
    }
}
fn address_at(value: String, path: &'static str) -> Result<ParagraphAddress, ParseError> {
    serde_json::from_value(Value::String(value)).map_err(|e| ParseError {
        code: "invalid_address",
        path,
        message: format!("invalid paragraph address: {e}"),
    })
}
fn address(value: String) -> Result<ParagraphAddress, ParseError> {
    address_at(value, "at")
}
fn insert_anchor_at(value: String, path: &'static str) -> Result<InsertAnchor, ParseError> {
    serde_json::from_value(Value::String(value)).map_err(|e| ParseError {
        code: "invalid_address",
        path,
        message: format!("invalid insert anchor: {e}"),
    })
}
fn chrome_kind(value: Option<String>, path: &'static str) -> Result<Option<ChromeKind>, ParseError> {
    match value {
        None => Ok(None),
        Some(kind) => Ok(Some(match kind.as_str() {
            "default" => ChromeKind::Default,
            "first" => ChromeKind::First,
            "even" => ChromeKind::Even,
            other => {
                return Err(ParseError {
                    code: "invalid_chrome_kind",
                    path,
                    message: format!("invalid chrome kind: {other} (expected default, first, or even)"),
                })
            }
        })),
    }
}
fn points(value: Option<String>, name: &'static str) -> Result<Option<Points>, ParseError> {
    value
        .map(|s| {
            s.parse::<f64>()
                .map_err(|_| ParseError {
                    code: "invalid_points",
                    path: name,
                    message: format!("{name} must be points"),
                })
                .and_then(|n| {
                    Points::from_points(n).map_err(|e| ParseError {
                        code: "invalid_points",
                        path: name,
                        message: format!("{name}: {e}"),
                    })
                })
        })
        .transpose()
}
fn spacing(value: Option<String>) -> Result<Option<LineSpacing>, ParseError> {
    let Some(raw) = value else { return Ok(None) };
    let (kind, number) = raw.split_once(':').unwrap_or(("multiple", raw.as_str()));
    let n: f64 = number.parse().map_err(|_| ParseError {
        code: "invalid_line_spacing",
        path: "line_spacing",
        message: "--line-spacing must be MODE:VALUE".into(),
    })?;
    Ok(Some(match kind {
        "multiple" => LineSpacing::Multiple(n),
        "exact" => LineSpacing::Exact(Points::from_points(n).map_err(|e| ParseError {
            code: "invalid_line_spacing",
            path: "line_spacing",
            message: e.to_string(),
        })?),
        "at_least" => LineSpacing::AtLeast(Points::from_points(n).map_err(|e| ParseError {
            code: "invalid_line_spacing",
            path: "line_spacing",
            message: e.to_string(),
        })?),
        _ => {
            return Err(ParseError {
                code: "invalid_line_spacing",
                path: "line_spacing",
                message: "--line-spacing mode must be multiple, exact, or at_least".into(),
            })
        }
    }))
}

fn stale(source: SourceHash, message: impl Into<String>) -> CommandResult {
    CommandResult::Rejected {
        source: Some(source),
        result: None,
        diagnostic: docxdriver_core::Diagnostic {
            code: "source_changed_before_commit".into(),
            message: message.into(),
            path: None,
            span: None,
        },
    }
}
fn rejected(code: &str, message: impl Into<String>) -> CommandResult {
    CommandResult::Rejected {
        source: None,
        result: None,
        diagnostic: docxdriver_core::Diagnostic {
            code: code.into(),
            message: message.into(),
            path: None,
            span: None,
        },
    }
}
enum WriteFailure {
    Rejected(CommandResult),
    Infrastructure(String),
}
fn write_atomic(
    input: &Path,
    output: &Path,
    expected: &SourceHash,
    bytes: &[u8],
) -> Result<(), WriteFailure> {
    fn verify_parent(path: &Path) -> Result<(), WriteFailure> {
        let root = canonical_cwd().map_err(WriteFailure::Infrastructure)?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let actual = fs::canonicalize(parent)
            .map_err(|e| WriteFailure::Infrastructure(format!("resolve output directory: {e}")))?;
        if !actual.starts_with(root) {
            return Err(WriteFailure::Infrastructure(
                "output path changed outside cwd before write".into(),
            ));
        }
        Ok(())
    }
    let current = fs::read(input)
        .map_err(|e| WriteFailure::Infrastructure(format!("read source before commit: {e}")))?;
    let actual = SourceHash::from_bytes(&current);
    if actual != *expected {
        return Err(WriteFailure::Rejected(stale(
            actual,
            "source changed before commit",
        )));
    }
    verify_parent(output)?;
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let name = output
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("output.docx");
    let mut temp = None;
    for n in 0..64u32 {
        let candidate = parent.join(format!(".{name}.docxdriver-{}-{n}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut f) => {
                if let Err(e) = f.write_all(bytes).and_then(|_| f.sync_all()) {
                    let _ = fs::remove_file(&candidate);
                    return Err(WriteFailure::Infrastructure(format!(
                        "write temporary output: {e}"
                    )));
                }
                temp = Some(candidate);
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(WriteFailure::Infrastructure(format!(
                    "create temporary output: {e}"
                )))
            }
        }
    }
    let temp = temp.ok_or_else(|| {
        WriteFailure::Infrastructure("could not allocate temporary output".into())
    })?;
    let current = fs::read(input)
        .map_err(|e| WriteFailure::Infrastructure(format!("read source before rename: {e}")))?;
    let actual = SourceHash::from_bytes(&current);
    if actual != *expected {
        let _ = fs::remove_file(&temp);
        return Err(WriteFailure::Rejected(stale(
            actual,
            "source changed before commit",
        )));
    }
    verify_parent(output)?;
    if let Err(e) = fs::rename(&temp, output) {
        let _ = fs::remove_file(&temp);
        return Err(WriteFailure::Infrastructure(format!("atomic rename: {e}")));
    }
    Ok(())
}
fn write_create(output: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let name = output
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("output.docx");
    let temp = parent.join(format!(".{name}.docxdriver-create-{}.tmp", std::process::id()));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| format!("create temporary output: {e}"))?;
    if let Err(e) = f.write_all(bytes).and_then(|_| f.sync_all()) {
        let _ = fs::remove_file(&temp);
        return Err(format!("write temporary output: {e}"));
    }
    if let Err(e) = fs::hard_link(&temp, output) {
        let _ = fs::remove_file(&temp);
        return Err(format!("exclusive create: {e}"));
    }
    let _ = fs::remove_file(temp);
    Ok(())
}
fn result_value(result: &CommandResult) -> Result<Value, String> {
    core_json(result)
}
fn dispatch_parsed(
    ctx: &Context,
    args: &MutArgs,
    parsed: Result<EditOp, ParseError>,
) -> Result<ExitCode, String> {
    match parsed {
        Ok(op) => dispatch_command(ctx, args, op),
        Err(error) => invalid_input(ctx, error),
    }
}
fn dispatch_command(ctx: &Context, args: &MutArgs, op: EditOp) -> Result<ExitCode, String> {
    let path = &args.path;
    let (actual, input, source) = read_doc(path)?;
    let expected = match parse_expected(args.expect_source.as_deref()) {
        Ok(expected) => expected,
        Err(error) => return invalid_input(ctx, error),
    };
    let request = Request::Command(CommandRequest {
        command: Command::Edit(EditCommand {
            author: args.author.clone(),
            change_mode: mode(args.change_mode),
            op,
        }),
        expected_source: expected,
    });
    let mut result = match execute_request(Some(&input), &request) {
        docxdriver_core::RequestResult::Command(r) => r,
        _ => unreachable!(),
    };
    if !args.dry_run {
        if let CommandResult::Completed {
            bytes: Some(bytes), ..
        } = &result
        {
            let output = match args.output.as_deref() {
                Some(path) => safe_new(path)?,
                None => actual.clone(),
            };
            if let Err(failure) = write_atomic(&actual, &output, &source, bytes) {
                match failure {
                    WriteFailure::Rejected(rejection) => result = rejection,
                    WriteFailure::Infrastructure(message) => return Err(message),
                }
            }
        }
    }
    emit(ctx, &result_value(&result)?)?;
    Ok(if matches!(result, CommandResult::Completed { .. }) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}
pub fn dispatch(cli: Cli) -> Result<ExitCode, String> {
    let ctx = Context { json: cli.json };
    match cli.command {
        Commands::Create { path, html } => create(&ctx, &path, &html),
        Commands::Read { path, kind, view } => read(&ctx, &path, kind, view),
        Commands::Find {
            path,
            query,
            ignore_case,
            view,
        } => find(&ctx, &path, query, ignore_case, view),
        Commands::Plan {
            path,
            plan: plan_path,
            commit,
            output,
        } => plan(
            &ctx,
            &path,
            &plan_path,
            commit.as_deref(),
            output.as_deref(),
        ),
        Commands::ReplaceText(a) => dispatch_parsed(
            &ctx,
            &a.common,
            (|| {
                Ok(EditOp::ReplaceText {
                    at: address(a.at)?,
                    select: a.select,
                    with: a.with_text,
                    occurrence: a.occurrence,
                })
            })(),
        ),
        Commands::ReplaceParagraph(a) => dispatch_parsed(
            &ctx,
            &a.common,
            (|| {
                Ok(EditOp::ReplaceParagraph {
                    at: address(a.at)?,
                    with: a.with_text,
                })
            })(),
        ),
        Commands::FormatText(a) => dispatch_parsed(
            &ctx,
            &a.common,
            (|| {
                Ok(EditOp::FormatText {
                    at: address(a.at)?,
                    select: a.select,
                    occurrence: a.occurrence,
                    bold: a.bold,
                    italic: a.italic,
                    underline: a.underline,
                    strike: a.strike,
                    superscript: a.superscript,
                    subscript: a.subscript,
                    color: a.color,
                    font_size: a
                        .font_size
                        .map(|n| {
                            Points::from_points(n).map_err(|e| ParseError {
                                code: "invalid_points",
                                path: "font_size",
                                message: e.to_string(),
                            })
                        })
                        .transpose()?,
                    clear: a.clear,
                })
            })(),
        ),
        Commands::FormatParagraph(a) => dispatch_parsed(
            &ctx,
            &a.common,
            (|| {
                Ok(EditOp::FormatParagraph {
                    at: address(a.at)?,
                    style: a.style,
                    alignment: a.alignment,
                    indent_left: points(a.indent_left, "indent-left")?,
                    indent_right: points(a.indent_right, "indent-right")?,
                    space_before: points(a.space_before, "space-before")?,
                    space_after: points(a.space_after, "space-after")?,
                    line_spacing: spacing(a.line_spacing)?,
                    clear: a.clear,
                })
            })(),
        ),
        Commands::InsertParagraph(a) => {
            let parsed = (|| {
                let (at, position) = match (a.before, a.after) {
                    (Some(x), None) => (insert_anchor_at(x, "before")?, InsertPosition::Before),
                    (None, Some(x)) => (insert_anchor_at(x, "after")?, InsertPosition::After),
                    _ => {
                        return Err(ParseError {
                            code: "invalid_insert_position",
                            path: "before|after",
                            message: "exactly one of --before/--after is required".into(),
                        })
                    }
                };
                Ok(EditOp::InsertParagraph {
                    at,
                    position,
                    with: a.with_text,
                    style: a.style,
                    alias: None,
                })
            })();
            dispatch_parsed(&ctx, &a.common, parsed)
        }
        Commands::DeleteParagraphs(a) => dispatch_parsed(
            &ctx,
            &a.common,
            (|| {
                Ok(EditOp::DeleteParagraphs {
                    at: a.at.into_iter().map(address).collect::<Result<_, _>>()?,
                })
            })(),
        ),
        Commands::SetPageMargins(a) => dispatch_parsed(
            &ctx,
            &a.common,
            (|| {
                Ok(EditOp::SetPageMargins {
                    section: a.section,
                    top: points(a.top, "margin-top")?,
                    right: points(a.right, "margin-right")?,
                    bottom: points(a.bottom, "margin-bottom")?,
                    left: points(a.left, "margin-left")?,
                    header: points(a.header, "margin-header")?,
                    footer: points(a.footer, "margin-footer")?,
                    gutter: points(a.gutter, "margin-gutter")?,
                })
            })(),
        ),
        Commands::SetEvenAndOddHeaders(a) => dispatch_command(
            &ctx,
            &a.common,
            EditOp::SetEvenAndOddHeaders {
                even_and_odd: a.enabled,
            },
        ),
        Commands::SetHeader(a) => dispatch_parsed(
            &ctx,
            &a.common,
            (|| {
                Ok(EditOp::SetHeader {
                    section: a.section,
                    kind: chrome_kind(a.kind, "kind")?,
                    with: a.with_text,
                })
            })(),
        ),
        Commands::SetFooter(a) => dispatch_parsed(
            &ctx,
            &a.common,
            (|| {
                Ok(EditOp::SetFooter {
                    section: a.section,
                    kind: chrome_kind(a.kind, "kind")?,
                    with: a.with_text,
                })
            })(),
        ),
        Commands::ClearHeader(a) => dispatch_parsed(
            &ctx,
            &a.common,
            (|| {
                Ok(EditOp::ClearHeader {
                    section: a.section,
                    kind: chrome_kind(a.kind, "kind")?,
                })
            })(),
        ),
        Commands::ClearFooter(a) => dispatch_parsed(
            &ctx,
            &a.common,
            (|| {
                Ok(EditOp::ClearFooter {
                    section: a.section,
                    kind: chrome_kind(a.kind, "kind")?,
                })
            })(),
        ),
        Commands::CommentAdd(a) => dispatch_parsed(
            &ctx,
            &a.common,
            (|| {
                Ok(EditOp::CommentAdd {
                    at: address(a.at)?,
                    select: a.select,
                    occurrence: a.occurrence,
                    text: a.text,
                })
            })(),
        ),
        Commands::CommentReply(a) => dispatch_command(
            &ctx,
            &a.common,
            EditOp::CommentReply {
                comment_id: a.comment_id,
                text: a.text,
            },
        ),
        Commands::CommentSetStatus(a) => dispatch_command(
            &ctx,
            &a.common,
            EditOp::CommentSetStatus {
                comment_id: a.comment_id,
                status: if a.status == "resolved" {
                    CommentStatus::Resolved
                } else {
                    CommentStatus::Open
                },
            },
        ),
        Commands::CommentDelete(a) => dispatch_command(
            &ctx,
            &a.common,
            EditOp::CommentDelete {
                comment_id: a.comment_id,
            },
        ),
        Commands::RevisionSettle(a) => dispatch_command(
            &ctx,
            &a.common,
            EditOp::RevisionSettle {
                target: if a.target == "all" {
                    RevisionTarget::All
                } else {
                    RevisionTarget::Id(a.target)
                },
                action: if a.action == "accept" {
                    RevisionAction::Accept
                } else {
                    RevisionAction::Reject
                },
            },
        ),
    }
}
fn create(ctx: &Context, path: &Path, html: &str) -> Result<ExitCode, String> {
    let output = safe_new(path)?;
    if fs::symlink_metadata(&output).is_ok() {
        let result = rejected(
            "destination_exists",
            format!("refusing to overwrite {}", path.display()),
        );
        emit(ctx, &result_value(&result)?)?;
        return Ok(ExitCode::from(2));
    }
    let source = if html == "-" {
        let mut s = String::new();
        io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| e.to_string())?;
        s
    } else {
        html.to_string()
    };
    let request = Request::Command(CommandRequest {
        command: Command::Create(CoreCreate {
            paragraphs: Vec::new(),
            html: Some(source),
        }),
        expected_source: None,
    });
    let result = match execute_request(None, &request) {
        docxdriver_core::RequestResult::Command(r) => r,
        _ => unreachable!(),
    };
    if let CommandResult::Completed {
        bytes: Some(bytes), ..
    } = &result
    {
        write_create(&output, bytes)?;
    }
    emit(ctx, &result_value(&result)?)?;
    Ok(if matches!(result, CommandResult::Completed { .. }) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}
fn read(
    ctx: &Context,
    path: &Path,
    kind: Option<String>,
    view: Option<ReadView>,
) -> Result<ExitCode, String> {
    let (_p, input, _s) = read_doc(path)?;
    let request = Request::Command(CommandRequest {
        command: Command::Read(ReadCommand {
            read_kind: kind,
            view: view.map(|view| format!("{view:?}").to_ascii_lowercase()),
        }),
        expected_source: None,
    });
    let result = match execute_request(Some(&input), &request) {
        docxdriver_core::RequestResult::Command(r) => r,
        _ => unreachable!(),
    };
    emit(ctx, &result_value(&result)?)?;
    Ok(if matches!(result, CommandResult::Completed { .. }) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}
fn find(
    ctx: &Context,
    path: &Path,
    query: String,
    ignore_case: bool,
    view: ReadView,
) -> Result<ExitCode, String> {
    let (_p, input, _s) = read_doc(path)?;
    let request = Request::Command(CommandRequest {
        command: Command::Find(FindCommand {
            query,
            ignore_case,
            view: Some(match view {
                ReadView::Markup => FindView::Markup,
                ReadView::Final => FindView::Final,
                ReadView::Original => FindView::Original,
            }),
        }),
        expected_source: None,
    });
    let result = match execute_request(Some(&input), &request) {
        docxdriver_core::RequestResult::Command(r) => r,
        _ => unreachable!(),
    };
    emit(ctx, &result_value(&result)?)?;
    Ok(if matches!(result, CommandResult::Completed { .. }) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}
fn plan(
    ctx: &Context,
    path: &Path,
    plan_path: &Path,
    commit: Option<&str>,
    output: Option<&Path>,
) -> Result<ExitCode, String> {
    let (actual, input, source) = read_doc(path)?;
    let plan_file = safe_existing(plan_path)?;
    let text = fs::read_to_string(plan_file).map_err(|e| e.to_string())?;
    let parsed = match docxdriver_core::parse_plan_toml(&text) {
        Ok(plan) => plan,
        Err(diagnostic) => {
            let result = PlanResult::Rejected {
                source: Some(source),
                plan: None,
                diagnostic,
                report: None,
            };
            emit(ctx, &core_json(&result)?)?;
            return Ok(ExitCode::from(2));
        }
    };
    let key = commit.map(|s| PreviewKey(s.to_string()));
    let request = Request::Plan(PlanRequest {
        plan: parsed,
        preview_key: key,
    });
    let result = match execute_request(Some(&input), &request) {
        docxdriver_core::RequestResult::Plan(r) => r,
        _ => unreachable!(),
    };
    let mut result = result;
    if let PlanResult::Committed { bytes, .. } = &result {
        let dst = match output {
            Some(path) => safe_new(path)?,
            None => actual.clone(),
        };
        if let Err(failure) = write_atomic(&actual, &dst, &source, bytes) {
            match failure {
                WriteFailure::Rejected(CommandResult::Rejected {
                    source: rejected_source,
                    diagnostic,
                    ..
                }) => {
                    result = PlanResult::Rejected {
                        source: rejected_source,
                        plan: None,
                        diagnostic,
                        report: None,
                    }
                }
                WriteFailure::Rejected(CommandResult::Completed { .. }) => unreachable!(),
                WriteFailure::Infrastructure(message) => return Err(message),
            }
        }
    }
    emit(ctx, &core_json(&result)?)?;
    Ok(if matches!(result, PlanResult::Rejected { .. }) {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    })
}
