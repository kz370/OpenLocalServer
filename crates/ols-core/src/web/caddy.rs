//! Caddy (§22, Stage 10): Caddyfile generation behind [`WebServer`]. Caddy's own automatic
//! HTTPS is switched off — certificates come from the OpenLocalServer CA like every other
//! server, so a site looks identical whichever server is active.

use std::path::PathBuf;

use super::{
    cfg_path, https_redirect_port_suffix, Backend, Invocation, PoolSpec, Ports, ServerLayout,
    SiteSpec, WebServer, MANAGED_HEADER,
};

pub struct Caddy;

/// Not Caddy's default 2019, which other tools (and a second Caddy) commonly hold.
const ADMIN_ADDR: &str = "127.0.0.1:20019";

impl Caddy {
    fn invocation(layout: &ServerLayout, sub: &[&str]) -> Invocation {
        let mut args: Vec<String> = sub.iter().map(|s| s.to_string()).collect();
        args.extend([
            "--config".to_string(),
            cfg_path(&layout.prefix.join("Caddyfile")),
            "--adapter".to_string(),
            "caddyfile".to_string(),
        ]);
        Invocation {
            args,
            cwd: layout.prefix.clone(),
        }
    }
}

impl WebServer for Caddy {
    fn id(&self) -> &'static str {
        "caddy"
    }
    fn name(&self) -> &'static str {
        "Caddy"
    }
    fn config_ext(&self) -> &'static str {
        "caddy"
    }
    fn main_config(&self, layout: &ServerLayout) -> PathBuf {
        layout.prefix.join("Caddyfile")
    }

    fn render_main(&self, layout: &ServerLayout, ports: Ports, _pools: &[PoolSpec]) -> String {
        let mut out = String::new();
        out.push_str(MANAGED_HEADER);
        out.push_str("{\n");
        out.push_str(&format!("    http_port {}\n", ports.http));
        out.push_str(&format!("    https_port {}\n", ports.https));
        out.push_str("    default_bind 127.0.0.1\n");
        out.push_str("    auto_https off\n");
        out.push_str(&format!("    admin {ADMIN_ADDR}\n"));
        out.push_str(&format!(
            "    log {{\n        output file \"{}\"\n        level WARN\n    }}\n",
            cfg_path(&layout.logs_dir.join("error.log"))
        ));
        out.push_str("}\n\n");
        out.push_str(&format!(
            "import \"{}/*.caddy\"\n",
            cfg_path(&layout.sites_dir)
        ));
        out
    }

    fn render_site(&self, site: &SiteSpec, ports: Ports) -> String {
        let mut out = String::new();
        out.push_str(MANAGED_HEADER);
        out.push_str(&format!("# site: {}\n\n", site.hostname));

        let names = site.server_names();
        let addrs = |scheme: &str, port: u16| {
            names
                .iter()
                .map(|n| format!("{scheme}://{n}:{port}"))
                .collect::<Vec<_>>()
                .join(", ")
        };

        out.push_str(&format!("{} {{\n", addrs("http", ports.http)));
        if site.tls.is_some() && site.redirect_https {
            out.push_str(&format!(
                "    redir https://{{host}}{}{{uri}} permanent\n",
                https_redirect_port_suffix(ports)
            ));
        } else {
            out.push_str(&body(site));
        }
        out.push_str("}\n");

        if let Some(tls) = &site.tls {
            out.push_str(&format!("\n{} {{\n", addrs("https", ports.https)));
            out.push_str(&format!(
                "    tls \"{}\" \"{}\"\n",
                cfg_path(&tls.cert),
                cfg_path(&tls.key)
            ));
            out.push_str(&body(site));
            out.push_str("}\n");
        }
        if let Some(host) = site.public_domain.as_deref() {
            let mut public = site.clone();
            public.hostname = host.to_string();
            public.wildcard = false;
            public.tls = None;
            public.redirect_https = false;
            public.forwarded_tls = true;
            out.push_str(&format!("\nhttp://{host}:{} {{\n", ports.http));
            out.push_str(&body(&public));
            out.push_str("}\n");
        }
        out
    }

    fn prepare(&self, layout: &ServerLayout) -> std::io::Result<()> {
        std::fs::create_dir_all(&layout.prefix)?;
        std::fs::create_dir_all(&layout.logs_dir)?;
        std::fs::create_dir_all(&layout.sites_dir)?;
        std::fs::create_dir_all(&layout.custom_dir)?;
        Ok(())
    }

    fn validate(&self, layout: &ServerLayout) -> Invocation {
        Self::invocation(layout, &["validate"])
    }
    fn start(&self, layout: &ServerLayout) -> Invocation {
        Self::invocation(layout, &["run"])
    }
    fn reload(&self, layout: &ServerLayout) -> Option<Invocation> {
        let mut inv = Self::invocation(layout, &["reload"]);
        inv.args
            .extend(["--address".to_string(), ADMIN_ADDR.to_string()]);
        Some(inv)
    }
    fn stop(&self, _layout: &ServerLayout) -> Option<Invocation> {
        Some(Invocation {
            args: vec![
                "stop".to_string(),
                "--address".to_string(),
                ADMIN_ADDR.to_string(),
            ],
            cwd: PathBuf::from("."),
        })
    }
    fn error_log(&self, layout: &ServerLayout) -> PathBuf {
        layout.logs_dir.join("error.log")
    }
}

