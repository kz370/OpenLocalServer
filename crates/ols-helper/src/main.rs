//! `ols-helper` — the only part of OLS that ever runs elevated (§138, §142).
//!
//! It accepts a small, *closed* set of commands and validates every argument before
//! touching anything, so a compromised or buggy caller can't turn it into a general
//! "run this as admin" tool:
//!
//!   hosts-apply <ip>=<host> [...]   replace the OLS block in the hosts file
//!   hosts-remove                    delete that block
//!   nrpt-add <.suffix> 127.0.0.1    make Windows resolve `*.suffix` through our local DNS
//!   nrpt-remove <.suffix>           undo that
//!
//! Resident mode (see `service.rs`): `install-service` / `uninstall-service` (run elevated,
//! once) and `service` (started by Windows) serve the same commands over a local pipe, so
//! the app stops needing an administrator prompt per change.
//!
//! Exit codes: 0 ok · 1 failed · 2 rejected arguments · 5 access denied (caller should
//! retry elevated).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ols_core::hosts::{apply_hosts_block, remove_hosts_block};

#[cfg(windows)]
mod service;

#[derive(Debug, PartialEq, Eq)]
pub enum HelperError {
    Rejected(String),
    AccessDenied(String),
    Failed(String),
}

impl HelperError {
    fn code(&self) -> u8 {
        match self {
            HelperError::Failed(_) => 1,
            HelperError::Rejected(_) => 2,
            HelperError::AccessDenied(_) => 5,
        }
    }
    fn message(&self) -> &str {
        match self {
            HelperError::Rejected(m) | HelperError::AccessDenied(m) | HelperError::Failed(m) => m,
        }
    }
}

fn hosts_file_path() -> PathBuf {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    PathBuf::from(root)
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts")
}

fn valid_host(host: &str) -> bool {
    ols_core::domain::validate_hostname(host).is_ok()
}

/// A hosts line may only ever point a name at loopback.
fn valid_entry(entry: &str) -> Option<(String, String)> {
    let (ip, host) = entry.split_once('=')?;
    if ip != "127.0.0.1" && ip != "::1" {
        return None;
    }
    valid_host(host).then(|| (ip.to_string(), host.to_string()))
}

/// `.test` style DNS suffix: a leading dot then a valid hostname.
fn valid_suffix(suffix: &str) -> bool {
    suffix.strip_prefix('.').is_some_and(|rest| {
        !rest.is_empty() && {
            // A single label like "test" is a legal namespace even though it isn't a hostname
            // by itself, so validate it as a label under a dummy parent.
            valid_host(&format!("{rest}.x")) && valid_host(&format!("x.{rest}"))
        }
    })
}

fn map_io(e: std::io::Error) -> HelperError {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        HelperError::AccessDenied(e.to_string())
    } else {
        HelperError::Failed(e.to_string())
    }
}

pub fn execute(args: &[String], hosts_file: &Path) -> Result<(), HelperError> {
    let Some((command, rest)) = args.split_first() else {
        return Err(HelperError::Rejected("no command given".into()));
    };
    match command.as_str() {
        "hosts-apply" => {
            let mut entries = Vec::new();
            for raw in rest {
                entries.push(valid_entry(raw).ok_or_else(|| HelperError::Rejected(format!("bad hosts entry: {raw}")))?);
            }
            let existing = std::fs::read_to_string(hosts_file).map_err(map_io)?;
            std::fs::write(hosts_file, apply_hosts_block(&existing, &entries)).map_err(map_io)
        }
        "hosts-remove" => {
            if !rest.is_empty() {
                return Err(HelperError::Rejected("hosts-remove takes no arguments".into()));
            }
            let existing = std::fs::read_to_string(hosts_file).map_err(map_io)?;
            std::fs::write(hosts_file, remove_hosts_block(&existing)).map_err(map_io)
        }
        "nrpt-add" => match rest {
            [suffix, ns] if valid_suffix(suffix) && ns == "127.0.0.1" => powershell(&format!(
                "Get-DnsClientNrptRule | Where-Object {{ $_.Comment -eq 'OpenLocalServer' -and $_.Namespace -contains '{suffix}' }} | Remove-DnsClientNrptRule -Force; \
                 Add-DnsClientNrptRule -Namespace '{suffix}' -NameServers '{ns}' -Comment 'OpenLocalServer'"
            )),
            _ => Err(HelperError::Rejected("usage: nrpt-add .suffix 127.0.0.1".into())),
        },
        "nrpt-remove" => match rest {
            [suffix] if valid_suffix(suffix) => powershell(&format!(
                "Get-DnsClientNrptRule | Where-Object {{ $_.Comment -eq 'OpenLocalServer' -and $_.Namespace -contains '{suffix}' }} | Remove-DnsClientNrptRule -Force"
            )),
            _ => Err(HelperError::Rejected("usage: nrpt-remove .suffix".into())),
        },
        other => Err(HelperError::Rejected(format!("unknown command: {other}"))),
    }
}

