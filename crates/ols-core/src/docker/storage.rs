//! §166: the Images and Volumes tabs — list, pull, run, remove — and disk usage with a
//! "Reclaim space" cleanup that shows exactly what it would delete before deleting it.
//!
//! Everything here is read on demand (tab opened, Refresh, an action); `/system/df` is
//! slow on a big engine, so its answer is cached for 60 seconds.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::ContainerSummary;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageSummary {
    /// `sha256:...`
    pub id: String,
    /// `repo:tag` names; empty for an untagged ("dangling") image.
    pub tags: Vec<String>,
    pub digest: Option<String>,
    pub size: u64,
    /// Unix seconds.
    pub created: i64,
    /// Containers (running or not) made from it.
    pub used_by: Vec<String>,
    pub dangling: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VolumeSummary {
    pub name: String,
    pub driver: String,
    pub created: Option<String>,
    /// Docker made it for an image's VOLUME line (no name was given).
    pub anonymous: bool,
    pub compose_project: Option<String>,
    /// Containers (running or not) that use it.
    pub used_by: Vec<String>,
    /// Bytes, when disk usage has been read (it is slow, so only on demand).
    pub size: Option<u64>,
}

/// Disk used by Docker, and how much of it is reclaimable.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiskUsage {
    pub images: u64,
    pub images_reclaimable: u64,
    pub containers: u64,
    pub containers_reclaimable: u64,
    pub volumes: u64,
    pub volumes_reclaimable: u64,
    pub build_cache: u64,
    pub build_cache_reclaimable: u64,
    /// Per-volume sizes from the same answer (name -> bytes).
    #[serde(skip)]
    pub volume_sizes: BTreeMap<String, u64>,
}

/// What "Reclaim space" may remove.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PruneTargets {
    #[serde(default)]
    pub stopped_containers: bool,
    /// Untagged images nothing uses.
    #[serde(default)]
    pub dangling_images: bool,
    /// Every image no container uses (tagged ones too).
    #[serde(default)]
    pub unused_images: bool,
    /// Volumes no container uses. Named volumes hold data, so this is opt-in.
    #[serde(default)]
    pub unused_volumes: bool,
    #[serde(default)]
    pub build_cache: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PruneItem {
    /// `container`, `image`, `volume`, `build cache`.
    pub kind: String,
    pub name: String,
    pub size: u64,
}

/// The dry run: exactly what a cleanup would remove.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrunePlan {
    pub items: Vec<PruneItem>,
    pub total: u64,
}

/// Create-and-start settings for "Run" on an image.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunSpec {
    pub image: String,
    #[serde(default)]
    pub name: Option<String>,
    /// (port on this computer, container port). Bound to 127.0.0.1 only.
    #[serde(default)]
    pub ports: Vec<(u16, u16)>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    /// `no`, `always`, `unless-stopped`, `on-failure`.
    #[serde(default)]
    pub restart_policy: Option<String>,
}

fn u(v: &Value) -> u64 {
    v.as_i64().map(|n| n.max(0) as u64).unwrap_or(0)
}

/// `GET /images/json` -> summaries, with the containers that use each image.
pub fn parse_images(v: &Value, containers: &[ContainerSummary]) -> Vec<ImageSummary> {
    let mut out: Vec<ImageSummary> = v
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| {
            let id = i["Id"].as_str().unwrap_or_default().to_string();
            let tags: Vec<String> = i["RepoTags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t.as_str())
                .filter(|t| *t != "<none>:<none>")
                .map(str::to_string)
                .collect();
            let digest = i["RepoDigests"]
                .as_array()
                .and_then(|d| d.first())
                .and_then(|d| d.as_str())
                .and_then(|d| d.split_once('@').map(|(_, h)| h.to_string()));
            let used_by = containers
                .iter()
                .filter(|c| c.image_id == id)
                .map(|c| c.name.clone())
                .collect();
            ImageSummary {
                dangling: tags.is_empty(),
                tags,
                digest,
                size: u(&i["Size"]),
                created: i["Created"].as_i64().unwrap_or(0),
                used_by,
                id,
            }
        })
        .collect();
    out.sort_by_key(|b| std::cmp::Reverse(b.created));
    out
}

/// A volume Docker created for an image's `VOLUME` line has a 64-hex name.
pub fn is_anonymous(name: &str, labels: &Value) -> bool {
    labels["com.docker.volume.anonymous"].is_string()
        || (name.len() == 64 && name.chars().all(|c| c.is_ascii_hexdigit()))
}

