//! End-of-life dates for runtimes and databases (§169, part A).
//!
//! The data is `catalog/eol.json`, built at development time by `scripts/update-eol.py` from
//! endoflife.date and compiled into the app. Nothing is fetched at runtime.

use std::collections::HashMap;
use std::sync::OnceLock;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

static DATA: &str = include_str!("../catalog/eol.json");

#[derive(Debug, Deserialize)]
struct File {
    #[serde(default)]
    products: HashMap<String, Vec<Cycle>>,
}

#[derive(Debug, Deserialize)]
struct Cycle {
    cycle: String,
    /// End of active support; security fixes may continue until `eol`.
    #[serde(default)]
    active_until: Option<String>,
    #[serde(default)]
    eol: Option<String>,
}

fn data() -> &'static File {
    static PARSED: OnceLock<File> = OnceLock::new();
    PARSED.get_or_init(|| {
        serde_json::from_str(DATA).unwrap_or(File {
            products: HashMap::new(),
        })
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EolStatus {
    Supported,
    /// Only security fixes until `until`.
    SecurityOnly {
        until: String,
    },
    /// No fixes any more since `since`.
    Ended {
        since: String,
    },
    Unknown,
}

/// A runtime or service id to the endoflife.date product name.
pub fn product_for(runtime_id: &str) -> Option<&'static str> {
    Some(match runtime_id {
        "php" => "php",
        "node" => "nodejs",
        "python" => "python",
        "mariadb" => "mariadb",
        "postgres" => "postgresql",
        "mongodb" => "mongodb",
        "redis" => "redis",
        "nginx" => "nginx",
        _ => return None,
    })
}

/// The release cycle `version` belongs to: the longest prefix, cut at a dot, that the data
/// knows (`8.3.14` -> `8.3`, Node `22.11.0` -> `22`).
pub fn cycle_of(product: &str, version: &str) -> Option<String> {
    let cycles = data().products.get(product)?;
    let version = version.trim().trim_start_matches('v');
    let mut prefix = version;
    loop {
        if cycles.iter().any(|c| c.cycle == prefix) {
            return Some(prefix.to_string());
        }
        match prefix.rfind('.') {
            Some(i) => prefix = &prefix[..i],
            None => return None,
        }
    }
}

fn date(text: &Option<String>) -> Option<NaiveDate> {
    text.as_deref()
        .and_then(|t| NaiveDate::parse_from_str(t, "%Y-%m-%d").ok())
}

/// Where `version` of `product` stands on `today`.
pub fn status(product: &str, version: &str, today: NaiveDate) -> EolStatus {
    let Some(cycle) = cycle_of(product, version) else {
        return EolStatus::Unknown;
    };
    let Some(entry) = data()
        .products
        .get(product)
        .and_then(|l| l.iter().find(|c| c.cycle == cycle))
    else {
        return EolStatus::Unknown;
    };
    if let (Some(eol), Some(text)) = (date(&entry.eol), entry.eol.clone()) {
        if today >= eol {
            return EolStatus::Ended { since: text };
        }
        if date(&entry.active_until).is_some_and(|a| today >= a) {
            return EolStatus::SecurityOnly { until: text };
        }
        return EolStatus::Supported;
    }
    // No end date known: nothing to warn about.
    EolStatus::Supported
}

/// `status` for the real clock.
pub fn status_now(product: &str, version: &str) -> EolStatus {
    status(product, version, chrono::Local::now().date_naive())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EolItem {
    pub runtime: String,
    pub version: String,
    pub status: EolStatus,
    /// Sites that run on it (PHP only).
    pub used_by: Vec<String>,
}

impl crate::app::Inner {
    /// Every installed runtime and database version with its end-of-life status. Cheap: the
    /// installed folders and the compiled-in dates only.
    pub fn eol_report(&self) -> Vec<EolItem> {
        self.eol_report_on(chrono::Local::now().date_naive())
    }

    pub(crate) fn eol_report_on(&self, today: NaiveDate) -> Vec<EolItem> {
        let mut items = Vec::new();
        // PHP: which sites run on which installed version.
        let sites = self.domains.lock().unwrap().list();
        for version in self.php.all_versions() {
            let used_by: Vec<String> = sites
                .iter()
                .filter(|d| {
                    matches!(&d.kind, crate::domain::SiteKind::Php { version: v }
                        if self.php.pick_version(v.as_deref()).as_deref() == Some(version.as_str()))
                })
                .map(|d| d.hostname.clone())
                .collect();
            items.push(EolItem {
                runtime: "php".into(),
                status: status("php", &version, today),
                version,
                used_by,
            });
        }
        for id in [
            "node", "python", "mariadb", "postgres", "mongodb", "redis", "nginx",
        ] {
            let Some(product) = product_for(id) else {
                continue;
            };
            for version in self.runtimes.installed_versions(id) {
                items.push(EolItem {
                    runtime: id.into(),
                    status: status(product, &version, today),
                    version,
                    used_by: Vec::new(),
                });
            }
        }
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn the_data_is_loaded_and_covers_every_product() {
        for id in [
            "php", "node", "python", "mariadb", "postgres", "mongodb", "redis", "nginx",
        ] {
            let p = product_for(id).unwrap();
            assert!(data().products.get(p).is_some_and(|l| !l.is_empty()), "{p}");
        }
        assert!(product_for("caddy").is_none());
    }

    #[test]
    fn versions_match_their_cycle_at_a_dot_boundary() {
        assert_eq!(cycle_of("php", "8.3.14").as_deref(), Some("8.3"));
        assert_eq!(cycle_of("nodejs", "22.11.0").as_deref(), Some("22"));
        assert_eq!(cycle_of("nodejs", "v22.11.0").as_deref(), Some("22"));
        // "8.1" must not swallow "8.10"-style prefixes by plain string matching.
        assert_ne!(cycle_of("php", "8.1.34").as_deref(), Some("8"));
        assert_eq!(cycle_of("php", "99.0.0"), None);
        assert_eq!(cycle_of("nope", "1.0"), None);
    }

    #[test]
    fn status_changes_at_the_boundaries_of_a_fixed_day() {
        // PHP 8.1: active support ended 2023-11-25, security fixes ended 2025-12-31.
        let st = |d: &str| status("php", "8.1.34", day(d));
        assert_eq!(st("2023-01-01"), EolStatus::Supported);
        assert_eq!(
            st("2023-11-25"),
            EolStatus::SecurityOnly {
                until: "2025-12-31".into()
            }
        );
        assert_eq!(
            st("2025-12-30"),
            EolStatus::SecurityOnly {
                until: "2025-12-31".into()
            }
        );
        assert_eq!(
            st("2025-12-31"),
            EolStatus::Ended {
                since: "2025-12-31".into()
            }
        );
        assert_eq!(
            st("2030-01-01"),
            EolStatus::Ended {
                since: "2025-12-31".into()
            }
        );
        assert_eq!(status("php", "99.1", day("2025-01-01")), EolStatus::Unknown);
        assert_eq!(
            status("unknown", "1", day("2025-01-01")),
            EolStatus::Unknown
        );
    }

    #[test]
    fn the_report_lists_installed_php_with_its_status_and_spawns_nothing() {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        let inner = crate::app::Inner::new(settings, home.paths.clone()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("ext")).unwrap();
        inner
            .php
            .set_external(vec![("8.1.34".into(), dir.path().join("php.exe"))]);
        let items = inner.eol_report_on(day("2026-06-01"));
        let php = items
            .iter()
            .find(|i| i.runtime == "php" && i.version == "8.1.34")
            .unwrap();
        assert!(matches!(php.status, EolStatus::Ended { .. }));
        // The diagnostics rule turns it into a warning with a stable id.
        let findings = inner.diagnose();
        assert!(
            findings.iter().any(|f| f.id == "eol:php:8.1"),
            "{:?}",
            findings.iter().map(|f| &f.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn node_22_is_supported_early_and_ended_late() {
        assert_eq!(
            status("nodejs", "22.11.0", day("2024-11-01")),
            EolStatus::Supported
        );
        assert!(matches!(
            status("nodejs", "22.11.0", day("2031-01-01")),
            EolStatus::Ended { .. }
        ));
    }
}
