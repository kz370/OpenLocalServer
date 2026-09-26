//! Nginx (§22–28): config generation and command lines. Pure rendering — nothing here
//! touches a running server.

use std::path::PathBuf;

use super::{
    cfg_path, https_redirect_port_suffix, Backend, Invocation, PoolSpec, Ports, ServerLayout, SiteSpec, WebServer,
    MANAGED_HEADER,
};

pub struct Nginx;

impl Nginx {
    fn common_args(layout: &ServerLayout) -> Vec<String> {
        vec![
            "-p".into(),
            format!("{}/", cfg_path(&layout.prefix)),
            "-c".into(),
            cfg_path(&layout.prefix.join("conf").join("nginx.conf")),
        ]
    }

    fn invocation(layout: &ServerLayout, extra: &[&str]) -> Invocation {
        let mut args = Self::common_args(layout);
        args.extend(extra.iter().map(|s| s.to_string()));
        Invocation { args, cwd: layout.prefix.clone() }
    }
}

impl WebServer for Nginx {
    fn id(&self) -> &'static str {
        "nginx"
    }
    fn name(&self) -> &'static str {
        "Nginx"
    }
    fn config_ext(&self) -> &'static str {
        "conf"
    }
    fn main_config(&self, layout: &ServerLayout) -> PathBuf {
        layout.prefix.join("conf").join("nginx.conf")
    }

    fn render_main(&self, layout: &ServerLayout, ports: Ports, pools: &[PoolSpec]) -> String {
        let mut out = String::new();
        out.push_str(MANAGED_HEADER);
        out.push_str("worker_processes 1;\n");
        out.push_str(&format!("error_log \"{}\";\n", cfg_path(&layout.logs_dir.join("error.log"))));
        out.push_str(&format!("pid \"{}\";\n\n", cfg_path(&layout.logs_dir.join("nginx.pid"))));
        out.push_str("events {\n    worker_connections 1024;\n}\n\n");
        out.push_str("http {\n");
        out.push_str("    include mime.types;\n");
        out.push_str("    default_type application/octet-stream;\n");
        out.push_str(&format!("    access_log \"{}\";\n", cfg_path(&layout.logs_dir.join("access.log"))));
        // sendfile holds files open on Windows, which blocks editors saving a served file.
        out.push_str("    sendfile off;\n");
        out.push_str("    keepalive_timeout 30;\n");
        out.push_str("    client_max_body_size 128m;\n");
        out.push_str("    server_names_hash_bucket_size 128;\n");
        out.push_str("    map $http_upgrade $connection_upgrade {\n        default upgrade;\n        '' close;\n    }\n\n");
        out.push_str("    map $http_x_forwarded_proto $ols_forwarded_proto { default $http_x_forwarded_proto; '' $scheme; }\n\n");

        for pool in pools {
            out.push_str(&format!("    upstream ols_{} {{\n", pool.id));
            for port in &pool.ports {
                out.push_str(&format!("        server 127.0.0.1:{port};\n"));
            }
            out.push_str("    }\n\n");
        }

        // A request for an unknown name must not silently land on the first site.
        out.push_str(&format!(
            "    server {{\n        listen 127.0.0.1:{} default_server;\n        server_name _;\n        return 404;\n    }}\n\n",
            ports.http
        ));
        out.push_str(&format!("    include \"{}/*.conf\";\n", cfg_path(&layout.sites_dir)));
        out.push_str("}\n");
        out
    }

    fn render_site(&self, site: &SiteSpec, ports: Ports) -> String {
        let mut out = String::new();
        out.push_str(MANAGED_HEADER);
        out.push_str(&format!("# site: {}\n\n", site.hostname));

        for upstream in &site.blocks.upstreams {
            out.push_str(&format!("upstream {} {{\n", upstream.name));
            for server in &upstream.servers {
                out.push_str(&format!("    server {server};\n"));
            }
            out.push_str("}\n\n");
        }

        let names = site.server_names().join(" ");
        let body = render_body(site);

        // Plain-HTTP server (§52): either the whole site, or a redirect to HTTPS.
        out.push_str("server {\n");
        out.push_str(&format!("    listen 127.0.0.1:{};\n", ports.http));
        out.push_str(&format!("    server_name {names};\n"));
        if site.tls.is_some() && site.redirect_https {
            out.push_str(&format!(
                "    return 301 https://$host{}$request_uri;\n",
                https_redirect_port_suffix(ports)
            ));
        } else {
            out.push_str(&body);
        }
        out.push_str("}\n");

        if let Some(tls) = &site.tls {
            out.push_str("\nserver {\n");
            out.push_str(&format!("    listen 127.0.0.1:{} ssl;\n", ports.https));
            out.push_str("    http2 on;\n");
            out.push_str(&format!("    server_name {names};\n"));
            out.push_str(&format!("    ssl_certificate \"{}\";\n", cfg_path(&tls.cert)));
            out.push_str(&format!("    ssl_certificate_key \"{}\";\n", cfg_path(&tls.key)));
            out.push_str("    ssl_protocols TLSv1.2 TLSv1.3;\n");
            out.push_str(&body);
            out.push_str("}\n");
        }
        if let Some(host) = site.public_domain.as_deref() {
            let mut public = site.clone();
            public.hostname = host.to_string();
            public.wildcard = false;
            public.tls = None;
            public.redirect_https = false;
            public.forwarded_tls = true;
            out.push_str("\nserver {\n");
            out.push_str(&format!("    listen 127.0.0.1:{};\n", ports.http));
            out.push_str(&format!("    server_name {host};\n"));
            out.push_str(&render_body(&public));
            out.push_str("}\n");
        }
        out
    }

    fn prepare(&self, layout: &ServerLayout) -> std::io::Result<()> {
        std::fs::create_dir_all(layout.prefix.join("conf"))?;
        std::fs::create_dir_all(&layout.logs_dir)?;
        std::fs::create_dir_all(&layout.sites_dir)?;
        std::fs::create_dir_all(&layout.custom_dir)?;
        // nginx creates client_body_temp etc. under here but not the folder itself.
        std::fs::create_dir_all(layout.prefix.join("temp"))?;
        let mime = layout.prefix.join("conf").join("mime.types");
        if !mime.exists() {
            std::fs::copy(layout.install_dir.join("conf").join("mime.types"), mime)?;
        }
        // Stock fastcgi_params, extended with what PHP frameworks expect.
        let fastcgi = layout.prefix.join("conf").join("fastcgi_params");
        if !fastcgi.exists() {
            std::fs::copy(layout.install_dir.join("conf").join("fastcgi_params"), fastcgi)?;
        }
        Ok(())
    }

    fn validate(&self, layout: &ServerLayout) -> Invocation {
        Self::invocation(layout, &["-t"])
    }
    fn start(&self, layout: &ServerLayout) -> Invocation {
        Self::invocation(layout, &[])
    }
    fn reload(&self, layout: &ServerLayout) -> Option<Invocation> {
        Some(Self::invocation(layout, &["-s", "reload"]))
    }
    fn stop(&self, layout: &ServerLayout) -> Option<Invocation> {
        Some(Self::invocation(layout, &["-s", "quit"]))
    }
    fn error_log(&self, layout: &ServerLayout) -> PathBuf {
        layout.logs_dir.join("error.log")
    }
}

