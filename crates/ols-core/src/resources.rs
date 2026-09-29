//! Resource controls (§129). Optional limits, all off by default:
//!
//! - database / cache memory, passed to each server as its own setting when it starts
//!   (MariaDB's buffer pool, PostgreSQL's shared buffers, Redis' maxmemory, MongoDB's cache);
//! - CPU, capped through the `cpulimit` utility from kz370/win-utils, which puts the
//!   service in a Windows job object with a hard CPU rate cap — see [`cpu_cap`];
//! - Node's heap size, through `NODE_OPTIONS` for every project command;
//! - how many copies one queue worker may run, and how many processes the app may start.
//!
//! CPU was left out before because Windows has no per-process CPU cap without Job Objects.
//! It is not missing any more, but it is still not something this crate does itself: the
//! cap is `cpulimit.exe <percent> <program> <args>`, an external utility. The installer
//! ships that binary next to the app and [`cpu_cap`] looks for it there first, so a
//! default install needs no configuration; the limit is only honoured when the utility is
//! on disk, and the UI is told plainly when it is not instead of the cap being silently
//! dropped. Changes apply the next time a service starts.
//!
//! A percentage is a share of *total* CPU across all cores, not of one core — the same
//! meaning `cpulimit` gives it, passed through unchanged. Capping something to about one
//! core on an 8-core machine is therefore 12%, not 100%.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::error::CoreError;

const KEY: &str = "resources";

/// Where `cpulimit.exe` is looked for, in order: the directory of the running executable
/// (where the installer puts it, and where `elevate::helper_path` looks for the helper),
/// then the path the user configured, then `OLS_CPULIMIT_PATH`, the standard per-user
/// folder from the utility's own installer, then `PATH`. See [`ResourceLimits::find_cpu_limiter`]
/// for why the sibling comes first. It is shipped, not downloaded at runtime; provenance and
/// the pinned hash are in `vendor/cpulimit/`.
const LIMITER_ENV: &str = "OLS_CPULIMIT_PATH";
const LIMITER_EXE: &str = "cpulimit.exe";
const LIMITER_HINT: &str =
    "CPU limits need cpulimit.exe, which Open Local Server ships with. It is not in the install \
     folder — reinstall, or set the path to it in Resources.";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResourceLimits {
    #[serde(default)]
    pub mariadb_buffer_pool_mb: Option<u32>,
    #[serde(default)]
    pub postgres_shared_buffers_mb: Option<u32>,
    #[serde(default)]
    pub redis_maxmemory_mb: Option<u32>,
    #[serde(default)]
    pub memcached_max_memory_mb: Option<u32>,
    #[serde(default)]
    pub mongodb_cache_mb: Option<u32>,
    /// Total CPU the service may use, as a percentage of every core on the machine
    /// (1–100). Applied through the external `cpulimit` utility; see the module docs.
    #[serde(default)]
    pub cpu_percent: Option<u32>,
    /// The same cap expressed in threads: `threads / logical cores` of the machine, so the
    /// user does not have to work out the percentage for their own CPU. Mutually exclusive
    /// with `cpu_percent` — `validate` refuses a set with both.
    #[serde(default)]
    pub cpu_threads: Option<u32>,
    /// An explicit path to `cpulimit.exe`, for a build installed somewhere the search misses.
    #[serde(default)]
    pub cpu_limiter_path: Option<String>,
    #[serde(default)]
    pub node_max_old_space_mb: Option<u32>,
    #[serde(default)]
    pub max_worker_count: Option<u32>,
    /// Processes the app itself may run at once (services, sites, workers, commands).
    #[serde(default)]
    pub max_processes: Option<u32>,
    /// The most virtual users a load-test script may ask for (default 200).
    #[serde(default)]
    pub k6_max_vus: Option<u32>,
}

