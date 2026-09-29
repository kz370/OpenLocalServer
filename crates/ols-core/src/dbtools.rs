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
            id: "heidisql".into(),
            name: "HeidiSQL".into(),
            found_path: find_heidisql(),
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
        DbTool {
            id: "dbbrowser".into(),
            name: "DB Browser for SQLite".into(),
            found_path: find_dbbrowser(),
            engines: vec!["sqlite".into()],
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
    /// Engines this tool can open: "mariadb" | "sqlite" | "mongodb" | "postgres" | "redis".
    pub engines: Vec<String>,
    pub executable: String,
    /// Arguments with placeholders: {host} {port} {user} {database} {path} {uri}.
    pub args: Vec<String>,
}

pub struct ExternalToolStore {
    paths: AppPaths,
    tools: Vec<ExternalTool>,
}

impl ExternalToolStore {
    pub fn load(paths: &AppPaths) -> Result<Self, CoreError> {
        let tools = crate::db::load_docs(paths, "external_tools")?;
        Ok(Self {
            paths: paths.clone(),
            tools,
        })
    }

    pub fn list(&self) -> Vec<ExternalTool> {
        self.tools.clone()
    }

    pub fn for_engine(&self, engine: &str) -> Option<&ExternalTool> {
        self.tools
            .iter()
            .find(|t| t.engines.iter().any(|e| e == engine))
    }

    pub fn get(&self, id: &str) -> Option<&ExternalTool> {
        self.tools.iter().find(|t| t.id == id)
    }

    /// Adds or replaces (by id) a tool. The executable must exist — a typo here would
    /// otherwise only surface as a confusing failure at click time.
    pub fn save(&mut self, tool: ExternalTool) -> Result<(), CoreError> {
        if tool.id.trim().is_empty() || tool.name.trim().is_empty() {
            return Err(CoreError::ServiceError(
                "a tool needs an id and a name".into(),
            ));
        }
        if !std::path::Path::new(&tool.executable).is_file() {
            return Err(CoreError::ServiceError(format!(
                "{} does not exist",
                tool.executable
            )));
        }
        match self.tools.iter_mut().find(|t| t.id == tool.id) {
            Some(existing) => *existing = tool,
            None => self.tools.push(tool),
        }
        self.persist()
    }

    pub fn remove(&mut self, id: &str) -> Result<(), CoreError> {
        self.tools.retain(|t| t.id != id);
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
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
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
    fn store_requires_an_existing_executable_and_persists() {
        let home = crate::test_support::isolated_home();
        let mut store = ExternalToolStore::load(&home.paths).unwrap();
        let exe = home.paths.root().join("tool.exe");
        std::fs::write(&exe, "x").unwrap();

        let tool = ExternalTool {
            id: "compass".into(),
            name: "Compass".into(),
            engines: vec!["mongodb".into()],
            executable: exe.display().to_string(),
            args: vec!["{uri}".into()],
        };
        store.save(tool.clone()).unwrap();
        assert!(store
            .save(ExternalTool {
                executable: "C:/nope/x.exe".into(),
                ..tool.clone()
            })
            .is_err());
        assert_eq!(store.for_engine("mongodb").unwrap().id, "compass");
        assert!(store.for_engine("mysql").is_none());

        assert_eq!(
            ExternalToolStore::load(&home.paths).unwrap().list().len(),
            1
        );
    }
}
