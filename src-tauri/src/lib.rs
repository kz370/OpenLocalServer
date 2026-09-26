use std::sync::Arc;
use std::time::Duration;

use ols_core::process::{ProcessEvent, ProcessState};
use ols_core::runtime::RuntimeEvent;
use ols_core::{AppPaths, Core, CoreCommand, CoreResponse, Diagnostic, ProcessSupervisor, RuntimeManager, SettingsService};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_notification::NotificationExt;

/// The single front door from the UI into the application core (architecture decision 1).
/// The UI never calls a manager directly — every action is a `CoreCommand` routed through here.
///
/// Commands run on a blocking thread: applying the web config or installing a runtime can
/// take seconds, and must never freeze the window.
#[tauri::command]
async fn run_command(command: CoreCommand, state: tauri::State<'_, Core>) -> Result<CoreResponse, Diagnostic> {
    let core = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core.dispatch(command))
        .await
        .map_err(|e| Diagnostic { problem: "The command crashed.".into(), cause: e.to_string(), fix: None })?
}

fn notifications_enabled(core: &Core) -> bool {
    core.inner().setting_bool("notifications.enabled", true)
}

/// §119: a native notification, unless the user turned them off.
fn notify(app: &AppHandle, core: &Core, title: &str, body: &str) {
    if !notifications_enabled(core) {
        return;
    }
    let _ = app.notification().builder().title(title).body(body).show();
}

fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Stops everything DevForge started so nothing is left running after "Quit".
fn shutdown(core: &Core) {
    core.inner().terminals.close_all();
    core.inner().web.stop();
    for s in core.services().list() {
        if s.running {
            core.services().stop(&s.id);
        }
    }
}

fn build_tray(app: &AppHandle, core: Core) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open OpenLocalServer", true, None::<&str>)?;
    let apply = MenuItem::with_id(app, "web_start", "Start / apply web server", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "web_stop", "Stop web server", true, None::<&str>)?;
    let mailpit = MenuItem::with_id(app, "mailpit", "Open Mailpit", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &sep1, &apply, &stop, &mailpit, &sep2, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("OpenLocalServer")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let core = core.clone();
            match event.id.as_ref() {
                "open" => show_main_window(app),
                "web_start" => {
                    let handle = app.clone();
                    std::thread::spawn(move || match core.inner().apply_web(&[]) {
                        Ok(_) => notify(&handle, &core, "Web server", "Sites are up to date."),
                        Err(e) => notify(&handle, &core, "Web server failed", &e.to_string()),
                    });
                }
                "web_stop" => {
                    core.inner().web.stop();
                }
                "mailpit" => {
                    let _ = core.inner().open_url("http://127.0.0.1:8025");
                }
                "quit" => {
                    shutdown(&core);
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let paths = AppPaths::resolve();
    paths.ensure_dirs().expect("failed to create app directories");
    let migration_notes = paths.migrate_legacy();
    ols_core::logging::init(&paths.logs_dir());
    for note in &migration_notes {
        tracing::info!("{note}");
    }
    tracing::info!(version = env!("CARGO_PKG_VERSION"), home = %paths.root().display(), "OpenLocalServer starting");

    let settings = SettingsService::load(&paths).expect("failed to load settings");
    // Supervisor/runtimes are shared with `Core` so the setup hook below can subscribe to
    // their events and forward them to the webview — the UI shouldn't have to poll.
    let supervisor = Arc::new(ProcessSupervisor::new());
    let runtimes = Arc::new(RuntimeManager::new(paths.clone()));
    let core = Core::with_parts(settings, paths, supervisor.clone(), runtimes.clone());
    let start_hidden = std::env::args().any(|a| a == "--minimized") && core.inner().setting_bool("startup.minimized", true);

    let setup_core = core.clone();
    let window_core = core.clone();
    // ols_core::logging::init() above already installs the global tracing subscriber
    // (JSON, redacted, to disk — §118/§141). tauri-plugin-log would try to install a
    // second global logger and panic on startup, so it is intentionally not used here.
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(core)
        .setup(move |app| {
            let core = setup_core;

            let process_handle = app.handle().clone();
            let process_core = core.clone();
            let mut process_events = supervisor.subscribe();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = process_events.recv().await {
                    let _ = process_handle.emit("process-event", &event);
                    // §119: tell the user when something they rely on dies.
                    if let ProcessEvent::StateChanged { id, state: ProcessState::Crashed | ProcessState::Failed } = &event {
                        let name = process_core.supervisor().snapshot().into_iter().find(|p| p.id == *id).map(|p| p.name).unwrap_or_default();
                        notify(&process_handle, &process_core, "A process stopped unexpectedly", &name);
                    }
                }
            });

            // Terminal output goes straight to the webview (xterm.js draws it).
            let terminal_handle = app.handle().clone();
            let mut terminal_events = core.inner().terminals.subscribe();
            tauri::async_runtime::spawn(async move {
                while let Some(event) = ols_core::terminal::next_event(&mut terminal_events).await {
                    let _ = terminal_handle.emit("terminal-event", &event);
                }
            });

            let runtime_handle = app.handle().clone();
            let runtime_core = core.clone();
            let mut runtime_events = runtimes.subscribe();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = runtime_events.recv().await {
                    let _ = runtime_handle.emit("runtime-event", &event);
                    match &event {
                        RuntimeEvent::Installed { id, version, .. } => notify(&runtime_handle, &runtime_core, "Installed", &format!("{id} {version} is ready")),
                        RuntimeEvent::Failed { id, version, message } => notify(&runtime_handle, &runtime_core, "Install failed", &format!("{id} {version}: {message}")),
                        _ => {}
                    }
                }
            });

            // Quick App runs: notify once when each finishes.
            let run_handle = app.handle().clone();
            let run_core = core.clone();
            std::thread::spawn(move || {
                let mut announced = std::collections::HashSet::new();
                loop {
                    std::thread::sleep(Duration::from_secs(2));
                    for run in run_core.inner().runs.list() {
                        if run.finished_ms.is_some() && announced.insert(run.id.clone()) {
                            let (title, body) = match run.error {
                                None => (format!("{} is ready", run.app_name), run.open_url.clone().unwrap_or_default()),
                                Some(e) => (format!("{} failed", run.app_name), e),
                            };
                            notify(&run_handle, &run_core, &title, &body);
                        }
                    }
                }
            });

            build_tray(app.handle(), core.clone())?;

            if start_hidden {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            // §121: start what the user asked to have started with the app.
            let autostart_core = core.clone();
            std::thread::spawn(move || autostart_core.inner().run_autostart());
            Ok(())
        })
        .on_window_event(move |window, event| {
            // §120: closing the window keeps the servers running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window_core.inner().setting_bool("startup.close_to_tray", true) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![run_command])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
