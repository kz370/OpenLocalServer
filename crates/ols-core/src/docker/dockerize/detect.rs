//! What Dockerize can learn from a project folder: environment files, dependency lists, the
//! services a project talks to, and system packages its libraries are known to need.
//!
//! Everything here only *chooses* from closed sets (a service id, a package name from a table).
//! No text from a project file is ever copied into a generated file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::{exists, read_small};

/// Dependency files whose text is searched for known library names.
const DEP_FILES: &[&str] = &[
    "composer.json",
    "composer.lock",
    "requirements.txt",
    "requirements-dev.txt",
    "pyproject.toml",
    "Pipfile",
    "Gemfile",
    "go.mod",
    "Cargo.toml",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
];

/// What was read from the folder once, so each rule does not read files again.
#[derive(Debug, Default)]
pub struct Signals {
    /// `.env.example` then `.env` (the second wins). Keys are upper-case words only.
    pub env: BTreeMap<String, String>,
    /// Lower-cased text of the dependency files above.
    pub blob: String,
    /// Names in `dependencies`, `devDependencies` and `optionalDependencies` of package.json.
    pub node_deps: BTreeSet<String>,
    pub package: Option<serde_json::Value>,
}

pub fn collect(dir: &Path) -> Signals {
    let mut s = Signals::default();
    for file in [".env.example", ".env"] {
        let Some(text) = read_small(dir, file) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let k = k.trim();
            if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            let v = v
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_lowercase();
            s.env.insert(k.to_uppercase(), v);
        }
    }
    for file in DEP_FILES {
        if let Some(text) = read_small(dir, file) {
            s.blob.push_str(&text.to_lowercase());
            s.blob.push('\n');
        }
    }
    if let Some(text) = read_small(dir, "package.json") {
        s.package = serde_json::from_str(&text).ok();
    }
    if let Some(p) = &s.package {
        for section in ["dependencies", "devDependencies", "optionalDependencies"] {
            if let Some(obj) = p.get(section).and_then(|v| v.as_object()) {
                s.node_deps.extend(obj.keys().cloned());
            }
        }
    }
    s
}

impl Signals {
    /// A library named in one of the dependency files (substring match, lower-case).
    pub fn dep(&self, name: &str) -> bool {
        self.blob.contains(name)
    }
    pub fn node(&self, name: &str) -> bool {
        self.node_deps.contains(name)
    }
    pub fn env_is(&self, key: &str, values: &[&str]) -> bool {
        self.env
            .get(key)
            .is_some_and(|v| values.contains(&v.as_str()))
    }
    pub fn env_has(&self, key: &str) -> bool {
        self.env.get(key).is_some_and(|v| !v.is_empty())
    }
    pub fn env_starts(&self, key: &str, prefix: &str) -> bool {
        self.env.get(key).is_some_and(|v| v.starts_with(prefix))
    }
}

/// One service Dockerize can add. `service` is the name used in the compose file.
#[derive(Debug, Clone, Copy)]
pub struct Catalog {
    pub id: &'static str,
    /// The builder preset this service is made from.
    pub preset: &'static str,
    pub name: &'static str,
    pub service: &'static str,
    /// SQL databases compete for the `DB_*` variables: only the first one picked gets them.
    pub sql: bool,
    /// Offered but never ticked by default.
    pub optional: bool,
}

pub const SERVICES: &[Catalog] = &[
    Catalog {
        id: "mariadb",
        preset: "mariadb",
        name: "MariaDB (MySQL-compatible)",
        service: "db",
        sql: true,
        optional: false,
    },
    Catalog {
        id: "postgres",
        preset: "postgres",
        name: "PostgreSQL",
        service: "postgres",
        sql: true,
        optional: false,
    },
    Catalog {
        id: "mongodb",
        preset: "mongodb",
        name: "MongoDB",
        service: "mongo",
        sql: false,
        optional: false,
    },
    Catalog {
        id: "redis",
        preset: "redis",
        name: "Redis",
        service: "redis",
        sql: false,
        optional: false,
    },
    Catalog {
        id: "rabbitmq",
        preset: "rabbitmq",
        name: "RabbitMQ",
        service: "rabbitmq",
        sql: false,
        optional: false,
    },
    Catalog {
        id: "meilisearch",
        preset: "meilisearch",
        name: "Meilisearch",
        service: "search",
        sql: false,
        optional: false,
    },
    Catalog {
        id: "mailpit",
        preset: "mailpit",
        name: "Mailpit (catches outgoing mail)",
        service: "mail",
        sql: false,
        optional: false,
    },
    Catalog {
        id: "adminer",
        preset: "adminer",
        name: "Adminer (database manager)",
        service: "adminer",
        sql: false,
        optional: true,
    },
];

