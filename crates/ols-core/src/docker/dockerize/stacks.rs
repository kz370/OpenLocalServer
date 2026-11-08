//! One builder per kind of project. Each returns a `Built`: the Dockerfile text plus what the
//! compose file needs (port, environment, watch rules, worker processes).
//!
//! Every word that ends up in a generated file is either a literal written here, a version
//! number, or an identifier that passed `ident()` (letters, digits, `_`, `-`, `.`).

use std::collections::BTreeSet;
use std::path::Path;

use super::detect::{self, Signals};
use super::{exists, first_number, read_small, Built, DockerizeMode, Framework, Watch, Worker};

const PROD: fn(DockerizeMode) -> bool = |m| m == DockerizeMode::Prod;

/// A name that is safe to write into a Dockerfile or a command.
pub(super) fn ident(s: &str) -> Option<String> {
    (!s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
    .then(|| s.to_string())
}

fn json_cmd(parts: &[&str]) -> String {
    let items: Vec<String> = parts
        .iter()
        .map(|p| format!("\"{}\"", p.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!("[{}]", items.join(", "))
}

fn cmd_vec(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn apt_line(pkgs: &[String]) -> String {
    if pkgs.is_empty() {
        return String::new();
    }
    format!(
        "RUN apt-get update && apt-get install -y --no-install-recommends {} && rm -rf /var/lib/apt/lists/*\n",
        pkgs.join(" ")
    )
}

fn apk_line(pkgs: &[String]) -> String {
    if pkgs.is_empty() {
        return String::new();
    }
    format!("RUN apk add --no-cache {}\n", pkgs.join(" "))
}

fn present<'a>(dir: &Path, names: &[&'a str]) -> Vec<&'a str> {
    names.iter().copied().filter(|n| exists(dir, n)).collect()
}

fn watch_sync(target: &str, ignore: &[&str]) -> Watch {
    Watch {
        action: "sync",
        path: ".".into(),
        target: Some(target.into()),
        ignore: ignore.iter().map(|s| s.to_string()).collect(),
    }
}

fn watch_rebuild(path: &str, ignore: &[&str]) -> Watch {
    Watch {
        action: "rebuild",
        path: path.into(),
        target: None,
        ignore: ignore.iter().map(|s| s.to_string()).collect(),
    }
}

fn rebuild_on(dir: &Path, files: &[&str]) -> Vec<Watch> {
    present(dir, files)
        .into_iter()
        .map(|f| watch_rebuild(f, &[]))
        .collect()
}

fn blank(kind: &'static str) -> Built {
    Built {
        kind,
        framework: Framework::Generic,
        dockerfile: String::new(),
        port: 8080,
        env: vec![],
        app_secrets: vec![],
        watch: vec![],
        workers: vec![],
        ignore: vec![],
        detected: vec![],
        notes: vec![],
        php_extensions: vec![],
        unknown_extensions: vec![],
    }
}

// ------------------------------------------------------------------ node

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pm {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl Pm {
    fn name(self) -> &'static str {
        match self {
            Pm::Npm => "npm",
            Pm::Pnpm => "pnpm",
            Pm::Yarn => "yarn",
            Pm::Bun => "bun",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NodeFw {
    Next,
    Nuxt,
    SvelteKit,
    Remix,
    Astro,
    Angular,
    Nest,
    Vite,
    Generic,
}

impl NodeFw {
    fn label(self) -> &'static str {
        match self {
            NodeFw::Next => "Next.js",
            NodeFw::Nuxt => "Nuxt",
            NodeFw::SvelteKit => "SvelteKit",
            NodeFw::Remix => "Remix",
            NodeFw::Astro => "Astro",
            NodeFw::Angular => "Angular",
            NodeFw::Nest => "NestJS",
            NodeFw::Vite => "Vite",
            NodeFw::Generic => "Node.js",
        }
    }
    /// Dev-server arguments that make it listen on all interfaces, and its usual port.
    fn dev(self) -> (&'static [&'static str], u16) {
        match self {
            NodeFw::Next => (&["-H", "0.0.0.0"], 3000),
            NodeFw::Nuxt => (&["--host", "0.0.0.0"], 3000),
            NodeFw::SvelteKit | NodeFw::Remix | NodeFw::Vite => (&["--host", "0.0.0.0"], 5173),
            NodeFw::Astro => (&["--host", "0.0.0.0"], 4321),
            NodeFw::Angular => (&["--host", "0.0.0.0"], 4200),
            NodeFw::Nest | NodeFw::Generic => (&[], 3000),
        }
    }
}

fn node_framework(sig: &Signals) -> NodeFw {
    if sig.node("next") {
        NodeFw::Next
    } else if sig.node("nuxt") {
        NodeFw::Nuxt
    } else if sig.node("@sveltejs/kit") {
        NodeFw::SvelteKit
    } else if sig.node("astro") {
        NodeFw::Astro
    } else if sig.node("@remix-run/dev") || sig.node("@remix-run/node") || sig.node("react-router")
    {
        NodeFw::Remix
    } else if sig.node("@angular/core") {
        NodeFw::Angular
    } else if sig.node("@nestjs/core") {
        NodeFw::Nest
    } else if sig.node("vite") {
        NodeFw::Vite
    } else {
        NodeFw::Generic
    }
}

fn node_major(dir: &Path, sig: &Signals) -> (u32, &'static str) {
    let from_file = |name: &str, label: &'static str| {
        read_small(dir, name)
            .and_then(|t| first_number(&t))
            .map(|(m, _)| (m, label))
    };
    let tool_versions = read_small(dir, ".tool-versions").and_then(|t| {
        t.lines()
            .find(|l| l.starts_with("nodejs ") || l.starts_with("node "))
            .and_then(first_number)
            .map(|(m, _)| (m, "from .tool-versions"))
    });
    let volta = sig
        .package
        .as_ref()
        .and_then(|p| p.pointer("/volta/node"))
        .and_then(|v| v.as_str())
        .and_then(first_number)
        .map(|(m, _)| (m, "from package.json volta.node"));
    let engines = sig
        .package
        .as_ref()
        .and_then(|p| p.pointer("/engines/node"))
        .and_then(|v| v.as_str())
        .and_then(first_number)
        .map(|(m, _)| (m, "from package.json engines.node"));
    let (major, src) = from_file(".nvmrc", "from .nvmrc")
        .or_else(|| from_file(".node-version", "from .node-version"))
        .or(tool_versions)
        .or(volta)
        .or(engines)
        .unwrap_or((
            22,
            "default: no .nvmrc, .node-version or engines.node found",
        ));
    (major.clamp(18, 24), src)
}

fn node_pm(dir: &Path, sig: &Signals) -> (Pm, Option<&'static str>) {
    let declared = sig
        .package
        .as_ref()
        .and_then(|p| p.get("packageManager"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if exists(dir, "bun.lockb") || exists(dir, "bun.lock") || declared.starts_with("bun@") {
        let lock = if exists(dir, "bun.lock") {
            Some("bun.lock")
        } else if exists(dir, "bun.lockb") {
            Some("bun.lockb")
        } else {
            None
        };
        (Pm::Bun, lock)
    } else if exists(dir, "pnpm-lock.yaml") || declared.starts_with("pnpm@") {
        (
            Pm::Pnpm,
            exists(dir, "pnpm-lock.yaml").then_some("pnpm-lock.yaml"),
        )
    } else if exists(dir, "yarn.lock") || declared.starts_with("yarn@") {
        (Pm::Yarn, exists(dir, "yarn.lock").then_some("yarn.lock"))
    } else if exists(dir, "package-lock.json") {
        (Pm::Npm, Some("package-lock.json"))
    } else if exists(dir, "npm-shrinkwrap.json") {
        (Pm::Npm, Some("npm-shrinkwrap.json"))
    } else {
        (Pm::Npm, None)
    }
}

/// `<pm> run <script> -- <args>` in exec form.
fn run_script(pm: Pm, script: &str, args: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = match pm {
        Pm::Npm => vec!["npm".into(), "run".into(), script.into()],
        Pm::Pnpm => vec!["pnpm".into(), "run".into(), script.into()],
        Pm::Yarn => vec!["yarn".into(), script.into()],
        Pm::Bun => vec!["bun".into(), "run".into(), script.into()],
    };
    if !args.is_empty() {
        if pm == Pm::Npm {
            v.push("--".into());
        }
        v.extend(args.iter().map(|s| s.to_string()));
    }
    v
}

fn cmd_text(parts: &[String]) -> String {
    json_cmd(&parts.iter().map(String::as_str).collect::<Vec<_>>())
}

/// The lines that copy the manifests and install dependencies (cached between builds).
fn node_install(dir: &Path, pm: Pm, lock: Option<&str>, prod_deps_only: bool) -> (String, bool) {
    let workspaces = sig_workspaces(dir);
    let berry = exists(dir, ".yarnrc.yml");
    let mut s = String::new();
    if matches!(pm, Pm::Pnpm) || (pm == Pm::Yarn && berry) {
        s.push_str("RUN corepack enable\n");
    }
    let install = match (pm, lock) {
        (Pm::Npm, Some(_)) if prod_deps_only => "npm ci --omit=dev",
        (Pm::Npm, Some(_)) => "npm ci",
        (Pm::Npm, None) => "npm install",
        (Pm::Pnpm, Some(_)) => "pnpm install --frozen-lockfile",
        (Pm::Pnpm, None) => "pnpm install",
        (Pm::Yarn, Some(_)) if berry => "yarn install --immutable",
        (Pm::Yarn, Some(_)) => "yarn install --frozen-lockfile",
        (Pm::Yarn, None) => "yarn install",
        (Pm::Bun, Some(_)) => "bun install --frozen-lockfile",
        (Pm::Bun, None) => "bun install",
    };
    let cache = match pm {
        Pm::Npm => "/root/.npm",
        Pm::Pnpm => "/root/.local/share/pnpm/store",
        Pm::Yarn => "/usr/local/share/.cache/yarn",
        Pm::Bun => "/root/.bun/install/cache",
    };
    if workspaces {
        // Workspace packages need their own manifests: copy everything, then install.
        s.push_str("COPY . .\n");
    } else {
        let mut files = vec!["package.json"];
        files.extend(present(
            dir,
            &[
                "package-lock.json",
                "npm-shrinkwrap.json",
                "pnpm-lock.yaml",
                "yarn.lock",
                "bun.lockb",
                "bun.lock",
                ".npmrc",
                ".yarnrc.yml",
            ],
        ));
        s.push_str(&format!("COPY {} ./\n", files.join(" ")));
        if detect::has_dir(dir, "prisma") {
            s.push_str("COPY prisma ./prisma\n");
        }
    }
    s.push_str(&format!(
        "RUN --mount=type=cache,target={cache} {install}\n"
    ));
    if !workspaces {
        s.push_str("COPY . .\n");
    }
    (s, workspaces)
}

fn sig_workspaces(dir: &Path) -> bool {
    exists(dir, "pnpm-workspace.yaml")
        || read_small(dir, "package.json").is_some_and(|t| t.contains("\"workspaces\""))
}

pub(super) fn node(dir: &Path, sig: &Signals, mode: DockerizeMode) -> Built {
    let mut b = blank("node");
    b.detected
        .push("Project type: Node.js (found package.json)".into());
    if sig.package.is_none() {
        b.notes
            .push("package.json could not be read as JSON: defaults were used.".into());
    }
    let (major, src) = node_major(dir, sig);
    let (pm, lock) = node_pm(dir, sig);
    let fw = node_framework(sig);
    b.framework = Framework::Generic;
    if fw != NodeFw::Generic {
        b.detected.push(format!(
            "Framework: {} (found in package.json dependencies)",
            fw.label()
        ));
    }
    let image = if pm == Pm::Bun {
        b.detected
            .push("Runtime: Bun (found bun.lock or bun.lockb)".into());
        "oven/bun:1-alpine".to_string()
    } else {
        b.detected.push(format!("Node {major} ({src})"));
        format!("node:{major}-alpine")
    };
    if pm != Pm::Bun {
        b.detected.push(format!(
            "Package manager: {} ({})",
            pm.name(),
            lock.map_or("default: no lockfile found".to_string(), |l| format!(
                "found {l}"
            ))
        ));
    }
    let has = |s: &str| {
        sig.package
            .as_ref()
            .and_then(|p| p.pointer(&format!("/scripts/{s}")))
            .is_some()
    };
    let apk = detect::apk_packages(sig);
    if !apk.is_empty() {
        b.detected.push(format!(
            "System packages: {} (needed by libraries in package.json)",
            apk.join(", ")
        ));
    }
    if sig.node("puppeteer") {
        b.env
            .push(("PUPPETEER_SKIP_DOWNLOAD".into(), "true".into()));
        b.env.push((
            "PUPPETEER_EXECUTABLE_PATH".into(),
            "/usr/bin/chromium".into(),
        ));
    }
    let (dev_args, dev_port) = fw.dev();
    let alpine = pm != Pm::Bun || image.contains("alpine");

    let dev_script = if fw == NodeFw::Nest && has("start:dev") {
        Some("start:dev")
    } else if has("dev") {
        Some("dev")
    } else if has("start") {
        Some("start")
    } else {
        None
    };
    let (install, workspaces) = node_install(dir, pm, lock, false);
    if workspaces {
        b.notes.push("This is a monorepo (workspaces): the whole folder is copied before installing, so dependency caching is weaker.".into());
    }
    let apk_text = if alpine {
        apk_line(&apk)
    } else {
        String::new()
    };

    // Prod: a plain Vite site is built and served by nginx; everything else runs its start script.
    if PROD(mode) {
        let vite_site = fw == NodeFw::Vite && has("build");
        let build_step = if has("build") {
            format!("RUN {}\n", run_script(pm, "build", &[]).join(" "))
        } else {
            String::new()
        };
        if !has("build") {
            b.notes.push(
                "No build script found in package.json: nothing is built for production.".into(),
            );
        }
        if vite_site {
            b.port = 80;
            b.detected
                .push("Production: static build served by nginx (Vite site)".into());
            b.notes.push("The Vite build is expected in dist/ (the Vite default). Single-page apps that use client-side routes need an nginx rule for index.html: add one if a refresh gives 404.".into());
            b.dockerfile = format!(
                "FROM {image} AS build\nWORKDIR /app\n{apk_text}{install}{build_step}\nFROM nginx:1.28-alpine\nCOPY --from=build /app/dist /usr/share/nginx/html\nEXPOSE 80\n"
            );
            b.ignore = vec!["node_modules", ".next", ".nuxt", ".output", "coverage"];
            return b;
        }
        b.port = 3000;
        let prune = match pm {
            Pm::Npm => "RUN npm prune --omit=dev\n",
            Pm::Pnpm => "RUN pnpm prune --prod\n",
            _ => "",
        };
        let start: Vec<String> = if fw == NodeFw::Nuxt {
            cmd_vec(&["node", ".output/server/index.mjs"])
        } else if has("start") {
            run_script(pm, "start", &[])
        } else if pm == Pm::Bun {
            cmd_vec(&["bun", "."])
        } else {
            b.notes
                .push("No start script found in package.json: the container runs `node .`.".into());
            cmd_vec(&["node", "."])
        };
        b.detected
            .push(format!("Start command: {} (production)", start.join(" ")));
        b.detected
            .push("Port: 3000 (guessed: set through the PORT variable)".into());
        b.env.push(("NODE_ENV".into(), "production".into()));
        b.env.push(("PORT".into(), "3000".into()));
        b.env.push(("HOST".into(), "0.0.0.0".into()));
        let user = if pm == Pm::Bun { "bun" } else { "node" };
        b.dockerfile = format!(
            "FROM {image} AS build\nWORKDIR /app\n{apk_text}{install}{build_step}{prune}\nFROM {image}\nWORKDIR /app\n{apk_text}ENV NODE_ENV=production\nCOPY --from=build --chown={user}:{user} /app /app\nUSER {user}\nEXPOSE 3000\nCMD {}\n",
            cmd_text(&start)
        );
        b.ignore = vec!["node_modules", ".next", ".nuxt", ".output", "coverage"];
        return b;
    }

    // Dev.
    let cmd: Vec<String> = match dev_script {
        Some(s) => {
            b.detected.push(format!(
                "Start command: {} (from package.json scripts)",
                run_script(pm, s, dev_args).join(" ")
            ));
            run_script(pm, s, dev_args)
        }
        None => {
            b.detected
                .push("Start command: node . (default: no dev or start script found)".into());
            b.notes.push(
                "No dev or start script found in package.json: the container runs `node .`.".into(),
            );
            if pm == Pm::Bun {
                cmd_vec(&["bun", "."])
            } else {
                cmd_vec(&["node", "."])
            }
        }
    };
    if dev_args.is_empty() && dev_script == Some("dev") {
        b.notes.push("Dev servers such as Vite listen on localhost only unless started with --host: set that in your dev script if the page does not load.".into());
    }
    b.port = dev_port;
    b.detected.push(if fw == NodeFw::Generic {
        "Port: 3000 (guessed: Node apps commonly use 3000, change it if yours differs)".into()
    } else {
        format!("Port: {dev_port} (the usual port for {})", fw.label())
    });
    b.env.push(("NODE_ENV".into(), "development".into()));
    b.env.push(("HOST".into(), "0.0.0.0".into()));
    b.dockerfile = format!(
        "FROM {image}\nWORKDIR /app\n{apk_text}{install}EXPOSE {dev_port}\nCMD {}\n",
        cmd_text(&cmd)
    );
    let ignored = [
        "node_modules/",
        ".git/",
        ".next/",
        ".nuxt/",
        ".svelte-kit/",
        ".output/",
        "dist/",
    ];
    b.watch.push(watch_sync("/app", &ignored));
    b.watch.extend(rebuild_on(
        dir,
        &[
            "package.json",
            "package-lock.json",
            "pnpm-lock.yaml",
            "yarn.lock",
            "bun.lockb",
            "bun.lock",
        ],
    ));
    b.ignore = vec![
        "node_modules",
        ".next",
        ".nuxt",
        ".output",
        ".svelte-kit",
        "coverage",
    ];
    b
}

// ------------------------------------------------------------------ php

/// Extensions OLS may install. Anything else is reported, never written to a file.
pub(super) fn php(
    dir: &Path,
    sig: &Signals,
    mode: DockerizeMode,
    laravel: bool,
    wordpress: bool,
) -> Built {
    let mut b = blank("php");
    let prod = PROD(mode);
    let symfony = exists(dir, "bin/console") && sig.dep("symfony/");
    b.framework = if laravel {
        Framework::Laravel
    } else if wordpress {
        Framework::WordPress
    } else {
        Framework::Generic
    };
    b.detected.push(if laravel {
        "Project type: Laravel (found artisan and composer.json)".to_string()
    } else if wordpress {
        "Project type: WordPress (found wp-config.php, wp-load.php or wp-content)".to_string()
    } else if symfony {
        "Project type: Symfony (found bin/console)".to_string()
    } else {
        "Project type: PHP (found composer.json)".to_string()
    });
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
            b.detected
                .push(format!("PHP 8.{m} (from composer.json require.php)"));
            m
        }
        None => {
            b.detected.push(
                "PHP 8.3 (default: composer.json has no usable \"php\" requirement)".to_string(),
            );
            3
        }
    };

    let (declared, guessed, invalid) = super::needed_php_extensions(dir, sig, laravel, wordpress);
    let mut unknown: BTreeSet<String> = BTreeSet::new();
    let mut chosen: BTreeSet<String> = BTreeSet::new();
    for name in declared.iter().chain(guessed.iter()) {
        if super::PHP_BUILTIN.contains(&name.as_str()) {
            continue;
        }
        if super::PHP_EXT_ALLOW.contains(&name.as_str()) {
            chosen.insert(name.clone());
        } else {
            unknown.insert(name.clone());
        }
    }
    if prod {
        chosen.insert("opcache".into());
    }
    if invalid {
        unknown.insert("invalid-name".into());
    }
    for name in &unknown {
        b.notes.push(format!(
            "Needs the PHP extension {name}, which OLS cannot install automatically: add it to the Dockerfile yourself."
        ));
    }
    let guessed_only: Vec<&String> = chosen
        .iter()
        .filter(|n| !declared.contains(*n) && !(prod && n.as_str() == "opcache"))
        .collect();
    if !guessed_only.is_empty() {
        b.detected.push(format!(
            "PHP extensions {} (guessed from your .env / project type)",
            guessed_only
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        b.notes.push(
            "Some PHP extensions were guessed from your .env / project type: check the list."
                .into(),
        );
    }
    let declared_chosen: Vec<&String> = chosen.iter().filter(|n| declared.contains(*n)).collect();
    if !declared_chosen.is_empty() {
        b.detected.push(format!(
            "PHP extensions {} (declared in composer.json / composer.lock)",
            declared_chosen
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let docroot = if laravel || symfony || exists(dir, "public/index.php") {
        Some("public")
    } else if sig.dep("cakephp/cakephp") {
        Some("webroot")
    } else {
        None
    };
    b.detected.push("Port: 80 (Apache in the php image)".into());

    let has_composer = composer.is_some();
    let mut apt = detect::apt_packages("php", sig);
    if has_composer {
        for p in ["git", "unzip"] {
            if !apt.iter().any(|x| x == p) {
                apt.push(p.into());
            }
        }
    }
    if !apt.is_empty() {
        b.detected.push(format!(
            "System packages: {} (needed by libraries in composer.json)",
            apt.join(", ")
        ));
    }

    let mut df = String::new();
    // Laravel with Vite: build the assets in their own stage (production only).
    let vite_assets = laravel
        && prod
        && exists(dir, "package.json")
        && (sig.node("vite") || sig.node("laravel-vite-plugin"))
        && sig
            .package
            .as_ref()
            .and_then(|p| p.pointer("/scripts/build"))
            .is_some();
    if vite_assets {
        let (pm, lock) = node_pm(dir, sig);
        let (major, _) = node_major(dir, sig);
        let (install, _) = node_install(dir, pm, lock, false);
        df.push_str(&format!(
            "FROM node:{major}-alpine AS assets\nWORKDIR /app\n{install}RUN {}\n\n",
            run_script(pm, "build", &[]).join(" ")
        ));
        b.detected
            .push("Assets: Vite build in a Node stage (found vite in package.json)".into());
    } else if laravel && exists(dir, "package.json") && !prod {
        b.notes.push("This Laravel project has front-end assets: run `npm run dev` on your computer (or add a Node service) for hot reload.".into());
    }
    df.push_str(&format!("FROM php:8.{minor_value}-apache\n"));
    df.push_str(&apt_line(&apt));
    let extensions: Vec<String> = chosen.into_iter().collect();
    if !extensions.is_empty() {
        df.push_str(&format!(
            "COPY --from=ghcr.io/mlocati/php-extension-installer:{} /usr/bin/install-php-extensions /usr/local/bin/\nRUN install-php-extensions {}\n",
            super::PHP_EXT_INSTALLER,
            extensions.join(" ")
        ));
    }
    if has_composer {
        df.push_str("COPY --from=composer:2 /usr/bin/composer /usr/bin/composer\nENV COMPOSER_CACHE_DIR=/tmp/composer-cache\n");
    }
    if let Some(root) = docroot {
        df.push_str(&format!(
            "ENV APACHE_DOCUMENT_ROOT /var/www/html/{root}\nRUN sed -ri -e 's!/var/www/html!${{APACHE_DOCUMENT_ROOT}}!g' /etc/apache2/sites-available/*.conf && sed -ri -e 's!/var/www/!${{APACHE_DOCUMENT_ROOT}}!g' /etc/apache2/apache2.conf /etc/apache2/conf-available/*.conf\n"
        ));
        b.detected.push(format!(
            "Web root: {root}/ (found {})",
            if laravel { "artisan" } else { "its index.php" }
        ));
    }
    df.push_str("RUN a2enmod rewrite && sed -ri -e 's/AllowOverride None/AllowOverride All/' /etc/apache2/apache2.conf && echo 'ServerName localhost' > /etc/apache2/conf-available/servername.conf && a2enconf servername\n");
    df.push_str(&format!(
        "RUN mv \"$PHP_INI_DIR/php.ini-{}\" \"$PHP_INI_DIR/php.ini\"\n",
        if prod { "production" } else { "development" }
    ));
    df.push_str("WORKDIR /var/www/html\n");
    let no_dev = if prod { " --no-dev" } else { "" };
    if has_composer {
        df.push_str(&format!(
            "COPY composer.json composer.lock* ./\nRUN --mount=type=cache,target=/tmp/composer-cache composer install --no-interaction --no-scripts --no-autoloader{no_dev}\n"
        ));
    }
    df.push_str(if prod {
        "COPY --chown=www-data:www-data . /var/www/html\n"
    } else {
        "COPY . /var/www/html\n"
    });
    if vite_assets {
        df.push_str("COPY --from=assets --chown=www-data:www-data /app/public/build /var/www/html/public/build\n");
    }
    if has_composer {
        df.push_str(&format!(
            "RUN composer dump-autoload --optimize --no-scripts{no_dev}\n"
        ));
    }
    if laravel {
        df.push_str("RUN mkdir -p storage/framework/cache storage/framework/sessions storage/framework/views storage/logs bootstrap/cache && chown -R www-data:www-data storage bootstrap/cache\n");
        b.env.push((
            "APP_ENV".into(),
            if prod { "production" } else { "local" }.into(),
        ));
        b.env.push((
            "APP_DEBUG".into(),
            if prod { "false" } else { "true" }.into(),
        ));
        b.notes.push("Laravel needs APP_KEY: your .env is not copied into the image. Run `php artisan key:generate --show` and pass the value as the APP_KEY environment variable.".into());
        // Workers share the app image.
        let queue = ["QUEUE_CONNECTION"]
            .iter()
            .any(|k| sig.env_is(k, &["redis", "database", "sqs", "beanstalkd", "rabbitmq"]));
        if queue {
            b.workers.push(Worker {
                suffix: "queue",
                command: cmd_vec(&["php", "artisan", "queue:work", "--tries=3"]),
                why: "queue worker (QUEUE_CONNECTION in .env is not sync)".into(),
            });
        }
        if exists(dir, "app/Console/Kernel.php") || exists(dir, "routes/console.php") {
            b.workers.push(Worker {
                suffix: "scheduler",
                command: cmd_vec(&["php", "artisan", "schedule:work"]),
                why: "scheduler (found a console schedule)".into(),
            });
        }
    }
    if wordpress {
        b.notes.push("WordPress reads its database settings from wp-config.php: the compose file passes WORDPRESS_DB_* variables, so make wp-config.php use getenv() for them.".into());
    }
    if !has_composer && !wordpress {
        b.notes
            .push("No composer.json found: no PHP dependencies are installed.".into());
    }
    df.push_str("EXPOSE 80\n");
    b.dockerfile = df;
    b.port = 80;
    b.php_extensions = extensions;
    b.unknown_extensions = unknown.into_iter().collect();
    if !prod {
        b.watch.push(watch_sync(
            "/var/www/html",
            &["vendor/", "node_modules/", ".git/", "storage/logs/"],
        ));
        b.watch
            .extend(rebuild_on(dir, &["composer.json", "composer.lock"]));
    }
    b.ignore = vec![
        "vendor",
        "node_modules",
        "storage/logs",
        "storage/framework/cache",
    ];
    b
}

// ------------------------------------------------------------------ python

fn python_minor(dir: &Path) -> (u32, &'static str) {
    let tv = read_small(dir, ".tool-versions").and_then(|t| {
        t.lines()
            .find(|l| l.starts_with("python "))
            .and_then(first_number)
    });
    let pyproject = read_small(dir, "pyproject.toml").and_then(|t| {
        t.lines()
            .find(|l| {
                l.trim_start().starts_with("requires-python")
                    || l.trim_start().starts_with("python =")
            })
            .and_then(|l| l.split_once('=').map(|x| x.1.to_string()))
            .and_then(|v| first_number(&v))
    });
    let found = read_small(dir, ".python-version")
        .and_then(|t| first_number(&t))
        .map(|v| (v, "from .python-version"))
        .or_else(|| {
            read_small(dir, "runtime.txt")
                .and_then(|t| first_number(&t))
                .map(|v| (v, "from runtime.txt"))
        })
        .or(tv.map(|v| (v, "from .tool-versions")))
        .or(pyproject.map(|v| (v, "from pyproject.toml")));
    match found {
        Some(((3, Some(m)), src)) => (m.clamp(9, 13), src),
        _ => (12, "default: no usable version file found"),
    }
}

fn first_file<'a>(dir: &Path, names: &[&'a str]) -> Option<&'a str> {
    names.iter().copied().find(|n| exists(dir, n))
}

pub(super) fn python(dir: &Path, sig: &Signals, mode: DockerizeMode) -> Built {
    let mut b = blank("python");
    let prod = PROD(mode);
    let req = exists(dir, "requirements.txt");
    let uv = exists(dir, "uv.lock") && exists(dir, "pyproject.toml");
    let poetry = exists(dir, "poetry.lock") && exists(dir, "pyproject.toml");
    b.detected.push(if uv {
        "Project type: Python (found uv.lock and pyproject.toml)".to_string()
    } else if poetry {
        "Project type: Python (found poetry.lock and pyproject.toml)".to_string()
    } else if req {
        "Project type: Python (found requirements.txt)".to_string()
    } else {
        "Project type: Python (found pyproject.toml)".to_string()
    });
    let (minor, src) = python_minor(dir);
    b.detected.push(format!("Python 3.{minor} ({src})"));
    let apt = detect::apt_packages("python", sig);
    if !apt.is_empty() {
        b.detected.push(format!(
            "System packages: {} (needed by libraries in your requirements)",
            apt.join(", ")
        ));
    }

    // What runs.
    let django = exists(dir, "manage.py");
    let wsgi_pkg = std::fs::read_dir(dir).ok().and_then(|rd| {
        let mut names: Vec<String> = rd
            .flatten()
            .filter(|e| e.path().join("wsgi.py").is_file())
            .filter_map(|e| e.file_name().to_str().and_then(ident))
            .filter(|n| !n.contains('-') && !n.contains('.'))
            .collect();
        names.sort();
        names.into_iter().next()
    });
    let fastapi_mod = if sig.dep("fastapi") {
        [
            ("main.py", "main:app"),
            ("app/main.py", "app.main:app"),
            ("src/main.py", "src.main:app"),
            ("app.py", "app:app"),
        ]
        .iter()
        .find(|(f, _)| read_small(dir, f).is_some_and(|t| t.contains("FastAPI(")))
        .map(|(_, m)| *m)
    } else {
        None
    };
    let flask_mod = if sig.dep("flask") && !django {
        first_file(dir, &["app.py", "wsgi.py", "main.py", "application.py"])
            .map(|f| f.trim_end_matches(".py"))
    } else {
        None
    };
    let entry = first_file(dir, &["main.py", "app.py", "run.py", "server.py"]);

    let (dev_cmd, prod_cmd, server_pkg): (Vec<String>, Vec<String>, Option<&str>) = if django {
        b.framework = Framework::Django;
        b.detected
            .push("Start command: python manage.py runserver (Django, found manage.py)".into());
        b.notes.push("Django only answers for hosts in ALLOWED_HOSTS: add localhost (and the compose service name) there if you get a 400 error.".into());
        let prod_cmd = match &wsgi_pkg {
            Some(p) => {
                let target = format!("{p}.wsgi:application");
                cmd_vec(&["gunicorn", "--bind", "0.0.0.0:8000", &target])
            }
            None => {
                b.notes.push("No wsgi.py was found next to manage.py: the production container runs runserver.".into());
                cmd_vec(&["python", "manage.py", "runserver", "0.0.0.0:8000"])
            }
        };
        (
            cmd_vec(&["python", "manage.py", "runserver", "0.0.0.0:8000"]),
            prod_cmd,
            Some("gunicorn"),
        )
    } else if let Some(m) = fastapi_mod {
        b.detected.push(format!(
            "Start command: uvicorn {m} (FastAPI, found in {})",
            m.split(':').next().unwrap_or("main").replace('.', "/") + ".py"
        ));
        (
            cmd_vec(&[
                "uvicorn", m, "--host", "0.0.0.0", "--port", "8000", "--reload",
            ]),
            cmd_vec(&["uvicorn", m, "--host", "0.0.0.0", "--port", "8000"]),
            Some("uvicorn"),
        )
    } else if let Some(m) = flask_mod {
        b.detected.push(format!(
            "Start command: flask --app {m} run (Flask, found {m}.py)"
        ));
        let target = format!("{m}:app");
        (
            cmd_vec(&[
                "flask", "--app", m, "run", "--host", "0.0.0.0", "--port", "8000", "--debug",
            ]),
            cmd_vec(&["gunicorn", "--bind", "0.0.0.0:8000", &target]),
            Some("gunicorn"),
        )
    } else {
        let file = entry.unwrap_or("main.py");
        b.detected.push(format!(
            "Start command: python {file} (default: no Django, FastAPI or Flask app found)"
        ));
        b.notes.push(format!(
            "No Django, FastAPI or Flask app was recognised: the container runs `python {file}`."
        ));
        (cmd_vec(&["python", file]), cmd_vec(&["python", file]), None)
    };
    b.detected
        .push("Port: 8000 (guessed: the usual port for Django, uvicorn and gunicorn)".into());
    b.port = 8000;
    if sig.dep("celery") {
        match &wsgi_pkg {
            Some(p) if exists(dir, &format!("{p}/celery.py")) => b.workers.push(Worker {
                suffix: "celery",
                command: cmd_vec(&["celery", "-A", p, "worker", "-l", "info"]),
                why: "Celery worker (found celery.py)".into(),
            }),
            _ => b.notes.push(
                "Celery is in your dependencies but no celery.py was found: no worker was added."
                    .into(),
            ),
        }
    }

    // Dockerfile.
    let mut df = format!(
        "FROM python:3.{minor}-slim\nWORKDIR /app\nENV PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1\n{}",
        apt_line(&apt)
    );
    let no_dev = if prod { " --no-dev" } else { "" };
    if uv {
        df.push_str(&format!(
            "COPY --from=ghcr.io/astral-sh/uv:0.8 /uv /usr/local/bin/uv\nCOPY pyproject.toml uv.lock ./\nRUN --mount=type=cache,target=/root/.cache/uv uv sync --frozen --no-install-project{no_dev}\nCOPY . .\nRUN --mount=type=cache,target=/root/.cache/uv uv sync --frozen{no_dev}\nENV PATH=\"/app/.venv/bin:$PATH\"\n"
        ));
    } else if poetry {
        let only = if prod { " --only main" } else { "" };
        df.push_str(&format!(
            "RUN pip install --no-cache-dir poetry\nENV POETRY_VIRTUALENVS_CREATE=false\nCOPY pyproject.toml poetry.lock ./\nRUN --mount=type=cache,target=/root/.cache/pypoetry poetry install --no-root{only}\nCOPY . .\n"
        ));
    } else if req {
        df.push_str("COPY requirements.txt ./\nRUN --mount=type=cache,target=/root/.cache/pip pip install -r requirements.txt\nCOPY . .\n");
    } else {
        b.notes.push("Only pyproject.toml was found: the image installs the project with pip install . — check that it builds.".into());
        df.push_str("COPY . .\nRUN --mount=type=cache,target=/root/.cache/pip pip install .\n");
    }
    if prod {
        if let Some(pkg) = server_pkg {
            if !sig.dep(pkg) {
                df.push_str(&format!(
                    "RUN --mount=type=cache,target=/root/.cache/pip pip install {pkg}\n"
                ));
            }
        }
        df.push_str("RUN useradd --system --uid 10001 app && chown -R app /app\nUSER app\n");
    } else if let Some(pkg) = server_pkg {
        if !sig.dep(pkg) {
            df.push_str(&format!(
                "RUN --mount=type=cache,target=/root/.cache/pip pip install {pkg}\n"
            ));
        }
    }
    df.push_str(&format!(
        "EXPOSE 8000\nCMD {}\n",
        cmd_text(if prod { &prod_cmd } else { &dev_cmd })
    ));
    b.dockerfile = df;
    if !prod {
        b.watch.push(watch_sync(
            "/app",
            &[".venv/", "__pycache__/", ".git/", "venv/"],
        ));
        b.watch.extend(rebuild_on(
            dir,
            &[
                "requirements.txt",
                "pyproject.toml",
                "uv.lock",
                "poetry.lock",
            ],
        ));
    }
    b.ignore = vec![
        "__pycache__",
        "*.pyc",
        ".venv",
        "venv",
        ".pytest_cache",
        ".mypy_cache",
    ];
    b
}

// ------------------------------------------------------------------ go

pub(super) fn go(dir: &Path, mode: DockerizeMode) -> Built {
    let mut b = blank("go");
    let prod = PROD(mode);
    b.detected.push("Project type: Go (found go.mod)".into());
    let minor = read_small(dir, "go.mod")
        .and_then(|t| {
            t.lines()
                .find(|l| l.starts_with("go "))
                .and_then(first_number)
        })
        .filter(|(maj, m)| *maj == 1 && m.is_some())
        .and_then(|(_, m)| m)
        .map(|m| m.clamp(21, 25));
    let minor = match minor {
        Some(m) => {
            b.detected.push(format!("Go 1.{m} (from go.mod)"));
            m
        }
        None => {
            b.detected
                .push("Go 1.24 (default: go.mod has no usable go line)".into());
            24
        }
    };
    let target = if exists(dir, "main.go") {
        ".".to_string()
    } else {
        let cmds: Vec<String> = std::fs::read_dir(dir.join("cmd"))
            .ok()
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().join("main.go").is_file())
                    .filter_map(|e| e.file_name().to_str().and_then(ident))
                    .collect()
            })
            .unwrap_or_default();
        if cmds.len() == 1 {
            format!("./cmd/{}", cmds[0])
        } else {
            b.notes.push("No main.go at the top and not exactly one cmd/<name>/main.go: the build uses `.` — change the build line if your main package is elsewhere.".into());
            ".".to_string()
        }
    };
    b.detected.push(format!("Build target: {target}"));
    b.detected
        .push("Port: 8080 (guessed: change it if your server listens elsewhere)".into());
    b.port = 8080;
    b.env.push(("PORT".into(), "8080".into()));
    let user = if prod {
        "RUN adduser -D -u 10001 app\nUSER app\n"
    } else {
        ""
    };
    b.dockerfile = format!(
        "FROM golang:1.{minor}-alpine AS build\nWORKDIR /src\nCOPY go.mod go.sum* ./\nRUN --mount=type=cache,target=/go/pkg/mod go mod download\nCOPY . .\nRUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build CGO_ENABLED=0 go build -o /out/app {target}\n\nFROM alpine:3.22\nWORKDIR /app\nRUN apk add --no-cache ca-certificates tzdata\nCOPY --from=build /out/app /app/app\n{user}EXPOSE 8080\nCMD [\"/app/app\"]\n"
    );
    if !prod {
        b.watch.push(watch_rebuild(".", &[".git/", "bin/", "tmp/"]));
        b.notes.push("In dev, every saved change rebuilds the image. For faster loops use a tool such as air in your own Dockerfile.".into());
    }
    b.ignore = vec!["bin", "tmp"];
    b
}

