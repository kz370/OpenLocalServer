//! Compose builder (§166): make a `compose.yaml` from picked services.
//!
//! - Free ports on this computer are picked automatically (and never twice in one file).
//! - Passwords are generated, stored in the OS credential store, and written to the file
//!   only as `${OLS_SECRET_*:?...}` variables, so the file can be shared or committed.
//! - Images use fixed version tags. Docker itself verifies every layer's SHA-256 on pull.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::compose::{secret_key, valid_service, COMPOSE_FILE_NAMES};
use crate::error::CoreError;

/// How an environment value is filled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnvValue {
    Literal {
        value: String,
    },
    /// A generated password kept in the credential store.
    Secret,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresetEnv {
    pub key: String,
    pub value: EnvValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresetPort {
    pub container: u16,
    pub default_host: u16,
    pub label: String,
    /// Serves a web page (a candidate for "Create site" and "Open").
    pub web: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresetVolume {
    /// Named volume (`data`) or a path relative to the project folder (`./`).
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub read_only: bool,
}

/// One ready-made service the builder offers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServicePreset {
    pub id: String,
    pub name: String,
    pub category: String,
    pub description: String,
    pub image: String,
    pub tag: String,
    /// Other tags offered in the version picker.
    pub tags: Vec<String>,
    pub default_service: String,
    pub ports: Vec<PresetPort>,
    pub env: Vec<PresetEnv>,
    pub volumes: Vec<PresetVolume>,
    pub command: Option<String>,
    pub working_dir: Option<String>,
    /// Made by the user (Docker → Compose → My presets), not built in.
    #[serde(default)]
    pub custom: bool,
}

fn lit(key: &str, value: &str) -> PresetEnv {
    PresetEnv {
        key: key.into(),
        value: EnvValue::Literal {
            value: value.into(),
        },
    }
}
fn secret(key: &str) -> PresetEnv {
    PresetEnv {
        key: key.into(),
        value: EnvValue::Secret,
    }
}
fn port(container: u16, default_host: u16, label: &str, web: bool) -> PresetPort {
    PresetPort {
        container,
        default_host,
        label: label.into(),
        web,
    }
}
fn vol(source: &str, target: &str) -> PresetVolume {
    PresetVolume {
        source: source.into(),
        target: target.into(),
        read_only: false,
    }
}

#[allow(clippy::too_many_arguments)]
fn preset(
    id: &str,
    name: &str,
    category: &str,
    description: &str,
    image: &str,
    tags: &[&str],
    service: &str,
    ports: Vec<PresetPort>,
    env: Vec<PresetEnv>,
    volumes: Vec<PresetVolume>,
) -> ServicePreset {
    ServicePreset {
        id: id.into(),
        name: name.into(),
        category: category.into(),
        description: description.into(),
        image: image.into(),
        tag: tags[0].into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        default_service: service.into(),
        ports,
        env,
        volumes,
        command: None,
        working_dir: None,
        custom: false,
    }
}

/// The builder's catalog. Tags were checked against Docker Hub when added.
pub fn presets() -> Vec<ServicePreset> {
    let mut node = preset(
        "node-app",
        "Node.js app",
        "App",
        "Runs your project folder with Node (`npm run dev`).",
        "node",
        &["22-alpine", "24-alpine"],
        "app",
        vec![port(3000, 3000, "App", true)],
        vec![lit("NODE_ENV", "development")],
        vec![vol("./", "/app")],
    );
    node.command = Some("sh -c \"npm install && npm run dev\"".into());
    node.working_dir = Some("/app".into());
    vec![
        node,
        preset(
            "php-apache",
            "PHP + Apache",
            "App",
            "Serves your project folder with PHP and Apache.",
            "php",
            &["8.3-apache", "8.4-apache", "8.2-apache"],
            "web",
            vec![port(80, 8080, "Web", true)],
            vec![],
            vec![vol("./", "/var/www/html")],
        ),
        preset(
            "nginx-static",
            "Nginx (static files)",
            "App",
            "Serves the project folder as a static site.",
            "nginx",
            &["1.28-alpine"],
            "web",
            vec![port(80, 8080, "Web", true)],
            vec![],
            vec![PresetVolume {
                source: "./".into(),
                target: "/usr/share/nginx/html".into(),
                read_only: true,
            }],
        ),
        preset(
            "mariadb",
            "MariaDB",
            "Database",
            "MySQL-compatible database with a database and user ready.",
            "mariadb",
            &["11.4", "10.11"],
            "db",
            vec![port(3306, 3307, "MySQL protocol", false)],
            vec![
                secret("MARIADB_ROOT_PASSWORD"),
                lit("MARIADB_DATABASE", "app"),
                lit("MARIADB_USER", "app"),
                secret("MARIADB_PASSWORD"),
            ],
            vec![vol("data", "/var/lib/mysql")],
        ),
        preset(
            "mysql",
            "MySQL",
            "Database",
            "MySQL with a database and user ready.",
            "mysql",
            &["8.4"],
            "mysql",
            vec![port(3306, 3308, "MySQL protocol", false)],
            vec![
                secret("MYSQL_ROOT_PASSWORD"),
                lit("MYSQL_DATABASE", "app"),
                lit("MYSQL_USER", "app"),
                secret("MYSQL_PASSWORD"),
            ],
            vec![vol("data", "/var/lib/mysql")],
        ),
        preset(
            "postgres",
            "PostgreSQL",
            "Database",
            "PostgreSQL with a database and user ready.",
            "postgres",
            &["17", "16"],
            "postgres",
            vec![port(5432, 5433, "PostgreSQL", false)],
            vec![
                lit("POSTGRES_USER", "app"),
                secret("POSTGRES_PASSWORD"),
                lit("POSTGRES_DB", "app"),
            ],
            vec![vol("data", "/var/lib/postgresql/data")],
        ),
        preset(
            "mongodb",
            "MongoDB",
            "Database",
            "Document database with a root user.",
            "mongo",
            &["8.0"],
            "mongo",
            vec![port(27017, 27018, "MongoDB", false)],
            vec![
                lit("MONGO_INITDB_ROOT_USERNAME", "root"),
                secret("MONGO_INITDB_ROOT_PASSWORD"),
            ],
            vec![vol("data", "/data/db")],
        ),
        preset(
            "redis",
            "Redis",
            "Cache & queue",
            "In-memory cache and queue.",
            "redis",
            &["7.4"],
            "redis",
            vec![port(6379, 6380, "Redis", false)],
            vec![],
            vec![vol("data", "/data")],
        ),
        preset(
            "memcached",
            "Memcached",
            "Cache & queue",
            "Simple in-memory cache.",
            "memcached",
            &["1.6"],
            "memcached",
            vec![port(11211, 11212, "Memcached", false)],
            vec![],
            vec![],
        ),
        preset(
            "rabbitmq",
            "RabbitMQ",
            "Cache & queue",
            "Message queue with the management web UI.",
            "rabbitmq",
            &["4.1-management"],
            "rabbitmq",
            vec![
                port(5672, 5673, "AMQP", false),
                port(15672, 15672, "Management UI", true),
            ],
            vec![
                lit("RABBITMQ_DEFAULT_USER", "app"),
                secret("RABBITMQ_DEFAULT_PASS"),
            ],
            vec![vol("data", "/var/lib/rabbitmq")],
        ),
        preset(
            "meilisearch",
            "Meilisearch",
            "Search",
            "Fast search engine with a web UI.",
            "getmeili/meilisearch",
            &["v1.15"],
            "search",
            vec![port(7700, 7700, "API and UI", true)],
            vec![secret("MEILI_MASTER_KEY")],
            vec![vol("data", "/meili_data")],
        ),
        preset(
            "mailpit",
            "Mailpit",
            "Tools",
            "Catches outgoing mail so you can read it in a web page.",
            "axllent/mailpit",
            &["v1.27"],
            "mail",
            vec![
                port(8025, 8026, "Web UI", true),
                port(1025, 1026, "SMTP", false),
            ],
            vec![],
            vec![],
        ),
        preset(
            "adminer",
            "Adminer",
            "Tools",
            "Database manager in the browser (MariaDB, MySQL, PostgreSQL).",
            "adminer",
            &["4.8.1"],
            "adminer",
            vec![port(8080, 8081, "Web UI", true)],
            vec![],
            vec![],
        ),
        preset(
            "phpmyadmin",
            "phpMyAdmin",
            "Tools",
            "MariaDB / MySQL manager in the browser.",
            "phpmyadmin",
            &["5.2"],
            "phpmyadmin",
            vec![port(80, 8082, "Web UI", true)],
            vec![lit("PMA_ARBITRARY", "1")],
            vec![],
        ),
    ]
}

/// One service the user picked.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildService {
    /// A preset id, or `custom`.
    pub preset: String,
    /// Service name in the file.
    pub name: String,
    #[serde(default)]
    pub tag: Option<String>,
    /// Host port per preset port (same order); `None` = pick a free one.
    #[serde(default)]
    pub host_ports: Vec<Option<u16>>,
    /// `custom` only: the image (with tag) and its port.
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub container_port: Option<u16>,
    /// Overrides for literal environment values.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// What to build.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildSpec {
    /// Folder the file goes into (normally a project folder).
    pub dir: String,
    /// Compose project name (`name:`); the folder name when empty.
    #[serde(default)]
    pub name: Option<String>,
    pub services: Vec<BuildService>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannedPort {
    pub service: String,
    pub label: String,
    pub host: u16,
    pub container: u16,
    pub web: bool,
}

/// The file the builder would write, shown before anything is written.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildPlan {
    pub file: String,
    pub exists: bool,
    pub yaml: String,
    pub ports: Vec<PlannedPort>,
    /// `OLS_SECRET_*` variables that get generated passwords.
    pub secrets: Vec<String>,
    pub warnings: Vec<String>,
}

