//! Profiler (§174): "why is this page slow?" Xdebug already ships with OLS; with its
//! `profile` mode on, a request opened with `?XDEBUG_TRIGGER=1` writes a cachegrind file and
//! this module reads it into a table of top functions and an approximate call tree.
//!
//! Files are read as a stream (never whole into memory) and refused over 300 MB. Names given
//! to `get_profile` / `delete_profiles` are plain file names starting with `cachegrind.out.`
//! that exist in one of the Xdebug output folders; no path is built from user text.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::error::CoreError;
use crate::snapshots::is_plain_file_name;
use crate::xdebug::XdebugSettings;

const PREFIX: &str = "cachegrind.out.";
const MAX_BYTES: u64 = 300 * 1024 * 1024;
const TOP_FUNCTIONS: usize = 300;
const MAX_DEPTH: usize = 40;
const MAX_CHILDREN: usize = 30;
const MAX_NODES: usize = 5_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileFile {
    /// File name only.
    pub file: String,
    pub php_version: String,
    pub host: Option<String>,
    pub created_ms: u64,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FnStat {
    pub name: String,
    pub file: String,
    pub self_ns: u64,
    pub inclusive_ns: u64,
    pub calls: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallNode {
    pub name: String,
    pub inclusive_ns: u64,
    pub calls: u64,
    pub children: Vec<CallNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub total_time_ns: u64,
    /// Top 300 by inclusive time.
    pub functions: Vec<FnStat>,
    /// Approximate: Xdebug records totals per caller → callee pair, not every path.
    pub tree: CallNode,
}

// ------------------------------------------------------------------ file names

/// `cachegrind.out.1762600000_123456.shop.test` → `shop.test`. The part up to the first dot
/// after the prefix is the time (`%u`); the rest is the host (`%H`).
pub fn host_of(file: &str) -> Option<String> {
    let rest = file.strip_prefix(PREFIX)?;
    let (_, host) = rest.split_once('.')?;
    (!host.is_empty()).then(|| host.to_string())
}

fn is_profile_name(name: &str) -> bool {
    is_plain_file_name(name) && name.starts_with(PREFIX)
}

// ------------------------------------------------------------------ parser

#[derive(Default)]
struct Raw {
    names: HashMap<u32, String>,
    files: HashMap<u32, String>,
    fn_file: HashMap<u32, u32>,
    self_units: HashMap<u32, u64>,
    calls: HashMap<u32, u64>,
    /// (caller, callee) → (calls, inclusive units)
    edges: HashMap<(u32, u32), (u64, u64)>,
    summary_units: Option<u64>,
}

fn parse_id_name(rest: &str) -> Option<(u32, Option<String>)> {
    let rest = rest.trim();
    let inner = rest.strip_prefix('(')?;
    let (id, after) = inner.split_once(')')?;
    let id = id.trim().parse().ok()?;
    let name = after.trim();
    Some((id, (!name.is_empty()).then(|| name.to_string())))
}

/// Streams a callgrind file written by Xdebug 3. Unknown lines are ignored.
pub fn parse_cachegrind<R: BufRead>(reader: R) -> Result<Profile, String> {
    let mut raw = Raw::default();
    let mut cur_file: Option<u32> = None;
    let mut cur_fn: Option<u32> = None;
    let mut callee: Option<u32> = None;
    let mut callee_file: Option<u32> = None;
    let mut pending: Option<(u32, u32, u64)> = None; // (caller, callee, calls)
    let mut saw_header = false;
    let mut saw_cost = false;

    for line in reader.lines() {
        let line = line.map_err(|e| format!("could not read the profile: {e}"))?;
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        if let Some(v) = line.strip_prefix("events:") {
            saw_header = true;
            let _ = v; // the first event is time (10 ns units) in Xdebug's files
            continue;
        }
        if let Some(v) = line
            .strip_prefix("summary:")
            .or_else(|| line.strip_prefix("totals:"))
        {
            if let Some(n) = v.split_whitespace().next().and_then(|t| t.parse().ok()) {
                raw.summary_units = Some(n);
            }
            continue;
        }
        if let Some(v) = line.strip_prefix("fl=") {
            if let Some((id, name)) = parse_id_name(v) {
                if let Some(n) = name {
                    raw.files.insert(id, n);
                }
                cur_file = Some(id);
            }
            continue;
        }
        if let Some(v) = line.strip_prefix("fn=") {
            if let Some((id, name)) = parse_id_name(v) {
                if let Some(n) = name {
                    raw.names.insert(id, n);
                }
                if let Some(f) = cur_file {
                    raw.fn_file.entry(id).or_insert(f);
                }
                cur_fn = Some(id);
            }
            continue;
        }
        if let Some(v) = line.strip_prefix("cfl=") {
            if let Some((id, name)) = parse_id_name(v) {
                if let Some(n) = name {
                    raw.files.insert(id, n);
                }
                callee_file = Some(id);
            }
            continue;
        }
        if let Some(v) = line.strip_prefix("cfn=") {
            if let Some((id, name)) = parse_id_name(v) {
                if let Some(n) = name {
                    raw.names.insert(id, n);
                }
                if let Some(f) = callee_file {
                    raw.fn_file.entry(id).or_insert(f);
                }
                callee = Some(id);
            }
            continue;
        }
        if let Some(v) = line.strip_prefix("calls=") {
            let n: u64 = v
                .split_whitespace()
                .next()
                .and_then(|t| t.parse().ok())
                .unwrap_or(1);
            if let (Some(a), Some(b)) = (cur_fn, callee) {
                pending = Some((a, b, n));
            }
            continue;
        }
        // A cost line: `<line> <time> [<memory>]`.
        if line.starts_with(|c: char| c.is_ascii_digit() || c == '+' || c == '-' || c == '*') {
            let mut parts = line.split_whitespace();
            let _position = parts.next();
            let Some(units) = parts.next().and_then(|t| t.parse::<u64>().ok()) else {
                continue;
            };
            saw_cost = true;
            if let Some((a, b, n)) = pending.take() {
                let e = raw.edges.entry((a, b)).or_insert((0, 0));
                e.0 += n;
                e.1 += units;
                *raw.calls.entry(b).or_insert(0) += n;
            } else if let Some(f) = cur_fn {
                *raw.self_units.entry(f).or_insert(0) += units;
            }
        }
    }
    if !saw_header && !saw_cost {
        return Err("This does not look like an Xdebug profile (no cost lines were found).".into());
    }
    Ok(build_profile(raw))
}

fn build_profile(raw: Raw) -> Profile {
    const NS: u64 = 10; // Xdebug counts time in units of 10 ns
    let ids: Vec<u32> = {
        let mut set: Vec<u32> = raw.names.keys().copied().collect();
        set.sort_unstable();
        set
    };
    let mut out_edges: HashMap<u32, Vec<(u32, u64, u64)>> = HashMap::new();
    for ((a, b), (n, c)) in &raw.edges {
        out_edges.entry(*a).or_default().push((*b, *n, *c));
    }
    let self_of = |id: u32| raw.self_units.get(&id).copied().unwrap_or(0);
    let incl_of = |id: u32| {
        self_of(id)
            + out_edges
                .get(&id)
                .map(|v| v.iter().map(|e| e.2).sum::<u64>())
                .unwrap_or(0)
    };
    let name_of = |id: u32| {
        raw.names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("#{id}"))
    };
    let file_of = |id: u32| {
        raw.fn_file
            .get(&id)
            .and_then(|f| raw.files.get(f))
            .cloned()
            .unwrap_or_default()
    };

    let mut functions: Vec<FnStat> = ids
        .iter()
        .map(|&id| FnStat {
            name: name_of(id),
            file: file_of(id),
            self_ns: self_of(id) * NS,
            inclusive_ns: incl_of(id) * NS,
            calls: raw.calls.get(&id).copied().unwrap_or(0),
        })
        .collect();
    functions.sort_by(|a, b| {
        b.inclusive_ns
            .cmp(&a.inclusive_ns)
            .then(a.name.cmp(&b.name))
    });
    functions.truncate(TOP_FUNCTIONS);

    let sum_self: u64 = raw.self_units.values().sum();
    let main = ids.iter().copied().find(|&id| name_of(id) == "{main}");
    let root = main.or_else(|| ids.iter().copied().max_by_key(|&id| incl_of(id)));
    let total_units = raw
        .summary_units
        .or_else(|| main.map(incl_of))
        .unwrap_or(sum_self);
    let total_ns = total_units * NS;

    let mut budget = MAX_NODES;
    let tree = match root {
        Some(r) => {
            let mut path = vec![r];
            node(
                r,
                incl_of(r) * NS,
                raw.calls.get(&r).copied().unwrap_or(1).max(1),
                &out_edges,
                &incl_of,
                &name_of,
                total_ns,
                &mut path,
                &mut budget,
            )
        }
        None => CallNode {
            name: "(empty)".into(),
            inclusive_ns: 0,
            calls: 0,
            children: Vec::new(),
        },
    };
    Profile {
        total_time_ns: total_ns,
        functions,
        tree,
    }
}

#[allow(clippy::too_many_arguments)]
fn node(
    id: u32,
    inclusive_ns: u64,
    calls: u64,
    out_edges: &HashMap<u32, Vec<(u32, u64, u64)>>,
    incl_of: &dyn Fn(u32) -> u64,
    name_of: &dyn Fn(u32) -> String,
    total_ns: u64,
    path: &mut Vec<u32>,
    budget: &mut usize,
) -> CallNode {
    let mut n = CallNode {
        name: name_of(id),
        inclusive_ns,
        calls,
        children: Vec::new(),
    };
    if path.len() >= MAX_DEPTH || *budget == 0 {
        return n;
    }
    let Some(edges) = out_edges.get(&id) else {
        return n;
    };
    // The function's own global inclusive time scales its callees down to what this call
    // site accounts for, so the tree adds up even though Xdebug keeps no per-path totals.
    let global = incl_of(id).max(1) as f64 * 10.0;
    let factor = (inclusive_ns as f64 / global).min(1.0);
    let mut sorted: Vec<&(u32, u64, u64)> = edges.iter().collect();
    sorted.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    let threshold = total_ns / 200; // 0.5 %
    let (mut other_ns, mut other_calls) = (0u64, 0u64);
    for &&(callee, ncalls, cost) in sorted.iter().take(MAX_CHILDREN * 4) {
        if path.contains(&callee) {
            continue; // a cycle (recursion): do not follow it again
        }
        let ns = (cost as f64 * 10.0 * factor) as u64;
        if ns < threshold || n.children.len() >= MAX_CHILDREN || *budget == 0 {
            other_ns += ns;
            other_calls += ncalls;
            continue;
        }
        *budget -= 1;
        path.push(callee);
        let child = node(
            callee, ns, ncalls, out_edges, incl_of, name_of, total_ns, path, budget,
        );
        path.pop();
        n.children.push(child);
    }
    if other_ns > 0 {
        n.children.push(CallNode {
            name: "other".into(),
            inclusive_ns: other_ns,
            calls: other_calls,
            children: Vec::new(),
        });
    }
    n
}

/// Opens and parses a profile file, refusing very large ones.
pub fn parse_file(path: &Path) -> Result<Profile, CoreError> {
    let md = std::fs::metadata(path).map_err(|e| {
        CoreError::failed_fix(
            "OLS could not open that profile.",
            e.to_string(),
            "Refresh the list; the file may have been deleted.",
        )
    })?;
    if md.len() > MAX_BYTES {
        return Err(CoreError::failed_fix(
            "That profile is too big to open.",
            format!(
                "It is {} MB; OLS opens profiles up to 300 MB.",
                md.len() / 1_048_576
            ),
            "Profile a lighter request (one page, not a whole crawl), or delete this file.",
        ));
    }
    if md.len() == 0 {
        return Err(CoreError::failed_fix(
            "That profile is empty.",
            "Xdebug did not write anything into it, usually because the request stopped early.",
            "Delete it and profile the request again.",
        ));
    }
    let f = std::fs::File::open(path).map_err(CoreError::from)?;
    parse_cachegrind(std::io::BufReader::new(f)).map_err(|why| {
        CoreError::failed_fix(
            "OLS could not read that profile.",
            why,
            "Delete it and profile the request again.",
        )
    })
}

// ------------------------------------------------------------------ settings toggle

/// What to do with the remembered `start_with_request` when profiling is toggled.
#[derive(Debug, PartialEq, Eq)]
pub enum PrevStart {
    Remember(String),
    Restore(String),
    Nothing,
}

/// Adds or removes `profile` without touching other modes (pure).
pub fn toggle_profiling(
    s: &XdebugSettings,
    on: bool,
    remembered: Option<&str>,
) -> (XdebugSettings, PrevStart) {
    let mut s = s.clone();
    let mut prev = PrevStart::Nothing;
    if on {
        if s.modes == ["off"] {
            s.modes = vec!["profile".into()];
        } else if !s.modes.iter().any(|m| m == "profile") {
            s.modes.push("profile".into());
        }
        if s.start_with_request == "yes" {
            prev = PrevStart::Remember("yes".into());
            s.start_with_request = "trigger".into();
        }
    } else {
        s.modes.retain(|m| m != "profile");
        if s.modes.is_empty() {
            s.modes = vec!["off".into()];
        }
        if let Some(old) = remembered {
            s.start_with_request = old.to_string();
            prev = PrevStart::Restore(old.to_string());
        }
    }
    (s, prev)
}

// ------------------------------------------------------------------ Inner

impl Inner {
    fn xdebug_dirs(&self) -> Vec<(String, PathBuf)> {
        self.php
            .all_versions()
            .into_iter()
            .map(|v| {
                let dir = self.php.ini_dir(&v).join("xdebug");
                (v, dir)
            })
            .collect()
    }

    pub fn list_profiles(&self) -> Vec<ProfileFile> {
        let mut out = Vec::new();
        for (version, dir) in self.xdebug_dirs() {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !is_profile_name(&name) {
                    continue;
                }
                let Ok(md) = e.metadata() else { continue };
                if !md.is_file() {
                    continue;
                }
                out.push(ProfileFile {
                    host: host_of(&name),
                    created_ms: md
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0),
                    size: md.len(),
                    php_version: version.clone(),
                    file: name,
                });
            }
        }
        out.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then(a.file.cmp(&b.file)));
        out
    }

    /// The path of a profile by name, validated against the Xdebug folders.
    fn profile_path(&self, file: &str) -> Result<PathBuf, CoreError> {
        if is_profile_name(file) {
            for (_, dir) in self.xdebug_dirs() {
                let p = dir.join(file);
                if p.is_file() {
                    return Ok(p);
                }
            }
        }
        Err(CoreError::failed_fix(
            "That profile does not exist.",
            format!("\"{file}\" is not a profile OLS knows."),
            "Refresh the list and pick a profile from it.",
        ))
    }

    pub fn get_profile(&self, file: &str) -> Result<Profile, CoreError> {
        parse_file(&self.profile_path(file)?)
    }

    pub fn delete_profiles(&self, files: &[String]) -> Result<(), CoreError> {
        let paths: Vec<PathBuf> = files
            .iter()
            .map(|f| self.profile_path(f))
            .collect::<Result<_, _>>()?;
        for p in paths {
            std::fs::remove_file(p)?;
        }
        Ok(())
    }

    /// Turns profiling on or off for the PHP version a site runs on.
    pub fn set_site_profiling(
        &self,
        hostname: &str,
        on: bool,
    ) -> Result<crate::xdebug::XdebugReport, CoreError> {
        let domain = self.domains.lock().unwrap().get(hostname).ok_or_else(|| {
            CoreError::failed_fix(
                "OLS cannot profile that site.",
                format!("No site called \"{hostname}\" exists."),
                "Pick a site from the Sites page.",
            )
        })?;
        let crate::domain::SiteKind::Php { version } = &domain.kind else {
            return Err(CoreError::failed_fix(
                "OLS can only profile PHP sites.",
                format!("{hostname} does not run PHP."),
                "Open a PHP site and try again.",
            ));
        };
        let ver = self.php.pick_version(version.as_deref()).ok_or_else(|| {
            CoreError::failed_fix(
                "PHP is not installed.",
                "Profiling needs the PHP version the site runs on.",
                "Install PHP on the Runtimes page.",
            )
        })?;
        let report = self.php.xdebug_report(&ver);
        if on && !(report.installed && report.enabled) {
            return Err(CoreError::failed_fix(
                "Xdebug is off for this PHP version.",
                format!("The profiler is part of Xdebug, and Xdebug is not enabled for PHP {ver}."),
                format!("Turn on Xdebug for PHP {ver} on the Runtimes page."),
            ));
        }
        let key = format!("xdebug.profiling_prev_start.{ver}");
        let remembered = self
            .settings
            .lock()
            .unwrap()
            .get(&key)
            .and_then(|v| v.as_str().map(str::to_string));
        let (settings, prev) = toggle_profiling(&report.settings, on, remembered.as_deref());
        self.php
            .set_xdebug_settings(&ver, &settings)
            .map_err(CoreError::ServiceError)?;
        match prev {
            PrevStart::Remember(v) => self
                .settings
                .lock()
                .unwrap()
                .set(&key, serde_json::json!(v))?,
            PrevStart::Restore(_) => self
                .settings
                .lock()
                .unwrap()
                .set(&key, serde_json::Value::Null)?,
            PrevStart::Nothing => {}
        }
        Ok(self.php.xdebug_report(&ver))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// main (self 100) -> a (self 200; calls b x2: 300 total) ; a recurses into itself once;
    /// main -> b directly (50). Name compression: ids seen with a name once, then bare.
    const FIXTURE: &str = "version: 1\ncreator: xdebug 3.4.0\ncmd: /var/www/index.php\npart: 1\npositions: line\n\nevents: Time_(10ns) Memory_(bytes)\n\nfl=(1) /var/www/index.php\nfn=(1) {main}\n1 100 0\ncfl=(2) /var/www/a.php\ncfn=(2) a\ncalls=1 0 0\n5 650 0\ncfl=(3) /var/www/b.php\ncfn=(3) b\ncalls=1 0 0\n6 50 0\n\nfl=(2)\nfn=(2)\n10 200 0\ncfl=(3)\ncfn=(3)\ncalls=2 0 0\n11 300 0\ncfn=(2)\ncalls=1 0 0\n12 100 0\n\nfl=(3)\nfn=(3)\n20 350 0\n\nsummary: 1300\n";

    #[test]
    fn exact_self_and_inclusive_numbers() {
        let p = parse_cachegrind(FIXTURE.as_bytes()).unwrap();
        assert_eq!(p.total_time_ns, 13_000);
        let f = |n: &str| p.functions.iter().find(|f| f.name == n).unwrap().clone();
        let (m, a, b) = (f("{main}"), f("a"), f("b"));
        assert_eq!(m.self_ns, 1_000);
        assert_eq!(a.self_ns, 2_000);
        assert_eq!(b.self_ns, 3_500);
        // main inclusive = self + a (650) + b (50)
        assert_eq!(m.inclusive_ns, (100 + 650 + 50) * 10);
        // a inclusive = self + b (300) + a (100, recursion)
        assert_eq!(a.inclusive_ns, (200 + 300 + 100) * 10);
        assert_eq!(b.inclusive_ns, 3_500);
        assert_eq!(a.calls, 2, "one call from main plus one recursive");
        assert_eq!(b.calls, 3);
        assert_eq!(a.file, "/var/www/a.php");
        // sorted by inclusive, biggest first
        assert_eq!(p.functions[0].name, "{main}");
    }

    #[test]
    fn recursion_does_not_loop_and_the_tree_starts_at_main() {
        let p = parse_cachegrind(FIXTURE.as_bytes()).unwrap();
        assert_eq!(p.tree.name, "{main}");
        let a = p.tree.children.iter().find(|c| c.name == "a").unwrap();
        assert!(
            a.children.iter().all(|c| c.name != "a"),
            "a must not appear under itself"
        );
        fn depth(n: &CallNode) -> usize {
            1 + n.children.iter().map(depth).max().unwrap_or(0)
        }
        assert!(depth(&p.tree) <= MAX_DEPTH + 1);
    }

    #[test]
    fn empty_and_garbage_files_are_errors_not_panics() {
        assert!(parse_cachegrind("".as_bytes()).is_err());
        assert!(parse_cachegrind("hello world\nnothing here\n".as_bytes()).is_err());
        let t = tempfile::tempdir().unwrap();
        let empty = t.path().join("cachegrind.out.1_1.x");
        std::fs::write(&empty, "").unwrap();
        assert!(parse_file(&empty).is_err());
        let junk = t.path().join("cachegrind.out.2_2.x");
        std::fs::write(&junk, "\u{0}\u{1}binary junk").unwrap();
        assert!(parse_file(&junk).is_err());
    }

    #[test]
    fn host_comes_from_the_file_name() {
        assert_eq!(
            host_of("cachegrind.out.1762600000_123456.shop.test").as_deref(),
            Some("shop.test")
        );
        assert_eq!(host_of("cachegrind.out.1762600000_123456"), None);
        assert_eq!(host_of("other.1.x"), None);
    }

    #[test]
    fn only_plain_cachegrind_names_are_profiles() {
        for bad in [
            "../x",
            "..\\x",
            "C:\\x",
            "foo.txt",
            "cachegrind.out.1/../../x",
            "",
            "cachegrind.out.1\\..\\x",
        ] {
            assert!(!is_profile_name(bad), "{bad:?}");
        }
        assert!(is_profile_name("cachegrind.out.1_2.shop.test"));
    }

    #[test]
    fn toggling_profiling_keeps_the_other_modes() {
        let base = XdebugSettings {
            modes: vec!["debug".into(), "develop".into()],
            start_with_request: "yes".into(),
            ..Default::default()
        };
        let (on, prev) = toggle_profiling(&base, true, None);
        assert_eq!(on.modes, vec!["debug", "develop", "profile"]);
        assert_eq!(on.start_with_request, "trigger");
        assert_eq!(prev, PrevStart::Remember("yes".into()));
        let (off, prev) = toggle_profiling(&on, false, Some("yes"));
        assert_eq!(off.modes, vec!["debug", "develop"]);
        assert_eq!(off.start_with_request, "yes");
        assert_eq!(prev, PrevStart::Restore("yes".into()));
        // off -> profile only -> off again
        let none = XdebugSettings {
            modes: vec!["off".into()],
            ..Default::default()
        };
        let (p, _) = toggle_profiling(&none, true, None);
        assert_eq!(p.modes, vec!["profile"]);
        let (back, _) = toggle_profiling(&p, false, None);
        assert_eq!(back.modes, vec!["off"]);
        assert!(back.validate().is_ok());
    }

    #[test]
    fn a_wide_call_graph_is_pruned_and_merged() {
        // main calls 100 functions of equal cost: only the ones above 0.5 % get nodes, up to
        // 30, and the rest are merged into "other".
        let mut text = String::from("events: Time_(10ns)\nfl=(1) /m.php\nfn=(1) {main}\n1 10 0\n");
        for i in 2..102 {
            text.push_str(&format!(
                "cfl=({i}) /f{i}.php\ncfn=({i}) f{i}\ncalls=1 0\n1 100\n"
            ));
        }
        for i in 2..102 {
            text.push_str(&format!("fl=({i})\nfn=({i})\n1 100\n"));
        }
        let p = parse_cachegrind(text.as_bytes()).unwrap();
        assert!(p.tree.children.len() <= MAX_CHILDREN + 1);
        assert!(p.tree.children.iter().any(|c| c.name == "other"));
    }
}
