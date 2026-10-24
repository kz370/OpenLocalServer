use ols_mcp::run_stdio;
use std::env;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    fmt().with_env_filter(EnvFilter::from_default_env()).init();

    // --token <value> or OLS_MCP_TOKEN env var: bearer token that every MCP
    // client must send. Set either in the MCP host config (args / env) so the
    // token lives next to the command, not inside every tools/call.
    let token = env::args()
        .skip(1)
        .collect::<Vec<_>>()
        .windows(2)
        .find_map(|w| {
            if w[0] == "--token" {
                Some(w[1].clone())
            } else {
                None
            }
        })
        .or_else(|| env::var("OLS_MCP_TOKEN").ok());

    run_stdio(token).await?;
    Ok(())
}
