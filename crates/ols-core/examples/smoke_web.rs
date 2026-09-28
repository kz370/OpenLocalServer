//! Stage 6/9/10 smoke test: the real thing, end to end, through `Core::dispatch`.
//! Installs Nginx, PHP 8.1 + 8.4 and Node, serves three HTTPS sites (PHP 8.1, PHP 8.4, a
//! Node dev server behind a proxy), checks each over TLS against only our CA, exercises
//! HTTP→HTTPS, wildcard DNS + wildcard certificate, config ownership/drift/rollback, and
//! then repeats the PHP checks on Apache and Caddy.
//!
//! Run with a scratch home so nothing touches the real machine:
//!   OLS_HOME=<dir> OLS_HOSTS_FILE=<dir>/hosts cargo run --release --example smoke_web -p ols-core

use std::collections::BTreeMap;
use std::net::{SocketAddr, UdpSocket};
use std::thread::sleep;
use std::time::Duration;

use ols_core::domain::{AppSpec, Domain, Ownership, SiteBlocks, SiteKind};
use ols_core::web::manager::{ApplyReport, ConfigPart};
use ols_core::{AppPaths, Core, CoreCommand, CoreResponse, SettingsService};

fn dispatch(core: &Core, cmd: CoreCommand) -> CoreResponse {
    core.dispatch(cmd)
        .unwrap_or_else(|d| panic!("command failed: {} — {}", d.problem, d.cause))
}

/// An apply now returns one report per server; pick the one for `server`.
fn pick_report(reports: &[ApplyReport], server: &str) -> ApplyReport {
    reports
        .iter()
        .find(|r| r.server == server)
        .unwrap_or_else(|| panic!("no apply report for {server}, got {reports:?}"))
        .clone()
}

fn apply_report(core: &Core, server: &str, overwrite: &[String]) -> ApplyReport {
    let CoreResponse::Applied { reports } =
        dispatch(core, CoreCommand::ApplyWeb { overwrite: overwrite.to_vec() })
    else {
        panic!("expected Applied")
    };
    pick_report(&reports, server)
}

fn expect_err(core: &Core, cmd: CoreCommand) -> String {
    match core.dispatch(cmd) {
        Ok(_) => panic!("expected the command to fail"),
        Err(d) => d.cause,
    }
}

fn wait_installed(core: &Core, id: &str, version_prefix: &str) {
    for _ in 0..600 {
        if let CoreResponse::RuntimeCatalog { entries } =
            dispatch(core, CoreCommand::ListRuntimeCatalog)
        {
            if entries
                .iter()
                .any(|e| e.id == id && e.version.starts_with(version_prefix) && e.installed)
            {
                println!("[smoke] {id} {version_prefix} installed");
                return;
            }
        }
        sleep(Duration::from_secs(1));
    }
    panic!("{id} {version_prefix} did not install in time");
}

fn install(core: &Core, id: &str, prefix: &str) {
    let CoreResponse::RuntimeCatalog { entries } = dispatch(core, CoreCommand::ListRuntimeCatalog)
    else {
        panic!()
    };
    let entry = entries
        .iter()
        .find(|e| e.id == id && e.version.starts_with(prefix))
        .unwrap_or_else(|| panic!("{id} {prefix} is not in the catalog"));
    if !entry.installed {
        dispatch(
            core,
            CoreCommand::InstallRuntime {
                id: id.into(),
                version: entry.version.clone(),
            },
        );
    }
    wait_installed(core, id, prefix);
}

fn set(core: &Core, key: &str, value: serde_json::Value) {
    dispatch(
        core,
        CoreCommand::SetSetting {
            key: key.into(),
            value,
        },
    );
}

fn domain(host: &str, root: &std::path::Path, kind: SiteKind) -> Domain {
    Domain {
        hostname: host.into(),
        project_id: None,
        root: root.display().to_string(),
        kind,
        https: true,
        redirect_https: true,
        wildcard: false,
        enabled: true,
        ownership: Ownership::Managed,
        app: None,
        blocks: SiteBlocks::default(),
        generated_hashes: BTreeMap::new(),
        tunnel_id: None,
         server: None,
        public_domain: None,
    }
}

