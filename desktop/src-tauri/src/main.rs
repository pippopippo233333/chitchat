//! ChitChat desktop: the ChitChat web client (bundled from `web/dist`) in a window that keeps
//! running in the tray, so notifications (real system notifications) arrive with it closed.
//!
//! The app is not a host: like the website, it connects to hosts you choose. Their pages never
//! load inside the app; links to anything outside it open in the normal browser.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Mutex;
use std::time::Duration;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
#[cfg(not(windows))]
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;
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

/// How often the app, which may run for weeks in the tray, looks for a new version.
const UPDATE_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// The version the person said "Later" to: not asked again until they check by hand.
struct Postponed(Mutex<Option<String>>);

/// Asks GitHub Releases for a newer signed version (`plugins.updater` in tauri.conf.json) and
/// offers to install it. `by_hand`: from the tray menu, so also say when there's nothing new.
async fn check_for_update(app: AppHandle, by_hand: bool) {
    let say = |text: String| {
        let app = app.clone();
        async move {
            let _ = tauri::async_runtime::spawn_blocking(move || app.dialog().message(text).title("ChitChat updates").blocking_show()).await;
        }
    };
    let found = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let update = match found {
        Ok(Some(update)) => update,
        Ok(None) => {
            if by_hand {
                say(format!("You have the latest version of ChitChat ({}).", app.package_info().version)).await;
            }
            return;
        }
        Err(e) => {
            if by_hand {
                say(format!("Couldn't check for updates: {e}")).await;
            }
            return;
        }
    };
    let postponed = app.state::<Postponed>();
    if !by_hand && postponed.0.lock().unwrap().as_deref() == Some(update.version.as_str()) {
        return;
    }
    let notes = update
        .body
        .clone()
        .filter(|b| !b.trim().is_empty())
        .map(|b| {
            format!(
                "

What's new:
{b}"
            )
        })
        .unwrap_or_default();
    let text = format!(
        "ChitChat {} is available (you have {}).{notes}

Updating restarts ChitChat.",
        update.version, update.current_version
    );
    let ask = app.clone();
    let yes = tauri::async_runtime::spawn_blocking(move || {
        ask.dialog().message(text).title("Update ChitChat").buttons(MessageDialogButtons::OkCancelCustom("Update now".into(), "Later".into())).blocking_show()
    })
    .await
    .unwrap_or(false);
    if !yes {
        *postponed.0.lock().unwrap() = Some(update.version.clone());
        return;
    }
    // The download is checked against the public key in tauri.conf.json before anything runs.
    match update.download_and_install(|_, _| {}, || {}).await {
        Ok(()) => app.restart(),
        Err(e) => say(format!("The update couldn't be installed: {e}")).await,
    }
}

/// From the client: this app's version (shown with "Check for updates").
#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// From the client: the same as "Check for updates" in the tray menu.
#[tauri::command]
fn check_updates(app: AppHandle) {
    tauri::async_runtime::spawn(check_for_update(app, true));
}

/// From the client: a system notification about a new message. `target` (opaque to the app, at
/// most 1 KiB) comes back to the client in a `notification-click` event when it is clicked. With
/// `when_closed` false, nothing shows while the window is closed to the tray.
#[tauri::command]
fn notify(app: AppHandle, title: String, body: String, target: Option<String>, when_closed: Option<bool>) -> Result<(), String> {
    let open = app.get_webview_window("main").and_then(|w| w.is_visible().ok()).unwrap_or(false);
    if when_closed == Some(false) && !open {
        return Ok(());
    }
    let cut = |s: String, n: usize| s.chars().take(n).collect::<String>();
    let target = target.filter(|t| t.len() <= 1024);
    #[cfg(windows)]
    return windows_toast(&app, &cut(title, 120), &cut(body, 300), target).map_err(|e| e.to_string());
    #[cfg(not(windows))]
    {
        let _ = target; // Click handling is Windows-only for now.
        app.notification().builder().title(cut(title, 120)).body(cut(body, 300)).show().map_err(|e| e.to_string())
    }
}

/// A Windows toast that opens the app (and the conversation) when clicked. Silent: the client
/// plays its own notification sound, with the user's volume.
#[cfg(windows)]
fn windows_toast(app: &AppHandle, title: &str, body: &str, target: Option<String>) -> tauri_winrt_notification::Result<()> {
    use tauri::Emitter;
    use tauri_winrt_notification::Toast;
    // Like the notification plugin: the installed app has its own identity (its Start menu
    // shortcut); a bare build from target/ borrows PowerShell's so toasts still show.
    let installed = tauri::utils::platform::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| !dir.ends_with("target/debug") && !dir.ends_with("target/release")))
        .unwrap_or(true);
    let id = if installed { app.config().identifier.clone() } else { Toast::POWERSHELL_APP_ID.to_string() };
    let clicked = app.clone();
    Toast::new(&id)
        .title(title)
        .text1(body)
        .sound(None)
        .on_activated(move |_| {
            show(&clicked);
            if let Some(target) = &target {
                let _ = clicked.emit_to("main", "notification-click", target.clone());
            }
            Ok(())
        })
        .show()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show(app)))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(Postponed(Mutex::new(None)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![HIDDEN_ARG])))
        .invoke_handler(tauri::generate_handler![notify, app_version, check_updates])
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
            let updates = MenuItem::with_id(app, "updates", "Check for updates", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit ChitChat", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &startup, &updates, &PredefinedMenuItem::separator(app)?, &quit])?;
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
                    "updates" => {
                        tauri::async_runtime::spawn(check_for_update(app.clone(), true));
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

            let updater = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(Duration::from_secs(15)).await;
                loop {
                    check_for_update(updater.clone(), false).await;
                    tokio::time::sleep(UPDATE_EVERY).await;
                }
            });
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
