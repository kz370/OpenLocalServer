//! External DB GUI tool detection (§102, extended per user request — Stage 5).
//! Detect first; only ever consider a download once nothing is found (§126 applied
//! generally: never assume, never silently modify or replace an existing install).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTool {
    pub id: String,
    pub name: String,
    pub found_path: Option<String>,
    pub engines: Vec<String>,
}

pub fn detect_db_tools() -> Vec<DbTool> {
    vec![
        DbTool {
            id: "letos".into(),
            name: "Letos DB Browser".into(),
            found_path: find_letos(),
            // Letos is a SQLite manager (no network-database plugins): it only opens files.
            engines: vec!["sqlite".into()],
        },
        DbTool {
            id: "dbbrowser".into(),
            name: "DB Browser for SQLite".into(),
            found_path: find_dbbrowser(),
            engines: vec!["sqlite".into()],
        },
        DbTool {
            id: "heidisql".into(),
            name: "HeidiSQL".into(),
            found_path: find_heidisql(),
            engines: vec!["mariadb".into(), "postgres".into(), "sqlite".into()],
        },
        DbTool {
            id: "dbeaver".into(),
            name: "DBeaver".into(),
            found_path: find_dbeaver(),
            engines: vec!["mariadb".into(), "postgres".into(), "sqlite".into()],
        },
        DbTool {
            id: "pgadmin".into(),
            name: "pgAdmin 4".into(),
            found_path: find_pgadmin(),
            engines: vec!["postgres".into()],
        },
        DbTool {
            id: "nosqlbooster".into(),
            name: "NoSQLBooster for MongoDB".into(),
            found_path: find_nosqlbooster(),
            engines: vec!["mongodb".into()],
        },
        DbTool {
            id: "tinyrdm".into(),
            name: "Tiny RDM".into(),
            found_path: find_tinyrdm(),
            engines: vec!["redis".into()],
        },
    ]
}

fn find_on_path(exe_name: &str) -> Option<String> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(exe_name))
        .find(|p| p.is_file())
        .map(|p| p.display().to_string())
}

