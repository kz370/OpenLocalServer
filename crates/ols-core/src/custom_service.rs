//! Custom services (§67): any program the user wants started, stopped and watched like the
//! built-in ones. A definition is a name, an executable, its arguments, an optional port and
//! an optional health check. Nothing is run through a shell; arguments are passed as given.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::paths::AppPaths;

/// Ids of custom services all start with this, which keeps them apart from the built-ins.
pub const ID_PREFIX: &str = "custom-";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HealthCheck {
    /// Don't check; "running" is all we know.
    #[default]
    None,
    /// The port accepts a connection.
    Tcp,
    /// `GET path` on the port answers with a 2xx or 3xx status.
    Http { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomService {
    /// `custom-<name>`; left empty when creating and filled in from the name.
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub health: HealthCheck,
    /// Start it again (up to 3 times) when it exits on its own.
    #[serde(default)]
    pub restart_on_crash: bool,
}

pub fn is_custom_id(id: &str) -> bool {
    id.starts_with(ID_PREFIX)
}

/// `My API (v2)` → `my-api-v2`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

fn valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn validate(s: &CustomService) -> Result<(), String> {
    let name = s.name.trim();
    if name.is_empty() || name.len() > 60 {
        return Err("give the service a name (up to 60 characters)".into());
    }
    if slug(name).is_empty() {
        return Err("the name needs at least one letter or digit".into());
    }
    if !std::path::Path::new(&s.executable).is_file() {
        return Err(format!("{} does not exist", s.executable));
    }
    if s.args.iter().any(|a| a.contains('\0')) {
        return Err("an argument contains an invalid character".into());
    }
    if let Some(cwd) = s.cwd.as_deref().filter(|c| !c.is_empty()) {
        if !std::path::Path::new(cwd).is_dir() {
            return Err(format!("the working folder {cwd} does not exist"));
        }
    }
    if let Some((key, _)) = s.env.iter().find(|(k, _)| !valid_env_key(k)) {
        return Err(format!(
            "\"{key}\" is not a valid environment variable name"
        ));
    }
    if s.port == Some(0) {
        return Err("the port must be between 1 and 65535".into());
    }
    match &s.health {
        HealthCheck::None => {}
        HealthCheck::Tcp if s.port.is_none() => return Err("a port check needs a port".into()),
        HealthCheck::Tcp => {}
        HealthCheck::Http { path } => {
            if s.port.is_none() {
                return Err("an HTTP check needs a port".into());
            }
            if !path.starts_with('/') || path.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err("the health check path must start with / and contain no spaces".into());
            }
        }
    }
    Ok(())
}

/// Runs the service's health check against the loopback interface.
pub fn probe(port: u16, health: &HealthCheck) -> Option<bool> {
    match health {
        HealthCheck::None => None,
        HealthCheck::Tcp => Some(connect(port).is_some()),
        HealthCheck::Http { path } => Some(http_ok(port, path)),
    }
}

fn connect(port: u16) -> Option<TcpStream> {
    TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(300),
    )
    .ok()
}

fn http_ok(port: u16, path: &str) -> bool {
    let Some(mut stream) = connect(port) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(700)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(700)));
    if stream
        .write_all(
            format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .is_err()
    {
        return false;
    }
    let mut head = [0u8; 32];
    let n = stream.read(&mut head).unwrap_or(0);
    status_ok(&head[..n])
}

/// `HTTP/1.1 204 No Content` → true for any 2xx/3xx.
fn status_ok(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(head);
    let mut parts = text.split_whitespace();
    parts.next().is_some_and(|p| p.starts_with("HTTP/"))
        && parts
            .next()
            .and_then(|c| c.parse::<u16>().ok())
            .is_some_and(|c| (200..400).contains(&c))
}

pub struct CustomServiceStore {
    paths: AppPaths,
    services: Vec<CustomService>,
}

impl CustomServiceStore {
    /// Missing or unreadable DB is an empty list; the app must still start.
    pub fn load(paths: &AppPaths) -> Self {
        let services = crate::db::load_docs(paths, "custom_services").unwrap_or_default();
        Self {
            paths: paths.clone(),
            services,
        }
    }

    pub fn list(&self) -> Vec<CustomService> {
        self.services.clone()
    }

    pub fn get(&self, id: &str) -> Option<CustomService> {
        self.services.iter().find(|s| s.id == id).cloned()
    }

