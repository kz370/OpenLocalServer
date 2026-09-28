//! Global search (§123): one box that looks through projects, services, sites, Quick Apps,
//! Quick Commands, runtimes, web configs and logs. Each hit says where it lives, so the UI
//! can take the user straight there.

use serde::{Deserialize, Serialize};

use crate::app::Inner;

/// Results per kind, so one noisy log can't bury everything else.
const PER_KIND: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    /// project, service, site, quick_app, quick_command, runtime, config, log.
    pub kind: String,
    /// What to open: a project id, service id, hostname, app id, log source, ...
    pub target: String,
    pub title: String,
    pub subtitle: String,
    /// For configs and logs: the matching line.
    pub excerpt: Option<String>,
    /// Higher is better.
    #[serde(skip)]
    score: u32,
}

fn score(query: &str, text: &str) -> Option<u32> {
    let t = text.to_lowercase();
    if t == query {
        Some(100)
    } else if t.starts_with(query) {
        Some(80)
    } else if t
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(query))
    {
        Some(60)
    } else if t.contains(query) {
        Some(40)
    } else {
        None
    }
}

fn best(query: &str, fields: &[&str]) -> Option<u32> {
    fields
        .iter()
        .enumerate()
        .filter_map(|(i, f)| score(query, f).map(|s| s.saturating_sub(i as u32 * 5)))
        .max()
}

fn excerpt(line: &str, query: &str) -> String {
    let lower = line.to_lowercase();
    let at = lower.find(query).unwrap_or(0);
    let start = line
        .char_indices()
        .map(|(i, _)| i)
        .rfind(|i| *i + 60 <= at)
        .unwrap_or(0);
    let s: String = line[start..].chars().take(160).collect();
    if start > 0 {
        format!("…{}", s.trim())
    } else {
        s.trim().to_string()
    }
}

impl Inner {
    pub fn global_search(&self, query: &str) -> Vec<SearchHit> {
        let q = query.trim().to_lowercase();
        if q.len() < 2 {
            return Vec::new();
        }
        let mut hits: Vec<SearchHit> = Vec::new();
        let mut push = |kind: &str,
                        target: String,
                        title: String,
                        subtitle: String,
                        s: u32,
                        excerpt: Option<String>| {
            hits.push(SearchHit {
                kind: kind.into(),
                target,
                title,
                subtitle,
                excerpt,
                score: s,
            });
        };

        for p in self.projects.lock().unwrap().list() {
            if let Some(s) = best(&q, &[&p.name, &p.path]) {
                push(
                    "project",
                    p.id.clone(),
                    p.name.clone(),
                    p.path.clone(),
                    s + 10,
                    None,
                );
            }
        }
        for s in self.services.list() {
            if let Some(sc) = best(&q, &[&s.name, &s.id]) {
                let state = if s.running {
                    "running"
                } else if s.installed {
                    "stopped"
                } else {
                    "not installed"
                };
                push(
                    "service",
                    s.id.clone(),
                    s.name.clone(),
                    state.to_string(),
                    sc,
                    None,
                );
            }
        }
        for d in self.domains.lock().unwrap().list() {
            if let Some(s) = best(&q, &[&d.hostname, &d.root]) {
                push(
                    "site",
                    d.hostname.clone(),
                    d.hostname.clone(),
                    d.root.clone(),
                    s + 5,
                    None,
                );
            }
        }
        for a in self.catalog.lock().unwrap().list() {
            if let Some(s) = best(&q, &[&a.name, &a.id, &a.description]) {
                push(
                    "quick_app",
                    a.id.clone(),
                    a.name.clone(),
                    a.description.clone(),
                    s,
                    None,
                );
            }
        }
        for c in self.quick_commands.list() {
            if let Some(s) = best(&q, &[&c.name, &c.id, &c.description]) {
                push(
                    "quick_command",
                    c.id.clone(),
                    c.name.clone(),
                    c.description.clone(),
                    s,
                    None,
                );
            }
        }
        for r in self.runtimes.catalog() {
            if let Some(s) = best(&q, &[&r.name, &r.id]) {
                let state = if r.installed {
                    "installed"
                } else {
                    "available"
                };
                push(
                    "runtime",
                    r.id.clone(),
                    format!("{} {}", r.name, r.version),
                    state.to_string(),
                    s,
                    None,
                );
            }
        }
        for t in self.list_tunnels() {
            if let Some(s) = best(&q, &[&t.config.name, &t.config.target]) {
                push(
                    "tunnel",
                    t.config.id.clone(),
                    t.config.name.clone(),
                    t.config.target.clone(),
                    s,
                    None,
                );
            }
        }

        // Web configs: the text of every site's file, on the server that renders it.
        let cfg = self.web_config();
        let hosts: Vec<(String, String)> = self
            .domains
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .map(|d| {
                let server = crate::domain::resolved_server(&d, &cfg);
                (d.hostname, server)
            })
            .collect();
        let mut found = 0;
        for (h, server) in hosts {
            if found >= PER_KIND {
                break;
            }
            if let Ok(text) =
                self.web
                    .read_config(&server, Some(&h), crate::web::manager::ConfigPart::Site)
            {
                if let Some((n, line)) = text
                    .lines()
                    .enumerate()
                    .find(|(_, l)| l.to_lowercase().contains(&q))
                {
                    push(
                        "config",
                        h.clone(),
                        format!("{h} web config"),
                        format!("line {}", n + 1),
                        30,
                        Some(excerpt(line, &q)),
                    );
                    found += 1;
                }
            }
        }

        // Logs: the latest lines of each source, newest match first.
        for src in self.log_sources() {
            let Ok(lines) = self.read_log(&src.id, 2000) else {
                continue;
            };
            let mut n = 0;
            for line in lines.iter().rev().filter(|l| l.to_lowercase().contains(&q)) {
                push(
                    "log",
                    src.id.clone(),
                    src.name.clone(),
                    String::new(),
                    20,
                    Some(excerpt(line, &q)),
                );
                n += 1;
                if n >= 3 {
                    break;
                }
            }
        }

        hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(&b.title)));
        let mut per: std::collections::HashMap<String, usize> = Default::default();
        hits.retain(|h| {
            let c = per.entry(h.kind.clone()).or_default();
            *c += 1;
            *c <= PER_KIND
        });
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Core, CoreCommand};

    #[test]
    fn scoring_prefers_exact_then_prefix_then_word_then_substring() {
        assert!(score("shop", "shop") > score("shop", "shopify"));
        assert!(score("shop", "shopify") > score("shop", "my-shop"));
        assert!(score("shop", "my-shop") > score("shop", "workshop"));
        assert_eq!(score("zzz", "shop"), None);
    }

    #[test]
    fn projects_sites_and_commands_are_found() {
        let home = crate::test_support::isolated_home();
        let settings = crate::settings::SettingsService::load(&home.paths).unwrap();
        let core = Core::new(settings, home.paths.clone());
        let dir = home.paths.root().join("www").join("webshop");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "x").unwrap();
        core.dispatch(CoreCommand::RegisterProject {
            path: dir.display().to_string(),
        })
        .unwrap();
        let hits = core.inner().global_search("webshop");
        assert!(hits.iter().any(|h| h.kind == "project"), "{hits:?}");
        assert!(hits
            .iter()
            .any(|h| h.kind == "site" && h.target == "webshop.local"));
        assert!(
            core.inner().global_search("x").is_empty(),
            "one letter is too short to search"
        );
        assert!(core
            .inner()
            .global_search("migrat")
            .iter()
            .any(|h| h.kind == "quick_command"));
    }
}