fn find_heidisql() -> Option<String> {
    for base in [
        r"C:\Program Files\HeidiSQL",
        r"C:\Program Files (x86)\HeidiSQL",
    ] {
        let candidate = PathBuf::from(base).join("heidisql.exe");
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    find_on_path("heidisql.exe")
}

/// pgAdmin 4 installs under a version-numbered subfolder (`pgAdmin 4\v8\runtime\...`),
/// so this scans the parent instead of guessing the version.
fn find_pgadmin() -> Option<String> {
    for base in [
        r"C:\Program Files\pgAdmin 4",
        r"C:\Program Files (x86)\pgAdmin 4",
    ] {
        let Ok(entries) = std::fs::read_dir(base) else {
            continue;
        };
        for entry in entries.flatten() {
            let candidate = entry.path().join("runtime").join("pgAdmin4.exe");
            if candidate.is_file() {
                return Some(candidate.display().to_string());
            }
        }
    }
    find_on_path("pgAdmin4.exe")
}

fn find_nosqlbooster() -> Option<String> {
    let exe = "NoSQLBooster for MongoDB.exe";
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let candidate = PathBuf::from(local_app_data)
            .join("Programs")
            .join("nosqlbooster4mongo")
            .join(exe);
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    for base in [
        PathBuf::from(r"C:\Program Files\nosqlbooster4mongo"),
        PathBuf::from(r"C:\Program Files\NoSQLBooster for MongoDB"),
        PathBuf::from(r"C:\Program Files (x86)\nosqlbooster4mongo"),
    ] {
        let candidate = base.join(exe);
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    find_on_path(exe)
}

fn find_dbbrowser() -> Option<String> {
    for base in [
        r"C:\Program Files\DB Browser for SQLite",
        r"C:\Program Files (x86)\DB Browser for SQLite",
    ] {
        for exe in ["DB Browser for SQLite.exe", "sqlitebrowser.exe"] {
            let candidate = PathBuf::from(base).join(exe);
            if candidate.is_file() {
                return Some(candidate.display().to_string());
            }
        }
    }
    find_on_path("DB Browser for SQLite.exe").or_else(|| find_on_path("sqlitebrowser.exe"))
}

fn find_tinyrdm() -> Option<String> {
    const EXES: &[&str] = &["Tiny RDM.exe", "tiny-rdm.exe", "TinyRDM.exe"];
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        for exe in EXES {
            let candidate = PathBuf::from(&local_app_data)
                .join("Programs")
                .join("Tiny RDM")
                .join(exe);
            if candidate.is_file() {
                return Some(candidate.display().to_string());
            }
        }
    }
    for base in [
        r"C:\Program Files\Tiny RDM",
        r"C:\Program Files (x86)\Tiny RDM",
    ] {
        for exe in EXES {
            let candidate = PathBuf::from(base).join(exe);
            if candidate.is_file() {
                return Some(candidate.display().to_string());
            }
        }
    }
    EXES.iter().find_map(|exe| find_on_path(exe))
}

fn find_letos() -> Option<String> {
    for base in [
        r"C:\Program Files\Letos",
        r"C:\Program Files (x86)\Letos",
        r"C:\Program Files\SQLiteStudio",
        r"C:\Program Files (x86)\SQLiteStudio",
    ] {
        for exe in ["letos.exe", "sqlitestudio.exe"] {
            let candidate = PathBuf::from(base).join(exe);
            if candidate.is_file() {
                return Some(candidate.display().to_string());
            }
        }
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        for sub in ["Programs\\Letos", "Programs\\SQLiteStudio"] {
            for exe in ["letos.exe", "sqlitestudio.exe"] {
                let candidate = PathBuf::from(&local_app_data).join(sub).join(exe);
                if candidate.is_file() {
                    return Some(candidate.display().to_string());
                }
            }
        }
    }
    find_on_path("letos.exe").or_else(|| find_on_path("sqlitestudio.exe"))
}

fn find_dbeaver() -> Option<String> {
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let candidate = PathBuf::from(local_app_data)
            .join("Programs")
            .join("DBeaver")
            .join("dbeaver.exe");
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    for base in [
        r"C:\Program Files\DBeaver",
        r"C:\Program Files (x86)\DBeaver",
    ] {
        let candidate = PathBuf::from(base).join("dbeaver.exe");
        if candidate.is_file() {
            return Some(candidate.display().to_string());
        }
    }
    find_on_path("dbeaver.exe")
}

// ---------------------------------------------------------------------------------------
// §102 external tool configuration (Stage 10): the user can register any tool per engine,
// and "open this database" fills in the connection details for it.

use crate::error::CoreError;
use crate::paths::AppPaths;
use crate::service::ConnectionInfo;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalTool {
    pub id: String,
    pub name: String,
    /// Engines this tool can open: any id in [`KNOWN_ENGINES`]. An empty list means the
    /// tool is offered for every engine, which is what a multi-protocol explorer wants.
    pub engines: Vec<String>,
    #[serde(
        default = "default_launch_mode",
        rename = "launchMode",
        alias = "launch_mode"
    )]
    pub launch_mode: String,
    #[serde(default)]
    pub executable: String,
    /// Arguments with placeholders: {host} {port} {user} {database} {path} {uri}. Either one
    /// list for every engine or a map of engine id to its own list (see [`ToolArgs`]).
    #[serde(default)]
    pub args: ToolArgs,
    /// Calculated availability status (not persisted to disk).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available: Option<bool>,
}

/// Key of a per-engine args map that applies to engines without their own entry.
pub const DEFAULT_ARGS_KEY: &str = "default";

