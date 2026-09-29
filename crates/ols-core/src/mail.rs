//! Mailpit integration (§63, §66): pointing a project's `.env` at Mailpit after showing the
//! change, a checklist of what has to be true for mail to arrive, and a test message.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::app::Inner;
use crate::detection::Framework;
use crate::envfile;
use crate::error::CoreError;
use crate::service::mailpit_smtp_port;

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::ServiceError(msg.into())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailChange {
    pub key: String,
    /// What the file has now, if the key is there at all.
    pub current: Option<String>,
    pub new: String,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailEnvPlan {
    pub file: String,
    pub framework: String,
    pub changes: Vec<MailChange>,
    /// Nothing to change: every value already points at Mailpit.
    pub up_to_date: bool,
    /// Set when there is nothing to write to an env file (WordPress).
    pub note: Option<String>,
}

/// The variables that make `framework` send through Mailpit's SMTP port.
pub fn wanted_values(framework: &Framework, smtp_port: u16) -> Vec<(&'static str, String)> {
    let port = smtp_port.to_string();
    match framework {
        Framework::Laravel => vec![
            ("MAIL_MAILER", "smtp".into()),
            ("MAIL_HOST", "127.0.0.1".into()),
            ("MAIL_PORT", port),
            ("MAIL_USERNAME", "null".into()),
            ("MAIL_PASSWORD", "null".into()),
            ("MAIL_ENCRYPTION", "null".into()),
        ],
        Framework::Symfony => vec![("MAILER_DSN", format!("smtp://127.0.0.1:{port}"))],
        Framework::Django => vec![("EMAIL_HOST", "127.0.0.1".into()), ("EMAIL_PORT", port)],
        Framework::WordPress => Vec::new(),
        _ => vec![("MAIL_HOST", "127.0.0.1".into()), ("MAIL_PORT", port)],
    }
}

fn framework_name(framework: &Framework) -> &'static str {
    match framework {
        Framework::Laravel => "Laravel",
        Framework::Symfony => "Symfony",
        Framework::WordPress => "WordPress",
        Framework::GenericPhp => "PHP",
        Framework::Node => "Node",
        Framework::Django => "Django",
        Framework::Flask => "Flask",
        Framework::FastApi => "FastAPI",
        Framework::GenericPython => "Python",
        Framework::Unknown => "Unknown",
    }
}

/// What would change in `content`, without changing anything.
pub fn plan(framework: &Framework, file: &str, content: &str, smtp_port: u16) -> MailEnvPlan {
    let entries = envfile::parse(content);
    let changes: Vec<MailChange> = wanted_values(framework, smtp_port)
        .into_iter()
        .map(|(key, new)| {
            // The last assignment wins, as it does when the file is loaded.
            let current = entries
                .iter()
                .rev()
                .find(|e| e.key == key)
                .map(|e| e.value.clone());
            let changed = current.as_deref() != Some(new.as_str());
            MailChange {
                key: key.to_string(),
                current,
                new,
                changed,
            }
        })
        .collect();
    let note = matches!(framework, Framework::WordPress).then(|| {
        format!("WordPress has no .env file. In an SMTP plugin, use host 127.0.0.1, port {smtp_port}, no encryption and no login.")
    });
    MailEnvPlan {
        file: file.to_string(),
        framework: framework_name(framework).to_string(),
        up_to_date: changes.iter().all(|c| !c.changed),
        changes,
        note,
    }
}

/// `content` with every change in the plan written, touching only those lines.
pub fn apply(framework: &Framework, content: &str, smtp_port: u16) -> Result<String, String> {
    let mut out = content.to_string();
    for change in plan(framework, "", content, smtp_port)
        .changes
        .into_iter()
        .filter(|c| c.changed)
    {
        out = envfile::set(&out, &change.key, &change.new)?;
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailCheck {
    pub id: String,
    pub label: String,
    pub ok: bool,
    pub detail: String,
    /// What to do when it isn't ok.
    pub fix: Option<String>,
}

fn reachable(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(500),
    )
    .is_ok()
}

// ----------------------------------------------------------------- SMTP test message

/// Reads one SMTP reply (possibly several `250-...` lines) and returns its code.
fn read_reply(reader: &mut impl BufRead) -> Result<(u16, String), String> {
    let mut text = String::new();
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| format!("the mail server stopped answering: {e}"))?;
        if n == 0 {
            return Err("the mail server closed the connection".into());
        }
        text.push_str(line.trim_end());
        text.push('\n');
        let bytes = line.as_bytes();
        // "250 ok" ends the reply; "250-more" continues it.
        if bytes.len() < 4 || bytes[3] != b'-' {
            let code = line
                .get(..3)
                .and_then(|c| c.parse::<u16>().ok())
                .ok_or_else(|| format!("unexpected reply: {}", line.trim()))?;
            return Ok((code, text.trim().to_string()));
        }
    }
}

fn expect(reader: &mut impl BufRead, want: u16, step: &str) -> Result<(), String> {
    let (code, text) = read_reply(reader)?;
    if code == want {
        Ok(())
    } else {
        Err(format!("the mail server refused {step}: {text}"))
    }
}

