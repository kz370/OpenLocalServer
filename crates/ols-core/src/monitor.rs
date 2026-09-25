//! System monitoring for the Processes page: machine-wide CPU and memory, free space on
//! the drives OpenLocalServer uses, plus
//! CPU/memory for each process OpenLocalServer manages — counted with its children,
//! since nginx, Apache and php-cgi do their work in child processes.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskStats {
    /// "C:\"
    pub mount: String,
    pub used: u64,
    pub total: u64,
    /// What lives there: "OpenLocalServer data", "Projects".
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
}

impl Default for Monitor {
    fn default() -> Self {
        Self { sys: Mutex::new(System::new_with_specifics(RefreshKind::nothing())), disks: Mutex::new(None) }
    }
}

const DISK_REFRESH: Duration = Duration::from_secs(60);

impl Monitor {
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
                .filter(|d| folder.starts_with(&d.mount_point().display().to_string().to_ascii_lowercase()))
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
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cpu().with_memory());

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
                (root, ProcessStats { cpu_percent: cpu / cores as f32, memory, count })
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
        assert_eq!(stats.processes[&(u32::MAX - 1)].count, 0, "an unknown PID reports nothing rather than failing");
    }
}