fn bad(problem: &str, cause: String, fix: &str) -> CoreError {
    CoreError::failed_fix(problem, cause, fix)
}

/// Double-quoted YAML scalar.
fn q(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(crate) fn secret_var(service: &str, key: &str) -> String {
    let clean = |s: &str| {
        s.to_uppercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
    };
    format!("OLS_SECRET_{}_{}", clean(service), clean(key))
}

pub(crate) fn valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 128
        && tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

pub(crate) fn valid_image(image: &str) -> bool {
    !image.is_empty()
        && image.len() <= 255
        && image
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '/' | ':' | '@'))
}

/// Builds the plan. `is_free` says whether a host port can be used (real port check in
/// the app; a fake in tests).
pub fn plan(spec: &BuildSpec, is_free: &dyn Fn(u16) -> bool) -> Result<BuildPlan, CoreError> {
    plan_with(spec, &presets(), is_free)
}

/// `plan` against a given catalog (built-in plus the user's own presets).
pub fn plan_with(
    spec: &BuildSpec,
    catalog: &[ServicePreset],
    is_free: &dyn Fn(u16) -> bool,
) -> Result<BuildPlan, CoreError> {
    let dir = PathBuf::from(&spec.dir);
    if !dir.is_absolute() || !dir.is_dir() {
        return Err(bad(
            "Choose a folder for the compose file.",
            format!("\"{}\" is not an existing folder.", spec.dir),
            "Pick one of your projects, or browse to a folder.",
        ));
    }
    if spec.services.is_empty() {
        return Err(bad(
            "Add at least one service.",
            "The compose file would be empty.".into(),
            "Pick a service such as MariaDB or Redis.",
        ));
    }
    let file = COMPOSE_FILE_NAMES
        .iter()
        .map(|n| dir.join(n))
        .find(|f| f.is_file())
        .unwrap_or_else(|| dir.join("compose.yaml"));
    let name = spec
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(super::compose::sanitize_name)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| super::compose::project_name(&file, None));

    let mut used_names = BTreeSet::new();
    let mut used_ports = BTreeSet::new();
    let mut ports = Vec::new();
    let mut secrets = Vec::new();
    let mut warnings = Vec::new();
    let mut named_volumes = Vec::new();
    let mut out = String::new();
    out.push_str("# Created by Open Local Server (Docker → Compose).\n");
    out.push_str(
        "# Passwords are not in this file: OLS keeps them in the Windows credential store\n",
    );
    out.push_str("# and passes them in when you start the stack from OLS.\n");
    out.push_str(&format!("name: {}\n\nservices:\n", q(&name)));

    for s in &spec.services {
        let svc = s.name.trim();
        if !valid_service(svc) || !used_names.insert(svc.to_string()) {
            return Err(bad(
                "Each service needs its own simple name.",
                format!("\"{svc}\" is empty, repeated, or uses characters other than letters, digits, `_`, `.` and `-`."),
                "Rename the service.",
            ));
        }
        let (image, preset_ports, env, volumes, command, working_dir) = if s.preset == "custom" {
            let image = s.image.as_deref().map(str::trim).unwrap_or_default();
            if !valid_image(image) {
                return Err(bad(
                    "Enter the image for the custom service.",
                    format!("\"{image}\" is not an image name like `nginx:1.28` or `ghcr.io/org/app:1.0`."),
                    "Type the image name with its tag.",
                ));
            }
            let ports = s
                .container_port
                .map(|p| vec![port(p, p.max(1024), "Port", true)])
                .unwrap_or_default();
            (image.to_string(), ports, Vec::new(), Vec::new(), None, None)
        } else {
            let p = catalog.iter().find(|p| p.id == s.preset).ok_or_else(|| {
                bad(
                    "Unknown service type.",
                    format!("\"{}\" is not in the builder's list.", s.preset),
                    "Pick a service from the list.",
                )
            })?;
            let tag = s.tag.as_deref().unwrap_or(&p.tag);
            if !valid_tag(tag) {
                return Err(bad(
                    "That version is not valid.",
                    format!("\"{tag}\" is not a Docker tag."),
                    "Pick a version from the list.",
                ));
            }
            (
                format!("{}:{tag}", p.image),
                p.ports.clone(),
                p.env.clone(),
                p.volumes.clone(),
                p.command.clone(),
                p.working_dir.clone(),
            )
        };
        let last = image.rsplit('/').next().unwrap_or(&image);
        if !last.contains(':') || last.ends_with(":latest") {
            warnings.push(format!("`{svc}` uses `{image}` without a fixed version."));
        }

        out.push_str(&format!("  {svc}:\n    image: {}\n", q(&image)));
        if let Some(wd) = working_dir {
            out.push_str(&format!("    working_dir: {}\n", q(&wd)));
        }
        if let Some(cmd) = command {
            // Quoted: a user preset's command is text, never YAML structure.
            out.push_str(&format!("    command: {}\n", q(&cmd)));
        }
        out.push_str("    restart: unless-stopped\n");
        if !preset_ports.is_empty() {
            out.push_str("    ports:\n");
        }
        for (i, pp) in preset_ports.iter().enumerate() {
            let wanted = s.host_ports.get(i).copied().flatten();
            let host = match wanted {
                Some(h) => {
                    if h == 0 || !used_ports.insert(h) {
                        return Err(bad(
                            "Two services use the same port.",
                            format!("Port {h} is picked more than once."),
                            "Leave the port empty so OLS picks a free one, or choose another.",
                        ));
                    }
                    if !is_free(h) {
                        warnings.push(format!(
                            "Port {h} for `{svc}` is in use on this computer right now; the stack will not start until it is free."
                        ));
                    }
                    h
                }
                None => {
                    let start = pp.default_host;
                    let found = (0..200u16)
                        .filter_map(|o| start.checked_add(o))
                        .find(|p| !used_ports.contains(p) && is_free(*p))
                        .ok_or_else(|| {
                            bad(
                                "No free port was found.",
                                format!(
                                    "Ports {start}–{} are all in use.",
                                    start.saturating_add(199)
                                ),
                                "Enter a port yourself.",
                            )
                        })?;
                    used_ports.insert(found);
                    found
                }
            };
            // Loopback only: a dev stack is not published to the network.
            out.push_str(&format!(
                "      - {}\n",
                q(&format!("127.0.0.1:{host}:{}", pp.container))
            ));
            ports.push(PlannedPort {
                service: svc.into(),
                label: pp.label.clone(),
                host,
                container: pp.container,
                web: pp.web,
            });
        }
        if !env.is_empty() {
            out.push_str("    environment:\n");
        }
        for e in &env {
            let value = match &e.value {
                EnvValue::Literal { value } => {
                    q(s.env.get(&e.key).map(String::as_str).unwrap_or(value))
                }
                EnvValue::Secret => {
                    let var = secret_var(svc, &e.key);
                    secrets.push(var.clone());
                    q(&format!(
                        "${{{var}:?Start this stack from Open Local Server (Docker → Compose)}}"
                    ))
                }
            };
            out.push_str(&format!("      {}: {value}\n", e.key));
        }
        if !volumes.is_empty() {
            out.push_str("    volumes:\n");
        }
        for v in &volumes {
            let source = if v.source.starts_with("./") || v.source == "." {
                v.source.clone()
            } else {
                let named = format!("{svc}-{}", v.source);
                named_volumes.push(named.clone());
                named
            };
            let ro = if v.read_only { ":ro" } else { "" };
            out.push_str(&format!(
                "      - {}\n",
                q(&format!("{source}:{}{ro}", v.target))
            ));
        }
        out.push_str("    extra_hosts:\n      - \"host.docker.internal:host-gateway\"\n");
        out.push_str("    labels:\n      dev.ols.managed: \"true\"\n");
    }
    if !named_volumes.is_empty() {
        out.push_str("\nvolumes:\n");
        for v in &named_volumes {
            out.push_str(&format!("  {v}: {{}}\n"));
        }
    }
    // The builder's own output must pass its own checks.
    let check = super::compose::check_text(&out);
    if !check.ok {
        return Err(bad(
            "The compose file could not be built.",
            check.errors.join(" "),
            "Change the service names or ports and try again.",
        ));
    }
    Ok(BuildPlan {
        exists: file.is_file(),
        file: file.display().to_string(),
        yaml: out,
        ports,
        secrets,
        warnings,
    })
}

