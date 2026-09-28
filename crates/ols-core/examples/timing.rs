//! Times the read-only commands the UI polls. Uses the real app data dir.
use ols_core::{AppPaths, Core, CoreCommand, SettingsService};
use std::time::Instant;

fn main() {
    eprintln!("1. Resolving paths");
    let paths = AppPaths::resolve();
    eprintln!("2. Loading settings");
    let settings = SettingsService::load(&paths).unwrap();
    eprintln!("3. Creating core");
    let core = Core::new(settings, paths);
    eprintln!("4. Core created successfully");
    eprintln!("Test A: web_config");
    let cfg = core.inner().web_config();
    eprintln!("Test A done: default_server={}", cfg.default_server);

    eprintln!("Test B: services.list()");
    let svcs = core.inner().services.list();
    eprintln!("Test B done: {} services", svcs.len());

    eprintln!("Test C: domains.lock()");
    let doms = core.inner().domains.lock().unwrap().list();
    eprintln!("Test C done: {} domains", doms.len());

    eprintln!("Test D: web.status()");
    let web_status = core.inner().web.status(&cfg, &doms);
    eprintln!("Test D done: running={}", web_status.running);

    eprintln!("Test E: domain_summaries()");
    let summaries = core.inner().domain_summaries();
    eprintln!("Test E done: {} summaries", summaries.len());

    eprintln!("Test F: projects.lock()");
    let projs = core.inner().projects.lock().unwrap().list();
    eprintln!("Test F done: {} projects", projs.len());

    eprintln!("Test G: environment_health()");
    let health = core.inner().environment_health();
    eprintln!("Test G done: {} health items", health.len());

    eprintln!("Test H: core.dispatch(GetDashboard)");
    let res = core.dispatch(CoreCommand::GetDashboard);
    eprintln!("Test H done: {:?}", res.is_ok());
}
