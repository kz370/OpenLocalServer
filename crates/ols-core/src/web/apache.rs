//! Apache HTTP Server (§22, Stage 10): VirtualHosts + module loading behind [`WebServer`].
//! Run in the foreground (`httpd -f conf`), not as a Windows service — that keeps it a
//! plain supervised process like everything else, and needs no admin rights.

use std::path::PathBuf;

use super::{
    cfg_path, https_redirect_port_suffix, Backend, Invocation, PoolSpec, Ports, ServerLayout,
    SiteSpec, WebServer, MANAGED_HEADER,
};

pub struct Apache;

/// Modules every generated site relies on. Anything missing from the install is skipped
/// at render time by the caller checking `modules/`, so a slimmer build still starts.
const MODULES: &[&str] = &[
    "authz_core",
    "authz_host",
    "authn_core",
    "access_compat",
    "alias",
    "dir",
    "env",
    "headers",
    "log_config",
    "mime",
    "rewrite",
    "setenvif",
    "proxy",
    "proxy_fcgi",
    "proxy_http",
    "proxy_wstunnel",
    "proxy_balancer",
    "lbmethod_byrequests",
    "slotmem_shm",
    "socache_shmcb",
    "ssl",
];

impl Apache {
    fn invocation(layout: &ServerLayout, extra: &[&str]) -> Invocation {
        let mut args = vec![
            "-f".to_string(),
            cfg_path(&layout.prefix.join("conf").join("httpd.conf")),
        ];
        args.extend(extra.iter().map(|s| s.to_string()));
        Invocation {
            args,
            cwd: layout.install_dir.clone(),
        }
    }
}

