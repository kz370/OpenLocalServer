//! §166: container settings — what `docker inspect` knows, and the changes Docker can make
//! to a running container without recreating it (name, restart policy, memory and CPU
//! limits). Read only when the settings dialog opens.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One `KEY=value` from the container's environment.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvVar {
    pub key: String,
    /// Empty and `masked` for password-like keys: the value never leaves the core.
    pub value: String,
    pub masked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MountInfo {
    /// `bind`, `volume`, `tmpfs`.
    pub kind: String,
    /// Folder on this computer, or the volume name.
    pub source: String,
    pub target: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkInfo {
    pub name: String,
    pub ip: Option<String>,
    pub aliases: Vec<String>,
}

/// Everything the settings dialog shows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContainerDetails {
    pub id: String,
    pub name: String,
    pub image: String,
    pub created: String,
    pub state: String,
    pub started_at: Option<String>,
    /// `healthy`, `unhealthy`, `starting`, when the image defines a health check.
    pub health: Option<String>,
    pub restart_count: u64,
    pub command: Vec<String>,
    pub entrypoint: Vec<String>,
    pub working_dir: Option<String>,
    pub env: Vec<EnvVar>,
    pub ports: Vec<super::types::PortBinding>,
    pub mounts: Vec<MountInfo>,
    pub networks: Vec<NetworkInfo>,
    pub labels: BTreeMap<String, String>,
    /// `no`, `always`, `unless-stopped`, `on-failure`.
    pub restart_policy: String,
    pub max_retries: u64,
    /// Memory limit in MB; 0 = no limit.
    pub memory_mb: u64,
    /// CPU limit in cores (e.g. 1.5); 0 = no limit.
    pub cpus: f64,
    pub compose_project: Option<String>,
}

/// Live changes. `None` leaves a setting as it is.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ContainerUpdate {
    #[serde(default)]
    pub restart_policy: Option<String>,
    #[serde(default)]
    pub max_retries: Option<u64>,
    /// 0 removes the limit.
    #[serde(default)]
    pub memory_mb: Option<u64>,
    /// 0 removes the limit.
    #[serde(default)]
    pub cpus: Option<f64>,
}

pub const RESTART_POLICIES: &[&str] = &["no", "always", "unless-stopped", "on-failure"];

fn looks_secret(key: &str) -> bool {
    let k = key.to_uppercase();
    [
        "PASSWORD",
        "PASSWD",
        "PASS",
        "SECRET",
        "TOKEN",
        "KEY",
        "CREDENTIAL",
        "PRIVATE",
    ]
    .iter()
    .any(|w| k.contains(w))
}

fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        Value::String(s) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string).filter(|s| !s.is_empty())
}

