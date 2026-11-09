//! Windows Defender exclusions and a Dev Drive hint (§173).
//!
//! `npm install` and `composer install` are much faster when Defender does not scan project
//! folders in real time. OLS offers to exclude **exactly** the folders it manages (project
//! roots, the sites folder and its own data folder), with the user's consent, and can remove
//! them again. It lists and removes only what it added: Windows does not let normal users read
//! the exclusion list, so OLS keeps its own in the setting `defender.excluded_by_ols`.
//!
//! **Security:** Defender does not scan excluded folders in real time. Malware downloaded into
//! an excluded folder (a malicious npm package, say) is not caught there. Only exclude folders
//! with code you trust. OLS never excludes anything without asking, never excludes processes,
//! and the elevated helper re-validates every path (`valid_exclusion_path` in `ols-helper`).

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::app::{Inner, PROJECT_ROOTS};
use crate::error::CoreError;

pub const SETTING_EXCLUDED: &str = "defender.excluded_by_ols";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DefenderState {
    pub available: bool,
    pub realtime_on: bool,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevDriveInfo {
    pub path: String,
    pub file_system: String,
    /// ReFS: a Dev Drive (or at least a drive Windows treats well for development).
    pub is_dev_drive: bool,
}

/// Same rules as the helper's check (the helper keeps its own copy because it must not trust
/// the caller). A local drive path to an existing folder, no characters that could break out
/// of a PowerShell string, no `.`/`..` parts, not a drive root, not inside a system folder,
/// not a user profile root or its `AppData`.
pub fn valid_exclusion_path(p: &str) -> bool {
    const BAD: &[char] = &[
        '\'', '"', '`', '$', ';', '&', '|', '<', '>', '%', '^', '*', '?', '/',
    ];
    let b = p.as_bytes();
    if p.len() < 4 || p.len() > 240 {
        return false;
    }
    if !(b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\') {
        return false;
    }
    if p.chars().any(|c| c.is_control() || BAD.contains(&c)) || p[2..].contains(':') {
        return false;
    }
    let trimmed = p.trim_end_matches('\\');
    if trimmed.len() <= 2
        || trimmed[3..]
            .split('\\')
            .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return false;
    }
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    let norm = |s: String| s.trim_end_matches('\\').to_lowercase();
    let lower = trimmed.to_lowercase();
    let inside = [
        norm(env("SystemRoot", r"C:\Windows")),
        norm(env("ProgramFiles", r"C:\Program Files")),
        norm(env("ProgramFiles(x86)", r"C:\Program Files (x86)")),
        norm(env("ProgramData", r"C:\ProgramData")),
    ];
    if inside
        .iter()
        .any(|r| lower == *r || lower.starts_with(&format!("{r}\\")))
    {
        return false;
    }
    let users = norm(format!(r"{}\Users", env("SystemDrive", "C:")));
    if lower == users {
        return false;
    }
    if let Some(rest) = lower.strip_prefix(&format!("{users}\\")) {
        let parts: Vec<&str> = rest.split('\\').collect();
        if parts.len() == 1 || (parts.len() == 2 && parts[1] == "appdata") {
            return false;
        }
    }
    Path::new(p).is_dir()
}

/// The only paths OLS will ever add, from folder lists (pure, for testing): valid ones,
/// de-duplicated case-insensitively, without any that sit inside another suggestion.
pub fn pick_suggestions(candidates: &[String], valid: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut unique: Vec<String> = Vec::new();
    for c in candidates {
        let t = c.trim().trim_end_matches('\\').to_string();
        if valid(&t) && seen.insert(t.to_lowercase()) {
            unique.push(t);
        }
    }
    let lowered: Vec<String> = unique.iter().map(|s| s.to_lowercase()).collect();
    unique
        .into_iter()
        .enumerate()
        .filter(|(i, _)| {
            !lowered
                .iter()
                .enumerate()
                .any(|(j, other)| j != *i && lowered[*i].starts_with(&format!("{other}\\")))
        })
        .map(|(_, s)| s)
        .collect()
}

