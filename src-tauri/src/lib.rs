use std::sync::{Arc, Mutex};

use ols_core::{
    AppPaths, Core, CoreCommand, CoreResponse, Diagnostic, ProcessSupervisor, ProjectStore, RuntimeManager,
    SettingsService,
};
use tauri::Emitter;

struct CoreState(Mutex<Core>);

/// The single front door from the UI into the application core (architecture decision 1).
/// The UI never calls a manager directly — every action is a `CoreCommand` routed through here.
#[tauri::command]
fn run_command(
    command: CoreCommand,
    state: tauri::State<CoreState>,
) -> Result<CoreResponse, Diagnostic> {
    let mut core = state.0.lock().expect("core mutex poisoned");
    core.dispatch(command)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let paths = AppPaths::resolve();
    paths.ensure_dirs().expect("failed to create app directories");
    ols_core::logging::init(&paths.logs_dir());
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        home = %paths.root().display(),
        "OpenLocalServer starting"
    );

    let settings = SettingsService::load(&paths).expect("failed to load settings");
    let projects = ProjectStore::load(&paths).expect("failed to load project store");
    // Supervisor/runtimes held separately from `Core` (which also holds a clone of each)
    // so the setup hook below can subscribe to their events and forward them to the
    // webview independent of the `CoreState` mutex — the UI shouldn't have to poll.
    let supervisor = Arc::new(ProcessSupervisor::new());
    let runtimes = Arc::new(RuntimeManager::new(paths.clone()));
    let core = Core::with_managers(settings, supervisor.clone(), runtimes.clone(), projects);

    // ols_core::logging::init() above already installs the global tracing subscriber
    // (JSON, redacted, to disk — §118/§141). tauri-plugin-log would try to install a
    // second global logger and panic on startup, so it is intentionally not used here.
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(CoreState(Mutex::new(core)))
        .setup(move |app| {
            let process_handle = app.handle().clone();
            let mut process_events = supervisor.subscribe();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = process_events.recv().await {
                    let _ = process_handle.emit("process-event", &event);
                }
            });

            let runtime_handle = app.handle().clone();
            let mut runtime_events = runtimes.subscribe();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = runtime_events.recv().await {
                    let _ = runtime_handle.emit("runtime-event", &event);
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![run_command])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
