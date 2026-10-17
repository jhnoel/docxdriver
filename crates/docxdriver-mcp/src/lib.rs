mod server;
mod workspace;

pub use server::DocxServer;

use axum::Router;
use rmcp::service::ServiceExt;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

pub async fn serve_stdio(server: DocxServer) -> Result<(), String> {
    let running = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|error| format!("start stdio MCP server: {error}"))?;
    running
        .waiting()
        .await
        .map_err(|error| format!("stdio MCP server failed: {error}"))?;
    Ok(())
}

pub fn http_router(server: DocxServer) -> Router {
    http_router_with_token(server, CancellationToken::new())
}

pub fn http_router_with_token(server: DocxServer, cancellation: CancellationToken) -> Router {
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_cancellation_token(cancellation);
    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    Router::new().nest_service("/mcp", service)
}

pub async fn serve_http(server: DocxServer, listen: SocketAddr) -> Result<(), String> {
    let cancellation = CancellationToken::new();
    let router = http_router_with_token(server, cancellation.clone());
    let listener = TcpListener::bind(listen)
        .await
        .map_err(|error| format!("bind {listen}: {error}"))?;
    let actual = listener
        .local_addr()
        .map_err(|error| format!("read listener address: {error}"))?;
    eprintln!("docxdriver-mcp listening at http://{actual}/mcp");

    let shutdown = cancellation.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            shutdown.cancel();
        }
    });
    axum::serve(listener, router)
        .with_graceful_shutdown(async move { cancellation.cancelled().await })
        .await
        .map_err(|error| format!("HTTP MCP server failed: {error}"))
}
