//! Dockerize a project (2.11.0, extended in 2.11.0): a Dockerfile, a compose file and a
//! `.dockerignore` made from what OLS can see in a project folder. They are a starting point,
//! never a guarantee, and the user is told so.
//!
//! 2.11.0: the project is COPIED into the image (no bind mount, so it is fast on Windows);
//! dev stacks sync edits with `docker compose watch`; the services a project talks to
//! (database, cache, queue, mail, search) are added as real, pinned images with generated
//! passwords; Go, Rust, Ruby, Java and .NET join PHP, Node, Python and static sites; a
//! Production mode builds multi-stage images that run as a non-root user.
//!
//! Safety rules (binding):
//! - Nothing from a project file is pasted into a generated file except values taken from a
//!   closed set: a project kind, version digits, a script *name* (`dev`/`start`/`build`), an
//!   identifier made only of letters, digits, `_`, `-`, `.`, a PHP extension on the allow-list
//!   and system packages from fixed tables. Project scripts are never run.
//! - Existing files are never overwritten: an existing Dockerfile is kept, an existing compose
//!   file is replaced only when asked (and backed up first).
//! - Each detected file is read up to 64 KB, and never through a symbolic link.
//! - Every image tag is pinned; nothing is downloaded by OLS (Docker pulls the images).
//! - Passwords are generated, kept in the OS credential store and written to the compose file
//!   only as `${OLS_SECRET_*}` variables.

mod detect;
mod stacks;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::builder::{self, secret_var, EnvValue};
use super::compose;
use crate::error::CoreError;
use detect::{Catalog, Signals, SERVICES};

/// Always shown, never reworded per project.
pub const DISCLAIMER: &str = "OLS generated these files from what it could detect in your project. They are a starting point, not a guaranteed working setup. Review them, then build and test before you rely on them.";

/// "OLS cannot see these": always all five.
pub const CHECKLIST: [&str; 5] = [
    "System packages your app needs are only installed when listed above (for example ffmpeg, imagemagick, wkhtmltopdf may be missing).",
    "Queue workers, schedulers and cron jobs are only started when listed above.",
    "Build steps (for example npm run build, composer install --no-dev, asset compilation) are only included when listed above.",
    "Your .env file and other secrets are not copied into the image: pass them with environment variables.",
    "Services your app talks to are only created when listed above: add the others in Docker → Compose, or connect to the ones OLS runs.",
];

/// The installer image's fixed tag. A maintainer must confirm it exists (ideally add
/// `@sha256:<digest>`); it is never `latest`.
pub const PHP_EXT_INSTALLER: &str = "2.7";

/// Extensions OLS may install. Anything else is reported, never written to a file.
pub const PHP_EXT_ALLOW: &[&str] = &[
    "bcmath",
    "calendar",
    "exif",
    "gd",
    "gettext",
    "imagick",
    "intl",
    "mysqli",
    "opcache",
    "pcntl",
    "pdo_mysql",
    "pdo_pgsql",
    "pgsql",
    "redis",
    "soap",
    "sockets",
    "sysvsem",
    "xsl",
    "zip",
    "apcu",
    "memcached",
    "mongodb",
    "xdebug",
];

/// Built into the official `php` image: needing one of these asks for nothing.
const PHP_BUILTIN: &[&str] = &[
    "core",
    "ctype",
    "curl",
    "date",
    "dom",
    "fileinfo",
    "filter",
    "ftp",
    "hash",
    "iconv",
    "json",
    "libxml",
    "mbstring",
    "mysqlnd",
    "openssl",
    "pcre",
    "pdo",
    "pdo_sqlite",
    "phar",
    "posix",
    "readline",
    "reflection",
    "session",
    "simplexml",
    "sodium",
    "spl",
    "sqlite3",
    "standard",
    "tokenizer",
    "xml",
    "xmlreader",
    "xmlwriter",
    "zlib",
];

const MAX_READ: u64 = 64 * 1024;

/// Dev watches files and syncs edits; Prod builds a lean image that runs as a non-root user.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DockerizeMode {
    #[default]
    Dev,
    Prod,
}

/// What the user chose in the dialog.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerizeOptions {
    #[serde(default)]
    pub mode: DockerizeMode,
    /// Service ids to add (`mariadb`, `redis`, …). `None` = everything OLS detected.
    #[serde(default)]
    pub services: Option<Vec<String>>,
}

