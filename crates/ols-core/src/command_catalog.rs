//! The commands a project's own tools offer (§89 extended): every `php artisan` /
//! `bin/console` / `composer` command with its arguments and options, the package.json
//! scripts and Django's `manage.py` commands. The Commands page lists them, builds a form
//! from the definition and runs the result like any other command line.
//!
//! This file only parses; `project_tools.rs` runs the tools that print the lists.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandArgument {
    pub name: String,
    pub description: String,
    pub required: bool,
    /// Takes several values (`is_array`).
    pub multiple: bool,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandOption {
    /// With the dashes: "--force".
    pub name: String,
    pub shortcut: Option<String>,
    pub description: String,
    /// False = a plain on/off flag.
    pub accepts_value: bool,
    pub value_required: bool,
    pub multiple: bool,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DiscoveredCommand {
    pub name: String,
    pub description: String,
    pub help: String,
    pub arguments: Vec<CommandArgument>,
    pub options: Vec<CommandOption>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandSource {
    /// "artisan", "console", "composer", "scripts" or "django".
    pub id: String,
    pub label: String,
    /// What goes before the command's name: ["php", "artisan"], ["npm", "run"], ...
    pub prefix: Vec<String>,
    pub commands: Vec<DiscoveredCommand>,
    /// Why the list is empty, when the tool failed to print it.
    pub error: Option<String>,
    /// The list is there but second-best: the tool failed and the commands were read from
    /// the source files instead. Says why, so the page can show it next to the list.
    #[serde(default)]
    pub warning: Option<String>,
}

/// Options every Symfony console application has. They only clutter a form (output
/// verbosity, colours, `--help`), so they are left out; the free "extra arguments" field
/// still takes them.
const GLOBAL_OPTIONS: &[&str] = &[
    "--help",
    "--quiet",
    "--silent",
    "--verbose",
    "--version",
    "--ansi",
    "--no-ansi",
    "--no-interaction",
    "--profile",
    "--working-dir",
    "--no-plugins",
    "--no-cache",
];

fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn default_text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Null | Value::Bool(false) => None,
        Value::String(s) if s.is_empty() => None,
        Value::String(s) => Some(s.clone()),
        Value::Array(a) if a.is_empty() => None,
        other => Some(other.to_string()),
    }
}

/// PHP writes an empty map as `[]`, so a definition section is an object or an empty list.
fn entries(v: Option<&Value>) -> Vec<&Value> {
    match v {
        Some(Value::Object(m)) => m.values().collect(),
        Some(Value::Array(a)) => a.iter().collect(),
        _ => vec![],
    }
}

/// Finds the JSON document in a tool's output. PHP notices and deprecation warnings are
/// often printed before it, and some tools add a trailing newline banner after it.
pub fn extract_json(output: &str) -> Option<Value> {
    let mut rest = output;
    while let Some(start) = rest.find('{') {
        let candidate = &rest[start..];
        if let Some(Ok(v)) = serde_json::Deserializer::from_str(candidate)
            .into_iter::<Value>()
            .next()
        {
            return Some(v);
        }
        rest = &candidate[1..];
    }
    None
}

