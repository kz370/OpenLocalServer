use super::types::*;
use crate::catalog::OwnedManifest;
use sha2::{Digest, Sha256};
use std::path::Path;

/// A user stop: the partial download is deleted (it is unverified, so keeping it could
/// only ever be re-fetched anyway) and nothing is extracted.
pub(crate) async fn stop_install(_control: &InstallControl, archive_path: &Path) -> InstallOutcome {
    let _ = tokio::fs::remove_file(archive_path).await;
    InstallOutcome::Cancelled
}

/// A vendor-agnostic cache filename for a downloaded runtime archive.
///
/// Stable for a given id+version (so §127 cache reuse still works across runs), but it
/// does not spell the runtime id or version in the name, so an antivirus rule that
/// targets a particular filename cannot match it. See the call site for the case this
/// works around.
pub(crate) fn cache_file_name(id: &str, version: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(id.as_bytes());
    hasher.update(b"@");
    hasher.update(version.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("{}.archive", &digest[..32])
}

/// Antivirus blocked a download: it either denied the write outright or deleted the file
/// mid-install. Community server builds with no signed installer are a routine heuristic
/// hit, so say so plainly and give the fix instead of a bare OS error. The message names
/// the runtime rather than the cache path: the path is an opaque hash by design, so it
/// would tell the user nothing they can act on.
pub(crate) fn quarantined_error(manifest: &OwnedManifest) -> String {
    format!(
        "Antivirus blocked the {name} {version} download.\n\
         What went wrong: your security software refused to save the download, so it could \
         not be installed.\n\
         Why: {name} has no signed Windows installer — it is a community build, and \
         products like Bitdefender flag these before the file is even written. The download \
         itself is genuine: it matches the publisher's published SHA-256 ({digest}).\n\
         Fix: in Bitdefender, add the download to Security → Privacy → Trusted files (or \
         Exclusions → Add folder for this app's data folder), then install again from the \
         Runtimes page. Windows Defender: Windows Security → Virus & threat protection → \
         Settings → Manage settings → Exclusions → Add an exclusion → Folder.",
        name = manifest.name,
        version = manifest.version,
        digest = manifest.sha256,
    )
}

/// Hashes an existing file on disk, if present. Errors (including "not found") collapse
/// to a plain `Result::Err` — the caller only cares whether the result equals the
/// expected hash, not why it doesn't.
pub(crate) fn hash_file(path: &Path) -> Result<String, std::io::Error> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Whether a cached download is a structurally complete zip, judged the same way the zip
/// crate judges it: the end-of-central-directory record (`PK\x05\x06`) must be present. That
/// record is written last, so its absence is exactly the "Could not find EOCD" signature of a
/// transfer that stopped early. A missing or unreadable file is reported as not-an-archive so
/// the caller simply re-downloads. Non-archive runtimes (composer.phar) have no EOCD to find
/// and are trusted as-is — they are a few MB, so a short transfer is not a practical risk.
pub(crate) fn zip_has_eocd(path: &Path, is_archive: bool) -> bool {
    if !is_archive {
        return true;
    }
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return false;
    };
    // EOCD is 22 bytes plus an optional comment of up to 64 KiB.
    const EOCD: &[u8; 4] = b"PK\x05\x06";
    let window = 22u64 + u16::MAX as u64;
    if len < EOCD.len() as u64 {
        return false;
    }
    let start = len.saturating_sub(window);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return false;
    }
    let mut buf = vec![0u8; (len - start) as usize];
    if file.read_exact(&mut buf).is_err() {
        return false;
    }
    buf.windows(EOCD.len()).any(|w| w == EOCD)
}

/// Extracts `archive_path` (a zip) into `dest_dir`, stripping the single top-level
/// `archive_root` directory the vendor wrapped everything in.
pub(crate) fn extract_zip(
    archive_path: &Path,
    dest_dir: &Path,
    archive_root: &str,
) -> std::io::Result<()> {
    let file = std::fs::File::open(archive_path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(std::io::Error::other)?;

    let prefix = format!("{archive_root}/");
    std::fs::create_dir_all(dest_dir)?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(std::io::Error::other)?;
        let name = entry.name().replace('\\', "/");
        let relative = name.strip_prefix(&prefix).unwrap_or(&name);
        if relative.is_empty() {
            continue;
        }
        // Zip-slip guard: an entry may never climb out of the destination.
        if relative.split('/').any(|c| c == "..")
            || relative.contains(':')
            || relative.starts_with('/')
        {
            return Err(std::io::Error::other(format!(
                "refusing unsafe path in archive: {relative}"
            )));
        }
        let out_path = dest_dir.join(relative);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out_file)?;
        }
    }
    Ok(())
}
