use docxdriver_mcp::{http_router_with_token, DocxServer};
use rmcp::{
    model::{CallToolRequestParams, ClientInfo, ProtocolVersion},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    ClientLifecycleMode, ClientServiceExt, ServiceExt,
};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
#[cfg(unix)]
use std::thread;
use tokio_util::sync::CancellationToken;

fn fixture_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test-docs/ctnf-18690238-data-stream.docx")
}

#[test]
fn exposes_exactly_the_five_docx_tools() {
    let server = DocxServer::new(fixture_root()).unwrap();
    let mut names = server
        .tools()
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        [
            "docx_create",
            "docx_edit",
            "docx_find",
            "docx_help",
            "docx_read"
        ]
    );
    let edit = server
        .tools()
        .into_iter()
        .find(|tool| tool.name == "docx_edit")
        .unwrap();
    let schema = serde_json::to_value(edit.input_schema.as_ref()).unwrap();
    assert_eq!(
        schema["properties"]["plan"]["anyOf"][0]["properties"]["operations"]["items"]["$defs"]
            ["ParagraphAddress"]["type"],
        "string"
    );
    assert_eq!(
        schema["properties"]["plan"]["anyOf"][0]["properties"]["operations"]["items"]["$defs"]
            ["Points"]["type"],
        "number"
    );
    let create = server
        .tools()
        .into_iter()
        .find(|tool| tool.name == "docx_create")
        .unwrap();
    let create_schema = serde_json::to_value(create.input_schema.as_ref()).unwrap();
    assert_eq!(create_schema["oneOf"].as_array().unwrap().len(), 2);
}

#[test]
fn unknown_tool_is_rejected_without_touching_files() {
    let server = DocxServer::new(fixture_root()).unwrap();
    let error = server
        .call_named("not_a_docx_tool", serde_json::json!({}))
        .unwrap_err();
    assert!(error.contains("unknown tool"));
}

#[test]
fn create_and_read_stay_inside_workspace() {
    let root = tempfile::tempdir().unwrap();
    let server = DocxServer::new(root.path()).unwrap();
    let created = server
        .call_named(
            "docx_create",
            serde_json::json!({"path": "new.docx", "paragraphs": ["hello"]}),
        )
        .unwrap();
    assert_eq!(created["outcome"], "completed");
    assert!(root.path().join("new.docx").is_file());
    let read = server
        .call_named("docx_read", serde_json::json!({"path": "new.docx"}))
        .unwrap();
    assert_eq!(read["outcome"], "completed");
}

#[test]
fn edit_requires_preview_before_commit_and_writes_atomically() {
    let root = tempfile::tempdir().unwrap();
    std::fs::copy(fixture_path(), root.path().join("work.docx")).unwrap();
    let server = DocxServer::new(root.path()).unwrap();
    let document = server
        .call_named("docx_read", serde_json::json!({"path": "work.docx"}))
        .unwrap();
    let paragraph_id = document["result"]["markup"]
        .as_str()
        .unwrap()
        .split_once("id=\"")
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0;
    let plan = serde_json::json!({
        "operations": [{"op": "replace_paragraph", "at": paragraph_id, "with": "changed"}]
    });
    let preview = server
        .call_named(
            "docx_edit",
            serde_json::json!({"path": "work.docx", "plan": plan}),
        )
        .unwrap();
    assert_eq!(preview["outcome"], "previewed");
    let committed = server
        .call_named(
            "docx_edit",
            serde_json::json!({
                "path": "work.docx",
                "plan": plan,
                "preview_key": preview["preview_key"]
            }),
        )
        .unwrap();
    assert_eq!(committed["outcome"], "committed");
    assert_eq!(committed["written"], true);
}

#[test]
fn stale_preview_is_rejected_without_writing() {
    let root = tempfile::tempdir().unwrap();
    std::fs::copy(fixture_path(), root.path().join("work.docx")).unwrap();
    let server = DocxServer::new(root.path()).unwrap();
    let document = server
        .call_named("docx_read", serde_json::json!({"path": "work.docx"}))
        .unwrap();
    let paragraph_id = document["result"]["markup"]
        .as_str()
        .unwrap()
        .split_once("id=\"")
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0;
    let plan = serde_json::json!({
        "operations": [{"op": "replace_paragraph", "at": paragraph_id, "with": "changed"}]
    });
    let preview = server
        .call_named(
            "docx_edit",
            serde_json::json!({"path": "work.docx", "plan": plan}),
        )
        .unwrap();
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docxdriver-core/assets/blank.docx"),
        root.path().join("work.docx"),
    )
    .unwrap();
    let result = server
        .call_named(
            "docx_edit",
            serde_json::json!({
                "path": "work.docx",
                "plan": plan,
                "preview_key": preview["preview_key"]
            }),
        )
        .unwrap();
    assert_eq!(result["outcome"], "rejected");
    assert_eq!(result["diagnostic"]["code"], "preview_key_mismatch");
}