/// Parses `list --format=json` from any Symfony console application (Laravel's artisan,
/// Symfony's bin/console, Composer). Hidden commands are skipped.
pub fn parse_symfony_list(output: &str) -> Result<Vec<DiscoveredCommand>, String> {
    let json = extract_json(output).ok_or("the command list was not valid JSON")?;
    let commands = json
        .get("commands")
        .and_then(Value::as_array)
        .ok_or("the command list has no \"commands\"")?;
    let mut out: Vec<DiscoveredCommand> = commands
        .iter()
        .filter(|c| !c.get("hidden").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|c| {
            let name = text(c.get("name"));
            if name.is_empty() || name == "_complete" || name == "completion" {
                return None;
            }
            let def = c.get("definition");
            let arguments = entries(def.and_then(|d| d.get("arguments")))
                .into_iter()
                .map(|a| CommandArgument {
                    name: text(a.get("name")),
                    description: text(a.get("description")),
                    required: a
                        .get("is_required")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    multiple: a.get("is_array").and_then(Value::as_bool).unwrap_or(false),
                    default: default_text(a.get("default")),
                })
                .filter(|a| !a.name.is_empty() && a.name != "command")
                .collect();
            let options = entries(def.and_then(|d| d.get("options")))
                .into_iter()
                .map(|o| CommandOption {
                    name: text(o.get("name")),
                    shortcut: Some(text(o.get("shortcut"))).filter(|s| !s.is_empty()),
                    description: text(o.get("description")),
                    accepts_value: o
                        .get("accept_value")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    value_required: o
                        .get("is_value_required")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    multiple: o
                        .get("is_multiple")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    default: default_text(o.get("default")),
                })
                .filter(|o| o.name.starts_with("--") && !GLOBAL_OPTIONS.contains(&o.name.as_str()))
                .collect();
            Some(DiscoveredCommand {
                name,
                description: text(c.get("description")),
                help: text(c.get("help")),
                arguments,
                options,
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// package.json `scripts`, by name; the description is the script itself.
pub fn package_scripts(package_json: &str) -> Vec<DiscoveredCommand> {
    let Ok(json) = serde_json::from_str::<Value>(package_json) else {
        return vec![];
    };
    let Some(Value::Object(scripts)) = json.get("scripts") else {
        return vec![];
    };
    scripts
        .iter()
        .map(|(name, body)| DiscoveredCommand {
            name: name.clone(),
            description: body.as_str().unwrap_or_default().to_string(),
            ..Default::default()
        })
        .collect()
}

/// `python manage.py help --commands`: one name per line.
pub fn parse_name_list(output: &str) -> Vec<DiscoveredCommand> {
    let mut names: BTreeMap<String, ()> = BTreeMap::new();
    for line in output.lines().map(str::trim) {
        if !line.is_empty()
            && line
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ':')
        {
            names.insert(line.to_string(), ());
        }
    }
    names
        .into_keys()
        .map(|name| DiscoveredCommand {
            name,
            ..Default::default()
        })
        .collect()
}

/// How a package.json script runs with the project's package manager.
pub fn script_prefix(manager: Option<&str>) -> Vec<String> {
    vec![manager.unwrap_or("npm").to_string(), "run".to_string()]
}

// ------------------------------------------------------- artisan without booting

/// Removes terminal colour codes; artisan's error pages are full of them.
pub fn strip_ansi(s: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").unwrap())
        .replace_all(s, "")
        .into_owned()
}

/// One plain sentence for the usual reasons a Laravel app fails to start from the
/// command line, or `None` when the error isn't one of them.
pub fn boot_failure_hint(output: &str) -> Option<&'static str> {
    let o = output.to_ascii_lowercase();
    Some(if o.contains("could not find driver") {
        "The PHP this project uses has no database driver (pdo_mysql / pdo_pgsql) switched on. Enable it for that PHP version, or give the project a PHP that has it."
    } else if o.contains("unknown database") {
        "The database in the project's .env does not exist yet. Create it on the Databases page."
    } else if o.contains("[2002]")
        || o.contains("connection refused")
        || o.contains("actively refused")
    {
        "The database server is not running. Start it on the Services page."
    } else if o.contains("access denied for user") {
        "The database user or password in the project's .env is wrong."
    } else if o.contains("vendor/autoload.php") || o.contains("vendor\\autoload.php") {
        "The project's dependencies are not installed. Run composer install."
    } else if o.contains("no application encryption key") {
        "The project has no APP_KEY. Run php artisan key:generate."
    } else {
        return None;
    })
}

fn php_string(raw: &str) -> String {
    raw.replace("\\'", "'")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

/// Reads a class property's string value: `protected $field = '...'` (single or double
/// quoted, may span lines). Local variables of the same name don't count.
fn php_property(src: &str, field: &str) -> Option<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r#"(?s)(?:protected|public|private)\s+(?:static\s+)?(?:\??\w+\s+)?\$(signature|name|description)\s*=\s*(?:'((?:[^'\\]|\\.)*)'|"((?:[^"\\]|\\.)*)")"#).unwrap()
    });
    let c = re.captures_iter(src).find(|c| &c[1] == field)?;
    Some(php_string(c.get(2).or_else(|| c.get(3))?.as_str()))
}

/// `#[AsCommand(name: 'x', description: 'y')]`.
fn as_command(src: &str, key: &str) -> Option<String> {
    static ATTR: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static ARG: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let attr = ATTR
        .get_or_init(|| regex::Regex::new(r"(?s)#\[AsCommand\((.*?)\)\]").unwrap())
        .captures(src)?
        .get(1)?
        .as_str();
    let arg = ARG.get_or_init(|| {
        regex::Regex::new(r"(name|description)\s*:\s*'((?:[^'\\]|\\.)*)'").unwrap()
    });
    let c = arg.captures_iter(attr).find(|c| &c[1] == key)?;
    Some(php_string(&c[2]))
}

/// Parses a Laravel command signature: `migrate {name} {user?} {ids*} {--force : Why}
/// {--Q|queue=default}` into the command name, its arguments and its options.
pub fn parse_signature(signature: &str) -> (String, Vec<CommandArgument>, Vec<CommandOption>) {
    let name = signature
        .split(|c: char| c.is_whitespace() || c == '{')
        .next()
        .unwrap_or_default()
        .to_string();
    let mut args = Vec::new();
    let mut opts = Vec::new();
    // Braces don't nest in signatures, but descriptions may contain "{" so walk by depth.
    let mut depth = 0;
    let mut cur = String::new();
    for ch in signature.chars() {
        match ch {
            '{' => {
                if depth > 0 {
                    cur.push(ch);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    parse_token(&std::mem::take(&mut cur), &mut args, &mut opts);
                } else {
                    cur.push(ch);
                }
            }
            _ if depth > 0 => cur.push(ch),
            _ => {}
        }
    }
    (name, args, opts)
}

fn parse_token(token: &str, args: &mut Vec<CommandArgument>, opts: &mut Vec<CommandOption>) {
    let (spec, description) = match token.split_once(" : ") {
        Some((s, d)) => (s.trim(), d.split_whitespace().collect::<Vec<_>>().join(" ")),
        None => (token.trim(), String::new()),
    };
    if let Some(opt) = spec.strip_prefix("--") {
        let (shortcut, rest) = match opt.split_once('|') {
            Some((s, r)) => (Some(format!("-{s}")), r),
            None => (None, opt),
        };
        let (name, value) = match rest.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (rest, None),
        };
        let multiple = value.is_some_and(|v| v.starts_with('*'));
        let default = value
            .map(|v| v.trim_start_matches('*').to_string())
            .filter(|v| !v.is_empty());
        opts.push(CommandOption {
            name: format!("--{}", name.trim()),
            shortcut,
            description,
            accepts_value: value.is_some(),
            value_required: false,
            multiple,
            default,
        });
    } else {
        let (name, default) = match spec.split_once('=') {
            Some((n, d)) => (
                n.trim(),
                Some(d.trim().to_string()).filter(|d| !d.is_empty()),
            ),
            None => (spec, None),
        };
        let multiple = name.ends_with('*');
        let optional = name.ends_with('?') || default.is_some() || name.ends_with("?*");
        let name = name.trim_end_matches(['?', '*']).to_string();
        if !name.is_empty() {
            args.push(CommandArgument {
                name,
                description,
                required: !optional,
                multiple,
                default,
            });
        }
    }
}