/// A tool's command-line arguments. Most tools take the same arguments for every engine
/// (`["{path}"]`), but some need a different driver or URL form per engine, so the JSON may
/// also be `{"mariadb": [...], "postgres": [...], "sqlite": [...]}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolArgs {
    List(Vec<String>),
    PerEngine(std::collections::BTreeMap<String, Vec<String>>),
}

impl Default for ToolArgs {
    fn default() -> Self {
        ToolArgs::List(Vec::new())
    }
}

impl From<Vec<String>> for ToolArgs {
    fn from(list: Vec<String>) -> Self {
        ToolArgs::List(list)
    }
}

impl ToolArgs {
    /// The argument templates for one engine: its own entry, else `default`, else none.
    pub fn for_engine(&self, engine: &str) -> Vec<String> {
        match self {
            ToolArgs::List(list) => list.clone(),
            ToolArgs::PerEngine(map) => map
                .get(engine)
                .or_else(|| map.get(DEFAULT_ARGS_KEY))
                .cloned()
                .unwrap_or_default(),
        }
    }

    /// True when there are no templates at all, for any engine.
    pub fn is_empty(&self) -> bool {
        match self {
            ToolArgs::List(list) => list.is_empty(),
            ToolArgs::PerEngine(map) => map.values().all(Vec::is_empty),
        }
    }
}

fn default_launch_mode() -> String {
    "args".to_string()
}

/// Every engine id a registered tool may claim. These are the ids the Databases page
/// passes as `engine`, so a tool naming anything else could never be reached from a page
/// — it would save, then never appear in any "Open with" list, which reads as a broken
/// button. The list is validated on save so that failure is loud and immediate.
pub const KNOWN_ENGINES: &[&str] = &[
    "mariadb",
    "postgres",
    "mongodb",
    "redis",
    "memcached",
    "sqlite",
];

pub struct ExternalToolStore {
    paths: AppPaths,
    tools: Vec<ExternalTool>,
}

impl ExternalToolStore {
    pub fn load(paths: &AppPaths) -> Result<Self, CoreError> {
        let mut tools: Vec<ExternalTool> = crate::db::load_docs(paths, "external_tools")?;
        for t in &mut tools {
            // Letos only opens SQLite files; drop network engines saved by older versions.
            if t.id == "letos" && t.engines != ["sqlite"] {
                t.engines = vec!["sqlite".into()];
            }
        }
        Ok(Self {
            paths: paths.clone(),
            tools,
        })
    }

    pub fn list(&self) -> Vec<ExternalTool> {
        let mut list = self.tools.clone();
        for tool in &mut list {
            tool.available = Some(
                !tool.executable.is_empty() && std::path::Path::new(&tool.executable).is_file(),
            );
        }
        list
    }

    /// The tool to use when none is chosen: the first one registered for this engine.
    /// A tool registered with no engines is offered everywhere but never picked here —
    /// an engine-specific tool and the built-in default both rank above it, so a
    /// catch-all cannot quietly displace HeidiSQL or pgAdmin.
    pub fn for_engine(&self, engine: &str) -> Option<&ExternalTool> {
        self.tools
            .iter()
            .find(|t| t.engines.iter().any(|e| e.eq_ignore_ascii_case(engine)))
    }

    pub fn get(&self, id: &str) -> Option<&ExternalTool> {
        self.tools.iter().find(|t| t.id.eq_ignore_ascii_case(id))
    }

