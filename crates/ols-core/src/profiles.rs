//! Profiles (§69) and project modes (§70).
//!
//! A **profile** is a reusable environment definition: a manifest without the project's
//! own name and site. Built-in ones cover the SRS list; users save their own (from a
//! project, or by editing YAML), and export / import them as files (§132). Applying a
//! profile writes the project's `.openlocalserver/environment.yaml`, filling in its name,
//! site and database; the Environment tab then shows the plan before anything changes.
//!
//! A **mode** (Development, Testing, Debugging, Demo) switches a set of things at once:
//! Xdebug, `.env` values, services, workers and the scheduler. A project's manifest can
//! define its own modes; otherwise sensible defaults apply.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::error::CoreError;
use crate::manifest::{self, DatabaseManifest, DomainManifest, EnvironmentManifest, ModeManifest, SchedulerEntry, ServiceToggle, WorkerEntry};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub environment: EnvironmentManifest,
    /// Shipped with the app: can be copied, not changed or deleted.
    #[serde(default, skip_deserializing)]
    pub builtin: bool,
}

fn svc(list: &[&str]) -> BTreeMap<String, ServiceToggle> {
    list.iter().map(|s| (s.to_string(), ServiceToggle::On(true))).collect()
}

fn db(engine: &str) -> Option<DatabaseManifest> {
    Some(DatabaseManifest { engine: engine.into(), version: None, name: None })
}

fn site(https: bool, port: Option<u16>) -> Option<DomainManifest> {
    // The hostname is filled in per project when the profile is applied.
    Some(DomainManifest { hostname: String::new(), https, wildcard: false, root: None, port })
}

pub fn builtin() -> Vec<Profile> {
    let laravel = |services: &[&str], engine: &str| {
        let mut e = EnvironmentManifest { domain: site(true, None), database: db(engine), services: svc(services), scheduler: Some(SchedulerEntry::On(true)), ..Default::default() };
        e.runtime.php = Some("8.4".into());
        e.runtime.node = Some("24".into());
        e.workers.insert("queue".into(), WorkerEntry::On(true));
        e
    };
    let mut node_api = EnvironmentManifest { domain: site(true, Some(3000)), services: svc(&["redis"]), ..Default::default() };
    node_api.runtime.node = Some("24".into());
    let mut python_api = EnvironmentManifest { domain: site(true, Some(8000)), database: db("postgres"), ..Default::default() };
    python_api.runtime.python = Some("3".into());
    let mut full = laravel(&["redis", "mailpit"], "mariadb");
    full.package_manager = Some("pnpm".into());
    let mut wordpress = EnvironmentManifest { domain: site(true, None), database: db("mariadb"), services: svc(&["mailpit"]), ..Default::default() };
    wordpress.runtime.php = Some("8.4".into());
    wordpress.extensions = vec!["mysqli".into(), "gd".into(), "exif".into()];

    [
        ("laravel-standard", "Laravel Standard", "PHP 8.4, MariaDB, a queue worker and the scheduler.", laravel(&[], "mariadb")),
        ("laravel-redis", "Laravel + Redis", "Laravel Standard with Redis for cache, sessions and queues.", laravel(&["redis"], "mariadb")),
        ("laravel-mysql-mailpit", "Laravel + MySQL + Mailpit", "Laravel with a MySQL-compatible database and Mailpit catching mail.", laravel(&["mailpit"], "mysql")),
        ("node-api", "Node API", "Node.js 24 app on port 3000 behind HTTPS, with Redis.", node_api),
        ("python-api", "Python API", "Python app on port 8000 behind HTTPS, with PostgreSQL.", python_api),
        ("full-stack", "Full Stack", "PHP and Node with pnpm, MariaDB, Redis and Mailpit, workers and scheduler.", full),
        ("wordpress", "WordPress", "PHP 8.4 with the extensions WordPress uses, MariaDB and Mailpit.", wordpress),
        ("custom", "Custom", "An empty starting point.", EnvironmentManifest::default()),
    ]
    .into_iter()
    .map(|(id, name, description, environment)| Profile { id: id.into(), name: name.into(), description: description.into(), environment, builtin: true })
    .collect()
}

pub struct ProfileStore {
    dir: PathBuf,
}

impl ProfileStore {
    pub fn new(paths: &crate::paths::AppPaths) -> Self {
        Self { dir: paths.data_dir().join("profiles") }
    }