/// One service Dockerize can add to the compose file.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannedService {
    pub id: String,
    pub name: String,
    /// Name in the compose file.
    pub service: String,
    /// Pinned image with its tag.
    pub image: String,
    /// Why OLS thinks the project needs it (empty when it does not).
    pub reason: String,
    pub detected: bool,
    pub selected: bool,
    /// Offered, but never ticked by default.
    pub optional: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerizePlan {
    /// `node`, `php`, `python`, `go`, `rust`, `ruby`, `java`, `dotnet` or `static`.
    pub kind: String,
    pub mode: DockerizeMode,
    /// `None` when a Dockerfile already exists (it is kept).
    pub dockerfile: Option<String>,
    pub compose: String,
    /// `None` when a `.dockerignore` already exists (it is kept).
    pub dockerignore: Option<String>,
    pub dockerfile_path: String,
    pub compose_path: String,
    /// Files that already exist and will not be overwritten.
    pub existing: Vec<String>,
    pub port: u16,
    pub notes: Vec<String>,
    /// PHP only: extensions that will be installed.
    pub php_extensions: Vec<String>,
    /// PHP only: needed but not installable by OLS.
    pub unknown_extensions: Vec<String>,
    /// What OLS found and where; defaults say "default", inferred values say "guessed".
    pub detected: Vec<String>,
    pub disclaimer: String,
    pub checklist: Vec<String>,
    /// Every service OLS can add, with the detected ones ticked.
    pub services: Vec<PlannedService>,
    /// Extra processes started from the same image (queue worker, scheduler, …).
    pub workers: Vec<String>,
    /// `OLS_SECRET_*` variables that get a generated password when the files are written.
    pub secrets: Vec<String>,
    /// The compose file has `develop.watch` rules: start it with `docker compose watch`.
    pub watch: bool,
}

fn bad(problem: &str, cause: impl Into<String>, fix: &str) -> CoreError {
    CoreError::failed_fix(problem, cause, fix)
}

/// Which family of settings the app receives for the services it uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Framework {
    Generic,
    Laravel,
    WordPress,
    Django,
    Rails,
}

/// One `develop.watch` rule.
pub(crate) struct Watch {
    pub action: &'static str,
    pub path: String,
    pub target: Option<String>,
    pub ignore: Vec<String>,
}

/// A process that runs from the app image.
pub(crate) struct Worker {
    pub suffix: &'static str,
    pub command: Vec<String>,
    pub why: String,
}

pub(crate) struct Built {
    pub kind: &'static str,
    pub framework: Framework,
    pub dockerfile: String,
    pub port: u16,
    /// Literal environment values (all written in this crate).
    pub env: Vec<(String, String)>,
    /// Environment keys filled from a generated password.
    pub app_secrets: Vec<&'static str>,
    pub watch: Vec<Watch>,
    pub workers: Vec<Worker>,
    /// Extra `.dockerignore` lines.
    pub ignore: Vec<&'static str>,
    pub detected: Vec<String>,
    pub notes: Vec<String>,
    pub php_extensions: Vec<String>,
    pub unknown_extensions: Vec<String>,
}

/// Reads up to 64 KB of a regular file (not a link) inside the project.
pub(crate) fn read_small(dir: &Path, name: &str) -> Option<String> {
    let p = dir.join(name);
    let meta = std::fs::symlink_metadata(&p).ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    let mut raw = Vec::new();
    std::fs::File::open(&p)
        .ok()?
        .take(MAX_READ)
        .read_to_end(&mut raw)
        .ok()?;
    Some(String::from_utf8_lossy(&raw).into_owned())
}

pub(crate) fn exists(dir: &Path, name: &str) -> bool {
    std::fs::symlink_metadata(dir.join(name)).is_ok()
}

/// The folder name as a compose service name.
fn slug(dir: &Path) -> String {
    let raw = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let s: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() || !s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) {
        "app".into()
    } else {
        s
    }
}

/// Digits of the first number in `text` (`^8.3` -> `8.3`; `>=20` -> `20`).
pub(crate) fn first_number(text: &str) -> Option<(u32, Option<u32>)> {
    let re = regex::Regex::new(r"(\d{1,3})(?:\.(\d{1,3}))?").ok()?;
    let c = re.captures(text)?;
    let major = c[1].parse().ok()?;
    let minor = c.get(2).and_then(|m| m.as_str().parse().ok());
    Some((major, minor))
}