/// Everything inside a `server { }` block after the listen/server_name lines.
/// Files get `expires -1` (Cache-Control: no-cache): the browser must check back on every
/// load instead of replaying a heuristically cached copy, which after a config fix would
/// keep "downloading" the old wrong response. Unchanged files still come back as a 304.
fn render_body(site: &SiteSpec) -> String {
    let mut out = String::new();
    let https = site.tls.is_some() || site.forwarded_tls;
    out.push_str(&format!("    root \"{}\";\n", site.root.replace('\\', "/")));
    out.push_str("    index index.php index.html index.htm;\n");

    for header in &site.blocks.headers {
        out.push_str(&format!("    add_header {} \"{}\" always;\n", header.name, header.value));
    }
    for include in &site.blocks.includes {
        out.push_str(&format!("    include \"{}\";\n", include.replace('\\', "/")));
    }
    if let Some(snippet) = &site.custom_snippet {
        out.push_str(&format!("    include \"{}\";\n", cfg_path(snippet)));
    }
    for redirect in &site.blocks.redirects {
        out.push_str(&format!("    location = {} {{\n        return {} {};\n    }}\n", redirect.from, redirect.code, redirect.to));
    }
    for mapping in &site.blocks.mappings {
        out.push_str(&format!("    location {} {{\n", mapping.path));
        out.push_str(&proxy_directives(&mapping.upstream, "        ", site.forwarded_tls));
        out.push_str("    }\n");
    }

    match &site.backend {
        Backend::Php { pool, .. } => {
            out.push_str("    location / {\n        try_files $uri $uri/ /index.php?$query_string;\n        expires -1;\n    }\n");
            out.push_str("    location ~ \\.php(/|$) {\n");
            out.push_str("        fastcgi_split_path_info ^(.+?\\.php)(/.*)$;\n");
            out.push_str("        include fastcgi_params;\n");
            out.push_str("        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;\n");
            out.push_str("        fastcgi_param PATH_INFO $fastcgi_path_info;\n");
            out.push_str("        fastcgi_param HTTP_X_FORWARDED_PROTO $ols_forwarded_proto;\n");
            if https {
                out.push_str("        fastcgi_param HTTPS on;\n");
            }
            out.push_str("        fastcgi_read_timeout 300;\n");
            out.push_str(&format!("        fastcgi_pass ols_{pool};\n"));
            out.push_str("    }\n");
            out.push_str("    location ~ /\\.(?!well-known) {\n        deny all;\n    }\n");
        }
        Backend::Proxy { upstream } => {
            out.push_str("    location / {\n");
            out.push_str(&proxy_directives(upstream, "        ", site.forwarded_tls));
            out.push_str("    }\n");
        }
        Backend::Static => {
            out.push_str("    location / {\n        try_files $uri $uri/ =404;\n        expires -1;\n    }\n");
        }
    }
    out
}