    /// Adds or replaces (by id) a tool. Engine ids are normalized (trimmed, lowercased,
    /// de-duplicated) and checked against [`KNOWN_ENGINES`]: a misspelt engine saves
    /// happily and then never matches any page, which looks like the button is broken,
    /// so it is refused here with the way out instead. An empty or missing executable is
    /// allowed so imported tools appear in the unavailable state until configured.
    pub fn save(&mut self, tool: ExternalTool) -> Result<(), CoreError> {
        let mut tool = tool;
        if tool.id.trim().is_empty() || tool.name.trim().is_empty() {
            return Err(CoreError::ServiceError(
                "a tool needs an id and a name".into(),
            ));
        }
        tool.id = tool.id.trim().to_ascii_lowercase();
        tool.name = tool.name.trim().to_string();
        if tool.launch_mode.trim().is_empty()
            || (tool.launch_mode != "args" && tool.launch_mode != "clipboard")
        {
            tool.launch_mode = "args".to_string();
        }
        tool.executable = tool.executable.trim().to_string();
        tool.available = None;
        let mut engines: Vec<String> = Vec::with_capacity(tool.engines.len());
        for raw in &tool.engines {
            let e = raw.trim().to_ascii_lowercase();
            if e.is_empty() || engines.contains(&e) {
                continue;
            }
            if !KNOWN_ENGINES.contains(&e.as_str()) {
                return Err(CoreError::failed_fix(
                    format!("\"{}\" is not a database engine OLS has a page for.", raw.trim()),
                    format!(
                        "The engine ids are: {}. Anything else can never be opened from a page.",
                        KNOWN_ENGINES.join(", ")
                    ),
                    "Pick the engines this tool opens from the list, or leave them all off to offer it everywhere.",
                ));
            }
            engines.push(e);
        }
        tool.engines = engines;
        if let ToolArgs::PerEngine(map) = &tool.args {
            let mut normalized = std::collections::BTreeMap::new();
            for (raw, list) in map {
                let key = raw.trim().to_ascii_lowercase();
                if key != DEFAULT_ARGS_KEY && !KNOWN_ENGINES.contains(&key.as_str()) {
                    return Err(CoreError::failed_fix(
                        format!("\"{}\" is not a database engine in this tool's arguments.", raw.trim()),
                        format!(
                            "Per-engine arguments are keyed by one of: {}, or \"{DEFAULT_ARGS_KEY}\".",
                            KNOWN_ENGINES.join(", ")
                        ),
                        "Rename the key to an engine id, or use a plain list of arguments for every engine.",
                    ));
                }
                normalized.insert(key, list.clone());
            }
            tool.args = ToolArgs::PerEngine(normalized);
        }
        match self.tools.iter_mut().find(|t| t.id == tool.id) {
            Some(existing) => *existing = tool,
            None => self.tools.push(tool),
        }
        self.persist()
    }

    pub fn remove(&mut self, id: &str) -> Result<(), CoreError> {
        self.tools.retain(|t| !t.id.eq_ignore_ascii_case(id));
        self.persist()
    }

    fn persist(&self) -> Result<(), CoreError> {
        let refs: Vec<(String, &ExternalTool)> =
            self.tools.iter().map(|t| (t.id.clone(), t)).collect();
        crate::db::save_docs(&self.paths, "external_tools", &refs)
    }
}

/// Fills `{placeholders}` in a user-registered tool's arguments. Unknown placeholders are
/// left alone; nothing is ever passed through a shell.
pub fn expand_args(args: &[String], info: &ConnectionInfo) -> Vec<String> {
    args.iter()
        .map(|a| {
            a.replace("{host}", &info.host)
                .replace(
                    "{port}",
                    &info.port.map(|p| p.to_string()).unwrap_or_default(),
                )
                .replace("{user}", info.user.as_deref().unwrap_or(""))
                .replace("{database}", info.database.as_deref().unwrap_or(""))
                .replace("{path}", info.path.as_deref().unwrap_or(""))
                .replace("{uri}", &info.uri)
        })
        .filter(|s| !s.trim().is_empty())
        .collect()
}