/// The services the project appears to use: `(service id, why)`, strongest evidence first.
pub fn detect_services(dir: &Path, sig: &Signals, wordpress: bool) -> Vec<(&'static str, String)> {
    let mut found: Vec<(&'static str, String)> = Vec::new();
    let mut add = |id: &'static str, why: String| {
        if !found.iter().any(|(i, _)| *i == id) {
            found.push((id, why));
        }
    };

    // --- databases
    if sig.env_is("DB_CONNECTION", &["mysql", "mariadb"]) {
        add("mariadb", "DB_CONNECTION in .env".into());
    }
    if sig.env_is("DB_CONNECTION", &["pgsql", "postgres", "postgresql"]) {
        add("postgres", "DB_CONNECTION in .env".into());
    }
    for key in ["DATABASE_URL", "DB_URL"] {
        if sig.env_starts(key, "mysql") || sig.env_starts(key, "mariadb") {
            add("mariadb", format!("{key} in .env"));
        }
        if sig.env_starts(key, "postgres") {
            add("postgres", format!("{key} in .env"));
        }
        if sig.env_starts(key, "mongodb") {
            add("mongodb", format!("{key} in .env"));
        }
    }
    for key in ["MONGODB_URI", "MONGO_URL", "MONGO_URI"] {
        if sig.env_has(key) {
            add("mongodb", format!("{key} in .env"));
        }
    }
    if let Some(schema) = read_small(dir, "prisma/schema.prisma") {
        let schema = schema.to_lowercase().replace(' ', "");
        if schema.contains("provider=\"postgresql\"") || schema.contains("provider=\"postgres\"") {
            add("postgres", "prisma/schema.prisma".into());
        }
        if schema.contains("provider=\"mysql\"") {
            add("mariadb", "prisma/schema.prisma".into());
        }
        if schema.contains("provider=\"mongodb\"") {
            add("mongodb", "prisma/schema.prisma".into());
        }
    }
    let node_any = |names: &[&str]| names.iter().any(|n| sig.node(n));
    if node_any(&["pg", "postgres", "pg-promise", "@prisma/adapter-pg"])
        || [
            "psycopg",
            "asyncpg",
            "lib/pq",
            "jackc/pgx",
            "'pg'",
            "\"pg\"",
        ]
        .iter()
        .any(|n| sig.dep(n))
    {
        add(
            "postgres",
            "a PostgreSQL library in your dependencies".into(),
        );
    }
    if node_any(&["mysql2", "mysql"])
        || [
            "mysqlclient",
            "pymysql",
            "mysql-connector",
            "go-sql-driver/mysql",
            "mysql2",
        ]
        .iter()
        .any(|n| sig.dep(n))
    {
        add("mariadb", "a MySQL library in your dependencies".into());
    }
    if node_any(&["mongoose", "mongodb"])
        || ["pymongo", "mongoengine", "mongo-driver", "mongodb/mongodb"]
            .iter()
            .any(|n| sig.dep(n))
    {
        add("mongodb", "a MongoDB library in your dependencies".into());
    }
    if wordpress {
        add(
            "mariadb",
            "WordPress needs a MySQL-compatible database".into(),
        );
    }

    // --- cache and queues
    for key in [
        "CACHE_DRIVER",
        "CACHE_STORE",
        "SESSION_DRIVER",
        "QUEUE_CONNECTION",
    ] {
        if sig.env_is(key, &["redis"]) {
            add("redis", format!("{key}=redis in .env"));
        }
    }
    if sig.env_has("REDIS_HOST") || sig.env_has("REDIS_URL") {
        add("redis", "REDIS_HOST or REDIS_URL in .env".into());
    }
    if node_any(&["redis", "ioredis", "bullmq", "bull"])
        || [
            "django-redis",
            "go-redis",
            "redigo",
            "predis/predis",
            "'redis'",
            "\"redis\"",
            "redis==",
            "redis>",
            "redis\n",
            "sidekiq",
        ]
        .iter()
        .any(|n| sig.dep(n))
    {
        add("redis", "a Redis library in your dependencies".into());
    }
    if sig.env_is("QUEUE_CONNECTION", &["rabbitmq"])
        || sig.env_has("RABBITMQ_HOST")
        || sig.env_starts("AMQP_URL", "amqp")
        || sig.env_starts("CELERY_BROKER_URL", "amqp")
        || node_any(&["amqplib", "amqp-connection-manager"])
        || ["pika", "kombu", "bunny", "streadway/amqp"]
            .iter()
            .any(|n| sig.dep(n))
    {
        add("rabbitmq", "a RabbitMQ setting or library".into());
    }

    // --- search and mail
    if sig.env_is("SCOUT_DRIVER", &["meilisearch"])
        || sig.env_has("MEILISEARCH_HOST")
        || sig.env_has("MEILI_HOST")
        || sig.node("meilisearch")
        || sig.dep("meilisearch")
    {
        add("meilisearch", "a Meilisearch setting or library".into());
    }
    if sig.env_is("MAIL_MAILER", &["smtp"])
        || sig.env_has("MAIL_HOST")
        || sig.env_has("SMTP_HOST")
        || sig.env_has("EMAIL_HOST")
        || sig.node("nodemailer")
    {
        add("mailpit", "outgoing mail is configured".into());
    }
    found
}

