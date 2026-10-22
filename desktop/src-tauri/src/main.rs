//! ChitChat desktop: the ChitChat web client (bundled from `web/dist`) in a window that keeps
//! running in the tray, so notifications (real system notifications) arrive with it closed.
//!
//! The app is not a host: like the website, it connects to hosts you choose. Their pages never
//! load inside the app; links to anything outside it open in the normal browser.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use url::Url;

/// Started with the computer: stay in the tray until opened.
const HIDDEN_ARG: &str = "--hidden";

/// The bundled client's own pages.
fn is_app_page(url: &Url) -> bool {
    matches!((url.scheme(), url.host_str()), ("tauri", Some("localhost")) | ("http" | "https", Some("tauri.localhost")))
}

/// Links that leave the app open in the normal browser (or mail app).
fn open_outside(app: &AppHandle, url: &Url) {
    if matches!(url.scheme(), "http" | "https" | "mailto") {
        let _ = app.opener().open_url(url.as_str(), None::<&str>);
    }
}

fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// From the client: a system notification about a new message.
#[tauri::command]
fn notify(app: AppHandle, title: String, body: String) -> Result<(), String> {
    let cut = |s: String, n: usize| s.chars().take(n).collect::<String>();
    app.notification().builder().title(cut(title, 120)).body(cut(body, 300)).show().map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show(app)))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![HIDDEN_ARG])))
        .invoke_handler(tauri::generate_handler![notify])
        .setup(|app| {
            let nav = app.handle().clone();
            let popup = app.handle().clone();
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("ChitChat")
                .inner_size(1200.0, 800.0)
                .min_inner_size(380.0, 560.0)
                .visible(!std::env::args().any(|a| a == HIDDEN_ARG))
                // Tauri's defaults, plus: the ChitChat sound may play without a click first.
                .additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required")
                .on_navigation(move |url| {
                    if is_app_page(url) || url.scheme() == "about" {
                        return true;
                    }
                    open_outside(&nav, url);
                    false
                })
                .on_new_window(move |url, _features| {
                    open_outside(&popup, &url);
                    NewWindowResponse::Deny
                })
                .build()?;

            // Closing the window keeps ChitChat running (and notifying) in the tray.
            let w = window.clone();
            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = w.hide();
                }
            });

            let open = MenuItem::with_id(app, "open", "Open ChitChat", true, None::<&str>)?;
            let autostart = app.autolaunch().is_enabled().unwrap_or(false);
            let startup = CheckMenuItem::with_id(app, "autostart", "Start when the computer starts", true, autostart, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit ChitChat", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &startup, &PredefinedMenuItem::separator(app)?, &quit])?;
            let check = startup.clone();
            TrayIconBuilder::with_id("tray")
                .icon(app.default_window_icon().cloned().expect("app icon"))
                .tooltip("ChitChat")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "open" => show(app),
                    "autostart" => {
                        let launcher = app.autolaunch();
                        let on = !launcher.is_enabled().unwrap_or(false);
                        let _ = if on { launcher.enable() } else { launcher.disable() };
                        let _ = check.set_checked(launcher.is_enabled().unwrap_or(on));
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        show(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running ChitChat");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_bundled_pages_stay_in_the_app() {
        assert!(is_app_page(&Url::parse("http://tauri.localhost/s/dev").unwrap()));
        assert!(is_app_page(&Url::parse("tauri://localhost/index.html").unwrap()));
        assert!(!is_app_page(&Url::parse("https://chat.example.org/").unwrap()));
        assert!(!is_app_page(&Url::parse("https://tauri.localhost.evil.com/").unwrap()));
        assert!(!is_app_page(&Url::parse("http://localhost/").unwrap()));
    }
}
