//! Turns a Quick App + the user's answers into a concrete, reviewable plan (§81, §139).
//! Nothing here executes anything: the plan is exactly what the review dialog shows
//! ("Quick App wants to: install software, write files, create a database...") and what
//! the runner later performs, step for step.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use minijinja::{Environment, UndefinedBehavior, Value};
use serde::{Deserialize, Serialize};

use super::schema::{QuickApp, Requirement, Step, StepSpec, Variable};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

pub struct PlanCtx {
    /// Where new projects go when the app doesn't ask (`parent_dir` variable default).
    pub projects_dir: PathBuf,
    /// The active web server id ("nginx" | "apache" | "caddy") — installed when a domain is needed.
    pub web_server: String,
    /// Web ports in use, so a dev server's hot-reload socket can be pointed back through them.
    pub http_port: u16,
    pub https_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepBody {
    Run { program: String, args: Vec<String>, cwd: Option<String> },
    Action { action: String, with: BTreeMap<String, String> },
    WriteFile { path: String, content: String, overwrite: bool },
    WriteEnv { file: String, key: String, value: String },
    EnsureRuntime { id: String, version: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedStep {
    pub stage: String,
    pub name: String,
    /// What the review dialog shows for this step.
    pub display: String,
    pub body: StepBody,
    pub elevated: bool,
    pub allow_failure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReqSpec {
    pub id: String,
    pub wanted: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunPlan {
    pub app_id: String,
    pub app_name: String,
    /// Answers with secrets masked — safe to send to the UI and the log.
    pub display_values: BTreeMap<String, String>,
    pub project_path: Option<String>,
    pub hostname: Option<String>,
    pub https: bool,
    pub steps: Vec<PlannedStep>,
    pub permissions: Vec<Permission>,
    pub requirements: Vec<ReqSpec>,
    pub warnings: Vec<String>,
    /// The real answers, including secrets. Never serialized.
    #[serde(skip)]
    pub values: BTreeMap<String, String>,
}

// ------------------------------------------------------------------------------ templates

pub fn slugify(s: &str) -> String {
    crate::domain::slugify(s)
}

fn random_string(n: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut bytes = vec![0u8; n];
    // A failing OS RNG would otherwise yield a predictable "secret" — fail loudly instead.
    getrandom::fill(&mut bytes).expect("the operating system random number generator failed");
    bytes.iter().map(|b| ALPHABET[*b as usize % ALPHABET.len()] as char).collect()
}

fn env() -> Environment<'static> {
    let mut env = Environment::new();
    // A typo'd variable must fail the render, not silently become an empty string.
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_keep_trailing_newline(true);
    env.add_filter("slug", |s: String| slugify(&s));
    env.add_filter("lower", |s: String| s.to_lowercase());
    env.add_function("random_string", |n: usize| random_string(n.min(256)));
    env
}

fn context(values: &BTreeMap<String, String>, vars: &[Variable]) -> BTreeMap<String, Value> {
    let mut ctx = BTreeMap::new();
    for (k, v) in values {
        let ty = vars.iter().find(|x| &x.name == k).map(|x| x.var_type.as_str());
        let value = match ty {
            Some("boolean") => Value::from(is_truthy(v)),
            Some("number") | Some("port") => v.parse::<i64>().map(Value::from).unwrap_or_else(|_| Value::from(v.as_str())),
            _ => Value::from(v.as_str()),
        };
        ctx.insert(k.clone(), value);
    }
    ctx
}

pub fn render(template: &str, values: &BTreeMap<String, String>, vars: &[Variable]) -> Result<String, String> {
    let env = env();
    env.render_str(template, context(values, vars)).map_err(|e| format!("template error in \"{template}\": {e}"))
}

// ---------------------------------------------------------------------------- conditions

fn is_truthy(v: &str) -> bool {
    !matches!(v.trim().to_ascii_lowercase().as_str(), "" | "false" | "0" | "no" | "none" | "off")
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"').and_then(|x| x.strip_suffix('"')).or_else(|| s.strip_prefix('\'').and_then(|x| x.strip_suffix('\''))).unwrap_or(s)
}

/// §85: `database == mysql`, `redis == true`, `install_node`, `!x`, joined by `&&` / `||`
/// (also `and` / `or`). Missing variables read as empty.
pub fn eval_condition(expr: &str, values: &BTreeMap<String, String>) -> bool {
    let lookup = |name: &str| values.get(name.trim()).cloned().unwrap_or_default();
    let normalized = expr.replace(" and ", " && ").replace(" or ", " || ");
    normalized.split("||").any(|or_part| {
        or_part.split("&&").all(|atom| {
            let atom = atom.trim();
            if let Some((l, r)) = atom.split_once("==") {
                lookup(l).eq_ignore_ascii_case(unquote(r))
            } else if let Some((l, r)) = atom.split_once("!=") {
                !lookup(l).eq_ignore_ascii_case(unquote(r))
            } else if let Some(rest) = atom.strip_prefix('!').or_else(|| atom.strip_prefix("not ")) {
                !is_truthy(&lookup(rest))
            } else {
                is_truthy(&lookup(atom))
            }
        })
    })
}

// -------------------------------------------------------------------------------- tokens

/// Splits a command line into arguments the way a person reads it: whitespace separates,
/// quotes group. Backslashes are *not* escapes (Windows paths are full of them) except `\"`
/// inside double quotes.
pub fn split_command_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        // A `{{ ... }}` / `{% ... %}` template tag is never split, even unquoted and with
        // spaces inside (`127.0.0.1:{{ port }}`); it becomes part of the current argument.
        if quote.is_none() && c == '{' && matches!(chars.peek(), Some('{') | Some('%')) {
            let open = chars.next().unwrap();
            let close = if open == '{' { '}' } else { '%' };
            cur.push('{');
            cur.push(open);
            let mut prev = '\0';
            for t in chars.by_ref() {
                cur.push(t);
                if t == '}' && prev == close {
                    break;
                }
                prev = t;
            }
            started = true;
            continue;
        }
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') if chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            (Some(_), c) => cur.push(c),
            (None, '"') | (None, '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if started || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Tokenises first, then renders each token, so a value containing spaces (a project
/// folder like `C:\Users\Jo Smith\Sites`) stays one argument.
fn render_command(line: &str, values: &BTreeMap<String, String>, vars: &[Variable]) -> Result<(String, Vec<String>), String> {
    let tokens = split_command_line(line);
    let mut rendered = Vec::with_capacity(tokens.len());
    for t in &tokens {
        rendered.push(render(t, values, vars)?);
    }
    let mut it = rendered.into_iter();
    let program = it.next().filter(|p| !p.is_empty()).ok_or_else(|| format!("empty command in \"{line}\""))?;
    Ok((program, it.collect()))
}

// --------------------------------------------------------------------------------- values

/// Applies defaults, conditions and validation to what the user typed (§84). Returns every
/// problem at once so the wizard can mark all bad fields, not just the first.
pub fn resolve_values(
    app: &QuickApp,
    provided: &BTreeMap<String, String>,
    ctx: &PlanCtx,
) -> Result<BTreeMap<String, String>, Vec<FieldError>> {
    let (values, errors) = resolve_lenient(app, provided, ctx);
    if errors.is_empty() {
        Ok(values)
    } else {
        Err(errors)
    }
}

/// Like [`resolve_values`], but always returns the best-effort answers next to the errors —
/// the wizard uses them to show templated defaults ("shop.test") while the form is still
/// being filled in.
pub fn resolve_lenient(app: &QuickApp, provided: &BTreeMap<String, String>, ctx: &PlanCtx) -> (BTreeMap<String, String>, Vec<FieldError>) {
    let mut values: BTreeMap<String, String> = BTreeMap::new();
    values.insert("default_projects_dir".into(), ctx.projects_dir.display().to_string());
    let mut errors = Vec::new();

    for v in &app.variables {
        let visible = v.show_if.as_deref().map(|c| eval_condition(c, &values)).unwrap_or(true);
        let raw = provided.get(&v.name).cloned().or_else(|| {
            v.default.as_ref().map(|d| {
                let text = d.as_string();
                // Defaults may reference earlier answers ("{{project_name}}.test").
                render(&text, &values, &app.variables).unwrap_or(text)
            })
        });
        let mut value = raw.unwrap_or_default().trim().to_string();

        if v.var_type == "boolean" {
            value = if is_truthy(&value) { "true".into() } else { "false".into() };
        }
        if !visible {
            values.insert(v.name.clone(), if v.var_type == "boolean" { "false".into() } else { String::new() });
            continue;
        }
        if value.is_empty() {
            if v.required && v.var_type != "boolean" {
                errors.push(FieldError { field: v.name.clone(), message: format!("{} is required", label_of(v)) });
            }
            values.insert(v.name.clone(), value);
            continue;
        }

        if let Some(msg) = validate_value(v, &value) {
            errors.push(FieldError { field: v.name.clone(), message: msg });
        }
        values.insert(v.name.clone(), value);
    }
    // Derived, always available to templates.
    let https_on = values.get("https").map(|v| is_truthy(v)).or(app.environment.https).unwrap_or(false);
    values.insert("site_port".into(), if https_on { ctx.https_port } else { ctx.http_port }.to_string());
    if !values.contains_key("parent_dir") || values["parent_dir"].is_empty() {
        values.insert("parent_dir".into(), ctx.projects_dir.display().to_string());
    }
    if let Some(name) = values.get("project_name").filter(|n| !n.is_empty()).cloned() {
        let project_path = Path::new(&values["parent_dir"]).join(&name);
        values.insert("project_path".into(), project_path.display().to_string());
        values.insert("project_slug".into(), slugify(&name));
    }
    (values, errors)
}

fn label_of(v: &Variable) -> &str {
    if v.label.is_empty() {
        &v.name
    } else {
        &v.label
    }
}

fn validate_value(v: &Variable, value: &str) -> Option<String> {
    let label = label_of(v);
    if let Some(pattern) = &v.validation {
        // Anchored so "a-z" doesn't accept "abc; rm -rf".
        let re = regex::Regex::new(&format!("^(?:{pattern})$")).ok()?;
        if !re.is_match(value) {
            return Some(format!("{label} isn't in the expected format"));
        }
    }
    match v.var_type.as_str() {
        "number" if value.parse::<i64>().is_err() => Some(format!("{label} must be a number")),
        "port" => match value.parse::<u16>() {
            Ok(p) if p >= 1 => None,
            _ => Some(format!("{label} must be a port between 1 and 65535")),
        },
        "select" if !v.options.iter().any(|o| o.as_string() == value) => {
            Some(format!("{label} must be one of: {}", v.options.iter().map(|o| o.as_string()).collect::<Vec<_>>().join(", ")))
        }
        "multiselect" => value
            .split(',')
            .map(str::trim)
            .find(|p| !v.options.iter().any(|o| o.as_string() == *p))
            .map(|bad| format!("{label} has an unknown choice \"{bad}\"")),
        "domain" => crate::domain::validate_hostname(value).err().map(|e| e.to_string()),
        _ => None,
    }
}

// ----------------------------------------------------------------------------------- plan

fn req_version(r: &Requirement) -> Option<String> {
    match r {
        Requirement::Flag(_) => None,
        Requirement::Version(s) => Some(s.as_string()),
    }
}

fn req_enabled(r: &Requirement) -> bool {
    !matches!(r, Requirement::Flag(false))
}

/// A requirement key may be switched off by a matching boolean variable (`mailpit: false`).
fn req_wanted(id: &str, r: &Requirement, values: &BTreeMap<String, String>) -> bool {
    req_enabled(r) && values.get(id).map(|v| is_truthy(v) || !matches!(v.as_str(), "false")).unwrap_or(true)
}

pub fn build_plan(app: &QuickApp, values: BTreeMap<String, String>, ctx: &PlanCtx) -> Result<RunPlan, String> {
    let vars = &app.variables;
    let mut steps: Vec<PlannedStep> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut requirements: Vec<ReqSpec> = Vec::new();

    let flag = |name: &str, fallback: Option<bool>| -> bool {
        values.get(name).map(|v| is_truthy(v)).or(fallback).unwrap_or(false)
    };
    let https = flag("https", app.environment.https);
    let wildcard = flag("wildcard", app.environment.wildcard);
    let mailpit_wanted = flag("mailpit", app.environment.mailpit);

    // 1. Requirements → make sure each runtime is installed (§154 "satisfy automatically").
    let mut services_to_start: Vec<String> = Vec::new();
    for (id, req) in &app.requirements {
        if !req_wanted(id, req, &values) {
            continue;
        }
        match id.as_str() {
            "redis" => {
                warnings.push("Redis has no official Windows build yet, so it was skipped.".to_string());
            }
            "mailpit" => {
                if mailpit_wanted || !values.contains_key("mailpit") {
                    requirements.push(ReqSpec { id: id.clone(), wanted: None });
                    services_to_start.push("mailpit".into());
                }
            }
            "mysql" | "mariadb" | "mongodb" => {
                // Only when the chosen database is this one (or the app has no `database` choice).
                let chosen = values.get("database").map(|d| d == id).unwrap_or(true);
                if chosen {
                    requirements.push(ReqSpec { id: id.clone(), wanted: req_version(req) });
                    services_to_start.push(id.clone());
                }
            }
            "composer" => {
                requirements.push(ReqSpec { id: "php".into(), wanted: values.get("php_version").cloned() });
                requirements.push(ReqSpec { id: "composer".into(), wanted: None });
            }
            "php" => {
                let wanted = values.get("php_version").cloned().or_else(|| req_version(req));
                requirements.push(ReqSpec { id: "php".into(), wanted });
            }
            "node" => {
                let wanted = values.get("node_version").cloned().or_else(|| req_version(req));
                requirements.push(ReqSpec { id: "node".into(), wanted });
            }
            other => requirements.push(ReqSpec { id: other.to_string(), wanted: req_version(req) }),
        }
    }
    if app.domain.is_some() {
        requirements.push(ReqSpec { id: ctx.web_server.clone(), wanted: None });
    }
    // De-duplicate while keeping the first (most specific) entry per runtime.
    let mut seen = std::collections::HashSet::new();
    requirements.retain(|r| seen.insert(r.id.clone()));

    for r in &requirements {
        steps.push(PlannedStep {
            stage: "requirements".into(),
            name: match &r.wanted {
                Some(v) => format!("Make sure {} {v} is installed", r.id),
                None => format!("Make sure {} is installed", r.id),
            },
            display: format!("ensure runtime {}", r.id),
            body: StepBody::EnsureRuntime { id: r.id.clone(), version: r.wanted.clone() },
            elevated: false,
            allow_failure: false,
        });
    }
    for svc in &services_to_start {
        steps.push(PlannedStep {
            stage: "requirements".into(),
            name: format!("Start {svc}"),
            display: format!("start service {svc}"),
            body: StepBody::Action { action: "start_service".into(), with: BTreeMap::from([("id".to_string(), svc.clone())]) },
            elevated: false,
            allow_failure: false,
        });
    }

    let push_steps = |stage: &str, list: &[Step], steps: &mut Vec<PlannedStep>| -> Result<(), String> {
        for step in list {
            if let Some(p) = plan_step(stage, &step.spec(), &values, vars)? {
                steps.push(p);
            }
        }
        Ok(())
    };

    push_steps("pre_create", &app.pre_create, &mut steps)?;
    push_steps("create", &app.commands, &mut steps)?;
    for c in &app.conditions {
        if eval_condition(&c.cond, &values) {
            push_steps("create", &c.commands, &mut steps)?;
        }
    }

    // Files and env vars are written after creation (the folder exists by then).
    let project_path = values.get("project_path").cloned();
    if let Some(root) = &project_path {
        for f in &app.files {
            if f.cond.as_deref().is_some_and(|c| !eval_condition(c, &values)) {
                continue;
            }
            let rel = render(&f.path, &values, vars)?;
            let abs = safe_join(root, &rel)?;
            steps.push(PlannedStep {
                stage: "files".into(),
                name: format!("Write {rel}"),
                display: format!("write file {rel}"),
                body: StepBody::WriteFile { path: abs, content: render(&f.content, &values, vars)?, overwrite: f.overwrite },
                elevated: false,
                allow_failure: false,
            });
        }
        for e in &app.env_vars {
            if e.cond.as_deref().is_some_and(|c| !eval_condition(c, &values)) {
                continue;
            }
            let file = safe_join(root, &render(&e.file, &values, vars)?)?;
            let key = e.key.clone();
            steps.push(PlannedStep {
                stage: "files".into(),
                name: format!("Set {key} in {}", e.file),
                display: format!("set {key} in {}", e.file),
                body: StepBody::WriteEnv { file, key, value: render(&e.value, &values, vars)? },
                elevated: false,
                allow_failure: false,
            });
        }
    }

    push_steps("post_create", &app.post_create, &mut steps)?;
    push_steps("pre_install", &app.pre_install, &mut steps)?;
    push_steps("install", &app.install, &mut steps)?;
    push_steps("post_install", &app.post_install, &mut steps)?;

    // 4. Finalize: project, domain, certificate, web server, health (§155 result list).
    let mut hostname = None;
    if let Some(root) = &project_path {
        steps.push(action_step("finalize", "Register the project", "register_project", [("path".to_string(), root.clone())]));
    }
    push_steps("pre_start", &app.pre_start, &mut steps)?;

    if let Some(d) = &app.domain {
        if d.cond.as_deref().map(|c| eval_condition(c, &values)).unwrap_or(true) {
            let host = render(&d.hostname, &values, vars)?.to_ascii_lowercase();
            let kind = render(&d.kind, &values, vars)?;
            if !matches!(kind.as_str(), "php" | "proxy" | "static") {
                return Err(format!("domain kind \"{kind}\" must be php, proxy or static"));
            }
            crate::domain::validate_hostname(&host).map_err(|e| e.to_string())?;
            let root = match (&d.root, &project_path) {
                (Some(r), _) => render(r, &values, vars)?,
                (None, Some(p)) => p.clone(),
                (None, None) => return Err("the domain needs a document root, but the app has no project folder".into()),
            };
            let mut with: BTreeMap<String, String> = BTreeMap::from([
                ("hostname".into(), host.clone()),
                ("kind".into(), kind.clone()),
                ("root".into(), root),
                ("https".into(), https.to_string()),
                ("wildcard".into(), wildcard.to_string()),
            ]);
            if let Some(p) = &d.port {
                with.insert("port".into(), render(p, &values, vars)?);
            }
            if let Some(h) = &d.host {
                with.insert("upstream_host".into(), render(h, &values, vars)?);
            }
            if let Some(s) = &d.upstream_https {
                with.insert("upstream_https".into(), render(s, &values, vars)?);
            }
            if let Some(php) = values.get("php_version").filter(|v| !v.is_empty()) {
                with.insert("php_version".into(), php.clone());
            }
            let app_cmd = d.app.as_ref().filter(|a| a.cond.as_deref().map(|c| eval_condition(c, &values)).unwrap_or(true));
            if kind != "proxy" && d.port.is_some() {
                with.remove("port");
            }
            if let Some(app_cmd) = app_cmd {
                let (program, args) = render_command(&app_cmd.run, &values, vars)?;
                with.insert("app_program".into(), program);
                with.insert("app_args".into(), serde_json::to_string(&args).unwrap_or_default());
                if let Some(rt) = &app_cmd.runtime {
                    let rt = render(rt, &values, vars)?;
                    if !rt.is_empty() && rt != "none" {
                        with.insert("app_runtime".into(), rt);
                    }
                }
                let cwd = match &app_cmd.cwd {
                    Some(c) => render(c, &values, vars)?,
                    None => project_path.clone().unwrap_or_default(),
                };
                with.insert("app_cwd".into(), cwd);
            }
            hostname = Some(host.clone());
            steps.push(action_step("finalize", &format!("Create {host}"), "create_domain", with));
            if https {
                {
                    // Windows shows its own confirmation for a root certificate; if it's
                    // declined the site still works, the browser just warns.
                    let mut trust = action_step("finalize", "Trust the local certificate authority", "trust_ca", []);
                    trust.allow_failure = true;
                    steps.push(trust);
                }
            }
            steps.push(action_step("finalize", "Apply web server config", "apply_web", []));
            steps.push(PlannedStep {
                stage: "finalize".into(),
                name: "Health checks".into(),
                display: format!("health check {host}"),
                body: StepBody::Action { action: "health_check".into(), with: BTreeMap::from([("hostname".to_string(), host)]) },
                elevated: false,
                allow_failure: true,
            });
        }
    }
    push_steps("post_start", &app.post_start, &mut steps)?;

    let permissions = derive_permissions(&steps, https, hostname.is_some());
    let display_values = values
        .iter()
        .map(|(k, v)| {
            let secret = vars.iter().any(|x| &x.name == k && x.is_secret());
            (k.clone(), if secret && !v.is_empty() { "••••••••".to_string() } else { v.clone() })
        })
        .collect();

    Ok(RunPlan {
        app_id: app.id.clone(),
        app_name: app.name.clone(),
        display_values,
        project_path,
        hostname,
        https,
        steps,
        permissions,
        requirements,
        warnings,
        values,
    })
}

fn action_step(stage: &str, name: &str, action: &str, with: impl IntoIterator<Item = (String, String)>) -> PlannedStep {
    PlannedStep {
        stage: stage.into(),
        name: name.into(),
        display: action.replace('_', " "),
        body: StepBody::Action { action: action.into(), with: with.into_iter().collect() },
        elevated: false,
        allow_failure: false,
    }
}

fn plan_step(
    stage: &str,
    spec: &StepSpec,
    values: &BTreeMap<String, String>,
    vars: &[Variable],
) -> Result<Option<PlannedStep>, String> {
    if spec.cond.as_deref().is_some_and(|c| !eval_condition(c, values)) {
        return Ok(None);
    }
    let cwd = spec.cwd.as_deref().map(|c| render(c, values, vars)).transpose()?;
    if let Some(line) = &spec.run {
        let (program, args) = render_command(line, values, vars)?;
        let display = std::iter::once(program.clone()).chain(args.iter().map(|a| quote_for_display(a))).collect::<Vec<_>>().join(" ");
        return Ok(Some(PlannedStep {
            stage: stage.into(),
            name: spec.name.clone().unwrap_or_else(|| display.clone()),
            display,
            body: StepBody::Run { program, args, cwd },
            elevated: spec.elevated,
            allow_failure: spec.allow_failure,
        }));
    }
    let action = spec.action.clone().expect("validated: run or action");
    let mut with = BTreeMap::new();
    for (k, v) in &spec.with {
        with.insert(k.clone(), render(&v.as_string(), values, vars)?);
    }
    Ok(Some(PlannedStep {
        stage: stage.into(),
        name: spec.name.clone().unwrap_or_else(|| action.replace('_', " ")),
        display: format!("{action} {}", with.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" ")).trim().to_string(),
        body: StepBody::Action { action, with },
        elevated: spec.elevated,
        allow_failure: spec.allow_failure,
    }))
}

fn quote_for_display(arg: &str) -> String {
    if arg.contains(' ') {
        format!("\"{arg}\"")
    } else {
        arg.to_string()
    }
}

/// Joins `rel` under `root`, refusing anything that escapes it (`..`, absolute paths, drive letters).
pub fn safe_join(root: &str, rel: &str) -> Result<String, String> {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute()
        || rel.contains(':')
        || rel_path.components().any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::RootDir | std::path::Component::Prefix(_)))
    {
        return Err(format!("\"{rel}\" must stay inside the project folder"));
    }
    Ok(Path::new(root).join(rel_path).display().to_string())
}

