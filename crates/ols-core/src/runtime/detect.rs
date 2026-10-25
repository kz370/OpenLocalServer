use super::types::*;

/// Look for an existing, unmanaged install of `id` on PATH. Never touches or registers
/// it — purely informational, so the UI can say "found on your system" instead of only
/// ever offering a download (§126: never modify an existing install without asking).
pub fn detect_system_install(id: &str) -> Option<SystemInstall> {
    // Each probe spawns `<tool> --version`, and the UI polls the catalog — remember answers briefly.
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<
            std::collections::HashMap<String, (std::time::Instant, Option<SystemInstall>)>,
        >,
    > = std::sync::OnceLock::new();
    const TTL: std::time::Duration = std::time::Duration::from_secs(60);
    let cache = CACHE.get_or_init(Default::default);
    if let Some((at, found)) = cache.lock().unwrap().get(id) {
        if at.elapsed() < TTL {
            return found.clone();
        }
    }
    let found = probe_system_install(id);
    cache
        .lock()
        .unwrap()
        .insert(id.to_string(), (std::time::Instant::now(), found.clone()));
    found
}

fn clean_system_version(id: &str, raw: &str) -> String {
    let trimmed = raw.trim();
    match id {
        "python" => trimmed
            .strip_prefix("Python ")
            .unwrap_or(trimmed)
            .trim()
            .to_string(),
        "node" => trimmed
            .strip_prefix('v')
            .unwrap_or(trimmed)
            .trim()
            .to_string(),
        "php" => {
            if let Some(rest) = trimmed.strip_prefix("PHP ") {
                rest.split_whitespace().next().unwrap_or(rest).to_string()
            } else {
                trimmed.to_string()
            }
        }
        _ => trimmed.to_string(),
    }
}

fn probe_system_install(id: &str) -> Option<SystemInstall> {
    let (exe_name, version_flag) = crate::catalog::system_probe(id)?;
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let exe_path = dir.join(exe_name);
        if exe_path.is_file() {
            if let Some(raw) = probe_version(&exe_path, version_flag) {
                let cleaned = clean_system_version(id, &raw);
                if !cleaned.is_empty() && cleaned != "unknown version" {
                    return Some(SystemInstall {
                        path: exe_path.display().to_string(),
                        version: cleaned,
                    });
                }
            }
        }
    }
    None
}

/// First line of `<exe> <flag>`. Some tools (an old Windows Redis) never exit on their version flag, or
/// leave a child holding the output pipe open, so the answer goes to a temp file instead of a pipe and
/// the probe is killed after a few seconds. Nothing here can block the catalog for long.
fn probe_version(exe: &std::path::Path, flag: &str) -> Option<String> {
    use std::process::Stdio;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let out_path =
        std::env::temp_dir().join(format!("ols-probe-{}-{nanos}.txt", std::process::id()));
    let out = std::fs::File::create(&out_path).ok()?;
    let err = out.try_clone().ok()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(flag).stdin(Stdio::null()).stdout(out).stderr(err);
    crate::exec::hide_window(&mut cmd);
    let spawned = cmd.spawn();
    let text = spawned.ok().and_then(|mut child| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match child.try_wait().ok()? {
                Some(_) => break,
                None if std::time::Instant::now() >= deadline => {
                    let _ = child.kill();
                    return None;
                }
                None => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
        std::fs::read_to_string(&out_path).ok()
    });
    let _ = std::fs::remove_file(&out_path);
    text.map(|t| {
        t.lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("unknown version")
            .trim()
            .to_string()
    })
}