/// `add` only for a suggestion (case-insensitive exact match).
pub fn check_add(path: &str, suggestions: &[String]) -> Result<String, CoreError> {
    suggestions
        .iter()
        .find(|s| s.eq_ignore_ascii_case(path.trim().trim_end_matches('\\')))
        .cloned()
        .ok_or_else(|| {
            CoreError::failed_fix(
                "OLS will not exclude that folder.",
                format!("\"{path}\" is not one of the folders OLS manages (project folders, the sites folder and its data folder)."),
                "Pick a folder from the list in Settings → Performance.",
            )
        })
}

/// `remove` only for a path OLS added itself.
pub fn check_remove(path: &str, ours: &[String]) -> Result<String, CoreError> {
    ours.iter()
        .find(|s| s.eq_ignore_ascii_case(path.trim().trim_end_matches('\\')))
        .cloned()
        .ok_or_else(|| {
            CoreError::failed_fix(
                "OLS did not add that exclusion.",
                format!("\"{path}\" is not in the list of exclusions OLS made, and OLS only removes its own."),
                "Remove other exclusions in Windows Security → Virus & threat protection → Manage settings.",
            )
        })
}

fn run_ps(script: &str) -> crate::exec::Captured {
    crate::exec::run_capture(
        &crate::exec::powershell_path(),
        &[
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            script.into(),
        ],
        None,
        &[],
        Duration::from_secs(10),
    )
}

/// Parses `Get-MpComputerStatus | Select AMServiceEnabled,RealTimeProtectionEnabled | ConvertTo-Json`.
pub fn parse_state(stdout: &str) -> DefenderState {
    match serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        Ok(v) => {
            let on = |k: &str| v.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
            DefenderState {
                available: on("AMServiceEnabled"),
                realtime_on: on("RealTimeProtectionEnabled"),
                error: None,
            }
        }
        Err(_) => DefenderState {
            available: false,
            realtime_on: false,
            error: Some("Windows Defender did not answer.".into()),
        },
    }
}

/// Asks Windows (not elevated, 10 s limit). A third-party antivirus or a policy makes this fail,
/// which is reported as "not available", not as an error.
pub fn query_state() -> DefenderState {
    let out = run_ps(
        "(Get-MpComputerStatus | Select-Object -Property AMServiceEnabled,RealTimeProtectionEnabled | ConvertTo-Json)",
    );
    if out.success() {
        parse_state(&out.stdout)
    } else {
        DefenderState {
            available: false,
            realtime_on: false,
            error: Some(
                out.combined()
                    .lines()
                    .next()
                    .unwrap_or("Windows Defender is not active here.")
                    .to_string(),
            ),
        }
    }
}

/// Parses `... AntiVirusProduct | Select-Object -ExpandProperty displayName | ConvertTo-Json`:
/// one product is a JSON string, several are an array. Microsoft Defender itself is left out,
/// and names are de-duplicated (Security Center can list one product twice).
pub fn parse_antivirus(stdout: &str) -> Vec<String> {
    let names: Vec<String> = match serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        Ok(serde_json::Value::String(s)) => vec![s],
        Ok(serde_json::Value::Array(a)) => a
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    let mut out: Vec<String> = Vec::new();
    for n in names {
        let n = n.trim().to_string();
        let lower = n.to_ascii_lowercase();
        let is_defender =
            lower.contains("windows defender") || lower.contains("microsoft defender");
        if !n.is_empty() && !is_defender && !out.iter().any(|o| o.eq_ignore_ascii_case(&n)) {
            out.push(n);
        }
    }
    out
}

/// Antivirus products other than Defender that Windows Security Center knows (Bitdefender,
/// Avast, Kaspersky, ...). OLS cannot change their settings; the card only names them so the
/// user knows where the exclusions have to go. Empty when Security Center is not there
/// (Windows Server) or does not answer.
pub fn query_other_antivirus() -> Vec<String> {
    let out = run_ps(
        "(Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct -ErrorAction Stop | Select-Object -ExpandProperty displayName | ConvertTo-Json)",
    );
    if out.success() {
        parse_antivirus(&out.stdout)
    } else {
        Vec::new()
    }
}