/// `GET /volumes` -> summaries, with the containers that use each volume.
pub fn parse_volumes(
    v: &Value,
    containers: &[ContainerSummary],
    sizes: &BTreeMap<String, u64>,
) -> Vec<VolumeSummary> {
    let mut out: Vec<VolumeSummary> = v["Volumes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|vol| {
            let name = vol["Name"].as_str().unwrap_or_default().to_string();
            VolumeSummary {
                driver: vol["Driver"].as_str().unwrap_or("local").into(),
                created: vol["CreatedAt"].as_str().map(str::to_string),
                anonymous: is_anonymous(&name, &vol["Labels"]),
                compose_project: vol["Labels"]["com.docker.compose.project"]
                    .as_str()
                    .map(str::to_string),
                used_by: containers
                    .iter()
                    .filter(|c| c.volumes.contains(&name))
                    .map(|c| c.name.clone())
                    .collect(),
                size: sizes.get(&name).copied(),
                name,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        (a.anonymous, &a.compose_project, &a.name).cmp(&(b.anonymous, &b.compose_project, &b.name))
    });
    out
}

/// `GET /system/df` -> totals and reclaimable amounts.
pub fn parse_disk_usage(v: &Value) -> DiskUsage {
    let mut d = DiskUsage::default();
    for i in v["Images"].as_array().into_iter().flatten() {
        let size = u(&i["Size"]);
        d.images += size;
        if i["Containers"].as_i64().unwrap_or(0) == 0 {
            d.images_reclaimable += size;
        }
    }
    for c in v["Containers"].as_array().into_iter().flatten() {
        let size = u(&c["SizeRw"]);
        d.containers += size;
        if c["State"] != "running" {
            d.containers_reclaimable += size;
        }
    }
    for vol in v["Volumes"].as_array().into_iter().flatten() {
        let size = u(&vol["UsageData"]["Size"]);
        d.volumes += size;
        if vol["UsageData"]["RefCount"].as_i64().unwrap_or(0) == 0 {
            d.volumes_reclaimable += size;
        }
        if let Some(n) = vol["Name"].as_str() {
            d.volume_sizes.insert(n.to_string(), size);
        }
    }
    for b in v["BuildCache"].as_array().into_iter().flatten() {
        let size = u(&b["Size"]);
        d.build_cache += size;
        if b["InUse"].as_bool() != Some(true) {
            d.build_cache_reclaimable += size;
        }
    }
    d
}

/// The dry run, from one `/system/df` answer. Only what the targets allow; nothing in use.
pub fn prune_plan(df: &Value, t: &PruneTargets) -> PrunePlan {
    let mut items = Vec::new();
    if t.stopped_containers {
        for c in df["Containers"].as_array().into_iter().flatten() {
            if c["State"] != "running" && c["State"] != "paused" && c["State"] != "restarting" {
                let name = c["Names"][0]
                    .as_str()
                    .unwrap_or_default()
                    .trim_start_matches('/')
                    .to_string();
                items.push(PruneItem {
                    kind: "container".into(),
                    name,
                    size: u(&c["SizeRw"]),
                });
            }
        }
    }
    if t.dangling_images || t.unused_images {
        // A stopped container still holds its image; when stopped containers go too, so does that hold.
        let removing_stopped = t.stopped_containers;
        let held: BTreeSet<String> = df["Containers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| {
                !removing_stopped
                    || matches!(
                        c["State"].as_str(),
                        Some("running" | "paused" | "restarting")
                    )
            })
            .filter_map(|c| c["ImageID"].as_str().map(str::to_string))
            .collect();
        for i in df["Images"].as_array().into_iter().flatten() {
            let id = i["Id"].as_str().unwrap_or_default();
            if held.contains(id) {
                continue;
            }
            let tags: Vec<&str> = i["RepoTags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t.as_str())
                .filter(|t| *t != "<none>:<none>")
                .collect();
            if tags.is_empty() || t.unused_images {
                let name = if tags.is_empty() {
                    id.chars().skip(7).take(12).collect()
                } else {
                    tags.join(", ")
                };
                items.push(PruneItem {
                    kind: "image".into(),
                    name,
                    size: u(&i["Size"]),
                });
            }
        }
    }
    if t.unused_volumes {
        for vol in df["Volumes"].as_array().into_iter().flatten() {
            if vol["UsageData"]["RefCount"].as_i64().unwrap_or(1) == 0 {
                items.push(PruneItem {
                    kind: "volume".into(),
                    name: vol["Name"].as_str().unwrap_or_default().into(),
                    size: u(&vol["UsageData"]["Size"]),
                });
            }
        }
    }
    if t.build_cache {
        let size: u64 = df["BuildCache"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["InUse"].as_bool() != Some(true))
            .map(|b| u(&b["Size"]))
            .sum();
        if size > 0 {
            items.push(PruneItem {
                kind: "build cache".into(),
                name: "Unused build cache".into(),
                size,
            });
        }
    }
    let total = items.iter().map(|i| i.size).sum();
    PrunePlan { items, total }
}

