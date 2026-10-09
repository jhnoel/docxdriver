# docxdriver MCP Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a self-contained Rust `docxdriver-mcp` binary that exposes the existing five DOCX interfaces over either stdio or localhost stateless Streamable HTTP.

**Architecture:** Add a new workspace crate with one transport-independent `DocxServer` implementation backed directly by `docxdriver-core`. The binary selects `stdio` (default) or `http`; stdio uses rmcp's server-side stdio transport, while HTTP uses rmcp's stateless Streamable HTTP service bound to loopback. Filesystem resolution and atomic writes are performed inside the server's explicit workspace root.

**Tech Stack:** Rust 2021, `docxdriver-core`, `cap-std` capability-scoped filesystem, official `rmcp` 3.1.x SDK, Tokio, Axum, Clap, Serde/JSON, tempfile tests.

## Global Constraints

- The MCP surface contains exactly `docx_create`, `docx_read`, `docx_find`, `docx_edit`, and `docx_help`.
- `docx_edit` is preview-first; a write requires the core-generated `preview_key` and revalidates the current source hash before atomic replacement.
- Relative and absolute paths must remain beneath the configured workspace root, including symlink resolution and write-time revalidation.
- stdio writes protocol messages only to stdout; diagnostics and the HTTP listening URL go to stderr.
- HTTP binds to loopback by default and uses rmcp stateless Streamable HTTP mode without protocol sessions.
- Existing CLI and core behavior must remain unchanged.

---

### Task 1: Add the MCP crate contract and failing transport-independent tests

**Files:**
- Modify: `Cargo.toml` — add `crates/docxdriver-mcp` to workspace members.
- Create: `crates/docxdriver-mcp/Cargo.toml` — declare the binary/library and runtime dependencies.
- Create: `crates/docxdriver-mcp/src/lib.rs` — public server/test API stubs only; no production behavior before tests.
- Create: `crates/docxdriver-mcp/src/main.rs` — binary entrypoint stub.
- Create: `crates/docxdriver-mcp/tests/mcp_contract.rs` — failing contract tests.

**Interfaces:**
- Produces `DocxServer::new(root: impl AsRef<Path>) -> Result<DocxServer, String>`.
- Produces `DocxServer::tools() -> Vec<rmcp::model::Tool>` with five stable names.
- Produces `DocxServer::call_named(name: &str, arguments: serde_json::Value) -> Result<serde_json::Value, String>` for transport-independent tests.

- [ ] **Step 1: Add the workspace member and crate manifest.**

Use rmcp's official server, stdio, and Streamable HTTP features; keep the crate's client feature out of the production dependency set.

```toml
[package]
name = "docxdriver-mcp"
description = "MCP server for the docxdriver DOCX engine"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true

[lib]
path = "src/lib.rs"

[[bin]]
name = "docxdriver-mcp"
path = "src/main.rs"

[dependencies]
docxdriver-core = { path = "../docxdriver-core" }
axum = { version = "0.8", default-features = false, features = ["http1", "tokio"] }
clap = { version = "4", features = ["derive", "env"] }
rmcp = { version = "3.1.2", default-features = false, features = ["server", "macros", "transport-io", "transport-streamable-http-server"] }
schemars = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tempfile = "3"
tokio = { version = "1", features = ["io-std", "macros", "net", "rt-multi-thread", "signal"] }
tokio-util = { version = "0.7", features = ["rt"] }

[dev-dependencies]
rmcp = { version = "3.1.2", default-features = false, features = ["client", "server", "transport-io", "transport-streamable-http-client-reqwest", "transport-streamable-http-server"] }
tempfile = "3"
```

- [ ] **Step 2: Write failing tests for the stable interface list and invalid tool names.**

```rust
#[test]
fn exposes_exactly_the_five_docx_tools() {
    let server = DocxServer::new(fixture_root()).unwrap();
    let mut names = server.tools().into_iter().map(|tool| tool.name.into_owned()).collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["docx_create", "docx_edit", "docx_find", "docx_help", "docx_read"]);
}

#[test]
fn unknown_tool_is_rejected_without_touching_files() {
    let server = DocxServer::new(fixture_root()).unwrap();
    let error = server.call_named("not_a_docx_tool", serde_json::json!({})).unwrap_err();
    assert!(error.contains("unknown tool"));
}
```

- [ ] **Step 3: Run the new test and verify it fails for the missing server implementation.**

Run: `cargo test -p docxdriver-mcp --test mcp_contract`

Expected: FAIL because `DocxServer` and its tool registry are not implemented.

- [ ] **Step 4: Add only the minimal public stubs needed for the tests to compile, then rerun.**

The test must still fail on the assertions rather than pass accidentally.

- [ ] **Step 5: Commit the red contract scaffold.**

```bash
git add Cargo.toml crates/docxdriver-mcp
git commit -m "test: define docxdriver MCP interface contract"
```