/// Keeps only characters that are safe inside a YAML/Dockerfile comment.
fn comment_safe(text: &str) -> String {
    let ok = |c: char| c.is_ascii_alphanumeric() || " .,:;()_/\"'=^~<>-".contains(c);
    text.chars()
        .map(|c| if ok(c) { c } else { '?' })
        .take(300)
        .collect()
}

fn header(detected: &[String]) -> String {
    format!(
        "# Generated by Open Local Server (Docker -> Dockerize a project).\n# Starting point only, NOT guaranteed to work: review it, then build and test it.\n# Detected: {}\n",
        comment_safe(&detected.join("; "))
    )
}

fn yq(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// WordPress has no composer.json: its own files give it away.
fn is_wordpress(dir: &Path) -> bool {
    [
        "wp-config.php",
        "wp-config-sample.php",
        "wp-load.php",
        "wp-content",
    ]
    .iter()
    .any(|n| exists(dir, n))
}

/// A plain PHP project: no composer.json, just `.php` files at the top (or `public/index.php`).
fn has_php_files(dir: &Path) -> bool {
    if exists(dir, "public/index.php") {
        return true;
    }
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten().any(|e| {
                e.file_type().is_ok_and(|t| t.is_file())
                    && e.path()
                        .extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case("php"))
            })
        })
        .unwrap_or(false)
}

/// Extension names the project asks for. `declared` come from composer files; `guessed` from
/// `.env.example` / `.env` and the kind of project.
pub(crate) fn needed_php_extensions(
    dir: &Path,
    sig: &Signals,
    laravel: bool,
    wordpress: bool,
) -> (BTreeSet<String>, BTreeSet<String>, bool) {
    let mut declared = BTreeSet::new();
    let mut invalid = false;
    let re = regex::Regex::new(r#""ext-([^"]{1,64})""#).expect("valid regex");
    for file in ["composer.json", "composer.lock"] {
        let Some(text) = read_small(dir, file) else {
            continue;
        };
        for c in re.captures_iter(&text) {
            let name = c[1].to_lowercase();
            if !name.is_empty()
                && name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            {
                declared.insert(name);
            } else {
                invalid = true;
            }
        }
    }
    let mut guessed = BTreeSet::new();
    match sig.env.get("DB_CONNECTION").map(String::as_str) {
        Some("mysql" | "mariadb") => {
            guessed.insert("pdo_mysql".to_string());
        }
        Some("pgsql") => {
            guessed.insert("pdo_pgsql".to_string());
        }
        Some("sqlsrv") => {
            guessed.insert("pdo_sqlsrv".to_string());
        }
        _ => {}
    }
    if [
        "CACHE_DRIVER",
        "CACHE_STORE",
        "SESSION_DRIVER",
        "QUEUE_CONNECTION",
    ]
    .iter()
    .any(|k| sig.env_is(k, &["redis"]))
    {
        guessed.insert("redis".to_string());
    }
    if laravel {
        guessed.extend(["pdo_mysql", "zip", "bcmath", "intl", "gd"].map(String::from));
    }
    if wordpress {
        guessed.extend(
            ["mysqli", "gd", "zip", "intl", "exif", "imagick", "opcache"].map(String::from),
        );
    }
    (declared, guessed, invalid)
}

fn compose_file_in(dir: &Path) -> Option<String> {
    compose::COMPOSE_FILE_NAMES
        .iter()
        .find(|n| exists(dir, n))
        .map(|n| n.to_string())
}

const SECRET_MSG: &str = "Start this stack from Open Local Server (Docker → Compose)";

fn secret_ref(var: &str) -> String {
    format!("${{{var}:?{SECRET_MSG}}}")
}

/// Health checks for services other containers wait for.
fn healthcheck(id: &str) -> Option<&'static str> {
    Some(match id {
        "mariadb" => r#"["CMD", "healthcheck.sh", "--connect", "--innodb_initialized"]"#,
        "postgres" => r#"["CMD-SHELL", "pg_isready -U app -d app"]"#,
        "redis" => r#"["CMD", "redis-cli", "ping"]"#,
        "mongodb" => {
            r#"["CMD-SHELL", "mongosh --quiet --eval \"db.adminCommand('ping').ok\" | grep -q 1"]"#
        }
        "rabbitmq" => r#"["CMD", "rabbitmq-diagnostics", "-q", "ping"]"#,
        _ => return None,
    })
}

