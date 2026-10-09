use crate::workspace::{HostError, Workspace};
use docxdriver_core::{
    execute_request, ChangeMode, Command, CommandRequest, CommandResult, CreateCommand, EditOp,
    FindCommand, Plan, PlanRequest, PlanResult, PreviewKey, ReadCommand, Request, RequestResult,
    SourceHash,
};
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ErrorData, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool, ToolAnnotations,
    },
    service::RequestContext,
    RoleServer, ServerHandler,
};
use schemars::schema_for;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::Path;
use std::sync::Arc;

const TOOL_NAMES: [&str; 5] = [
    "docx_create",
    "docx_read",
    "docx_find",
    "docx_edit",
    "docx_help",
];

#[derive(Debug, Clone)]
pub struct DocxServer {
    workspace: Workspace,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateInput {
    path: String,
    paragraphs: Option<Vec<String>>,
    html: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadInput {
    path: String,
    kind: Option<String>,
    view: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FindInput {
    path: String,
    query: String,
    #[serde(default)]
    ignore_case: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditInput {
    path: String,
    plan: Value,
    preview_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HelpInput {
    topic: Option<String>,
}

#[derive(Debug)]
struct ToolFailure {
    code: String,
    message: String,
}

impl ToolFailure {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_arguments".into(),
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: "tool_failed".into(),
            message: message.into(),
        }
    }

    fn host(error: HostError) -> Self {
        Self {
            code: error.code.into(),
            message: error.message,
        }
    }
}

impl std::fmt::Display for ToolFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl DocxServer {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, String> {
        Workspace::new(root)
            .map(|workspace| Self { workspace })
            .map_err(|error| error.to_string())
    }

    pub fn workspace_root(&self) -> &Path {
        self.workspace.root()
    }

    pub fn tools(&self) -> Vec<Tool> {
        TOOL_NAMES
            .iter()
            .map(|name| tool_definition(name))
            .collect()
    }

    pub fn call_named(&self, name: &str, arguments: Value) -> Result<Value, String> {
        self.call_named_internal(name, arguments)
            .map_err(|error| error.to_string())
    }

    fn call_named_internal(&self, name: &str, arguments: Value) -> Result<Value, ToolFailure> {
        match name {
            "docx_create" => self.create(parse_arguments(arguments)?),
            "docx_read" => self.read(parse_arguments(arguments)?),
            "docx_find" => self.find(parse_arguments(arguments)?),
            "docx_edit" => self.edit(parse_arguments(arguments)?),
            "docx_help" => self.help(parse_arguments(arguments)?),
            _ => Err(ToolFailure::invalid(format!("unknown tool: {name}"))),
        }
    }

    fn create(&self, input: CreateInput) -> Result<Value, ToolFailure> {
        if input.html.is_some() == input.paragraphs.is_some() {
            return Err(ToolFailure::invalid(
                "pass exactly one of paragraphs or html",
            ));
        }
        let result = match execute_request(
            None,
            &Request::Command(CommandRequest {
                command: Command::Create(CreateCommand {
                    paragraphs: input.paragraphs.unwrap_or_default(),
                    html: input.html,
                }),
                expected_source: None,
            }),
        ) {
            RequestResult::Command(result) => result,
            RequestResult::Plan(_) => unreachable!(),
        };
        let bytes = match &result {
            CommandResult::Completed {
                bytes: Some(bytes), ..
            } => Some(bytes.clone()),
            _ => None,
        };
        let mut value = result_value(&input.path, &result).map_err(ToolFailure::internal)?;
        if let Some(bytes) = bytes {
            self.workspace
                .create_exclusive(&input.path, &bytes)
                .map_err(ToolFailure::host)?;
            if let Value::Object(map) = &mut value {
                map.remove("bytes");
                map.insert("written".into(), Value::Bool(true));
            }
        }
        Ok(value)
    }

    fn read(&self, input: ReadInput) -> Result<Value, ToolFailure> {
        let document = self
            .workspace
            .read_existing(&input.path)
            .map_err(ToolFailure::host)?;
        let kind = input.kind.unwrap_or_else(|| "document".into());
        let result = match execute_request(
            Some(&document.bytes),
            &Request::Command(CommandRequest {
                command: Command::Read(ReadCommand {
                    read_kind: Some(kind.clone()),
                    view: if kind == "document" { input.view } else { None },
                }),
                expected_source: None,
            }),
        ) {
            RequestResult::Command(result) => result,
            RequestResult::Plan(_) => unreachable!(),
        };
        result_value(&input.path, result).map_err(ToolFailure::internal)
    }

    fn find(&self, input: FindInput) -> Result<Value, ToolFailure> {
        let document = self
            .workspace
            .read_existing(&input.path)
            .map_err(ToolFailure::host)?;
        let result = match execute_request(
            Some(&document.bytes),
            &Request::Command(CommandRequest {
                command: Command::Find(FindCommand {
                    query: input.query,
                    ignore_case: input.ignore_case,
                    view: None,
                }),
                expected_source: None,
            }),
        ) {
            RequestResult::Command(result) => result,
            RequestResult::Plan(_) => unreachable!(),
        };
        result_value(&input.path, result).map_err(ToolFailure::internal)
    }

    fn edit(&self, input: EditInput) -> Result<Value, ToolFailure> {
        let document = self
            .workspace
            .read_existing(&input.path)
            .map_err(ToolFailure::host)?;
        let preview_key = input.preview_key.map(PreviewKey);
        let plan = match parse_plan(&self.workspace, &document.bytes, input.plan)? {
            ParsedPlan::Plan(plan) => plan,
            ParsedPlan::Rejected(diagnostic) => {
                return Ok(with_path(
                    &input.path,
                    serde_json::to_value(PlanResult::Rejected {
                        source: Some(document.source),
                        plan: None,
                        diagnostic,
                        report: None,
                    })
                    .map_err(|error| ToolFailure::internal(error.to_string()))?,
                ));
            }
        };
        let result = match execute_request(
            Some(&document.bytes),
            &Request::Plan(PlanRequest { plan, preview_key }),
        ) {
            RequestResult::Plan(result) => result,
            RequestResult::Command(_) => unreachable!(),
        };
        let should_write = matches!(result, PlanResult::Committed { .. });
        let bytes = match &result {
            PlanResult::Committed { bytes, .. } => Some(bytes.clone()),
            _ => None,
        };
        let mut value = result_value(&input.path, &result).map_err(ToolFailure::internal)?;
        if should_write {
            let bytes = bytes.ok_or_else(|| {
                ToolFailure::internal("committed result did not contain output bytes")
            })?;
            self.workspace
                .write_existing_atomic(&input.path, &document.source, &bytes)
                .map_err(ToolFailure::host)?;
            if let Value::Object(map) = &mut value {
                map.remove("bytes");
                map.insert("written".into(), Value::Bool(true));
            }
        }
        Ok(value)
    }

    fn help(&self, input: HelpInput) -> Result<Value, ToolFailure> {
        Ok(json!({
            "outcome": "completed",
            "topic": input.topic.unwrap_or_else(|| "all".into()),
            "tools": TOOL_NAMES,
            "plan": {
                "operations": edit_op_schema(),
                "preview": "preview_key required for commit"
            },
            "read": {
                "kinds": ["document", "styles", "comments", "revisions", "assets"],
                "document_surface": "HTML with MathML"
            }
        }))
    }
}

impl ServerHandler for DocxServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        TOOL_NAMES
            .iter()
            .find(|candidate| **candidate == name)
            .map(|_| tool_definition(name))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: self.tools(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        if !TOOL_NAMES.contains(&name.as_str()) {
            return Err(ErrorData::invalid_params(
                format!("unknown tool: {name}"),
                None,
            ));
        }
        let arguments = request
            .arguments
            .map(Value::Object)
            .unwrap_or_else(|| json!({}));
        match self.call_named_internal(&name, arguments) {
            Ok(value) if value.get("outcome") == Some(&Value::String("rejected".into())) => {
                Ok(CallToolResult::structured_error(value).into())
            }
            Ok(value) => Ok(CallToolResult::structured(value).into()),
            Err(error) => Ok(CallToolResult::structured_error(json!({
                "outcome": "rejected",
                "diagnostic": {"code": error.code, "message": error.message}
            }))
            .into()),
        }
    }
}

fn parse_arguments<T: for<'de> Deserialize<'de>>(arguments: Value) -> Result<T, ToolFailure> {
    serde_json::from_value(arguments)
        .map_err(|error| ToolFailure::invalid(format!("invalid tool arguments: {error}")))
}

enum ParsedPlan {
    Plan(Plan),
    Rejected(docxdriver_core::Diagnostic),
}

fn parse_plan(
    workspace: &Workspace,
    bytes: &[u8],
    value: Value,
) -> Result<ParsedPlan, ToolFailure> {
    let object = value
        .as_object()
        .ok_or_else(|| ToolFailure::invalid("plan must be an object"))?;
    if let Some(file) = object.get("file").and_then(Value::as_str) {
        let text = workspace
            .read_text_existing(file)
            .map_err(ToolFailure::host)?;
        return match docxdriver_core::parse_plan_toml(&text) {
            Ok(plan) => Ok(ParsedPlan::Plan(plan)),
            Err(diagnostic) => Ok(ParsedPlan::Rejected(diagnostic)),
        };
    }
    let operations = object
        .get("operations")
        .cloned()
        .ok_or_else(|| ToolFailure::invalid("inline plan requires operations"))?;
    let ops: Vec<EditOp> = serde_json::from_value(operations)
        .map_err(|error| ToolFailure::invalid(format!("invalid plan operations: {error}")))?;
    let author = object
        .get("author")
        .and_then(Value::as_str)
        .unwrap_or("docxdriver")
        .to_string();
    let change_mode = object
        .get("change_mode")
        .cloned()
        .unwrap_or_else(|| Value::String("track".into()));
    let change_mode: ChangeMode = serde_json::from_value(change_mode)
        .map_err(|error| ToolFailure::invalid(format!("invalid change_mode: {error}")))?;
    Ok(ParsedPlan::Plan(Plan {
        base: SourceHash::from_bytes(bytes),
        author,
        change_mode,
        ops,
    }))
}

fn result_value<T: serde::Serialize>(path: &str, result: T) -> Result<Value, String> {
    let mut value = serde_json::to_value(result).map_err(|error| error.to_string())?;
    if let Some(map) = value.as_object_mut() {
        map.remove("bytes");
        map.remove("plan");
        map.remove("canonical_toml");
        if map.get("outcome").and_then(Value::as_str) == Some("committed") {
            if let Some(ops) = map
                .get_mut("report")
                .and_then(|r| r.get_mut("ops"))
                .and_then(Value::as_array_mut)
            {
                for op in ops {
                    if let Some(op) = op.as_object_mut() {
                        op.remove("affected");
                    }
                }
            }
        }
    }
    Ok(with_path(path, value))
}

fn with_path(path: &str, mut value: Value) -> Value {
    if let Value::Object(map) = &mut value {
        map.insert("path".into(), Value::String(path.into()));
    }
    value
}

fn tool_definition(name: &str) -> Tool {
    let (description, schema, read_only, destructive) = match name {
        "docx_create" => (
            "Create a new DOCX and refuse to overwrite an existing path.",
            create_schema(),
            false,
            true,
        ),
        "docx_read" => (
            "Read document, styles, comments, or revisions.",
            read_schema(),
            true,
            false,
        ),
        "docx_find" => (
            "Find text and return bounded document context.",
            find_schema(),
            true,
            false,
        ),
        "docx_edit" => (
            "Preview or commit a typed DOCX plan; preview_key is required to write.",
            edit_schema(),
            false,
            true,
        ),
        "docx_help" => (
            "Return the DOCX tool and typed-plan reference.",
            help_schema(),
            true,
            false,
        ),
        _ => unreachable!("tool definition requested for unknown tool"),
    };
    Tool::new(name.to_owned(), description, Arc::new(schema)).with_annotations(
        ToolAnnotations::new()
            .read_only(read_only)
            .destructive(destructive)
            .open_world(false),
    )
}

fn object_schema(
    properties: impl IntoIterator<Item = (&'static str, Value)>,
    required: &[&str],
) -> Map<String, Value> {
    let mut schema = Map::new();
    schema.insert("type".into(), Value::String("object".into()));
    schema.insert("additionalProperties".into(), Value::Bool(false));
    schema.insert(
        "properties".into(),
        Value::Object(
            properties
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        ),
    );
    schema.insert(
        "required".into(),
        Value::Array(
            required
                .iter()
                .map(|value| Value::String((*value).into()))
                .collect(),
        ),
    );
    schema
}

fn path_schema() -> Value {
    json!({
        "type": "string",
        "description": "Path to a .docx file, relative to the workspace root or absolute beneath it"
    })
}

fn create_schema() -> Map<String, Value> {
    let mut schema = object_schema(
        [
            ("path", path_schema()),
            (
                "paragraphs",
                json!({"type": "array", "items": {"type": "string"}}),
            ),
            ("html", json!({"type": "string"})),
        ],
        &["path"],
    );
    schema.insert(
        "oneOf".into(),
        json!([
            {"required": ["paragraphs"], "not": {"required": ["html"]}},
            {"required": ["html"], "not": {"required": ["paragraphs"]}}
        ]),
    );
    schema
}

fn read_schema() -> Map<String, Value> {
    object_schema(
        [
            ("path", path_schema()),
            (
                "kind",
                json!({"type": "string", "enum": ["document", "styles", "comments", "revisions", "assets"]}),
            ),
            (
                "view",
                json!({"type": "string", "enum": ["markup", "final", "original"]}),
            ),
        ],
        &["path"],
    )
}

fn find_schema() -> Map<String, Value> {
    object_schema(
        [
            ("path", path_schema()),
            ("query", json!({"type": "string"})),
            ("ignore_case", json!({"type": "boolean"})),
        ],
        &["path", "query"],
    )
}

fn edit_op_schema() -> Value {
    let mut value =
        serde_json::to_value(schema_for!(EditOp)).expect("EditOp schema must serialize");
    let definitions = value
        .as_object_mut()
        .and_then(|root| root.remove("definitions"))
        .unwrap_or_else(|| Value::Object(Map::new()));
    let mut defs = Map::new();
    if let Value::Object(entries) = definitions {
        for (name, mut definition) in entries {
            rewrite_schema_refs(&mut definition);
            defs.insert(name, definition);
        }
    }
    rewrite_schema_refs(&mut value);
    defs.insert(
        "ParagraphAddress".into(),
        json!({
            "type": "string",
            "pattern": "^(?:[A-F0-9]{8}|\\$[A-Za-z_][A-Za-z0-9_]*)$"
        }),
    );
    defs.insert(
        "InsertAnchor".into(),
        json!({
            "type": "string",
            "pattern": "^(?:[A-F0-9]{8}|\\$[A-Za-z_][A-Za-z0-9_]*|table:[1-9][0-9]*)$"
        }),
    );
    defs.insert(
        "Points".into(),
        json!({"type": "number", "minimum": 0, "multipleOf": 0.05}),
    );
    defs.insert(
        "RevisionTarget".into(),
        json!({"type": "string", "pattern": "^(?:all|[0-9]+)$"}),
    );
    if let Value::Object(root) = &mut value {
        root.insert("$defs".into(), Value::Object(defs));
        root.insert(
            "$schema".into(),
            Value::String("https://json-schema.org/draft/2020-12/schema".into()),
        );
    }
    value
}

fn rewrite_schema_refs(value: &mut Value) {
    match value {
        Value::String(text) => {
            if let Some(rest) = text.strip_prefix("#/definitions/") {
                *text = format!("#/$defs/{rest}");
            }
        }
        Value::Array(items) => {
            for item in items {
                rewrite_schema_refs(item);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                rewrite_schema_refs(item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn edit_schema() -> Map<String, Value> {
    let operation_schema = edit_op_schema();
    let inline = object_schema(
        [
            (
                "operations",
                json!({"type": "array", "minItems": 1, "items": operation_schema}),
            ),
            ("author", json!({"type": "string"})),
            (
                "change_mode",
                json!({"type": "string", "enum": ["track", "direct"]}),
            ),
        ],
        &["operations"],
    );
    let file = object_schema(
        [(
            "file",
            json!({"type": "string", "description": "TOML plan path beneath the workspace root"}),
        )],
        &["file"],
    );
    object_schema(
        [
            ("path", path_schema()),
            ("plan", json!({"anyOf": [inline, file]})),
            ("preview_key", json!({"type": "string"})),
        ],
        &["path", "plan"],
    )
}

fn help_schema() -> Map<String, Value> {
    object_schema([("topic", json!({"type": "string"}))], &[])
}