### Task 2: Implement workspace-safe DOCX operations

**Files:**
- Modify: `crates/docxdriver-mcp/src/lib.rs` — add `Workspace`, path checks, atomic writes, request adapters, and result formatting.
- Modify: `crates/docxdriver-mcp/tests/mcp_contract.rs` — add failing tests for create/read, preview/commit, and path escape refusal.

**Interfaces:**
- `Workspace::read_existing(path) -> Result<DocumentBytes, HostError>`.
- `Workspace::resolve_new(path) -> Result<PathBuf, HostError>`.
- `Workspace::write_existing_atomic(path, expected: &SourceHash, bytes: &[u8]) -> Result<(), HostError>`.
- `Workspace::create_exclusive(path, bytes: &[u8]) -> Result<(), HostError>`.
- `DocxServer::call_named` returns JSON objects containing `path` and the serialized core result; successful commits omit raw `bytes` after writing.

- [ ] **Step 1: Write failing tests for the core DOCX flows.**

Cover one behavior per test:

```rust
#[test]
fn create_and_read_stay_inside_workspace() {
    let root = tempfile::tempdir().unwrap();
    let server = DocxServer::new(root.path()).unwrap();
    let created = server.call_named("docx_create", serde_json::json!({
        "path": "new.docx",
        "paragraphs": ["hello"]
    })).unwrap();
    assert_eq!(created["outcome"], "completed");
    assert!(root.path().join("new.docx").is_file());
    let read = server.call_named("docx_read", serde_json::json!({"path": "new.docx"})).unwrap();
    assert_eq!(read["outcome"], "completed");
}

#[test]
fn edit_requires_preview_before_commit_and_writes_atomically() {
    let root = tempfile::tempdir().unwrap();
    std::fs::copy(fixture_path(), root.path().join("work.docx")).unwrap();
    let server = DocxServer::new(root.path()).unwrap();
    let document = server.call_named("docx_read", serde_json::json!({"path": "work.docx"})).unwrap();
    let paragraph_id = document["result"]["blocks"][0]["paragraphs"][0].as_str().unwrap();
    let plan = serde_json::json!({"operations": [{"op": "replace_paragraph", "at": paragraph_id, "with": "changed"}]});
    let preview = server.call_named("docx_edit", serde_json::json!({"path": "work.docx", "plan": plan})).unwrap();
    assert_eq!(preview["outcome"], "previewed");
    let committed = server.call_named("docx_edit", serde_json::json!({"path": "work.docx", "plan": plan, "preview_key": preview["preview_key"]})).unwrap();
    assert_eq!(committed["outcome"], "committed");
}

#[test]
fn existing_symlink_escape_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::copy(fixture_path(), outside.path().join("outside.docx")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("outside.docx"), root.path().join("link.docx")).unwrap();
    let server = DocxServer::new(root.path()).unwrap();
    let error = server.call_named("docx_read", serde_json::json!({"path": "link.docx"})).unwrap_err();
    assert!(error.contains("escapes workspace"));
}
```

The edit test should use a real source DOCX fixture and assert that preview returns `outcome == "previewed"`, commit returns `outcome == "committed"`, and the resulting source hash changes.

- [ ] **Step 2: Run the tests and verify the expected failures.**

Run: `cargo test -p docxdriver-mcp --test mcp_contract`

Expected: FAIL because workspace operations are not implemented.

- [ ] **Step 3: Implement workspace root resolution.**

Canonicalize the root once at server construction and open it as a `cap_std::fs::Dir`. For existing paths, perform lexical containment and capability-scoped reads; reject symlink escapes. For new paths, perform lexical containment and use capability-scoped parent creation, exclusive creation, and rename so concurrent path replacement cannot redirect operations outside the root.

- [ ] **Step 4: Implement atomic existing-file replacement and exclusive creation.**

Before writing an edit, reread the source and compare `SourceHash`. Write to a unique `create_new` temporary file, `sync_all`, reread/revalidate the source and parent, then rename. Create operations must use a temporary file plus an exclusive hard link so an existing destination cannot be overwritten.

- [ ] **Step 5: Implement the five core adapters.**

Use `docxdriver_core::execute_request`:

- `docx_create`: require exactly one of `paragraphs` or `html`; execute with no input; write exclusively.
- `docx_read`: read the document and send `Command::Read`, defaulting to `document` and `markup`.
- `docx_find`: read the document and send `Command::Find`.
- `docx_edit`: read the source, construct either an inline `Plan` from `operations` or parse a TOML plan file, send `Request::Plan`, and write only for `committed`.
- `docx_help`: return the five tools, supported read kinds, preview rule, and generated edit-operation schema.

Expected core rejections remain structured results; path and filesystem failures become structured MCP tool errors with a diagnostic code/message.

