//! PHP FastCGI pools (§11, Stage 6): one supervised set of `php-cgi` workers per PHP
//! version. Each site's `fastcgi_pass` targets the pool of the version it resolves to, so
//! `a.test` can run 8.1 while `b.test` runs 8.4 on the same machine at the same time.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::paths::AppPaths;
use crate::process::{ProcessId, ProcessSpec, ProcessSupervisor, RestartPolicy};
use crate::runtime::RuntimeManager;
use crate::xdebug::{XdebugReport, XdebugSettings};

/// Extensions enabled when their DLL exists in the install's `ext/` folder — framework
/// requirements plus OPcache for faster PHP request handling (§154).
const EXTENSIONS: &[&str] = &[
    "curl", "fileinfo", "gd", "intl", "mbstring", "exif", "mysqli", "openssl", "opcache", "pdo_mysql", "pdo_sqlite", "sqlite3",
    "zip", "sodium", "bcmath", "soap", "gettext",
];

/// Extensions that hook the engine itself and must be loaded with `zend_extension=`.
const ZEND_EXTENSIONS: &[&str] = &["opcache", "xdebug"];

/// One extension DLL available to a PHP version, and whether its php.ini loads it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhpExtension {
    pub name: String,
    pub enabled: bool,
    /// Downloaded by us (PECL) rather than shipped in the install's own `ext/`.
    pub downloaded: bool,
    #[serde(skip)]
    dll: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhpExtensions {
    pub version: String,
    /// Thread-safe build: decides which PECL download matches.
    pub thread_safe: bool,
    pub extensions: Vec<PhpExtension>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolStatus {
    pub version: String,
    pub ports: Vec<u16>,
    pub running: bool,
}

struct Pool {
    version: String,
    ports: Vec<u16>,
    processes: Vec<ProcessId>,
}

pub struct PhpPools {
    paths: AppPaths,
    runtimes: Arc<RuntimeManager>,
    supervisor: Arc<ProcessSupervisor>,
    pools: Mutex<HashMap<String, Pool>>,
    /// PHP installs the user pointed at (version -> `php.exe`), alongside the managed ones.
    external: Mutex<HashMap<String, PathBuf>>,
    /// PECL's package list, fetched once per session.
    pecl_packages: Mutex<Option<Vec<String>>>,
}

impl PhpPools {
    pub fn new(paths: AppPaths, runtimes: Arc<RuntimeManager>, supervisor: Arc<ProcessSupervisor>) -> Self {
        Self {
            paths,
            runtimes,
            supervisor,
            pools: Mutex::new(HashMap::new()),
            external: Mutex::new(HashMap::new()),
            pecl_packages: Mutex::new(None),
        }
    }

    /// Replaces the set of user-registered PHP installs (version label -> `php.exe`).
    pub fn set_external(&self, entries: Vec<(String, PathBuf)>) {
        *self.external.lock().unwrap() = entries.into_iter().collect();
    }

    /// Managed plus user-registered versions.
    pub fn all_versions(&self) -> Vec<String> {
        let mut all = self.runtimes.installed_versions("php");
        for v in self.external.lock().unwrap().keys() {
            if !all.contains(v) {
                all.push(v.clone());
            }
        }
        all
    }

    /// The folder holding `php.exe`, `php-cgi.exe` and `ext/` for `version`.
    fn install_dir_of(&self, version: &str) -> PathBuf {
        match self.external.lock().unwrap().get(version).and_then(|exe| exe.parent()) {
            Some(dir) => dir.to_path_buf(),
            None => self.runtimes.install_dir("php", version),
        }
    }

    /// Which installed PHP satisfies `requested` ("8.1" or "8.1.34" or None = newest)?
    pub fn pick_version(&self, requested: Option<&str>) -> Option<String> {
        pick_version(&self.all_versions(), requested)
    }

    /// Name-safe pool id for a full version: "8.1.34" → "php_81".
    pub fn pool_id(version: &str) -> String {
        let mut parts = version.split('.');
        format!("php_{}{}", parts.next().unwrap_or("0"), parts.next().unwrap_or("0"))
    }

    /// Deterministic ports so a restart reuses the same ones: 10000 + major*100 + minor*10 + worker.
    fn ports_for(version: &str, workers: u16) -> Vec<u16> {
        let mut parts = version.split('.').map(|p| p.parse::<u16>().unwrap_or(0));
        let (major, minor) = (parts.next().unwrap_or(8), parts.next().unwrap_or(0));
        let base = 10_000 + major * 100 + minor * 10;
        (0..workers.clamp(1, 10)).map(|w| base + w).collect()
    }

    /// The directory holding this version's generated `php.ini`. Passing it as `PHPRC` makes
    /// both `php-cgi` and CLI `php` (Composer!) load it.
    pub fn ini_dir(&self, version: &str) -> PathBuf {
        self.paths.services_dir().join("php").join(version)
    }

    /// Writes a php.ini enabling the extensions this install actually ships.
    pub fn write_ini(&self, version: &str) -> Result<PathBuf, String> {
        let install = self.install_dir_of(version);
        let ext_dir = install.join("ext");
        let mut ini = String::new();
        ini.push_str("; Generated by OpenLocalServer - regenerated on every start.\n[PHP]\n");
        ini.push_str(&format!("extension_dir = \"{}\"\n", ext_dir.display().to_string().replace('\\', "/")));
        ini.push_str("display_errors = On\nerror_reporting = E_ALL\nmemory_limit = 512M\n");
        ini.push_str("upload_max_filesize = 128M\npost_max_size = 128M\nmax_execution_time = 120\n");
        ini.push_str("date.timezone = UTC\ncgi.fix_pathinfo = 1\ncgi.force_redirect = 0\nfastcgi.impersonate = 1\n");
        // PHP's mail() on Windows speaks SMTP directly — point it at Mailpit (§63).
        ini.push_str(&format!("SMTP = 127.0.0.1\nsmtp_port = {}\nsendmail_from = dev@localhost\n", crate::service::mailpit_smtp_port()));
        // Full paths: a bare name makes PHP guess the file name, which fails for DLLs
        // named like `php_foo_8.3.dll` and can't reach the ones we downloaded.
        let enabled: Vec<PhpExtension> = self.extensions(version).into_iter().filter(|e| e.enabled).collect();
        for ext in &enabled {
            let directive = if ZEND_EXTENSIONS.contains(&ext.name.as_str()) { "zend_extension" } else { "extension" };
            ini.push_str(&format!("{directive}=\"{}\"\n", ext.dll.display().to_string().replace('\\', "/")));
        }
        if enabled.iter().any(|e| e.name == "opcache") {
            // Keep the shared bytecode cache useful for Laravel's large dependency tree,
            // while checking files on every request so edits show up immediately.
            ini.push_str("opcache.memory_consumption=128\nopcache.max_accelerated_files=20000\nopcache.validate_timestamps=1\nopcache.revalidate_freq=0\n");
        }
        if enabled.iter().any(|e| e.name == "xdebug") {
            let output_dir = self.ini_dir(version).join("xdebug");
            let _ = std::fs::create_dir_all(&output_dir);
            ini.push_str(&self.xdebug_settings(version).ini_block(&output_dir));
        }
        let dir = self.ini_dir(version);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("php.ini"), ini).map_err(|e| e.to_string())?;
        Ok(dir)
    }

    /// Ensures `workers` php-cgi processes are up for `version`; returns their ports.
    pub fn ensure(&self, version: &str, workers: u16) -> Result<Vec<u16>, String> {
        let ports = Self::ports_for(version, workers);
        let mut pools = self.pools.lock().unwrap();
        if let Some(existing) = pools.get(version) {
            let all_alive = existing.processes.iter().all(|p| self.supervisor.is_alive(*p));
            if existing.ports == ports && all_alive {
                return Ok(ports);
            }
            // Wrong size or a dead worker: rebuild the whole pool so the ports line up.
            for id in &existing.processes {
                self.supervisor.stop(*id);
            }
            pools.remove(version);
        }

        let binary = self.install_dir_of(version).join("php-cgi.exe");
        if !binary.is_file() {
            return Err(format!("php-cgi.exe is missing from PHP {version}"));
        }
        let ini_dir = self.write_ini(version)?;

        let mut processes = Vec::new();
        for port in &ports {
            let id = self.supervisor.start(ProcessSpec {
                name: format!("PHP {version} FastCGI :{port}"),
                executable: binary.display().to_string(),
                args: vec!["-b".into(), format!("127.0.0.1:{port}")],
                cwd: None,
                env: vec![
                    ("PHPRC".into(), ini_dir.display().to_string()),
                    ("PHP_FCGI_MAX_REQUESTS".into(), "0".into()),
                    ("PATH".into(), self.path_with_extension_deps(version)),
                ],
                restart: Some(RestartPolicy { max_retries: 5, delay_ms: 1000 }),
            });
            processes.push(id);
        }
        pools.insert(version.to_string(), Pool { version: version.to_string(), ports: ports.clone(), processes });
        Ok(ports)
    }

    /// Restarts a running pool so it picks up a changed php.ini; a pool that isn't
    /// running is left alone (it reads the new ini whenever it next starts).
    pub fn restart(&self, version: &str) -> Result<(), String> {
        let workers = {
            let mut pools = self.pools.lock().unwrap();
            let Some(pool) = pools.remove(version) else { return Ok(()) };
            for id in &pool.processes {
                self.supervisor.stop(*id);
            }
            pool.ports.len() as u16
        };
        self.ensure(version, workers).map(|_| ())
    }

    // ------------------------------------------------------------ extensions

    /// Where extensions we download for `version` live: never inside the install
    /// itself, which may be the user's own (Laragon, XAMPP, ...) folder.
    fn downloaded_ext_dir(&self, version: &str) -> PathBuf {
        self.ini_dir(version).join("ext")
    }

    fn overrides_file(&self, version: &str) -> PathBuf {
        self.ini_dir(version).join("extensions.json")
    }

    /// The user's on/off choices; an extension without one follows [`EXTENSIONS`].
    fn overrides(&self, version: &str) -> BTreeMap<String, bool> {
        std::fs::read_to_string(self.overrides_file(version))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn save_overrides(&self, version: &str, map: &BTreeMap<String, bool>) -> Result<(), String> {
        let file = self.overrides_file(version);
        std::fs::create_dir_all(self.ini_dir(version)).map_err(|e| e.to_string())?;
        let raw = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
        std::fs::write(&file, raw).map_err(|e| e.to_string())
    }

    /// Every `php_*.dll` this version can load: its own `ext/` plus our downloads.
    pub fn extensions(&self, version: &str) -> Vec<PhpExtension> {
        let overrides = self.overrides(version);
        let mut found: BTreeMap<String, PhpExtension> = BTreeMap::new();
        let dirs = [(self.install_dir_of(version).join("ext"), false), (self.downloaded_ext_dir(version), true)];
        for (dir, downloaded) in dirs {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let file = e.file_name().to_string_lossy().to_lowercase();
                let Some(name) = file.strip_prefix("php_").and_then(|n| n.strip_suffix(".dll")) else { continue };
                let enabled = overrides.get(name).copied().unwrap_or(EXTENSIONS.contains(&name));
                found.insert(name.to_string(), PhpExtension { name: name.to_string(), enabled, downloaded, dll: e.path() });
            }
        }
        found.into_values().collect()
    }

    pub fn extensions_report(&self, version: &str) -> PhpExtensions {
        PhpExtensions { version: version.to_string(), thread_safe: self.is_thread_safe(version), extensions: self.extensions(version) }
    }

    /// Turns one extension on or off and restarts that version's pool if it's running.
    pub fn set_extension(&self, version: &str, name: &str, enabled: bool) -> Result<(), String> {
        if !self.extensions(version).iter().any(|e| e.name == name) {
            return Err(format!("PHP {version} has no {name} extension"));
        }
        let mut map = self.overrides(version);
        map.insert(name.to_string(), enabled);
        self.save_overrides(version, &map)?;
        self.restart(version)
    }

    // ---------------------------------------------------------------- xdebug (§13)

    fn xdebug_file(&self, version: &str) -> PathBuf {
        self.ini_dir(version).join("xdebug.json")
    }

    /// The saved Xdebug settings for `version`, or the defaults.
    pub fn xdebug_settings(&self, version: &str) -> XdebugSettings {
        std::fs::read_to_string(self.xdebug_file(version)).ok().and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default()
    }

    pub fn xdebug_report(&self, version: &str) -> XdebugReport {
        let xdebug = self.extensions(version).into_iter().find(|e| e.name == "xdebug");
        XdebugReport {
            version: version.to_string(),
            installed: xdebug.is_some(),
            enabled: xdebug.is_some_and(|e| e.enabled),
            settings: self.xdebug_settings(version),
        }
    }

    /// Saves the settings and restarts the version's pool so they take effect.
    pub fn set_xdebug_settings(&self, version: &str, settings: &XdebugSettings) -> Result<(), String> {
        settings.validate()?;
        std::fs::create_dir_all(self.ini_dir(version)).map_err(|e| e.to_string())?;
        let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
        std::fs::write(self.xdebug_file(version), raw).map_err(|e| e.to_string())?;
        self.restart(version)
    }

    /// TS builds ship `php8ts.dll`, NTS builds `php8.dll`.
    fn is_thread_safe(&self, version: &str) -> bool {
        let major = version.split('.').next().unwrap_or("8");
        self.install_dir_of(version).join(format!("php{major}ts.dll")).is_file()
    }

    /// The system PATH with our downloaded-extension folder first, so an extension's own
    /// dependency DLLs (imagick ships dozens) are found when PHP loads it.
    pub fn path_with_extension_deps(&self, version: &str) -> String {
        let system = std::env::var("PATH").unwrap_or_default();
        format!("{};{system}", self.downloaded_ext_dir(version).display())
    }

    /// Names of the extensions PECL publishes Windows builds for.
    pub fn pecl_packages(&self) -> Result<Vec<String>, String> {
        if let Some(list) = self.pecl_packages.lock().unwrap().clone() {
            return Ok(list);
        }
        let body = self.runtimes.fetch(pecl::RELEASES)?;
        let list = pecl::package_names(&String::from_utf8_lossy(&body));
        *self.pecl_packages.lock().unwrap() = Some(list.clone());
        Ok(list)
    }

    /// Downloads the newest PECL build of `name` matching this PHP (minor version,
    /// TS/NTS, x64), unpacks its DLLs into our extension folder and enables it.
    /// Returns the extension release installed.
    pub fn install_extension(&self, version: &str, name: &str) -> Result<String, String> {
        let name = name.trim().to_lowercase();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("\"{name}\" is not a valid extension name"));
        }
        let minor = version.split('.').take(2).collect::<Vec<_>>().join(".");
        let ts = self.is_thread_safe(version);
        let package_url = format!("{}{name}/", pecl::RELEASES);
        let listing = self.runtimes.fetch(&package_url).map_err(|_| format!("PECL has no Windows builds of \"{name}\""))?;
        // Newest releases often lag behind for older PHP; walk back a few to find a match.
        for release in pecl::releases_newest_first(&String::from_utf8_lossy(&listing)).into_iter().take(12) {
            let Ok(files) = self.runtimes.fetch(&format!("{package_url}{release}/")) else { continue };
            let Some(zip_name) = pecl::matching_zip(&String::from_utf8_lossy(&files), &name, &release, &minor, ts) else {
                continue;
            };
            let zip = self.runtimes.fetch(&format!("{package_url}{release}/{zip_name}"))?;
            pecl::extract_dlls(&zip, &self.downloaded_ext_dir(version), &name)?;
            let mut map = self.overrides(version);
            map.insert(name.clone(), true);
            self.save_overrides(version, &map)?;
            self.restart(version)?;
            return Ok(release);
        }
        Err(format!(
            "No recent PECL build of {name} matches PHP {minor} {} x64",
            if ts { "thread-safe" } else { "non-thread-safe" }
        ))
    }

    /// Each running pool's worker processes, by PHP version.
    pub fn pool_processes(&self) -> HashMap<String, Vec<ProcessId>> {
        self.pools.lock().unwrap().iter().map(|(v, p)| (v.clone(), p.processes.clone())).collect()
    }

    pub fn stop_all(&self) {
        let mut pools = self.pools.lock().unwrap();
        for (_, pool) in pools.drain() {
            for id in pool.processes {
                self.supervisor.stop(id);
            }
        }
    }

    /// Stops pools for versions not in `keep` (a version no site uses any more).
    pub fn retain(&self, keep: &[String]) {
        let mut pools = self.pools.lock().unwrap();
        let stale: Vec<String> = pools.keys().filter(|v| !keep.contains(v)).cloned().collect();
        for version in stale {
            if let Some(pool) = pools.remove(&version) {
                for id in pool.processes {
                    self.supervisor.stop(id);
                }
            }
        }
    }

    pub fn status(&self) -> Vec<PoolStatus> {
        let pools = self.pools.lock().unwrap();
        let mut list: Vec<PoolStatus> = pools
            .values()
            .map(|p| PoolStatus {
                version: p.version.clone(),
                ports: p.ports.clone(),
                running: p.processes.iter().all(|id| self.supervisor.is_alive(*id)),
            })
            .collect();
        list.sort_by(|a, b| a.version.cmp(&b.version));
        list
    }
}

