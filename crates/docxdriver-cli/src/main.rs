//! Thin typed CLI adapter for docxdriver-core.
mod typed_cli;

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ChangeModeArg {
    Track,
    Direct,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ReadView {
    Markup,
    Final,
    Original,
}

#[derive(Args, Debug, Clone)]
struct MutArgs {
    path: PathBuf,
    #[arg(long)]
    dry_run: bool,
    #[arg(long, value_name = "SHA256")]
    expect_source: Option<String>,
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,
    #[arg(long, default_value = "docxdriver")]
    author: String,
    #[arg(long, value_enum, default_value_t = ChangeModeArg::Track)]
    change_mode: ChangeModeArg,
}
#[derive(Args, Debug, Clone)]
struct ReplaceTextArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long)]
    at: String,
    #[arg(long)]
    select: String,
    #[arg(long = "with")]
    with_text: String,
    #[arg(long)]
    occurrence: Option<u32>,
}
#[derive(Args, Debug, Clone)]
struct ReplaceParagraphArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long)]
    at: String,
    #[arg(long = "with")]
    with_text: String,
}
#[derive(Args, Debug, Clone)]
struct FormatTextArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long)]
    at: String,
    #[arg(long)]
    select: String,
    #[arg(long)]
    occurrence: Option<u32>,
    #[arg(long)]
    bold: Option<bool>,
    #[arg(long)]
    italic: Option<bool>,
    #[arg(long)]
    underline: Option<bool>,
    #[arg(long)]
    strike: Option<bool>,
    #[arg(long)]
    superscript: Option<bool>,
    #[arg(long)]
    subscript: Option<bool>,
    #[arg(long)]
    color: Option<String>,
    #[arg(long = "font-size")]
    font_size: Option<f64>,
    #[arg(long, value_delimiter = ',')]
    clear: Vec<String>,
}
#[derive(Args, Debug, Clone)]
struct FormatParagraphArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long)]
    at: String,
    #[arg(long)]
    style: Option<String>,
    #[arg(long)]
    alignment: Option<String>,
    #[arg(long = "indent-left")]
    indent_left: Option<String>,
    #[arg(long = "indent-right")]
    indent_right: Option<String>,
    #[arg(long = "space-before")]
    space_before: Option<String>,
    #[arg(long = "space-after")]
    space_after: Option<String>,
    #[arg(long = "line-spacing")]
    line_spacing: Option<String>,
    #[arg(long, value_delimiter = ',')]
    clear: Vec<String>,
}
#[derive(Args, Debug, Clone)]
struct InsertParagraphArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long, conflicts_with = "after")]
    before: Option<String>,
    #[arg(long, conflicts_with = "before")]
    after: Option<String>,
    #[arg(long = "with", default_value = "")]
    with_text: String,
    #[arg(long)]
    style: Option<String>,
}
#[derive(Args, Debug, Clone)]
struct DeleteParagraphsArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long = "at", num_args = 1..)]
    at: Vec<String>,
}
#[derive(Args, Debug, Clone)]
struct PageMarginsArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long)]
    section: Option<u32>,
    #[arg(long = "margin-top")]
    top: Option<String>,
    #[arg(long = "margin-right")]
    right: Option<String>,
    #[arg(long = "margin-bottom")]
    bottom: Option<String>,
    #[arg(long = "margin-left")]
    left: Option<String>,
    #[arg(long = "margin-header")]
    header: Option<String>,
    #[arg(long = "margin-footer")]
    footer: Option<String>,
    #[arg(long = "margin-gutter")]
    gutter: Option<String>,
}
#[derive(Args, Debug, Clone)]
struct EvenOddHeadersArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long, action = ArgAction::Set)]
    enabled: bool,
}
#[derive(Args, Debug, Clone)]
struct SetChromeArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long)]
    section: Option<u32>,
    #[arg(long, value_parser = ["default", "first", "even"])]
    kind: Option<String>,
    #[arg(long = "with", default_value = "")]
    with_text: String,
}
#[derive(Args, Debug, Clone)]
struct ClearChromeArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long)]
    section: Option<u32>,
    #[arg(long, value_parser = ["default", "first", "even"])]
    kind: Option<String>,
}
#[derive(Args, Debug, Clone)]
struct CommentAddArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long)]
    at: String,
    #[arg(long)]
    select: Option<String>,
    #[arg(long)]
    occurrence: Option<u32>,
    #[arg(long)]
    text: String,
}
#[derive(Args, Debug, Clone)]
struct CommentReplyArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long = "comment-id")]
    comment_id: String,
    #[arg(long)]
    text: String,
}
#[derive(Args, Debug, Clone)]
struct CommentStatusArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long = "comment-id")]
    comment_id: String,
    #[arg(long, value_parser = ["open", "resolved"])]
    status: String,
}
#[derive(Args, Debug, Clone)]
struct CommentDeleteArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long = "comment-id")]
    comment_id: String,
}
#[derive(Args, Debug, Clone)]
struct RevisionSettleArgs {
    #[command(flatten)]
    common: MutArgs,
    #[arg(long, value_name = "ID|all")]
    target: String,
    #[arg(long, value_parser = ["accept", "reject"])]
    action: String,
}

#[derive(Parser, Debug)]
#[command(
    name = "docxdriver",
    about = "Typed DOCX command and plan CLI",
    version,
    disable_help_subcommand = true
)]
struct Cli {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand, Debug)]
enum Commands {
    Create {
        path: PathBuf,
        #[arg(long)]
        html: String,
    },
    Read {
        path: PathBuf,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, value_enum)]
        view: Option<ReadView>,
    },
    Find {
        path: PathBuf,
        query: String,
        #[arg(long)]
        ignore_case: bool,
        #[arg(long, value_enum, default_value_t = ReadView::Markup)]
        view: ReadView,
    },
    Plan {
        path: PathBuf,
        plan: PathBuf,
        #[arg(long, value_name = "PREVIEW-KEY")]
        commit: Option<String>,
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
    ReplaceText(ReplaceTextArgs),
    ReplaceParagraph(ReplaceParagraphArgs),
    FormatText(FormatTextArgs),
    FormatParagraph(FormatParagraphArgs),
    InsertParagraph(InsertParagraphArgs),
    DeleteParagraphs(DeleteParagraphsArgs),
    SetPageMargins(PageMarginsArgs),
    SetEvenAndOddHeaders(EvenOddHeadersArgs),
    SetHeader(SetChromeArgs),
    SetFooter(SetChromeArgs),
    ClearHeader(ClearChromeArgs),
    ClearFooter(ClearChromeArgs),
    CommentAdd(CommentAddArgs),
    CommentReply(CommentReplyArgs),
    CommentSetStatus(CommentStatusArgs),
    CommentDelete(CommentDeleteArgs),
    RevisionSettle(RevisionSettleArgs),
}
fn main() -> ExitCode {
    match Cli::try_parse() {
        Ok(cli) => match typed_cli::dispatch(cli) {
            Ok(code) => code,
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::from(1)
            }
        },
        Err(error) => error.exit(),
    }
}
