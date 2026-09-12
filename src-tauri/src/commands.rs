use crate::state::{Ledge, SessionEvent};
use crate::{detect, window};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

type Command<T> = Result<T, String>;

#[derive(Serialize)]
pub struct Integration {
    agent: &'static str,
    label: &'static str,
    available: bool,
}

#[derive(Serialize)]
pub struct Prefs {
    expanded: bool,
    autostart: bool,
}

#[tauri::command]
pub fn get_sessions(app: AppHandle) -> Vec<SessionEvent> {
    app.state::<Ledge>().snapshot()
}

#[tauri::command]
pub fn get_prefs(app: AppHandle) -> Prefs {
    let autostart = app.autolaunch().is_enabled().unwrap_or(false);
    let ledge = app.state::<Ledge>();
    let settings = ledge.settings.lock().unwrap();

    Prefs {
        expanded: settings.expanded,
        autostart,
    }
}

#[tauri::command]
pub fn set_expanded(app: AppHandle, expanded: bool) {
    let ledge = app.state::<Ledge>();
    ledge.settings.lock().unwrap().expanded = expanded;

    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_size(window::size_for(expanded));
    }
    ledge.save(&app);
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Command<()> {
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_project(app: AppHandle, session_id: String) -> Command<()> {
    let cwd = app
        .state::<Ledge>()
        .sessions
        .lock()
        .unwrap()
        .get(&session_id)
        .map(|session| session.cwd.clone())
        .filter(|cwd| !cwd.is_empty())
        .ok_or("no directory known for that session")?;

    if !Path::new(&cwd).is_dir() {
        return Err("that directory no longer exists".into());
    }
    app.opener()
        .open_path(cwd, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn agent_labels() -> HashMap<&'static str, &'static str> {
    detect::AGENTS
        .iter()
        .map(|agent| (agent.id, agent.label))
        .collect()
}

#[tauri::command]
pub fn open_settings(app: AppHandle) -> Command<()> {
    if let Some(existing) = app.get_webview_window("settings") {
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }

    let window =
        WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("settings.html".into()))
            .title("Ledge Settings")
            .inner_size(460.0, 560.0)
            .min_inner_size(460.0, 420.0)
            .resizable(true)
            .title_bar_style(tauri::TitleBarStyle::Transparent)
            .transparent(true)
            .center()
            .build()
            .map_err(|e| e.to_string())?;

    window::apply_material(&window);
    Ok(())
}

#[tauri::command]
pub fn integrations() -> Vec<Integration> {
    detect::AGENTS
        .iter()
        .map(|agent| Integration {
            agent: agent.id,
            label: agent.label,
            available: detect::is_available(agent.id),
        })
        .collect()
}