impl ResourceLimits {
    pub fn validate(&self) -> Result<(), String> {
        let check = |v: Option<u32>, min: u32, max: u32, what: &str| match v {
            Some(v) if v < min || v > max => Err(format!("{what} must be between {min} and {max}")),
            _ => Ok(()),
        };
        check(
            self.mariadb_buffer_pool_mb,
            16,
            65536,
            "The MariaDB buffer pool (MB)",
        )?;
        check(
            self.postgres_shared_buffers_mb,
            16,
            65536,
            "PostgreSQL shared buffers (MB)",
        )?;
        check(self.redis_maxmemory_mb, 8, 65536, "Redis memory (MB)")?;
        check(
            self.memcached_max_memory_mb,
            8,
            65536,
            "Memcached memory (MB)",
        )?;
        check(self.mongodb_cache_mb, 256, 65536, "The MongoDB cache (MB)")?;
        if self.cpu_percent.is_some() && self.cpu_threads.is_some() {
            return Err(
                "Set the CPU limit as either a percentage or a number of threads, not both".into(),
            );
        }
        check(self.cpu_percent, 1, 100, "The CPU limit (%)")?;
        check(
            self.cpu_threads,
            1,
            logical_cores(),
            "The CPU limit (threads)",
        )?;
        if self
            .cpu_limiter_path
            .as_deref()
            .is_some_and(|p| !p.trim().is_empty() && !PathBuf::from(p).is_file())
        {
            return Err(format!(
                "cpulimit.exe was not found at {}",
                self.cpu_limiter_path.as_deref().unwrap_or_default()
            ));
        }
        check(self.node_max_old_space_mb, 64, 65536, "Node memory (MB)")?;
        check(
            self.max_worker_count,
            1,
            crate::workers::MAX_COUNT,
            "Copies per worker",
        )?;
        check(self.max_processes, 4, 1000, "The process limit")?;
        check(self.k6_max_vus, 1, 5000, "The load-test virtual users")?;
        Ok(())
    }

