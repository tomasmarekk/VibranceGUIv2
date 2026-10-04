//! VibranceGUI v2's Windows desktop lifecycle and Rust command boundary.
//! Driver access runs on a single observer thread, independently of the webview.

mod observer;
mod platform;
mod settings;

use anyhow::Context;
use base64::{Engine as _, prelude::BASE64_STANDARD};
use observer::{AppState, Observer, SharedState};
use settings::{ColorSettings, ConfigStore, GlobalSettings, Profile};
use std::{
    fs,
    io::Write,
    sync::{Arc, Mutex},
};
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt;

/// Edge length of extracted program icons, sharp at 150% scaling in the profile header.
const ICON_SIZE: u16 = 96;
/// Bounds one icon request so the UI cannot queue unbounded shell work.
const MAX_ICON_REQUEST: usize = 512;

struct DesktopState {
    snapshot: SharedState,
    store: ConfigStore,
    observer: Mutex<Option<Observer>>,
}

fn publish(app: &tauri::AppHandle, snapshot: &AppState) {
    if let Err(error) = app.emit("state-changed", snapshot) {
        tracing::warn!(%error, "failed to publish application state");
    }
}

#[tauri::command]
fn get_state(state: tauri::State<'_, DesktopState>) -> AppState {
    state
        .snapshot
        .lock()
        .expect("application state mutex poisoned")
        .clone()
}

#[tauri::command]
fn save_settings(
    settings: GlobalSettings,
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> Result<AppState, String> {
    let mut snapshot = state
        .snapshot
        .lock()
        .expect("application state mutex poisoned");
    let old_autostart = app
        .autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())?;
    if settings.autostart {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    }
    .map_err(|error| error.to_string())?;
    let mut config = snapshot.config();
    config.settings = settings;
    if let Err(error) = state.store.save(&config) {
        let rollback = if old_autostart {
            app.autolaunch().enable()
        } else {
            app.autolaunch().disable()
        };
        if let Err(rollback_error) = rollback {
            tracing::error!(%rollback_error, "failed to restore autostart after settings save failed");
        }
        return Err(error.to_string());
    }
    snapshot.settings = config.settings;
    let result = snapshot.clone();
    drop(snapshot);
    publish(&app, &result);
    Ok(result)
}

