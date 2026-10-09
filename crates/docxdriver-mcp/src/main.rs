use clap::{Parser, ValueEnum};
use docxdriver_mcp::{serve_http, serve_stdio, DocxServer};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Transport {
    Stdio,
    Http,
}

#[derive(Debug, Parser)]
#[command(
    name = "docxdriver-mcp",
    about = "MCP server for safe, plan-based DOCX workflows",
    version
)]
struct Cli {
    /// Workspace root. All document and plan paths must remain beneath it.
    #[arg(long, env = "DOCXDRIVER_MCP_ROOT", default_value = ".")]
    root: PathBuf,

    /// MCP transport. stdio is suitable for client-managed local processes;
    /// http exposes a stateless Streamable HTTP endpoint on loopback.
    #[arg(long, value_enum, default_value_t = Transport::Stdio)]
    transport: Transport,

    /// HTTP listen address when --transport=http.
    #[arg(long, default_value = "127.0.0.1:39200")]
    listen: SocketAddr,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let server = match DocxServer::new(&cli.root) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("docxdriver-mcp: {error}");
            return ExitCode::from(2);
        }
    };
    let result = match cli.transport {
        Transport::Stdio => serve_stdio(server).await,
        Transport::Http => serve_http(server, cli.listen).await,
    };
    if let Err(error) = result {
        eprintln!("docxdriver-mcp: {error}");
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