/// An image reference safe to put in a URL or a command: `name[:tag][@sha256:…]`, or an id.
pub fn valid_image_ref(r: &str) -> bool {
    !r.is_empty()
        && r.len() <= 255
        && !r.starts_with(['-', '/', '.'])
        && !r.contains("..")
        && r.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '/' | ':' | '@'))
}

/// Volume names Docker accepts.
pub fn valid_volume_name(n: &str) -> bool {
    n.len() >= 2
        && n.len() <= 255
        && n.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

/// The create body for "Run", after checking every value.
pub fn run_body(spec: &RunSpec) -> Result<Value, String> {
    if !valid_image_ref(spec.image.trim()) {
        return Err(format!("\"{}\" is not an image name.", spec.image));
    }
    if let Some(n) = spec.name.as_deref().filter(|n| !n.trim().is_empty()) {
        if !super::types::valid_container_ref(n.trim()) || n.len() > 64 {
            return Err(format!(
                "\"{n}\" cannot be a container name (letters, digits, _, . and -)."
            ));
        }
    }
    let mut exposed = serde_json::Map::new();
    let mut bindings = serde_json::Map::new();
    let mut hosts = BTreeSet::new();
    for (host, container) in &spec.ports {
        if *host == 0 || *container == 0 || !hosts.insert(*host) {
            return Err("Each port needs a port on this computer and in the container (1–65535), used once.".into());
        }
        let key = format!("{container}/tcp");
        exposed.insert(key.clone(), serde_json::json!({}));
        bindings.insert(
            key,
            serde_json::json!([{ "HostIp": "127.0.0.1", "HostPort": host.to_string() }]),
        );
    }
    let mut keys = BTreeSet::new();
    let mut env = Vec::new();
    for (k, v) in &spec.env {
        let k = k.trim();
        if k.is_empty() {
            continue;
        }
        let ok = k
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !ok || !keys.insert(k.to_string()) || v.contains('\n') {
            return Err(format!("\"{k}\" is not a usable variable (or it is listed twice, or its value has a line break)."));
        }
        env.push(format!("{k}={v}"));
    }
    let policy = spec.restart_policy.as_deref().unwrap_or("no");
    if !super::details::RESTART_POLICIES.contains(&policy) {
        return Err(format!("\"{policy}\" is not a restart policy."));
    }
    Ok(serde_json::json!({
        "Image": spec.image.trim(),
        "Env": env,
        "ExposedPorts": exposed,
        "Labels": { super::types::LABEL_MANAGED: "true" },
        "HostConfig": {
            "PortBindings": bindings,
            "RestartPolicy": { "Name": policy },
            "ExtraHosts": ["host.docker.internal:host-gateway"],
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container(name: &str, image_id: &str, volumes: &[&str]) -> ContainerSummary {
        ContainerSummary {
            id: name.into(),
            name: name.into(),
            image: String::new(),
            state: "running".into(),
            status: String::new(),
            created: 0,
            ports: vec![],
            compose_project: None,
            compose_service: None,
            compose_file: None,
            ols_site: None,
            managed: false,
            image_id: image_id.into(),
            volumes: volumes.iter().map(|v| v.to_string()).collect(),
        }
    }

    #[test]
    fn images_know_who_uses_them() {
        let v = serde_json::json!([
            {"Id": "sha256:aaa", "RepoTags": ["nginx:1.28"], "RepoDigests": ["nginx@sha256:fff"], "Size": 50, "Created": 10},
            {"Id": "sha256:bbb", "RepoTags": ["<none>:<none>"], "Size": 7, "Created": 20},
        ]);
        let imgs = parse_images(&v, &[container("web", "sha256:aaa", &[])]);
        assert_eq!(imgs[0].id, "sha256:bbb", "newest first");
        assert!(imgs[0].dangling && imgs[0].tags.is_empty());
        assert_eq!(imgs[1].used_by, vec!["web"]);
        assert_eq!(imgs[1].digest.as_deref(), Some("sha256:fff"));
    }

    #[test]
    fn volumes_know_who_uses_them() {
        let anon = "a".repeat(64);
        let v = serde_json::json!({"Volumes": [
            {"Name": "shop_db-data", "Driver": "local", "Labels": {"com.docker.compose.project": "shop"}},
            {"Name": anon, "Driver": "local", "Labels": null},
        ]});
        let sizes = BTreeMap::from([("shop_db-data".to_string(), 1024u64)]);
        let vols = parse_volumes(&v, &[container("db", "", &["shop_db-data"])], &sizes);
        assert_eq!(vols[0].name, "shop_db-data", "named first");
        assert_eq!(
            (vols[0].used_by.clone(), vols[0].size),
            (vec!["db".to_string()], Some(1024))
        );
        assert!(vols[1].anonymous && vols[1].used_by.is_empty());
    }

    fn df() -> Value {
        serde_json::json!({
            "Images": [
                {"Id": "sha256:used", "RepoTags": ["nginx:1.28"], "Size": 100, "Containers": 1},
                {"Id": "sha256:old", "RepoTags": ["redis:6"], "Size": 40, "Containers": 0},
                {"Id": "sha256:dangling123456789", "RepoTags": ["<none>:<none>"], "Size": 5, "Containers": 0},
                {"Id": "sha256:heldbystopped", "RepoTags": ["node:20"], "Size": 30, "Containers": 1}
            ],
            "Containers": [
                {"Names": ["/web"], "State": "running", "ImageID": "sha256:used", "SizeRw": 2},
                {"Names": ["/old-job"], "State": "exited", "ImageID": "sha256:heldbystopped", "SizeRw": 3}
            ],
            "Volumes": [
                {"Name": "data", "UsageData": {"Size": 70, "RefCount": 1}},
                {"Name": "orphan", "UsageData": {"Size": 9, "RefCount": 0}}
            ],
            "BuildCache": [{"Size": 11, "InUse": false}, {"Size": 4, "InUse": true}]
        })
    }

    #[test]
    fn disk_usage_totals_and_reclaimable() {
        let d = parse_disk_usage(&df());
        assert_eq!((d.images, d.images_reclaimable), (175, 45));
        assert_eq!((d.containers, d.containers_reclaimable), (5, 3));
        assert_eq!((d.volumes, d.volumes_reclaimable), (79, 9));
        assert_eq!((d.build_cache, d.build_cache_reclaimable), (15, 11));
        assert_eq!(d.volume_sizes["orphan"], 9);
    }

    #[test]
    fn prune_plan_lists_only_what_is_allowed_and_unused() {
        let only_dangling = prune_plan(
            &df(),
            &PruneTargets {
                dangling_images: true,
                ..Default::default()
            },
        );
        assert_eq!(only_dangling.items.len(), 1);
        assert_eq!(only_dangling.items[0].name, "dangling1234");
        let unused = prune_plan(
            &df(),
            &PruneTargets {
                unused_images: true,
                ..Default::default()
            },
        );
        let names: Vec<&str> = unused.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["redis:6", "dangling1234"],
            "in-use and stopped-held images stay"
        );
        let with_stopped = prune_plan(
            &df(),
            &PruneTargets {
                unused_images: true,
                stopped_containers: true,
                ..Default::default()
            },
        );
        assert!(
            with_stopped.items.iter().any(|i| i.name == "node:20"),
            "removing the stopped container frees its image"
        );
        assert!(with_stopped
            .items
            .iter()
            .any(|i| i.kind == "container" && i.name == "old-job"));
        let vols = prune_plan(
            &df(),
            &PruneTargets {
                unused_volumes: true,
                build_cache: true,
                ..Default::default()
            },
        );
        assert_eq!(vols.items.iter().map(|i| i.size).sum::<u64>(), vols.total);
        assert_eq!(vols.total, 9 + 11);
        assert!(prune_plan(&df(), &PruneTargets::default()).items.is_empty());
    }

    #[test]
    fn run_body_is_checked_and_loopback_only() {
        let b = run_body(&RunSpec {
            image: "nginx:1.28".into(),
            name: Some("my-web".into()),
            ports: vec![(8080, 80)],
            env: vec![("TZ".into(), "UTC".into())],
            restart_policy: Some("unless-stopped".into()),
        })
        .unwrap();
        assert_eq!(
            b["HostConfig"]["PortBindings"]["80/tcp"][0]["HostIp"],
            "127.0.0.1"
        );
        assert_eq!(b["Env"][0], "TZ=UTC");
        let bad = |s: RunSpec| run_body(&s).is_err();
        assert!(bad(RunSpec {
            image: "bad image".into(),
            ..Default::default()
        }));
        assert!(bad(RunSpec {
            image: "nginx".into(),
            name: Some("a b".into()),
            ..Default::default()
        }));
        assert!(bad(RunSpec {
            image: "nginx".into(),
            ports: vec![(80, 80), (80, 81)],
            ..Default::default()
        }));
        assert!(bad(RunSpec {
            image: "nginx".into(),
            env: vec![("1X".into(), "y".into())],
            ..Default::default()
        }));
        assert!(bad(RunSpec {
            image: "nginx".into(),
            restart_policy: Some("sometimes".into()),
            ..Default::default()
        }));
        assert!(valid_image_ref("ghcr.io/acme/api:2.4@sha256:abc"));
        assert!(!valid_image_ref("../etc") && !valid_image_ref("-x") && !valid_image_ref("a b"));
        assert!(
            valid_volume_name("shop_db-data")
                && !valid_volume_name("../x")
                && !valid_volume_name("a")
        );
    }
}
