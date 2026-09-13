use crate::state::Ledge;
use std::collections::HashMap;
use std::time::Duration;
use tauri::menu::MenuItem;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, Monitor, PhysicalPosition, WebviewWindow,
    Window, Wry,
};

pub struct TrayToggle(pub MenuItem<Wry>);

pub const RAIL_WIDTH: f64 = 76.0;

pub const POPOVER_GAP: f64 = 9.0;

pub const POPOVER_WIDTH: f64 = 268.0;

pub fn popover_height(rows: usize) -> f64 {
    let rows = rows.max(1) as f64;
    26.0 + 31.0 + (31.0 * rows + 10.0 * (rows - 1.0))
}

/// Enough of the widget must stay on screen to be grabbable.
const VISIBLE_MARGIN: f64 = 60.0;
const MOVE_SETTLE: Duration = Duration::from_millis(600);

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, PanelLevel, StyleMask, WebviewWindowExt};

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(LedgePanel {
        config: {
            can_become_key_window: true,
            can_become_main_window: false,
            is_floating_panel: true
        }
    })
}

#[cfg(target_os = "macos")]
pub fn float_over_fullscreen(window: &WebviewWindow) {
    let panel = match window.to_panel::<LedgePanel>() {
        Ok(panel) => panel,
        Err(error) => {
            eprintln!(
                "ledge: [{}] could not create panel: {error}",
                window.label()
            );
            return;
        }
    };

    panel.set_level(PanelLevel::Status.value());
    panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .full_screen_auxiliary()
            .into(),
    );
}

#[cfg(not(target_os = "macos"))]
pub fn float_over_fullscreen(_window: &WebviewWindow) {}

pub fn apply_material(window: &WebviewWindow, radius: f64) {
    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};

    let _ = apply_vibrancy(
        window,
        NSVisualEffectMaterial::HudWindow,
        Some(NSVisualEffectState::Active),
        Some(radius),
    );
}

pub fn rail_size(agents: usize) -> LogicalSize<f64> {
    const ENDS: f64 = 16.0 + 28.0 + 18.0 + 14.0;
    const NODE_PADDING: f64 = 20.0 + 26.0;

    if agents == 0 {
        return LogicalSize::new(RAIL_WIDTH, RAIL_WIDTH);
    }

    let agents = agents as f64;
    let nodes = 38.0 * agents + 20.0 * (agents - 1.0);
    LogicalSize::new(RAIL_WIDTH, ENDS + NODE_PADDING + nodes)
}

/// Names are absent on some compositors, so geometry backs the key up.
fn display_key(monitor: &Monitor) -> String {
    let origin = monitor.position();
    let size = monitor.size();
    format!(
        "{}:{}x{}@{},{}",
        monitor.name().map(String::as_str).unwrap_or("display"),
        size.width,
        size.height,
        origin.x,
        origin.y
    )
}

fn holds_point(monitor: &Monitor, (x, y): (f64, f64)) -> bool {
    let scale = monitor.scale_factor();
    let origin = monitor.position().to_logical::<f64>(scale);
    let size = monitor.size().to_logical::<f64>(scale);

    x >= origin.x
        && x <= origin.x + size.width - VISIBLE_MARGIN
        && y >= origin.y
        && y <= origin.y + size.height - VISIBLE_MARGIN
}

fn logical(scale: f64, position: PhysicalPosition<i32>) -> (f64, f64) {
    let position = position.to_logical::<f64>(scale);
    (position.x, position.y)
}

pub fn restore(
    window: &WebviewWindow,
    positions: &HashMap<String, (f64, f64)>,
    last: Option<&str>,
) {
    let restored = window.available_monitors().ok().and_then(|monitors| {
        let connected: Vec<(String, Monitor)> =
            monitors.into_iter().map(|m| (display_key(&m), m)).collect();

        let preferred = last.and_then(|key| connected.iter().find(|(k, _)| k == key));
        preferred
            .into_iter()
            .chain(connected.iter().filter(|(k, _)| Some(k.as_str()) != last))
            .find_map(|(key, monitor)| {
                positions
                    .get(key)
                    .filter(|pos| holds_point(monitor, **pos))
                    .map(|(x, y)| LogicalPosition::new(*x, *y))
            })
    });

    match restored {
        Some(position) => drop(window.set_position(position)),
        None => drop(window.center()),
    }
}

pub fn recenter_if_stranded(window: &Window) {
    let (Ok(scale), Ok(position)) = (window.scale_factor(), window.outer_position()) else {
        return;
    };
    let Ok(monitors) = window.available_monitors() else {
        return;
    };
    let position = logical(scale, position);
    if !monitors.iter().any(|m| holds_point(m, position)) {
        let _ = window.center();
    }
}

pub fn remember_position(window: &Window) {
    if window.label() != "main" {
        return;
    }
    let app = window.app_handle().clone();
    let label = window.label().to_string();

    let ledge = app.state::<Ledge>();
    let mut seen = ledge.note_move();
    if !ledge.claim_move_watch() {
        return;
    }

    std::thread::spawn(move || {
        let ledge = app.state::<Ledge>();
        loop {
            std::thread::sleep(MOVE_SETTLE);
            let latest = ledge.move_generation();
            if latest == seen {
                break;
            }
            seen = latest;
        }

        if let Some(window) = app.get_webview_window(&label) {
            if let (Ok(scale), Ok(position)) = (window.scale_factor(), window.outer_position()) {
                let key = window
                    .current_monitor()
                    .ok()
                    .flatten()
                    .map(|monitor| display_key(&monitor))
                    .unwrap_or_else(|| "display".into());

                {
                    let mut settings = ledge.settings.lock().unwrap();
                    settings
                        .positions
                        .insert(key.clone(), logical(scale, position));
                    settings.last_display = Some(key);
                }
                ledge.save(&app);
            }
        }
        ledge.finish_move_watch();
        if ledge.move_generation() != seen {
            if let Some(window) = app.get_webview_window(&label) {
                remember_position(&window.as_ref().window());
            }
        }
    });
}

pub fn toggle_visibility(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        crate::commands::close_popover(app.clone(), None);
        let _ = window.hide();
    } else {
        let _ = window.show();
        let _ = window.set_focus();
    }
    sync_tray_label(app);
}

pub fn sync_tray_label(app: &AppHandle) {
    let Some(toggle) = app.try_state::<TrayToggle>() else {
        return;
    };
    let visible = app
        .get_webview_window("main")
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);

    let _ = toggle
        .0
        .set_text(if visible { "Hide Ledge" } else { "Show Ledge" });
}