#[tauri::command]
fn save_desktop(
    color: ColorSettings,
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> Result<AppState, String> {
    let mut snapshot = state
        .snapshot
        .lock()
        .expect("application state mutex poisoned");
    let mut config = snapshot.config();
    config.desktop = color;
    state
        .store
        .save(&config)
        .map_err(|error| error.to_string())?;
    snapshot.desktop = color;
    let result = snapshot.clone();
    drop(snapshot);
    publish(&app, &result);
    Ok(result)
}

#[tauri::command]
fn save_profile(
    profile: Profile,
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> Result<AppState, String> {
    let mut snapshot = state
        .snapshot
        .lock()
        .expect("application state mutex poisoned");
    let mut config = snapshot.config();
    if let Some(existing) = config
        .profiles
        .iter_mut()
        .find(|item| item.id == profile.id)
    {
        *existing = profile;
    } else {
        config.profiles.push(profile);
    }
    state
        .store
        .save(&config)
        .map_err(|error| error.to_string())?;
    snapshot.profiles = config.profiles;
    let result = snapshot.clone();
    drop(snapshot);
    publish(&app, &result);
    Ok(result)
}

#[tauri::command]
fn remove_profile(
    id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> Result<AppState, String> {
    let mut snapshot = state
        .snapshot
        .lock()
        .expect("application state mutex poisoned");
    let mut config = snapshot.config();
    config.profiles.retain(|profile| profile.id != id);
    state
        .store
        .save(&config)
        .map_err(|error| error.to_string())?;
    snapshot.profiles = config.profiles;
    let result = snapshot.clone();
    drop(snapshot);
    publish(&app, &result);
    Ok(result)
}

#[tauri::command]
fn set_enabled(
    enabled: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopState>,
) -> AppState {
    let mut snapshot = state
        .snapshot
        .lock()
        .expect("application state mutex poisoned");
    snapshot.status.enabled = enabled;
    let result = snapshot.clone();
    drop(snapshot);
    publish(&app, &result);
    result
}

/// Prefers the name Windows shows in Task Manager over the bare file stem.
fn display_name(executable_path: &str) -> String {
    platform::executable_description(executable_path)
        .unwrap_or_else(|| settings::executable_stem(executable_path))
}

/// Accepts a window title as a program name only when it looks like one.
///
/// Games without version resources usually title their window with the game's name,
/// while document apps add separators such as "file.txt - Editor"; those are skipped.
fn title_name(title: &str) -> Option<String> {
    let title = title.trim();
    let document_like = [" - ", " – ", " — ", " | "]
        .iter()
        .any(|separator| title.contains(separator));
    (!title.is_empty()
        && title.chars().count() <= 40
        && !document_like
        && !title.chars().any(char::is_control))
    .then(|| title.to_owned())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Executable {
    name: String,
    executable_path: String,
    pid: u32,
}

#[tauri::command]
async fn list_running_apps() -> Result<Vec<Executable>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        platform::running_apps()
            .into_iter()
            .map(|process| Executable {
                name: platform::executable_description(&process.exe_path)
                    .or_else(|| title_name(&process.title))
                    .unwrap_or_else(|| settings::executable_stem(&process.exe_name)),
                executable_path: process.exe_path,
                pid: process.pid,
            })
            .collect()
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn pick_executable(app: tauri::AppHandle) -> Result<Option<Executable>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("Windows applications", &["exe"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|error| error.to_string())?;
        if !path.is_file() {
            return Err("the selected executable no longer exists".into());
        }
        let executable_path = path
            .to_str()
            .ok_or("the executable path is not valid Unicode")?
            .to_owned();
        settings::validate_executable(&executable_path).map_err(|error| error.to_string())?;
        Ok(Some(Executable {
            name: display_name(&executable_path),
            executable_path,
            pid: 0,
        }))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Returns PNG data URLs in request order; unusable paths and icon-less files yield `None`.
#[tauri::command]
async fn executable_icons(paths: Vec<String>) -> Result<Vec<Option<String>>, String> {
    if paths.len() > MAX_ICON_REQUEST {
        return Err(format!(
            "at most {MAX_ICON_REQUEST} icons can be requested at once"
        ));
    }
    tauri::async_runtime::spawn_blocking(move || {
        paths
            .iter()
            .map(|path| {
                settings::validate_executable(path).ok()?;
                let png = platform::executable_icon_png(path, ICON_SIZE)?;
                Some(format!(
                    "data:image/png;base64,{}",
                    BASE64_STANDARD.encode(png)
                ))
            })
            .collect()
    })
    .await
    .map_err(|error| error.to_string())
}

fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        for result in [window.show(), window.unminimize(), window.set_focus()] {
            if let Err(error) = result {
                tracing::warn!(%error, "failed to show window");
            }
        }
    }
}

fn create_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::{
        menu::{Menu, MenuItem},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };
    let show = MenuItem::with_id(app, "show", "Open VibranceGUI v2", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Pause / resume profiles", true, None::<&str>)?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        "Exit and restore display settings",
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&show, &pause, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main-tray")
        .tooltip("VibranceGUI v2")
        .menu(&menu)
        .show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.on_menu_event(|app, event| match event.id.as_ref() {
        "show" => show_window(app),
        "pause" => {
            let state = app.state::<DesktopState>();
            let enabled = !state
                .snapshot
                .lock()
                .expect("application state mutex poisoned")
                .status
                .enabled;
            set_enabled(enabled, app.clone(), state);
        }
        "quit" => app.exit(0),
        _ => {}
    })
    .on_tray_icon_event(|tray, event| {
        if matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
        ) {
            show_window(tray.app_handle());
        }
    })
    .build(app)?;
    Ok(())
}

/// Starts the native desktop application and restores display state on exit.
///
/// # Errors
/// Returns an error if the desktop runtime cannot initialize.
pub fn run() -> anyhow::Result<()> {
    if std::env::args().any(|arg| arg == "--diagnostics") {
        let native =
            platform::NativeController::new().context("failed to inspect display capabilities")?;
        let diagnostics = serde_json::json!({ "version": env!("CARGO_PKG_VERSION"), "displays": native.displays(), "foreground": platform::foreground_app(), "runningApps": platform::running_apps() });
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer_pretty(&mut stdout, &diagnostics)?;
        stdout.write_all(b"\n")?;
        return Ok(());
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_window(app)
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--minimized"])
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_state,
            save_settings,
            save_desktop,
            save_profile,
            remove_profile,
            set_enabled,
            list_running_apps,
            pick_executable,
            executable_icons
        ])
        .setup(|app| {
            let directory = app.path().app_config_dir()?;
            fs::create_dir_all(&directory)?;
            let log_path = directory.join("application.log");
            let truncate =
                fs::metadata(&log_path).is_ok_and(|metadata| metadata.len() > 2 * 1024 * 1024);
            let log = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(!truncate)
                .truncate(truncate)
                .open(log_path)?;
            let _ = tracing_subscriber::fmt()
                .with_ansi(false)
                .with_max_level(tracing::Level::INFO)
                .with_writer(Mutex::new(log))
                .try_init();
            let store = ConfigStore::new(directory.join("settings.json"));
            let (mut config, error) = match store.load() {
                Ok(config) => (config, None),
                Err(error) => {
                    tracing::error!(%error, "failed to load settings");
                    (
                        settings::Config::default(),
                        Some(format!(
                            "Settings could not be loaded. Your file is preserved. {error}"
                        )),
                    )
                }
            };
            config.settings.autostart = app.autolaunch().is_enabled()?;
            let snapshot = Arc::new(Mutex::new(AppState::new(config, error)));
            if std::env::args().any(|arg| arg == "--paused") {
                snapshot
                    .lock()
                    .expect("application state mutex poisoned")
                    .status
                    .enabled = false;
            }
            app.manage(DesktopState {
                snapshot: Arc::clone(&snapshot),
                store,
                observer: Mutex::new(None),
            });
            create_tray(app.handle())?;
            let observer = Observer::start(snapshot, app.handle().clone())?;
            *app.state::<DesktopState>()
                .observer
                .lock()
                .expect("observer lifecycle mutex poisoned") = Some(observer);
            if !std::env::args().any(|arg| arg == "--minimized" || arg == "-minimized") {
                show_window(app.handle());
            }
            tracing::info!(version = env!("CARGO_PKG_VERSION"), "application started");
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Resized(_))
                && window.is_minimized().unwrap_or(false)
                && let Err(error) = window.hide()
            {
                tracing::warn!(%error, "failed to hide window to tray");
            }
        })
        .build(tauri::generate_context!())?;
    app.run(|app, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) && let Some(state) = app.try_state::<DesktopState>()
            && let Some(mut observer) = state
                .observer
                .lock()
                .expect("observer lifecycle mutex poisoned")
                .take()
        {
            observer.shutdown();
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::title_name;

    #[test]
    fn game_window_titles_become_program_names() {
        assert_eq!(title_name("  VALORANT "), Some("VALORANT".into()));
        assert_eq!(
            title_name("Counter-Strike 2"),
            Some("Counter-Strike 2".into())
        );
    }

    #[test]
    fn document_and_unusable_titles_are_ignored() {
        assert_eq!(title_name("notes.txt - Notepad"), None);
        assert_eq!(title_name("Inbox | Mail"), None);
        assert_eq!(title_name(""), None);
        assert_eq!(title_name(&"x".repeat(41)), None);
    }
}
