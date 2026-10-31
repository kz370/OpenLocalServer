use std::path::{Path, PathBuf};

use super::types::*;
use crate::app::Inner;
use crate::error::CoreError;

impl Inner {
    pub fn manifest_info(&self, project_id: &str) -> Result<ManifestInfo, CoreError> {
        let project = self.ensure_project_available(project_id)?;
        let root = PathBuf::from(&project.path);
        let path = manifest::manifest_path(&root);
        let text = std::fs::read_to_string(&path).ok();
        let (manifest, error) = match manifest::read_manifest(&root) {
            Ok(m) => (m, None),
            Err(e) => (None, Some(e)),
        };
        Ok(ManifestInfo {
            path: path.display().to_string(),
            found: text.is_some(),
            text,
            manifest,
            error,
            derived: self.derive_manifest(project_id)?,
            lock: manifest::read_lock(&root).ok().flatten(),
            has_commands: manifest::dir(&root).join("commands.yaml").is_file(),
            has_services: manifest::dir(&root).join("services.yaml").is_file(),
        })
    }

    pub fn save_manifest_text(&self, project_id: &str, text: &str) -> Result<String, CoreError> {
        serde_yaml_ng::from_str::<EnvironmentManifest>(text)
            .map_err(|e| CoreError::EnvError(format!("the manifest doesn't read as YAML: {e}")))?;
        let project = self.ensure_project_available(project_id)?;
        let path = manifest::manifest_path(Path::new(&project.path));
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, text)?;
        Ok(path.display().to_string())
    }

    /// Without a manifest: what detection says the project needs, as a manifest the user
    /// can save and edit.
    pub fn derive_manifest(&self, project_id: &str) -> Result<EnvironmentManifest, CoreError> {
        let project = self.ensure_project_available(project_id)?;
        let detail = self.project_detail(project_id).unwrap();
        let path = PathBuf::from(&project.path);
        let det = &detail.detection;
        let mut m = EnvironmentManifest {
            name: Some(detail.project.name.clone()),
            ..Default::default()
        };
        m.runtime.php = det.requirements.php.clone();
        m.runtime.node = det.requirements.node.clone();
        m.runtime.python = det.requirements.python.clone();
        if is_php(&det.framework) && m.runtime.php.is_none() {
            m.runtime.php = self
                .php
                .pick_version(None)
                .map(|v| v.split('.').take(2).collect::<Vec<_>>().join("."));
        }

        // A site: PHP and static projects are served directly; others need their own port.
        let existing = self
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .find(|d| d.project_id.as_deref() == Some(project_id));
        let servable = is_php(&det.framework) || path.join("index.html").is_file();
        if let Some(d) = existing {
            m.domain = Some(DomainManifest {
                hostname: d.hostname.clone(),
                https: d.https,
                wildcard: d.wildcard,
                root: Path::new(&d.root)
                    .strip_prefix(&path)
                    .ok()
                    .map(|r| r.display().to_string().replace('\\', "/"))
                    .filter(|r| !r.is_empty()),
                port: match d.kind {
                    SiteKind::Proxy {
                        upstream_port,
                        upstream_host: None,
                        ..
                    } => Some(upstream_port),
                    _ => None,
                },
            });
        } else if servable {
            let default_tld = self.setting_string("domains.default_tld", "local");
            m.domain = Some(DomainManifest {
                hostname: format!(
                    "{}.{default_tld}",
                    crate::domain::slugify(&detail.project.name)
                ),
                https: true,
                wildcard: false,
                root: det.doc_root.clone(),
                port: None,
            });
        }

        // The database the project's .env already points at.
        if let Some(conn) = env_value(&path, "DB_CONNECTION") {
            let engine = match conn.as_str() {
                "mysql" | "mariadb" => Some("mariadb"),
                "pgsql" | "postgres" | "postgresql" => Some("postgres"),
                "sqlite" => Some("sqlite"),
                "mongodb" => Some("mongodb"),
                _ => None,
            };
            if let Some(engine) = engine {
                let name = env_value(&path, "DB_DATABASE").filter(|n| !n.is_empty());
                m.database = Some(DatabaseManifest {
                    engine: engine.into(),
                    version: None,
                    name,
                });
            }
            if env_value(&path, "REDIS_HOST").is_some()
                && ["redis"].iter().any(|k| {
                    env_value(&path, "CACHE_STORE")
                        .or_else(|| env_value(&path, "CACHE_DRIVER"))
                        .as_deref()
                        == Some(k)
                        || env_value(&path, "QUEUE_CONNECTION").as_deref() == Some(k)
                })
            {
                m.services.insert("redis".into(), ServiceToggle::On(true));
            }
            if env_value(&path, "MAIL_MAILER").is_some() {
                m.services.insert("mailpit".into(), ServiceToggle::On(true));
            }
            // Memcached projects name it directly: Laravel's cache stores are `memcached`,
            // and Drupal/others just export MEMCACHED_* variables.
            let uses_memcached = env_value(&path, "CACHE_STORE")
                .or_else(|| env_value(&path, "CACHE_DRIVER"))
                .or_else(|| env_value(&path, "MEMCACHED_HOST"))
                .is_some_and(|v| v.eq_ignore_ascii_case("memcached"));
            if uses_memcached {
                m.services
                    .insert("memcached".into(), ServiceToggle::On(true));
            }
        }
        if det.framework == Framework::Laravel {
            if env_value(&path, "QUEUE_CONNECTION").is_some_and(|q| q != "sync") {
                m.workers.insert("queue".into(), WorkerEntry::On(true));
            }
            m.scheduler = Some(SchedulerEntry::On(true));
        }
        if let Some(pm) = crate::nodepm::info(&path, None).detected {
            if pm != "npm" {
                m.package_manager = Some(pm);
            }
        }
        Ok(m)
    }

    /// Writes a manifest (the derived one when `manifest` is `None`) into the project.
    pub fn save_manifest(
        &self,
        project_id: &str,
        manifest: Option<EnvironmentManifest>,
    ) -> Result<String, CoreError> {
        let project = self.ensure_project_available(project_id)?;
        let m = match manifest {
            Some(m) => m,
            None => self.derive_manifest(project_id)?,
        };
        let path =
            manifest::write_manifest(Path::new(&project.path), &m).map_err(CoreError::EnvError)?;
        Ok(path.display().to_string())
    }
}