/// A PHP install found by [`scan_folder`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScannedPhp {
    pub version: String,
    pub php_exe: String,
}

/// Finds PHP installs under `dir`: the folder itself and every folder up to two levels
/// below it that holds both `php.exe` and `php-cgi.exe` (a Laragon-style `bin/php/php-8.4.22...`
/// layout works when pointed at either `bin` or `php`). Versions come from `php -v`, so
/// folder names don't matter.
pub fn scan_folder(dir: &std::path::Path) -> Vec<ScannedPhp> {
    fn candidates(dir: &std::path::Path, depth: u8, out: &mut Vec<PathBuf>) {
        if dir.join("php.exe").is_file() && dir.join("php-cgi.exe").is_file() {
            out.push(dir.to_path_buf());
            return;
        }
        if depth == 0 {
            return;
        }
        if let Ok(rd) = std::fs::read_dir(dir) {
            let mut kids: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
            kids.sort();
            for kid in kids {
                candidates(&kid, depth - 1, out);
            }
        }
    }
    let mut dirs = Vec::new();
    candidates(dir, 2, &mut dirs);
    let mut found = Vec::new();
    for d in dirs {
        let exe = d.join("php.exe");
        let out = crate::exec::run_capture(&exe, &["-v".to_string()], None, &[], std::time::Duration::from_secs(10));
        if let Some(version) = parse_php_version(&out.stdout) {
            found.push(ScannedPhp { version, php_exe: exe.display().to_string() });
        }
    }
    found
}

