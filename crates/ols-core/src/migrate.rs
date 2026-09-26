//! Moving databases over from Laragon, XAMPP or WampServer — no `.sql` export needed.
//!
//! If the old server is running, it's dumped live. If not, its data folder is copied to a
//! temporary folder and the old install's own `mysqld` is started on that copy on a spare
//! port, so the original files are never opened (and can't be damaged or upgraded). Each
//! database is dumped with the old install's `mysqldump` and loaded into ours.

use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::exec::run_capture;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationSource {
    /// Stable id for later calls: the data folder path.
    pub id: String,
    /// "Laragon · MySQL 8.4"
    pub label: String,
    /// "mysql" | "mariadb"
    pub engine: String,
    pub bin_dir: String,
    pub data_dir: String,
    pub size_bytes: u64,
    /// Set when the old server is running right now (dumped live, nothing copied).
    pub running_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigratedDb {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

const SYSTEM_DBS: &[&str] = &["information_schema", "mysql", "performance_schema", "sys"];

// ------------------------------------------------------------------------- progress

/// What a scan or import is doing right now, for the UI to poll while the (blocking)
/// command runs. Byte counts are real where we move the bytes ourselves (copying the data
/// folder, feeding a dump to our server) and a growing file size while `mysqldump` writes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MigrationProgress {
    pub running: bool,
    /// "scan" or "import".
    pub kind: String,
    /// What is happening now, in words: "Copying the data folder", "Exporting shop", ...
    pub step: String,
    /// The database being worked on (1-based) and how many there are; 0 before they are known.
    pub db_index: usize,
    pub db_total: usize,
    /// The database being worked on.
    pub current_db: Option<String>,
    /// Progress through the current step.
    pub bytes: u64,
    /// Size of the current step when known (an estimate for exports).
    pub bytes_total: Option<u64>,
    /// Databases finished so far.
    pub done: Vec<MigratedDb>,
    pub started_ms: u64,
    /// A file `mysqldump` is writing; its size is read on each poll.
    #[serde(skip)]
    watch_file: Option<PathBuf>,
}

#[derive(Default)]
pub struct Progress(std::sync::Mutex<MigrationProgress>);

impl Progress {
    pub fn begin(&self, kind: &str) {
        let started_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        *self.0.lock().unwrap() = MigrationProgress { running: true, kind: kind.into(), started_ms, ..Default::default() };
    }

    pub fn end(&self) {
        let mut p = self.0.lock().unwrap();
        p.running = false;
        p.watch_file = None;
    }

    /// Starts a new step; its byte counter resets.
    pub fn step(&self, step: impl Into<String>, bytes_total: Option<u64>) {
        let mut p = self.0.lock().unwrap();
        p.step = step.into();
        p.bytes = 0;
        p.bytes_total = bytes_total;
        p.watch_file = None;
    }

    pub fn database(&self, index: usize, total: usize, name: &str) {
        let mut p = self.0.lock().unwrap();
        p.db_index = index;
        p.db_total = total;
        p.current_db = Some(name.to_string());
    }

    pub fn add_bytes(&self, n: u64) {
        self.0.lock().unwrap().bytes += n;
    }

    fn watch(&self, file: &Path) {
        self.0.lock().unwrap().watch_file = Some(file.to_path_buf());
    }

    pub fn finished(&self, db: MigratedDb) {
        let mut p = self.0.lock().unwrap();
        p.current_db = None;
        p.done.push(db);
    }

    pub fn snapshot(&self) -> MigrationProgress {
        let mut p = self.0.lock().unwrap().clone();
        if let Some(file) = &p.watch_file {
            p.bytes = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
        }
        p
    }
}

// ------------------------------------------------------------------------ discovery

/// Finds Laragon, XAMPP and WampServer MySQL/MariaDB data folders on every drive.
pub fn detect() -> Vec<MigrationSource> {
    let mut out = Vec::new();
    let running = running_servers();
    for drive in b'C'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\", drive as char));
        if !root.exists() {
            continue;
        }
        laragon(&root.join("laragon"), &running, &mut out);
        xampp(&root.join("xampp"), &running, &mut out);
        wamp(&root.join("wamp64"), &running, &mut out);
        wamp(&root.join("wamp"), &running, &mut out);
    }
    out
}