impl WebServer for Apache {
    fn id(&self) -> &'static str {
        "apache"
    }
    fn name(&self) -> &'static str {
        "Apache"
    }
    fn config_ext(&self) -> &'static str {
        "conf"
    }
    fn main_config(&self, layout: &ServerLayout) -> PathBuf {
        layout.prefix.join("conf").join("httpd.conf")
    }

    fn render_main(&self, layout: &ServerLayout, ports: Ports, pools: &[PoolSpec]) -> String {
        let install = cfg_path(&layout.install_dir);
        let mut out = String::new();
        out.push_str(MANAGED_HEADER);
        out.push_str(&format!("ServerRoot \"{install}\"\n"));
        out.push_str("ServerName localhost\n");
        out.push_str("ServerTokens Prod\n");
        out.push_str(&format!(
            "PidFile \"{}\"\n",
            cfg_path(&layout.logs_dir.join("httpd.pid"))
        ));
        out.push_str(&format!("Listen 127.0.0.1:{}\n", ports.http));
        out.push_str(&format!("Listen 127.0.0.1:{}\n\n", ports.https));

        for module in MODULES {
            let present = layout
                .install_dir
                .join("modules")
                .join(format!("mod_{module}.so"))
                .is_file();
            // `is_file` is false for a not-yet-created install in unit tests; render anyway
            // when the modules directory doesn't exist so tests can assert on the output.
            if present || !layout.install_dir.join("modules").is_dir() {
                out.push_str(&format!(
                    "LoadModule {module}_module modules/mod_{module}.so\n"
                ));
            }
        }
        out.push('\n');
        out.push_str(&format!(
            "ErrorLog \"{}\"\n",
            cfg_path(&layout.logs_dir.join("error.log"))
        ));
        out.push_str("LogLevel warn\n");
        out.push_str("<IfModule log_config_module>\n");
        out.push_str("    LogFormat \"%h %l %u %t \\\"%r\\\" %>s %b\" common\n");
        out.push_str(&format!(
            "    CustomLog \"{}\" common\n",
            cfg_path(&layout.logs_dir.join("access.log"))
        ));
        out.push_str("</IfModule>\n");
        out.push_str(&format!("TypesConfig \"{install}/conf/mime.types\"\n"));
        out.push_str("DirectoryIndex index.php index.html\n");
        out.push_str("UseCanonicalName Off\n");
        out.push_str("EnableSendfile Off\n");
        out.push_str("<IfModule ssl_module>\n    SSLSessionCache \"shmcb:logs/ssl_scache(512000)\"\n</IfModule>\n\n");

        // Deny by default, then each site opens its own docroot.
        out.push_str(
            "<Directory />\n    AllowOverride None\n    Require all denied\n</Directory>\n\n",
        );

        // PHP FastCGI pools: one balancer per PHP version so every site uses all
        // workers of its version. Windows php-cgi serves one request at a time
        // per process, so balancing across N workers gives N-way concurrency.
        for pool in pools {
            out.push_str(&format!("<Proxy balancer://ols_{}>\n", pool.id));
            for port in &pool.ports {
                out.push_str(&format!("    BalancerMember fcgi://127.0.0.1:{port}\n"));
            }
            out.push_str("</Proxy>\n\n");
        }

        // A request for an unknown name must not silently land on the first site.
        out.push_str(&format!(
            "<VirtualHost 127.0.0.1:{}>\n    ServerName _default_.localhost\n    <Location />\n        Require all denied\n    </Location>\n</VirtualHost>\n\n",
            ports.http
        ));
        out.push_str(&format!(
            "IncludeOptional \"{}/*.conf\"\n",
            cfg_path(&layout.sites_dir)
        ));
        out
    }

    fn render_site(&self, site: &SiteSpec, ports: Ports) -> String {
        let mut out = String::new();
        out.push_str(MANAGED_HEADER);
        out.push_str(&format!("# site: {}\n\n", site.hostname));

        let redirect = site.tls.is_some() && site.redirect_https;
        out.push_str(&format!("<VirtualHost 127.0.0.1:{}>\n", ports.http));
        out.push_str(&head(site));
        if redirect {
            out.push_str("    RewriteEngine On\n");
            out.push_str(&format!(
                "    RewriteRule ^ https://%{{SERVER_NAME}}{}%{{REQUEST_URI}} [R=301,L]\n",
                https_redirect_port_suffix(ports)
            ));
        } else {
            out.push_str(&body(site));
        }
        out.push_str("</VirtualHost>\n");

        if let Some(tls) = &site.tls {
            out.push_str(&format!("\n<VirtualHost 127.0.0.1:{}>\n", ports.https));
            out.push_str(&head(site));
            out.push_str("    SSLEngine on\n");
            out.push_str(&format!(
                "    SSLCertificateFile \"{}\"\n",
                cfg_path(&tls.cert)
            ));
            out.push_str(&format!(
                "    SSLCertificateKeyFile \"{}\"\n",
                cfg_path(&tls.key)
            ));
            out.push_str(&body(site));
            out.push_str("</VirtualHost>\n");
        }
        if let Some(host) = site.public_domain.as_deref() {
            let mut public = site.clone();
            public.hostname = host.to_string();
            public.wildcard = false;
            public.tls = None;
            public.redirect_https = false;
            public.public_domain = None;
            public.forwarded_tls = true;
            out.push_str(&format!("\n<VirtualHost 127.0.0.1:{}>\n", ports.http));
            out.push_str(&head(&public));
            out.push_str(&body(&public));
            out.push_str("    RequestHeader set X-Forwarded-Proto \"https\"\n</VirtualHost>\n");
        }
        out
    }

    /// One vhost for every `localhost/<prefix>` route on this server (§55). Each route
    /// proxies to the site's own vhost on the same server with the prefix stripped, so
    /// the prefixed URL is answered by exactly the config that site already has.
    fn render_path_routes(&self, sites: &[SiteSpec], ports: Ports) -> String {
        let mut out = String::new();
        out.push_str(MANAGED_HEADER);
        out.push_str("# localhost/<prefix> routes. Each is served by that site's own vhost.\n\n");
        out.push_str(&format!("<VirtualHost 127.0.0.1:{}>\n", ports.http));
        out.push_str("    ServerName localhost\n");
        // Without this a bare `/shop` 404s instead of reaching the route below.
        out.push_str("    Alias / \"/\"\n");

        for site in sites
            .iter()
            .filter_map(|s| s.path_prefix.as_deref().map(|p| (s, p)))
        {
            let (site, prefix) = site;
            // A site with TLS answers its plain-HTTP vhost with a redirect to HTTPS, so
            // such a site is reached over the HTTPS vhost instead: proxying to the HTTP
            // port sent every prefixed request to `https://<site domain>` and the
            // localhost route was unreachable for exactly those sites.
            let tls = site.tls.is_some();
            let (scheme, port) = if tls {
                ("https", ports.https)
            } else {
                ("http", ports.http)
            };
            out.push_str(&format!("\n    # {}\n", site.hostname));
            out.push_str(&format!("    RedirectMatch 301 ^/{prefix}/?$ /{prefix}/\n"));
            out.push_str(&format!("    <Location /{prefix}/>\n"));
            // The trailing slash on the target is what strips the prefix: Apache replaces
            // the matched path with it, so `/shop/index.php` arrives as `/index.php`.
            out.push_str(&format!(
                "        ProxyPass /{prefix}/ {scheme}://127.0.0.1:{port}/\n"
            ));
            out.push_str(&format!(
                "        ProxyPassReverse /{prefix}/ {scheme}://127.0.0.1:{port}/\n"
            ));
            if tls {
                out.push_str("        SSLProxyEngine on\n");
                out.push_str("        SSLProxyVerifyPeer off\n");
                out.push_str("        SSLProxyCheckPeerName off\n");
            }
            out.push_str(&format!(
                "        RequestHeader set Host \"{}\"\n",
                site.hostname
            ));
            // The prefix is stripped on the way in; without this an application rebuilds
            // every absolute URL without `/<prefix>` and the browser leaves the route.
            out.push_str(&format!(
                "        RequestHeader set X-Forwarded-Prefix \"/{prefix}\"\n"
            ));
            out.push_str("    </Location>\n");
        }
        out.push_str("\n    <Location />\n        Require all denied\n    </Location>\n");
        out.push_str("</VirtualHost>\n");
        out
    }

    fn prepare(&self, layout: &ServerLayout) -> std::io::Result<()> {
        std::fs::create_dir_all(layout.prefix.join("conf"))?;
        std::fs::create_dir_all(&layout.logs_dir)?;
        std::fs::create_dir_all(&layout.sites_dir)?;
        std::fs::create_dir_all(&layout.custom_dir)?;
        Ok(())
    }

    fn validate(&self, layout: &ServerLayout) -> Invocation {
        Self::invocation(layout, &["-t"])
    }
    fn start(&self, layout: &ServerLayout) -> Invocation {
        Self::invocation(layout, &[])
    }
    /// The Windows foreground mode has no graceful-reload signal, so the manager restarts it.
    fn reload(&self, _layout: &ServerLayout) -> Option<Invocation> {
        None
    }
    fn stop(&self, _layout: &ServerLayout) -> Option<Invocation> {
        None
    }
    fn error_log(&self, layout: &ServerLayout) -> PathBuf {
        layout.logs_dir.join("error.log")
    }
}

