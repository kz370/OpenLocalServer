//! Stage 5 smoke test: installs Mailpit + MariaDB for real (reusing whatever's already
//! verified in cache/ — see §127), starts them as managed services, sends a real SMTP
//! message and confirms Mailpit captured it via its own HTTP API, and creates a real
//! MariaDB database via the mariadb client. All through `Core::dispatch`, same as the GUI.
//! `cargo run --release --example smoke_services -p ols-core`

use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread::sleep;
use std::time::Duration;

use ols_core::{AppPaths, Core, CoreCommand, CoreResponse, SettingsService};

fn dispatch(core: &mut Core, cmd: CoreCommand) -> CoreResponse {
    core.dispatch(cmd)
        .unwrap_or_else(|d| panic!("command failed: {} — {}", d.problem, d.cause))
}

fn wait_installed(core: &mut Core, id: &str) {
    for _ in 0..180 {
        if let CoreResponse::RuntimeCatalog { entries } =
            dispatch(core, CoreCommand::ListRuntimeCatalog)
        {
            if entries.iter().any(|e| e.id == id && e.installed) {
                println!("[smoke] {id} installed");
                return;
            }
        }
        sleep(Duration::from_secs(1));
    }
    panic!("{id} did not finish installing in time");
}

fn wait_port_open(port: u16, label: &str) {
    for _ in 0..60 {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            println!("[smoke] {label} is accepting connections on port {port}");
            return;
        }
        sleep(Duration::from_millis(500));
    }
    panic!("{label} never opened port {port}");
}

/// Speaks just enough raw SMTP to hand Mailpit one message — no crate dependency needed
/// for a protocol this small.
fn send_test_email(port: u16) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to mailpit SMTP");
    let mut buf = [0u8; 4096];

    macro_rules! expect_reply {
        () => {{
            let n = stream.read(&mut buf).unwrap();
            let reply = String::from_utf8_lossy(&buf[..n]);
            print!("[smtp] {reply}");
        }};
    }
    macro_rules! send {
        ($line:expr) => {{
            stream
                .write_all(format!("{}\r\n", $line).as_bytes())
                .unwrap();
        }};
    }

    expect_reply!(); // banner
    send!("EHLO smoke-test");
    expect_reply!();
    send!("MAIL FROM:<smoke@openlocalserver.dev>");
    expect_reply!();
    send!("RCPT TO:<you@example.test>");
    expect_reply!();
    send!("DATA");
    expect_reply!();
    send!("Subject: OpenLocalServer Stage 5 smoke test\r\n\r\nIf you can read this, Mailpit captured it.\r\n.");
    expect_reply!();
    send!("QUIT");
    expect_reply!();
}

fn mailpit_message_count(http_port: u16) -> usize {
    let body = http_get(&format!("http://127.0.0.1:{http_port}/api/v1/messages"));
    let json: serde_json::Value =
        serde_json::from_str(&body).expect("mailpit API returned valid JSON");
    json["messages_count"].as_u64().unwrap_or(0) as usize
}

fn http_get(url: &str) -> String {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        reqwest::get(url)
            .await
            .expect("GET to mailpit API")
            .text()
            .await
            .expect("read mailpit API body")
    })
}

fn main() {
    let paths = AppPaths::resolve();
    paths.ensure_dirs().unwrap();
    println!("[smoke] using real app home: {}", paths.root().display());

    let settings = SettingsService::load(&paths).unwrap();
    let mut core = Core::new(settings, paths.clone());

    for (id, version) in [("mailpit", "1.31.2"), ("mariadb", "11.4.9")] {
        let entries = match dispatch(&mut core, CoreCommand::ListRuntimeCatalog) {
            CoreResponse::RuntimeCatalog { entries } => entries,
            _ => unreachable!(),
        };
        if entries.iter().any(|e| e.id == id && e.installed) {
            println!("[smoke] {id} already installed, skipping download");
        } else {
            println!("[smoke] installing {id} {version} (reusing cache if present)...");
            dispatch(
                &mut core,
                CoreCommand::InstallRuntime {
                    id: id.into(),
                    version: version.into(),
                },
            );
            wait_installed(&mut core, id);
        }
    }

    // -- Mailpit --
    println!("[smoke] starting mailpit...");
    dispatch(
        &mut core,
        CoreCommand::StartService {
            id: "mailpit".into(),
        },
    );
    wait_port_open(8025, "Mailpit web UI");
    wait_port_open(1025, "Mailpit SMTP");

    let before = mailpit_message_count(8025);
    println!("[smoke] mailpit message count before send: {before}");
    send_test_email(1025);
    sleep(Duration::from_millis(500));
    let after = mailpit_message_count(8025);
    println!("[smoke] mailpit message count after send: {after}");
    assert_eq!(
        after,
        before + 1,
        "mailpit did not capture the test message"
    );
    dispatch(
        &mut core,
        CoreCommand::StopService {
            id: "mailpit".into(),
        },
    );
    println!("[smoke] mailpit: SMTP -> capture -> API readback all verified, stopped");

    // -- MariaDB --
    println!("[smoke] starting mariadb (first start runs mariadb-install-db, can take a bit)...");
    dispatch(
        &mut core,
        CoreCommand::StartService {
            id: "mariadb".into(),
        },
    );
    wait_port_open(3306, "MariaDB");
    // mariadbd can accept TCP slightly before it's fully ready to authenticate; give it a beat.
    sleep(Duration::from_secs(3));

    dispatch(
        &mut core,
        CoreCommand::CreateDatabase {
            engine: "mariadb".into(),
            name: "smoke_test_db".into(),
        },
    );
    println!("[smoke] CREATE DATABASE smoke_test_db succeeded");
    dispatch(
        &mut core,
        CoreCommand::StopService {
            id: "mariadb".into(),
        },
    );
    println!("[smoke] mariadb: init -> start -> create database all verified, stopped");

    println!(
        "[smoke] ALL GOOD — Stage 5 service pipeline verified end-to-end against real app data."
    );
}
