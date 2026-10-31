//! Typed document API over the Request engine: one `.docx` in memory,
//! read, searched, reviewed and committed without JSON envelopes.
//!
//! ```text
//! let doc = Document::open(&bytes);
//! doc.markup(View::Markup)?            body with the ids plans address
//! doc.comments()? / revisions()? / styles()? / assets()? / bookmarks()?
//! doc.find("thirty", Case::Sensitive)?
//! let plan = doc.plan(json)?           bound to these bytes
//! let review = doc.review(&plan)?      key + report, nothing written
//! let bytes = doc.commit(&plan, &review.key)?
//! ```
//!
//! The preview key binds bytes and plan, so commit refuses anything that
//! was not reviewed against these exact bytes. No state lives between calls.

use serde_json::Value;

use crate::api::{
    execute_command, execute_plan, parse_plan_json, Command, CommandRequest, CommandResult,
    CreateCommand, Diagnostic, FindCommand, Plan, PlanReport, PlanRequest, PlanResult,
    PreviewKey, ReadCommand, SourceHash,
};

/// Which text of the body to project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Tracked changes shown as `<ins>`/`<del>`.
    Markup,
    /// As if every change were accepted.
    Final,
    /// As if every change were rejected.
    Original,
}

/// Case handling for [`Document::find`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Sensitive,
    Insensitive,
}

/// What [`create`] builds a new document from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Paragraphs(Vec<String>),
    Html(String),
}

/// A previewed plan: commit it with `key`; `report` says what each op did.
#[derive(Debug, Clone, PartialEq)]
pub struct Review {
    pub key: PreviewKey,
    pub report: PlanReport,
}

/// A `.docx` held as bytes.
#[derive(Debug, Clone, Copy)]
pub struct Document<'a> {
    bytes: &'a [u8],
}

/// Builds a new `.docx`.
pub fn create(source: Source) -> Result<Vec<u8>, Diagnostic> {
    let command = match source {
        Source::Paragraphs(paragraphs) => CreateCommand { paragraphs, html: None },
        Source::Html(html) => CreateCommand { paragraphs: Vec::new(), html: Some(html) },
    };
    match run(None, Command::Create(command)) {
        CommandResult::Completed { bytes: Some(bytes), .. } => Ok(bytes),
        CommandResult::Completed { .. } => Err(engine("create returned no document")),
        CommandResult::Rejected { diagnostic, .. } => Err(diagnostic),
    }
}