fn head(site: &SiteSpec) -> String {
    let mut out = String::new();
    out.push_str(&format!("    ServerName {}\n", site.hostname));
    if site.wildcard {
        out.push_str(&format!("    ServerAlias *.{}\n", site.hostname));
    }
    out
}

fn body(site: &SiteSpec) -> String {
    let root = site.root.replace('\\', "/");
    let mut out = String::new();
    out.push_str(&format!("    DocumentRoot \"{root}\"\n"));
    out.push_str(&format!(
        "    <Directory \"{root}\">\n        Options Indexes FollowSymLinks\n        AllowOverride All\n        Require all granted\n    </Directory>\n"
    ));
    let https = site.tls.is_some();
    // Dev sites always revalidate (see nginx), unless the app chose its own caching.
    out.push_str("    <IfModule headers_module>\n        Header setifempty Cache-Control \"no-cache\"\n    </IfModule>\n");

    for header in &site.blocks.headers {
        out.push_str(&format!(
            "    Header always set {} \"{}\"\n",
            header.name, header.value
        ));
    }
    for include in &site.blocks.includes {
        out.push_str(&format!("    Include \"{}\"\n", include.replace('\\', "/")));
    }
    if let Some(snippet) = &site.custom_snippet {
        out.push_str(&format!("    Include \"{}\"\n", cfg_path(snippet)));
    }
    for redirect in &site.blocks.redirects {
        out.push_str(&format!(
            "    Redirect {} \"{}\" \"{}\"\n",
            redirect.code, redirect.from, redirect.to
        ));
    }
    for mapping in &site.blocks.mappings {
        out.push_str(&format!(
            "    ProxyPass \"{}\" \"{}\"\n",
            mapping.path, mapping.upstream
        ));
        out.push_str(&format!(
            "    ProxyPassReverse \"{}\" \"{}\"\n",
            mapping.path, mapping.upstream
        ));
    }

    match &site.backend {
        Backend::Php { pool, .. } => {
            if https {
                out.push_str("    SetEnv HTTPS on\n");
            }
            // Balanced across all php-cgi workers of this version via the
            // <Proxy balancer://...> block in the main config.
            out.push_str(&format!(
                "    <FilesMatch \"\\.php$\">\n        SetHandler \"proxy:balancer://ols_{pool}/\"\n    </FilesMatch>\n"
            ));
            // On Windows mod_proxy_fcgi builds SCRIPT_FILENAME as "proxy:fcgi://host:port/C:/.."
            // (or "proxy:balancer://.../C:/.." when balanced), which php-cgi rejects
            // ("No input file specified"); strip the prefix back off.
            out.push_str(
                "    ProxyFCGISetEnvIf \"reqenv('SCRIPT_FILENAME') =~ m#^proxy:(?:fcgi|balancer)://[^/]+/(.*)$#\" SCRIPT_FILENAME \"$1\"\n",
            );
            // In VirtualHost context REQUEST_FILENAME may still be the URL path, before
            // Apache maps it to disk. FallbackResource preserves existing assets and sends
            // only missing paths to the PHP front controller.
            out.push_str("    FallbackResource /index.php\n");
        }
        Backend::Proxy { upstream } => {
            if upstream.starts_with("https://") {
                out.push_str("    SSLProxyEngine On\n    SSLProxyVerify none\n    SSLProxyCheckPeerName off\n");
            }
            out.push_str("    ProxyPreserveHost On\n");
            out.push_str("    RequestHeader set X-Forwarded-Proto \"");
            out.push_str(if https { "https" } else { "http" });
            out.push_str("\"\n");
            out.push_str("    RewriteEngine On\n");
            out.push_str("    RewriteCond %{HTTP:Upgrade} =websocket [NC]\n");
            let ws = upstream.replacen("http", "ws", 1);
            out.push_str(&format!("    RewriteRule ^/?(.*) {ws}/$1 [P,L]\n"));
            out.push_str(&format!("    ProxyPass \"/\" \"{upstream}/\"\n"));
            out.push_str(&format!("    ProxyPassReverse \"/\" \"{upstream}/\"\n"));
        }
        Backend::Static => {}
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
            wildcard: true,
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
            path_prefix: None,
        }
    }

    const PORTS: Ports = Ports {
        http: 80,
        https: 443,
    };

    #[test]
    fn https_php_site_has_two_vhosts_and_a_fcgi_handler() {
        let cfg = Apache.render_site(
            &site(
                Backend::Php {
                    pool: "php_84".into(),
                    ports: vec![10840],
                },
                true,
                true,
            ),
            PORTS,
        );
        assert!(cfg.contains("<VirtualHost 127.0.0.1:80>"));
        assert!(cfg.contains("<VirtualHost 127.0.0.1:443>"));
        assert!(cfg.contains("ServerAlias *.shop.test"));
        assert!(cfg.contains("RewriteRule ^ https://%{SERVER_NAME}%{REQUEST_URI} [R=301,L]"));
        assert!(cfg.contains("SSLCertificateFile \"C:/c/cert.pem\""));
        assert!(cfg.contains("SetHandler \"proxy:balancer://ols_php_84/\""));
        assert!(cfg.contains("DocumentRoot \"C:/sites/shop\""));
    }

    #[test]
    fn a_tls_site_is_proxied_to_over_https_so_its_own_redirect_does_not_bite() {
        // The site answers its `127.0.0.1:80` vhost with `RewriteRule ^ https://...` and
        // nothing else, so proxying the localhost route to the HTTP port sent every
        // prefixed request to the site domain and the route never rendered a page.
        let mut a = site(
            Backend::Php {
                pool: "php_84".into(),
                ports: vec![10840],
            },
            true,
            true,
        );
        a.path_prefix = Some("shop".into());
        let cfg = Apache.render_path_routes(&[a], PORTS);
        assert!(cfg.contains("ProxyPass /shop/ https://127.0.0.1:443/"));
        assert!(cfg.contains("ProxyPassReverse /shop/ https://127.0.0.1:443/"));
        assert!(cfg.contains("SSLProxyEngine on"));
        assert!(cfg.contains("RequestHeader set X-Forwarded-Prefix \"/shop\""));
    }

    #[test]
    fn a_site_without_tls_still_takes_the_plain_http_vhost() {
        let mut a = site(Backend::Static, false, false);
        a.path_prefix = Some("shop".into());
        let cfg = Apache.render_path_routes(&[a], PORTS);
        assert!(cfg.contains("ProxyPass /shop/ http://127.0.0.1:80/"));
        assert!(!cfg.contains("SSLProxyEngine"));
    }

    #[test]
    fn proxy_site_forwards_http_and_websockets() {
        let cfg = Apache.render_site(
            &site(
                Backend::Proxy {
                    upstream: "http://127.0.0.1:3000".into(),
                },
                false,
                false,
            ),
            PORTS,
        );
        assert!(cfg.contains("ProxyPass \"/\" \"http://127.0.0.1:3000/\""));
        assert!(cfg.contains("ws://127.0.0.1:3000/"));
        assert!(!cfg.contains("SSLEngine"));
    }

    #[test]
    fn main_config_loads_modules_and_includes_sites() {
        let layout = ServerLayout {
            install_dir: "C:/apache".into(),
            prefix: "C:/ols/web/apache".into(),
            sites_dir: "C:/ols/web/apache/sites".into(),
            custom_dir: "C:/ols/web/apache/custom".into(),
            logs_dir: "C:/ols/web/apache/logs".into(),
            binary: "C:/apache/bin/httpd.exe".into(),
        };
        let cfg = Apache.render_main(
            &layout,
            PORTS,
            &[PoolSpec {
                id: "php_84".into(),
                ports: vec![10840, 10841],
            }],
        );
        assert!(cfg.contains("LoadModule ssl_module"));
        assert!(cfg.contains("IncludeOptional \"C:/ols/web/apache/sites/*.conf\""));
        assert!(cfg.contains("<Proxy balancer://ols_php_84>"));
        assert!(cfg.contains("BalancerMember fcgi://127.0.0.1:10840"));
        assert!(cfg.contains("BalancerMember fcgi://127.0.0.1:10841"));
    }
}