- [ ] **Step 6: Run the focused tests and then the workspace tests.**

Run: `cargo test -p docxdriver-mcp --test mcp_contract`

Expected: PASS.

Run: `cargo test --workspace`

Expected: PASS with all pre-existing tests unchanged.

- [ ] **Step 7: Commit the operation layer.**

```bash
git add crates/docxdriver-mcp/src/lib.rs crates/docxdriver-mcp/tests/mcp_contract.rs
git commit -m "feat: implement safe DOCX MCP tools"
```

### Task 3: Add rmcp stdio and stateless HTTP transports

**Files:**
- Modify: `crates/docxdriver-mcp/src/lib.rs` — add rmcp `ServerHandler`, transport constructors, and HTTP router.
- Modify: `crates/docxdriver-mcp/src/main.rs` — add CLI transport selection and startup.
- Modify: `crates/docxdriver-mcp/tests/mcp_contract.rs` — add in-process stdio and HTTP transport tests.

**Interfaces:**
- `pub async fn serve_stdio(server: DocxServer) -> Result<(), String>`.
- `pub fn http_router(server: DocxServer) -> axum::Router`.
- CLI defaults to stdio and supports `--transport http --listen 127.0.0.1:39200`.

- [ ] **Step 1: Write a failing in-process stdio test.**

Use `tokio::io::duplex`, rmcp's `ServiceExt`, and a client-side test handler. Initialize the server, call `tools/list`, assert the five tools, then call `docx_help` and assert structured JSON content.

- [ ] **Step 2: Run the stdio test and verify it fails before the transport exists.**

Run: `cargo test -p docxdriver-mcp --test mcp_contract stdio`

Expected: FAIL because `ServerHandler`/`serve_stdio` are not implemented.

- [ ] **Step 3: Implement the rmcp server handler.**

Expose tool metadata from the same five-tool registry and route `tools/call` through `call_named`. Return `CallToolResult::structured` for successful and expected tool-level failures. Keep the server `Clone + Send + Sync` by storing the canonical workspace in an `Arc`.

- [ ] **Step 4: Implement stdio startup.**

Run the server with Tokio and rmcp's `transport::stdio()`. Never write startup logs to stdout; use stderr/tracing only.

- [ ] **Step 5: Write a failing HTTP test.**

Start `http_router` on an ephemeral loopback listener and issue a Streamable HTTP `tools/list` request. Assert a successful JSON-RPC response and reject a non-loopback Host header.

- [ ] **Step 6: Run the HTTP test and verify the expected failure.**

Run: `cargo test -p docxdriver-mcp --test mcp_contract http`

Expected: FAIL until the router and stateless configuration exist.

- [ ] **Step 7: Implement stateless Streamable HTTP.**

Use `StreamableHttpServerConfig::default()` with `legacy_session_mode(false)` and `json_response(true)`, nest the service at `/mcp`, and preserve rmcp's default loopback Host validation. Add graceful Ctrl-C shutdown through the config cancellation token.

- [ ] **Step 8: Run focused transport tests and the full workspace suite.**

Run: `cargo test -p docxdriver-mcp --test mcp_contract`

Expected: PASS.

Run: `cargo test --workspace`

Expected: PASS.

- [ ] **Step 9: Commit transport support.**

```bash
git add crates/docxdriver-mcp/src crates/docxdriver-mcp/tests/mcp_contract.rs
git commit -m "feat: expose docxdriver MCP over stdio and HTTP"
```

### Task 4: Document installation and client composition

**Files:**
- Create: `crates/docxdriver-mcp/README.md` — build, stdio, HTTP, workspace-root, and client configuration examples.
- Modify: `README.md` — link to the MCP server and state that the Pi extension is no longer the distribution boundary.
- Modify: `crates/docxdriver-cli/README.md` — cross-link the MCP server where appropriate.
- Modify: `crates/docxdriver-mcp/tests/mcp_contract.rs` — add a CLI help smoke test if needed.

**Interfaces:**
- Document `cargo run -p docxdriver-mcp -- --transport stdio --root .`.
- Document `cargo run -p docxdriver-mcp -- --transport http --root . --listen 127.0.0.1:39200`.
- Document the `/mcp` endpoint and that the server exposes exactly five tools.

- [ ] **Step 1: Add installation and composition documentation.**

Explain that clients compose independent MCP servers, while the five DOCX tools share one server boundary. Include the safety model for relative paths, preview keys, and loopback HTTP.

- [ ] **Step 2: Run formatting, tests, and binary help.**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo run -p docxdriver-mcp -- --help
```

Expected: all pass; help shows stdio default, HTTP transport, root, and listen options.

- [ ] **Step 3: Commit documentation and final verification.**

```bash
git add README.md crates/docxdriver-cli/README.md crates/docxdriver-mcp/README.md
git commit -m "docs: document docxdriver MCP distribution"
```