impl<'a> Document<'a> {
    pub fn open(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// The body as HTML-like markup with stable paragraph ids.
    pub fn markup(&self, view: View) -> Result<String, Diagnostic> {
        let view = match view {
            View::Markup => "markup",
            View::Final => "final",
            View::Original => "original",
        };
        let result = self.read("document", Some(view))?;
        match result.get("markup") {
            Some(Value::String(markup)) => Ok(markup.clone()),
            _ => Err(engine("document read returned no markup")),
        }
    }

    /// Equations by paragraph, e.g. `{"at": "0F537164", "equation": 1, ...}`,
    /// the addresses `replace_equation`/`delete_equation` take.
    pub fn equations(&self) -> Result<Vec<Value>, Diagnostic> {
        Ok(array(self.read("document", None)?, "equations"))
    }

    /// Comment threads with anchors and replies.
    pub fn comments(&self) -> Result<Vec<Value>, Diagnostic> {
        self.list("comments")
    }

    /// Tracked changes; their ids feed `AcceptRevision`/`RejectRevision`.
    pub fn revisions(&self) -> Result<Vec<Value>, Diagnostic> {
        self.list("revisions")
    }

    /// Style definitions.
    pub fn styles(&self) -> Result<Vec<Value>, Diagnostic> {
        self.list("styles")
    }

    /// Bookmarks in every story: name, id, hidden, start/end paragraph ids,
    /// bookmarked text and part.
    pub fn bookmarks(&self) -> Result<Vec<Value>, Diagnostic> {
        self.list("bookmarks")
    }

    /// Embedded images and media.
    pub fn assets(&self) -> Result<Vec<Value>, Diagnostic> {
        self.list("assets")
    }

    /// Paragraphs containing `query`, with their ids.
    pub fn find(&self, query: &str, case: Case) -> Result<Vec<Value>, Diagnostic> {
        let command = FindCommand {
            query: query.into(),
            ignore_case: case == Case::Insensitive,
            view: None,
        };
        let result = completed(run(Some(self.bytes), Command::Find(command)))?;
        Ok(array(result, "matches"))
    }

    /// Parses a plan's JSON and binds it to these bytes; any `base` given
    /// is replaced. E.g. `{"author":"A","change_mode":"track","ops":[...]}`.
    pub fn plan(&self, json: &str) -> Result<Plan, Diagnostic> {
        let mut value: Value = serde_json::from_str(json).map_err(|error| invalid(error.to_string()))?;
        let Some(object) = value.as_object_mut() else {
            return Err(invalid("a plan is a JSON object".into()));
        };
        object.insert("base".into(), Value::String(SourceHash::from_bytes(self.bytes).as_str()));
        parse_plan_json(&value.to_string())
    }

    /// Previews `plan`; writes nothing.
    pub fn review(&self, plan: &Plan) -> Result<Review, Diagnostic> {
        match self.execute(plan, None) {
            PlanResult::Previewed { preview_key, report, .. } => Ok(Review { key: preview_key, report }),
            PlanResult::Committed { .. } => Err(engine("preview committed")),
            PlanResult::Rejected { diagnostic, .. } => Err(diagnostic),
        }
    }

    /// Applies a reviewed plan, returning the new document's bytes. Its
    /// revisions and comments carry the current UTC time, read by the
    /// `clock` feature; without it this is a `missing_now` error, and
    /// [`Document::commit_at`] takes the time instead.
    pub fn commit(&self, plan: &Plan, key: &PreviewKey) -> Result<Vec<u8>, Diagnostic> {
        self.commit_at(plan, key, None)
    }

    /// [`Document::commit`] at a fixed time (`YYYY-MM-DDTHH:MM:SSZ`), for
    /// byte-stable output; `None` is the current UTC time.
    pub fn commit_at(&self, plan: &Plan, key: &PreviewKey, now: Option<&str>) -> Result<Vec<u8>, Diagnostic> {
        match execute_plan(self.bytes, &PlanRequest { plan: plan.clone(), preview_key: Some(key.clone()), now: now.map(str::to_owned) }) {
            PlanResult::Committed { bytes, .. } => Ok(bytes),
            PlanResult::Previewed { .. } => Err(engine("commit only previewed")),
            PlanResult::Rejected { diagnostic, .. } => Err(diagnostic),
        }
    }

    fn execute(&self, plan: &Plan, preview_key: Option<PreviewKey>) -> PlanResult {
        execute_plan(self.bytes, &PlanRequest { plan: plan.clone(), preview_key, now: None })
    }

    // One listing read, e.g. "comments" → result["comments"].
    fn list(&self, part: &str) -> Result<Vec<Value>, Diagnostic> {
        Ok(array(self.read(part, None)?, part))
    }

    fn read(&self, part: &str, view: Option<&str>) -> Result<Value, Diagnostic> {
        let command = ReadCommand { read_kind: Some(part.into()), view: view.map(Into::into) };
        completed(run(Some(self.bytes), Command::Read(command)))
    }
}

fn run(input: Option<&[u8]>, command: Command) -> CommandResult {
    execute_command(input, &CommandRequest { command, expected_source: None, now: None })
}

fn completed(result: CommandResult) -> Result<Value, Diagnostic> {
    match result {
        CommandResult::Completed { result, .. } => Ok(result.unwrap_or(Value::Null)),
        CommandResult::Rejected { diagnostic, .. } => Err(diagnostic),
    }
}

fn array(mut result: Value, field: &str) -> Vec<Value> {
    match result.get_mut(field).map(Value::take) {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

fn invalid(message: String) -> Diagnostic {
    Diagnostic { code: "invalid_plan".into(), message, path: None, span: None }
}

fn engine(message: &str) -> Diagnostic {
    Diagnostic { code: "engine".into(), message: message.into(), path: None, span: None }
}