#[tokio::test]
async fn stdio_transport_exposes_tools_and_calls_help() {
    let root = tempfile::tempdir().unwrap();
    let (server_transport, client_transport) = tokio::io::duplex(16 * 1024);
    let server_root = root.path().to_path_buf();
    let server_task = tokio::spawn(async move {
        let server = DocxServer::new(server_root).unwrap();
        server
            .serve(server_transport)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap();
    });
    let client = ().serve(client_transport).await.unwrap();
    let tools = client.list_tools(None).await.unwrap();
    assert_eq!(tools.tools.len(), 5);
    let result = client
        .call_tool(CallToolRequestParams::new("docx_help"))
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(false));
    assert!(result
        .structured_content
        .as_ref()
        .and_then(|value| value.get("tools"))
        .is_some());
    let escaped = client
        .call_tool(
            CallToolRequestParams::new("docx_read").with_arguments(
                serde_json::json!({"path": "../outside.docx"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    assert_eq!(escaped.is_error, Some(true));
    assert_eq!(
        escaped.structured_content.as_ref().unwrap()["diagnostic"]["code"],
        "path_escape"
    );
    client.cancel().await.unwrap();
    server_task.await.unwrap();
}

#[tokio::test]
async fn http_transport_exposes_tools_without_protocol_sessions() {
    let root = tempfile::tempdir().unwrap();
    let cancellation = CancellationToken::new();
    let router = http_router_with_token(
        DocxServer::new(root.path()).unwrap(),
        cancellation.child_token(),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = cancellation.clone();
    let server_task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { shutdown.cancelled().await })
            .await
            .unwrap();
    });
    let url = format!("http://{address}/mcp");
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(url.clone()),
    );
    let client = ClientInfo::default()
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .unwrap();
    let tools = client.list_tools(None).await.unwrap();
    assert_eq!(tools.tools.len(), 5);

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "tools/list",
        "params": {"_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientInfo": {"name": "test", "version": "1"},
            "io.modelcontextprotocol/clientCapabilities": {}
        }}
    });
    let raw = reqwest::Client::new()
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2026-07-28")
        .header("Mcp-Method", "tools/list")
        .header("Mcp-Name", "tools/list")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(raw.status().is_success());
    assert!(raw.headers().get("Mcp-Session-Id").is_none());

    let hostile = reqwest::Client::new()
        .post(&url)
        .header("Host", "evil.example")
        .body(body.to_string())
        .send()
        .await
        .unwrap();
    assert!(!hostile.status().is_success());

    client.cancel().await.unwrap();
    cancellation.cancel();
    server_task.await.unwrap();
}

#[test]
fn relative_parent_escape_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let server = DocxServer::new(root.path()).unwrap();
    let error = server
        .call_named(
            "docx_create",
            serde_json::json!({"path": "../outside.docx", "paragraphs": ["no"]}),
        )
        .unwrap_err();
    assert!(error.contains("path_escape"));
}

#[cfg(unix)]
#[test]
fn existing_symlink_escape_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::copy(fixture_path(), outside.path().join("outside.docx")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("outside.docx"),
        root.path().join("link.docx"),
    )
    .unwrap();
    let server = DocxServer::new(root.path()).unwrap();
    let error = server
        .call_named("docx_read", serde_json::json!({"path": "link.docx"}))
        .unwrap_err();
    assert!(error.contains("path_escape"));
}

#[cfg(unix)]
#[test]
fn new_path_through_external_symlink_does_not_create_outside() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
    let server = DocxServer::new(root.path()).unwrap();
    let error = server
        .call_named(
            "docx_create",
            serde_json::json!({"path": "link/nested/new.docx", "paragraphs": ["no"]}),
        )
        .unwrap_err();
    assert!(error.contains("path_escape"));
    assert!(!outside.path().join("nested").exists());
}

#[cfg(unix)]
#[test]
fn concurrent_parent_swap_never_reads_outside_workspace() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docxdriver-core/assets/blank.docx"),
        root.path().join("safe.docx"),
    )
    .unwrap();
    std::fs::copy(fixture_path(), outside.path().join("outside.docx")).unwrap();
    std::fs::hard_link(root.path().join("safe.docx"), root.path().join("race.docx")).unwrap();
    let server = DocxServer::new(root.path()).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_attacker = stop.clone();
    let race = root.path().join("race.docx");
    let outside_file = outside.path().join("outside.docx");
    let attacker = thread::spawn(move || {
        while !stop_attacker.load(Ordering::Relaxed) {
            let _ = std::fs::remove_file(&race);
            let _ = std::os::unix::fs::symlink(&outside_file, &race);
            let _ = std::fs::remove_file(&race);
            let _ = std::fs::hard_link(race.parent().unwrap().join("safe.docx"), &race);
        }
    });
    for _ in 0..500 {
        if let Ok(result) = server.call_named("docx_read", serde_json::json!({"path": "race.docx"}))
        {
            assert!(!result.to_string().contains("DETAILED ACTION"));
        }
    }
    stop.store(true, Ordering::Relaxed);
    attacker.join().unwrap();
}