/// HeidiSQL's own command line (its documented `--nettype` values: 0 = MariaDB/MySQL TCP/IP,
/// 10 = SQLite). Values are pre-quoted because HeidiSQL's parser truncates unquoted values at
/// the first dot (`-h=127.0.0.1` would read as `127`), and Rust's normal argument quoting
/// only quotes on spaces, so these are passed verbatim.
pub fn heidisql_args(info: &ConnectionInfo) -> Option<Vec<String>> {
    match info.engine.as_str() {
        "mariadb" | "postgres" => {
            // 0 = MariaDB/MySQL TCP/IP, 8 = PostgreSQL TCP/IP.
            let nettype = if info.engine == "postgres" { 8 } else { 0 };
            let mut args = vec![
                format!("--nettype={nettype}"),
                format!("--host=\"{}\"", info.host),
                format!("--port={}", info.port?),
                format!(
                    "--user={}",
                    info.user
                        .as_deref()
                        .unwrap_or(if nettype == 8 { "postgres" } else { "root" })
                ),
            ];
            if let Some(db) = &info.database {
                let value = if info.engine == "postgres"
                    && !db.contains(' ')
                    && !db.contains('.')
                    && !db.contains(';')
                {
                    db.clone()
                } else {
                    format!("\"{db}\"")
                };
                args.push(format!("--databases={value}"));
            }
            Some(args)
        }
        "sqlite" => Some(vec![
            "--nettype=10".to_string(),
            "--library=sqlite3.dll".to_string(),
            format!("--host=\"{}\"", info.path.as_deref()?),
        ]),
        _ => None,
    }
}

/// DB Browser for SQLite opens a file directly: `sqlitebrowser.exe "C:\path\to.db"`.
pub fn dbbrowser_args(info: &ConnectionInfo) -> Option<Vec<String>> {
    if info.engine.as_str() != "sqlite" {
        return None;
    }
    Some(vec![info.path.clone()?])
}

pub fn heidisql_args_for_executable(
    info: &ConnectionInfo,
    executable: &str,
) -> Option<Vec<String>> {
    let mut args = heidisql_args(info)?;
    if info.engine == "postgres" {
        let directory = PathBuf::from(executable).parent()?.to_path_buf();
        let mut libraries = std::fs::read_dir(&directory)
            .ok()?
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let name = path.file_name()?.to_str()?;
                let suffix = name.strip_prefix("libpq-")?.strip_suffix(".dll")?;
                Some((suffix.parse::<u32>().ok(), name.to_string()))
            })
            .collect::<Vec<_>>();
        let library = if directory.join("libpq.dll").is_file() {
            "libpq.dll".to_string()
        } else {
            libraries.sort_by_key(|(version, _)| *version);
            libraries.pop()?.1
        };
        args.push(format!("--library={library}"));
    }
    Some(args)
}

