use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ols_core::process::{ProcessEvent, ProcessState};
use ols_core::runtime::RuntimeEvent;
use ols_core::{
    AppPaths, Core, CoreCommand, CoreResponse, Diagnostic, ProcessSupervisor, RuntimeManager,
    SettingsService,
};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent, Wry};
use tauri_plugin_notification::NotificationExt;

/// The single front door from the UI into the application core (architecture decision 1).
/// The UI never calls a manager directly — every action is a `CoreCommand` routed through here.
///
/// Commands run on a blocking thread: applying the web config or installing a runtime can
/// take seconds, and must never freeze the window.
#[tauri::command]
async fn run_command(
    command: CoreCommand,
    state: tauri::State<'_, Core>,
    app: AppHandle,
) -> Result<CoreResponse, Diagnostic> {
    let core = state.inner().clone();
    let dispatched = tauri::async_runtime::spawn_blocking(move || core.dispatch(command))
        .await
        .map_err(|e| Diagnostic {
            problem: "The command crashed.".into(),
            cause: e.to_string(),
            fix: None,
        })?;
    // Any command can start or stop things, so the tray, taskbar and in-app mark
    // are refreshed after every one of them rather than only the tray menu ones.
    sync_status_icon(&app, &state.inner().clone());
    dispatched
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

static SHUTDOWN_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// The green mark is the app's official icon; the red one is the same art in the
/// "nothing is running" colour. Both are shipped next to the bundle icons so the
/// tray, the taskbar and the window can switch between them at runtime.
const TRAY_ICON_GREEN: &[u8] = include_bytes!("../icons/32x32.png");
const TRAY_ICON_RED: &[u8] = include_bytes!("../icons/red/32x32.png");
const WINDOW_ICON_GREEN: &[u8] = include_bytes!("../icons/128x128.png");
const WINDOW_ICON_RED: &[u8] = include_bytes!("../icons/red/128x128.png");

/// What the tray and window currently show, so a state change only repaints once.
static ICON_SHOWS_RED: Mutex<Option<bool>> = Mutex::new(None);

fn decode_icon(bytes: &[u8]) -> Option<tauri::image::Image<'static>> {
    tauri::image::Image::from_bytes(bytes).ok()
}

/// Paints the status mark on the tray icon, the main window (taskbar) and, through
/// `ols:status-icon`, the mark inside the app. `force` repaints even when the mark
/// is already showing.
fn apply_status_icon(app: &AppHandle, red: bool, force: bool) {
    {
        let mut current = ICON_SHOWS_RED.lock().unwrap_or_else(|e| e.into_inner());
        if !force && *current == Some(red) {
            return;
        }
        *current = Some(red);
    }
    let (tray_bytes, window_bytes) = if red {
        (TRAY_ICON_RED, WINDOW_ICON_RED)
    } else {
        (TRAY_ICON_GREEN, WINDOW_ICON_GREEN)
    };
    if let Some(icon) = decode_icon(tray_bytes) {
        if let Some(tray) = app.tray_by_id("main") {
            let _ = tray.set_icon(Some(icon));
        }
    }
    if let Some(icon) = decode_icon(window_bytes) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.set_icon(icon);
        }
    }
    let _ = app.emit("ols:status-icon", red);
}

/// The mark is green while at least one managed process is running — a service or
/// a supervised process — and red once everything is stopped, so the tray, the
/// taskbar and the in-app mark always state what the app is doing.
fn sync_status_icon(app: &AppHandle, core: &Core) {
    let service_running = core.services().list().iter().any(|s| s.running);
    let process_running = core
        .supervisor()
        .snapshot()
        .iter()
        .any(|p| p.state == ProcessState::Running || p.state == ProcessState::Starting);
    apply_status_icon(app, !(service_running || process_running), false);
}

fn set_stopping_tray(app: &AppHandle) {
    apply_status_icon(app, true, true);
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some("OpenLocalServer is stopping"));
    }
}

/// Stops everything OpenLocalServer started and waits before allowing the app to exit.
fn shutdown(core: &Core) -> bool {
    ols_core::control::close(&core.inner().paths);
    core.inner().stop_all_tunnels();
    core.inner().stop_all_workers();
    core.inner().terminals.close_all();
    core.inner().web.stop();
    for s in core.services().list() {
        if s.running {
            core.services().stop(&s.id);
        }
    }
    core.supervisor().stop_all_and_wait(Duration::from_secs(30))
}

