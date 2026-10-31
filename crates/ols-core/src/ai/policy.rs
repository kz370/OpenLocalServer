use std::time::Duration;

use super::types::*;
use crate::command::CoreCommand;
use crate::error::CoreError;
use crate::redact::redact_text;

pub const KINDS: &[&str] = &["lmstudio", "ollama", "huggingface", "openrouter", "custom"];

/// The assistant is the single AI surface; every request routes through it.
pub const FEATURES: &[(&str, &str)] = &[("assistant", "Chat Assistant")];

pub const APP_GUIDE_MD: &str = include_str!("../../catalog/user_manual.md");

pub(crate) const MAX_ROUNDS: usize = 6;
pub(crate) const MAX_ACTIONS: usize = 10;
pub(crate) const MAX_PART: usize = 12_000;
pub(crate) const MAX_TOTAL: usize = 48_000;
pub(crate) const TOOL_OUTPUT: usize = 8_000;
pub(crate) const IDLE: Duration = Duration::from_secs(120);

pub(crate) fn fail(msg: impl Into<String>) -> CoreError {
    CoreError::failed("The AI assistant couldn't do that.", msg)
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn yes() -> bool {
    true
}

pub(crate) fn key_name(provider_id: &str) -> String {
    format!("ai:{provider_id}")
}

pub fn is_local_url(url: &str) -> bool {
    let Some(parsed) = reqwest::Url::parse(url.trim()).ok() else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

pub(crate) fn url_host(url: &str) -> String {
    reqwest::Url::parse(url.trim())
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default()
}

pub(crate) fn get_key(provider_id: &str) -> Option<String> {
    crate::secrets::get_secret(&key_name(provider_id))
        .ok()
        .flatten()
        .filter(|k| !k.is_empty())
}

pub(crate) fn endpoint(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim().trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

pub(crate) fn pick_provider<'a>(cfg: &'a AiSettings, feature: &str) -> Option<&'a AiProvider> {
    cfg.features
        .get(feature)
        .and_then(|id| cfg.providers.iter().find(|p| &p.id == id))
        .or_else(|| cfg.providers.first())
}

// ------------------------------------------------------------------------------ text helpers

pub(crate) fn clean(text: &str) -> String {
    crate::support::mask_home(&redact_text(text))
}

pub(crate) fn head(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let n = text.floor_char_boundary(max);
    format!("{}\n[cut: {} more characters]", &text[..n], text.len() - n)
}

pub(crate) struct Fence {
    pub(crate) lang: String,
    pub(crate) body: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// The ``` fenced blocks of a model's answer.
pub(crate) fn fences(text: &str) -> Vec<Fence> {
    let mut out = Vec::new();
    let mut open: Option<(usize, String, Vec<&str>)> = None;
    for (n, line) in text.lines().enumerate() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("```") {
            match open.take() {
                Some((start, lang, body)) => out.push(Fence {
                    lang,
                    body: body.join("\n"),
                    start,
                    end: n,
                }),
                None => open = Some((n, rest.trim().to_ascii_lowercase(), Vec::new())),
            }
        } else if let Some((_, _, body)) = open.as_mut() {
            body.push(line);
        }
    }
    out
}

pub(crate) fn without_fences(text: &str, drop: &[&Fence]) -> String {
    text.lines()
        .enumerate()
        .filter(|(n, _)| !drop.iter().any(|f| *n >= f.start && *n <= f.end))
        .map(|(_, l)| l)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

// ------------------------------------------------------------------------------ what a plan may contain

/// The commands an answer may propose. Everything else is refused, whatever the model says.
pub fn allowed(cmd: &CoreCommand) -> bool {
    use CoreCommand as C;
    matches!(
        cmd,
        // Sites / domains
        C::AddDomain { .. }
            | C::UpdateDomain { .. }
            | C::RemoveDomain { .. }
            | C::SetDomainEnabled { .. }
            | C::BulkSetDomainsEnabled { .. }
            | C::BulkRemoveDomains { .. }
            | C::BulkSetDomainServer { .. }
            | C::DuplicateDomain { .. }
            | C::RenameDomain { .. }
            | C::SyncAutoDomains
            | C::RegisterProject { .. }
            | C::RelocateProject { .. }
            | C::RemoveProject { .. }
            | C::DeleteProject { .. }
            // Site bundles
            | C::ExportSites { .. }
            | C::ImportSites { .. }
            | C::PreviewSiteBundle { .. }
            | C::ExportSupportBundle { .. }
            // Services
            | C::StartService { .. }
            | C::StopService { .. }
            | C::RestartService { .. }
            | C::StopAll
            | C::SaveCustomService { .. }
            | C::RemoveCustomService { .. }
            | C::StartWorker { .. }
            | C::StopWorker { .. }
            | C::RestartWorker { .. }
            // Tunnels
            | C::StartTunnel { .. }
            | C::StopTunnel { .. }
            | C::SaveTunnel { .. }
            | C::RemoveTunnel { .. }
            | C::CheckTunnel { .. }
            | C::SetTunnelToken { .. }
            | C::SetTunnelPassword { .. }
            // Databases
            | C::CreateDatabase { .. }
            | C::BackupDatabase { .. }
            | C::RestoreDatabase { .. }
            | C::ListDatabases { .. }
            | C::ListDatabaseTables { .. }
            | C::ExecuteSql { .. }
            // Web servers
            | C::ApplyWeb { .. }
            | C::ValidateWeb
            | C::StopWeb
            | C::RestartSiteApp { .. }
            // Runtimes & extensions
            | C::InstallRuntime { .. }
            | C::RemoveRuntime { .. }
            | C::SetPhpExtension { .. }
            | C::InstallPhpExtension { .. }
            // Certificates, DNS & networking
            | C::SyncHosts
            | C::TrustCa
            | C::RegenerateCertificate { .. }
            | C::VerifySslFiles { .. }
            | C::IssueLetsEncrypt { .. }
            | C::SyncDdns { .. }
            | C::DetectPublicIp
            | C::SyncAllDdns { .. }
            // Profiles & snapshots
            | C::ApplyProfile { .. }
            | C::CreateSnapshot { .. }
            | C::RestoreSnapshot { .. }
            // Quick apps & project tools
            | C::StartQuickApp { .. }
            | C::ApplySetup { .. }
            | C::ApplyMailpitEnv { .. }
            | C::SetEnvValue { .. }
            | C::RunComposer { .. }
    )
}

pub fn destructive(cmd: &CoreCommand) -> bool {
    crate::repair::is_destructive(cmd)
        || matches!(
            cmd,
            CoreCommand::RemoveDomain { .. }
                | CoreCommand::DeleteProject { .. }
                | CoreCommand::BulkRemoveDomains { .. }
                | CoreCommand::RemoveTunnel { .. }
                | CoreCommand::RemoveCustomService { .. }
                | CoreCommand::RemoveRuntime { .. }
                | CoreCommand::StopAll
                | CoreCommand::RestoreDatabase { .. }
                | CoreCommand::RestoreSnapshot { .. }
                | CoreCommand::ImportSites { .. }
                | CoreCommand::SetEnvValue { .. }
        )
}

/// A proposed command as it will actually run: an imported Quick App never gets its approval, and nothing elevated.
pub(crate) fn normalise(cmd: CoreCommand) -> CoreCommand {
    match cmd {
        CoreCommand::StartQuickApp { id, values, .. } => CoreCommand::StartQuickApp {
            id,
            values,
            approval: None,
            allow_elevated: false,
        },
        other => other,
    }
}

pub(crate) const PLAN_DOC: &str = r#"Commands you may propose, as JSON with the "type" first. Nothing else is accepted:
- Sites / domains:
  - add_domain: {"type":"add_domain","domain":"shop.test"} (create a new site)
  - update_domain: {"type":"update_domain","hostname":"shop.test","php":"8.3","root":"C:\\sites\\shop","server":"nginx"}
  - remove_domain: {"type":"remove_domain","hostname":"shop.test"} (DESTRUCTIVE: deletes a site)
  - set_domain_enabled: {"type":"set_domain_enabled","hostname":"shop.test","enabled":false}
  - bulk_set_domains_enabled: {"type":"bulk_set_domains_enabled","hostnames":["a.test","b.test"],"enabled":true}
  - bulk_remove_domains: {"type":"bulk_remove_domains","hostnames":["a.test","b.test"]} (DESTRUCTIVE)
  - bulk_set_domain_server: {"type":"bulk_set_domain_server","hostnames":["a.test"],"server":"nginx"}
  - duplicate_domain: {"type":"duplicate_domain","hostname":"shop.test","new_hostname":"shop2.test"}
  - rename_domain: {"type":"rename_domain","hostname":"shop.test","new_hostname":"shop2.test"}
  - sync_auto_domains: {"type":"sync_auto_domains"}
  - register_project: {"type":"register_project","path":"C:\\projects\\shop"}
  - remove_project: {"type":"remove_project","id":"..."} (DESTRUCTIVE)
  - delete_project: {"type":"delete_project","id":"..."} (DESTRUCTIVE)
- Site bundles:
  - export_sites: {"type":"export_sites","hostnames":["shop.test"],"dest":"C:\\backup.zip","password":"..."}
  - import_sites: {"type":"import_sites","source":"C:\\backup.zip","password":"...","on_conflict":"skip","name":"shop"} (DESTRUCTIVE)
  - preview_site_bundle: {"type":"preview_site_bundle","source":"C:\\backup.zip"}
  - export_support_bundle: {"type":"export_support_bundle","dest":"C:\\support.zip"}
- Services:
  - start_service, stop_service, restart_service: {"type":"start_service","id":"mariadb"} (ids from list_services)
  - stop_all: {"type":"stop_all"} (DESTRUCTIVE)
  - save_custom_service: {"type":"save_custom_service","service":{"id":"...","name":"...","command":"...","args":[]}}
  - remove_custom_service: {"type":"remove_custom_service","id":"..."} (DESTRUCTIVE)
  - start_worker, stop_worker, restart_worker: {"type":"start_worker","id":"..."}
- Tunnels:
  - start_tunnel: {"type":"start_tunnel","id":"..."}
  - stop_tunnel: {"type":"stop_tunnel","id":"..."}
  - save_tunnel: {"type":"save_tunnel","tunnel":{"id":"","name":"...","provider":"cloudflare","target":"https://shop.test"}}
  - remove_tunnel: {"type":"remove_tunnel","id":"..."} (DESTRUCTIVE)
  - check_tunnel: {"type":"check_tunnel","id":"..."}
  - set_tunnel_token: {"type":"set_tunnel_token","provider":"cloudflare","token":"..."}
  - set_tunnel_password: {"type":"set_tunnel_password","id":"...","password":"..."}
- Databases:
  - create_database: {"type":"create_database","engine":"mariadb","name":"shop","charset":"utf8mb4","collation":"utf8mb4_unicode_ci"}
  - backup_database: {"type":"backup_database","engine":"mariadb","database":"shop"}
  - restore_database: {"type":"restore_database","engine":"mariadb","database":"shop","file":"C:\\backup.sql"} (DESTRUCTIVE)
  - list_databases: {"type":"list_databases","engine":"mariadb"}
- Web servers:
  - apply_web: {"type":"apply_web","overwrite":[]} (regenerate and reload web config); validate_web; stop_web
  - restart_site_app: {"type":"restart_site_app","hostname":"shop.test"}
- Runtimes & extensions:
  - install_runtime: {"type":"install_runtime","id":"php","version":"8.4.26"}
  - remove_runtime: {"type":"remove_runtime","id":"php","version":"8.4.26"} (DESTRUCTIVE)
  - set_php_extension: {"type":"set_php_extension","version":"8.4.26","extension":"xdebug","enabled":true}
  - install_php_extension: {"type":"install_php_extension","version":"8.4.26","extension":"xdebug"}
- Certificates, DNS & networking:
  - sync_hosts: {"type":"sync_hosts"}
  - trust_ca: {"type":"trust_ca"}
  - regenerate_certificate: {"type":"regenerate_certificate","hostname":"shop.test"}
  - verify_ssl_files: {"type":"verify_ssl_files","hostname":"shop.test"}
  - issue_lets_encrypt: {"type":"issue_lets_encrypt","hostname":"shop.test"}
  - sync_ddns: {"type":"sync_ddns","hostname":"home.example.com"}
  - detect_public_ip: {"type":"detect_public_ip"}
  - sync_all_ddns: {"type":"sync_all_ddns","force":false}
- Profiles & snapshots:
  - apply_profile: {"type":"apply_profile","id":"..."}
  - create_snapshot: {"type":"create_snapshot","label":"Before change","project_id":"...","include_files":true,"include_env":true,"databases":["shop"]}
  - restore_snapshot: {"type":"restore_snapshot","file":"C:\\snapshot.zip","project_id":"...","options":{}} (DESTRUCTIVE)
- Quick apps & project tools:
  - start_quick_app: {"type":"start_quick_app","id":"laravel","values":{"project_name":"shop"}} (needs user approval; never elevated)
  - apply_setup: {"type":"apply_setup","project_id":"...","dry_run":false}
  - apply_mailpit_env: {"type":"apply_mailpit_env","project_id":"...","file":".env"}
  - set_env_value: {"type":"set_env_value","project_id":"...","file":".env","key":"DB_HOST","value":"127.0.0.1"} (DESTRUCTIVE)
  - run_composer: {"type":"run_composer","project_id":"...","action":"install"}
To propose changes, end the answer with exactly one block:
```ols-plan
[{"label":"Start MariaDB","command":{"type":"start_service","id":"mariadb"}}]
```
Propose only what you are sure about, at most 10 steps, and use ids and names you were given from the tools. Do not propose anything when no command is needed."#;

pub(crate) const SYSTEM: &str = "You are the OpenLocalServer (OLS) Assistant, the single conversational front door for the whole application. OLS manages Nginx, Apache and Caddy, PHP/Node/Python runtimes, MariaDB, PostgreSQL, MongoDB, Redis, Mailpit, trusted local HTTPS domains, tunnels, and project manifests in .openlocalserver/. Be concrete and brief; say what you are unsure of. You cannot run anything yourself: the user approves every change. Secrets in what you are given are replaced by [redacted]; never ask for them.";