/// Starts a GUI tool detached: not supervised, because the user drives it and closing
/// OLS shouldn't kill their open database window. `verbatim` args are appended
/// exactly as written (Windows) instead of being re-quoted.
pub fn launch(executable: &str, args: &[String], verbatim: bool) -> Result<(), String> {
    let mut cmd = std::process::Command::new(executable);
    if let Some(parent) = std::path::Path::new(executable).parent() {
        cmd.current_dir(parent);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let verbatim = verbatim
            || args.iter().any(|a| a.contains('"'))
            || executable.to_lowercase().contains("heidisql");
        for a in args {
            if verbatim {
                cmd.raw_arg(a);
            } else {
                cmd.arg(a);
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = verbatim;
        cmd.args(args);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("could not start {executable}: {e}"))
}

#[cfg(test)]
mod external_tool_tests {
    use super::*;
    use crate::error::Diagnostic;

    fn info(engine: &str) -> ConnectionInfo {
        ConnectionInfo {
            engine: engine.into(),
            host: "127.0.0.1".into(),
            port: Some(3307),
            user: Some("root".into()),
            database: Some("shop".into()),
            path: Some("C:\\my dbs\\a.sqlite".into()),
            uri: "mysql://root@127.0.0.1:3307/shop".into(),
        }
    }

    #[test]
    fn placeholders_expand_and_unknown_ones_survive() {
        let out = expand_args(
            &[
                "--h={host}".into(),
                "-P{port}".into(),
                "{database}".into(),
                "{nope}".into(),
            ],
            &info("mariadb"),
        );
        assert_eq!(out, ["--h=127.0.0.1", "-P3307", "shop", "{nope}"]);
    }

    #[test]
    fn per_engine_args_parse_pick_by_engine_and_fall_back_to_default() {
        let tool: ExternalTool = serde_json::from_str(
            r#"{"id":"dbeaver","name":"DBeaver","engines":["mariadb","sqlite"],
                "args":{"mariadb":["-con","name=OLS {database}|driver=mariaDB|host={host}"],
                        "sqlite":["-con","driver=sqlite_jdbc|database={path}"],
                        "default":["{uri}"]}}"#,
        )
        .unwrap();
        let maria = expand_args(&tool.args.for_engine("mariadb"), &info("mariadb"));
        assert_eq!(
            maria,
            ["-con", "name=OLS shop|driver=mariaDB|host=127.0.0.1"]
        );
        let lite = expand_args(&tool.args.for_engine("sqlite"), &info("sqlite"));
        assert_eq!(
            lite,
            ["-con", "driver=sqlite_jdbc|database=C:\\my dbs\\a.sqlite"]
        );
        assert_eq!(tool.args.for_engine("postgres"), ["{uri}"]);
        assert!(!tool.args.is_empty());

        // A plain list still means "the same for every engine".
        let plain: ExternalTool =
            serde_json::from_str(r#"{"id":"x","name":"X","engines":[],"args":["{path}"]}"#)
                .unwrap();
        assert_eq!(plain.args.for_engine("sqlite"), ["{path}"]);
        assert_eq!(plain.args.for_engine("mariadb"), ["{path}"]);
    }
    #[test]
    fn heidisql_args_quote_dotted_values_and_pick_the_right_nettype() {
        let mysql = heidisql_args(&info("mariadb")).unwrap();
        assert!(mysql.contains(&"--nettype=0".to_string()));
        assert!(
            mysql.contains(&"--host=\"127.0.0.1\"".to_string()),
            "dots must be quoted for HeidiSQL's parser"
        );
        assert!(mysql.contains(&"--port=3307".to_string()));
        assert!(mysql.contains(&"--databases=\"shop\"".to_string()));

        let sqlite = heidisql_args(&info("sqlite")).unwrap();
        assert_eq!(sqlite[0], "--nettype=10");
        assert_eq!(sqlite[1], "--library=sqlite3.dll");
        assert_eq!(sqlite[2], "--host=\"C:\\my dbs\\a.sqlite\"");

        assert!(heidisql_args(&info("mongodb")).is_none());
    }

    #[test]
    fn detect_lists_redis_one_click_tools() {
        let tools = detect_db_tools();
        let tool = tools
            .iter()
            .find(|t| t.id == "tinyrdm")
            .expect("detect_db_tools must list tinyrdm");
        assert!(
            tool.engines.contains(&"redis".to_string()),
            "tinyrdm must handle redis"
        );
        assert!(
            tools.iter().find(|t| t.id == "redisinsight").is_none(),
            "redisinsight removed per request"
        );
        assert!(
            heidisql_args(&info("redis")).is_none(),
            "HeidiSQL cannot open redis; custom tool required"
        );
    }

    #[test]
    fn dbbrowser_opens_sqlite_file_directly() {
        let tools = detect_db_tools();
        let tool = tools
            .iter()
            .find(|t| t.id == "dbbrowser")
            .expect("detect_db_tools must list dbbrowser");
        assert!(
            tool.engines.contains(&"sqlite".to_string()),
            "dbbrowser must handle sqlite"
        );
        assert_eq!(
            dbbrowser_args(&info("sqlite")).unwrap(),
            vec!["C:\\my dbs\\a.sqlite".to_string()]
        );
        assert!(dbbrowser_args(&info("mariadb")).is_none());
    }

    #[test]
    fn store_persists_and_reports_availability() {
        let home = crate::test_support::isolated_home();
        let mut store = ExternalToolStore::load(&home.paths).unwrap();
        let exe = home.paths.root().join("tool.exe");
        std::fs::write(&exe, "x").unwrap();

        let tool = ExternalTool {
            id: "compass".into(),
            name: "Compass".into(),
            engines: vec!["mongodb".into()],
            launch_mode: "args".into(),
            executable: exe.display().to_string(),
            args: vec!["{uri}".into()].into(),
            available: None,
        };
        store.save(tool.clone()).unwrap();

        let unavail = ExternalTool {
            id: "missing".into(),
            name: "Missing".into(),
            engines: vec!["mariadb".into()],
            launch_mode: "args".into(),
            executable: "C:/nope/x.exe".into(),
            args: vec!["{uri}".into()].into(),
            available: None,
        };
        store.save(unavail).unwrap();

        let list = store.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].available, Some(true));
        assert_eq!(list[1].available, Some(false));
        assert_eq!(store.for_engine("mongodb").unwrap().id, "compass");
        assert!(store.for_engine("mysql").is_none());

        assert_eq!(
            ExternalToolStore::load(&home.paths).unwrap().list().len(),
            2
        );
    }

    #[test]
    fn save_normalizes_engines_and_refuses_an_id_no_page_uses() {
        let home = crate::test_support::isolated_home();
        let mut store = ExternalToolStore::load(&home.paths).unwrap();
        let exe = home.paths.root().join("custom_db_explorer.exe");
        std::fs::write(&exe, "x").unwrap();
        let exe = exe.display().to_string();

        store
            .save(ExternalTool {
                id: "  MyExplorer ".into(),
                name: " My Explorer ".into(),
                engines: vec!["MariaDB".into(), " mariadb ".into(), "SQLite".into()],
                launch_mode: "args".into(),
                executable: exe.clone(),
                args: vec!["{uri}".into()].into(),
                available: None,
            })
            .unwrap();

        let list = store.list();
        assert_eq!(list[0].id, "myexplorer", "ids are trimmed and lowercased");
        assert_eq!(list[0].name, "My Explorer");
        assert_eq!(
            list[0].engines,
            vec!["mariadb".to_string(), "sqlite".to_string()],
            "engines are lowercased and de-duplicated"
        );
        assert_eq!(
            store.for_engine("MariaDB").map(|t| t.id.clone()),
            Some("myexplorer".to_string()),
            "lookup must not depend on the case either side was typed in"
        );

        let err = store
            .save(ExternalTool {
                id: "myexplorer".into(),
                name: "My Explorer".into(),
                engines: vec!["mysql".into()],
                launch_mode: "args".into(),
                executable: exe,
                args: vec![].into(),
                available: None,
            })
            .unwrap_err();
        let diag = Diagnostic::from(&err);
        let said = format!(
            "{} {} {}",
            diag.problem,
            diag.cause,
            diag.fix.unwrap_or_default()
        );
        assert!(
            said.contains("mariadb"),
            "the refusal must name the ids that work: {said}"
        );
        assert_eq!(
            store.list().len(),
            1,
            "a refused save must not disturb what is already registered"
        );
    }

    #[test]
    fn an_engine_less_tool_is_listed_everywhere_but_never_auto_picked() {
        let home = crate::test_support::isolated_home();
        let mut store = ExternalToolStore::load(&home.paths).unwrap();
        let exe = home.paths.root().join("custom_db_explorer.exe");
        std::fs::write(&exe, "x").unwrap();
        store
            .save(ExternalTool {
                id: "custom_db_explorer".into(),
                name: "My DB Explorer".into(),
                engines: vec![],
                launch_mode: "args".into(),
                executable: exe.display().to_string(),
                args: vec!["{uri}".into()].into(),
                available: None,
            })
            .unwrap();

        assert!(store.for_engine("mariadb").is_none());
        assert_eq!(
            store.get("CUSTOM_DB_EXPLORER").map(|t| t.id.clone()),
            Some("custom_db_explorer".to_string()),
            "an explicit choice finds it whatever the case"
        );
    }
}
