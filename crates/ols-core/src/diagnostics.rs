//! DiagnosticEngine v1 (§112, Stage 11). Looks over the whole setup and reports what is
//! wrong as findings, each shaped Problem / Cause / Fix. A finding that has a safe fix
//! carries the exact `CoreCommand` that applies it, so the UI's [Fix] button is just
//! "run this command" and the engine never has a second, private way of changing things.
//!
//! Checks only read state. Findings the user has chosen to [Ignore] are remembered in the
//! `diagnostics.ignored` setting and come back flagged rather than dropped, so they can
//! be un-ignored.

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::command::CoreCommand;
use crate::domain::SiteKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    /// Stable per problem and subject ("cert_expired:shop.test"), so an Ignore sticks.
    pub id: String,
    pub severity: Severity,
    pub problem: String,
    pub cause: String,
    pub fix: String,
    /// The command the [Fix] button runs; `None` when the fix is something only the user can do.
    pub fix_command: Option<CoreCommand>,
    /// Extra facts behind the finding, for [Details].
    pub details: Vec<String>,
    pub ignored: bool,
}

struct Builder {
    findings: Vec<Finding>,
}

impl Builder {
    fn add(&mut self, id: impl Into<String>, severity: Severity, problem: impl Into<String>, cause: impl Into<String>, fix: impl Into<String>, fix_command: Option<CoreCommand>, details: Vec<String>) {
        self.findings.push(Finding {
            id: id.into(),
            severity,
            problem: problem.into(),
            cause: cause.into(),
            fix: fix.into(),
            fix_command,
            details,
            ignored: false,
        });
    }
}

impl Inner {
    /// Every finding, most severe first. Ignored ones are flagged and sorted last.
    pub fn diagnose(&self) -> Vec<Finding> {
        let mut b = Builder { findings: Vec::new() };
        self.check_web(&mut b);
        self.check_https(&mut b);
        self.check_sites(&mut b);
        self.check_services(&mut b);
        self.check_projects(&mut b);
        self.check_php(&mut b);

        let ignored = self.ignored_diagnostics();
        for f in &mut b.findings {
            f.ignored = ignored.contains(&f.id);
        }
        let rank = |s: Severity| match s {
            Severity::Error => 0,
            Severity::Warning => 1,
            Severity::Info => 2,
        };
        b.findings.sort_by_key(|f| (f.ignored, rank(f.severity)));
        b.findings
    }

