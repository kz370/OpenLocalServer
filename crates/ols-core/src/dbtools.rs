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
}

pub fn detect_db_tools() -> Vec<DbTool> {
    vec![
        DbTool { id: "heidisql".into(), name: "HeidiSQL".into(), found_path: find_heidisql() },
        DbTool { id: "pgadmin".into(), name: "pgAdmin 4".into(), found_path: find_pgadmin() },
    ]
}

fn find_on_path(exe_name: &str) -> Option<String> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var).map(|dir| dir.join(exe_name)).find(|p| p.is_file()).map(|p| p.display().to_string())
}

fn find_heidisql() -> Option<String> {
    for base in [r"C:\Program Files\HeidiSQL", r"C:\Program Files (x86)\HeidiSQL"] {
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
    for base in [r"C:\Program Files\pgAdmin 4", r"C:\Program Files (x86)\pgAdmin 4"] {
        let Ok(entries) = std::fs::read_dir(base) else { continue };
        for entry in entries.flatten() {
            let candidate = entry.path().join("runtime").join("pgAdmin4.exe");
            if candidate.is_file() {
                return Some(candidate.display().to_string());
            }
        }
    }
    find_on_path("pgAdmin4.exe")
}
