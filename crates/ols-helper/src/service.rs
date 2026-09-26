//! Resident mode (user choice over per-operation UAC prompts): installed once as the
//! `OpenLocalServerHelper` Windows service, running as LocalSystem, it serves the same
//! closed, validated command set as the one-shot helper over a local named pipe. After the
//! one install prompt, the app never needs administrator approval again.
//!
//! Safety rails:
//! - The pipe rejects remote clients and only admits SYSTEM, administrators and
//!   interactively signed-in users.
//! - Every request goes through [`crate::execute`], so the only possible effects are
//!   loopback-only hosts entries in our block and NRPT rules pointing at 127.0.0.1.
//! - The service runs a copy of this binary under Program Files, which ordinary users
//!   can't replace.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use windows_service::service::{
    ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

pub const SERVICE_NAME: &str = "OpenLocalServerHelper";
pub const PIPE_NAME: &str = r"\\.\pipe\OpenLocalServerHelper";

/// Where the installed service binary lives: admin-only, unlike the app's own folder.
fn install_dir() -> PathBuf {
    let root = std::env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    root.join("OpenLocalServer")
}

// ------------------------------------------------------------------ install / remove

/// `ols-helper install-service` (run elevated): copies this binary to Program Files and
/// (re)creates the service, set to start automatically.
pub fn install() -> Result<(), String> {
    let dir = install_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let target = dir.join("ols-helper.exe");
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .map_err(|e| format!("could not open the service manager: {e}"))?;

    // Upgrading: stop and delete the old one so its binary can be replaced.
    if let Ok(old) = manager.open_service(
        SERVICE_NAME,
        ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS,
    ) {
        let _ = old.stop();
        for _ in 0..50 {
            if old
                .query_status()
                .map(|s| s.current_state == ServiceState::Stopped)
                .unwrap_or(true)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = old.delete();
        drop(old);
        std::thread::sleep(Duration::from_millis(500));
    }

    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    if me != target {
        std::fs::copy(&me, &target)
            .map_err(|e| format!("could not copy the helper to {}: {e}", target.display()))?;
    }

    let info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from("OpenLocalServer Helper"),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: target,
        launch_arguments: vec![OsString::from("service")],
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };
    let service = manager
        .create_service(&info, ServiceAccess::CHANGE_CONFIG | ServiceAccess::START)
        .map_err(|e| format!("could not create the service: {e}"))?;
    let _ = service.set_description("Lets OpenLocalServer add local domains to the hosts file without asking for administrator rights each time.");
    service
        .start::<&str>(&[])
        .map_err(|e| format!("could not start the service: {e}"))?;
    Ok(())
}

/// `ols-helper uninstall-service` (run elevated).
pub fn uninstall() -> Result<(), String> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(|e| e.to_string())?;
    let service = manager
        .open_service(
            SERVICE_NAME,
            ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS,
        )
        .map_err(|e| format!("the helper service is not installed: {e}"))?;
    let _ = service.stop();
    service.delete().map_err(|e| e.to_string())?;
    drop(service);
    std::thread::sleep(Duration::from_millis(500));
    let _ = std::fs::remove_file(install_dir().join("ols-helper.exe"));
    Ok(())
}

// ------------------------------------------------------------------ service runtime

define_windows_service!(ffi_service_main, service_main);

/// `ols-helper service`: only the Service Control Manager starts it this way.
pub fn run() -> Result<(), String> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main).map_err(|e| e.to_string())
}

fn service_main(_args: Vec<OsString>) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_flag = stop.clone();
    let handler = move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            stop_flag.store(true, Ordering::SeqCst);
            // Unblock the pipe server waiting for a client.
            let _ = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(PIPE_NAME);
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    };
    let Ok(status) = service_control_handler::register(SERVICE_NAME, handler) else {
        return;
    };
    let set = |state, accept| {
        let _ = status.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: accept,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        });
    };
    set(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
    );
    pipe::serve(PIPE_NAME, &stop, handle_request);
    set(ServiceState::Stopped, ServiceControlAccept::empty());
}

/// One request: a JSON array of helper arguments. Reply: `{"code":0,"message":""}`.
fn handle_request(raw: &[u8]) -> Vec<u8> {
    let (code, message) = match serde_json::from_slice::<Vec<String>>(raw) {
        Ok(args) if args.first().map(String::as_str) == Some("version") => {
            (0, env!("CARGO_PKG_VERSION").to_string())
        }
        Ok(args) => match crate::execute(&args, &crate::hosts_file_path()) {
            Ok(()) => (0, String::new()),
            Err(e) => (e.code(), e.message().to_string()),
        },
        Err(e) => (2, format!("bad request: {e}")),
    };
    let mut out = serde_json::json!({ "code": code, "message": message })
        .to_string()
        .into_bytes();
    out.push(b'\n');
    out
}

