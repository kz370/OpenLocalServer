//! Nginx (§22–28): config generation and command lines. Pure rendering — nothing here
//! touches a running server.

use std::path::PathBuf;

use super::{
    cfg_path, https_redirect_port_suffix, Backend, Invocation, PoolSpec, Ports, ServerLayout,
    SiteSpec, WebServer, MANAGED_HEADER,
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
        Invocation {
            args,
            cwd: layout.prefix.clone(),
        }
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
        out.push_str(&format!(
            "error_log \"{}\";\n",
            cfg_path(&layout.logs_dir.join("error.log"))
        ));
        out.push_str(&format!(
            "pid \"{}\";\n\n",
            cfg_path(&layout.logs_dir.join("nginx.pid"))
        ));
        out.push_str("events {\n    worker_connections 1024;\n}\n\n");
        out.push_str("http {\n");
        out.push_str("    include mime.types;\n");
        out.push_str("    default_type application/octet-stream;\n");
        out.push_str(&format!(
            "    access_log \"{}\";\n",
            cfg_path(&layout.logs_dir.join("access.log"))
        ));
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
        out.push_str(&format!(
            "    include \"{}/*.conf\";\n",
            cfg_path(&layout.sites_dir)
        ));
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
            out.push_str(&format!(
                "    ssl_certificate \"{}\";\n",
                cfg_path(&tls.cert)
            ));
            out.push_str(&format!(
                "    ssl_certificate_key \"{}\";\n",
                cfg_path(&tls.key)
            ));
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

    /// One `server` block for every `localhost/<prefix>` route on this server (§55).
    ///
    /// Each route proxies to the site's own vhost on the same server, with the prefix
    /// stripped and the site's hostname as `Host`. Going through the real vhost means the
    /// prefixed URL is answered by exactly the config the site already has — PHP pools,
    /// proxy upstreams, `Advanced` snippets and headers included — instead of a second
    /// copy of that logic that could drift from it.
    ///
    /// "The site's own vhost" is the one that *serves* it. A site with TLS and
    /// "Redirect to HTTPS" on answers its plain-HTTP vhost with nothing but
    /// `return 301 https://$host$request_uri`, and `$host` is the site hostname this
    /// route just set — so proxying to the HTTP port sent every prefixed request to
    /// `https://<site domain>` and the localhost route was unreachable for exactly the
    /// sites most likely to want it. Such a site is therefore reached over its HTTPS
    /// vhost, with SNI so the right `server` block answers.
    ///
    /// `X-Forwarded-Prefix` is sent because the prefix is stripped on the way in: without
    /// it an application rebuilds every absolute URL without `/<prefix>` and the browser
    /// leaves the prefixed route on its first link.
    fn render_path_routes(&self, sites: &[SiteSpec], ports: Ports) -> String {
        let mut out = String::new();
        out.push_str(MANAGED_HEADER);
        out.push_str("# localhost/<prefix> routes for the sites below.\n");
        out.push_str("# Each one is served by that site's own vhost on this server.\n\n");
        out.push_str("server {\n");
        out.push_str(&format!("    listen 127.0.0.1:{};\n", ports.http));
        out.push_str("    server_name localhost 127.0.0.1;\n");

        for site in sites
            .iter()
            .filter_map(|s| s.path_prefix.as_deref().map(|p| (s, p)))
        {
            let (site, prefix) = site;
            let tls = site.tls.is_some();
            out.push('\n');
            out.push_str(&format!("    # {}\n", site.hostname));
            // `http://localhost/shop` is a directory, not a page: send it to `shop/`
            // so the site's own relative links resolve.
            out.push_str(&format!(
                "    location = /{prefix} {{ return 301 /{prefix}/; }}\n"
            ));
            out.push_str(&format!("    location /{prefix}/ {{\n"));
            out.push_str(&format!(
                "        proxy_pass {}://127.0.0.1:{}/;\n",
                if tls { "https" } else { "http" },
                if tls { ports.https } else { ports.http }
            ));
            if tls {
                // The certificate is issued for the site hostname by the app's own local
                // CA, and nginx has no reason to hold that CA: this hop never leaves the
                // loopback interface, and the name is checked against the certificate below.
                out.push_str("        proxy_ssl_server_name on;\n");
                out.push_str(&format!("        proxy_ssl_name {};\n", site.hostname));
                out.push_str("        proxy_ssl_verify off;\n");
            }
            out.push_str(&format!(
                "        proxy_set_header Host {};\n",
                site.hostname
            ));
            out.push_str(&format!(
                "        proxy_set_header X-Forwarded-Prefix /{prefix};\n"
            ));
            out.push_str(&proxy_directives_tail("        ", false));
            out.push_str("    }\n");
        }
        out.push_str("\n    location / { return 404; }\n");
        out.push_str("}\n");
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
            std::fs::copy(
                layout.install_dir.join("conf").join("fastcgi_params"),
                fastcgi,
            )?;
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
        out.push_str(&format!(
            "    add_header {} \"{}\" always;\n",
            header.name, header.value
        ));
    }
    for include in &site.blocks.includes {
        out.push_str(&format!(
            "    include \"{}\";\n",
            include.replace('\\', "/")
        ));
    }
    if let Some(snippet) = &site.custom_snippet {
        out.push_str(&format!("    include \"{}\";\n", cfg_path(snippet)));
    }
    for redirect in &site.blocks.redirects {
        out.push_str(&format!(
            "    location = {} {{\n        return {} {};\n    }}\n",
            redirect.from, redirect.code, redirect.to
        ));
    }
    for mapping in &site.blocks.mappings {
        out.push_str(&format!("    location {} {{\n", mapping.path));
        out.push_str(&proxy_directives(
            &mapping.upstream,
            "        ",
            site.forwarded_tls,
        ));
        out.push_str("    }\n");
    }

    match &site.backend {
        Backend::Php { pool, .. } => {
            out.push_str("    location / {\n        try_files $uri $uri/ /index.php?$query_string;\n        expires -1;\n    }\n");
            out.push_str("    location ~ \\.php(/|$) {\n");
            out.push_str("        fastcgi_split_path_info ^(.+?\\.php)(/.*)$;\n");
            out.push_str("        include fastcgi_params;\n");
            out.push_str(
                "        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;\n",
            );
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
    out.push_str(&proxy_directives_tail(indent, forwarded_tls));
    out
}

/// Everything a proxied request needs after its destination and `Host` are set: the
/// client address, the forwarded-scheme header, WebSocket upgrade and the read timeout.
/// Shared by the per-site `proxy_pass` and the `localhost/<prefix>` routes (§55).
fn proxy_directives_tail(indent: &str, forwarded_tls: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{indent}proxy_set_header X-Real-IP $remote_addr;\n"
    ));
    out.push_str(&format!(
        "{indent}proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n"
    ));
    out.push_str(&format!(
        "{indent}proxy_set_header X-Forwarded-Proto {};\n",
        if forwarded_tls {
            "$ols_forwarded_proto"
        } else {
            "$scheme"
        }
    ));
    // WebSocket upgrade — Vite/Next HMR needs it.
    out.push_str(&format!(
        "{indent}proxy_set_header Upgrade $http_upgrade;\n"
    ));
    out.push_str(&format!(
        "{indent}proxy_set_header Connection $connection_upgrade;\n"
    ));
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
            tls: tls.then(|| CertPaths {
                cert: "C:/c/cert.pem".into(),
                key: "C:/c/key.pem".into(),
            }),
            redirect_https: redirect,
            blocks: SiteBlocks::default(),
            custom_snippet: None,
            public_domain: None,
            forwarded_tls: false,
            path_prefix: None,
        }
    }

    const PORTS: Ports = Ports {
        http: 80,
        https: 443,
    };

    #[test]
    fn one_server_block_serves_every_localhost_path_route() {
        // All these routes share `Host: localhost`, so they cannot be split across files
        // the way domains are: the first `server_name localhost` on the port would answer
        // for all of them. One block, one `location` per site.
        let mut a = site(Backend::Static, false, false);
        a.path_prefix = Some("shop".into());
        let mut b = site(Backend::Static, false, false);
        b.hostname = "api.test".into();
        b.path_prefix = Some("api".into());
        let out = Nginx.render_path_routes(&[a, b], PORTS);
        assert_eq!(out.matches("server {").count(), 1);
        assert!(out.contains("server_name localhost 127.0.0.1;"));
        assert!(out.contains("location /shop/ {"));
        assert!(out.contains("location /api/ {"));
        // The site's own vhost answers, with the prefix stripped by the trailing slash
        // on the proxy target and the site named as the Host.
        assert!(out.contains("proxy_pass http://127.0.0.1:80/;"));
        assert!(out.contains("proxy_set_header Host shop.test;"));
        assert!(out.contains("proxy_set_header Host api.test;"));
        // A bare `/shop` is a folder, not a page.
        assert!(out.contains("location = /shop { return 301 /shop/; }"));
        // Anything else under localhost is not a site and must not land on one.
        assert!(out.contains("location / { return 404; }"));
    }

    #[test]
    fn a_tls_site_is_proxied_to_over_https_so_its_own_redirect_does_not_bite() {
        // A site with TLS and "Redirect to HTTPS" answers its plain-HTTP vhost with
        // `return 301 https://$host$request_uri` and nothing else. Proxied to the HTTP
        // port, `$host` is the site hostname this route just set, so every prefixed
        // request was answered with a redirect to the site domain and the localhost
        // route never rendered a page. The HTTPS vhost is the one that serves it.
        let mut a = site(
            Backend::Php {
                pool: "php_81".into(),
                ports: vec![10811],
            },
            true,
            true,
        );
        a.path_prefix = Some("shop".into());
        let out = Nginx.render_path_routes(&[a], PORTS);
        assert!(out.contains("proxy_pass https://127.0.0.1:443/;"));
        // SNI, or the first `server` block on the port would answer instead.
        assert!(out.contains("proxy_ssl_server_name on;"));
        assert!(out.contains("proxy_ssl_name shop.test;"));
        assert!(out.contains("proxy_ssl_verify off;"));
    }

    #[test]
    fn a_site_without_tls_still_takes_the_plain_http_vhost() {
        let mut a = site(Backend::Static, false, false);
        a.path_prefix = Some("shop".into());
        let out = Nginx.render_path_routes(&[a], PORTS);
        assert!(out.contains("proxy_pass http://127.0.0.1:80/;"));
        assert!(!out.contains("proxy_ssl_server_name"));
    }

    #[test]
    fn the_stripped_prefix_is_advertised_so_urls_are_rebuilt_with_it() {
        let mut a = site(Backend::Static, false, false);
        a.path_prefix = Some("shop".into());
        let out = Nginx.render_path_routes(&[a], PORTS);
        assert!(out.contains("proxy_set_header X-Forwarded-Prefix /shop;"));
    }

    #[test]
    fn a_site_with_no_path_route_gets_no_location() {
        // The seed site and any site that never asked for a path must not appear here.
        let out = Nginx.render_path_routes(&[site(Backend::Static, false, false)], PORTS);
        assert!(!out.contains("location /shop/"));
        assert_eq!(out.matches("server {").count(), 1);
    }

    #[test]
    fn php_site_routes_php_to_its_pool_over_https() {
        let cfg = Nginx.render_site(
            &site(
                Backend::Php {
                    pool: "php_81".into(),
                    ports: vec![10810],
                },
                true,
                true,
            ),
            PORTS,
        );
        assert!(cfg.contains("server_name shop.test;"));
        assert!(
            cfg.contains("return 301 https://$host$request_uri;"),
            "http server must redirect"
        );
        assert!(cfg.contains("listen 127.0.0.1:443 ssl;"));
        assert!(cfg.contains("ssl_certificate \"C:/c/cert.pem\";"));
        assert!(cfg.contains("fastcgi_pass ols_php_81;"));
        assert!(cfg.contains("fastcgi_param HTTPS on;"));
        assert!(
            cfg.contains("root \"C:/sites/shop/public\";"),
            "backslashes must become forward slashes"
        );
    }

    #[test]
    fn redirect_toggle_off_serves_plain_http_directly() {
        let cfg = Nginx.render_site(&site(Backend::Static, true, false), PORTS);
        assert!(
            !cfg.contains("return 301"),
            "redirect disabled must not redirect (§52)"
        );
        assert_eq!(
            cfg.matches("try_files $uri $uri/ =404;").count(),
            2,
            "both http and https serve the site"
        );
    }

    #[test]
    fn redirect_includes_a_non_default_https_port() {
        let cfg = Nginx.render_site(
            &site(Backend::Static, true, true),
            Ports {
                http: 8080,
                https: 8443,
            },
        );
        assert!(cfg.contains("return 301 https://$host:8443$request_uri;"));
        assert!(cfg.contains("listen 127.0.0.1:8080;"));
    }

    #[test]
    fn proxy_site_forwards_with_websocket_upgrade() {
        let cfg = Nginx.render_site(
            &site(
                Backend::Proxy {
                    upstream: "http://127.0.0.1:5173".into(),
                },
                false,
                false,
            ),
            PORTS,
        );
        assert!(cfg.contains("proxy_pass http://127.0.0.1:5173;"));
        assert!(cfg.contains("proxy_set_header Upgrade $http_upgrade;"));
        assert!(!cfg.contains("ssl_certificate"));
    }

    #[test]
    fn wildcard_adds_a_wildcard_server_name() {
        let mut s = site(Backend::Static, false, false);
        s.wildcard = true;
        assert!(Nginx
            .render_site(&s, PORTS)
            .contains("server_name shop.test *.shop.test;"));
    }

    #[test]
    fn structured_blocks_render_headers_redirects_mappings_upstreams() {
        let mut s = site(Backend::Static, false, false);
        s.blocks = SiteBlocks {
            headers: vec![HeaderRule {
                name: "X-Frame-Options".into(),
                value: "DENY".into(),
            }],
            redirects: vec![RedirectRule {
                from: "/old".into(),
                to: "/new".into(),
                code: 301,
            }],
            mappings: vec![ProxyMapping {
                path: "/api/".into(),
                upstream: "http://127.0.0.1:8000".into(),
            }],
            upstreams: vec![UpstreamGroup {
                name: "backend".into(),
                servers: vec!["127.0.0.1:9001".into()],
            }],
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
        let cfg = Nginx.render_main(
            &layout,
            PORTS,
            &[PoolSpec {
                id: "php_81".into(),
                ports: vec![10810, 10811],
            }],
        );
        assert!(cfg.contains("upstream ols_php_81 {"));
        assert!(cfg.contains("server 127.0.0.1:10811;"));
        assert!(cfg.contains("include \"C:/ols/web/nginx/sites/*.conf\";"));
        assert!(cfg.contains("default_server"));
    }
}