/// What the app receives so it finds a service: every value is written here or is a
/// reference to a generated password.
fn service_env(cat: &Catalog, fw: Framework, primary_sql: bool) -> Vec<(String, String)> {
    let svc = cat.service;
    let pw = |key: &str| secret_ref(&secret_var(svc, key));
    let kv = |k: &str, v: String| (k.to_string(), v);
    match cat.id {
        "mariadb" if !primary_sql => vec![],
        "mariadb" => match fw {
            Framework::Laravel => vec![
                kv("DB_CONNECTION", "mysql".into()),
                kv("DB_HOST", svc.into()),
                kv("DB_PORT", "3306".into()),
                kv("DB_DATABASE", "app".into()),
                kv("DB_USERNAME", "app".into()),
                kv("DB_PASSWORD", pw("MARIADB_PASSWORD")),
            ],
            Framework::WordPress => vec![
                kv("WORDPRESS_DB_HOST", svc.into()),
                kv("WORDPRESS_DB_NAME", "app".into()),
                kv("WORDPRESS_DB_USER", "app".into()),
                kv("WORDPRESS_DB_PASSWORD", pw("MARIADB_PASSWORD")),
            ],
            _ => {
                let scheme = if fw == Framework::Rails {
                    "mysql2"
                } else {
                    "mysql"
                };
                vec![
                    kv("DB_HOST", svc.into()),
                    kv("DB_PORT", "3306".into()),
                    kv("DB_NAME", "app".into()),
                    kv("DB_USER", "app".into()),
                    kv("DB_PASSWORD", pw("MARIADB_PASSWORD")),
                    kv(
                        "DATABASE_URL",
                        format!("{scheme}://app:{}@{svc}:3306/app", pw("MARIADB_PASSWORD")),
                    ),
                ]
            }
        },
        "postgres" if !primary_sql => vec![],
        "postgres" => match fw {
            Framework::Laravel => vec![
                kv("DB_CONNECTION", "pgsql".into()),
                kv("DB_HOST", svc.into()),
                kv("DB_PORT", "5432".into()),
                kv("DB_DATABASE", "app".into()),
                kv("DB_USERNAME", "app".into()),
                kv("DB_PASSWORD", pw("POSTGRES_PASSWORD")),
            ],
            _ => {
                let scheme = if fw == Framework::Rails {
                    "postgresql"
                } else {
                    "postgres"
                };
                vec![
                    kv("DB_HOST", svc.into()),
                    kv("DB_PORT", "5432".into()),
                    kv("DB_NAME", "app".into()),
                    kv("DB_USER", "app".into()),
                    kv("DB_PASSWORD", pw("POSTGRES_PASSWORD")),
                    kv(
                        "DATABASE_URL",
                        format!("{scheme}://app:{}@{svc}:5432/app", pw("POSTGRES_PASSWORD")),
                    ),
                ]
            }
        },
        "mongodb" => {
            let uri = format!(
                "mongodb://root:{}@{svc}:27017/?authSource=admin",
                pw("MONGO_INITDB_ROOT_PASSWORD")
            );
            vec![kv("MONGODB_URI", uri.clone()), kv("MONGO_URL", uri)]
        }
        "redis" => vec![
            kv("REDIS_HOST", svc.into()),
            kv("REDIS_PORT", "6379".into()),
            kv("REDIS_URL", format!("redis://{svc}:6379")),
        ],
        "rabbitmq" => vec![
            kv("RABBITMQ_HOST", svc.into()),
            kv("RABBITMQ_PORT", "5672".into()),
            kv("RABBITMQ_USER", "app".into()),
            kv("RABBITMQ_PASSWORD", pw("RABBITMQ_DEFAULT_PASS")),
            kv(
                "AMQP_URL",
                format!("amqp://app:{}@{svc}:5672", pw("RABBITMQ_DEFAULT_PASS")),
            ),
        ],
        "meilisearch" => vec![
            kv("MEILISEARCH_HOST", format!("http://{svc}:7700")),
            kv("MEILI_HOST", format!("http://{svc}:7700")),
            kv("MEILISEARCH_KEY", pw("MEILI_MASTER_KEY")),
        ],
        "mailpit" => vec![
            kv("MAIL_MAILER", "smtp".into()),
            kv("MAIL_HOST", svc.into()),
            kv("MAIL_PORT", "1025".into()),
            kv("SMTP_HOST", svc.into()),
            kv("SMTP_PORT", "1025".into()),
        ],
        _ => vec![],
    }
}