mod pipe {
    use super::*;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, LocalFree, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows_sys::Win32::Storage::FileSystem::{
        FlushFileBuffers, ReadFile, WriteFile, PIPE_ACCESS_DUPLEX,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    /// SYSTEM and administrators: full; interactive (signed-in) users: read/write.
    const SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";
    const MAX_REQUEST: usize = 64 * 1024;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn serve(pipe_name: &str, stop: &AtomicBool, handle: fn(&[u8]) -> Vec<u8>) {
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let sddl = wide(SDDL);
        // SAFETY: valid NUL-terminated wide string; `sd` receives a LocalAlloc'd descriptor.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return;
        }
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };
        let name = wide(pipe_name);
        while !stop.load(Ordering::SeqCst) {
            // SAFETY: `name` and `sa` outlive the call.
            let h = unsafe {
                CreateNamedPipeW(
                    name.as_ptr(),
                    PIPE_ACCESS_DUPLEX,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1,
                    MAX_REQUEST as u32,
                    MAX_REQUEST as u32,
                    0,
                    &sa,
                )
            };
            if h == INVALID_HANDLE_VALUE {
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
            // SAFETY: `h` is a valid pipe handle owned by this loop iteration.
            let connected = unsafe { ConnectNamedPipe(h, std::ptr::null_mut()) } != 0
                || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
            if connected && !stop.load(Ordering::SeqCst) {
                let mut conn = Conn(h);
                let mut request = Vec::new();
                let mut buf = [0u8; 4096];
                while request.len() < MAX_REQUEST && !request.contains(&b'\n') {
                    match conn.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let line = request.split(|b| *b == b'\n').next().unwrap_or_default();
                let _ = conn.write_all(&handle(line));
                // SAFETY: valid handle.
                unsafe { FlushFileBuffers(h) };
            }
            // SAFETY: valid handle; disconnected before closing.
            unsafe {
                DisconnectNamedPipe(h);
                CloseHandle(h);
            }
        }
        // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        unsafe { LocalFree(sd as _) };
    }

    struct Conn(windows_sys::Win32::Foundation::HANDLE);

    impl Read for Conn {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut n = 0u32;
            // SAFETY: `buf` is valid for `buf.len()` bytes.
            let ok = unsafe {
                ReadFile(
                    self.0,
                    buf.as_mut_ptr(),
                    buf.len() as u32,
                    &mut n,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(n as usize)
            }
        }
    }

    impl Write for Conn {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut n = 0u32;
            // SAFETY: `buf` is valid for `buf.len()` bytes.
            let ok = unsafe {
                WriteFile(
                    self.0,
                    buf.as_ptr(),
                    buf.len() as u32,
                    &mut n,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(n as usize)
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_round_trip_answers_version_and_rejects_bad_commands() {
        let name = format!(r"\\.\pipe\OpenLocalServerHelperTest{}", std::process::id());
        let stop = Arc::new(AtomicBool::new(false));
        let (server_name, server_stop) = (name.clone(), stop.clone());
        let server =
            std::thread::spawn(move || pipe::serve(&server_name, &server_stop, handle_request));

        let ask = |request: &str| -> serde_json::Value {
            for _ in 0..50 {
                if let Ok(mut p) = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&name)
                {
                    p.write_all(format!("{request}\n").as_bytes()).unwrap();
                    let mut reply = Vec::new();
                    let mut buf = [0u8; 256];
                    while !reply.contains(&b'\n') {
                        match p.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => reply.extend_from_slice(&buf[..n]),
                        }
                    }
                    return serde_json::from_slice(reply.split(|b| *b == b'\n').next().unwrap())
                        .unwrap();
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            panic!("pipe never came up");
        };
        let v = ask(r#"["version"]"#);
        assert_eq!(v["code"], 0);
        assert_eq!(v["message"], env!("CARGO_PKG_VERSION"));
        let v = ask(r#"["hosts-apply","6.6.6.6=bank.test"]"#);
        assert_eq!(
            v["code"], 2,
            "the service applies the same validation as the one-shot helper"
        );
        let v = ask(r#"["install-service"]"#);
        assert_eq!(
            v["code"], 2,
            "installing is never reachable through the pipe"
        );

        stop.store(true, Ordering::SeqCst);
        let _ = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&name);
        server.join().unwrap();
    }
}