fn engine_of(dir_name: &str) -> &'static str {
    if dir_name.to_ascii_lowercase().contains("mariadb") {
        "mariadb"
    } else {
        "mysql"
    }
}

/// `…\bin\mysql\mysql-8.4.3-winx64` + `…\data\mysql-8.4`. Laragon names data folders by
/// engine and short version (`mysql-8` for 8.0), so match the most specific binary.
fn laragon(root: &Path, running: &[(PathBuf, u16)], out: &mut Vec<MigrationSource>) {
    let Ok(datas) = std::fs::read_dir(root.join("data")) else { return };
    let bins: Vec<PathBuf> = std::fs::read_dir(root.join("bin").join("mysql"))
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.join("bin").join("mysqld.exe").is_file()).collect())
        .unwrap_or_default();
    for data in datas.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        let name = data.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
        if !(name.starts_with("mysql") || name.starts_with("mariadb")) {
            continue;
        }
        let bin_name = |b: &PathBuf| b.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();
        // "mysql-8" means 8.0; otherwise the data name is a prefix of the binary's name.
        let wanted = if name == "mysql-8" { "mysql-8.0".to_string() } else { name.clone() };
        let bin = bins
            .iter()
            .find(|b| bin_name(b).starts_with(&format!("{wanted}.")) || bin_name(b).starts_with(&format!("{wanted}-")))
            .or_else(|| bins.iter().find(|b| bin_name(b).starts_with(&format!("{name}."))));
        if let Some(bin) = bin {
            push(out, &format!("Laragon · {}", pretty(&name)), engine_of(&name), &bin.join("bin"), &data, running);
        }
    }
}

fn xampp(root: &Path, running: &[(PathBuf, u16)], out: &mut Vec<MigrationSource>) {
    let bin = root.join("mysql").join("bin");
    if bin.join("mysqld.exe").is_file() {
        let engine = if bin.join("mariadbd.exe").is_file() || bin.join("mariadb.exe").is_file() { "mariadb" } else { "mysql" };
        push(out, &format!("XAMPP · {}", if engine == "mariadb" { "MariaDB" } else { "MySQL" }), engine, &bin, &root.join("mysql").join("data"), running);
    }
}

/// `…\bin\mysql\mysql8.0.31\{bin,data}` and `…\bin\mariadb\mariadb10.x\{bin,data}`.
fn wamp(root: &Path, running: &[(PathBuf, u16)], out: &mut Vec<MigrationSource>) {
    for family in ["mysql", "mariadb"] {
        let Ok(rd) = std::fs::read_dir(root.join("bin").join(family)) else { continue };
        for dir in rd.flatten().map(|e| e.path()) {
            if dir.join("bin").join("mysqld.exe").is_file() && dir.join("data").is_dir() {
                let name = dir.file_name().unwrap_or_default().to_string_lossy().to_string();
                push(out, &format!("WampServer · {name}"), family, &dir.join("bin"), &dir.join("data"), running);
            }
        }
    }
}

fn pretty(name: &str) -> String {
    let (engine, version) = name.split_once('-').unwrap_or((name, ""));
    let engine = if engine == "mariadb" { "MariaDB" } else { "MySQL" };
    format!("{engine} {version}").trim().to_string()
}

fn push(out: &mut Vec<MigrationSource>, label: &str, engine: &str, bin: &Path, data: &Path, running: &[(PathBuf, u16)]) {
    if !data.is_dir() {
        return;
    }
    let exe = bin.join("mysqld.exe");
    let running_port = running.iter().find(|(p, _)| same_file(p, &exe)).map(|(_, port)| *port);
    out.push(MigrationSource {
        id: data.display().to_string(),
        label: label.to_string(),
        engine: engine.to_string(),
        bin_dir: bin.display().to_string(),
        data_dir: data.display().to_string(),
        size_bytes: dir_size(data),
        running_port,
    });
}

