//! Times the read-only commands the UI polls. Uses the real app data dir.
use ols_core::{AppPaths, Core, CoreCommand, SettingsService};
use std::time::Instant;

fn main() {
    let paths = AppPaths::resolve();
    let settings = SettingsService::load(&paths).unwrap();
    let core = Core::new(settings, paths);
    let cmds: Vec<(&str, CoreCommand)> = vec![
        ("get_dashboard", CoreCommand::GetDashboard),
        ("get_web_status", CoreCommand::GetWebStatus),
        ("get_web_config", CoreCommand::GetWebConfig),
        ("list_domains", CoreCommand::ListDomains),
        ("list_certificates", CoreCommand::ListCertificates),
        ("get_ca_info", CoreCommand::GetCaInfo),
        ("list_projects", CoreCommand::ListProjects),
        ("list_runtime_catalog", CoreCommand::ListRuntimeCatalog),
        ("list_services", CoreCommand::ListServices),
        ("list_processes", CoreCommand::ListProcesses),
        ("get_environment_health", CoreCommand::GetEnvironmentHealth),
        ("list_web_configs", CoreCommand::ListWebConfigs),
    ];
    for (name, cmd) in cmds {
        for i in 0..2 {
            let t = Instant::now();
            let r = core.dispatch(cmd.clone());
            println!(
                "{name:26} #{i} {:>6} ms {}",
                t.elapsed().as_millis(),
                if r.is_ok() { "ok" } else { "ERR" }
            );
        }
    }
}