/// `GET /containers/{id}/json` -> details. Password-like environment values are dropped here.
pub fn parse(v: &Value) -> ContainerDetails {
    let config = &v["Config"];
    let host = &v["HostConfig"];
    let labels: BTreeMap<String, String> = config["Labels"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let env = strings(&config["Env"])
        .into_iter()
        .map(|kv| {
            let (k, val) = kv.split_once('=').unwrap_or((kv.as_str(), ""));
            let masked = looks_secret(k) && !val.is_empty();
            EnvVar {
                key: k.to_string(),
                value: if masked {
                    String::new()
                } else {
                    val.to_string()
                },
                masked,
            }
        })
        .collect();
    let mut ports = Vec::new();
    if let Some(map) = v["NetworkSettings"]["Ports"].as_object() {
        for (spec, bindings) in map {
            let (port, proto) = spec.split_once('/').unwrap_or((spec.as_str(), "tcp"));
            let Ok(container_port) = port.parse::<u16>() else {
                continue;
            };
            let list = bindings.as_array().cloned().unwrap_or_default();
            if list.is_empty() {
                ports.push(super::types::PortBinding {
                    container_port,
                    protocol: proto.into(),
                    host_ip: None,
                    host_port: None,
                });
            }
            for b in list {
                let host_port = b["HostPort"].as_str().and_then(|p| p.parse().ok());
                let host_ip = opt_str(&b["HostIp"]);
                // Same port on IPv4 and IPv6: keep one.
                if ports.iter().any(|p: &super::types::PortBinding| {
                    p.container_port == container_port
                        && p.host_port == host_port
                        && p.protocol == proto
                }) {
                    continue;
                }
                ports.push(super::types::PortBinding {
                    container_port,
                    protocol: proto.into(),
                    host_ip,
                    host_port,
                });
            }
        }
    }
    ports.sort_by_key(|p| (p.container_port, p.host_port));
    let mounts = v["Mounts"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|m| MountInfo {
                    kind: m["Type"].as_str().unwrap_or("bind").into(),
                    source: opt_str(&m["Name"])
                        .filter(|_| m["Type"] == "volume")
                        .or_else(|| opt_str(&m["Source"]))
                        .unwrap_or_default(),
                    target: m["Destination"].as_str().unwrap_or_default().into(),
                    read_only: m["RW"].as_bool() == Some(false),
                })
                .collect()
        })
        .unwrap_or_default();
    let networks = v["NetworkSettings"]["Networks"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(name, n)| NetworkInfo {
                    name: name.clone(),
                    ip: opt_str(&n["IPAddress"]),
                    aliases: strings(&n["Aliases"]),
                })
                .collect()
        })
        .unwrap_or_default();
    ContainerDetails {
        id: v["Id"].as_str().unwrap_or_default().into(),
        name: v["Name"]
            .as_str()
            .unwrap_or_default()
            .trim_start_matches('/')
            .into(),
        image: config["Image"].as_str().unwrap_or_default().into(),
        created: v["Created"].as_str().unwrap_or_default().into(),
        state: v["State"]["Status"].as_str().unwrap_or_default().into(),
        started_at: opt_str(&v["State"]["StartedAt"]).filter(|s| !s.starts_with("0001")),
        health: opt_str(&v["State"]["Health"]["Status"]),
        restart_count: v["RestartCount"].as_u64().unwrap_or(0),
        command: strings(&config["Cmd"]),
        entrypoint: strings(&config["Entrypoint"]),
        working_dir: opt_str(&config["WorkingDir"]),
        env,
        ports,
        mounts,
        networks,
        compose_project: labels.get("com.docker.compose.project").cloned(),
        labels,
        restart_policy: opt_str(&host["RestartPolicy"]["Name"]).unwrap_or_else(|| "no".into()),
        max_retries: host["RestartPolicy"]["MaximumRetryCount"]
            .as_u64()
            .unwrap_or(0),
        memory_mb: host["Memory"].as_u64().unwrap_or(0) / (1024 * 1024),
        cpus: host["NanoCpus"].as_u64().unwrap_or(0) as f64 / 1e9,
    }
}