    /// Extra command-line arguments for a service, from these limits.
    pub fn service_args(&self, id: &str) -> Vec<String> {
        match id {
            "mariadb" => self
                .mariadb_buffer_pool_mb
                .map(|m| vec![format!("--innodb-buffer-pool-size={m}M")])
                .unwrap_or_default(),
            "postgres" => self
                .postgres_shared_buffers_mb
                .map(|m| vec!["-c".into(), format!("shared_buffers={m}MB")])
                .unwrap_or_default(),
            "redis" => self
                .redis_maxmemory_mb
                .map(|m| {
                    vec![
                        "--maxmemory".into(),
                        format!("{m}mb"),
                        "--maxmemory-policy".into(),
                        "allkeys-lru".into(),
                    ]
                })
                .unwrap_or_default(),
            "memcached" => self
                .memcached_max_memory_mb
                .map(|m| vec!["-m".into(), m.to_string()])
                .unwrap_or_default(),
            "mongodb" => self
                .mongodb_cache_mb
                .map(|m| {
                    vec![
                        "--wiredTigerCacheSizeGB".into(),
                        format!("{:.2}", (m as f64 / 1024.0).max(0.25)),
                    ]
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// The cap as `cpulimit` takes it: a whole-number percentage of all cores, or `None`
    /// when no CPU limit is set. Threads are converted against the machine's own core
    /// count and rounded up, so asking for one thread on a 6-core box gives 17% (≈1.02
    /// cores) rather than 16% (0.96 of a core, which the rate cap then rounds to 1).
    pub fn effective_cpu_percent(&self) -> Option<u32> {
        if let Some(p) = self.cpu_percent {
            return Some(p.clamp(1, 100));
        }
        let threads = self.cpu_threads?;
        let cores = logical_cores();
        let pct = threads.saturating_mul(100).div_ceil(cores.max(1));
        Some(pct.clamp(1, 100))
    }

    /// `cpulimit.exe` if it is installed.
    ///
    /// The copy beside the running executable wins, and that is deliberate. The installer
    /// puts it there and the build hash-checks it, so it is the one binary we vouched for;
    /// a `cpu_limiter_path` the user set once is stored in a settings database that a dev
    /// build and an installed build share, so letting a stored path outrank the shipped
    /// copy would leave an installed app pointing at whatever folder a dev build once
    /// used. A configured path is therefore the fallback for when there is no sibling —
    /// a portable copy kept elsewhere, or a hand-built utility — which is the only case
    /// the UI offers the field in anyway.
    ///
    /// `OLS_CPULIMIT_PATH`, the utility's own per-user install folder and `PATH` follow,
    /// for a dev build run from `target\` where nothing was ever installed.
    pub fn find_cpu_limiter(&self) -> Option<PathBuf> {
        Self::find_cpu_limiter_from(
            std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(|p| p.to_path_buf())),
            self,
        )
    }

    /// The lookup itself, with the app's own directory passed in so the order is testable
    /// without depending on where the test binary happens to live.
    fn find_cpu_limiter_from(app_dir: Option<PathBuf>, limits: &ResourceLimits) -> Option<PathBuf> {
        if let Some(candidate) = app_dir.map(|d| d.join(LIMITER_EXE)).filter(|p| p.is_file()) {
            return Some(candidate);
        }
        if let Some(path) = limits
            .cpu_limiter_path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_file())
        {
            return Some(path);
        }
        if let Some(p) = std::env::var_os(LIMITER_ENV)
            .map(PathBuf::from)
            .filter(|p| p.is_file())
        {
            return Some(p);
        }
        // A dev build run from target\debug has no installed sibling, and pointing it at
        // whatever copy happens to sit in %LOCALAPPDATA% makes the Resources card show a
        // path that has nothing to do with this checkout. CARGO_MANIFEST_DIR is fixed at
        // compile time, so this is the vendored binary this build was made from — the same
        // bytes the installer ships, and the same one the build hash-checks. On a user's
        // machine the baked path does not exist, so this never fires outside a checkout.
        if let Some(dev) = repo_limiter() {
            return Some(dev);
        }
        // Last resort: a build installed with the utility's own install.bat.
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let candidate = PathBuf::from(local)
                .join("Programs")
                .join("cpulimit")
                .join(LIMITER_EXE);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(LIMITER_EXE))
                .find(|p| p.is_file())
        })
    }

    /// Wraps a process so it runs under the CPU cap: `cpulimit <percent> <program> <args>`.
    /// `cpulimit` exits with the program's own exit code and stays alive for its whole
    /// life, so the supervisor's stop/restart bookkeeping keeps working unchanged.
    ///
    /// `Ok(None)` means "no cap wanted" or "no cap possible" — a limit that cannot be
    /// applied is never silently pretended to be in force. `Err` is the case where the
    /// user asked for a cap, the utility is missing, and the reason says how to get it.
    pub fn cap_process(
        &self,
        program: &str,
        args: &[String],
    ) -> Result<Option<(String, Vec<String>)>, String> {
        self.cap_process_with(self.find_cpu_limiter(), program, args)
    }

    /// `cap_process` with the search already done, so the "wanted but not installed" refusal
    /// can be tested without depending on whether the machine running the tests happens to
    /// have the utility somewhere.
    fn cap_process_with(
        &self,
        limiter: Option<PathBuf>,
        program: &str,
        args: &[String],
    ) -> Result<Option<(String, Vec<String>)>, String> {
        let Some(percent) = self.effective_cpu_percent() else {
            return Ok(None);
        };
        let Some(limiter) = limiter else {
            return Err(LIMITER_HINT.to_string());
        };
        let mut capped = vec![percent.to_string(), program.to_string()];
        capped.extend(args.iter().cloned());
        Ok(Some((limiter.display().to_string(), capped)))
    }

    /// Whether a CPU limit set here can actually be applied, and why not when it cannot.
    /// The Resources card renders this instead of the old "not available on Windows" note.
    pub fn cpu_cap_status(&self) -> CpuCapStatus {
        CpuCapStatus {
            wanted: self.effective_cpu_percent().is_some(),
            limiter_path: self.find_cpu_limiter().map(|p| p.display().to_string()),
            logical_cores: logical_cores(),
            effective_percent: self.effective_cpu_percent(),
            hint: LIMITER_HINT.to_string(),
        }
    }
}

/// Logical cores on this machine. `available_parallelism` respects a process affinity
/// mask, which is the right answer for a cap: the user is capping this machine as the
/// app sees it, not every core the hardware has.
pub fn logical_cores() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1)
}

/// The vendored binary in this checkout, for a dev build run from `target\`. `None`
/// anywhere else, because the path was fixed at compile time and only exists on a machine
/// that has the repository.
fn repo_limiter() -> Option<PathBuf> {
    let candidate =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/cpulimit/cpulimit.exe");
    candidate.is_file().then_some(candidate)
}

/// What the Resources card needs to tell the truth about the CPU limit: whether one is
/// wanted, whether the utility to apply it is installed, and the percentage a thread
/// count works out to on this machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuCapStatus {
    /// A CPU limit is set in the settings (as a percentage or as threads).
    pub wanted: bool,
    /// `cpulimit.exe` when found; `None` means a wanted cap cannot be applied.
    pub limiter_path: Option<String>,
    pub logical_cores: u32,
    /// The percentage the cap will actually be applied at, or `None` when unset.
    pub effective_percent: Option<u32>,
    /// How to install the utility. Always filled in, so the card can say the way out.
    pub hint: String,
}