fn same_file(a: &Path, b: &Path) -> bool {
    a.display().to_string().eq_ignore_ascii_case(&b.display().to_string())
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// mysqld processes running now, with their port (from `--port=`, else 3306).
fn running_servers() -> Vec<(PathBuf, u16)> {
    let script = "Get-CimInstance Win32_Process -Filter \"Name='mysqld.exe' or Name='mariadbd.exe'\" | ForEach-Object { \"$($_.ExecutablePath)|$($_.CommandLine)\" }";
    let out = run_capture(
        Path::new("powershell"),
        &["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), script.into()],
        None,
        &[],
        Duration::from_secs(20),
    );
    out.stdout
        .lines()
        .filter_map(|l| {
            let (exe, cmd) = l.split_once('|')?;
            let port = cmd.split("--port=").nth(1).and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next()).and_then(|p| p.parse().ok()).unwrap_or(3306);
            (!exe.is_empty()).then(|| (PathBuf::from(exe), port))
        })
        .collect()
}

// ------------------------------------------------------------------------ the old server

/// A connection to the old server: live, or a throwaway copy we started.
pub struct Session {
    pub source: MigrationSource,
    port: u16,
    password: String,
    temp: Option<(Child, PathBuf)>,
}

impl Session {
    pub fn open(source: MigrationSource, password: &str, work_dir: &Path, progress: &Progress) -> Result<Self, String> {
        if let Some(port) = source.running_port {
            progress.step(format!("Connecting to the running {}", source.label), None);
            return Ok(Self { source, port, password: password.to_string(), temp: None });
        }
        let copy = work_dir.join(format!("migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&copy);
        progress.step("Copying the data folder (the original is not touched)", Some(source.size_bytes));
        copy_data(Path::new(&source.data_dir), &copy, &|n| progress.add_bytes(n)).map_err(|e| format!("could not copy {}: {e}", source.data_dir))?;
        progress.step(format!("Starting a temporary copy of {}", source.label), None);
        let port = free_port().ok_or("no free port for the temporary server")?;
        let bin = PathBuf::from(&source.bin_dir);
        let mut cmd = Command::new(bin.join("mysqld.exe"));
        cmd.args([
            "--no-defaults".to_string(),
            format!("--basedir={}", bin.parent().unwrap_or(&bin).display()),
            format!("--datadir={}", copy.display()),
            format!("--port={port}"),
            "--bind-address=127.0.0.1".to_string(),
            "--skip-log-bin".to_string(),
            "--loose-mysqlx=OFF".to_string(),
            "--innodb-buffer-pool-size=128M".to_string(),
            format!("--log-error={}", copy.join("migrate.err").display()),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        // Not piped: nothing reads it while the server runs, and a full pipe would stall it.
        .stderr(Stdio::null());
        crate::exec::hide_window(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| format!("could not start {}: {e}", bin.join("mysqld.exe").display()))?;
        let started = Instant::now();
        loop {
            if TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300)).is_ok() {
                break;
            }
            if let Ok(Some(status)) = child.try_wait() {
                let err = std::fs::read_to_string(copy.join("migrate.err")).unwrap_or_default();
                let _ = std::fs::remove_dir_all(&copy);
                let tail: Vec<&str> = err.lines().rev().take(4).collect();
                return Err(format!("the old server stopped ({status}): {}", tail.into_iter().rev().collect::<Vec<_>>().join(" / ")));
            }
            if started.elapsed() > Duration::from_secs(90) {
                kill_tree(&mut child);
                let _ = std::fs::remove_dir_all(&copy);
                return Err("the old server did not start within 90 seconds".into());
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        Ok(Self { source, port, password: password.to_string(), temp: Some((child, copy)) })
    }

    fn env(&self) -> Vec<(String, String)> {
        vec![("MYSQL_PWD".into(), self.password.clone())]
    }

    fn conn_args(&self) -> Vec<String> {
        vec!["-h".into(), "127.0.0.1".into(), "-P".into(), self.port.to_string(), "-u".into(), "root".into()]
    }

    pub fn databases(&self) -> Result<Vec<String>, String> {
        let mut args = self.conn_args();
        args.extend(["--batch".into(), "--skip-column-names".into(), "-e".into(), "SHOW DATABASES".into()]);
        let out = run_capture(&PathBuf::from(&self.source.bin_dir).join("mysql.exe"), &args, None, &self.env(), Duration::from_secs(30));
        if !out.success() {
            return Err(access_hint(&out.combined()));
        }
        Ok(out.stdout.lines().map(str::trim).filter(|l| !l.is_empty() && !SYSTEM_DBS.contains(l)).map(str::to_string).collect())
    }

    /// Approximate size of each database (data + indexes), for export progress.
    pub fn sizes(&self) -> std::collections::HashMap<String, u64> {
        let mut args = self.conn_args();
        let sql = "SELECT table_schema, COALESCE(SUM(data_length + index_length), 0) FROM information_schema.tables GROUP BY table_schema";
        args.extend(["--batch".into(), "--skip-column-names".into(), "-e".into(), sql.into()]);
        let out = run_capture(&PathBuf::from(&self.source.bin_dir).join("mysql.exe"), &args, None, &self.env(), Duration::from_secs(30));
        if !out.success() {
            return Default::default();
        }
        out.stdout
            .lines()
            .filter_map(|l| {
                let (name, size) = l.split_once('\t')?;
                Some((name.to_string(), size.trim().parse().ok()?))
            })
            .collect()
    }

    /// Dumps one database to `file` (with routines, triggers and events).
    pub fn dump(&self, db: &str, file: &Path, progress: &Progress) -> Result<(), String> {
        progress.watch(file);
        let mut args = self.conn_args();
        args.extend([
            "--single-transaction".into(),
            "--routines".into(),
            "--triggers".into(),
            "--events".into(),
            "--hex-blob".into(),
            "--default-character-set=utf8mb4".into(),
            format!("--result-file={}", file.display()),
            "--databases".into(),
            db.into(),
        ]);
        if self.source.engine == "mysql" {
            // GTID statements would fail on a server that isn't set up for replication.
            args.push("--set-gtid-purged=OFF".into());
        }
        let out = run_capture(&PathBuf::from(&self.source.bin_dir).join("mysqldump.exe"), &args, None, &self.env(), Duration::from_secs(3600));
        if out.success() { Ok(()) } else { Err(access_hint(&out.combined())) }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some((mut child, copy)) = self.temp.take() {
            // A throwaway copy: a hard stop is fine, then the copy goes.
            kill_tree(&mut child);
            for _ in 0..20 {
                if std::fs::remove_dir_all(&copy).is_ok() || !copy.exists() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
}

/// MySQL 8 on Windows runs the real server as a child of a monitor process, so a plain
/// kill would leave it running (and holding the copied files).
fn kill_tree(child: &mut Child) {
    let mut kill = Command::new("taskkill");
    kill.args(["/PID", &child.id().to_string(), "/T", "/F"]).stdout(Stdio::null()).stderr(Stdio::null());
    crate::exec::hide_window(&mut kill);
    let _ = kill.status();
    let _ = child.kill();
    let _ = child.wait();
}

fn access_hint(err: &str) -> String {
    if err.contains("Access denied") {
        format!("{} (enter the old server's root password; Laragon and XAMPP use none by default)", err.trim())
    } else {
        err.trim().to_string()
    }
}

/// Copies a data folder, leaving out binary logs, error logs and pid files: they can be
/// large and the temporary server doesn't need them. `on_bytes` hears every chunk copied.
fn copy_data(from: &Path, to: &Path, on_bytes: &dyn Fn(u64)) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let name = e.file_name().to_string_lossy().to_ascii_lowercase();
        let skip = name.ends_with(".pid")
            || name.ends_with(".err")
            || name.ends_with(".log") && !name.starts_with("ib_logfile") && !name.starts_with("aria_log")
            || name.starts_with("binlog.")
            || name.contains("-bin.")
            || name.ends_with(".index") && (name.contains("bin") || name.contains("relay"));
        if skip {
            continue;
        }
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_data(&e.path(), &target, on_bytes)?;
        } else {
            copy_file(&e.path(), &target, on_bytes)?;
        }
    }
    Ok(())
}

/// `std::fs::copy` in 1 MB chunks, so progress moves during a multi-gigabyte `ibdata1`.
fn copy_file(from: &Path, to: &Path, on_bytes: &dyn Fn(u64)) -> std::io::Result<()> {
    use std::io::{Read, Write};
    let mut input = std::fs::File::open(from)?;
    let mut output = std::fs::File::create(to)?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        output.write_all(&buf[..n])?;
        on_bytes(n as u64);
    }
    Ok(())
}

fn free_port() -> Option<u16> {
    std::net::TcpListener::bind(("127.0.0.1", 0)).ok()?.local_addr().ok().map(|a| a.port())
}

/// Loads a dump into our server through its own client. We feed the file through stdin
/// ourselves, so every byte the server has taken shows up in `progress`.
pub fn import(client: &Path, port: u16, file: &Path, progress: &Progress) -> Result<(), String> {
    use std::io::{Read, Write};
    let mut input = std::fs::File::open(file).map_err(|e| format!("could not read {}: {e}", file.display()))?;
    let mut cmd = Command::new(client);
    cmd.args(["-h", "127.0.0.1", "-P", &port.to_string(), "-u", "root", "--default-character-set=utf8mb4"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    crate::exec::hide_window(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("could not start {}: {e}", client.display()))?;
    // Read stderr on its own thread: a client that fills that pipe would otherwise stall.
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = input.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        // A write error means the client quit early; its exit status and stderr say why.
        if stdin.write_all(&buf[..n]).is_err() {
            break;
        }
        progress.add_bytes(n as u64);
    }
    drop(stdin);
    let status = child.wait().map_err(|e| e.to_string())?;
    let err = errors.join().unwrap_or_default();
    if status.success() { Ok(()) } else { Err(err.trim().to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn laragon_data_folders_pair_with_the_right_binaries() {
        let root = tempfile::tempdir().unwrap();
        for bin in ["mysql-8.0.30-winx64", "mysql-8.4.3-winx64", "mariadb-11.8.5"] {
            let b = root.path().join("bin").join("mysql").join(bin).join("bin");
            std::fs::create_dir_all(&b).unwrap();
            std::fs::write(b.join("mysqld.exe"), "").unwrap();
        }
        for data in ["mysql-8", "mysql-8.4", "mariadb-11.8", "postgresql-18"] {
            std::fs::create_dir_all(root.path().join("data").join(data)).unwrap();
        }
        let mut found = Vec::new();
        laragon(root.path(), &[], &mut found);
        let pairs: Vec<(String, String)> = found
            .iter()
            .map(|s| (Path::new(&s.data_dir).file_name().unwrap().to_string_lossy().to_string(), Path::new(&s.bin_dir).parent().unwrap().file_name().unwrap().to_string_lossy().to_string()))
            .collect();
        assert!(pairs.contains(&("mysql-8".into(), "mysql-8.0.30-winx64".into())));
        assert!(pairs.contains(&("mysql-8.4".into(), "mysql-8.4.3-winx64".into())));
        assert!(pairs.contains(&("mariadb-11.8".into(), "mariadb-11.8.5".into())));
        assert_eq!(found.len(), 3, "PostgreSQL isn't a MySQL-family source");
        assert!(found.iter().any(|s| s.engine == "mariadb" && s.label == "Laragon · MariaDB 11.8"));
    }

    #[test]
    fn copies_skip_binary_logs_but_keep_innodb_files() {
        let from = tempfile::tempdir().unwrap();
        for f in ["ibdata1", "ib_logfile0", "binlog.000003", "binlog.index", "HOST.err", "aria_log.00000001", "mysql.pid"] {
            std::fs::write(from.path().join(f), "x").unwrap();
        }
        std::fs::create_dir_all(from.path().join("shop")).unwrap();
        std::fs::write(from.path().join("shop").join("orders.ibd"), "x").unwrap();
        let to = tempfile::tempdir().unwrap();
        let copied = std::cell::Cell::new(0u64);
        copy_data(from.path(), to.path(), &|n| copied.set(copied.get() + n)).unwrap();
        assert_eq!(copied.get(), 4,"only the kept files count toward progress");
        for kept in ["ibdata1", "ib_logfile0", "aria_log.00000001", "shop/orders.ibd"] {
            assert!(to.path().join(kept).exists(), "{kept} must be copied");
        }
        for skipped in ["binlog.000003", "binlog.index", "HOST.err", "mysql.pid"] {
            assert!(!to.path().join(skipped).exists(), "{skipped} must be skipped");
        }
    }
}