    /// Adds a service, or replaces the one with the same id. A new one gets its id from its name.
    pub fn save(&mut self, mut service: CustomService) -> Result<CustomService, String> {
        validate(&service)?;
        service.name = service.name.trim().to_string();
        if service.id.is_empty() {
            let base = format!("{ID_PREFIX}{}", slug(&service.name));
            let mut id = base.clone();
            let mut n = 2;
            while self.services.iter().any(|s| s.id == id) {
                id = format!("{base}-{n}");
                n += 1;
            }
            service.id = id;
        } else if !is_custom_id(&service.id) {
            return Err("that is not a custom service".into());
        }
        match self.services.iter_mut().find(|s| s.id == service.id) {
            Some(existing) => *existing = service.clone(),
            None => self.services.push(service.clone()),
        }
        self.persist()?;
        Ok(service)
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        self.services.retain(|s| s.id != id);
        self.persist()
    }

    fn persist(&self) -> Result<(), String> {
        let refs: Vec<(String, &CustomService)> =
            self.services.iter().map(|s| (s.id.clone(), s)).collect();
        crate::db::save_docs(&self.paths, "custom_services", &refs).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn service(exe: &std::path::Path) -> CustomService {
        CustomService {
            id: String::new(),
            name: "My API (v2)".into(),
            executable: exe.display().to_string(),
            args: vec!["--port".into(), "9000".into()],
            cwd: None,
            env: vec![("MODE".into(), "dev".into())],
            port: Some(9000),
            health: HealthCheck::Tcp,
            restart_on_crash: false,
        }
    }

    #[test]
    fn slugs_are_lowercase_and_dash_separated() {
        assert_eq!(slug("My API (v2)"), "my-api-v2");
        assert_eq!(slug("  --Redis  "), "redis");
        assert_eq!(slug("???"), "");
    }

    #[test]
    fn validation_rejects_each_bad_field() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("svc.exe");
        std::fs::write(&exe, "x").unwrap();
        assert!(validate(&service(&exe)).is_ok());

        let mut s = service(&exe);
        s.name = "  ".into();
        assert!(validate(&s).is_err());
        let mut s = service(&exe);
        s.executable = dir.path().join("missing.exe").display().to_string();
        assert!(validate(&s).is_err());
        let mut s = service(&exe);
        s.env = vec![("BAD KEY".into(), "1".into())];
        assert!(validate(&s).is_err());
        let mut s = service(&exe);
        s.port = None;
        assert!(validate(&s).is_err(), "a port check needs a port");
        let mut s = service(&exe);
        s.health = HealthCheck::Http {
            path: "health".into(),
        };
        assert!(validate(&s).is_err());
        s.health = HealthCheck::Http {
            path: "/health".into(),
        };
        assert!(validate(&s).is_ok());
        let mut s = service(&exe);
        s.cwd = Some(dir.path().join("nope").display().to_string());
        assert!(validate(&s).is_err());
    }

    #[test]
    fn store_assigns_unique_ids_persists_and_removes() {
        let home = crate::test_support::isolated_home();
        let exe = home.paths.root().join("svc.exe");
        std::fs::write(&exe, "x").unwrap();

        let mut store = CustomServiceStore::load(&home.paths);
        let a = store.save(service(&exe)).unwrap();
        let b = store.save(service(&exe)).unwrap();
        assert_eq!(a.id, "custom-my-api-v2");
        assert_eq!(b.id, "custom-my-api-v2-2");

        let mut edited = a.clone();
        edited.args = vec![];
        store.save(edited).unwrap();
        assert_eq!(store.list().len(), 2, "saving an existing id replaces it");

        let reloaded = CustomServiceStore::load(&home.paths);
        assert_eq!(reloaded.list().len(), 2);
        assert!(reloaded.get(&a.id).unwrap().args.is_empty());

        let mut store = reloaded;
        store.remove(&a.id).unwrap();
        assert_eq!(CustomServiceStore::load(&home.paths).list().len(), 1);

        let mut not_custom = service(&exe);
        not_custom.id = "mariadb".into();
        assert!(store.save(not_custom).is_err());
    }

    #[test]
    fn http_status_lines_are_read_correctly() {
        assert!(status_ok(b"HTTP/1.1 200 OK\r\n"));
        assert!(status_ok(b"HTTP/1.0 302 Found\r\n"));
        assert!(!status_ok(b"HTTP/1.1 500 Internal Server Error\r\n"));
        assert!(!status_ok(b"SSH-2.0-OpenSSH\r\n"));
        assert!(!status_ok(b""));
    }

    #[test]
    fn probes_see_a_listening_port_and_a_closed_one() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert_eq!(probe(port, &HealthCheck::None), None);
        assert_eq!(probe(port, &HealthCheck::Tcp), Some(true));
        let closed = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        assert_eq!(probe(closed, &HealthCheck::Tcp), Some(false));
        assert_eq!(
            probe(closed, &HealthCheck::Http { path: "/".into() }),
            Some(false)
        );
    }
}
