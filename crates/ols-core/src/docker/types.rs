//! Docker data shown to the UI, CLI and MCP (§166). Engine JSON is parsed into these
//! smaller, stable shapes so the front doors never depend on Docker's own field names.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Label OLS puts on everything it creates.
pub const LABEL_MANAGED: &str = "dev.ols.managed";
/// Label naming the OLS site a container serves.
pub const LABEL_SITE: &str = "dev.ols.site";
const LABEL_COMPOSE_PROJECT: &str = "com.docker.compose.project";
const LABEL_COMPOSE_SERVICE: &str = "com.docker.compose.service";

/// Whether Docker is available, as the Docker page's status bar shows it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerStatus {
    /// Docker Desktop or the `docker` CLI was found on this computer.
    pub installed: bool,
    /// The engine answered.
    pub running: bool,
    pub version: Option<String>,
    pub api_version: Option<String>,
    /// `linux` or `windows` containers.
    pub os_type: Option<String>,
    /// Where OLS looked for the engine.
    pub endpoint: String,
    /// Docker Desktop's program file, when found (used by "Start Docker").
    pub desktop_path: Option<String>,
    /// Why the engine could not be reached, in plain words, when `running` is false.
    pub message: Option<String>,
}

/// One published (or exposed) container port.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortBinding {
    pub container_port: u16,
    /// `tcp` or `udp`.
    pub protocol: String,
    pub host_ip: Option<String>,
    /// `None` when the port is exposed inside Docker but not published to this computer.
    pub host_port: Option<u16>,
}

/// One row of the Containers tab.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContainerSummary {
    pub id: String,
    pub name: String,
    pub image: String,
    /// `running`, `exited`, `paused`, `restarting`, `created`, `dead`.
    pub state: String,
    /// Docker's own human text, e.g. "Up 3 hours".
    pub status: String,
    /// Unix seconds.
    pub created: i64,
    pub ports: Vec<PortBinding>,
    pub compose_project: Option<String>,
    pub compose_service: Option<String>,
    /// The OLS site this container serves (`dev.ols.site` label).
    pub ols_site: Option<String>,
    /// Created by OLS (`dev.ols.managed=true`).
    pub managed: bool,
}

/// What the Containers tab can do to one container.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContainerAction {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
}

impl ContainerAction {
    pub fn path_segment(self) -> &'static str {
        match self {
            ContainerAction::Start => "start",
            ContainerAction::Stop => "stop",
            ContainerAction::Restart => "restart",
            ContainerAction::Pause => "pause",
            ContainerAction::Unpause => "unpause",
        }
    }
}

// ---- engine JSON (only the fields we read) ----

