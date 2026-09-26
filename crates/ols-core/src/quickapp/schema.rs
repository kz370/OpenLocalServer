//! Quick App definition schema (§79–86, §152). Editable YAML in, typed structure out.
//! Everything user-supplied here ends up in commands and files, so parsing is strict and
//! `validate` rejects definitions that can't work before they ever get near a run.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

/// A YAML scalar that may be written as a bool, number or string (`default: 8.4`,
/// `default: true`, `default: "shop"`), kept as written so export/edit round-trips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl Scalar {
    pub fn as_string(&self) -> String {
        match self {
            Scalar::Bool(b) => b.to_string(),
            Scalar::Int(i) => i.to_string(),
            Scalar::Float(f) => f.to_string(),
            Scalar::Str(s) => s.clone(),
        }
    }
}

/// §84 supported variable types.
pub const VARIABLE_TYPES: &[&str] = &[
    "text",
    "number",
    "boolean",
    "select",
    "multiselect",
    "path",
    "directory",
    "file",
    "password",
    "secret",
    "port",
    "domain",
    "runtime-version",
    "database-version",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Variable {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "text_type", rename = "type")]
    pub var_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<Scalar>,
    #[serde(default)]
    pub options: Vec<Scalar>,
    /// Regex the value must fully match (§84 "validated").
    #[serde(default)]
    pub validation: Option<String>,
    /// §84 "conditional": only asked for (and required) when this condition holds.
    #[serde(default, rename = "show_if")]
    pub show_if: Option<String>,
    /// For `runtime-version`: which runtime's versions to offer ("php", "node").
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub help: Option<String>,
}

fn text_type() -> String {
    "text".to_string()
}

impl Variable {
    pub fn is_secret(&self) -> bool {
        matches!(self.var_type.as_str(), "password" | "secret")
    }
}