/// The rows of `getOptions()` / `getArguments()`: `['name', 'c', InputOption::VALUE_NONE, 'Why']`.
fn array_rows<'a>(src: &'a str, method: &str) -> Vec<Vec<&'a str>> {
    let Some(start) = src.find(&format!("function {method}(")) else {
        return vec![];
    };
    let body = &src[start..];
    let Some(end) = body.find("\n    }") else {
        return vec![];
    };
    static ROW: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static CELL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let row = ROW.get_or_init(|| regex::Regex::new(r"(?m)^\s*\[(.+)\],?\s*$").unwrap());
    let cell = CELL.get_or_init(|| {
        regex::Regex::new(r"'((?:[^'\\]|\\.)*)'|([A-Za-z_:|\s]+|null|\d+)").unwrap()
    });
    row.captures_iter(&body[..end])
        .map(|c| {
            cell.captures_iter(c.get(1).unwrap().as_str())
                .filter_map(|m| m.get(1).or_else(|| m.get(2)).map(|x| x.as_str().trim()))
                .filter(|x| !x.is_empty())
                .collect()
        })
        .collect()
}

/// A Laravel command class read as text, for when the app can't boot to list itself.
/// Returns `None` for files that aren't a concrete command.
pub fn parse_command_source(src: &str) -> Option<DiscoveredCommand> {
    if !src.contains("extends ")
        || src.contains("abstract class")
        || !(src.contains("$signature") || src.contains("$name") || src.contains("AsCommand"))
    {
        return None;
    }
    let (name, mut arguments, mut options) = if let Some(sig) = php_property(src, "signature") {
        parse_signature(sig.trim())
    } else {
        let name = as_command(src, "name").or_else(|| php_property(src, "name"))?;
        (name, vec![], vec![])
    };
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_' | '.'))
    {
        return None;
    }
    for row in array_rows(src, "getArguments") {
        if let [name, mode, rest @ ..] = row.as_slice() {
            arguments.push(CommandArgument {
                name: php_string(name),
                description: rest.first().map(|d| php_string(d)).unwrap_or_default(),
                required: mode.contains("REQUIRED"),
                multiple: mode.contains("IS_ARRAY"),
                default: None,
            });
        }
    }
    if arguments.is_empty() && src.contains("extends GeneratorCommand") {
        arguments.push(CommandArgument {
            name: "name".into(),
            description: "The name of the class".into(),
            required: true,
            ..Default::default()
        });
    }
    for row in array_rows(src, "getOptions") {
        if let [name, shortcut, mode, rest @ ..] = row.as_slice() {
            options.push(CommandOption {
                name: format!("--{}", php_string(name)),
                shortcut: Some(*shortcut)
                    .filter(|s| *s != "null" && !s.is_empty())
                    .map(|s| format!("-{}", php_string(s))),
                description: rest.first().map(|d| php_string(d)).unwrap_or_default(),
                accepts_value: !mode.contains("VALUE_NONE"),
                value_required: mode.contains("VALUE_REQUIRED"),
                multiple: mode.contains("IS_ARRAY"),
                default: None,
            });
        }
    }
    let description = php_property(src, "description")
        .or_else(|| as_command(src, "description"))
        .unwrap_or_default();
    Some(DiscoveredCommand {
        name,
        description,
        help: String::new(),
        arguments,
        options,
    })
}