// ------------------------------------------------------------------ rust

pub(super) fn rust(dir: &Path, mode: DockerizeMode) -> Built {
    let mut b = blank("rust");
    let prod = PROD(mode);
    b.detected
        .push("Project type: Rust (found Cargo.toml)".into());
    let text = read_small(dir, "Cargo.toml").unwrap_or_default();
    let name = regex::Regex::new(r#"(?m)^\s*name\s*=\s*"([^"]{1,64})""#)
        .ok()
        .and_then(|re| re.captures(&text).and_then(|c| ident(&c[1])));
    if text.contains("[workspace]") {
        b.notes.push("This is a Cargo workspace: the build uses the root package. Change the build line to pick another binary.".into());
    }
    let bin = match name {
        Some(n) => n,
        None => {
            b.notes.push("The package name could not be read from Cargo.toml: the binary is assumed to be called `app`.".into());
            "app".into()
        }
    };
    b.detected.push(format!("Binary: {bin} (from Cargo.toml)"));
    b.detected
        .push("Port: 8080 (guessed: change it if your server listens elsewhere)".into());
    b.port = 8080;
    b.env.push(("PORT".into(), "8080".into()));
    let user = if prod {
        "RUN adduser -D -u 10001 app\nUSER app\n"
    } else {
        ""
    };
    b.dockerfile = format!(
        "FROM rust:1-alpine AS build\nRUN apk add --no-cache musl-dev\nWORKDIR /src\nCOPY . .\nRUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/src/target cargo build --release && cp target/release/{bin} /out-app\n\nFROM alpine:3.22\nWORKDIR /app\nRUN apk add --no-cache ca-certificates\nCOPY --from=build /out-app /app/app\n{user}EXPOSE 8080\nCMD [\"/app/app\"]\n"
    );
    if !prod {
        b.watch.push(watch_rebuild(".", &[".git/", "target/"]));
    }
    b.ignore = vec!["target"];
    b
}

// ------------------------------------------------------------------ ruby

pub(super) fn ruby(dir: &Path, sig: &Signals, mode: DockerizeMode) -> Built {
    let mut b = blank("ruby");
    let prod = PROD(mode);
    let rails = exists(dir, "config/application.rb") && sig.dep("rails");
    b.framework = if rails {
        Framework::Rails
    } else {
        Framework::Generic
    };
    b.detected.push(if rails {
        "Project type: Ruby on Rails (found config/application.rb and Gemfile)".to_string()
    } else {
        "Project type: Ruby (found Gemfile)".to_string()
    });
    let from_gemfile = read_small(dir, "Gemfile").and_then(|t| {
        regex::Regex::new(r#"(?m)^\s*ruby\s+["'](\d+)\.(\d+)"#)
            .ok()
            .and_then(|re| re.captures(&t).map(|c| format!("{}.{}", &c[1], &c[2])))
    });
    let minor = read_small(dir, ".ruby-version")
        .and_then(|t| first_number(&t))
        .or_else(|| from_gemfile.as_deref().and_then(first_number))
        .filter(|(maj, _)| *maj == 3)
        .and_then(|(_, m)| m)
        .map(|m| m.clamp(1, 4));
    let minor = match minor {
        Some(m) => {
            b.detected
                .push(format!("Ruby 3.{m} (from .ruby-version or Gemfile)"));
            m
        }
        None => {
            b.detected
                .push("Ruby 3.3 (default: no .ruby-version or Gemfile ruby line)".into());
            3
        }
    };
    let mut apt = detect::apt_packages("ruby", sig);
    for p in ["build-essential", "git", "libyaml-dev"] {
        if !apt.iter().any(|x| x == p) {
            apt.push(p.into());
        }
    }
    let rails_bin = exists(dir, "bin/rails");
    let cmd: Vec<String> = if rails {
        if rails_bin {
            cmd_vec(&["bin/rails", "server", "-b", "0.0.0.0", "-p", "3000"])
        } else {
            cmd_vec(&[
                "bundle", "exec", "rails", "server", "-b", "0.0.0.0", "-p", "3000",
            ])
        }
    } else if exists(dir, "config.ru") {
        cmd_vec(&["bundle", "exec", "rackup", "-o", "0.0.0.0", "-p", "3000"])
    } else {
        b.notes
            .push("No Rails app or config.ru found: the container runs `ruby main.rb`.".into());
        cmd_vec(&["ruby", "main.rb"])
    };
    b.detected.push(format!("Start command: {}", cmd.join(" ")));
    b.detected
        .push("Port: 3000 (the usual port for Rails and Rack)".into());
    b.port = 3000;
    if sig.dep("sidekiq") {
        b.workers.push(Worker {
            suffix: "sidekiq",
            command: cmd_vec(&["bundle", "exec", "sidekiq"]),
            why: "Sidekiq worker (found sidekiq in Gemfile)".into(),
        });
    }
    let mut df = format!("FROM ruby:3.{minor}-slim\nWORKDIR /app\n{}", apt_line(&apt));
    if prod {
        df.push_str("ENV RAILS_ENV=production BUNDLE_WITHOUT=\"development:test\" RAILS_LOG_TO_STDOUT=1 RAILS_SERVE_STATIC_FILES=1\n");
    }
    df.push_str("COPY Gemfile Gemfile.lock* ./\nRUN --mount=type=cache,target=/usr/local/bundle/cache bundle install\nCOPY . .\n");
    if prod && rails && detect::has_dir(dir, "app/assets") {
        df.push_str("RUN SECRET_KEY_BASE_DUMMY=1 bundle exec rails assets:precompile\n");
    }
    if prod {
        df.push_str("RUN useradd --system --uid 10001 app && chown -R app /app\nUSER app\n");
    }
    df.push_str(&format!("EXPOSE 3000\nCMD {}\n", cmd_text(&cmd)));
    b.dockerfile = df;
    if rails {
        b.app_secrets.push("SECRET_KEY_BASE");
        b.env.push((
            "RAILS_ENV".into(),
            if prod { "production" } else { "development" }.into(),
        ));
        b.notes
            .push("SECRET_KEY_BASE is generated by OLS and kept in the credential store.".into());
    }
    if !prod {
        b.watch.push(watch_sync(
            "/app",
            &[".git/", "tmp/", "log/", "node_modules/"],
        ));
        b.watch
            .extend(rebuild_on(dir, &["Gemfile", "Gemfile.lock"]));
    }
    b.ignore = vec!["log", "tmp", "vendor/bundle", "node_modules"];
    b
}

// ------------------------------------------------------------------ java

pub(super) fn java(dir: &Path, mode: DockerizeMode) -> Built {
    let mut b = blank("java");
    let prod = PROD(mode);
    let maven = exists(dir, "pom.xml");
    b.detected.push(if maven {
        "Project type: Java (found pom.xml, Maven)".to_string()
    } else {
        "Project type: Java (found build.gradle, Gradle)".to_string()
    });
    let text = read_small(
        dir,
        if maven {
            "pom.xml"
        } else if exists(dir, "build.gradle") {
            "build.gradle"
        } else {
            "build.gradle.kts"
        },
    )
    .unwrap_or_default();
    let re = if maven {
        r"<(?:java\.version|maven\.compiler\.release|maven\.compiler\.source)>(?:1\.)?(\d+)<"
    } else {
        r"(?:JavaLanguageVersion\.of\(|VERSION_|sourceCompatibility\s*=\s*['\x22]?)(?:1_)?(\d+)"
    };
    let wanted: Option<u32> = regex::Regex::new(re)
        .ok()
        .and_then(|re| re.captures(&text).and_then(|c| c[1].parse().ok()));
    let jdk = match wanted {
        Some(v) if v <= 11 => 11,
        Some(v) if v <= 17 => 17,
        Some(v) if v <= 21 => 21,
        Some(_) => 25,
        None => 21,
    };
    b.detected.push(if wanted.is_some() {
        format!(
            "Java {jdk} (from {})",
            if maven { "pom.xml" } else { "build.gradle" }
        )
    } else {
        "Java 21 (default: no Java version found in the build file)".to_string()
    });
    b.detected
        .push("Port: 8080 (the usual port for Spring Boot and similar)".into());
    b.port = 8080;
    b.notes.push("The container runs `java -jar` on the built jar: this works for executable jars (for example Spring Boot).".into());
    let build = if maven {
        format!("FROM maven:3.9-eclipse-temurin-{jdk} AS build\nWORKDIR /app\nCOPY pom.xml ./\nRUN --mount=type=cache,target=/root/.m2 mvn -q -B dependency:go-offline\nCOPY . .\nRUN --mount=type=cache,target=/root/.m2 mvn -q -B -DskipTests package && cp \"$(ls target/*.jar | grep -v -e '-plain' -e original | head -n 1)\" /app.jar\n")
    } else {
        let gradle = if exists(dir, "gradlew") {
            "chmod +x gradlew && ./gradlew --no-daemon bootJar"
        } else {
            "gradle --no-daemon bootJar"
        };
        format!("FROM gradle:8-jdk{jdk} AS build\nWORKDIR /app\nCOPY . .\nRUN --mount=type=cache,target=/home/gradle/.gradle {gradle} && cp \"$(ls build/libs/*.jar | grep -v -e '-plain' | head -n 1)\" /app.jar\n")
    };
    let user = if prod {
        "RUN useradd --system --uid 10001 app\nUSER app\n"
    } else {
        ""
    };
    b.dockerfile = format!(
        "{build}\nFROM eclipse-temurin:{jdk}-jre\nWORKDIR /app\nCOPY --from=build /app.jar /app/app.jar\n{user}EXPOSE 8080\nCMD [\"java\", \"-jar\", \"/app/app.jar\"]\n"
    );
    if !prod {
        b.watch.push(watch_rebuild(
            ".",
            &[".git/", "target/", "build/", ".gradle/"],
        ));
    }
    b.ignore = vec!["target", "build", ".gradle"];
    b
}

// ------------------------------------------------------------------ dotnet

pub(super) fn dotnet(dir: &Path, mode: DockerizeMode) -> Option<Built> {
    let projects: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .filter(|n| n.ends_with(".csproj"))
        .collect();
    let mut b = blank("dotnet");
    let prod = PROD(mode);
    if projects.len() != 1 {
        return None;
    }
    let file = &projects[0];
    let stem = ident(file.trim_end_matches(".csproj"));
    b.detected
        .push(format!("Project type: .NET (found {file})"));
    let major = read_small(dir, file)
        .and_then(|t| {
            regex::Regex::new(r"<TargetFramework>net(\d+)\.\d+</TargetFramework>")
                .ok()
                .and_then(|re| re.captures(&t).and_then(|c| c[1].parse::<u32>().ok()))
        })
        .map(|m| m.clamp(6, 10));
    let major = match major {
        Some(m) => {
            b.detected.push(format!(".NET {m}.0 (from {file})"));
            m
        }
        None => {
            b.detected
                .push(".NET 8.0 (default: no TargetFramework found)".into());
            8
        }
    };
    let dll = match stem {
        Some(s) => s,
        None => {
            b.notes.push("The project file name has characters OLS will not write into a file: the start command uses `app.dll` — fix it.".into());
            "app".into()
        }
    };
    b.notes.push("The start command assumes the assembly is named after the .csproj file. If you set <AssemblyName>, change the CMD line.".into());
    b.detected
        .push("Port: 8080 (set through ASPNETCORE_URLS)".into());
    b.port = 8080;
    b.env
        .push(("ASPNETCORE_URLS".into(), "http://+:8080".into()));
    b.env.push((
        "ASPNETCORE_ENVIRONMENT".into(),
        if prod { "Production" } else { "Development" }.into(),
    ));
    let user = if prod {
        "RUN useradd --system --uid 10001 app\nUSER app\n"
    } else {
        ""
    };
    b.dockerfile = format!(
        "FROM mcr.microsoft.com/dotnet/sdk:{major}.0 AS build\nWORKDIR /src\nCOPY *.csproj ./\nRUN --mount=type=cache,target=/root/.nuget/packages dotnet restore\nCOPY . .\nRUN --mount=type=cache,target=/root/.nuget/packages dotnet publish -c Release -o /out --no-restore\n\nFROM mcr.microsoft.com/dotnet/aspnet:{major}.0\nWORKDIR /app\nCOPY --from=build /out .\n{user}EXPOSE 8080\nCMD [\"dotnet\", \"{dll}.dll\"]\n"
    );
    if !prod {
        b.watch.push(watch_rebuild(".", &[".git/", "bin/", "obj/"]));
    }
    b.ignore = vec!["bin", "obj"];
    Some(b)
}

// ------------------------------------------------------------------ static

pub(super) fn static_site(mode: DockerizeMode) -> Built {
    let mut b = blank("static");
    b.dockerfile = "FROM nginx:1.28-alpine\nCOPY . /usr/share/nginx/html\nEXPOSE 80\n".into();
    b.port = 80;
    b.detected = vec![
        "Project type: Static site (found index.html)".into(),
        "Port: 80 (nginx)".into(),
    ];
    if !PROD(mode) {
        b.watch
            .push(watch_sync("/usr/share/nginx/html", &[".git/"]));
    }
    b
}