/// `variables:` may be a list (each with a `name`) or a map keyed by name (§83's example).
fn de_variables<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Variable>, D::Error> {
    use serde::de::Error;
    let value = serde_yaml_ng::Value::deserialize(d)?;
    match value {
        serde_yaml_ng::Value::Null => Ok(Vec::new()),
        serde_yaml_ng::Value::Sequence(items) => items
            .into_iter()
            .map(|i| serde_yaml_ng::from_value(i).map_err(D::Error::custom))
            .collect(),
        serde_yaml_ng::Value::Mapping(map) => {
            let mut out = Vec::new();
            for (k, v) in map {
                let name = k
                    .as_str()
                    .ok_or_else(|| D::Error::custom("variable names must be strings"))?
                    .to_string();
                let mut var: Variable = serde_yaml_ng::from_value(v).map_err(D::Error::custom)?;
                var.name = name;
                out.push(var);
            }
            Ok(out)
        }
        _ => Err(D::Error::custom("variables must be a list or a map")),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Step {
    /// `- composer create-project ...`
    Line(String),
    Full(StepSpec),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StepSpec {
    #[serde(default)]
    pub name: Option<String>,
    /// A command line, run without a shell.
    #[serde(default)]
    pub run: Option<String>,
    /// Or a built-in action: create_database, start_service, write_file, ...
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub with: BTreeMap<String, Scalar>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// §85: skip the step unless this holds.
    #[serde(default, rename = "if")]
    pub cond: Option<String>,
    /// §92: needs administrator rights — asks for its own confirmation.
    #[serde(default)]
    pub elevated: bool,
    #[serde(default)]
    pub allow_failure: bool,
}

impl Step {
    pub fn spec(&self) -> StepSpec {
        match self {
            Step::Line(line) => StepSpec {
                run: Some(line.clone()),
                ..Default::default()
            },
            Step::Full(spec) => spec.clone(),
        }
    }
}

/// §85 top-level conditional commands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    #[serde(rename = "if")]
    pub cond: String,
    #[serde(default)]
    pub commands: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileSpec {
    /// Relative to the project folder.
    pub path: String,
    pub content: String,
    #[serde(default, rename = "if")]
    pub cond: Option<String>,
    #[serde(default = "yes")]
    pub overwrite: bool,
}

fn yes() -> bool {
    true
}

/// A key written into a dotenv-style file (§103).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvVar {
    #[serde(default = "dot_env")]
    pub file: String,
    pub key: String,
    pub value: String,
    #[serde(default, rename = "if")]
    pub cond: Option<String>,
}

fn dot_env() -> String {
    ".env".to_string()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppCommand {
    /// Command line for the dev server, e.g. `npm run dev`.
    pub run: String,
    /// Runtime whose bin dir goes first on PATH: "node" | "python" | "php".
    #[serde(default)]
    pub runtime: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default, rename = "if")]
    pub cond: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DomainSpec {
    pub hostname: String,
    /// "php" | "proxy" | "static"
    #[serde(default = "php_kind")]
    pub kind: String,
    /// Document root, templated. Defaults to the project folder.
    #[serde(default)]
    pub root: Option<String>,
    /// Upstream port for `proxy` sites, templated.
    #[serde(default)]
    pub port: Option<String>,
    /// Upstream host for `proxy` sites (blank = this machine), templated.
    #[serde(default)]
    pub host: Option<String>,
    /// "true" when the upstream only speaks HTTPS, templated.
    #[serde(default)]
    pub upstream_https: Option<String>,
    #[serde(default)]
    pub app: Option<AppCommand>,
    #[serde(default, rename = "if")]
    pub cond: Option<String>,
}

fn php_kind() -> String {
    "php".to_string()
}

/// A requirement is either a version string (`php: "8.4"`) or a flag (`redis: true`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Requirement {
    Flag(bool),
    Version(Scalar),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Environment {
    #[serde(default)]
    pub https: Option<bool>,
    #[serde(default)]
    pub mailpit: Option<bool>,
    #[serde(default)]
    pub wildcard: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuickApp {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// "php" | "node" | "python" | "static" | "custom" — drives the gallery filter.
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub requirements: BTreeMap<String, Requirement>,
    #[serde(default, deserialize_with = "de_variables")]
    pub variables: Vec<Variable>,
    /// Project creation commands (§79).
    #[serde(default)]
    pub commands: Vec<Step>,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(default)]
    pub files: Vec<FileSpec>,
    #[serde(default)]
    pub env_vars: Vec<EnvVar>,
    #[serde(default)]
    pub install: Vec<Step>,
    // §86
    #[serde(default)]
    pub pre_create: Vec<Step>,
    #[serde(default)]
    pub post_create: Vec<Step>,
    #[serde(default)]
    pub pre_install: Vec<Step>,
    #[serde(default)]
    pub post_install: Vec<Step>,
    #[serde(default)]
    pub pre_start: Vec<Step>,
    #[serde(default)]
    pub post_start: Vec<Step>,
    #[serde(default)]
    pub environment: Environment,
    #[serde(default)]
    pub domain: Option<DomainSpec>,
}

pub fn parse(yaml: &str) -> Result<QuickApp, String> {
    let app: QuickApp =
        serde_yaml_ng::from_str(yaml).map_err(|e| format!("invalid Quick App YAML: {e}"))?;
    validate(&app)?;
    Ok(app)
}

pub fn to_yaml(app: &QuickApp) -> Result<String, String> {
    serde_yaml_ng::to_string(app).map_err(|e| e.to_string())
}

/// Ids become file names and log-source names, so keep them boring.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-')
}

pub fn validate(app: &QuickApp) -> Result<(), String> {
    if !valid_id(&app.id) {
        return Err(format!(
            "id \"{}\" must be lowercase letters, digits and dashes",
            app.id
        ));
    }
    if app.name.trim().is_empty() {
        return Err("name is required".into());
    }
    let mut seen = std::collections::HashSet::new();
    for v in &app.variables {
        if v.name.is_empty()
            || !v
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(format!(
                "variable name \"{}\" must be letters, digits and underscores",
                v.name
            ));
        }
        if !seen.insert(v.name.clone()) {
            return Err(format!("variable \"{}\" is defined twice", v.name));
        }
        if !VARIABLE_TYPES.contains(&v.var_type.as_str()) {
            return Err(format!(
                "variable \"{}\" has unknown type \"{}\"",
                v.name, v.var_type
            ));
        }
        if matches!(v.var_type.as_str(), "select" | "multiselect") && v.options.is_empty() {
            return Err(format!("select variable \"{}\" needs options", v.name));
        }
        if let Some(pattern) = &v.validation {
            regex::Regex::new(pattern).map_err(|e| {
                format!(
                    "variable \"{}\" has an invalid validation pattern: {e}",
                    v.name
                )
            })?;
        }
    }
    let all_steps = app
        .commands
        .iter()
        .chain(&app.install)
        .chain(&app.pre_create)
        .chain(&app.post_create)
        .chain(&app.pre_install)
        .chain(&app.post_install)
        .chain(&app.pre_start)
        .chain(&app.post_start)
        .chain(app.conditions.iter().flat_map(|c| c.commands.iter()));
    for step in all_steps {
        let spec = step.spec();
        match (&spec.run, &spec.action) {
            (Some(_), None) | (None, Some(_)) => {}
            _ => return Err("every step needs exactly one of `run` or `action`".into()),
        }
    }
    if let Some(d) = &app.domain {
        // `kind` may be a template ("{{ kind }}"); those are checked when the plan is built.
        if !d.kind.contains("{{") && !matches!(d.kind.as_str(), "php" | "proxy" | "static") {
            return Err(format!(
                "domain kind \"{}\" must be php, proxy or static",
                d.kind
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_srs_section_152_example_with_map_style_variables() {
        let yaml = r#"
id: laravel-shop
name: Laravel Shop
description: Create a Laravel application with MySQL, Redis and Mailpit
requirements:
  php: "8.4"
  node: "22"
  mysql: "8.4"
  redis: true
  mailpit: true
variables:
  project_name:
    type: text
    label: Project Name
    required: true
  domain:
    type: domain
    label: Domain
    default: "{{project_name}}.test"
commands:
  - composer create-project laravel/laravel "{{project_name}}"
post_create:
  - php artisan key:generate
  - php artisan migrate
environment:
  https: true
  mailpit: true
"#;
        let app = parse(yaml).unwrap();
        assert_eq!(app.variables.len(), 2);
        assert_eq!(
            app.variables[0].name, "project_name",
            "map key becomes the variable name, in order"
        );
        assert!(app.variables[0].required);
        assert_eq!(
            app.variables[1].default,
            Some(Scalar::Str("{{project_name}}.test".into()))
        );
        assert_eq!(
            app.requirements["php"],
            Requirement::Version(Scalar::Str("8.4".into()))
        );
        assert_eq!(app.requirements["redis"], Requirement::Flag(true));
        assert_eq!(app.commands.len(), 1);
        assert_eq!(
            app.post_create[1].spec().run.as_deref(),
            Some("php artisan migrate")
        );
        assert_eq!(app.environment.https, Some(true));
    }

    #[test]
    fn list_style_variables_conditions_and_full_steps_parse() {
        let yaml = r#"
id: demo
name: Demo
variables:
  - name: database
    type: select
    options: [none, mysql, mariadb]
    default: mysql
  - name: db_name
    show_if: "database != none"
    validation: "^[a-z_]+$"
conditions:
  - if: "database == mysql"
    commands:
      - action: create_database
        with: { engine: mysql }
commands:
  - name: Say hi
    run: echo hi
    allow_failure: true
"#;
        let app = parse(yaml).unwrap();
        assert_eq!(app.variables[0].options.len(), 3);
        assert_eq!(
            app.variables[1].show_if.as_deref(),
            Some("database != none")
        );
        assert_eq!(
            app.conditions[0].commands[0].spec().action.as_deref(),
            Some("create_database")
        );
        assert!(app.commands[0].spec().allow_failure);
    }

    #[test]
    fn rejects_bad_definitions_with_a_clear_reason() {
        assert!(parse("id: Bad Id\nname: x")
            .unwrap_err()
            .contains("lowercase"));
        assert!(
            parse("id: ok\nname: x\nvariables:\n  - name: a\n    type: wat")
                .unwrap_err()
                .contains("unknown type")
        );
        assert!(
            parse("id: ok\nname: x\nvariables:\n  - name: a\n  - name: a")
                .unwrap_err()
                .contains("twice")
        );
        assert!(
            parse("id: ok\nname: x\nvariables:\n  - name: a\n    type: select")
                .unwrap_err()
                .contains("options")
        );
        assert!(
            parse("id: ok\nname: x\nvariables:\n  - name: a\n    validation: '('")
                .unwrap_err()
                .contains("validation")
        );
        assert!(parse("id: ok\nname: x\ncommands:\n  - name: nothing")
            .unwrap_err()
            .contains("run"));
        assert!(
            parse("id: ok\nname: x\ncommands:\n  - run: a\n    action: b")
                .unwrap_err()
                .contains("run")
        );
        assert!(
            parse("id: ok\nname: x\ndomain: { hostname: a.test, kind: cgi }")
                .unwrap_err()
                .contains("kind")
        );
    }

    #[test]
    fn round_trips_through_yaml() {
        let app = parse("id: demo\nname: Demo\ncommands:\n  - echo hi\n").unwrap();
        assert_eq!(parse(&to_yaml(&app).unwrap()).unwrap(), app);
    }
}