fn powershell(script: &str) -> Result<(), HelperError> {
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let output = cmd
        .output()
        .map_err(|e| HelperError::Failed(e.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(HelperError::Failed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    {
        let resident = match args.first().map(String::as_str) {
            Some("service") => Some(service::run()),
            Some("install-service") => Some(service::install()),
            Some("uninstall-service") => Some(service::uninstall()),
            _ => None,
        };
        if let Some(result) = resident {
            return match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{e}");
                    // Installing needs elevation; report it the same way as a hosts write.
                    ExitCode::from(
                        if e.contains("Access is denied") || e.contains("os error 5") {
                            5
                        } else {
                            1
                        },
                    )
                }
            };
        }
    }
    match execute(&args, &hosts_file_path()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", e.message());
            ExitCode::from(e.code())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn hosts_in_temp(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn hosts_apply_writes_only_the_managed_block() {
        let (_dir, path) = hosts_in_temp("127.0.0.1 localhost\n");
        execute(
            &args(&[
                "hosts-apply",
                "127.0.0.1=shop.test",
                "127.0.0.1=api.shop.test",
            ]),
            &path,
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("127.0.0.1 localhost"));
        assert!(text.contains("127.0.0.1 shop.test"));
        assert!(text.contains("127.0.0.1 api.shop.test"));

        execute(&args(&["hosts-remove"]), &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("127.0.0.1 localhost"));
        assert!(!text.contains("shop.test"));
    }

    #[test]
    fn rejects_entries_that_could_hijack_real_sites() {
        let (_dir, path) = hosts_in_temp("");
        // Pointing a name at anything but loopback would let a caller redirect real traffic.
        assert!(matches!(
            execute(&args(&["hosts-apply", "6.6.6.6=bank.test"]), &path),
            Err(HelperError::Rejected(_))
        ));
        assert!(matches!(
            execute(
                &args(&["hosts-apply", "127.0.0.1=evil.test\n1.2.3.4 x"]),
                &path
            ),
            Err(HelperError::Rejected(_))
        ));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "",
            "a rejected command must not write"
        );
    }

    #[test]
    fn rejects_unknown_commands_and_bad_nrpt_arguments() {
        let (_dir, path) = hosts_in_temp("");
        assert!(matches!(
            execute(&args(&["format-c"]), &path),
            Err(HelperError::Rejected(_))
        ));
        assert!(matches!(
            execute(&args(&["nrpt-add", ".test", "8.8.8.8"]), &path),
            Err(HelperError::Rejected(_))
        ));
        assert!(matches!(
            execute(&args(&["nrpt-add", "test'; calc; '", "127.0.0.1"]), &path),
            Err(HelperError::Rejected(_))
        ));
        assert!(matches!(execute(&[], &path), Err(HelperError::Rejected(_))));
    }

    #[test]
    fn suffix_validation() {
        assert!(valid_suffix(".test"));
        assert!(valid_suffix(".shop.test"));
        assert!(!valid_suffix("test"));
        assert!(!valid_suffix("."));
        assert!(!valid_suffix(".te st"));
        assert!(!valid_suffix(".test'"));
    }
}
