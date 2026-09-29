//! System monitoring for the Processes page: machine-wide CPU and memory, free space on
//! the drives OLS uses, plus
//! CPU/memory for each process OLS manages — counted with its children,
//! since nginx, Apache and php-cgi do their work in child processes.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use sysinfo::{Disks, Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStats {
    /// 0–100, across all cores.
    pub cpu_percent: f32,
    pub cpu_cores: usize,
    pub memory_used: u64,
    pub memory_total: u64,
    /// Drives holding our data or the user's projects (not every drive on the machine).
    #[serde(default)]
    pub disks: Vec<DiskStats>,
    /// Keyed by the managed process's root PID.
    pub processes: HashMap<u32, ProcessStats>,
    /// What each site costs; filled in by the caller, which knows the sites.
    #[serde(default)]
    pub sites: Vec<SiteUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteUsage {
    pub hostname: String,
    /// What does its work: "App process", "PHP 8.4 workers", "Web server", ...
    pub via: String,
    pub cpu_percent: f32,
    pub memory: u64,
    /// How many sites share those processes (1 = this site alone).
    pub shared_by: usize,
    /// False when the work happens elsewhere (a Docker container, another computer).
    pub measured: bool,
    /// Size of the site's folder on disk; `None` until the first background count ends.
    #[serde(default)]
    pub disk: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskStats {
    /// "C:\"
    pub mount: String,
    pub used: u64,
    pub total: u64,
    /// What lives there: "OLS data", "Projects".
    pub holds: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessStats {
    /// 0–100 of the whole machine (not per core), summed over the process tree.
    pub cpu_percent: f32,
    pub memory: u64,
    /// Processes in the tree (1 = no children).
    pub count: usize,
}

/// CPU usage is measured between two refreshes, so one `System` lives for the app's life
/// and each poll reports usage since the previous one.
pub struct Monitor {
    sys: Mutex<System>,
    /// Free space barely moves; re-read it at most once a minute.
    disks: Mutex<Option<(Instant, Vec<DiskStats>)>>,
    /// Site folder sizes, counted in the background (walking vendor/ and node_modules/
    /// is slow) and refreshed every few minutes.
    folders: Arc<Mutex<FolderSizes>>,
}

#[derive(Default)]
struct FolderSizes {
    sizes: HashMap<PathBuf, u64>,
    counted_at: Option<Instant>,
    counting: bool,
}

impl Default for Monitor {
    fn default() -> Self {
        Self {
            sys: Mutex::new(System::new_with_specifics(RefreshKind::nothing())),
            disks: Mutex::new(None),
            folders: Arc::new(Mutex::new(FolderSizes::default())),
        }
    }
}

const DISK_REFRESH: Duration = Duration::from_secs(60);
const FOLDER_REFRESH: Duration = Duration::from_secs(600);

impl Monitor {
    /// Last known size of each folder. Starts a background count when the numbers are
    /// missing or old; never blocks the caller.
    pub fn folder_sizes(&self, folders: &[PathBuf]) -> HashMap<PathBuf, u64> {
        let mut state = self.folders.lock().unwrap();
        let missing = folders.iter().any(|f| !state.sizes.contains_key(f));
        let stale = state
            .counted_at
            .is_none_or(|t| t.elapsed() > FOLDER_REFRESH);
        if (missing || stale) && !state.counting {
            state.counting = true;
            let shared = self.folders.clone();
            let mut todo = folders.to_vec();
            todo.sort();
            todo.dedup();
            std::thread::spawn(move || {
                for folder in todo {
                    let size = tree_size(&folder);
                    shared.lock().unwrap().sizes.insert(folder, size);
                }
                let mut s = shared.lock().unwrap();
                s.counted_at = Some(Instant::now());
                s.counting = false;
            });
        }
        state.sizes.clone()
    }

    /// `places` are (label, folder) pairs; each drive holding one of them is reported.
    pub fn disks(&self, places: &[(String, PathBuf)]) -> Vec<DiskStats> {
        let mut cache = self.disks.lock().unwrap();
        if let Some((at, list)) = cache.as_ref() {
            if at.elapsed() < DISK_REFRESH {
                return list.clone();
            }
        }
        let all = Disks::new_with_refreshed_list();
        let mut out: Vec<DiskStats> = Vec::new();
        for (label, folder) in places {
            let folder = folder.display().to_string().to_ascii_lowercase();
            // The deepest mount point containing the folder is its drive.
            let Some(disk) = all
                .iter()
                .filter(|d| {
                    folder.starts_with(&d.mount_point().display().to_string().to_ascii_lowercase())
                })
                .max_by_key(|d| d.mount_point().as_os_str().len())
            else {
                continue;
            };
            let mount = disk.mount_point().display().to_string();
            match out.iter_mut().find(|d| d.mount == mount) {
                Some(d) if !d.holds.contains(label) => d.holds.push(label.clone()),
                Some(_) => {}
                None => out.push(DiskStats {
                    mount,
                    used: disk.total_space().saturating_sub(disk.available_space()),
                    total: disk.total_space(),
                    holds: vec![label.clone()],
                }),
            }
        }
        *cache = Some((Instant::now(), out.clone()));
        out
    }

    pub fn stats(&self, roots: &[u32]) -> SystemStats {
        let mut sys = self.sys.lock().unwrap();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );

        let cores = sys.cpus().len().max(1);
        let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
        for (pid, p) in sys.processes() {
            if let Some(parent) = p.parent() {
                children.entry(parent).or_default().push(*pid);
            }
        }
        let processes = roots
            .iter()
            .map(|&root| {
                let (mut cpu, mut memory, mut count) = (0.0f32, 0u64, 0usize);
                let mut stack = vec![Pid::from_u32(root)];
                // Windows reuses PIDs, so a stale parent link can form a loop.
                let mut seen = std::collections::HashSet::new();
                while let Some(pid) = stack.pop() {
                    if !seen.insert(pid) {
                        continue;
                    }
                    if let Some(p) = sys.process(pid) {
                        cpu += p.cpu_usage();
                        memory += p.memory();
                        count += 1;
                    }
                    if let Some(kids) = children.get(&pid) {
                        stack.extend(kids);
                    }
                }
                (
                    root,
                    ProcessStats {
                        cpu_percent: cpu / cores as f32,
                        memory,
                        count,
                    },
                )
            })
            .collect();

        SystemStats {
            cpu_percent: sys.global_cpu_usage(),
            cpu_cores: cores,
            memory_used: sys.used_memory(),
            memory_total: sys.total_memory(),
            disks: Vec::new(),
            processes,
            sites: Vec::new(),
        }
    }
}

/// Bytes under `dir`. Links and junctions are not followed, so a linked folder is neither
/// counted twice nor walked in a loop.
fn tree_size(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                stack.push(e.path());
            } else if let Ok(m) = e.metadata() {
                total += m.len();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_machine_and_a_live_process_tree() {
        let monitor = Monitor::default();
        let me = std::process::id();
        let stats = monitor.stats(&[me, u32::MAX - 1]);
        assert!(stats.memory_total > 0 && stats.memory_used <= stats.memory_total);
        assert!(stats.cpu_cores >= 1);
        assert!(stats.processes[&me].count >= 1 && stats.processes[&me].memory > 0);
        assert_eq!(
            stats.processes[&(u32::MAX - 1)].count,
            0,
            "an unknown PID reports nothing rather than failing"
        );
    }

    #[test]
    fn folder_sizes_are_counted_in_the_background() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("vendor").join("x")).unwrap();
        std::fs::write(dir.path().join("index.php"), vec![0u8; 1000]).unwrap();
        std::fs::write(
            dir.path().join("vendor").join("x").join("lib.php"),
            vec![0u8; 500],
        )
        .unwrap();
        let monitor = Monitor::default();
        let folder = dir.path().to_path_buf();
        for _ in 0..100 {
            if let Some(size) = monitor.folder_sizes(&[folder.clone()]).get(&folder) {
                assert_eq!(*size, 1500);
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the background count never finished");
    }
}