fn dockerignore_text(extra: &[&str]) -> String {
    let mut lines: Vec<&str> = vec![
        ".git",
        ".idea",
        ".vscode",
        ".DS_Store",
        "*.log",
        ".env",
        ".env.*",
        "!.env.example",
    ];
    for e in extra {
        if !lines.contains(e) {
            lines.push(e);
        }
    }
    format!(
        "# Generated by Open Local Server (Docker -> Dockerize a project). Review it.\n# Keeps secrets and heavy folders out of the image, so builds are fast and small.\n{}\n",
        lines.join("\n")
    )
}

/// Builds the plan without touching the disk. `is_free` says whether a host port is usable.
pub fn plan(
    project_dir: &Path,
    _project_type: Option<&str>,
    is_free: &dyn Fn(u16) -> bool,
    options: &DockerizeOptions,
) -> Result<DockerizePlan, CoreError> {
    if !project_dir.is_dir() {
        return Err(bad(
            "That project folder was not found.",
            format!("{} is not a folder on disk.", project_dir.display()),
            "Check that the folder still exists, or remove the project from the list.",
        ));
    }
    let mode = options.mode;
    let sig = detect::collect(project_dir);
    let wordpress = is_wordpress(project_dir);
    let d = project_dir;
    let built = if wordpress || exists(d, "composer.json") {
        stacks::php(d, &sig, mode, exists(d, "artisan"), wordpress)
    } else if exists(d, "package.json") {
        stacks::node(d, &sig, mode)
    } else if exists(d, "go.mod") {
        stacks::go(d, mode)
    } else if exists(d, "Cargo.toml") {
        stacks::rust(d, mode)
    } else if exists(d, "Gemfile") {
        stacks::ruby(d, &sig, mode)
    } else if exists(d, "pom.xml") || exists(d, "build.gradle") || exists(d, "build.gradle.kts") {
        stacks::java(d, mode)
    } else if let Some(b) = stacks::dotnet(d, mode) {
        b
    } else if exists(d, "requirements.txt") || exists(d, "pyproject.toml") {
        stacks::python(d, &sig, mode)
    } else if has_php_files(d) {
        stacks::php(d, &sig, mode, false, false)
    } else if exists(d, "index.html") {
        stacks::static_site(mode)
    } else {
        let hint = detect::SUBFOLDERS
            .iter()
            .find(|s| detect::has_dir(d, s) && detect::marker_in(&d.join(s)))
            .map(|s| {
                format!(" A project was found in the \"{s}\" folder: add that folder as its own project and Dockerize it.")
            })
            .unwrap_or_default();
        return Err(bad(
            "OLS could not recognise this project.",
            format!("None of composer.json, package.json, go.mod, Cargo.toml, Gemfile, pom.xml, build.gradle, a .csproj file, requirements.txt, pyproject.toml, a .php file or index.html was found at the top of the folder.{hint}"),
            "Dockerize supports PHP, Node, Python, Go, Rust, Ruby, Java, .NET and static sites. Write a Dockerfile by hand for anything else.",
        ));
    };

    let host = (0..200u16)
        .filter_map(|o| built.port.checked_add(o))
        .find(|p| is_free(*p))
        .ok_or_else(|| {
            bad(
                "No free port was found.",
                format!(
                    "Ports {}–{} are all in use.",
                    built.port,
                    built.port.saturating_add(199)
                ),
                "Stop something that uses them, or write the port in the compose file yourself.",
            )
        })?;

    // ---- services
    let detected_services = detect::detect_services(project_dir, &sig, wordpress);
    let presets = builder::presets();
    let mut services: Vec<PlannedService> = Vec::new();
    for cat in SERVICES {
        let Some(preset) = presets.iter().find(|p| p.id == cat.preset) else {
            continue;
        };
        let why = detected_services
            .iter()
            .find(|(id, _)| *id == cat.id)
            .map(|(_, w)| w.clone());
        let detected = why.is_some();
        let selected = match &options.services {
            Some(list) => list.iter().any(|s| s == cat.id),
            None => {
                detected && !cat.optional && !(mode == DockerizeMode::Prod && cat.id == "mailpit")
            }
        };
        services.push(PlannedService {
            id: cat.id.into(),
            name: cat.name.into(),
            service: cat.service.into(),
            image: format!("{}:{}", preset.image, preset.tag),
            reason: why.unwrap_or_default(),
            detected,
            selected,
            optional: cat.optional,
        });
    }

    let mut existing = Vec::new();
    let dockerfile_exists = exists(project_dir, "Dockerfile");
    if dockerfile_exists {
        existing.push("Dockerfile".to_string());
    }
    let existing_compose = compose_file_in(project_dir);
    if let Some(name) = &existing_compose {
        existing.push(name.clone());
    }
    let mut notes = built.notes.clone();
    let mut detected = built.detected.clone();
    if dockerfile_exists {
        notes.push(
            "A Dockerfile already exists and is kept: the compose file builds from it.".into(),
        );
    }
    if existing_compose.is_some() {
        notes.push("A compose file already exists: it is only replaced if you choose to (the old one is backed up).".into());
    }
    for s in services.iter().filter(|s| s.selected) {
        detected.push(format!(
            "Service: {} ({})",
            s.name,
            if s.reason.is_empty() {
                "added by you".to_string()
            } else {
                s.reason.clone()
            }
        ));
    }
    for w in &built.workers {
        detected.push(format!("Worker: {}", w.why));
    }
    if mode == DockerizeMode::Prod {
        detected.push("Mode: Production (multi-stage image, no dev tools)".into());
    } else if !built.watch.is_empty() {
        detected.push("Mode: Development (docker compose watch syncs your edits)".into());
    }

    // ---- compose file
    let head = header(&detected);
    let name = slug(project_dir);
    let dev = mode == DockerizeMode::Dev;
    let mut secrets: BTreeSet<String> = BTreeSet::new();
    let mut used_ports: BTreeSet<u16> = BTreeSet::from([host]);

    let chosen: Vec<(&Catalog, &builder::ServicePreset)> = SERVICES
        .iter()
        .filter(|c| services.iter().any(|s| s.id == c.id && s.selected))
        .filter_map(|c| presets.iter().find(|p| p.id == c.preset).map(|p| (c, p)))
        .collect();
    let primary_sql = chosen.iter().find(|(c, _)| c.sql).map(|(c, _)| c.id);

    // The app's environment: its own literals, then what it needs to reach each service.
    let mut env: Vec<(String, String)> = built.env.clone();
    for (cat, _) in &chosen {
        for (k, v) in service_env(cat, built.framework, primary_sql == Some(cat.id)) {
            if !env.iter().any(|(ek, _)| *ek == k) {
                env.push((k, v));
            }
        }
    }
    for key in &built.app_secrets {
        let var = secret_var(&name, key);
        env.push((key.to_string(), secret_ref(&var)));
        secrets.insert(var);
    }
    let depends: Vec<(String, &str)> = chosen
        .iter()
        .map(|(c, _)| {
            (
                c.service.to_string(),
                if healthcheck(c.id).is_some() {
                    "service_healthy"
                } else {
                    "service_started"
                },
            )
        })
        .collect();

    let mut text = format!(
        "{head}services:\n  {name}:\n    build: .\n    image: {}\n    ports:\n      - {}\n",
        // Names the image OLS builds, so it is easy to find on the Images tab.
        yq(&format!("{name}:local")),
        yq(&format!("127.0.0.1:{host}:{}", built.port))
    );
    let env_block = |out: &mut String| {
        if !env.is_empty() {
            out.push_str("    environment:\n");
            for (k, v) in &env {
                out.push_str(&format!("      {k}: {}\n", yq(v)));
            }
        }
    };
    env_block(&mut text);
    let depends_block = |out: &mut String, app: Option<&str>| {
        if app.is_some() || !depends.is_empty() {
            out.push_str("    depends_on:\n");
            if let Some(app) = app {
                out.push_str(&format!(
                    "      {app}:\n        condition: service_started\n"
                ));
            }
            for (svc, cond) in &depends {
                out.push_str(&format!("      {svc}:\n        condition: {cond}\n"));
            }
        }
    };
    depends_block(&mut text, None);
    if dev && !built.watch.is_empty() {
        text.push_str("    develop:\n      watch:\n");
        for w in &built.watch {
            text.push_str(&format!(
                "        - action: {}\n          path: {}\n",
                w.action,
                yq(&w.path)
            ));
            if let Some(t) = &w.target {
                text.push_str(&format!("          target: {}\n", yq(t)));
            }
            if !w.ignore.is_empty() {
                text.push_str("          ignore:\n");
                for i in &w.ignore {
                    text.push_str(&format!("            - {}\n", yq(i)));
                }
            }
        }
    }
    text.push_str("    restart: unless-stopped\n    labels:\n      dev.ols.managed: \"true\"\n    extra_hosts:\n      - \"host.docker.internal:host-gateway\"\n");

    for w in &built.workers {
        let cmd: Vec<String> = w.command.iter().map(|c| yq(c)).collect();
        text.push_str(&format!(
            "  {name}-{}:\n    image: {}\n    pull_policy: never\n    command: [{}]\n",
            w.suffix,
            yq(&format!("{name}:local")),
            cmd.join(", ")
        ));
        env_block(&mut text);
        depends_block(&mut text, Some(&name));
        text.push_str("    restart: unless-stopped\n    labels:\n      dev.ols.managed: \"true\"\n    extra_hosts:\n      - \"host.docker.internal:host-gateway\"\n");
    }

    let mut volumes: Vec<String> = Vec::new();
    for (cat, preset) in &chosen {
        let svc = cat.service;
        text.push_str(&format!(
            "  {svc}:\n    image: {}\n    restart: unless-stopped\n",
            yq(&format!("{}:{}", preset.image, preset.tag))
        ));
        if dev && !preset.ports.is_empty() {
            text.push_str("    ports:\n");
            for pp in &preset.ports {
                let found = (0..200u16)
                    .filter_map(|o| pp.default_host.checked_add(o))
                    .find(|p| !used_ports.contains(p) && is_free(*p))
                    .ok_or_else(|| {
                        bad(
                            "No free port was found.",
                            format!(
                                "Ports {}–{} are all in use.",
                                pp.default_host,
                                pp.default_host.saturating_add(199)
                            ),
                            "Stop something that uses them, or untick the service.",
                        )
                    })?;
                used_ports.insert(found);
                text.push_str(&format!(
                    "      - {}\n",
                    yq(&format!("127.0.0.1:{found}:{}", pp.container))
                ));
            }
        }
        let mut svc_env: Vec<(String, String)> = Vec::new();
        for e in &preset.env {
            match &e.value {
                EnvValue::Literal { value } => svc_env.push((e.key.clone(), value.clone())),
                EnvValue::Secret => {
                    let var = secret_var(svc, &e.key);
                    svc_env.push((e.key.clone(), secret_ref(&var)));
                    secrets.insert(var);
                }
            }
        }
        if cat.id == "adminer" {
            if let Some(sql) = primary_sql.and_then(|id| SERVICES.iter().find(|c| c.id == id)) {
                svc_env.push(("ADMINER_DEFAULT_SERVER".into(), sql.service.into()));
            }
        }
        if !svc_env.is_empty() {
            text.push_str("    environment:\n");
            for (k, v) in &svc_env {
                text.push_str(&format!("      {k}: {}\n", yq(v)));
            }
        }
        if !preset.volumes.is_empty() {
            text.push_str("    volumes:\n");
            for v in &preset.volumes {
                let named = format!("{svc}-{}", v.source);
                volumes.push(named.clone());
                text.push_str(&format!(
                    "      - {}\n",
                    yq(&format!("{named}:{}", v.target))
                ));
            }
        }
        if let Some(test) = healthcheck(cat.id) {
            text.push_str(&format!(
                "    healthcheck:\n      test: {test}\n      interval: 5s\n      timeout: 5s\n      retries: 20\n"
            ));
        }
        text.push_str("    labels:\n      dev.ols.managed: \"true\"\n");
    }
    if !volumes.is_empty() {
        text.push_str("\nvolumes:\n");
        for v in &volumes {
            text.push_str(&format!("  {v}: {{}}\n"));
        }
    }
    let check = compose::check_text(&text);
    if !check.ok {
        return Err(bad(
            "OLS made a compose file that did not pass its own check.",
            check.errors.join(" "),
            "This is a bug in OLS: write the compose file by hand and report the problem.",
        ));
    }

    let dockerfile = (!dockerfile_exists).then(|| format!("{head}{}", built.dockerfile));
    let dockerignore =
        (!exists(project_dir, ".dockerignore")).then(|| dockerignore_text(&built.ignore));
    let compose_name = existing_compose.unwrap_or_else(|| "compose.yaml".to_string());
    Ok(DockerizePlan {
        kind: built.kind.into(),
        mode,
        dockerfile,
        compose: text,
        dockerignore,
        dockerfile_path: project_dir.join("Dockerfile").display().to_string(),
        compose_path: project_dir.join(compose_name).display().to_string(),
        existing,
        port: host,
        notes,
        php_extensions: built.php_extensions,
        unknown_extensions: built.unknown_extensions,
        detected,
        disclaimer: DISCLAIMER.into(),
        checklist: CHECKLIST.iter().map(|s| s.to_string()).collect(),
        services,
        workers: built.workers.iter().map(|w| w.why.clone()).collect(),
        secrets: secrets.into_iter().collect(),
        watch: dev && !built.watch.is_empty(),
    })
}

