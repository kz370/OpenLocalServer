//! Desktop alerts, raised by the app itself and wearing the app's own mark.
//!
//! This used to go through `tauri-plugin-notification`. On Windows that plugin hands the
//! toast to `notify-rust`, which falls back to PowerShell's AppUserModelID whenever the
//! notification carries none (`Toast::POWERSHELL_APP_ID`), and the plugin deliberately
//! leaves the id unset whenever the executable sits in `target\debug` or `target\release`.
//! Every alert therefore arrived titled "Windows PowerShell" with the PowerShell icon.
//!
//! The toast is built here instead. The identity is always the app's `identifier` from
//! `tauri.conf.json`, and `init` claims it for this process, registers the `HKCU` entries
//! Windows needs to resolve a name and an icon for it, and gives it the Start Menu shortcut
//! an installed app gets from its installer — without that shortcut the shell treats every
//! alert as a notification *group*, which is what puts a timestamp header above the app name
//! and centres the name instead of leaving it at the left edge. The artwork is the same
//! 32x32 mark the tray and the window are painting, written to the app data directory
//! because Windows reads toast images from disk — and it is set as `appLogoOverride`,
//! the small corner mark, not as the large image slot, which turns every alert into a
//! banner.

use std::path::{Path, PathBuf};
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

fn identity() -> &'static str {
    REGISTERED.get().map(String::as_str).unwrap_or(APP_ID)
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Claims the notification identity for this process, writes the artwork for the current
/// theme into `data_dir` and registers both under
/// `HKCU\Software\Classes\AppUserModelId\<identifier>`, plus the Start Menu shortcut an
/// installed app gets from its installer. Runs once at startup. A failure here costs a
/// plainer toast, never a missing one, so nothing is escalated into an error.
pub fn init(identifier: &str, display_name: &str, data_dir: &Path, dark: bool) {
    let icon = set_theme(data_dir, dark);
    if REGISTERED.set(identifier.to_string()).is_err() {
        return;
    }
    #[cfg(windows)]
    {
        install_shortcut(identifier, display_name);
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

/// Puts a Start Menu entry for this executable in front of the shell, carrying the same
/// AppUserModelID. An installed app has one from its installer; a development build has
/// nothing, and a toast whose identity has no shortcut is not treated as a normal app's —
/// it arrives as a notification group, which is what drew a timestamp header above the
/// app name and centred the name instead of leaving it at the left edge.
///
/// On its own thread because COM wants an apartment per thread and the caller is the main
/// one. Rewritten every launch, since the executable that built the last one is often gone
/// after a rebuild.
#[cfg(windows)]
fn install_shortcut(identifier: &str, display_name: &str) {
    use windows::core::{Interface, GUID};
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{
        FOLDERID_Programs, IShellLinkW, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
    };

    const SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_C000_000000000046);
    /// `System.AppUserModel.ID`, which is how a shortcut claims an identity.
    const APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3),
        pid: 5,
    };

    let (identifier, display_name) = (identifier.to_string(), display_name.to_string());
    std::thread::spawn(move || {
        // SAFETY: COM is initialised on this thread and never uninitialised, which is what
        // an apartment thread is for. `link` is released with the thread's COM scope, and
        // the folder path from SHGetKnownFolderPath is freed explicitly below.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let exe = match std::env::current_exe() {
                Ok(exe) => exe,
                Err(e) => {
                    return tracing::warn!(error = %e, "the alert shortcut needs an executable path")
                }
            };
            let folder = match SHGetKnownFolderPath(&FOLDERID_Programs, KF_FLAG_DEFAULT, None) {
                Ok(folder) => folder,
                Err(e) => {
                    return tracing::warn!(error = %e, "the Start Menu folder could not be read")
                }
            };
            let link = PathBuf::from(String::from_utf16_lossy(folder.as_wide()));
            CoTaskMemFree(Some(folder.0 as *const std::ffi::c_void));
            let target = link.join(format!("{display_name}.lnk"));

            let shell_link: IShellLinkW = match CoCreateInstance(
                &SHELL_LINK,
                None::<&windows::core::IUnknown>,
                CLSCTX_INPROC_SERVER,
            ) {
                Ok(shell_link) => shell_link,
                Err(e) => {
                    return tracing::warn!(error = %e, "the alert shortcut could not be created")
                }
            };
            let written = shell_link
                .SetPath(&HSTRING::from(exe.to_string_lossy().as_ref()))
                .and_then(|()| {
                    let store: IPropertyStore = shell_link.cast()?;
                    store.SetValue(&APP_USER_MODEL_ID, &PROPVARIANT::from(identifier.as_str()))
                })
                .and_then(|()| {
                    let persist: IPersistFile = shell_link.cast()?;
                    persist.Save(&HSTRING::from(target.to_string_lossy().as_ref()), true)
                });
            if let Err(e) = written {
                tracing::warn!(error = %e, "the alert shortcut could not be saved");
            }
        }
    });
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
///
/// The toast carries no `<image>`. The mark beside the message came from
/// `placement="appLogoOverride"`, and Windows draws that slot inline with the text as well
/// as the app's own mark in the header — the app looked like it had two logos, the second
/// one large. The header icon is read from the registered identity instead, which is the
/// same file, so dropping the image loses nothing.
pub fn show(title: &str, body: &str) {
    let title = title.to_string();
    let body = body.to_string();
    let app_id = identity().to_string();
    std::thread::spawn(move || {
        let xml = format!(
            concat!(
                r#"<toast Duration="short">"#,
                r#"<visual><binding template="ToastGeneric">"#,
                r#"<text>{0}</text><text>{1}</text>"#,
                "</binding></visual>",
                r#"<audio src="ms-winsoundevent:Notification.Default" />"#,
                "</toast>"
            ),
            escape(&title),
            escape(&body)
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
