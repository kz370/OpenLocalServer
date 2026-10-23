use ols_core::{Core, SettingsService};
use ols_mcp::run_stdio;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let paths = ols_core::paths::AppPaths::resolve();
    let settings = SettingsService::load(&paths)?;
    let core = Core::new(settings, paths);
    run_stdio(core).await?;
    Ok(())
}