fn file_system(letter: char) -> String {
    let out = run_ps(&format!(
        "(Get-Volume -DriveLetter {letter}).FileSystemType"
    ));
    if out.success() {
        out.stdout.trim().to_string()
    } else {
        String::new()
    }
}

impl Inner {
    fn excluded_by_ols(&self) -> Vec<String> {
        self.string_list(SETTING_EXCLUDED)
    }

    /// Folders OLS would offer to exclude.
    pub fn defender_suggestions(&self) -> Vec<String> {
        let mut c = self.string_list(PROJECT_ROOTS);
        c.push(self.paths.sites_dir().display().to_string());
        c.push(self.paths.data_dir().display().to_string());
        pick_suggestions(&c, &valid_exclusion_path)
    }

    /// State + OLS's own exclusions + suggestions + drive file systems. Runs PowerShell, so
    /// only call it when the Settings card opens.
    pub fn defender_status(&self) -> DefenderStatus {
        let state = query_state();
        *self.defender_cache.lock().unwrap() = Some(state.clone());
        let suggestions = self.defender_suggestions();
        let mut letters: Vec<char> = suggestions
            .iter()
            .filter_map(|s| s.chars().next())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        letters.sort();
        letters.dedup();
        let dev_drive = if state.available {
            letters
                .into_iter()
                .map(|l| {
                    let fs = file_system(l);
                    DevDriveInfo {
                        path: format!("{l}:\\"),
                        is_dev_drive: fs.eq_ignore_ascii_case("refs"),
                        file_system: fs,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        DefenderStatus {
            state,
            excluded_by_ols: self.excluded_by_ols(),
            suggestions,
            dev_drive,
            other_antivirus: query_other_antivirus(),
        }
    }

    pub fn add_defender_exclusion(&self, path: &str) -> Result<(), CoreError> {
        let path = check_add(path, &self.defender_suggestions())?;
        crate::elevate::run_helper(&["defender-exclude-add".into(), path.clone()]).map_err(
            |e| {
                CoreError::failed_fix(
                "Windows Defender was not changed.",
                e,
                "Allow the administrator prompt, or install the OLS helper service from Settings.",
            )
            },
        )?;
        let mut ours = self.excluded_by_ols();
        if !ours.iter().any(|o| o.eq_ignore_ascii_case(&path)) {
            ours.push(path);
        }
        self.settings
            .lock()
            .unwrap()
            .set(SETTING_EXCLUDED, serde_json::json!(ours))
    }

    pub fn remove_defender_exclusion(&self, path: &str) -> Result<(), CoreError> {
        let path = check_remove(path, &self.excluded_by_ols())?;
        crate::elevate::run_helper(&["defender-exclude-remove".into(), path.clone()]).map_err(
            |e| {
                CoreError::failed_fix(
                    "Windows Defender was not changed.",
                    e,
                    "Allow the administrator prompt, or install the OLS helper service from Settings.",
                )
            },
        )?;
        let mut ours = self.excluded_by_ols();
        ours.retain(|o| !o.eq_ignore_ascii_case(&path));
        self.settings
            .lock()
            .unwrap()
            .set(SETTING_EXCLUDED, serde_json::json!(ours))
    }

    /// For Diagnostics: cheap, no PowerShell. Only fires when the Settings card has already
    /// read the state, Defender's real-time scan is on, and none of the suggestions is excluded.
    pub fn defender_advice(&self) -> bool {
        let cached = self.defender_cache.lock().unwrap().clone();
        let Some(state) = cached else { return false };
        if !state.available || !state.realtime_on {
            return false;
        }
        let ours = self.excluded_by_ols();
        let suggestions = self.defender_suggestions();
        !suggestions.is_empty()
            && !suggestions
                .iter()
                .any(|s| ours.iter().any(|o| o.eq_ignore_ascii_case(s)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefenderStatus {
    pub state: DefenderState,
    pub excluded_by_ols: Vec<String>,
    pub suggestions: Vec<String>,
    pub dev_drive: Vec<DevDriveInfo>,
    /// Other antivirus products installed (names from Windows Security Center).
    #[serde(default)]
    pub other_antivirus: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_antivirus_names_leave_defender_out() {
        assert_eq!(
            parse_antivirus(r#""Bitdefender Antivirus""#),
            vec!["Bitdefender Antivirus"]
        );
        assert_eq!(
            parse_antivirus(
                r#"["Windows Defender","Avast Antivirus","avast antivirus","Microsoft Defender Antivirus"]"#
            ),
            vec!["Avast Antivirus"]
        );
        assert!(parse_antivirus("").is_empty());
        assert!(parse_antivirus("not json").is_empty());
    }

    #[test]
    fn dangerous_paths_are_never_valid() {
        for bad in [
            "C:\\",
            "C:\\x'; calc; '",
            "C:\\x$(calc)",
            "C:\\x\\..\\Windows",
            "\\\\server\\share",
            "C:\\Windows\\Temp",
            "C:\\Program Files\\x",
            "C:\\Users",
            "C:\\Users\\bob",
            "C:\\Users\\bob\\AppData",
            "",
            "relative",
        ] {
            assert!(!valid_exclusion_path(bad), "{bad:?}");
        }
        let t = tempfile::tempdir().unwrap();
        assert!(valid_exclusion_path(&t.path().display().to_string()));
    }

    #[test]
    fn add_refuses_paths_that_are_not_suggestions() {
        let suggestions = vec![r"D:\Sites".to_string(), r"D:\ols\data".to_string()];
        assert!(check_add(r"C:\", &suggestions).is_err());
        let t = tempfile::tempdir().unwrap();
        assert!(check_add(&t.path().display().to_string(), &suggestions).is_err());
        assert_eq!(check_add(r"d:\sites\", &suggestions).unwrap(), r"D:\Sites");
    }

    #[test]
    fn remove_refuses_exclusions_ols_did_not_add() {
        let ours = vec![r"D:\Sites".to_string()];
        assert!(check_remove(r"D:\Other", &ours).is_err());
        assert!(check_remove(r"C:\Windows", &ours).is_err());
        assert_eq!(check_remove(r"D:\SITES", &ours).unwrap(), r"D:\Sites");
    }

    #[test]
    fn suggestions_are_deduplicated_and_nested_ones_dropped() {
        let all = |_: &str| true;
        let got = pick_suggestions(
            &[
                r"D:\Sites".into(),
                r"d:\sites\".into(),
                r"D:\Sites\inner".into(),
                r"E:\Work".into(),
                r"D:\SitesExtra".into(),
            ],
            &all,
        );
        assert_eq!(got, vec![r"D:\Sites", r"E:\Work", r"D:\SitesExtra"]);
        let none = pick_suggestions(&[r"D:\x".into()], &|_| false);
        assert!(none.is_empty());
    }

    #[test]
    fn state_json_is_read_and_garbage_is_not_available() {
        let s = parse_state(r#"{"AMServiceEnabled":true,"RealTimeProtectionEnabled":true}"#);
        assert!(s.available && s.realtime_on);
        let off = parse_state(r#"{"AMServiceEnabled":true,"RealTimeProtectionEnabled":false}"#);
        assert!(off.available && !off.realtime_on);
        let bad = parse_state("not json");
        assert!(!bad.available && bad.error.is_some());
    }

    #[test]
    fn the_excluded_list_round_trips_through_settings() {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        let core = crate::command::Core::new(settings, home.paths.clone());
        let inner = core.inner();
        assert!(inner.excluded_by_ols().is_empty());
        inner
            .settings
            .lock()
            .unwrap()
            .set(SETTING_EXCLUDED, serde_json::json!([r"D:\Sites"]))
            .unwrap();
        assert_eq!(inner.excluded_by_ols(), vec![r"D:\Sites".to_string()]);
        // The helper call is disabled under test, so a real add/remove can never run here.
        assert!(inner.remove_defender_exclusion(r"D:\Sites").is_err());
        assert_eq!(
            inner.excluded_by_ols().len(),
            1,
            "a failed remove changes nothing"
        );
    }
}