/// GET https://host:port/ trusting ONLY our CA, resolving `host` to loopback.
fn https_get(ca_pem: &str, host: &str, port: u16) -> (u16, String) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let cert = reqwest::Certificate::from_pem(ca_pem.as_bytes()).unwrap();
        let client = reqwest::Client::builder()
            .tls_built_in_root_certs(false)
            .add_root_certificate(cert)
            .resolve(host, SocketAddr::from(([127, 0, 0, 1], port)))
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap();
        let resp = client
            .get(format!("https://{host}:{port}/"))
            .send()
            .await
            .unwrap_or_else(|e| panic!("GET https://{host}:{port}/ failed: {e:?}"));
        (
            resp.status().as_u16(),
            resp.text().await.unwrap_or_default(),
        )
    })
}

fn http_get_no_follow(host: &str, port: u16) -> (u16, Option<String>) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let client = reqwest::Client::builder()
            .resolve(host, SocketAddr::from(([127, 0, 0, 1], port)))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let resp = client
            .get(format!("http://{host}:{port}/"))
            .send()
            .await
            .unwrap();
        (
            resp.status().as_u16(),
            resp.headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string),
        )
    })
}

fn check(label: &str, ok: bool, detail: impl std::fmt::Display) {
    println!(
        "[smoke] {} {label}: {detail}",
        if ok { "PASS" } else { "FAIL" }
    );
    if !ok {
        std::process::exit(1);
    }
}