/// "PHP 8.4.22 (cli) (built: ...)" gives "8.4.22".
fn parse_php_version(output: &str) -> Option<String> {
    let rest = output.lines().next()?.trim().strip_prefix("PHP ")?;
    let version = rest.split_whitespace().next()?;
    version.chars().next().is_some_and(|c| c.is_ascii_digit()).then(|| version.to_string())
}

/// "8.1" matches "8.1.34"; None (or an exact version) picks accordingly; newest wins ties.
pub fn pick_version(installed: &[String], requested: Option<&str>) -> Option<String> {
    let mut candidates: Vec<&String> = match requested {
        Some(req) if !req.is_empty() => {
            let prefix = format!("{req}.");
            installed.iter().filter(|v| v.as_str() == req || v.starts_with(&prefix)).collect()
        }
        _ => installed.iter().collect(),
    };
    candidates.sort_by_key(|v| version_key(v));
    candidates.last().map(|v| (*v).clone())
}

fn version_key(version: &str) -> Vec<u32> {
    version.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Parsing and unpacking for PECL's Windows build server.
mod pecl {
    use std::io::Read;
    use std::path::Path;

    pub const RELEASES: &str = "https://downloads.php.net/~windows/pecl/releases/";

    /// Every `href="..."` target in an Apache directory listing.
    fn hrefs(html: &str) -> impl Iterator<Item = &str> {
        html.split("href=\"").skip(1).filter_map(|rest| rest.split('"').next())
    }

    pub fn package_names(html: &str) -> Vec<String> {
        let mut names: Vec<String> = hrefs(html)
            .filter_map(|h| h.strip_suffix('/'))
            .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
            .map(|n| n.to_lowercase())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    /// Release folders, stable ones newest first, then pre-releases newest first.
    pub fn releases_newest_first(html: &str) -> Vec<String> {
        let key = |v: &str| -> Vec<u32> {
            v.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).map(|p| p.parse().unwrap_or(0)).collect()
        };
        let mut all: Vec<String> = hrefs(html)
            .filter_map(|h| h.strip_suffix('/'))
            .filter(|v| v.starts_with(|c: char| c.is_ascii_digit()))
            .map(str::to_string)
            .collect();
        all.sort_by(|a, b| {
            let stable = |v: &str| v.chars().all(|c| c.is_ascii_digit() || c == '.');
            stable(b).cmp(&stable(a)).then_with(|| key(b).cmp(&key(a)))
        });
        all.dedup();
        all
    }

    /// `php_redis-6.2.0-8.3-nts-vs16-x64.zip` for PHP 8.3 NTS; the compiler part varies.
    pub fn matching_zip(html: &str, name: &str, release: &str, minor: &str, ts: bool) -> Option<String> {
        let prefix = format!("php_{name}-{release}-{minor}-{}-", if ts { "ts" } else { "nts" }).to_lowercase();
        hrefs(html)
            .map(|h| h.rsplit('/').next().unwrap_or(h))
            .find(|f| f.to_lowercase().starts_with(&prefix) && f.to_lowercase().ends_with("-x64.zip"))
            .map(str::to_string)
    }

    /// Writes every DLL in the zip into `dest` (flattened): the extension itself plus any
    /// libraries it depends on. Fails when the extension's own DLL isn't in there.
    pub fn extract_dlls(zip: &[u8], dest: &Path, name: &str) -> Result<(), String> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| e.to_string())?;
        let wanted = format!("php_{name}.dll");
        let mut files = Vec::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            let path = entry.name().replace('\\', "/");
            let file = path.rsplit('/').next().unwrap_or_default().to_string();
            if !entry.is_file() || !file.to_lowercase().ends_with(".dll") {
                continue;
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            files.push((file, bytes));
        }
        if !files.iter().any(|(f, _)| f.eq_ignore_ascii_case(&wanted)) {
            return Err(format!("the download has no {wanted}"));
        }
        std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
        for (file, bytes) in files {
            // A running php-cgi holds its DLLs open; the caller restarts the pool after.
            std::fs::write(dest.join(&file), bytes).map_err(|e| format!("could not write {file}: {e} (stop the web server and retry)"))?;
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        const RELEASES_HTML: &str = r#"<a href="?C=N;O=D">Name</a><a href="/~windows/pecl/releases/">Parent</a>
            <a href="6.1.0/">6.1.0/</a><a href="6.10.0/">6.10.0/</a><a href="6.2.0RC1/">6.2.0RC1/</a><a href="logs/">logs/</a>"#;

        #[test]
        fn releases_sort_numerically_with_stable_first() {
            assert_eq!(releases_newest_first(RELEASES_HTML), vec!["6.10.0", "6.1.0", "6.2.0RC1"]);
        }

        #[test]
        fn picks_the_zip_for_this_php_minor_and_thread_safety() {
            let html = r#"<a href="php_redis-6.2.0-8.3-nts-vs16-x86.zip">
                <a href="php_redis-6.2.0-8.3-ts-vs16-x64.zip"><a href="php_redis-6.2.0-8.3-nts-vs16-x64.zip">
                <a href="php_redis-6.2.0-8.4-nts-vs17-x64.zip">"#;
            assert_eq!(matching_zip(html, "redis", "6.2.0", "8.3", false).as_deref(), Some("php_redis-6.2.0-8.3-nts-vs16-x64.zip"));
            assert_eq!(matching_zip(html, "redis", "6.2.0", "8.3", true).as_deref(), Some("php_redis-6.2.0-8.3-ts-vs16-x64.zip"));
            assert_eq!(matching_zip(html, "redis", "6.2.0", "8.5", false), None);
        }

        #[test]
        fn package_names_skip_sort_links_and_parents() {
            let html = r#"<a href="?C=N;O=D"><a href="/~windows/pecl/"><a href="apcu/"><a href="Xdebug/"><a href="redis/">"#;
            assert_eq!(package_names(html), vec!["apcu", "redis", "xdebug"]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_the_version_out_of_php_dash_v() {
        assert_eq!(parse_php_version("PHP 8.4.22 (cli) (built: Jul  1 2026) (NTS)\nCopyright").as_deref(), Some("8.4.22"));
        assert_eq!(parse_php_version("PHP 8.5.0-dev (cli)").as_deref(), Some("8.5.0-dev"));
        assert_eq!(parse_php_version("not php"), None);
    }

    #[test]
    fn scan_ignores_folders_without_php_cgi_and_survives_unrunnable_binaries() {
        let root = tempfile::tempdir().unwrap();
        let fake = root.path().join("php").join("php-8.1");
        std::fs::create_dir_all(&fake).unwrap();
        std::fs::write(fake.join("php.exe"), "").unwrap();
        std::fs::write(fake.join("php-cgi.exe"), "").unwrap();
        let cli_only = root.path().join("php").join("cli-only");
        std::fs::create_dir_all(&cli_only).unwrap();
        std::fs::write(cli_only.join("php.exe"), "").unwrap();
        // Empty stand-ins can't report a version, so nothing is registered and nothing panics.
        assert!(scan_folder(root.path()).is_empty());
    }

    #[test]
    fn pick_version_matches_minor_prefix_and_defaults_to_newest() {
        let installed = v(&["8.1.34", "8.4.26", "8.4.9"]);
        assert_eq!(pick_version(&installed, Some("8.1")).as_deref(), Some("8.1.34"));
        assert_eq!(pick_version(&installed, Some("8.4")).as_deref(), Some("8.4.26"), "8.4.26 > 8.4.9 numerically");
        assert_eq!(pick_version(&installed, Some("8.4.9")).as_deref(), Some("8.4.9"));
        assert_eq!(pick_version(&installed, None).as_deref(), Some("8.4.26"));
        assert_eq!(pick_version(&installed, Some("7.4")), None);
        assert_eq!(pick_version(&installed, Some("8.4.2")), None, "'8.4.2' must not prefix-match '8.4.26'");
    }

    #[test]
    fn pool_ids_and_ports_are_stable_and_distinct_per_minor_version() {
        assert_eq!(PhpPools::pool_id("8.1.34"), "php_81");
        let a = PhpPools::ports_for("8.1.34", 3);
        let b = PhpPools::ports_for("8.4.26", 3);
        assert_eq!(a, vec![10810, 10811, 10812]);
        assert!(a.iter().all(|p| !b.contains(p)));
    }
}
