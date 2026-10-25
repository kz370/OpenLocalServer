use std::path::Path;

use super::on_off;
use super::Planner;
use crate::manifest::SchedulerEntry;
use crate::setup::types::*;

impl Planner<'_> {
    pub(crate) fn domain(
        &mut self,
        project_id: &str,
        path: &Path,
        det: &crate::detection::DetectionResult,
        dm: &DomainManifest,
        php: Option<&str>,
    ) -> Option<String> {
        let host = dm.hostname.trim().to_ascii_lowercase();
        if host.is_empty() || !host.contains('.') {
            self.conflict(
                "domain",
                true,
                format!("\"{}\" is not a usable site name.", dm.hostname),
                "Use a name like shop.test.",
            );
            return None;
        }
        let root_rel = dm.root.clone().or_else(|| det.doc_root.clone());
        let root = match &root_rel {
            Some(r) => match crate::quickapp::plan::safe_join(&path.display().to_string(), r) {
                Ok(p) => p,
                Err(e) => {
                    self.conflict(
                        "domain",
                        true,
                        format!("The site root {r} is not allowed: {e}"),
                        "Use a folder inside the project.",
                    );
                    return None;
                }
            },
            None => path.display().to_string(),
        };
        let kind = match dm.port {
            Some(port) => SiteKind::Proxy {
                upstream_port: port,
                upstream_host: None,
                upstream_https: false,
            },
            None if is_php(&det.framework) => SiteKind::Php {
                version: php.map(|v| v.split('.').take(2).collect::<Vec<_>>().join(".")),
            },
            None => SiteKind::Static,
        };
        let existing = self.inner.domains.lock().unwrap().get(&host);
        let wanted = Domain {
            hostname: host.clone(),
            project_id: Some(project_id.to_string()),
            root: root.clone(),
            kind: kind.clone(),
            https: dm.https,
            redirect_https: dm.https,
            wildcard: dm.wildcard,
            enabled: true,
            ownership: Ownership::Managed,
            app: None::<AppSpec>,
            blocks: Default::default(),
            generated_hashes: Default::default(),
            public_domain: None,
            tunnel_id: None,
            server: None,
            path_prefix: None,
            icon: None,
            ..Default::default()
        };
        match existing {
            Some(d) if d.project_id.as_deref().is_some_and(|p| p != project_id) => {
                let other = d
                    .project_id
                    .and_then(|p| self.inner.projects.lock().unwrap().get(&p))
                    .map(|p| p.name)
                    .unwrap_or_else(|| "another project".into());
                self.conflict(
                    "domain",
                    true,
                    format!("{host} already belongs to {other}."),
                    "Choose another domain.hostname, or remove that site first.",
                );
                return None;
            }
            Some(d) => {
                if d.ownership != Ownership::Managed {
                    self.conflict("file_ownership", false, format!("{host}'s web config is edited by hand ({:?}); setup leaves that file as it is.", d.ownership).to_lowercase(), "Switch the site back to Managed on the Web config page if you want setup to rewrite it.");
                }
                if d.https != dm.https || d.wildcard != dm.wildcard || !d.enabled {
                    let mut updated = d.clone();
                    updated.https = dm.https;
                    updated.redirect_https = dm.https && (d.redirect_https || !d.https);
                    updated.wildcard = dm.wildcard;
                    updated.enabled = true;
                    updated.project_id = Some(project_id.to_string());
                    self.step(
                        "configure",
                        format!(
                            "Update {host} (HTTPS {}, wildcard {})",
                            on_off(dm.https),
                            on_off(dm.wildcard)
                        ),
                        SetupAction::UpdateDomain {
                            domain: Box::new(updated),
                        },
                    );
                } else {
                    self.done(
                        "configure",
                        format!("Site {host}"),
                        SetupAction::AddDomain {
                            domain: Box::new(wanted),
                        },
                    );
                }
            }
            None => {
                let label = format!(
                    "Add site {host}{}{} → {}",
                    if dm.https { " with HTTPS" } else { "" },
                    if dm.wildcard { " and *." } else { "" },
                    match &kind {
                        SiteKind::Proxy { upstream_port, .. } => format!("port {upstream_port}"),
                        _ => root_rel.clone().unwrap_or_else(|| "project folder".into()),
                    }
                );
                let label = if dm.wildcard {
                    label.replace(" and *.", &format!(" and *.{host}"))
                } else {
                    label
                };
                self.step(
                    "configure",
                    label,
                    SetupAction::AddDomain {
                        domain: Box::new(wanted),
                    },
                );
            }
        }

        // DNS: .test / .localhost / .internal resolve through the built-in DNS; other names need the hosts file.
        let tld = host.rsplit('.').next().unwrap_or_default();
        if !["test", "localhost", "internal", "example", "invalid"].contains(&tld) {
            let hosts = std::fs::read_to_string(crate::hosts::hosts_path()).unwrap_or_default();
            if crate::hosts::lists(&hosts, &host) {
                self.done(
                    "configure",
                    format!("{host} is in the hosts file"),
                    SetupAction::SyncHosts,
                );
            } else {
                self.step(
                    "configure",
                    format!(
                        "Add {host} to the Windows hosts file (asks for administrator approval)"
                    ),
                    SetupAction::SyncHosts,
                );
            }
            if dm.wildcard {
                self.conflict(
                    "domain",
                    false,
                    format!("*.{host} can't be resolved through the hosts file."),
                    "Use a .test name for wildcard sites; they resolve through the built-in DNS.",
                );
            }
        }

        // SSL.
        if dm.https {
            let ca = self.inner.certs.ca_info();
            if !ca.trusted {
                self.step(
                    "configure",
                    "Trust the local certificate authority (one Windows confirmation)".to_string(),
                    SetupAction::TrustCa,
                );
            }
            if let Some(cert) = self.inner.certs.info(&host) {
                if cert.status == crate::certs::CertStatus::Expired {
                    self.conflict(
                        "certificate",
                        false,
                        format!("The certificate for {host} has expired."),
                        "It is renewed when the web config is applied.",
                    );
                }
            }
        }
        Some(host)
    }

    pub(crate) fn workers(
        &mut self,
        project_id: &str,
        framework: &Framework,
        manifest: &EnvironmentManifest,
    ) {
        let existing = self.inner.workers_for(project_id);
        for (name, entry) in &manifest.workers {
            let def = match entry {
                WorkerEntry::On(false) => continue,
                WorkerEntry::On(true) => match crate::workers::default_command(framework) {
                    Some(cmd) => crate::manifest::WorkerManifest {
                        command: cmd.into(),
                        count: 1,
                        timeout_secs: None,
                        memory_mb: None,
                    },
                    None => {
                        self.conflict("manifest", true, format!("The \"{name}\" worker has no command, and this kind of project has no usual one."), "Give it one: workers: { name: { command: \"node worker.js\" } }.");
                        continue;
                    }
                },
                WorkerEntry::Custom(w) => w.clone(),
            };
            let id = crate::workers::worker_id(project_id, name);
            if existing
                .iter()
                .any(|w| w.id == id && w.command == def.command && w.count == def.count.max(1))
            {
                self.done(
                    "configure",
                    format!("Worker \"{name}\""),
                    SetupAction::StartWorkers,
                );
                continue;
            }
            let worker = crate::workers::Worker {
                id,
                project_id: project_id.to_string(),
                name: name.to_string(),
                command: def.command.clone(),
                count: def.count.max(1),
                timeout_secs: def.timeout_secs,
                memory_mb: def.memory_mb,
                max_retries: 5,
                restart: true,
                autostart: true,
            };
            self.step(
                "configure",
                format!(
                    "Add worker \"{name}\": {} × {}",
                    worker.count, worker.command
                ),
                SetupAction::AddWorker {
                    worker: Box::new(worker),
                },
            );
        }

        let tasks: Vec<crate::manifest::ScheduledTaskManifest> = match &manifest.scheduler {
            Some(SchedulerEntry::On(true)) => {
                match crate::scheduler::default_task(framework) {
                    Some(t) => vec![t],
                    None => {
                        self.conflict("manifest", false, "scheduler: true, but this kind of project has no usual scheduler command.".to_string(), "List the tasks instead: scheduler: [{ name, schedule, command }].");
                        vec![]
                    }
                }
            }
            Some(SchedulerEntry::Tasks(list)) => list.clone(),
            _ => vec![],
        };
        let existing = self.inner.schedules_for(project_id);
        for t in tasks {
            if let Err(e) = crate::scheduler::Schedule::parse(&t.schedule) {
                self.conflict("manifest", true, format!("Scheduled task \"{}\": {e}", t.name), "Use a cron expression (\"*/5 * * * *\") or every_minute, hourly, daily, weekly.");
                continue;
            }
            let id = crate::workers::worker_id(project_id, &t.name);
            if existing
                .iter()
                .any(|x| x.id == id && x.command == t.command && x.schedule == t.schedule)
            {
                self.done(
                    "configure",
                    format!("Scheduled task \"{}\"", t.name),
                    SetupAction::WriteLock,
                );
                continue;
            }
            let task = crate::scheduler::ScheduledTask {
                id,
                project_id: Some(project_id.to_string()),
                name: t.name.clone(),
                schedule: t.schedule.clone(),
                command: t.command.clone(),
                enabled: true,
            };
            self.step(
                "configure",
                format!(
                    "Schedule \"{}\" ({}): {}",
                    t.name,
                    crate::scheduler::describe(&t.schedule),
                    t.command
                ),
                SetupAction::AddSchedule {
                    task: Box::new(task),
                },
            );
        }
    }
}
