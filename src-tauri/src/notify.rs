//! Desktop alerts, raised by the app itself and wearing the app's own mark.
//!
//! This used to go through `tauri-plugin-notification`. On Windows that plugin hands the
//! toast to `notify-rust`, which falls back to PowerShell's AppUserModelID whenever the
//! notification carries none (`Toast::POWERSHELL_APP_ID`), and the plugin deliberately
//! leaves the id unset whenever the executable sits in `target\debug` or `target\release`.
//! Every alert therefore arrived titled "Windows PowerShell" with the PowerShell icon.
//!
//! The toast is built here instead. The identity is always the app's `identifier` from
//! `tauri.conf.json`, and `init` claims it for this process and registers the `HKCU`
//! entries Windows needs to resolve a name and an icon for it. The artwork is the same
//! 32x32 mark the tray and the window are painting, written to the app data directory
//! because Windows reads toast images from disk — and it is set as `appLogoOverride`,
//! the small corner mark, not as the large image slot, which turns every alert into a
//! banner.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

use windows::core::HSTRING;
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

/// Fallback identity, used when nothing has been registered yet. Must stay equal to
/// `identifier` in `tauri.conf.json`.
const APP_ID: &str = "dev.openlocalserver.app";

/// The two colourways the tray and the window already switch between.
const ICON_LIGHT: &[u8] = include_bytes!("../icons/32x32.png");
const ICON_DARK: &[u8] = include_bytes!("../icons/dark/32x32.png");

const ICON_FILE: &str = "notification-icon.png";

static REGISTERED: OnceLock<String> = OnceLock::new();

static ICON: OnceLock<PathBuf> = OnceLock::new();

/// Windows keys notifications by tag and group. Reusing either one makes a new alert a
/// *replacement* of the old one, and a replacement keeps the original group's timestamp --
/// so the banner grows a "9 days ago" header above the app name and the name stops sitting
/// at the left edge. Every alert therefore gets its own pair.
static ALERT_SEQ: AtomicU32 = AtomicU32::new(0);

fn identity() -> &'static str {
    REGISTERED.get().map(String::as_str).unwrap_or(APP_ID)
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Claims the notification identity for this process, writes the artwork for the current
/// theme into `data_dir` and registers both under
/// `HKCU\Software\Classes\AppUserModelId\<identifier>`. Runs once at startup. A failure
/// here costs a plainer toast, never a missing one, so nothing is escalated into an error.
pub fn init(identifier: &str, display_name: &str, data_dir: &Path, dark: bool) {
    let icon = set_theme(data_dir, dark);
    if REGISTERED.set(identifier.to_string()).is_err() {
        return;
    }
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::System::Registry::{
            RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_WRITE,
            REG_OPTION_NON_VOLATILE, REG_SZ,
        };
        use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

        // SAFETY: every pointer below is a NUL-terminated wide string owned by this
        // function and outliving its call, `key` is closed before returning, and the
        // byte slices are exactly as long as the values they describe.
        unsafe {
            let id = wide(identity());
            if SetCurrentProcessExplicitAppUserModelID(PCWSTR(id.as_ptr())).is_err() {
                tracing::warn!("the process could not claim the notification identity");
            }

            let subkey = wide(&format!(r"Software\Classes\AppUserModelId\{}", identity()));
            let mut key = HKEY::default();
            if RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut key,
                None,
            )
            .is_err()
            {
                tracing::warn!("the notification identity could not be registered");
                return;
            }

            let display = wide(display_name);
            let icon_uri = wide(&icon.to_string_lossy());
            for (name, value) in [("DisplayName", &display), ("IconUri", &icon_uri)] {
                let value_name = wide(name);
                // REG_SZ carries UTF-16 without the terminating null.
                let bytes = std::slice::from_raw_parts(
                    value.as_ptr().cast::<u8>(),
                    value.len().saturating_sub(1) * 2,
                );
                if RegSetValueExW(key, PCWSTR(value_name.as_ptr()), None, REG_SZ, Some(bytes))
                    .is_err()
                {
                    tracing::warn!(name, "a notification identity value could not be written");
                }
            }
            let _ = RegCloseKey(key);
        }
    }
    #[cfg(not(windows))]
    let _ = display_name;
}

/// Points the alerts at the mark for the current theme and returns its path. Called on
/// every theme flip, because the mark the tray paints changes with it.
pub fn set_theme(data_dir: &Path, dark: bool) -> PathBuf {
    let path = data_dir.join(ICON_FILE);
    let bytes = if dark { ICON_DARK } else { ICON_LIGHT };
    match std::fs::write(&path, bytes) {
        Ok(()) => {
            let _ = ICON.set(path.clone());
        }
        Err(e) => tracing::warn!(error = %e, "the notification icon could not be written"),
    }
    path
}

/// Raises one alert. Off the calling thread: building a Windows toast talks to WinRT and
/// can take a moment, and the callers are the tray and supervisor threads.
pub fn show(title: &str, body: &str) {
    let title = title.to_string();
    let body = body.to_string();
    let app_id = identity().to_string();
    let icon = ICON.get().cloned();
    std::thread::spawn(move || {
        let seq = ALERT_SEQ.fetch_add(1, Ordering::Relaxed);
        let image = match icon {
            // Windows reads toast images from disk, as a file URI. Backslashes are not
            // valid in one, and a raw path silently drops the artwork.
            Some(icon) => format!(
                r#"<image placement="appLogoOverride" src="file:///{}" alt="OLS" />"#,
                icon.to_string_lossy()
                    .replace('\\', "/")
                    .trim_start_matches("file:///")
            ),
            None => String::new(),
        };
        let xml = format!(
            concat!(
                r#"<toast Tag="ols{0}" Group="ols{0}" Duration="short">"#,
                r#"<visual><binding template="ToastGeneric">"#,
                r#"<text>{1}</text><text>{2}</text>{3}"#,
                "</binding></visual>",
                r#"<audio src="ms-winsoundevent:Notification.Default" />"#,
                "</toast>"
            ),
            seq,
            escape(&title),
            escape(&body),
            image
        );
        if let Err(e) = present(&app_id, &xml) {
            tracing::warn!(error = %e, "the desktop alert could not be raised");
        }
    });
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn present(app_id: &str, xml: &str) -> Result<(), String> {
    let document = XmlDocument::new().map_err(|e| e.to_string())?;
    document
        .LoadXml(&HSTRING::from(xml))
        .map_err(|e| e.to_string())?;
    let toast = ToastNotification::CreateToastNotification(&document).map_err(|e| e.to_string())?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))
        .and_then(|notifier| notifier.Show(&toast))
        .map_err(|e| e.to_string())
}