/// Body for `POST /containers/{id}/update`, after checking the values.
pub fn update_body(u: &ContainerUpdate) -> Result<Value, String> {
    let mut body = serde_json::Map::new();
    if let Some(policy) = &u.restart_policy {
        if !RESTART_POLICIES.contains(&policy.as_str()) {
            return Err(format!(
                "\"{policy}\" is not a restart policy (no, always, unless-stopped, on-failure)."
            ));
        }
        let retries = if policy == "on-failure" {
            u.max_retries.unwrap_or(0).min(1000)
        } else {
            0
        };
        body.insert(
            "RestartPolicy".into(),
            serde_json::json!({ "Name": policy, "MaximumRetryCount": retries }),
        );
    }
    if let Some(mb) = u.memory_mb {
        if mb != 0 && !(6..=1024 * 1024).contains(&mb) {
            return Err("The memory limit must be at least 6 MB (or 0 for no limit).".into());
        }
        let bytes = mb * 1024 * 1024;
        body.insert("Memory".into(), bytes.into());
        // Docker requires swap >= memory; unlimited swap keeps the change to memory only.
        body.insert("MemorySwap".into(), (-1i64).into());
    }
    if let Some(cpus) = u.cpus {
        if !(0.0..=1024.0).contains(&cpus) || !cpus.is_finite() {
            return Err(
                "The CPU limit must be between 0 (no limit) and the number of cores.".into(),
            );
        }
        body.insert("NanoCpus".into(), ((cpus * 1e9).round() as u64).into());
    }
    if body.is_empty() {
        return Err("Nothing to change.".into());
    }
    Ok(Value::Object(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_json_becomes_details_without_secrets() {
        let v: Value = serde_json::from_str(r#"{
            "Id": "abc123", "Name": "/shop-db-1", "Created": "2026-11-01T10:00:00Z", "RestartCount": 2,
            "State": {"Status": "running", "StartedAt": "2026-11-05T09:00:00Z", "Health": {"Status": "healthy"}},
            "Config": {"Image": "mariadb:11.4", "Cmd": ["mariadbd"], "Entrypoint": null, "WorkingDir": "",
                "Env": ["MARIADB_PASSWORD=hunter2", "MARIADB_DATABASE=app", "API_TOKEN=", "TZ=UTC"],
                "Labels": {"com.docker.compose.project": "shop"}},
            "HostConfig": {"RestartPolicy": {"Name": "unless-stopped", "MaximumRetryCount": 0},
                "Memory": 536870912, "NanoCpus": 1500000000},
            "NetworkSettings": {
                "Ports": {"3306/tcp": [{"HostIp": "0.0.0.0", "HostPort": "3307"}, {"HostIp": "::", "HostPort": "3307"}], "33060/tcp": null},
                "Networks": {"shop_default": {"IPAddress": "172.18.0.2", "Aliases": ["db"]}}},
            "Mounts": [{"Type": "volume", "Name": "shop_db-data", "Source": "/var/lib/docker/volumes/x", "Destination": "/var/lib/mysql", "RW": true},
                       {"Type": "bind", "Source": "C:\\Sites\\shop", "Destination": "/app", "RW": false}]
        }"#).unwrap();
        let d = parse(&v);
        assert_eq!(d.name, "shop-db-1");
        assert_eq!(d.health.as_deref(), Some("healthy"));
        assert_eq!(
            (d.restart_policy.as_str(), d.memory_mb),
            ("unless-stopped", 512)
        );
        assert!((d.cpus - 1.5).abs() < 1e-9);
        let pw = d.env.iter().find(|e| e.key == "MARIADB_PASSWORD").unwrap();
        assert!(
            pw.masked && pw.value.is_empty(),
            "secret values never leave the core"
        );
        assert!(
            !d.env.iter().find(|e| e.key == "API_TOKEN").unwrap().masked,
            "empty values have nothing to hide"
        );
        assert_eq!(d.env.iter().find(|e| e.key == "TZ").unwrap().value, "UTC");
        assert_eq!(
            d.ports.len(),
            2,
            "IPv4/IPv6 duplicate collapsed, unpublished port kept"
        );
        assert_eq!(d.ports[0].host_port, Some(3307));
        assert_eq!(d.mounts[0].source, "shop_db-data");
        assert!(d.mounts[1].read_only);
        assert_eq!(d.networks[0].aliases, vec!["db"]);
        assert_eq!(d.compose_project.as_deref(), Some("shop"));
        assert!(d.entrypoint.is_empty() && d.working_dir.is_none());
    }

    #[test]
    fn update_bodies_are_checked() {
        let b = update_body(&ContainerUpdate {
            restart_policy: Some("on-failure".into()),
            max_retries: Some(5),
            memory_mb: Some(512),
            cpus: Some(1.5),
        })
        .unwrap();
        assert_eq!(b["RestartPolicy"]["MaximumRetryCount"], 5);
        assert_eq!(b["Memory"], 536870912u64);
        assert_eq!(b["MemorySwap"], -1);
        assert_eq!(b["NanoCpus"], 1500000000u64);
        let b = update_body(&ContainerUpdate {
            restart_policy: Some("always".into()),
            max_retries: Some(5),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            b["RestartPolicy"]["MaximumRetryCount"], 0,
            "retries only apply to on-failure"
        );
        assert!(update_body(&ContainerUpdate {
            restart_policy: Some("sometimes".into()),
            ..Default::default()
        })
        .is_err());
        assert!(update_body(&ContainerUpdate {
            memory_mb: Some(2),
            ..Default::default()
        })
        .is_err());
        assert!(update_body(&ContainerUpdate {
            cpus: Some(-1.0),
            ..Default::default()
        })
        .is_err());
        assert!(update_body(&ContainerUpdate::default()).is_err());
        assert_eq!(
            update_body(&ContainerUpdate {
                memory_mb: Some(0),
                ..Default::default()
            })
            .unwrap()["Memory"],
            0
        );
    }
}

/// One environment variable in an edit. `value: None` keeps the container's current value
/// (how a hidden password stays unchanged); a key left out of the list is removed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvEdit {
    pub key: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// What a recreate sends to Docker: the create body and the extra networks to join after.
#[derive(Debug, Clone, PartialEq)]
pub struct RecreatePlan {
    pub create_body: Value,
    pub extra_networks: Vec<(String, Vec<String>)>,
    pub env: Vec<String>,
}

fn valid_env_key(k: &str) -> bool {
    let mut c = k.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
        && k.len() <= 256
}

/// Builds the new container from the old one's `docker inspect`, with `edits` as its
/// environment. Pure: tested without Docker.
///
/// Data is never left behind: every volume the old container uses, including unnamed
/// ("anonymous") volumes Docker made for the image, is attached to the new one by name.
pub fn recreate_plan(inspect: &Value, edits: &[EnvEdit]) -> Result<RecreatePlan, String> {
    if inspect["Config"]["Labels"]["com.docker.compose.project"].is_string() {
        return Err("This container belongs to a Compose stack; change its environment in the compose file and press Up.".into());
    }
    let current: BTreeMap<String, String> = strings(&inspect["Config"]["Env"])
        .into_iter()
        .map(|kv| match kv.split_once('=') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => (kv, String::new()),
        })
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut env = Vec::new();
    for e in edits {
        let key = e.key.trim();
        if !valid_env_key(key) {
            return Err(format!("\"{key}\" is not a variable name (letters, digits and _, not starting with a digit)."));
        }
        if !seen.insert(key.to_string()) {
            return Err(format!("{key} is listed twice."));
        }
        let value = match &e.value {
            Some(v) => {
                if v.contains('\n') || v.contains('\r') || v.contains('\0') {
                    return Err(format!("The value of {key} has a line break."));
                }
                v.clone()
            }
            None => current
                .get(key)
                .cloned()
                .ok_or_else(|| format!("{key} is new, so it needs a value."))?,
        };
        env.push(format!("{key}={value}"));
    }

    let mut config = inspect["Config"].clone();
    let obj = config
        .as_object_mut()
        .ok_or("Docker's answer has no container config.")?;
    obj.insert("Env".into(), Value::from(env.clone()));
    // Docker fills these in for the new container.
    obj.remove("Hostname");
    let mut host = inspect["HostConfig"].clone();
    let host_obj = host
        .as_object_mut()
        .ok_or("Docker's answer has no host config.")?;
    // Keep every volume: named and anonymous ones the image created.
    let mut binds: Vec<String> = strings(&host_obj.get("Binds").cloned().unwrap_or(Value::Null));
    let bound_targets: std::collections::BTreeSet<String> = binds
        .iter()
        .filter_map(|b| b.split(':').nth(1).map(str::to_string))
        .chain(
            host_obj
                .get("Mounts")
                .and_then(|m| m.as_array())
                .into_iter()
                .flatten()
                .filter_map(|m| m["Target"].as_str().map(str::to_string)),
        )
        .collect();
    for m in inspect["Mounts"].as_array().into_iter().flatten() {
        let (Some(name), Some(dest)) = (m["Name"].as_str(), m["Destination"].as_str()) else {
            continue;
        };
        if m["Type"] == "volume" && !bound_targets.contains(dest) {
            let ro = if m["RW"].as_bool() == Some(false) {
                ":ro"
            } else {
                ""
            };
            binds.push(format!("{name}:{dest}{ro}"));
        }
    }
    host_obj.insert("Binds".into(), Value::from(binds));

    // The main network goes in the create call; the others are joined afterwards.
    let mode = inspect["HostConfig"]["NetworkMode"]
        .as_str()
        .unwrap_or("default")
        .to_string();
    let mut endpoints = serde_json::Map::new();
    let mut extra_networks = Vec::new();
    let short_id: String = inspect["Id"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(12)
        .collect();
    for (name, n) in inspect["NetworkSettings"]["Networks"]
        .as_object()
        .into_iter()
        .flatten()
    {
        // Docker adds the old container's short id as an alias; it would be wrong now.
        let aliases: Vec<String> = strings(&n["Aliases"])
            .into_iter()
            .filter(|a| *a != short_id)
            .collect();
        if *name == mode
            || (endpoints.is_empty()
                && matches!(mode.as_str(), "default" | "bridge")
                && name == "bridge")
        {
            endpoints.insert(name.clone(), serde_json::json!({ "Aliases": aliases }));
        } else {
            extra_networks.push((name.clone(), aliases));
        }
    }
    let mut body = config;
    let b = body.as_object_mut().expect("object");
    b.insert("HostConfig".into(), host);
    b.insert(
        "NetworkingConfig".into(),
        serde_json::json!({ "EndpointsConfig": Value::Object(endpoints) }),
    );
    Ok(RecreatePlan {
        create_body: body,
        extra_networks,
        env,
    })
}

#[cfg(test)]
mod recreate_tests {
    use super::*;

    fn inspect() -> Value {
        serde_json::from_str(r#"{
            "Id": "0123456789abcdef", "Name": "/uptime",
            "Config": {"Hostname": "0123456789ab", "Image": "louislam/uptime-kuma:1",
                "Env": ["TZ=UTC", "ADMIN_PASSWORD=s3cret", "PATH=/usr/bin"], "Labels": {}},
            "HostConfig": {"NetworkMode": "mynet", "Binds": ["C:\\data:/app/data"],
                "PortBindings": {"3001/tcp": [{"HostPort": "3001"}]}, "RestartPolicy": {"Name": "always"}},
            "Mounts": [
                {"Type": "bind", "Source": "C:\\data", "Destination": "/app/data", "RW": true},
                {"Type": "volume", "Name": "a1b2c3anon", "Destination": "/var/lib/cache", "RW": true}],
            "NetworkSettings": {"Networks": {
                "mynet": {"Aliases": ["uptime", "0123456789ab"]},
                "other": {"Aliases": []}}}
        }"#).unwrap()
    }

    #[test]
    fn plan_keeps_hidden_values_volumes_and_networks() {
        let edits = vec![
            EnvEdit {
                key: "TZ".into(),
                value: Some("Africa/Cairo".into()),
            },
            EnvEdit {
                key: "ADMIN_PASSWORD".into(),
                value: None,
            },
            EnvEdit {
                key: "NEW_FLAG".into(),
                value: Some("1".into()),
            },
        ];
        let p = recreate_plan(&inspect(), &edits).unwrap();
        assert_eq!(
            p.env,
            vec!["TZ=Africa/Cairo", "ADMIN_PASSWORD=s3cret", "NEW_FLAG=1"],
            "PATH removed, password kept"
        );
        let b = &p.create_body;
        assert_eq!(b["Image"], "louislam/uptime-kuma:1");
        assert!(b.get("Hostname").is_none());
        assert_eq!(b["HostConfig"]["RestartPolicy"]["Name"], "always");
        assert_eq!(
            b["HostConfig"]["PortBindings"]["3001/tcp"][0]["HostPort"],
            "3001"
        );
        let binds: Vec<&str> = b["HostConfig"]["Binds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            binds,
            vec!["C:\\data:/app/data", "a1b2c3anon:/var/lib/cache"],
            "anonymous volume kept by name"
        );
        assert_eq!(
            b["NetworkingConfig"]["EndpointsConfig"]["mynet"]["Aliases"],
            serde_json::json!(["uptime"])
        );
        assert_eq!(p.extra_networks, vec![("other".to_string(), vec![])]);
    }

    #[test]
    fn plan_refuses_bad_edits_and_compose_containers() {
        let bad = |edits: Vec<EnvEdit>| recreate_plan(&inspect(), &edits).is_err();
        assert!(bad(vec![EnvEdit {
            key: "1X".into(),
            value: Some("a".into())
        }]));
        assert!(bad(vec![
            EnvEdit {
                key: "A".into(),
                value: Some("x".into())
            },
            EnvEdit {
                key: "A".into(),
                value: Some("y".into())
            }
        ]));
        assert!(bad(vec![EnvEdit {
            key: "A".into(),
            value: Some("x\ny".into())
        }]));
        assert!(
            bad(vec![EnvEdit {
                key: "NEW".into(),
                value: None
            }]),
            "a new key needs a value"
        );
        let mut compose = inspect();
        compose["Config"]["Labels"]["com.docker.compose.project"] = "shop".into();
        assert!(recreate_plan(&compose, &[])
            .unwrap_err()
            .contains("compose file"));
    }
}
