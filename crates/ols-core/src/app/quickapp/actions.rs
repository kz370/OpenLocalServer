use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app::quickapp::*;
use crate::app::*;
use crate::domain::{Domain, Ownership};
use crate::error::CoreError;
use crate::quickapp::commands::QuickCommand;
use crate::quickapp::plan::RunPlan;
use crate::quickapp::run::{ActionOutcome, ResolvedProgram, RunCtx};
use crate::quickapp::QuickHost;

impl Inner {
    pub(crate) fn quick_action(
        &self,
        action: &str,
        with: &BTreeMap<String, String>,
        ctx: &RunCtx,
        log: &mut dyn FnMut(&str),
    ) -> Result<ActionOutcome, String> {
        let get = |k: &str| {
            with.get(k)
                .map(String::as_str)
                .ok_or_else(|| format!("the \"{action}\" action needs \"{k}\""))
        };
        match action {
            "make_dir" => {
                let path = get("path")?;
                std::fs::create_dir_all(path)
                    .map_err(|e| format!("could not create {path}: {e}"))?;
                Ok(ActionOutcome::default())
            }
            "replace_in_file" => {
                let root = ctx.project_path.as_ref().ok_or("no project folder")?;
                let file = plan::safe_join(&root.display().to_string(), get("file")?)?;
                let text = std::fs::read_to_string(&file)
                    .map_err(|e| format!("could not read {file}: {e}"))?;
                let find = get("find")?;
                if !text.contains(find) {
                    return Err(format!("could not find \"{find}\" in {file}"));
                }
                std::fs::write(&file, text.replacen(find, get("replace")?, 1))
                    .map_err(|e| e.to_string())?;
                Ok(ActionOutcome::default())
            }
            "start_service" => {
                self.start_service_and_wait(get("id")?, log)?;
                Ok(ActionOutcome::default())
            }
            "create_database" => {
                let (engine, name) = (get("engine")?, get("name")?);
                let started = Instant::now();
                loop {
                    match self.services.create_database(engine, name) {
                        Ok(()) => {
                            return Ok(ActionOutcome {
                                detail: Some(format!("database {name}")),
                                ..Default::default()
                            })
                        }
                        // The server may still be finishing startup right after its port opened.
                        Err(e)
                            if started.elapsed() < Duration::from_secs(30)
                                && e.to_lowercase().contains("connect") =>
                        {
                            std::thread::sleep(Duration::from_millis(700));
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            "register_project" => {
                let path = get("path")?;
                let project = self
                    .projects
                    .lock()
                    .unwrap()
                    .register(path)
                    .map_err(|e| e.to_string())?;
                Ok(ActionOutcome {
                    project_id: Some(project.id),
                    ..Default::default()
                })
            }
            "create_domain" => {
                let hostname = get("hostname")?.to_string();
                let kind = match get("kind")? {
                    "php" => SiteKind::Php {
                        version: with.get("php_version").cloned(),
                    },
                    "static" => SiteKind::Static,
                    "proxy" => SiteKind::Proxy {
                        upstream_port: with
                            .get("port")
                            .and_then(|p| p.parse().ok())
                            .ok_or("a proxy site needs a valid port")?,
                        upstream_host: with
                            .get("upstream_host")
                            .map(|h| h.trim().to_string())
                            .filter(|h| !h.is_empty() && h != "127.0.0.1" && h != "localhost"),
                        upstream_https: with.get("upstream_https").is_some_and(|v| v == "true"),
                    },
                    other => return Err(format!("unknown site kind {other}")),
                };
                let app = with.get("app_program").map(|program| AppSpec {
                    executable: program.clone(),
                    args: with
                        .get("app_args")
                        .and_then(|a| serde_json::from_str(a).ok())
                        .unwrap_or_default(),
                    cwd: with.get("app_cwd").cloned().unwrap_or_default(),
                    runtime: with.get("app_runtime").cloned(),
                });
                let root = get("root")?.replace('/', "\\");
                let project_id = ctx
                    .project_path
                    .as_ref()
                    .and_then(|p| self.project_by_path(&p.display().to_string()))
                    .map(|p| p.id);
                let https = with.get("https").is_some_and(|v| v == "true");
                let domain = Domain {
                    hostname: hostname.clone(),
                    project_id,
                    root,
                    kind,
                    https,
                    redirect_https: https,
                    wildcard: with.get("wildcard").is_some_and(|v| v == "true"),
                    enabled: true,
                    ownership: Ownership::Managed,
                    app,
                    blocks: SiteBlocks::default(),
                    generated_hashes: BTreeMap::new(),
                    public_domain: None,
                    tunnel_id: None,
                    server: None,
                    path_prefix: None,
                    icon: None,
                    ..Default::default()
                };
                let cfg = self.web_config();
                let url = self.site_url(&domain, &cfg);
                let mut domains = self.domains.lock().unwrap();
                match domains.get(&hostname) {
                    // Re-running a recipe reuses the domain instead of failing on the duplicate.
                    Some(existing) => {
                        let mut updated = domain;
                        updated.generated_hashes = existing.generated_hashes;
                        domains.update(updated).map_err(|e| e.to_string())?;
                    }
                    None => {
                        domains.add(domain).map_err(|e| e.to_string())?;
                    }
                }
                Ok(ActionOutcome {
                    detail: Some(url.clone()),
                    open_url: Some(url),
                    ..Default::default()
                })
            }
            "trust_ca" => {
                self.certs.ca().ensure_created()?;
                if self.certs.ca().is_trusted() {
                    return Ok(ActionOutcome {
                        detail: Some("already trusted".into()),
                        ..Default::default()
                    });
                }
                self.certs
                    .ca()
                    .trust_current_user()
                    .map_err(|e| format!("Windows did not trust the certificate authority: {e}"))?;
                Ok(ActionOutcome::default())
            }
            "apply_web" => {
                let reports = self.apply_web(&[]).map_err(|e| e.to_string())?;
                for r in &reports {
                    for w in &r.warnings {
                        log(&format!("warning: {w}"));
                    }
                }
                Ok(ActionOutcome::default())
            }
            "health_check" => {
                let report = self
                    .health_check(get("hostname")?)
                    .map_err(|e| e.to_string())?;
                for s in &report.steps {
                    log(&format!(
                        "{} {}: {}",
                        if s.ok { "✓" } else { "✗" },
                        s.name,
                        s.detail
                    ));
                }
                if report.ok {
                    Ok(ActionOutcome::default())
                } else {
                    let failed: Vec<String> = report
                        .steps
                        .iter()
                        .filter(|s| !s.ok)
                        .map(|s| format!("{}: {}", s.name, s.detail))
                        .collect();
                    Err(failed.join("; "))
                }
            }
            "download_extract" => {
                self.download_extract(
                    get("url")?,
                    with.get("sha1_url").map(String::as_str),
                    with.get("sha256").map(String::as_str),
                    get("dest")?,
                    with.get("strip").map(String::as_str),
                    log,
                )?;
                Ok(ActionOutcome::default())
            }
            other => Err(format!("unknown action \"{other}\"")),
        }
    }

    /// Downloads a zip over HTTPS, verifies its checksum, and extracts it (stripping a
    /// leading folder). Used by recipes like WordPress that ship as one archive.
    fn download_extract(
        &self,
        url: &str,
        sha1_url: Option<&str>,
        sha256: Option<&str>,
        dest: &str,
        strip: Option<&str>,
        log: &mut dyn FnMut(&str),
    ) -> Result<(), String> {
        use sha1::Digest as _;
        if !url.starts_with("https://") {
            return Err("downloads must use HTTPS".into());
        }
        if sha1_url.is_none() && sha256.is_none() {
            return Err("a download needs a checksum (sha1_url or sha256)".into());
        }
        log(&format!("Downloading {url}"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let client = reqwest::Client::new();
        let (bytes, expected) = rt.block_on(async {
            let bytes = client
                .get(url)
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| e.to_string())?
                .bytes()
                .await
                .map_err(|e| e.to_string())?;
            let expected = match (sha1_url, sha256) {
                (Some(u), _) => {
                    let text = client
                        .get(u)
                        .send()
                        .await
                        .and_then(|r| r.error_for_status())
                        .map_err(|e| e.to_string())?
                        .text()
                        .await
                        .map_err(|e| e.to_string())?;
                    (
                        "sha1",
                        text.split_whitespace().next().unwrap_or("").to_lowercase(),
                    )
                }
                (None, Some(h)) => ("sha256", h.to_lowercase()),
                _ => unreachable!(),
            };
            Ok::<_, String>((bytes, expected))
        })?;
        let actual = if expected.0 == "sha1" {
            format!("{:x}", sha1::Sha1::digest(&bytes))
        } else {
            format!("{:x}", sha2::Sha256::digest(&bytes))
        };
        if actual != expected.1 {
            return Err(format!(
                "checksum mismatch: expected {}, got {actual}. Nothing was extracted.",
                expected.1
            ));
        }
        log("Checksum verified");
        let cache = self
            .paths
            .cache_dir()
            .join(format!("download-{}.zip", &actual[..16]));
        std::fs::create_dir_all(self.paths.cache_dir()).map_err(|e| e.to_string())?;
        std::fs::write(&cache, &bytes).map_err(|e| e.to_string())?;
        let result = crate::runtime::extract_zip(&cache, Path::new(dest), strip.unwrap_or(""))
            .map_err(|e| e.to_string());
        let _ = std::fs::remove_file(&cache);
        result
    }

    // --------------------------------------------------------------- quick commands
}