#[derive(Debug, Deserialize)]
pub(crate) struct EngineVersion {
    #[serde(rename = "Version")]
    pub version: Option<String>,
    #[serde(rename = "ApiVersion")]
    pub api_version: Option<String>,
    #[serde(rename = "Os")]
    pub os: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EnginePort {
    #[serde(rename = "IP")]
    ip: Option<String>,
    #[serde(rename = "PrivatePort")]
    private_port: u16,
    #[serde(rename = "PublicPort")]
    public_port: Option<u16>,
    #[serde(rename = "Type")]
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EngineContainer {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Names", default)]
    names: Vec<String>,
    #[serde(rename = "Image", default)]
    image: String,
    #[serde(rename = "State", default)]
    state: String,
    #[serde(rename = "Status", default)]
    status: String,
    #[serde(rename = "Created", default)]
    created: i64,
    #[serde(rename = "Ports", default)]
    ports: Vec<EnginePort>,
    #[serde(rename = "Labels", default)]
    labels: Option<BTreeMap<String, String>>,
}

impl From<EngineContainer> for ContainerSummary {
    fn from(c: EngineContainer) -> Self {
        let labels = c.labels.unwrap_or_default();
        let name = c
            .names
            .first()
            .map(|n| n.trim_start_matches('/').to_string())
            .unwrap_or_else(|| c.id.chars().take(12).collect());
        // Docker lists a published port once per address family (0.0.0.0 and ::). Keep one,
        // preferring the IPv4 entry, and keep the list ordered by container port.
        let mut ports: Vec<PortBinding> = Vec::new();
        for p in c.ports {
            let protocol = p.kind.unwrap_or_else(|| "tcp".into());
            let dup = ports.iter_mut().find(|q| {
                q.container_port == p.private_port
                    && q.protocol == protocol
                    && q.host_port == p.public_port
            });
            match dup {
                Some(existing) => {
                    if existing
                        .host_ip
                        .as_deref()
                        .is_some_and(|ip| ip.contains(':'))
                        && p.ip.as_deref().is_some_and(|ip| !ip.contains(':'))
                    {
                        existing.host_ip = p.ip;
                    }
                }
                None => ports.push(PortBinding {
                    container_port: p.private_port,
                    protocol,
                    host_ip: p.ip,
                    host_port: p.public_port,
                }),
            }
        }
        ports.sort_by_key(|p| (p.container_port, p.host_port));
        ContainerSummary {
            id: c.id,
            name,
            image: c.image,
            state: c.state,
            status: c.status,
            created: c.created,
            ports,
            compose_project: labels.get(LABEL_COMPOSE_PROJECT).cloned(),
            compose_service: labels.get(LABEL_COMPOSE_SERVICE).cloned(),
            ols_site: labels.get(LABEL_SITE).cloned(),
            managed: labels.get(LABEL_MANAGED).is_some_and(|v| v == "true"),
        }
    }
}

/// A container id or name safe to put in a URL path: Docker ids are hex, names are
/// `[a-zA-Z0-9][a-zA-Z0-9_.-]*`.
pub fn valid_container_ref(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_json_becomes_summary() {
        let json = r#"[{
            "Id":"abc123","Names":["/shop-web-1"],"Image":"nginx:1.27","State":"running",
            "Status":"Up 2 hours","Created":1700000000,
            "Ports":[
                {"IP":"::","PrivatePort":80,"PublicPort":8080,"Type":"tcp"},
                {"IP":"0.0.0.0","PrivatePort":80,"PublicPort":8080,"Type":"tcp"},
                {"PrivatePort":443,"Type":"tcp"}
            ],
            "Labels":{"com.docker.compose.project":"shop","com.docker.compose.service":"web",
                      "dev.ols.managed":"true","dev.ols.site":"shop.test"}
        }]"#;
        let parsed: Vec<EngineContainer> = serde_json::from_str(json).unwrap();
        let c: ContainerSummary = parsed.into_iter().next().unwrap().into();
        assert_eq!(c.name, "shop-web-1");
        assert_eq!(c.compose_project.as_deref(), Some("shop"));
        assert_eq!(c.compose_service.as_deref(), Some("web"));
        assert_eq!(c.ols_site.as_deref(), Some("shop.test"));
        assert!(c.managed);
        assert_eq!(c.ports.len(), 2, "IPv4/IPv6 duplicate collapsed");
        assert_eq!(c.ports[0].container_port, 80);
        assert_eq!(c.ports[0].host_port, Some(8080));
        assert_eq!(c.ports[0].host_ip.as_deref(), Some("0.0.0.0"));
        assert_eq!(c.ports[1].host_port, None);
    }

    #[test]
    fn null_labels_are_fine() {
        let json = r#"[{"Id":"abcdef0123456789","Names":[],"Labels":null}]"#;
        let parsed: Vec<EngineContainer> = serde_json::from_str(json).unwrap();
        let c: ContainerSummary = parsed.into_iter().next().unwrap().into();
        assert_eq!(c.name, "abcdef012345");
        assert!(!c.managed);
    }

    #[test]
    fn container_refs_are_path_safe() {
        assert!(valid_container_ref("shop-web-1"));
        assert!(valid_container_ref("0123abcdef"));
        assert!(!valid_container_ref("../images"));
        assert!(!valid_container_ref("a/b"));
        assert!(!valid_container_ref("a?x=1"));
        assert!(!valid_container_ref(""));
        assert!(!valid_container_ref("-flag"));
    }
}