impl Inner {
    pub fn resource_limits(&self) -> ResourceLimits {
        self.settings
            .lock()
            .unwrap()
            .get(KEY)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    pub fn set_resource_limits(&self, limits: ResourceLimits) -> Result<(), CoreError> {
        limits.validate().map_err(CoreError::ServiceError)?;
        self.settings
            .lock()
            .unwrap()
            .set(KEY.to_string(), serde_json::to_value(&limits)?)?;
        self.services.set_limits(limits.clone());
        self.web.set_limits(limits);
        Ok(())
    }

    /// Room for one more process under the process limit.
    pub fn process_slot_free(&self) -> bool {
        match self.resource_limits().max_processes {
            Some(max) => {
                (self
                    .supervisor
                    .snapshot()
                    .into_iter()
                    .filter(|p| self.supervisor.is_alive(p.id))
                    .count() as u32)
                    < max
            }
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_become_each_servers_own_flags() {
        let l = ResourceLimits {
            mariadb_buffer_pool_mb: Some(256),
            redis_maxmemory_mb: Some(64),
            mongodb_cache_mb: Some(256),
            ..Default::default()
        };
        assert_eq!(
            l.service_args("mariadb"),
            ["--innodb-buffer-pool-size=256M"]
        );
        assert_eq!(
            l.service_args("redis")[..2],
            ["--maxmemory".to_string(), "64mb".to_string()]
        );
        assert_eq!(l.service_args("mongodb")[1], "0.25");
        assert!(
            l.service_args("postgres").is_empty(),
            "unset limits add nothing"
        );
    }

    #[test]
    fn silly_values_are_refused() {
        assert!(ResourceLimits {
            redis_maxmemory_mb: Some(1),
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(ResourceLimits {
            max_worker_count: Some(99),
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(ResourceLimits::default().validate().is_ok());
    }

    /// A percentage is a share of every core, so a thread count has to be converted
    /// against the machine's own core count and rounded up — one thread on a 6-core box is
    /// 17%, and 16% would be under the core the user asked for.
    #[test]
    fn threads_become_a_percentage_of_total_cpu() {
        let cores = logical_cores().max(1);
        let l = ResourceLimits {
            cpu_threads: Some(1),
            ..Default::default()
        };
        let pct = l.effective_cpu_percent().expect("a thread count is a cap");
        assert_eq!(pct, (100u32).div_ceil(cores));
        assert!((1..=100).contains(&pct), "a cap is always in range");

        let all = ResourceLimits {
            cpu_threads: Some(cores),
            ..Default::default()
        };
        assert_eq!(all.effective_cpu_percent(), Some(100));
    }

    #[test]
    fn a_percentage_is_passed_through_and_a_cap_needs_only_one_of_the_two() {
        assert_eq!(
            ResourceLimits {
                cpu_percent: Some(25),
                ..Default::default()
            }
            .effective_cpu_percent(),
            Some(25)
        );
        assert_eq!(
            ResourceLimits {
                cpu_percent: Some(0),
                ..Default::default()
            }
            .validate(),
            Err("The CPU limit (%) must be between 1 and 100".into())
        );
        let both = ResourceLimits {
            cpu_percent: Some(25),
            cpu_threads: Some(2),
            ..Default::default()
        };
        assert!(both.validate().is_err(), "two ways to say the same thing");
        assert!(
            ResourceLimits {
                cpu_threads: Some(logical_cores() + 1),
                ..Default::default()
            }
            .validate()
            .is_err(),
            "more threads than the machine has cores"
        );
    }

    /// The wrap is `cpulimit <percent> <program> <args>` — the utility stays in front for
    /// the whole life of the service, and the real program's arguments follow untouched.
    #[test]
    fn a_cap_wraps_the_program_in_cpulimit() {
        let dir = tempfile::tempdir().unwrap();
        let limiter = dir.path().join("cpulimit.exe");
        std::fs::write(&limiter, b"MZ").unwrap();
        let l = ResourceLimits {
            cpu_percent: Some(30),
            cpu_limiter_path: Some(limiter.display().to_string()),
            ..Default::default()
        };
        let (program, args) = l
            .cap_process("redis-server.exe", &["--port".into(), "6379".into()])
            .expect("the cap can be applied")
            .expect("a cap was asked for");
        assert_eq!(program, limiter.display().to_string());
        assert_eq!(args, ["30", "redis-server.exe", "--port", "6379"]);
    }

    #[test]
    fn no_cap_asked_for_adds_nothing() {
        assert_eq!(
            ResourceLimits::default()
                .cap_process("redis-server.exe", &["--port".into(), "6379".into()]),
            Ok(None)
        );
    }

    /// A wanted cap that cannot be applied is an error naming the way out — never a silent
    /// start of an uncapped service the UI would still describe as limited.
    #[test]
    fn a_wanted_cap_without_the_utility_is_refused_with_the_fix() {
        let l = ResourceLimits {
            cpu_percent: Some(30),
            cpu_limiter_path: Some(r"C:\nowhere\cpulimit.exe".into()),
            ..Default::default()
        };
        let err = l
            .cap_process_with(None, "redis-server.exe", &[])
            .expect_err("a cap that cannot be applied must not look like success");
        assert!(
            err.contains("cpulimit"),
            "the message names the utility: {err}"
        );
        assert!(l.validate().is_err(), "and a bad path is refused on save");
    }

    /// A saved path that no longer exists must not take the whole search down with it. It
    /// used to: the lookup returned early on "that file is missing", so a stale setting
    /// left the cap permanently dead even in an install that ships the utility beside the
    /// app.
    #[test]
    fn a_stale_saved_path_does_not_hide_the_copy_beside_the_app() {
        let app = tempfile::tempdir().unwrap();
        let shipped = app.path().join("cpulimit.exe");
        std::fs::write(&shipped, b"MZ").unwrap();
        let l = ResourceLimits {
            cpu_percent: Some(30),
            cpu_limiter_path: Some(r"C:\nowhere\cpulimit.exe".into()),
            ..Default::default()
        };
        let (program, _) = l
            .cap_process_with(
                ResourceLimits::find_cpu_limiter_from(Some(app.path().to_path_buf()), &l),
                "redis-server.exe",
                &[],
            )
            .expect("the shipped copy is found despite the stale setting")
            .expect("a cap was asked for");
        assert_eq!(program, shipped.display().to_string());
    }

    /// A dev build has no installed sibling, and the useful answer is the binary this
    /// checkout vendored — the same one the build hash-checks and the installer ships —
    /// rather than whatever copy happens to sit in %LOCALAPPDATA%.
    #[test]
    fn a_dev_build_resolves_the_vendored_binary_not_a_stray_install() {
        let vendored = repo_limiter().expect("this checkout has the vendored binary");
        assert!(
            vendored.ends_with("vendor/cpulimit/cpulimit.exe"),
            "the dev fallback is the vendored copy, got {}",
            vendored.display()
        );
    }

    /// The copy beside the app beats everything, including a path saved in settings. This
    /// matters because the settings database is shared: a path a user saved while running
    /// a dev build would otherwise keep an *installed* app pointing at that dev machine's
    /// folder.
    #[test]
    fn the_copy_beside_the_app_beats_a_path_saved_in_settings() {
        let app = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let shipped = app.path().join("cpulimit.exe");
        std::fs::write(&shipped, b"MZ").unwrap();
        let saved = other.path().join("cpulimit.exe");
        std::fs::write(&saved, b"MZ").unwrap();
        let limits = ResourceLimits {
            cpu_limiter_path: Some(saved.display().to_string()),
            ..Default::default()
        };
        assert_eq!(
            ResourceLimits::find_cpu_limiter_from(Some(app.path().to_path_buf()), &limits),
            Some(shipped),
            "an installed app resolves the limiter it shipped with, not a stale setting"
        );
    }

    /// With no copy beside the app, the path the user saved is what is used — the case the
    /// Resources card offers that field for.
    #[test]
    fn a_configured_path_is_used_when_the_app_has_no_sibling_copy() {
        let app = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let saved = other.path().join("cpulimit.exe");
        std::fs::write(&saved, b"MZ").unwrap();
        let limits = ResourceLimits {
            cpu_limiter_path: Some(saved.display().to_string()),
            ..Default::default()
        };
        assert_eq!(
            ResourceLimits::find_cpu_limiter_from(Some(app.path().to_path_buf()), &limits),
            Some(saved),
            "the escape hatch still works, and is the only thing it is for"
        );
    }
}