fn begin_shutdown(app: &AppHandle, core: Core) {
    if SHUTDOWN_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        return;
    }
    set_stopping_tray(app);
    let app = app.clone();
    std::thread::spawn(move || {
        let mut notified = false;
        loop {
            if shutdown(&core) {
                app.exit(0);
                return;
            }
            if !notified {
                notify(
                    &app,
                    &core,
                    "Still stopping",
                    "OpenLocalServer will keep waiting and exit when its managed processes stop.",
                );
                notified = true;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

fn navigate(app: &AppHandle, route: &str) {
    show_main_window(app);
    let _ = app.emit("ols:navigate", route);
}

fn tray_menu(app: &AppHandle, core: &Core) -> tauri::Result<Menu<Wry>> {
    let open = MenuItem::with_id(app, "open", "Open OpenLocalServer", true, None::<&str>)?;
    let start_all = MenuItem::with_id(app, "start_all", "Start all", true, None::<&str>)?;
    let stop_all = MenuItem::with_id(app, "stop_all", "Stop all", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;

    let mut site_items: Vec<Box<dyn tauri::menu::IsMenuItem<Wry>>> = Vec::new();
    for site in core.inner().domains.lock().unwrap().list() {
        site_items.push(Box::new(MenuItem::with_id(
            app,
            &format!("site:{}", site.hostname),
            &site.hostname,
            true,
            None::<&str>,
        )?));
    }
    site_items.push(Box::new(MenuItem::with_id(
        app,
        "sites_folder",
        "Open sites folder",
        true,
        None::<&str>,
    )?));
    site_items.push(Box::new(MenuItem::with_id(
        app,
        "site_add",
        "Add site...",
        true,
        None::<&str>,
    )?));
    let site_refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> =
        site_items.iter().map(|item| item.as_ref()).collect();
    let sites = Submenu::with_items(app, "Sites", true, &site_refs)?;

    let web_start = MenuItem::with_id(app, "web_start", "Start / reload", true, None::<&str>)?;
    let web_stop = MenuItem::with_id(app, "web_stop", "Stop", true, None::<&str>)?;
    let active_server = core.inner().web_config().server;
    let mut server_items = Vec::new();
    for server in ["nginx", "apache", "caddy"] {
        server_items.push(CheckMenuItem::with_id(
            app,
            &format!("server:{server}"),
            server,
            true,
            active_server == server,
            None::<&str>,
        )?);
    }
    let server_refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = server_items
        .iter()
        .map(|item| item as &dyn tauri::menu::IsMenuItem<Wry>)
        .collect();
    let server_switch = Submenu::with_items(app, "Server", true, &server_refs)?;
    let web_config =
        MenuItem::with_id(app, "web_config", "Open config folder", true, None::<&str>)?;
    let web = Submenu::with_items(
        app,
        "Web server",
        true,
        &[&web_start, &web_stop, &server_switch, &web_config],
    )?;

    let php_versions = core.inner().runtimes.installed_versions("php");
    let php = MenuItem::with_id(
        app,
        "php_page",
        if php_versions.is_empty() {
            "PHP (none installed)"
        } else {
            "PHP"
        },
        true,
        None::<&str>,
    )?;

    let mut database_items: Vec<Box<dyn tauri::menu::IsMenuItem<Wry>>> = Vec::new();
    for service in core
        .services()
        .list()
        .into_iter()
        .filter(|service| service.kind == "sql" || service.kind == "document")
    {
        let label = format!(
            "{} ({})",
            service.name,
            if service.running { "stop" } else { "start" }
        );
        database_items.push(Box::new(MenuItem::with_id(
            app,
            &format!("db:{}", service.id),
            label,
            true,
            None::<&str>,
        )?));
    }
    database_items.push(Box::new(MenuItem::with_id(
        app,
        "databases_page",
        "Open databases",
        true,
        None::<&str>,
    )?));
    let database_refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> =
        database_items.iter().map(|item| item.as_ref()).collect();
    let databases = Submenu::with_items(app, "Databases", true, &database_refs)?;

    let mut service_items: Vec<Box<dyn tauri::menu::IsMenuItem<Wry>>> = Vec::new();
    for service in core
        .services()
        .list()
        .into_iter()
        .filter(|service| service.kind == "mail" || service.kind == "cache")
    {
        let label = format!(
            "{} ({})",
            service.name,
            if service.running { "stop" } else { "start" }
        );
        service_items.push(Box::new(MenuItem::with_id(
            app,
            &format!("service:{}", service.id),
            label,
            true,
            None::<&str>,
        )?));
    }
    service_items.push(Box::new(MenuItem::with_id(
        app,
        "mailpit",
        "Open Mailpit",
        true,
        None::<&str>,
    )?));
    let service_refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> =
        service_items.iter().map(|item| item.as_ref()).collect();
    let services = Submenu::with_items(app, "Services", true, &service_refs)?;

    let quick_apps = MenuItem::with_id(app, "quick_apps", "Recipes", true, None::<&str>)?;
    let quick = Submenu::with_items(app, "Quick App", true, &[&quick_apps])?;
    let terminal = MenuItem::with_id(app, "terminal", "Terminal", true, None::<&str>)?;
    let doctor = MenuItem::with_id(app, "doctor", "Run doctor", true, None::<&str>)?;
    let data = MenuItem::with_id(app, "data_folder", "Data folder", true, None::<&str>)?;
    let tools = Submenu::with_items(app, "Tools", true, &[&terminal, &doctor, &data])?;
    let preferences = MenuItem::with_id(app, "preferences", "Preferences...", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &open,
            &start_all,
            &stop_all,
            &separator,
            &sites,
            &web,
            &php,
            &databases,
            &services,
            &quick,
            &tools,
            &preferences,
            &separator,
            &quit,
        ],
    )
}

fn refresh_tray(app: &AppHandle, core: &Core) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("main"), tray_menu(app, core)) {
        let _ = tray.set_menu(Some(menu));
    }
    sync_status_icon(app, core);
}

fn build_tray(app: &AppHandle, core: Core) -> tauri::Result<()> {
    let menu = tray_menu(app, &core)?;
    let menu_core = core.clone();
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("OpenLocalServer")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let core = core.clone();
            match event.id.as_ref() {
                "open" => show_main_window(app),
                "start_all" | "web_start" => {
                    let handle = app.clone();
                    std::thread::spawn(move || {
                        match core.inner().apply_web(&[]) {
                            Ok(_) => notify(&handle, &core, "Web server", "Sites are up to date."),
                            Err(e) => notify(&handle, &core, "Web server failed", &e.to_string()),
                        }
                        if event.id.as_ref() == "start_all" {
                            for service in core
                                .services()
                                .list()
                                .into_iter()
                                .filter(|service| !service.running)
                            {
                                let _ = core.services().start(&service.id);
                            }
                        }
                        refresh_tray(&handle, &core);
                    });
                }
                "stop_all" => {
                    core.inner().web.stop();
                    for service in core
                        .services()
                        .list()
                        .into_iter()
                        .filter(|service| service.running)
                    {
                        core.services().stop(&service.id);
                    }
                    refresh_tray(app, &core);
                }
                "web_stop" => {
                    core.inner().web.stop();
                    refresh_tray(app, &core);
                }
                "mailpit" => {
                    let _ = core.inner().open_url("http://127.0.0.1:8025");
                }
                "sites_folder" => {
                    let _ = core.inner().open_path(
                        &core
                            .inner()
                            .paths
                            .root()
                            .join("sites")
                            .display()
                            .to_string(),
                    );
                }
                "web_config" => {
                    let _ = core
                        .inner()
                        .open_path(&core.inner().paths.web_dir().display().to_string());
                }
                "data_folder" => {
                    let _ = core
                        .inner()
                        .open_path(&core.inner().paths.data_dir().display().to_string());
                }
                "preferences" => navigate(app, "settings"),
                "databases_page" => navigate(app, "databases"),
                "php_page" => navigate(app, "runtimes"),
                "quick_apps" => navigate(app, "quick-apps"),
                "terminal" => navigate(app, "terminal"),
                "doctor" => navigate(app, "diagnostics"),
                "site_add" => navigate(app, "sites"),
                id if id.starts_with("site:") => {
                    if let Some(site) = core.inner().domains.lock().unwrap().get(&id[5..]) {
                        let _ = core.inner().open_url(&format!(
                            "{}://{}",
                            if site.https { "https" } else { "http" },
                            site.hostname
                        ));
                    }
                }
                id if id.starts_with("service:") || id.starts_with("db:") => {
                    let service_id = id
                        .split_once(':')
                        .map(|(_, value)| value)
                        .unwrap_or_default();
                    if core
                        .services()
                        .list()
                        .into_iter()
                        .find(|service| service.id == service_id)
                        .map(|service| service.running)
                        .unwrap_or(false)
                    {
                        core.services().stop(service_id);
                    } else {
                        let _ = core.services().start(service_id);
                    }
                    refresh_tray(app, &core);
                }
                id if id.starts_with("server:") => {
                    let _ = core.dispatch(CoreCommand::SetSetting {
                        key: "web.server".into(),
                        value: serde_json::Value::String(id[7..].into()),
                    });
                    refresh_tray(app, &core);
                }
                "quit" => begin_shutdown(app, core.clone()),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    sync_status_icon(app, &menu_core);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let paths = AppPaths::resolve();
    paths
        .ensure_dirs()
        .expect("failed to create app directories");
    let migration_notes = paths.migrate_legacy();
    ols_core::logging::init(&paths.logs_dir());
    for note in &migration_notes {
        tracing::info!("{note}");
    }
    tracing::info!(version = env!("CARGO_PKG_VERSION"), home = %paths.root().display(), "OpenLocalServer starting");

    // One owner of the core at a time: a background `ols daemon` hands over to the app.
    ols_core::control::take_over_from_daemon(&paths);
    let settings = SettingsService::load(&paths).expect("failed to load settings");
    // Supervisor/runtimes are shared with `Core` so the setup hook below can subscribe to
    // their events and forward them to the webview — the UI shouldn't have to poll.
    let supervisor = Arc::new(ProcessSupervisor::new());
    let runtimes = Arc::new(RuntimeManager::new(paths.clone()));
    let core = Core::with_parts(
        settings,
        paths.clone(),
        supervisor.clone(),
        runtimes.clone(),
    );
    // §136: the `ols` command line talks to the app through this.
    if let Err(e) = ols_core::control::serve(core.clone(), &paths, "app") {
        tracing::warn!(error = %e, "the command-line control channel is not available");
    }
    let start_hidden = std::env::args().any(|a| a == "--minimized")
        && core.inner().setting_bool("startup.minimized", true);

    let setup_core = core.clone();
    let window_core = core.clone();
    // ols_core::logging::init() above already installs the global tracing subscriber
    // (JSON, redacted, to disk — §118/§141). tauri-plugin-log would try to install a
    // second global logger and panic on startup, so it is intentionally not used here.
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
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
                    // A service that dies on its own changes what the tray and
                    // taskbar mark should say.
                    sync_status_icon(&process_handle, &process_core);
                    // §119: tell the user when something they rely on dies.
                    if let ProcessEvent::StateChanged {
                        id,
                        state: ProcessState::Crashed | ProcessState::Failed,
                    } = &event
                    {
                        let name = process_core
                            .supervisor()
                            .snapshot()
                            .into_iter()
                            .find(|p| p.id == *id)
                            .map(|p| p.name)
                            .unwrap_or_default();
                        notify(
                            &process_handle,
                            &process_core,
                            "A process stopped unexpectedly",
                            &name,
                        );
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
                        RuntimeEvent::Installed { id, version, .. } => notify(
                            &runtime_handle,
                            &runtime_core,
                            "Installed",
                            &format!("{id} {version} is ready"),
                        ),
                        RuntimeEvent::Failed {
                            id,
                            version,
                            message,
                        } => notify(
                            &runtime_handle,
                            &runtime_core,
                            "Install failed",
                            &format!("{id} {version}: {message}"),
                        ),
                        _ => {}
                    }
                }
            });

            let projects_handle = app.handle().clone();
            let mut project_events = core.inner().subscribe_project_events();
            tauri::async_runtime::spawn(async move {
                while project_events.recv().await.is_ok() {
                    let _ = projects_handle.emit("projects-changed", ());
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
                                None => (
                                    format!("{} is ready", run.app_name),
                                    run.open_url.clone().unwrap_or_default(),
                                ),
                                Some(e) => (format!("{} failed", run.app_name), e),
                            };
                            notify(&run_handle, &run_core, &title, &body);
                        }
                    }
                }
            });

            build_tray(app.handle(), core.clone())?;
            // §106: scheduled tasks run while the app is open.
            ols_core::scheduler::start_clock(core.inner());

            if start_hidden {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            // §121: start what the user asked to have started with the app.
            let autostart_core = core.clone();
            let auto_fix_app = app.handle().clone();
            std::thread::spawn(move || {
                autostart_core.inner().run_autostart();
                sync_status_icon(&auto_fix_app, &autostart_core);
                loop {
                    for result in autostart_core.auto_fix_diagnostics() {
                        if result.ok {
                            notify(
                                &auto_fix_app,
                                &autostart_core,
                                "Diagnostics",
                                &format!("Fixed: {}", result.problem),
                            );
                        } else {
                            notify(
                                &auto_fix_app,
                                &autostart_core,
                                "Automatic fix failed",
                                &format!("{}: {}", result.problem, result.detail),
                            );
                        }
                    }
                    std::thread::sleep(Duration::from_secs(300));
                }
            });
            Ok(())
        })
        .on_window_event(move |window, event| {
            // §120: closing the window keeps the servers running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window_core
                    .inner()
                    .setting_bool("startup.close_to_tray", true)
                {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    api.prevent_close();
                    begin_shutdown(&window.app_handle(), window_core.clone());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![run_command])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