/// Writes the planned file and stores a fresh password for every secret.
/// Refuses to replace an existing file unless `overwrite`.
pub fn create(
    spec: &BuildSpec,
    overwrite: bool,
    backups_dir: &Path,
    is_free: &dyn Fn(u16) -> bool,
    store_secret: &dyn Fn(&str, &str) -> Result<(), String>,
) -> Result<BuildPlan, CoreError> {
    create_with(
        spec,
        &presets(),
        overwrite,
        backups_dir,
        is_free,
        store_secret,
    )
}

/// `create` against a given catalog.
pub fn create_with(
    spec: &BuildSpec,
    catalog: &[ServicePreset],
    overwrite: bool,
    backups_dir: &Path,
    is_free: &dyn Fn(u16) -> bool,
    store_secret: &dyn Fn(&str, &str) -> Result<(), String>,
) -> Result<BuildPlan, CoreError> {
    let plan = plan_with(spec, catalog, is_free)?;
    let file = PathBuf::from(&plan.file);
    if plan.exists && !overwrite {
        return Err(bad(
            "This folder already has a compose file.",
            format!("{} exists.", plan.file),
            "Open it from the Compose tab, or choose to replace it (the old file is backed up).",
        ));
    }
    for var in &plan.secrets {
        let value = crate::crypto::random_password().map_err(|e| {
            bad(
                "A password could not be generated.",
                e.to_string(),
                "Try again.",
            )
        })?;
        store_secret(&secret_key(&file, var), &value).map_err(|e| {
            bad(
                "A password could not be stored.",
                format!("The Windows credential store answered: {e}"),
                "Sign in to Windows normally (not as a temporary profile) and try again.",
            )
        })?;
    }
    if plan.exists {
        super::compose::save_file(&plan.file, &plan.yaml, backups_dir)?;
    } else {
        std::fs::write(&file, &plan.yaml).map_err(|e| {
            bad(
                "The compose file could not be written.",
                format!("{}: {e}", file.display()),
                "Check that the folder is writable.",
            )
        })?;
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn svc(preset: &str, name: &str) -> BuildService {
        BuildService {
            preset: preset.into(),
            name: name.into(),
            tag: None,
            host_ports: vec![],
            image: None,
            container_port: None,
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn presets_are_pinned_and_unique() {
        let all = presets();
        let ids: BTreeSet<_> = all.iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids.len(), all.len());
        for p in &all {
            assert!(
                valid_tag(&p.tag) && p.tag != "latest",
                "{} must pin a version",
                p.id
            );
            assert_eq!(p.tags[0], p.tag);
        }
    }

    #[test]
    fn plan_picks_free_ports_and_keeps_secrets_out() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = BuildSpec {
            dir: tmp.path().display().to_string(),
            name: Some("Shop".into()),
            services: vec![
                svc("mariadb", "db"),
                svc("adminer", "adminer"),
                svc("redis", "cache"),
            ],
        };
        // 3307 and 8081 are "taken".
        let plan = plan(&spec, &|p| p != 3307 && p != 8081).unwrap();
        assert!(!plan.exists);
        assert!(plan.file.ends_with("compose.yaml"));
        let db = plan.ports.iter().find(|p| p.service == "db").unwrap();
        assert_eq!(db.host, 3308);
        let adminer = plan.ports.iter().find(|p| p.service == "adminer").unwrap();
        assert_eq!((adminer.host, adminer.web), (8082, true));
        assert_eq!(
            plan.secrets,
            vec![
                "OLS_SECRET_DB_MARIADB_ROOT_PASSWORD",
                "OLS_SECRET_DB_MARIADB_PASSWORD"
            ]
        );
        assert!(plan.yaml.contains("${OLS_SECRET_DB_MARIADB_PASSWORD:?"));
        assert!(plan.yaml.contains("name: \"shop\""));
        assert!(plan.yaml.contains("\"127.0.0.1:3308:3306\""));
        assert!(plan.yaml.contains("db-data: {}"));
        // Valid YAML that compose's own checks accept.
        let check = super::super::compose::check_text(&plan.yaml);
        assert!(
            check.ok && check.warnings.is_empty(),
            "{check:?}\n{}",
            plan.yaml
        );
        let doc: serde_yaml_ng::Value = serde_yaml_ng::from_str(&plan.yaml).unwrap();
        assert_eq!(super::super::compose::parse_services(&doc).len(), 3);
    }

    #[test]
    fn plan_rejects_bad_input() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().display().to_string();
        let free = |_| true;
        let mk = |services| BuildSpec {
            dir: dir.clone(),
            name: None,
            services,
        };
        assert!(plan(&mk(vec![]), &free).is_err());
        assert!(
            plan(&mk(vec![svc("redis", "a"), svc("redis", "a")]), &free).is_err(),
            "duplicate names"
        );
        assert!(plan(&mk(vec![svc("redis", "a b")]), &free).is_err());
        assert!(plan(&mk(vec![svc("nope", "a")]), &free).is_err());
        let mut custom = svc("custom", "x");
        custom.image = Some("bad image".into());
        assert!(plan(&mk(vec![custom]), &free).is_err());
        let mut a = svc("redis", "a");
        a.host_ports = vec![Some(7000)];
        let mut b = svc("memcached", "b");
        b.host_ports = vec![Some(7000)];
        assert!(plan(&mk(vec![a, b]), &free).is_err(), "same port twice");
        let missing = BuildSpec {
            dir: "/definitely/not/here".into(),
            name: None,
            services: vec![svc("redis", "r")],
        };
        assert!(plan(&missing, &free).is_err());
    }

    #[test]
    fn create_stores_secrets_and_refuses_silent_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = BuildSpec {
            dir: tmp.path().display().to_string(),
            name: None,
            services: vec![svc("postgres", "db")],
        };
        let stored = RefCell::new(Vec::new());
        let store = |k: &str, v: &str| {
            stored.borrow_mut().push((k.to_string(), v.to_string()));
            Ok(())
        };
        let backups = tmp.path().join("backups");
        let plan = create(&spec, false, &backups, &|_| true, &store).unwrap();
        let text = std::fs::read_to_string(&plan.file).unwrap();
        let stored = stored.borrow();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].0.ends_with(":OLS_SECRET_DB_POSTGRES_PASSWORD"));
        assert!(stored[0].1.len() >= 16);
        assert!(
            !text.contains(&stored[0].1),
            "the password never lands in the file"
        );
        assert!(create(&spec, false, &backups, &|_| true, &|_, _| Ok(())).is_err());
        assert!(create(&spec, true, &backups, &|_| true, &|_, _| Ok(())).is_ok());
    }
}
