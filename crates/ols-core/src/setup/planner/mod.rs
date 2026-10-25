pub mod rules;
pub mod text;

pub use self::text::*;

use crate::app::Inner;
use crate::manifest::LockFile;
use crate::setup::types::*;

pub(crate) struct Planner<'a> {
    pub(crate) inner: &'a Inner,
    pub(crate) steps: Vec<PlanStep>,
    pub(crate) conflicts: Vec<Conflict>,
    pub(crate) lock: &'a LockFile,
}

impl Planner<'_> {
    pub(crate) fn step(&mut self, group: &str, label: String, action: SetupAction) {
        self.steps.push(PlanStep {
            group: group.into(),
            label,
            action,
            done: false,
            note: None,
        });
    }

    pub(crate) fn done(&mut self, group: &str, label: String, action: SetupAction) {
        self.steps.push(PlanStep {
            group: group.into(),
            label,
            action,
            done: true,
            note: None,
        });
    }

    pub(crate) fn note(&mut self, text: String) {
        if let Some(last) = self.steps.last_mut() {
            last.note = Some(text);
        } else {
            self.conflicts.push(Conflict {
                kind: "info".into(),
                blocking: false,
                message: text,
                resolution: String::new(),
            });
        }
    }

    pub(crate) fn conflict(
        &mut self,
        kind: &str,
        blocking: bool,
        message: String,
        resolution: &str,
    ) {
        self.conflicts.push(Conflict {
            kind: kind.into(),
            blocking,
            message,
            resolution: resolution.into(),
        });
    }

    /// Plans a managed runtime. Returns the version that will be used.
    pub(crate) fn runtime(&mut self, id: &str, wanted: Option<&str>) -> Option<String> {
        let wanted = wanted?;
        let name = self
            .inner
            .runtimes
            .display_name(id)
            .unwrap_or_else(|| id.to_string());
        // A lock pins the exact version a working setup used (§72).
        let locked = self.lock.get(id).cloned();
        let installed = self.inner.runtimes.installed_versions(id);
        let mut custom: Vec<String> = self
            .inner
            .custom_installs
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .filter(|c| c.id == id)
            .map(|c| c.label)
            .collect();
        custom.sort();
        let want = locked.as_deref().unwrap_or(wanted);
        if let Some(v) = crate::php::pick_version(&installed, Some(want))
            .or_else(|| crate::php::pick_version(&custom, Some(want)))
        {
            self.done(
                "install",
                format!("{name} {v}"),
                SetupAction::InstallRuntime {
                    id: id.into(),
                    version: v.clone(),
                },
            );
            return Some(v);
        }
        let available: Vec<String> = crate::catalog::builtin_catalog()
            .iter()
            .filter(|m| m.id == id)
            .map(|m| m.version.to_string())
            .collect();
        if let Some(locked) = &locked {
            if let Some(v) = crate::php::pick_version(&installed, Some(wanted))
                .or_else(|| crate::php::pick_version(&available, Some(wanted)))
            {
                self.conflict("runtime", false, format!("The lock file pins {name} {locked}, which isn't available; {v} matches the manifest's \"{wanted}\"."), "Use it; the lock file is updated after setup.");
                return self.install_or_use(id, &name, &installed, v);
            }
        }
        match crate::php::pick_version(&available, Some(wanted)) {
            Some(v) => self.install_or_use(id, &name, &installed, v),
            None => {
                self.conflict("runtime", true, format!("{name} {wanted} is needed, and it isn't available to install (available: {})", available.join(", ")), "Point OpenLocalServer at your own install on the Runtimes page, or change the manifest's version.");
                None
            }
        }
    }

    pub(crate) fn install_or_use(
        &mut self,
        id: &str,
        name: &str,
        installed: &[String],
        v: String,
    ) -> Option<String> {
        if installed.contains(&v) {
            self.done(
                "install",
                format!("{name} {v}"),
                SetupAction::InstallRuntime {
                    id: id.into(),
                    version: v.clone(),
                },
            );
        } else {
            self.step(
                "install",
                format!("Install {name} {v}"),
                SetupAction::InstallRuntime {
                    id: id.into(),
                    version: v.clone(),
                },
            );
        }
        Some(v)
    }

    /// Plans installing (if needed) and starting a service, with port conflict checks.
    pub(crate) fn service(&mut self, id: &str) {
        if self
            .steps
            .iter()
            .any(|s| matches!(&s.action, SetupAction::StartService { id: x } if x == id))
        {
            return;
        }
        let status = self.inner.services.status(id);
        if !crate::custom_service::is_custom_id(id) && !status.installed {
            if let Some(m) = crate::catalog::builtin_catalog()
                .into_iter()
                .find(|m| m.id == id)
            {
                self.step(
                    "install",
                    format!("Install {} {}", m.name, m.version),
                    SetupAction::InstallRuntime {
                        id: id.into(),
                        version: m.version.into(),
                    },
                );
            }
        }
        if status.running {
            self.done(
                "start",
                format!("{} is running", status.name),
                SetupAction::StartService { id: id.into() },
            );
            return;
        }
        if let Some(port) = status.port {
            if let crate::port::PortStatus::InUse { process_name, pid } =
                crate::port::check_port(port)
            {
                let who = process_name
                    .map(|n| {
                        format!(
                            "{n}{}",
                            pid.map(|p| format!(" (PID {p})")).unwrap_or_default()
                        )
                    })
                    .unwrap_or_else(|| "another program".into());
                self.conflict("port", true, format!("{} needs port {port}, which {who} is using.", status.name), "Stop that program yourself (OLS never stops programs it didn't start), then plan again.");
            }
        }
        self.step(
            "start",
            format!("Start {}", status.name),
            SetupAction::StartService { id: id.into() },
        );
    }
}
