use crate::state::{Ledge, SessionEvent};
use crate::{detect, window};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};
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
    autostart: bool,
}

const REOPEN_GUARD: Duration = Duration::from_millis(400);

#[derive(Default)]
struct Popover {
    anchor: f64,
    open: Option<String>,
    closed: Option<(String, Instant)>,
}

#[derive(Default)]
pub struct PopoverState(Mutex<Popover>);

#[tauri::command]
pub fn get_sessions(app: AppHandle) -> Vec<SessionEvent> {
    app.state::<Ledge>().snapshot()
}

#[tauri::command]
pub fn get_prefs(app: AppHandle) -> Prefs {
    Prefs {
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
    }
}

#[tauri::command]
pub fn set_agent_count(app: AppHandle, count: usize) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.set_size(window::rail_size(count));
    }
}

#[tauri::command]
pub fn open_popover(app: AppHandle, agent: String, anchor: f64) -> Command<()> {
    {
        let state = app.state::<PopoverState>();
        let mut popover = state.0.lock().unwrap();

        let dismissing = popover.open.as_deref() == Some(&agent)
            || matches!(&popover.closed, Some((was, at))
                if was == &agent && at.elapsed() < REOPEN_GUARD);

        if dismissing {
            drop(popover);
            close_popover(app, None);
            return Ok(());
        }
        popover.anchor = anchor;
        popover.open = Some(agent.clone());
    }

    let rows = app
        .state::<Ledge>()
        .snapshot()
        .iter()
        .filter(|session| session.agent == agent)
        .count();

    let height = window::popover_height(rows);
    let label = popover_label(&agent);
    hide_popovers(&app, Some(&label));

    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.set_size(tauri::LogicalSize::new(window::POPOVER_WIDTH, height));
        place(&app, &label, height);
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(
        &app,
        &label,
        WebviewUrl::App(format!("popover.html?agent={agent}").into()),
    )
    .inner_size(window::POPOVER_WIDTH, height)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .accept_first_mouse(true)
    .visible_on_all_workspaces(true)
    .resizable(false)
    .shadow(true)
    .visible(false)
    .build()
    .map_err(|e| e.to_string())?;

    window::apply_material(&window, 16.0);
    window::float_over_fullscreen(&window);
    place(&app, &label, height);
    let _ = window.show();
    Ok(())
}

fn popover_label(agent: &str) -> String {
    format!("popover-{agent}")
}

fn hide_popovers(app: &AppHandle, except: Option<&str>) {
    for (label, window) in app.webview_windows() {
        if label.starts_with("popover-") && Some(label.as_str()) != except {
            let _ = window.hide();
        }
    }
}

fn place(app: &AppHandle, label: &str, height: f64) {
    let (Some(main), Some(popover)) = (
        app.get_webview_window("main"),
        app.get_webview_window(label),
    ) else {
        return;
    };
    let Ok(scale) = main.scale_factor() else { return };
    let Ok(origin) = main.outer_position() else { return };

    let anchor = app.state::<PopoverState>().0.lock().unwrap().anchor;
    let gap = (window::RAIL_WIDTH + window::POPOVER_GAP) * scale;
    let width = window::POPOVER_WIDTH * scale;

    let mut x = origin.x as f64 + gap;
    let mut y = origin.y as f64 + (anchor - height / 2.0) * scale;

    if let Some(monitor) = main.current_monitor().ok().flatten() {
        let screen = monitor.position();
        let size = monitor.size();
        let (right, bottom) = (
            screen.x as f64 + size.width as f64,
            screen.y as f64 + size.height as f64,
        );

        if x + width > right - 8.0 {
            x = origin.x as f64 - window::POPOVER_GAP * scale - width;
        }
        x = x.clamp(screen.x as f64 + 8.0, (right - width - 8.0).max(screen.x as f64));
        y = y.clamp(
            screen.y as f64 + 8.0,
            (bottom - height * scale - 8.0).max(screen.y as f64),
        );
    }

    let _ = popover.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

#[tauri::command]
pub fn size_popover(app: AppHandle, agent: String, height: f64) {
    let label = popover_label(&agent);
    let open = app.state::<PopoverState>().0.lock().unwrap().open.clone();
    if open.as_deref() != Some(agent.as_str()) {
        return;
    }

    if let Some(popover) = app.get_webview_window(&label) {
        let _ = popover.set_size(tauri::LogicalSize::new(window::POPOVER_WIDTH, height));
    }
    place(&app, &label, height);
}

#[tauri::command]
pub fn close_popover(app: AppHandle, agent: Option<String>) {
    {
        let state = app.state::<PopoverState>();
        let mut popover = state.0.lock().unwrap();

        if agent.is_some() && popover.open != agent {
            return;
        }
        if let Some(agent) = popover.open.take() {
            popover.closed = Some((agent, Instant::now()));
        }
    }

    hide_popovers(&app, None);
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.emit("popover-closed", ());
    }
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

    WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Ledge Settings")
        .inner_size(460.0, 560.0)
        .min_inner_size(460.0, 420.0)
        .resizable(true)
        .title_bar_style(tauri::TitleBarStyle::Transparent)
        .center()
        .build()
        .map_err(|e| e.to_string())?;

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
