mod server;
#[cfg(feature = "server")]
mod transport;
mod workspace;

pub use server::DocxServer;

// stdio and Streamable HTTP transports; off with default-features = false.
#[cfg(feature = "server")]
pub use transport::{http_router, http_router_with_token, serve_http, serve_stdio};
