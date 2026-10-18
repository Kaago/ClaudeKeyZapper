//! Menu bar (macOS) or notification area (Windows) icon: budgets at a glance and a project's profile switched
//! without opening the window. Closing the window keeps KeyZapper running here.

use crate::model::TrayInfo;
use crate::{changed, refresh, show_window, with_model};
use keyzapper_core::l;
use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

const TRAY_ID: &str = "main";
const OPEN: &str = "open";
const QUIT: &str = "quit";
const BIND: &str = "bind";

/// The content shown last (main thread only); the menu is only rebuilt when it changes, so it does not flicker while open.
static SHOWN: Mutex<Option<TrayInfo>> = Mutex::new(None);

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    // macOS: a black template image that the menu bar tints; Windows: the colored app icon.
    #[cfg(target_os = "macos")]
    let icon = tauri::include_image!("icons/tray.png");
    #[cfg(not(target_os = "macos"))]
    let icon = tauri::include_image!("icons/32x32.png");
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .show_menu_on_left_click(cfg!(target_os = "macos"))
        .tooltip("KeyZapper")
        .on_menu_event(|app, event| handle(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                if !cfg!(target_os = "macos") {
                    show_window(tray.app_handle());
                }
            }
        })
        .build(app)?;
    update(app);
    Ok(())
}

/// Rebuilds the menu if budgets, profiles or projects changed. Runs on the main thread, where the menu lives, so a
/// caller never waits for it.
pub fn update(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let info = with_model(&handle, |m| m.tray_info());
        let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
        if shown.as_ref() == Some(&info) {
            return;
        }
        let Some(tray) = handle.tray_by_id(TRAY_ID) else { return };
        if let Ok(menu) = build_menu(&handle, &info) {
            let _ = tray.set_menu(Some(menu));
            let _ = tray.set_tooltip(Some(tooltip(&info)));
            *shown = Some(info);
        }
    });
}

fn money(value: f64) -> String {
    if keyzapper_core::i18n::language() == "de" {
        format!("{value:.2} $").replace('.', ",")
    } else {
        format!("${value:.2}")
    }
}

fn budget_text(name: &str, (spend, max): (f64, Option<f64>)) -> String {
    match max {
        Some(max) => l!("%@: %@ übrig von %@", name, money((max - spend).max(0.0)), money(max)),
        None => l!("%@: %@ verbraucht", name, money(spend)),
    }
}

fn tooltip(info: &TrayInfo) -> String {
    let attention = info.projects.iter().filter(|p| !p.healthy).count();
    if info.disabled {
        l!("KeyZapper ist deaktiviert")
    } else if attention > 0 {
        l!("KeyZapper – %@ Projekt(e) brauchen Aufmerksamkeit", attention)
    } else {
        "KeyZapper".to_string()
    }
}

fn build_menu(app: &AppHandle, info: &TrayInfo) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    for profile in &info.profiles {
        if let Some(budget) = profile.budget {
            menu.append(&MenuItem::new(app, budget_text(&profile.name, budget), false, None::<&str>)?)?;
        }
    }
    if info.profiles.iter().any(|p| p.budget.is_some()) {
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    if info.disabled {
        menu.append(&MenuItem::new(app, l!("KeyZapper ist deaktiviert"), false, None::<&str>)?)?;
    } else if !info.projects.is_empty() {
        menu.append(&MenuItem::new(app, l!("Profil je Projekt"), false, None::<&str>)?)?;
        for project in &info.projects {
            let title = if project.healthy { project.name.clone() } else { format!("⚠︎ {}", project.name) };
            let submenu = Submenu::new(app, title, true)?;
            for profile in &info.profiles {
                let label = if profile.usable { profile.name.clone() } else { l!("%@ (kein Zugang)", profile.name) };
                let id = format!("{BIND}:{}:{}", project.id, profile.id);
                submenu.append(&CheckMenuItem::with_id(app, id, label, profile.usable, profile.id == project.profile_id, None::<&str>)?)?;
            }
            menu.append(&submenu)?;
        }
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, OPEN, l!("KeyZapper öffnen"), true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, QUIT, l!("KeyZapper beenden"), true, None::<&str>)?)?;
    Ok(menu)
}

fn handle(app: &AppHandle, id: &str) {
    match id.split(':').collect::<Vec<_>>().as_slice() {
        [OPEN] => show_window(app),
        [QUIT] => app.exit(0),
        [BIND, binding_id, profile_id] => {
            let (app, binding_id, profile_id) = (app.clone(), binding_id.to_string(), profile_id.to_string());
            // The click toggled the check mark; rebuild so the menu shows the real assignment again.
            *SHOWN.lock().unwrap_or_else(|e| e.into_inner()) = None;
            std::thread::spawn(move || {
                with_model(&app, |m| {
                    if let Some(path) = m.state.bindings.iter().find(|b| b.id == binding_id).map(|b| b.path.clone()) {
                        m.bind(&path, &profile_id);
                    }
                });
                refresh(&app);
                changed(&app);
            });
        }
        _ => {}
    }
}

pub fn exists(app: &AppHandle) -> bool {
    app.tray_by_id(TRAY_ID).is_some()
}