/// System packages a library is known to need. `(library text, packages)` for each family.
/// Only names from these tables ever reach a generated `apt-get` / `apk` line.
const APT_PYTHON: &[(&str, &[&str])] = &[
    (
        "mysqlclient",
        &[
            "build-essential",
            "default-libmysqlclient-dev",
            "pkg-config",
        ],
    ),
    ("opencv-python", &["libgl1", "libglib2.0-0"]),
    ("weasyprint", &["libpango-1.0-0", "libpangoft2-1.0-0"]),
    ("pdf2image", &["poppler-utils"]),
    ("pytesseract", &["tesseract-ocr"]),
    ("ffmpeg-python", &["ffmpeg"]),
    ("moviepy", &["ffmpeg"]),
    ("pydub", &["ffmpeg"]),
];
const APT_PHP: &[(&str, &[&str])] = &[
    ("ffmpeg", &["ffmpeg"]),
    ("tesseract", &["tesseract-ocr"]),
    ("poppler", &["poppler-utils"]),
    ("browsershot", &["chromium"]),
];
const APK_NODE: &[(&str, &[&str])] = &[
    ("puppeteer", &["chromium"]),
    (
        "canvas",
        &[
            "build-base",
            "cairo-dev",
            "pango-dev",
            "jpeg-dev",
            "giflib-dev",
            "librsvg-dev",
        ],
    ),
    ("bcrypt", &["python3", "make", "g++"]),
    ("better-sqlite3", &["python3", "make", "g++"]),
    ("sqlite3", &["python3", "make", "g++"]),
    ("argon2", &["python3", "make", "g++"]),
    ("node-sass", &["python3", "make", "g++"]),
    ("fluent-ffmpeg", &["ffmpeg"]),
];

fn pick(table: &[(&str, &[&str])], hit: impl Fn(&str) -> bool) -> Vec<String> {
    let mut out = BTreeSet::new();
    for (lib, pkgs) in table {
        if hit(lib) {
            out.extend(pkgs.iter().map(|p| p.to_string()));
        }
    }
    out.into_iter().collect()
}

/// apt packages for a Debian-based image (Python, PHP, Ruby, Java).
pub fn apt_packages(kind: &str, sig: &Signals) -> Vec<String> {
    let mut pkgs: BTreeSet<String> = match kind {
        "python" => pick(APT_PYTHON, |l| sig.dep(l)).into_iter().collect(),
        "php" => pick(APT_PHP, |l| sig.dep(l)).into_iter().collect(),
        _ => BTreeSet::new(),
    };
    if kind == "python" {
        // psycopg2 (not the -binary build) compiles against libpq.
        let blob = &sig.blob;
        let source_psycopg = blob
            .match_indices("psycopg2")
            .any(|(i, _)| !blob[i..].starts_with("psycopg2-binary"));
        if source_psycopg {
            pkgs.extend(["gcc", "libpq-dev"].map(String::from));
        }
    }
    if kind == "ruby" {
        if sig.dep("mysql2") {
            pkgs.insert("default-libmysqlclient-dev".into());
        }
        if sig.dep("'pg'") || sig.dep("\"pg\"") {
            pkgs.insert("libpq-dev".into());
        }
    }
    pkgs.into_iter().collect()
}

/// apk packages for an Alpine image (Node).
pub fn apk_packages(sig: &Signals) -> Vec<String> {
    pick(APK_NODE, |l| sig.node(l))
}

pub fn has_dir(dir: &Path, name: &str) -> bool {
    std::fs::symlink_metadata(dir.join(name))
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false)
}

/// Folders where a recognisable project might sit when the top of the folder has none.
pub const SUBFOLDERS: &[&str] = &[
    "backend", "server", "api", "app", "web", "frontend", "client",
];

pub fn marker_in(dir: &Path) -> bool {
    [
        "composer.json",
        "package.json",
        "go.mod",
        "Cargo.toml",
        "Gemfile",
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "requirements.txt",
        "pyproject.toml",
        "index.html",
    ]
    .iter()
    .any(|f| exists(dir, f))
}