fn body(site: &SiteSpec) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "    root * \"{}\"\n",
        site.root.replace('\\', "/")
    ));
    // Dev sites always revalidate (see nginx), unless the app chose its own caching.
    out.push_str("    header ?Cache-Control \"no-cache\"\n");
    for header in &site.blocks.headers {
        out.push_str(&format!(
            "    header {} \"{}\"\n",
            header.name, header.value
        ));
    }
    for include in &site.blocks.includes {
        out.push_str(&format!("    import \"{}\"\n", include.replace('\\', "/")));
    }
    if let Some(snippet) = &site.custom_snippet {
        out.push_str(&format!("    import \"{}\"\n", cfg_path(snippet)));
    }
    for redirect in &site.blocks.redirects {
        out.push_str(&format!(
            "    redir {} {} {}\n",
            redirect.from, redirect.to, redirect.code
        ));
    }
    for mapping in &site.blocks.mappings {
        let matcher = format!("{}*", mapping.path.trim_end_matches('*'));
        out.push_str(&format!(
            "    reverse_proxy {matcher} {}\n",
            mapping.upstream
        ));
    }
    match &site.backend {
        Backend::Php { ports, .. } => {
            let upstreams = ports
                .iter()
                .map(|p| format!("127.0.0.1:{p}"))
                .collect::<Vec<_>>()
                .join(" ");
            out.push_str(&format!("    php_fastcgi {upstreams}\n"));
            out.push_str("    file_server\n");
        }
        Backend::Proxy { upstream } => {
            out.push_str(&format!("    reverse_proxy {upstream}"));
            // Local HTTPS targets (Docker images) usually have self-signed certificates.
            if upstream.starts_with("https://") || site.forwarded_tls {
                out.push_str(" {\n");
                if upstream.starts_with("https://") {
                    out.push_str("        transport http {\n            tls_insecure_skip_verify\n        }\n");
                }
                if site.forwarded_tls {
                    out.push_str("        header_up X-Forwarded-Proto {http.request.header.X-Forwarded-Proto}\n");
                }
                out.push_str("    }");
            }
            out.push('\n');
        }
        Backend::Static => out.push_str("    file_server\n"),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::certs::CertPaths;
    use crate::domain::SiteBlocks;

    fn site(backend: Backend, tls: bool, redirect: bool) -> SiteSpec {
        SiteSpec {
            hostname: "shop.test".into(),
            wildcard: false,
            root: "C:\\sites\\shop".into(),
            backend,
            tls: tls.then(|| CertPaths {
                cert: "C:/c/cert.pem".into(),
                key: "C:/c/key.pem".into(),
            }),
            redirect_https: redirect,
            blocks: SiteBlocks::default(),
            custom_snippet: None,
            public_domain: None,
            forwarded_tls: false,
        }
    }

    #[test]
    fn php_https_site_uses_php_fastcgi_and_explicit_tls() {
        let cfg = Caddy.render_site(
            &site(
                Backend::Php {
                    pool: "php_82".into(),
                    ports: vec![10820, 10821],
                },
                true,
                true,
            ),
            Ports {
                http: 80,
                https: 443,
            },
        );
        assert!(cfg.contains("http://shop.test:80 {"));
        assert!(cfg.contains("redir https://{host}{uri} permanent"));
        assert!(cfg.contains("https://shop.test:443 {"));
        assert!(cfg.contains("tls \"C:/c/cert.pem\" \"C:/c/key.pem\""));
        assert!(cfg.contains("php_fastcgi 127.0.0.1:10820 127.0.0.1:10821"));
        assert!(cfg.contains("root * \"C:/sites/shop\""));
    }

    #[test]
    fn main_config_turns_off_auto_https_and_binds_loopback() {
        let layout = ServerLayout {
            install_dir: "C:/caddy".into(),
            prefix: "C:/ols/web/caddy".into(),
            sites_dir: "C:/ols/web/caddy/sites".into(),
            custom_dir: "C:/ols/web/caddy/custom".into(),
            logs_dir: "C:/ols/web/caddy/logs".into(),
            binary: "C:/caddy/caddy.exe".into(),
        };
        let cfg = Caddy.render_main(
            &layout,
            Ports {
                http: 8080,
                https: 8443,
            },
            &[],
        );
        assert!(cfg.contains("auto_https off"));
        assert!(cfg.contains("default_bind 127.0.0.1"));
        assert!(cfg.contains("http_port 8080"));
        assert!(cfg.contains("import \"C:/ols/web/caddy/sites/*.caddy\""));
    }

    #[test]
    fn caddy_invocations_use_the_caddyfile_adapter() {
        let layout = ServerLayout {
            install_dir: "C:/caddy".into(),
            prefix: "C:/ols/web/caddy".into(),
            sites_dir: "C:/ols/web/caddy/sites".into(),
            custom_dir: "C:/ols/web/caddy/custom".into(),
            logs_dir: "C:/ols/web/caddy/logs".into(),
            binary: "C:/caddy/caddy.exe".into(),
        };
        let v = Caddy.validate(&layout);
        assert_eq!(v.args[0], "validate");
        assert!(v.args.contains(&"caddyfile".to_string()));
    }
}