fn dns_a_record(port: u16, name: &str) -> Option<[u8; 4]> {
    let mut q = vec![0xAB, 0xCD, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    for l in name.split('.') {
        q.push(l.len() as u8);
        q.extend_from_slice(l.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]);
    let s = UdpSocket::bind("127.0.0.1:0").ok()?;
    s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    s.send_to(&q, ("127.0.0.1", port)).ok()?;
    let mut buf = [0u8; 512];
    let (n, _) = s.recv_from(&mut buf).ok()?;
    (u16::from_be_bytes([buf[6], buf[7]]) == 1)
        .then(|| [buf[n - 4], buf[n - 3], buf[n - 2], buf[n - 1]])
}

fn main() {
    let paths = AppPaths::resolve();
    paths.ensure_dirs().unwrap();
    let settings = SettingsService::load(&paths).unwrap();
    let core = Core::new(settings, paths.clone());

    let (http, https, dns) = (18080u16, 18443u16, 15353u16);
    set(&core, "web.http_port", http.into());
    set(&core, "web.https_port", https.into());
    set(&core, "web.dns_port", dns.into());
    set(&core, "web.php_workers", 2.into());
    set(&core, "web.server", "nginx".into());

    install(&core, "nginx", "1.28");
    install(&core, "php", "8.1");
    install(&core, "php", "8.4");
    install(&core, "node", "24");

    let work = paths.root().join("sites");
    let (a_dir, b_dir, c_dir, d_dir) = (
        work.join("a"),
        work.join("b"),
        work.join("c"),
        work.join("d"),
    );
    for d in [&a_dir, &b_dir, &c_dir, &d_dir] {
        std::fs::create_dir_all(d).unwrap();
    }
    for d in [&a_dir, &b_dir, &d_dir] {
        std::fs::write(d.join("index.php"), "<?php echo 'PHP=' . PHP_VERSION . ' HTTPS=' . ($_SERVER['HTTPS'] ?? 'off') . ' HOST=' . $_SERVER['HTTP_HOST'];").unwrap();
    }
    std::fs::write(
        c_dir.join("server.js"),
        "require('http').createServer((q,r)=>{r.end('node-app host='+q.headers.host+' port='+process.env.PORT)}).listen(process.env.PORT,'127.0.0.1')",
    )
    .unwrap();

    // A site saved as "static" whose folder is really PHP must not hand its source to the browser.
    let s_dir = work.join("s");
    std::fs::create_dir_all(&s_dir).unwrap();
    std::fs::write(s_dir.join("index.php"), "<?php echo 'PHP=' . PHP_VERSION;").unwrap();
    dispatch(
        &core,
        CoreCommand::AddDomain {
            domain: domain("s.test", &s_dir, SiteKind::Static),
        },
    );

    // Pinned to Apache later, to prove exclusive binding.
    let e_dir = work.join("e");
    std::fs::create_dir_all(&e_dir).unwrap();
    std::fs::write(e_dir.join("index.html"), "HOST=e.test").unwrap();

    dispatch(
        &core,
        CoreCommand::AddDomain {
            domain: domain(
                "a.test",
                &a_dir,
                SiteKind::Php {
                    version: Some("8.1".into()),
                },
            ),
        },
    );
    dispatch(
        &core,
        CoreCommand::AddDomain {
            domain: domain(
                "b.test",
                &b_dir,
                SiteKind::Php {
                    version: Some("8.4".into()),
                },
            ),
        },
    );
    let mut c = domain(
        "c.test",
        &c_dir,
        SiteKind::Proxy {
            upstream_port: 15173,
            upstream_host: None,
            upstream_https: false,
        },
    );
    c.app = Some(AppSpec {
        executable: "node".into(),
        args: vec!["server.js".into()],
        cwd: c_dir.display().to_string(),
        runtime: Some("node".into()),
    });
    dispatch(&core, CoreCommand::AddDomain { domain: c });

    // ---- Apply on Nginx
    let report = apply_report(&core, "nginx", &[]);
    check(
        "nginx started",
        report.started,
        format!("{:?}", report.warnings),
    );
    check(
        "validator ran",
        report.validator_output.to_lowercase().contains("ok")
            || report.validator_output.contains("successful"),
        &report.validator_output,
    );
    let hosts = std::fs::read_to_string(std::env::var("OLS_HOSTS_FILE").unwrap()).unwrap();
    check(
        "hosts file written",
        hosts.contains("127.0.0.1 a.test") && hosts.contains("127.0.0.1 c.test"),
        "block present",
    );

    let CoreResponse::CaInfo { info } = dispatch(&core, CoreCommand::GetCaInfo) else {
        panic!()
    };
    let ca_pem = std::fs::read_to_string(&info.cert_path).unwrap();

    let (s, body) = https_get(&ca_pem, "a.test", https);
    check(
        "a.test runs PHP 8.1 over HTTPS",
        s == 200 && body.contains("PHP=8.1.") && body.contains("HTTPS=on"),
        &body,
    );
    let (s, body) = https_get(&ca_pem, "s.test", https);
    check(
        "static-kind site with index.php runs through PHP instead of downloading",
        s == 200 && body.contains("PHP=8."),
        &body,
    );
    let (s, body) = https_get(&ca_pem, "b.test", https);
    check(
        "b.test runs PHP 8.4 over HTTPS",
        s == 200 && body.contains("PHP=8.4."),
        &body,
    );
    // The Node dev server needs a moment to boot.
    let mut node_body = String::new();
    for _ in 0..30 {
        let (s, body) = https_get(&ca_pem, "c.test", https);
        if s == 200 {
            node_body = body;
            break;
        }
        sleep(Duration::from_secs(1));
    }
    check(
        "c.test proxies the Node app",
        node_body.contains("node-app host=c.test") && node_body.contains("port=15173"),
        &node_body,
    );

    let (s, loc) = http_get_no_follow("a.test", http);
    check(
        "HTTP redirects to HTTPS",
        s == 301
            && loc
                .as_deref()
                .is_some_and(|l| l.starts_with(&format!("https://a.test:{https}/"))),
        format!("{s} {loc:?}"),
    );

    let CoreResponse::Health { report } = dispatch(
        &core,
        CoreCommand::HealthCheck {
            hostname: "a.test".into(),
        },
    ) else {
        panic!()
    };
    for st in &report.steps {
        println!(
            "[smoke]   health {} ok={} skipped={} {}",
            st.name, st.ok, st.skipped, st.detail
        );
    }
    check(
        "health: TCP+TLS+cert+HTTP pass",
        report
            .steps
            .iter()
            .filter(|s| ["TCP", "TLS", "Certificate", "HTTP"].contains(&s.name.as_str()))
            .all(|s| s.ok),
        "(DNS/Trust depend on this machine)",
    );

    // ---- HTTP→HTTPS toggle (§52)
    let CoreResponse::Domain { domain: mut a } = dispatch(
        &core,
        CoreCommand::GetDomain {
            hostname: "a.test".into(),
        },
    ) else {
        panic!()
    };
    a.redirect_https = false;
    dispatch(&core, CoreCommand::UpdateDomain { domain: *a });
    let report = apply_report(&core, "nginx", &[]);
    println!(
        "[smoke] apply: written={:?} reloaded={} warnings={:?}",
        report.written, report.reloaded, report.warnings
    );
    // Windows nginx swaps workers asynchronously after a reload; allow a moment.
    let mut s = 0;
    for _ in 0..20 {
        s = http_get_no_follow("a.test", http).0;
        if s == 200 {
            break;
        }
        sleep(Duration::from_millis(500));
    }
    check("redirect can be disabled", s == 200, s);

    // ---- Ownership, drift, rollback (§26–28)
    let CoreResponse::Configs { files } = dispatch(&core, CoreCommand::ListWebConfigs) else {
        panic!()
    };
    let a_file = files
        .iter()
        .find(|f| f.hostname.as_deref() == Some("a.test") && f.part == ConfigPart::Site)
        .unwrap()
        .clone();
    let err = expect_err(
        &core,
        CoreCommand::WriteWebConfig {
            hostname: "a.test".into(),
            part: ConfigPart::Site,
            content: "x".into(),
        },
    );
    check(
        "managed config can't be edited directly",
        err.contains("managed"),
        &err,
    );

    let original = std::fs::read_to_string(&a_file.path).unwrap();
    std::fs::write(&a_file.path, format!("{original}\n# hand edit\n")).unwrap();
    let report = apply_report(&core, "nginx", &[]);
    check(
        "hand edit is flagged as drift and preserved",
        report.drifted == vec!["a.test".to_string()]
            && std::fs::read_to_string(&a_file.path)
                .unwrap()
                .contains("# hand edit"),
        format!("{:?}", report.drifted),
    );
    let report = apply_report(&core, "nginx", &["a.test".into()]);
    check(
        "overwrite restores the generated file",
        report.drifted.is_empty()
            && !std::fs::read_to_string(&a_file.path)
                .unwrap()
                .contains("# hand edit"),
        "",
    );

    dispatch(
        &core,
        CoreCommand::SetOwnership {
            hostname: "a.test".into(),
            ownership: Ownership::Manual,
        },
    );
    let good = std::fs::read_to_string(&a_file.path).unwrap();
    let err = expect_err(
        &core,
        CoreCommand::WriteWebConfig {
            hostname: "a.test".into(),
            part: ConfigPart::Site,
            content: "server { this_is_not_a_directive on; }".into(),
        },
    );
    check(
        "invalid config is rejected by nginx -t",
        err.to_lowercase().contains("unknown directive") || err.contains("rejected"),
        &err,
    );
    check(
        "…and the previous file is restored",
        std::fs::read_to_string(&a_file.path).unwrap() == good,
        "rolled back",
    );
    let (s, _) = http_get_no_follow("a.test", http);
    check("…and the server kept serving", s == 200, s);

    let edited = good.replace("PHP", "PHP"); // valid, byte-different config
    let edited = format!("{edited}\n# edited by user\n");
    dispatch(
        &core,
        CoreCommand::WriteWebConfig {
            hostname: "a.test".into(),
            part: ConfigPart::Site,
            content: edited.clone(),
        },
    );
    let CoreResponse::ConfigVersions { versions } = dispatch(
        &core,
        CoreCommand::ListConfigHistory {
            hostname: "a.test".into(),
        },
    ) else {
        panic!()
    };
    check(
        "history recorded prior versions",
        !versions.is_empty(),
        versions.len(),
    );
    let oldest = versions.last().unwrap().id.clone();
    dispatch(
        &core,
        CoreCommand::RestoreConfigHistory {
            hostname: "a.test".into(),
            id: oldest,
        },
    );
    check(
        "restore brings back an older version",
        !std::fs::read_to_string(&a_file.path)
            .unwrap()
            .contains("# edited by user"),
        "",
    );

    // ---- Wildcard domain: DNS answer + wildcard certificate + serving
    dispatch(
        &core,
        CoreCommand::AddDomain {
            domain: Domain {
                wildcard: true,
                ..domain(
                    "d.test",
                    &d_dir,
                    SiteKind::Php {
                        version: Some("8.4".into()),
                    },
                )
            },
        },
    );
    let report = apply_report(&core, "nginx", &[]);
    println!("[smoke] warnings: {:?}", report.warnings);
    check(
        "wildcard DNS answers *.d.test with loopback",
        dns_a_record(dns, "tenant1.d.test") == Some([127, 0, 0, 1]),
        "",
    );
    check(
        "…but not foreign names",
        dns_a_record(dns, "example.com").is_none(),
        "",
    );
    let CoreResponse::Certificates { certs } = dispatch(&core, CoreCommand::ListCertificates)
    else {
        panic!()
    };
    let d_cert = certs.iter().find(|c| c.hostname == "d.test").unwrap();
    check(
        "wildcard certificate covers *.d.test",
        d_cert.sans.contains(&"*.d.test".to_string()),
        format!("{:?}", d_cert.sans),
    );
    let (s, body) = https_get(&ca_pem, "tenant1.d.test", https);
    check(
        "tenant1.d.test is served with the wildcard cert",
        s == 200 && body.contains("HOST=tenant1.d.test"),
        &body,
    );

    // ---- Stage 10: Apache and Caddy behind the same interface
    for (server, prefix) in [("caddy", "2.11"), ("apache", "2.4")] {
        install(&core, server, prefix);
        set(&core, "web.server", server.into());
        match core.dispatch(CoreCommand::ApplyWeb { overwrite: vec![] }) {
            Ok(CoreResponse::Applied { reports }) => {
                let report = pick_report(&reports, server);
                check(
                    &format!("{server} started"),
                    report.started,
                    &report.validator_output,
                );
                let (s, body) = https_get(&ca_pem, "b.test", https);
                check(
                    &format!("{server} serves b.test on PHP 8.4 over HTTPS"),
                    s == 200 && body.contains("PHP=8.4."),
                    &body,
                );
                let (s, body) = https_get(&ca_pem, "a.test", https);
                check(
                    &format!("{server} serves a.test on PHP 8.1 over HTTPS"),
                    s == 200 && body.contains("PHP=8.1."),
                    &body,
                );
                let (s, body) = https_get(&ca_pem, "c.test", https);
                check(
                    &format!("{server} proxies c.test"),
                    s == 200 && body.contains("node-app"),
                    &body,
                );
            }
            Ok(other) => panic!("{server} apply returned {other:?}"),
            Err(d) => check(
                &format!("{server} apply"),
                false,
                format!("{} — {}", d.problem, d.cause),
            ),
        }
    }

    // Back to nginx to prove switching works, then a clean stop.
    set(&core, "web.server", "nginx".into());
    dispatch(&core, CoreCommand::ApplyWeb { overwrite: vec![] });
    let (s, body) = https_get(&ca_pem, "b.test", https);
    check(
        "switching back to nginx works",
        s == 200 && body.contains("PHP=8.4."),
        &body,
    );

    // ---- Both at once: nginx keeps 80/443, apache moves to its own port and serves
    // only the site pinned to it.
    set(&core, "web.servers.apache.http_port", 8080.into());
    set(&core, "web.servers.apache.https_port", 8443.into());
    let CoreResponse::Applied { reports } =
        dispatch(&core, CoreCommand::ApplyWeb { overwrite: vec![] })
    else {
        panic!()
    };
    let apache = pick_report(&reports, "apache");
    let nginx = pick_report(&reports, "nginx");
    check(
        "both servers report their own start",
        apache.started && nginx.started,
        format!("apache={:?} nginx={:?}", apache.warnings, nginx.warnings),
    );

    let mut pinned = domain("e.test", &e_dir, SiteKind::Static);
    pinned.server = Some("apache".into());
    dispatch(&core, CoreCommand::AddDomain { domain: pinned });
    dispatch(
        &core,
        CoreCommand::StartService {
            id: "apache".into(),
        },
    );
    let report = apply_report(&core, "apache", &[]);
    check(
        "the pinned site is rendered on apache only",
        report.written.contains(&"e.test".to_string())
            && !report.written.contains(&"a.test".to_string()),
        format!("{:?}", report.written),
    );
    let (s, body) = https_get(&ca_pem, "e.test", 8443);
    check(
        "apache answers on its own port",
        s == 200 && body.contains("HOST=e.test"),
        &body,
    );
    let (s, _) = https_get(&ca_pem, "a.test", https);
    check(
        "nginx still answers for its own site on 443",
        s == 200,
        "a.test",
    );
    check(
        "stopping apache leaves nginx up",
        {
            dispatch(
                &core,
                CoreCommand::StopService {
                    id: "apache".into(),
                },
            );
            let CoreResponse::WebStatus { status } = dispatch(&core, CoreCommand::GetWebStatus)
            else {
                panic!()
            };
            !status.servers.iter().any(|s| s.id == "apache" && s.running)
                && status.servers.iter().any(|s| s.id == "nginx" && s.running)
        },
        "",
    );

    dispatch(&core, CoreCommand::StopWeb);
    sleep(Duration::from_secs(1));
    check(
        "stop closes the port",
        std::net::TcpStream::connect(("127.0.0.1", https)).is_err(),
        "",
    );
    println!("[smoke] ALL PASSED");
}
