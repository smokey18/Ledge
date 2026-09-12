mod claude;
mod codex;
mod commands;
mod detect;
mod ipc;
mod setup;
mod state;
mod window;

use serde_json::Value;
use state::Ledge;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;

const SWEEP_INTERVAL: Duration = Duration::from_secs(60);

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub fn dispatch(app: &AppHandle, agent: &str, payload: Value) {
    let incoming = match agent {
        "claude" => claude::map(&payload),
        "codex" => codex::map(&payload),
        "codex-notify" => codex::map_notify(&payload),
        _ => None,
    };
    let Some(incoming) = incoming else { return };
    dispatch_incoming(app, incoming);
}

pub fn dispatch_incoming(app: &AppHandle, incoming: state::Incoming) {
    let ledge = app.state::<Ledge>();
    if ledge.apply(incoming, now_ms()).is_some() {
        ledge.save(app);
        broadcast(app);
    }
    ensure_sweeper(app);
}

fn broadcast(app: &AppHandle) {
    let _ = app.emit("sessions", app.state::<Ledge>().snapshot());
}

/// Nothing heartbeats, so stale work is only caught by looking.
fn ensure_sweeper(app: &AppHandle) {
    static RUNNING: AtomicBool = AtomicBool::new(false);

    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(SWEEP_INTERVAL);

        let ledge = app.state::<Ledge>();
        if ledge.sweep(now_ms()) {
            ledge.save(&app);
            broadcast(&app);
        }
        if ledge.is_empty() {
            RUNNING.store(false, Ordering::SeqCst);
            return;
        }
    });
}

fn sync_claude_hook(app: &AppHandle) {
    if !detect::is_available("claude") {
        return;
    }
    let Some(binary) = ipc::hook_binary_path(app) else {
        eprintln!("ledge: the ledge-hook binary is missing");
        return;
    };
    if let Err(error) = setup::connect("claude", &binary) {
        eprintln!("ledge: could not connect Claude Code: {error}");
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Hide Ledge", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Ledge", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &quit])?;
    app.manage(window::TrayToggle(toggle.clone()));

    // macOS keeps the silhouette and discards the colour.
    let icon = Image::from_bytes(include_bytes!("../icons/tray.png"))?;

    TrayIconBuilder::new()
        .icon(icon)
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => window::toggle_visibility(app),
            "quit" => {
                app.state::<Ledge>().save(app);
                let _ = setup::disconnect("claude");
                ipc::cleanup();
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            commands::get_sessions,
            commands::get_prefs,
            commands::set_expanded,
            commands::set_autostart,
            commands::open_project,
            commands::agent_labels,
            commands::open_settings,
            commands::integrations,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            let ledge = Ledge::load(&handle);

            let (positions, last_display, expanded) = {
                let settings = ledge.settings.lock().unwrap();
                (
                    settings.positions.clone(),
                    settings.last_display.clone(),
                    settings.expanded,
                )
            };
            app.manage(ledge);

            if let Some(main) = app.get_webview_window("main") {
                window::apply_material(&main);
                let _ = main.set_size(window::size_for(expanded));
                window::restore(&main, &positions, last_display.as_deref());
            }

            if let Err(error) = ipc::start(handle.clone()) {
                eprintln!("ledge: could not open the hook endpoint: {error}");
            }
            setup::remove_legacy_codex();
            sync_claude_hook(&handle);
            codex::start(handle.clone());
            build_tray(&handle)?;
            window::sync_tray_label(&handle);
            broadcast(&handle);

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Tauri has no display-change event, so unplugs are caught here.
            WindowEvent::Moved(_) => {
                window::recenter_if_stranded(window);
                window::remember_position(window);
            }
            WindowEvent::ScaleFactorChanged { .. } => window::recenter_if_stranded(window),
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                api.prevent_close();
                let _ = window.hide();
                window::sync_tray_label(window.app_handle());
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running Ledge");
}