/// Where a Laravel project's commands live: its own `app/` and every package Laravel
/// auto-discovers (`extra.laravel` in vendor/composer/installed.json), framework included.
pub fn laravel_command_dirs(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut dirs = vec![
        root.join("app"),
        root.join("vendor/laravel/framework/src/Illuminate"),
    ];
    let composer_dir = root.join("vendor/composer");
    if let Some(json) = std::fs::read_to_string(composer_dir.join("installed.json"))
        .ok()
        .and_then(|r| serde_json::from_str::<Value>(&r).ok())
    {
        let packages = json
            .get("packages")
            .and_then(Value::as_array)
            .cloned()
            .or_else(|| json.as_array().cloned())
            .unwrap_or_default();
        for p in packages {
            if p.pointer("/extra/laravel").is_some() {
                if let Some(path) = p.get("install-path").and_then(Value::as_str) {
                    dirs.push(composer_dir.join(path));
                }
            }
        }
    }
    dirs.retain(|d| d.is_dir());
    dirs.dedup();
    dirs
}

/// Every command class under `dirs`, by name. Skips tests, stubs and fixtures.
pub fn scan_command_sources(dirs: &[std::path::PathBuf]) -> Vec<DiscoveredCommand> {
    fn walk(dir: &std::path::Path, depth: usize, out: &mut BTreeMap<String, DiscoveredCommand>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if depth < 12
                    && !matches!(
                        name.to_ascii_lowercase().as_str(),
                        "tests" | "test" | "stubs" | "fixtures" | "node_modules" | "resources"
                    )
                {
                    walk(&path, depth + 1, out);
                }
            } else if name.ends_with(".php") {
                let in_console = path.components().any(|c| {
                    matches!(
                        c.as_os_str().to_str(),
                        Some("Console" | "Commands" | "Command")
                    )
                });
                if !(name.ends_with("Command.php") || in_console) {
                    continue;
                }
                if let Some(cmd) = std::fs::read_to_string(&path)
                    .ok()
                    .as_deref()
                    .and_then(parse_command_source)
                {
                    out.entry(cmd.name.clone()).or_insert(cmd);
                }
            }
        }
    }
    let mut found = BTreeMap::new();
    for d in dirs {
        walk(d, 0, &mut found);
    }
    found.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTISAN: &str = r#"PHP Deprecated:  something {weird} in vendor/x.php
{"application":{"name":"Laravel Framework","version":"11.0.0"},"commands":[
 {"name":"migrate","hidden":false,"usage":["migrate [--force]"],"description":"Run the database migrations","help":"","definition":{
   "arguments":[],
   "options":{
     "help":{"name":"--help","shortcut":"-h","accept_value":false,"is_value_required":false,"is_multiple":false,"description":"Display help","default":false},
     "force":{"name":"--force","shortcut":"","accept_value":false,"is_value_required":false,"is_multiple":false,"description":"Force the operation","default":false},
     "path":{"name":"--path","shortcut":"","accept_value":true,"is_value_required":true,"is_multiple":true,"description":"The path(s)","default":[]},
     "step":{"name":"--step","shortcut":"","accept_value":true,"is_value_required":false,"is_multiple":false,"description":"Steps","default":null}
   }}},
 {"name":"make:model","hidden":false,"description":"Create a new Eloquent model class","help":"","definition":{
   "arguments":{"name":{"name":"name","is_required":true,"is_array":false,"description":"The name of the model","default":null}},
   "options":[]}},
 {"name":"secret:thing","hidden":true,"description":"","definition":{"arguments":[],"options":[]}},
 {"name":"_complete","hidden":false,"description":"","definition":{"arguments":[],"options":[]}}
],"namespaces":[]}
"#;

    #[test]
    fn parses_symfony_lists_through_leading_notices() {
        let cmds = parse_symfony_list(ARTISAN).unwrap();
        let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["make:model", "migrate"],
            "hidden and completion commands are skipped, sorted"
        );

        let model = &cmds[0];
        assert_eq!(
            model.arguments,
            [CommandArgument {
                name: "name".into(),
                description: "The name of the model".into(),
                required: true,
                multiple: false,
                default: None
            }]
        );
        assert!(model.options.is_empty());

        let migrate = &cmds[1];
        let opts: Vec<&str> = migrate.options.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(
            opts,
            ["--force", "--path", "--step"],
            "global options like --help are dropped"
        );
        assert!(!migrate.options[0].accepts_value && migrate.options[0].shortcut.is_none());
        assert!(
            migrate.options[1].accepts_value
                && migrate.options[1].value_required
                && migrate.options[1].multiple
        );
        assert_eq!(migrate.options[1].default, None);
    }

    #[test]
    fn rejects_output_without_json() {
        assert!(parse_symfony_list("PHP Fatal error: Class \"X\" not found").is_err());
    }

    #[test]
    fn reads_package_scripts_and_picks_the_runner() {
        let scripts = package_scripts(r#"{"scripts":{"dev":"vite","build":"vite build"}}"#);
        assert_eq!(scripts.len(), 2);
        assert_eq!(
            scripts
                .iter()
                .find(|c| c.name == "build")
                .unwrap()
                .description,
            "vite build"
        );
        assert!(package_scripts("not json").is_empty());
        assert_eq!(script_prefix(Some("pnpm")), ["pnpm", "run"]);
        assert_eq!(script_prefix(None), ["npm", "run"]);
    }

    #[test]
    fn parses_laravel_signatures() {
        let (name, args, opts) = parse_signature(
            "mail:send {user : The user} {ids?*} {--Q|queue=default : Which queue}\n                {--force : Force it} {--tag=* : Tags}",
        );
        assert_eq!(name, "mail:send");
        assert_eq!(args.len(), 2);
        assert!(args[0].required && !args[0].multiple && args[0].description == "The user");
        assert!(!args[1].required && args[1].multiple && args[1].name == "ids");
        assert_eq!(
            opts[0],
            CommandOption {
                name: "--queue".into(),
                shortcut: Some("-Q".into()),
                description: "Which queue".into(),
                accepts_value: true,
                value_required: false,
                multiple: false,
                default: Some("default".into())
            }
        );
        assert!(!opts[1].accepts_value);
        assert!(opts[2].accepts_value && opts[2].multiple && opts[2].default.is_none());
    }

    #[test]
    fn reads_command_classes_without_booting_the_app() {
        let signature = r#"<?php
class BackupAutoRun extends Command
{
    protected $signature = 'backup:auto {--dry : Only show what would happen}';
    protected $description = 'Runs the scheduled backup';
    public function handle() { $name = 'not-a-command'; }
}"#;
        let cmd = parse_command_source(signature).unwrap();
        assert_eq!(
            (cmd.name.as_str(), cmd.description.as_str()),
            ("backup:auto", "Runs the scheduled backup")
        );
        assert_eq!(cmd.options[0].name, "--dry");

        let generator = r#"<?php
#[AsCommand(name: 'make:model')]
class ModelMakeCommand extends GeneratorCommand
{
    protected $name = 'make:model';
    protected $description = 'Create a new Eloquent model class';
    protected function getOptions()
    {
        return [
            ['all', 'a', InputOption::VALUE_NONE, 'Generate everything'],
            ['force', null, InputOption::VALUE_NONE, 'Create the class even if the model already exists'],
            ['path', null, InputOption::VALUE_REQUIRED, 'Where to put it'],
        ];
    }
}"#;
        let cmd = parse_command_source(generator).unwrap();
        assert_eq!(cmd.name, "make:model");
        assert_eq!(
            cmd.arguments[0].name, "name",
            "generator commands take a class name"
        );
        assert_eq!(cmd.options.len(), 3);
        assert_eq!(cmd.options[0].shortcut.as_deref(), Some("-a"));
        assert!(cmd.options[1].shortcut.is_none() && !cmd.options[1].accepts_value);
        assert!(cmd.options[2].accepts_value && cmd.options[2].value_required);

        assert!(parse_command_source(
            "<?php abstract class Base extends Command { protected $signature = 'x'; }"
        )
        .is_none());
        assert!(parse_command_source("<?php class Plain { }").is_none());
    }

    #[test]
    fn explains_common_boot_failures() {
        let out = strip_ansi(
            "\x1b[41;1m QueryException \x1b[49;22m could not find driver (Connection: mysql)",
        );
        assert!(!out.contains('\x1b'));
        assert!(boot_failure_hint(&out).unwrap().contains("pdo_mysql"));
        assert!(
            boot_failure_hint("SQLSTATE[HY000] [1049] Unknown database 'gymos'")
                .unwrap()
                .contains("Databases page")
        );
        assert!(boot_failure_hint("something else").is_none());
    }

    #[test]
    fn reads_django_name_lists() {
        let out = "migrate\nrunserver\n\n  shell\nWarning: something went wrong\n";
        let names: Vec<String> = parse_name_list(out).into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["migrate", "runserver", "shell"]);
    }
}