/// §139 / §92: what the review dialog lists before anything runs.
fn derive_permissions(steps: &[PlannedStep], https: bool, has_domain: bool) -> Vec<Permission> {
    let mut perms = Vec::new();
    let mut add = |id: &str, label: &str| {
        if !perms.iter().any(|p: &Permission| p.id == id) {
            perms.push(Permission { id: id.into(), label: label.into() });
        }
    };
    for s in steps {
        match &s.body {
            StepBody::Run { .. } => add("execute", "Run commands"),
            StepBody::EnsureRuntime { .. } => add("install", "Download and install software"),
            StepBody::WriteFile { .. } | StepBody::WriteEnv { .. } => add("write_files", "Write files in the project folder"),
            StepBody::Action { action, .. } => match action.as_str() {
                "create_database" => add("database", "Create a database"),
                "create_domain" => add("dns", "Add a domain (hosts file — Windows may ask for administrator approval)"),
                "start_service" => add("services", "Start background services"),
                _ => {}
            },
        }
        if s.elevated {
            add("elevated", "Run a step with administrator rights");
        }
    }
    if https && has_domain {
        add("certificate", "Create a certificate and trust the local CA");
    }
    perms
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quickapp::schema::parse;

    fn ctx() -> PlanCtx {
        PlanCtx { projects_dir: PathBuf::from("C:\\Sites"), web_server: "nginx".into(), http_port: 80, https_port: 443 }
    }

    fn vals(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn tokenizer_groups_quotes_and_keeps_windows_paths() {
        assert_eq!(split_command_line(r#"composer create-project laravel/laravel "C:\My Sites\shop""#), ["composer", "create-project", "laravel/laravel", r"C:\My Sites\shop"]);
        assert_eq!(split_command_line("a   b\tc"), ["a", "b", "c"]);
        assert_eq!(split_command_line(r#"echo "say \"hi\"""#), ["echo", r#"say "hi""#]);
        assert_eq!(split_command_line("x ''"), ["x", ""]);
        assert_eq!(split_command_line("runserver 127.0.0.1:{{ port }} --x"), ["runserver", "127.0.0.1:{{ port }}", "--x"], "template tags are atomic");
        assert_eq!(split_command_line("run {% if a %}--b{% endif %} c"), ["run", "{% if a %}--b{% endif %}", "c"]);
        assert!(split_command_line("   ").is_empty());
    }

    #[test]
    fn conditions_cover_equality_truthiness_negation_and_joins() {
        let v = vals(&[("database", "mysql"), ("redis", "true"), ("empty", ""), ("off", "false")]);
        assert!(eval_condition("database == mysql", &v));
        assert!(!eval_condition("database == mariadb", &v));
        assert!(eval_condition("database != none", &v));
        assert!(eval_condition("redis == true", &v));
        assert!(eval_condition("redis", &v));
        assert!(!eval_condition("empty", &v));
        assert!(!eval_condition("off", &v));
        assert!(eval_condition("!off", &v));
        assert!(eval_condition("not off", &v));
        assert!(eval_condition("database == mysql && redis", &v));
        assert!(!eval_condition("database == mysql and off", &v));
        assert!(eval_condition("off || redis", &v));
        assert!(eval_condition("database == 'mysql'", &v), "quotes are ignored");
        assert!(!eval_condition("missing", &v));
    }

    #[test]
    fn values_get_defaults_derived_paths_and_validation() {
        let app = parse(
            r#"
id: t
name: T
variables:
  - { name: project_name, type: text, required: true, validation: "[a-z][a-z0-9-]*" }
  - { name: domain, type: domain, default: "{{project_name}}.test" }
  - { name: https, type: boolean, default: true }
  - { name: port, type: port, default: 5173 }
  - { name: database, type: select, options: [none, mysql], default: mysql }
"#,
        )
        .unwrap();

        let v = resolve_values(&app, &vals(&[("project_name", "shop")]), &ctx()).unwrap();
        assert_eq!(v["domain"], "shop.test");
        assert_eq!(v["https"], "true");
        assert_eq!(v["port"], "5173");
        assert_eq!(v["project_path"], "C:\\Sites\\shop");

        let errs = resolve_values(&app, &vals(&[("project_name", "Bad Name; rm")]), &ctx()).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "project_name"), "{errs:?}");

        let errs = resolve_values(&app, &vals(&[]), &ctx()).unwrap_err();
        assert!(errs[0].message.contains("required"));

        let errs = resolve_values(&app, &vals(&[("project_name", "shop"), ("database", "oracle"), ("port", "99999")]), &ctx()).unwrap_err();
        assert_eq!(errs.len(), 2, "every bad field is reported at once");
    }

    #[test]
    fn conditional_variables_are_skipped_when_their_condition_fails() {
        let app = parse(
            r#"
id: t
name: T
variables:
  - { name: database, type: select, options: [none, mysql], default: none }
  - { name: db_name, required: true, show_if: "database != none" }
"#,
        )
        .unwrap();
        assert!(resolve_values(&app, &vals(&[]), &ctx()).is_ok(), "db_name isn't required when there's no database");
        assert!(resolve_values(&app, &vals(&[("database", "mysql")]), &ctx()).is_err());
    }

    #[test]
    fn plan_orders_stages_and_skips_false_conditions() {
        let app = parse(
            r#"
id: t
name: T
requirements: { php: "8.4", composer: true, mysql: "8.4", mailpit: true, redis: true }
variables:
  - { name: project_name, required: true }
  - { name: database, type: select, options: [none, mysql], default: mysql }
  - { name: redis, type: boolean, default: true }
  - { name: mailpit, type: boolean, default: true }
  - { name: https, type: boolean, default: true }
pre_create:
  - name: before
    run: echo before
commands:
  - composer create-project laravel/laravel "{{ project_path }}"
conditions:
  - if: "database == mysql"
    commands:
      - action: create_database
        with: { engine: mysql, name: "{{ project_name }}" }
  - if: "database == none"
    commands: [ "echo never" ]
files:
  - { path: "note.txt", content: "hello {{ project_name }}" }
env_vars:
  - { key: APP_URL, value: "https://{{ project_name }}.test" }
post_create:
  - php artisan migrate
domain: { hostname: "{{ project_name }}.test", kind: php, root: "{{ project_path }}/public" }
"#,
        )
        .unwrap();
        let v = resolve_values(&app, &vals(&[("project_name", "shop")]), &ctx()).unwrap();
        let plan = build_plan(&app, v, &ctx()).unwrap();

        let stages: Vec<&str> = plan.steps.iter().map(|s| s.stage.as_str()).collect();
        let first = |name: &str| stages.iter().position(|s| *s == name).unwrap_or_else(|| panic!("missing stage {name}: {stages:?}"));
        assert!(first("requirements") < first("pre_create"));
        assert!(first("pre_create") < first("create"));
        assert!(first("create") < first("files"));
        assert!(first("files") < first("post_create"));
        assert!(first("post_create") < first("finalize"));

        assert!(!plan.steps.iter().any(|s| s.display.contains("never")), "false condition must be skipped");
        assert!(plan.steps.iter().any(|s| matches!(&s.body, StepBody::Action { action, .. } if action == "create_database")));
        assert_eq!(plan.hostname.as_deref(), Some("shop.test"));
        assert!(plan.warnings.iter().any(|w| w.contains("Redis")));
        let ids: Vec<&str> = plan.requirements.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"php") && ids.contains(&"composer") && ids.contains(&"mysql") && ids.contains(&"mailpit") && ids.contains(&"nginx"));
        assert_eq!(ids.iter().filter(|i| **i == "php").count(), 1, "php requirement de-duplicated");

        let perm_ids: Vec<&str> = plan.permissions.iter().map(|p| p.id.as_str()).collect();
        for want in ["execute", "install", "write_files", "database", "dns", "certificate"] {
            assert!(perm_ids.contains(&want), "missing permission {want}: {perm_ids:?}");
        }
        // Stage order inside finalize: domain → trust → apply → health.
        let fin: Vec<&str> = plan.steps.iter().filter(|s| s.stage == "finalize").map(|s| s.name.as_str()).collect();
        assert_eq!(fin.last().copied(), Some("Health checks"));
    }

    #[test]
    fn paths_with_spaces_stay_one_argument() {
        let app = parse("id: t\nname: T\nvariables:\n  - { name: project_name }\ncommands:\n  - 'tool \"{{ project_path }}\" --flag'\n").unwrap();
        let mut v = resolve_values(&app, &vals(&[("project_name", "shop")]), &PlanCtx { projects_dir: "C:\\Jo Smith\\Sites".into(), web_server: "nginx".into(), http_port: 80, https_port: 443 }).unwrap();
        v.insert("x".into(), "y".into());
        let plan = build_plan(&app, v, &ctx()).unwrap();
        let StepBody::Run { args, .. } = &plan.steps.iter().find(|s| matches!(s.body, StepBody::Run { .. })).expect("a run step").body else { panic!() };
        assert_eq!(args, &["C:\\Jo Smith\\Sites\\shop", "--flag"]);
    }

    #[test]
    fn undefined_template_variables_fail_instead_of_rendering_blank() {
        let app = parse("id: t\nname: T\nvariables:\n  - { name: project_name }\ncommands:\n  - echo {{ typo_here }}\n").unwrap();
        let v = resolve_values(&app, &vals(&[("project_name", "a")]), &ctx()).unwrap();
        assert!(build_plan(&app, v, &ctx()).is_err());
    }

    #[test]
    fn files_cannot_escape_the_project_folder() {
        assert!(safe_join("C:\\p", "..\\evil.txt").is_err());
        assert!(safe_join("C:\\p", "C:\\Windows\\x").is_err());
        assert!(safe_join("C:\\p", "\\abs").is_err());
        assert!(safe_join("C:\\p", "sub/ok.txt").is_ok());
    }

    #[test]
    fn secrets_are_masked_in_the_display_values_but_kept_in_the_real_ones() {
        let app = parse("id: t\nname: T\nvariables:\n  - { name: project_name }\n  - { name: db_pass, type: password }\n").unwrap();
        let v = resolve_values(&app, &vals(&[("project_name", "a"), ("db_pass", "hunter2")]), &ctx()).unwrap();
        let plan = build_plan(&app, v, &ctx()).unwrap();
        assert_eq!(plan.display_values["db_pass"], "••••••••");
        assert_eq!(plan.values["db_pass"], "hunter2");
        assert!(!serde_json::to_string(&plan).unwrap().contains("hunter2"), "secrets must not serialize");
    }

    #[test]
    fn templates_support_filters_and_random_functions() {
        let v = vals(&[("name", "My Shop")]);
        assert_eq!(render("{{ name | slug }}.test", &v, &[]).unwrap(), "my-shop.test");
        assert_eq!(render("{{ random_string(40) }}", &v, &[]).unwrap().len(), 40);
        assert_ne!(render("{{ random_string(16) }}", &v, &[]).unwrap(), render("{{ random_string(16) }}", &v, &[]).unwrap());
    }
}