fn proxy_directives(upstream: &str, indent: &str, forwarded_tls: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!("{indent}proxy_pass {upstream};\n"));
    out.push_str(&format!("{indent}proxy_http_version 1.1;\n"));
    out.push_str(&format!("{indent}proxy_set_header Host $host;\n"));
    out.push_str(&format!("{indent}proxy_set_header X-Real-IP $remote_addr;\n"));
    out.push_str(&format!("{indent}proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n"));
    out.push_str(&format!("{indent}proxy_set_header X-Forwarded-Proto {};\n", if forwarded_tls { "$ols_forwarded_proto" } else { "$scheme" }));
    // WebSocket upgrade — Vite/Next HMR needs it.
    out.push_str(&format!("{indent}proxy_set_header Upgrade $http_upgrade;\n"));
    out.push_str(&format!("{indent}proxy_set_header Connection $connection_upgrade;\n"));
    out.push_str(&format!("{indent}proxy_read_timeout 300;\n"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::certs::CertPaths;
    use crate::domain::{HeaderRule, ProxyMapping, RedirectRule, SiteBlocks, UpstreamGroup};

    fn site(backend: Backend, tls: bool, redirect: bool) -> SiteSpec {
        SiteSpec {
            hostname: "shop.test".into(),
            wildcard: false,
            root: "C:\\sites\\shop\\public".into(),
            backend,
            tls: tls.then(|| CertPaths { cert: "C:/c/cert.pem".into(), key: "C:/c/key.pem".into() }),
            redirect_https: redirect,
            blocks: SiteBlocks::default(),
            custom_snippet: None,
            public_domain: None,
            forwarded_tls: false,
        }
    }

    const PORTS: Ports = Ports { http: 80, https: 443 };

    #[test]
    fn php_site_routes_php_to_its_pool_over_https() {
        let cfg = Nginx.render_site(&site(Backend::Php { pool: "php_81".into(), ports: vec![10810] }, true, true), PORTS);
        assert!(cfg.contains("server_name shop.test;"));
        assert!(cfg.contains("return 301 https://$host$request_uri;"), "http server must redirect");
        assert!(cfg.contains("listen 127.0.0.1:443 ssl;"));
        assert!(cfg.contains("ssl_certificate \"C:/c/cert.pem\";"));
        assert!(cfg.contains("fastcgi_pass ols_php_81;"));
        assert!(cfg.contains("fastcgi_param HTTPS on;"));
        assert!(cfg.contains("root \"C:/sites/shop/public\";"), "backslashes must become forward slashes");
    }

    #[test]
    fn redirect_toggle_off_serves_plain_http_directly() {
        let cfg = Nginx.render_site(&site(Backend::Static, true, false), PORTS);
        assert!(!cfg.contains("return 301"), "redirect disabled must not redirect (§52)");
        assert_eq!(cfg.matches("try_files $uri $uri/ =404;").count(), 2, "both http and https serve the site");
    }

    #[test]
    fn redirect_includes_a_non_default_https_port() {
        let cfg = Nginx.render_site(&site(Backend::Static, true, true), Ports { http: 8080, https: 8443 });
        assert!(cfg.contains("return 301 https://$host:8443$request_uri;"));
        assert!(cfg.contains("listen 127.0.0.1:8080;"));
    }

    #[test]
    fn proxy_site_forwards_with_websocket_upgrade() {
        let cfg = Nginx.render_site(&site(Backend::Proxy { upstream: "http://127.0.0.1:5173".into() }, false, false), PORTS);
        assert!(cfg.contains("proxy_pass http://127.0.0.1:5173;"));
        assert!(cfg.contains("proxy_set_header Upgrade $http_upgrade;"));
        assert!(!cfg.contains("ssl_certificate"));
    }

    #[test]
    fn wildcard_adds_a_wildcard_server_name() {
        let mut s = site(Backend::Static, false, false);
        s.wildcard = true;
        assert!(Nginx.render_site(&s, PORTS).contains("server_name shop.test *.shop.test;"));
    }

    #[test]
    fn structured_blocks_render_headers_redirects_mappings_upstreams() {
        let mut s = site(Backend::Static, false, false);
        s.blocks = SiteBlocks {
            headers: vec![HeaderRule { name: "X-Frame-Options".into(), value: "DENY".into() }],
            redirects: vec![RedirectRule { from: "/old".into(), to: "/new".into(), code: 301 }],
            mappings: vec![ProxyMapping { path: "/api/".into(), upstream: "http://127.0.0.1:8000".into() }],
            upstreams: vec![UpstreamGroup { name: "backend".into(), servers: vec!["127.0.0.1:9001".into()] }],
            includes: vec!["C:\\extra\\more.conf".into()],
        };
        s.custom_snippet = Some("C:/custom/shop.test.conf".into());
        let cfg = Nginx.render_site(&s, PORTS);
        assert!(cfg.contains("add_header X-Frame-Options \"DENY\" always;"));
        assert!(cfg.contains("location = /old {"));
        assert!(cfg.contains("return 301 /new;"));
        assert!(cfg.contains("location /api/ {"));
        assert!(cfg.contains("proxy_pass http://127.0.0.1:8000;"));
        assert!(cfg.contains("upstream backend {"));
        assert!(cfg.contains("include \"C:/extra/more.conf\";"));
        assert!(cfg.contains("include \"C:/custom/shop.test.conf\";"));
    }

    #[test]
    fn main_config_declares_php_pools_and_includes_sites() {
        let layout = ServerLayout {
            install_dir: "C:/nginx".into(),
            prefix: "C:/ols/web/nginx".into(),
            sites_dir: "C:/ols/web/nginx/sites".into(),
            custom_dir: "C:/ols/web/nginx/custom".into(),
            logs_dir: "C:/ols/web/nginx/logs".into(),
            binary: "C:/nginx/nginx.exe".into(),
        };
        let cfg = Nginx.render_main(&layout, PORTS, &[PoolSpec { id: "php_81".into(), ports: vec![10810, 10811] }]);
        assert!(cfg.contains("upstream ols_php_81 {"));
        assert!(cfg.contains("server 127.0.0.1:10811;"));
        assert!(cfg.contains("include \"C:/ols/web/nginx/sites/*.conf\";"));
        assert!(cfg.contains("default_server"));
    }
}