    pub fn ignored_diagnostics(&self) -> Vec<String> {
        self.settings
            .lock()
            .unwrap()
            .get("diagnostics.ignored")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    pub fn set_diagnostic_ignored(&self, id: &str, ignore: bool) -> Result<(), crate::error::CoreError> {
        let mut list = self.ignored_diagnostics();
        list.retain(|x| x != id);
        if ignore {
            list.push(id.to_string());
        }
        self.settings.lock().unwrap().set("diagnostics.ignored".to_string(), serde_json::json!(list))?;
        Ok(())
    }

    fn check_web(&self, b: &mut Builder) {
        let cfg = self.web_config();
        let enabled = self.domains.lock().unwrap().list().into_iter().filter(|d| d.enabled).count();
        let server_name = crate::web::server_by_id(&cfg.server).map(|s| s.name()).unwrap_or("The web server");

        if self.runtimes.installed_versions(&cfg.server).is_empty() {
            let version = crate::catalog::builtin_catalog().into_iter().find(|m| m.id == cfg.server).map(|m| m.version.to_string());
            b.add(
                "web_not_installed",
                Severity::Error,
                format!("{server_name} is not installed"),
                "Sites are served by the web server chosen in the Sites settings, and its files are not on this computer.",
                format!("Install {server_name} from the Runtimes page."),
                version.map(|version| CoreCommand::InstallRuntime { id: cfg.server.clone(), version }),
                vec![],
            );
            return;
        }

        let web = self.web.status(&cfg);
        if !web.running && enabled > 0 {
            b.add(
                "web_stopped",
                Severity::Warning,
                "The web server is stopped, so your sites are offline",
                format!("{enabled} enabled site(s) are waiting for {server_name} to start."),
                "Apply the web config to start it.",
                web.port_conflicts.is_empty().then(|| CoreCommand::ApplyWeb { overwrite: vec![] }),
                vec![],
            );
        }
        for (i, conflict) in web.port_conflicts.iter().enumerate() {
            b.add(
                format!("port_conflict:{i}:{conflict}"),
                Severity::Error,
                "A web port is already used by another program",
                conflict.clone(),
                "Stop that program (IIS, Skype and other web servers are common), or choose other ports in the Sites settings.",
                None,
                vec![format!("HTTP port {}, HTTPS port {}", cfg.http_port, cfg.https_port)],
            );
        }
    }

    fn check_https(&self, b: &mut Builder) {
        let domains = self.domains.lock().unwrap().list();
        let ca = self.certs.ca_info();
        if (domains.iter().any(|d| d.https) || ca.exists) && !ca.trusted {
            b.add(
                "ca_untrusted",
                Severity::Warning,
                "Browsers will warn about your HTTPS sites",
                "Windows does not trust the local certificate authority that signs their certificates.",
                "Trust the certificate authority (a one-time Windows confirmation).",
                Some(CoreCommand::TrustCa),
                vec![],
            );
        }
        for cert in self.certs.list() {
            match cert.status {
                crate::certs::CertStatus::Expired => b.add(
                    format!("cert_expired:{}", cert.hostname),
                    Severity::Error,
                    format!("The certificate for {} has expired", cert.hostname),
                    "Browsers refuse HTTPS connections with an expired certificate.",
                    "Regenerate the certificate.",
                    Some(CoreCommand::RegenerateCertificate { hostname: cert.hostname.clone() }),
                    vec![format!("Expired {} day(s) ago", -cert.days_left)],
                ),
                crate::certs::CertStatus::Expiring => b.add(
                    format!("cert_expiring:{}", cert.hostname),
                    Severity::Info,
                    format!("The certificate for {} expires soon", cert.hostname),
                    format!("It is valid for {} more day(s).", cert.days_left),
                    "It renews on the next apply; or regenerate it now.",
                    Some(CoreCommand::RegenerateCertificate { hostname: cert.hostname.clone() }),
                    vec![],
                ),
                crate::certs::CertStatus::Valid => {}
            }
        }
    }

    fn check_sites(&self, b: &mut Builder) {
        let domains = self.domains.lock().unwrap().list();
        let projects = self.projects.lock().unwrap().list();
        let hosts = std::fs::read_to_string(crate::hosts::hosts_path()).unwrap_or_default();

        let unresolved: Vec<String> = domains
            .iter()
            .filter(|d| d.enabled && !self.web.dns_covers(&d.hostname) && !crate::hosts::lists(&hosts, &d.hostname))
            .map(|d| d.hostname.clone())
            .collect();
        if !unresolved.is_empty() {
            b.add(
                "hosts_missing",
                Severity::Warning,
                "Some site names do not point at this computer yet",
                "Names outside .test / .localhost / .internal need an entry in the Windows hosts file, and it has not been written.",
                "Apply the web config; it writes the entries (one administrator prompt, or none with the helper service).",
                Some(CoreCommand::ApplyWeb { overwrite: vec![] }),
                unresolved,
            );
        }

        for d in domains.iter().filter(|d| d.enabled) {
            if !d.root.is_empty() && !std::path::Path::new(&d.root).is_dir() {
                b.add(
                    format!("site_root_missing:{}", d.hostname),
                    Severity::Error,
                    format!("The folder for {} does not exist", d.hostname),
                    "The site's folder was moved, renamed or deleted.",
                    "Edit the site and choose its folder again, or disable it.",
                    None,
                    vec![d.root.clone()],
                );
            }
            if let Some(pid) = &d.project_id {
                if !projects.iter().any(|p| &p.id == pid) {
                    b.add(
                        format!("site_project_gone:{}", d.hostname),
                        Severity::Info,
                        format!("{} points at a project that was removed", d.hostname),
                        "The project it was linked to is no longer registered.",
                        "Register the project again, or unlink it by editing the site.",
                        None,
                        vec![],
                    );
                }
            }
            if let SiteKind::Php { version } = &d.kind {
                if self.php.pick_version(version.as_deref()).is_none() {
                    let wanted = version.clone().unwrap_or_else(|| "any version".into());
                    b.add(
                        format!("php_missing:{}", d.hostname),
                        Severity::Error,
                        format!("{} needs PHP {wanted}, which is not installed", d.hostname),
                        "The site is set to run on a PHP version that is not on this computer.",
                        "Install that PHP version from the Runtimes page, or pick another in the site's settings.",
                        None,
                        vec![format!("Installed: {}", self.php.all_versions().join(", "))],
                    );
                }
            }
        }
    }

    fn check_services(&self, b: &mut Builder) {
        for s in self.services.list().into_iter().filter(|s| s.installed) {
            if s.running && s.healthy == Some(false) {
                b.add(
                    format!("service_unhealthy:{}", s.id),
                    Severity::Warning,
                    format!("{} is running but not answering", s.name),
                    "The process is alive but nothing accepts connections on its port; it may still be starting, or it is stuck.",
                    "Restart the service.",
                    Some(CoreCommand::RestartService { id: s.id.clone() }),
                    vec![s.port.map(|p| format!("Port {p}")).unwrap_or_default()],
                );
            }
        }
    }

    fn check_projects(&self, b: &mut Builder) {
        for p in self.projects.lock().unwrap().list() {
            let root = std::path::Path::new(&p.path);
            if !root.is_dir() {
                b.add(
                    format!("project_missing:{}", p.id),
                    Severity::Warning,
                    format!("The folder for project {} is gone", p.name),
                    "It was moved, renamed or deleted.",
                    "Remove the project from the Projects page, or restore the folder.",
                    None,
                    vec![p.path.clone()],
                );
                continue;
            }
            let composer = crate::composer::read(root);
            if composer.has_composer_json && !composer.vendor_installed && !composer.packages.is_empty() {
                b.add(
                    format!("composer_not_installed:{}", p.id),
                    Severity::Info,
                    format!("{} has no vendor folder", p.name),
                    "composer.json lists packages, but they have not been installed, so the site will fail to load them.",
                    "Run composer install.",
                    Some(CoreCommand::RunComposer { project_id: p.id.clone(), action: "install".into(), target: None }),
                    vec![],
                );
            }
            let venv = crate::venv::detect(root);
            if venv.exists && venv.base_missing {
                b.add(
                    format!("venv_broken:{}", p.id),
                    Severity::Warning,
                    format!("The Python environment of {} is broken", p.name),
                    "The Python it was created from has been moved or uninstalled.",
                    "Recreate the virtual environment.",
                    Some(CoreCommand::CreateVenv { project_id: p.id.clone(), recreate: true }),
                    venv.base_home.into_iter().collect(),
                );
            }
        }
    }

    fn check_php(&self, b: &mut Builder) {
        for version in self.php.all_versions() {
            let report = self.php.xdebug_report(&version);
            if report.enabled && report.settings.start_with_request == "yes" && report.settings.modes.iter().any(|m| m == "debug") {
                b.add(
                    format!("xdebug_always_on:{version}"),
                    Severity::Info,
                    format!("Xdebug slows every PHP {version} request"),
                    "It is set to start a debug session on every request, not only when triggered.",
                    "Set start_with_request to \"trigger\" in the Xdebug settings, or turn Xdebug off when you are not debugging.",
                    None,
                    vec![],
                );
            }
        }
    }
}