/// Writes the files. Never overwrites: a Dockerfile that exists is left alone, and a compose
/// file that exists needs `overwrite_compose` (the old one is saved under `backups_dir`).
/// A password is generated for every `OLS_SECRET_*` variable that has none yet.
pub fn write(
    plan: &DockerizePlan,
    overwrite_compose: bool,
    backups_dir: &Path,
    has_secret: &dyn Fn(&str) -> bool,
    store_secret: &dyn Fn(&str, &str) -> Result<(), String>,
) -> Result<(), CoreError> {
    let compose_path = PathBuf::from(&plan.compose_path);
    let compose_exists = std::fs::symlink_metadata(&compose_path).is_ok();
    if compose_exists && !overwrite_compose {
        return Err(bad(
            "This folder already has a compose file.",
            format!("{} exists.", compose_path.display()),
            "Open it from the Compose tab, or choose to replace it (the old file is backed up).",
        ));
    }
    let io = |what: &str, p: &Path, e: std::io::Error| {
        bad(
            "A file could not be written.",
            format!("{what} {}: {e}", p.display()),
            "Check that the project folder is writable.",
        )
    };
    let dir = compose_path.parent().unwrap_or(Path::new("."));
    for var in &plan.secrets {
        let key = compose::secret_key(&compose_path, var);
        if has_secret(&key) {
            // Kept: a database volume made earlier still has the old password.
            continue;
        }
        let value = crate::crypto::random_password().map_err(|e| {
            bad(
                "A password could not be generated.",
                e.to_string(),
                "Try again.",
            )
        })?;
        store_secret(&key, &value).map_err(|e| {
            bad(
                "A password could not be stored.",
                format!("The Windows credential store answered: {e}"),
                "Sign in to Windows normally (not as a temporary profile) and try again.",
            )
        })?;
    }
    if let Some(text) = &plan.dockerfile {
        let p = PathBuf::from(&plan.dockerfile_path);
        // create_new: a Dockerfile that appeared since the preview is never replaced.
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&p)
            .map_err(|e| io("creating", &p, e))?;
        std::io::Write::write_all(&mut f, text.as_bytes()).map_err(|e| io("writing", &p, e))?;
    }
    if compose_exists {
        compose::save_file(&plan.compose_path, &plan.compose, backups_dir)?;
    } else {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&compose_path)
            .map_err(|e| io("creating", &compose_path, e))?;
        std::io::Write::write_all(&mut f, plan.compose.as_bytes())
            .map_err(|e| io("writing", &compose_path, e))?;
    }
    if let Some(text) = &plan.dockerignore {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(".dockerignore"))
        {
            let _ = std::io::Write::write_all(&mut f, text.as_bytes());
        }
    }
    Ok(())
}
