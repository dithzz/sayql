//! `askdb-mcp`: AskDB's MCP server for Claude, over stdio. See src/mcp.rs.

#[tokio::main]
async fn main() {
    if let Err(e) = askdb_lib::mcp::serve_stdio().await {
        // stdout carries the protocol; errors go to stderr.
        eprintln!("askdb-mcp: {e}");
        std::process::exit(1);
    }
}