/// Sends a short test message to an SMTP server on the loopback interface.
pub fn send_test_mail(port: u16, to: &str) -> Result<(), String> {
    if to.is_empty()
        || to.len() > 200
        || to
            .chars()
            .any(|c| c.is_control() || c == '<' || c == '>' || c.is_whitespace())
        || !to.contains('@')
    {
        return Err("enter a valid email address to send the test to".into());
    }
    let stream = TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_secs(2),
    )
    .map_err(|e| format!("could not connect to the mail server on port {port}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);

    let mut send = |text: &str| {
        writer
            .write_all(text.as_bytes())
            .map_err(|e| format!("could not send to the mail server: {e}"))
    };

    expect(&mut reader, 220, "the connection")?;
    send("EHLO localhost\r\n")?;
    expect(&mut reader, 250, "EHLO")?;
    send("MAIL FROM:<test@openlocalserver.test>\r\n")?;
    expect(&mut reader, 250, "the sender")?;
    send(&format!("RCPT TO:<{to}>\r\n"))?;
    expect(&mut reader, 250, "the recipient")?;
    send("DATA\r\n")?;
    expect(&mut reader, 354, "DATA")?;
    send(&format!(
        "From: OLS <test@openlocalserver.test>\r\nTo: <{to}>\r\nSubject: OLS test message\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nIf you can read this in Mailpit, mail from your projects will arrive.\r\n.\r\n"
    ))?;
    expect(&mut reader, 250, "the message")?;
    let _ = send("QUIT\r\n");
    Ok(())
}

// ------------------------------------------------------------------- app-level actions

impl Inner {
    fn project_framework(&self, project_id: &str) -> Result<Framework, CoreError> {
        let detail = self
            .project_detail(project_id)
            .ok_or_else(|| CoreError::InvalidProjectPath(project_id.to_string()))?;
        Ok(detail.detection.framework)
    }

    /// The change that would point `file` at Mailpit, for the user to look at first (§63).
    pub fn mailpit_env_plan(&self, project_id: &str, file: &str) -> Result<MailEnvPlan, CoreError> {
        let framework = self.project_framework(project_id)?;
        let content = self.env_content(project_id, file)?;
        Ok(plan(&framework, file, &content, mailpit_smtp_port()))
    }

    /// Writes that change (one backup of the old file is kept by the env editor).
    pub fn apply_mailpit_env(
        &self,
        project_id: &str,
        file: &str,
    ) -> Result<MailEnvPlan, CoreError> {
        let framework = self.project_framework(project_id)?;
        let content = self.env_content(project_id, file)?;
        let updated = apply(&framework, &content, mailpit_smtp_port()).map_err(err)?;
        if updated != content {
            self.env_write(project_id, file, &updated)?;
        }
        self.mailpit_env_plan(project_id, file)
    }

    /// §66: each thing that has to be true for a message to reach Mailpit.
    pub fn mail_diagnostics(&self, project_id: Option<&str>) -> Vec<MailCheck> {
        let status = self.services.status("mailpit");
        let smtp = mailpit_smtp_port();
        let mut checks = vec![
            MailCheck {
                id: "installed".into(),
                label: "Mailpit is installed".into(),
                ok: status.installed,
                detail: if status.installed {
                    format!("version {}", status.version.clone().unwrap_or_default())
                } else {
                    "not installed".into()
                },
                fix: (!status.installed)
                    .then(|| "Install Mailpit from the Runtimes page.".to_string()),
            },
            MailCheck {
                id: "running".into(),
                label: "Mailpit is running".into(),
                ok: status.running,
                detail: if status.running {
                    "running".into()
                } else {
                    "stopped".into()
                },
                fix: (!status.running).then(|| "Start Mailpit on the Services page.".to_string()),
            },
        ];
        let smtp_ok = reachable(smtp);
        checks.push(MailCheck {
            id: "smtp".into(),
            label: format!("SMTP answers on port {smtp}"),
            ok: smtp_ok,
            detail: if smtp_ok {
                "connected".into()
            } else {
                "nothing is listening".into()
            },
            fix: (!smtp_ok).then(|| {
                format!("Start Mailpit, or stop whatever uses port {smtp} if it is not Mailpit.")
            }),
        });
        let ui_port = status.port.unwrap_or(8025);
        let ui_ok = reachable(ui_port);
        checks.push(MailCheck {
            id: "web".into(),
            label: format!("Web inbox answers on port {ui_port}"),
            ok: ui_ok,
            detail: if ui_ok {
                format!("http://127.0.0.1:{ui_port}")
            } else {
                "nothing is listening".into()
            },
            fix: (!ui_ok).then(|| "Start Mailpit on the Services page.".to_string()),
        });

        if let Some(id) = project_id {
            match self.mailpit_env_plan(id, ".env") {
                Ok(p) if p.note.is_some() => checks.push(MailCheck {
                    id: "project".into(),
                    label: "The project sends through Mailpit".into(),
                    ok: false,
                    detail: p.note.unwrap_or_default(),
                    fix: None,
                }),
                Ok(p) => {
                    let wrong: Vec<String> = p
                        .changes
                        .iter()
                        .filter(|c| c.changed)
                        .map(|c| c.key.clone())
                        .collect();
                    checks.push(MailCheck {
                        id: "project".into(),
                        label: format!("The project's .env points at Mailpit ({})", p.framework),
                        ok: wrong.is_empty(),
                        detail: if wrong.is_empty() {
                            "all mail settings match".into()
                        } else {
                            format!("differs: {}", wrong.join(", "))
                        },
                        fix: (!wrong.is_empty()).then(|| {
                            "Use “Point .env at Mailpit” to update these values.".to_string()
                        }),
                    });
                }
                Err(e) => checks.push(MailCheck {
                    id: "project".into(),
                    label: "The project's .env".into(),
                    ok: false,
                    detail: e.to_string(),
                    fix: None,
                }),
            }
        }
        checks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn laravel_plan_lists_only_what_differs() {
        let env = "APP_NAME=x\nMAIL_MAILER=log\nMAIL_HOST=smtp.example.com\nMAIL_PORT=1025\n";
        let p = plan(&Framework::Laravel, ".env", env, 1025);
        let changed: Vec<&str> = p
            .changes
            .iter()
            .filter(|c| c.changed)
            .map(|c| c.key.as_str())
            .collect();
        assert_eq!(
            changed,
            vec![
                "MAIL_MAILER",
                "MAIL_HOST",
                "MAIL_USERNAME",
                "MAIL_PASSWORD",
                "MAIL_ENCRYPTION"
            ]
        );
        assert!(!p.up_to_date);
        let mailer = p.changes.iter().find(|c| c.key == "MAIL_MAILER").unwrap();
        assert_eq!(mailer.current.as_deref(), Some("log"));
    }

    #[test]
    fn applying_the_plan_makes_it_up_to_date_and_keeps_other_lines() {
        let env = "# mail\nAPP_NAME=x\nMAIL_HOST=smtp.example.com\n";
        let out = apply(&Framework::Laravel, env, 1025).unwrap();
        assert!(out.starts_with("# mail\nAPP_NAME=x\n"));
        assert!(out.contains("MAIL_HOST=127.0.0.1") && out.contains("MAIL_PORT=1025"));
        assert!(plan(&Framework::Laravel, ".env", &out, 1025).up_to_date);
        assert_eq!(
            apply(&Framework::Laravel, &out, 1025).unwrap(),
            out,
            "a second apply changes nothing"
        );
    }

    #[test]
    fn other_frameworks_get_their_own_variables() {
        let sym = plan(&Framework::Symfony, ".env", "", 1025);
        assert_eq!(sym.changes[0].key, "MAILER_DSN");
        assert_eq!(sym.changes[0].new, "smtp://127.0.0.1:1025");
        let wp = plan(&Framework::WordPress, ".env", "", 1025);
        assert!(wp.changes.is_empty() && wp.note.is_some() && wp.up_to_date);
        assert_eq!(
            plan(&Framework::Django, ".env", "", 1025).changes[0].key,
            "EMAIL_HOST"
        );
    }

    /// A one-message SMTP server that records the DATA it receives.
    fn fake_smtp() -> (u16, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut writer = stream.try_clone().unwrap();
            let mut reader = BufReader::new(stream);
            let mut lines = Vec::new();
            writer.write_all(b"220 fake ready\r\n").unwrap();
            let mut in_data = false;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                let line = line.trim_end().to_string();
                if in_data {
                    if line == "." {
                        in_data = false;
                        writer.write_all(b"250 queued\r\n").unwrap();
                    } else {
                        lines.push(line);
                    }
                    continue;
                }
                lines.push(line.clone());
                let reply: &[u8] = match line.split(' ').next().unwrap_or("") {
                    "EHLO" => b"250-fake\r\n250 8BITMIME\r\n",
                    "DATA" => {
                        in_data = true;
                        b"354 go on\r\n"
                    }
                    "QUIT" => {
                        writer.write_all(b"221 bye\r\n").unwrap();
                        break;
                    }
                    _ => b"250 ok\r\n",
                };
                writer.write_all(reply).unwrap();
            }
            lines
        });
        (port, handle)
    }

    #[test]
    fn test_mail_walks_through_a_full_smtp_conversation() {
        let (port, server) = fake_smtp();
        send_test_mail(port, "dev@example.test").unwrap();
        let seen = server.join().unwrap();
        assert!(seen.iter().any(|l| l == "RCPT TO:<dev@example.test>"));
        assert!(seen.iter().any(|l| l == "Subject: OLS test message"));
    }

    #[test]
    fn test_mail_rejects_bad_addresses_and_reports_a_closed_port() {
        assert!(send_test_mail(1025, "not an address").is_err());
        assert!(send_test_mail(1025, "a@b>\r\nRCPT TO:<x@y").is_err());
        let closed = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let e = send_test_mail(closed, "dev@example.test").unwrap_err();
        assert!(e.contains("could not connect"), "{e}");
    }
}