    pub fn list(&self) -> Vec<Profile> {
        let mut all = builtin();
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            let mut own: Vec<Profile> = rd
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "yaml"))
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .filter_map(|t| serde_yaml_ng::from_str::<Profile>(&t).ok())
                .filter(|p| !all.iter().any(|b| b.id == p.id))
                .collect();
            own.sort_by_key(|p| p.name.to_lowercase());
            all.extend(own);
        }
        all
    }

    pub fn get(&self, id: &str) -> Option<Profile> {
        self.list().into_iter().find(|p| p.id == id)
    }

    pub fn save(&self, mut p: Profile) -> Result<Profile, String> {
        p.id = crate::domain::slugify(if p.id.is_empty() { &p.name } else { &p.id });
        if p.id.is_empty() || p.name.trim().is_empty() {
            return Err("a profile needs a name".into());
        }
        if builtin().iter().any(|b| b.id == p.id) {
            return Err(format!("\"{}\" is a built-in profile; save a copy under another name", p.id));
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        p.builtin = false;
        let yaml = serde_yaml_ng::to_string(&p).map_err(|e| e.to_string())?;
        std::fs::write(self.dir.join(format!("{}.yaml", p.id)), yaml).map_err(|e| e.to_string())?;
        Ok(p)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        if builtin().iter().any(|b| b.id == id) {
            return Err("built-in profiles can't be deleted".into());
        }
        let file = self.dir.join(format!("{}.yaml", crate::domain::slugify(id)));
        std::fs::remove_file(&file).map_err(|e| format!("{}: {e}", file.display()))
    }

    pub fn export(&self, id: &str, dest: &Path) -> Result<(), String> {
        let p = self.get(id).ok_or_else(|| format!("no profile \"{id}\""))?;
        std::fs::write(dest, serde_yaml_ng::to_string(&p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    }

    /// Reads a profile file for review (§132: imported configurations are reviewed first).
    pub fn read_file(source: &Path) -> Result<Profile, String> {
        let text = std::fs::read_to_string(source).map_err(|e| format!("{}: {e}", source.display()))?;
        serde_yaml_ng::from_str(&text).map_err(|e| format!("not a profile file: {e}"))
    }
}

/// A profile made concrete for one project: its name, site and database name filled in.
pub fn apply_to(profile: &EnvironmentManifest, project_name: &str, current: Option<&EnvironmentManifest>) -> EnvironmentManifest {
    let mut m = profile.clone();
    m.name = Some(project_name.to_string());
    if let Some(d) = m.domain.as_mut() {
        // Keep the project's existing site name if it has one.
        d.hostname = current.and_then(|c| c.domain.as_ref()).map(|c| c.hostname.clone()).filter(|h| !h.is_empty()).unwrap_or_else(|| format!("{}.test", crate::domain::slugify(project_name)));
        if d.root.is_none() {
            d.root = current.and_then(|c| c.domain.as_ref()).and_then(|c| c.root.clone());
        }
    }
    if let Some(db) = m.database.as_mut() {
        db.name = current.and_then(|c| c.database.as_ref()).and_then(|c| c.name.clone()).or_else(|| Some(crate::setup::db_name_for(project_name)));
    }
    // Modes the project defined itself survive a profile change.
    if let Some(c) = current {
        for (k, v) in &c.modes {
            m.modes.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    m
}

// ------------------------------------------------------------------------ modes (§70)

pub const MODES: &[&str] = &["development", "testing", "debugging", "demo"];

pub fn default_mode(name: &str) -> Option<ModeManifest> {
    let env = |pairs: &[(&str, &str)]| pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Some(match name {
        "development" => ModeManifest { xdebug: Some(false), workers: Some(true), scheduler: Some(true), env: env(&[("APP_DEBUG", "true")]), ..Default::default() },
        "testing" => ModeManifest { xdebug: Some(false), workers: Some(false), scheduler: Some(false), ..Default::default() },
        "debugging" => ModeManifest { xdebug: Some(true), services: vec!["mailpit".into()], workers: Some(true), env: env(&[("APP_DEBUG", "true"), ("LOG_LEVEL", "debug")]), ..Default::default() },
        "demo" => ModeManifest { xdebug: Some(false), workers: Some(true), scheduler: Some(true), env: env(&[("APP_DEBUG", "false"), ("LOG_LEVEL", "warning")]), ..Default::default() },
        _ => return None,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeInfo {
    pub name: String,
    pub mode: ModeManifest,
    /// Defined in the project's manifest rather than a default.
    pub custom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModesView {
    pub current: Option<String>,
    pub modes: Vec<ModeInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeResult {
    pub mode: String,
    /// What was changed, one line each.
    pub changes: Vec<String>,
    /// What couldn't be done (the rest still applied).
    pub problems: Vec<String>,
}

impl Inner {
    pub fn project_modes(&self, project_id: &str) -> Result<ModesView, CoreError> {
        let project = self.projects.lock().unwrap().get(project_id).ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        let own = manifest::read_manifest(Path::new(&project.path)).ok().flatten().map(|m| m.modes).unwrap_or_default();
        let mut modes: Vec<ModeInfo> = MODES.iter().map(|n| ModeInfo { name: n.to_string(), custom: own.contains_key(*n), mode: own.get(*n).cloned().or_else(|| default_mode(n)).unwrap_or_default() }).collect();
        for (k, v) in own.iter().filter(|(k, _)| !MODES.contains(&k.as_str())) {
            modes.push(ModeInfo { name: k.clone(), mode: v.clone(), custom: true });
        }
        let current = self.settings.lock().unwrap().get(&format!("project.{project_id}.mode")).and_then(|v| v.as_str().map(str::to_string));
        Ok(ModesView { current, modes })
    }

    /// Switches a project into a mode. Each part is tried; failures are reported, not fatal.
    pub fn set_project_mode(&self, project_id: &str, name: &str) -> Result<ModeResult, CoreError> {
        let view = self.project_modes(project_id)?;
        let mode = view.modes.into_iter().find(|m| m.name == name).ok_or_else(|| CoreError::EnvError(format!("no mode \"{name}\"")))?.mode;
        let mut r = ModeResult { mode: name.to_string(), changes: Vec::new(), problems: Vec::new() };

        if let Some(on) = mode.xdebug {
            let php = self.project_detail(project_id).and_then(|d| d.resolved.into_iter().find(|x| x.id == "php")).and_then(|x| x.installed_version);
            match php {
                Some(v) => {
                    let report = self.xdebug_report(&v);
                    if report.enabled == on {
                    } else if !report.installed && on {
                        r.problems.push(format!("Xdebug isn't installed for PHP {v}; install it from the project's Xdebug tab"));
                    } else {
                        match self.php.set_extension(&v, "xdebug", on) {
                            Ok(()) => r.changes.push(format!("Xdebug {} for PHP {v}", if on { "on" } else { "off" })),
                            Err(e) => r.problems.push(format!("Xdebug: {e}")),
                        }
                    }
                }
                None if on => r.problems.push("the project has no installed PHP, so Xdebug was left alone".into()),
                None => {}
            }
        }

        if !mode.env.is_empty() {
            let project = self.projects.lock().unwrap().get(project_id).ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
            if Path::new(&project.path).join(".env").is_file() {
                for (k, v) in &mode.env {
                    match self.env_set(project_id, ".env", k, v) {
                        Ok(_) => r.changes.push(format!(".env {k}={v}")),
                        Err(e) => r.problems.push(format!(".env {k}: {e}")),
                    }
                }
            }
        }

        for s in &mode.services {
            if !self.services.is_running(s) {
                match self.start_service_and_wait(s, &mut |_| {}) {
                    Ok(()) => r.changes.push(format!("started {s}")),
                    Err(e) => r.problems.push(format!("{s}: {e}")),
                }
            }
        }

        match mode.workers {
            Some(true) => match self.start_project_workers(project_id) {
                Ok(n) if n > 0 => r.changes.push(format!("{n} worker process(es) running")),
                Ok(_) => {}
                Err(e) => r.problems.push(format!("workers: {e}")),
            },
            Some(false) if self.worker_statuses(Some(project_id)).iter().any(|w| w.running > 0) => {
                self.stop_project_workers(project_id);
                r.changes.push("workers stopped".into());
            }
            Some(false) | None => {}
        }

        if let Some(on) = mode.scheduler {
            for mut t in self.schedules_for(project_id) {
                if t.enabled != on {
                    t.enabled = on;
                    let label = format!("scheduled task \"{}\" {}", t.name, if on { "on" } else { "paused" });
                    match self.save_schedule(t) {
                        Ok(_) => r.changes.push(label),
                        Err(e) => r.problems.push(e.to_string()),
                    }
                }
            }
        }

        self.settings.lock().unwrap().set(format!("project.{project_id}.mode"), serde_json::json!(name))?;
        tracing::info!(project = %project_id, mode = %name, changes = r.changes.len(), "project mode");
        Ok(r)
    }

    /// A profile from a project's current manifest ("Clone Profile", §158).
    pub fn profile_from_project(&self, project_id: &str, name: &str) -> Result<Profile, CoreError> {
        let project = self.projects.lock().unwrap().get(project_id).ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        let mut env = match manifest::read_manifest(Path::new(&project.path)).map_err(CoreError::EnvError)? {
            Some(m) => m,
            None => self.derive_manifest(project_id)?,
        };
        env.name = None;
        env.profile = None;
        if let Some(d) = env.domain.as_mut() {
            d.hostname.clear();
        }
        if let Some(d) = env.database.as_mut() {
            d.name = None;
        }
        let p = Profile { id: String::new(), name: name.to_string(), description: format!("Made from {}", project.name), environment: env, builtin: false };
        self.profiles.save(p).map_err(CoreError::EnvError)
    }

    /// Writes a profile into a project's manifest and returns the new manifest.
    pub fn apply_profile(&self, project_id: &str, profile_id: &str) -> Result<EnvironmentManifest, CoreError> {
        let project = self.projects.lock().unwrap().get(project_id).ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        let profile = self.profiles.get(profile_id).ok_or_else(|| CoreError::EnvError(format!("no profile \"{profile_id}\"")))?;
        let current = manifest::read_manifest(Path::new(&project.path)).ok().flatten();
        let mut m = apply_to(&profile.environment, &project.name, current.as_ref());
        m.profile = Some(profile.id.clone());
        manifest::write_manifest(Path::new(&project.path), &m).map_err(CoreError::EnvError)?;
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_srs_profiles_are_built_in() {
        let names: Vec<String> = builtin().into_iter().map(|p| p.name).collect();
        for want in ["Laravel Standard", "Laravel + Redis", "Laravel + MySQL + Mailpit", "Node API", "Python API", "Full Stack", "WordPress", "Custom"] {
            assert!(names.iter().any(|n| n == want), "{want}");
        }
    }

    #[test]
    fn applying_a_profile_fills_in_the_project() {
        let p = builtin().into_iter().find(|p| p.id == "laravel-redis").unwrap();
        let m = apply_to(&p.environment, "My Shop", None);
        assert_eq!(m.domain.as_ref().unwrap().hostname, "my-shop.test");
        assert_eq!(m.database.as_ref().unwrap().name.as_deref(), Some("my_shop"));
        assert!(m.enabled_services().contains(&"redis".to_string()));

        let mut current = EnvironmentManifest::default();
        current.domain = Some(DomainManifest { hostname: "keep.test".into(), ..Default::default() });
        current.modes.insert("review".into(), ModeManifest::default());
        let m = apply_to(&p.environment, "My Shop", Some(&current));
        assert_eq!(m.domain.unwrap().hostname, "keep.test", "an existing site name is kept");
        assert!(m.modes.contains_key("review"), "the project's own modes survive");
    }

    #[test]
    fn user_profiles_round_trip_and_builtins_are_protected() {
        let home = crate::test_support::isolated_home();
        let store = ProfileStore::new(&home.paths);
        let saved = store.save(Profile { id: String::new(), name: "Team API".into(), description: String::new(), environment: EnvironmentManifest::default(), builtin: false }).unwrap();
        assert_eq!(saved.id, "team-api");
        assert!(store.get("team-api").is_some());
        let file = home.paths.root().join("p.yaml");
        store.export("team-api", &file).unwrap();
        assert_eq!(ProfileStore::read_file(&file).unwrap().name, "Team API");
        assert!(store.delete("wordpress").is_err());
        assert!(store.save(Profile { id: "wordpress".into(), name: "x".into(), description: String::new(), environment: Default::default(), builtin: false }).is_err());
        store.delete("team-api").unwrap();
        assert!(store.get("team-api").is_none());
    }

    #[test]
    fn every_mode_has_defaults() {
        for m in MODES {
            assert!(default_mode(m).is_some());
        }
        assert_eq!(default_mode("debugging").unwrap().xdebug, Some(true));
    }
}
