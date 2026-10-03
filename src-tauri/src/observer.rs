//! Serial foreground observation and display transitions, independent of the UI.
//! A joined worker owns all driver handles so shutdown can restore captured state.

use crate::{
    platform::{self, DisplayInfo, DisplayMode, ForegroundApp, NativeController},
    settings::{ColorSettings, Config, GlobalSettings, Profile},
};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tauri::Emitter;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObserverStatus {
    pub enabled: bool,
    pub active_profile_id: Option<String>,
    pub gpu_name: String,
    pub supports_vibrance: bool,
    pub supports_gamma: bool,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppState {
    pub settings: GlobalSettings,
    pub desktop: ColorSettings,
    pub profiles: Vec<Profile>,
    pub status: ObserverStatus,
    pub resolutions: Vec<DisplayMode>,
    pub displays: Vec<DisplayInfo>,
}

impl AppState {
    pub(crate) fn new(config: Config, error: Option<String>) -> Self {
        Self {
            settings: config.settings,
            desktop: config.desktop,
            profiles: config.profiles,
            status: ObserverStatus {
                enabled: error.is_none(),
                active_profile_id: None,
                gpu_name: "Detecting graphics adapter…".into(),
                supports_vibrance: false,
                supports_gamma: false,
                message: error,
            },
            resolutions: Vec::new(),
            displays: Vec::new(),
        }
    }

    pub(crate) fn config(&self) -> Config {
        Config {
            schema_version: 1,
            settings: self.settings.clone(),
            desktop: self.desktop,
            profiles: self.profiles.clone(),
        }
    }
}

pub(crate) type SharedState = Arc<Mutex<AppState>>;

#[derive(Clone, Debug, PartialEq)]
struct DisplayPlan {
    id: String,
    color: ColorSettings,
    resolution: Option<DisplayMode>,
}

fn plan(
    config: &Config,
    displays: &[DisplayInfo],
    foreground: Option<&ForegroundApp>,
) -> (Option<String>, Vec<DisplayPlan>) {
    let profile = foreground.and_then(|app| config.matching_profile(&app.exe_name, &app.exe_path));
    let plans = displays
        .iter()
        .filter(|display| !config.settings.primary_only || display.primary)
        .map(|display| {
            let resolution = profile.and_then(|profile| profile.resolution).filter(|_| {
                !config.settings.never_change_resolution
                    && foreground.is_some_and(|app| app.display_id == display.id)
            });
            DisplayPlan {
                id: display.id.clone(),
                color: profile.map_or(config.desktop, |profile| profile.color),
                resolution,
            }
        })
        .collect();
    (profile.map(|profile| profile.id.clone()), plans)
}

fn restore_changed_targets<E>(
    previous: &std::collections::BTreeSet<String>,
    current: &std::collections::BTreeSet<String>,
    pending: &mut bool,
    restore: impl FnOnce() -> Result<bool, E>,
) -> Result<(), E> {
    if previous == current && !*pending {
        return Ok(());
    }
    // Set this before the fallible call: an excluded display still needs another
    // restore attempt even after the active displays successfully receive a profile.
    *pending = true;
    *pending = restore()?;
    Ok(())
}

pub(crate) struct Observer {
    stop: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Observer {
    pub(crate) fn start(state: SharedState, app: tauri::AppHandle) -> std::io::Result<Self> {
        let (stop, receiver) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("display-observer".into())
            .spawn(move || observe(state, app, receiver))?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    pub(crate) fn shutdown(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("display observer panicked during shutdown");
        }
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn notify(state: &SharedState, app: &tauri::AppHandle) {
    let snapshot = state
        .lock()
        .expect("application state mutex poisoned")
        .clone();
    if let Err(error) = app.emit("state-changed", snapshot) {
        tracing::warn!(%error, "failed to publish observer state");
    }
}

fn observe(state: SharedState, app: tauri::AppHandle, stop: mpsc::Receiver<()>) {
    let mut native = loop {
        match NativeController::new() {
            Ok(native) => break native,
            Err(error) => {
                let mut snapshot = state.lock().expect("application state mutex poisoned");
                snapshot.status.message =
                    Some(format!("{error}. Resume to retry display detection."));
                snapshot.status.enabled = false;
                snapshot.status.gpu_name = "No supported display".into();
                drop(snapshot);
                notify(&state, &app);
                loop {
                    match stop.recv_timeout(Duration::from_millis(150)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if state
                        .lock()
                        .expect("application state mutex poisoned")
                        .status
                        .enabled
                    {
                        break;
                    }
                }
            }
        }
    };
    let mut last_plan: Option<Vec<DisplayPlan>> = None;
    let mut applied_targets = std::collections::BTreeSet::new();
    let mut scope_restore_pending = false;
    let mut last_refresh = Instant::now() - Duration::from_secs(6);
    let mut last_attempt = Instant::now() - Duration::from_secs(6);
    let mut was_enabled = false;
    let mut restore_pending = false;
    let mut has_applied = false;
    loop {
        if last_refresh.elapsed() >= Duration::from_secs(5) {
            if let Err(error) = native.refresh_displays() {
                tracing::warn!(%error, "display refresh failed");
            }
            let displays = native.displays();
            let resolutions = displays
                .iter()
                .find(|display| display.primary)
                .or_else(|| displays.first())
                .and_then(|display| platform::display_modes(&display.id).ok())
                .unwrap_or_default();
            let mut snapshot = state.lock().expect("application state mutex poisoned");
            let old_ids: Vec<_> = snapshot
                .displays
                .iter()
                .map(|d| {
                    (
                        &d.id,
                        &d.adapter,
                        d.primary,
                        d.current_mode,
                        d.vibrance_supported,
                        d.gamma_supported,
                    )
                })
                .collect();
            let new_ids: Vec<_> = displays
                .iter()
                .map(|d| {
                    (
                        &d.id,
                        &d.adapter,
                        d.primary,
                        d.current_mode,
                        d.vibrance_supported,
                        d.gamma_supported,
                    )
                })
                .collect();
            if old_ids != new_ids {
                last_plan = None;
            }
            snapshot.status.gpu_name = displays
                .iter()
                .map(|d| d.adapter.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(" · ");
            snapshot.status.supports_vibrance = displays.iter().any(|d| d.vibrance_supported);
            snapshot.status.supports_gamma = displays.iter().any(|d| d.gamma_supported);
            snapshot.displays = displays;
            snapshot.resolutions = resolutions;
            drop(snapshot);
            notify(&state, &app);
            last_refresh = Instant::now();
            // Disconnected outputs retain dirty snapshots. A later reconnect while
            // paused must still restore those outputs instead of waiting for Resume.
            if has_applied && !was_enabled {
                restore_pending = true;
            }
        }
        let (config, enabled, previous_status) = {
            let snapshot = state.lock().expect("application state mutex poisoned");
            (
                snapshot.config(),
                snapshot.status.enabled,
                snapshot.status.clone(),
            )
        };
        let mut errors = Vec::new();
        let mut active_id = None;
        if enabled {
            let foreground = platform::foreground_app();
            let (id, current_plan) = plan(&config, &native.displays(), foreground.as_ref());
            active_id = id;
            let changed = last_plan.as_ref() != Some(&current_plan) || !was_enabled;
            if changed
                || ((previous_status.message.is_some() || scope_restore_pending)
                    && last_attempt.elapsed() >= Duration::from_secs(3))
            {
                let targets: std::collections::BTreeSet<_> = current_plan
                    .iter()
                    .map(|target| target.id.clone())
                    .collect();
                // A changed OS primary monitor can exclude an output without changing
                // preferences. Failed restores must survive subsequent profile applies.
                if let Err(error) = restore_changed_targets(
                    &applied_targets,
                    &targets,
                    &mut scope_restore_pending,
                    || native.restore_all().map(|()| native.has_pending_restore()),
                ) {
                    errors.push(error.to_string());
                }
                for target in &current_plan {
                    if let Err(error) = native.apply(
                        &target.id,
                        target.color.vibrance,
                        target.color.brightness,
                        target.color.gamma,
                        target.resolution.as_ref(),
                    ) {
                        errors.push(error.to_string());
                    }
                }
                last_plan = Some(current_plan);
                applied_targets = targets;
                last_attempt = Instant::now();
                has_applied = true;
            } else if let Some(message) = &previous_status.message {
                errors.push(message.clone());
            }
        } else if was_enabled
            || (restore_pending && last_attempt.elapsed() >= Duration::from_secs(3))
        {
            restore_pending = match native.restore_all() {
                Ok(()) => native.has_pending_restore(),
                Err(error) => {
                    errors.push(error.to_string());
                    true
                }
            };
            last_attempt = Instant::now();
            last_plan = None;
        } else if let Some(message) = previous_status.message.clone() {
            errors.push(message);
        }
        was_enabled = enabled;
        let message = if errors.is_empty() {
            None
        } else {
            Some(errors.join("\n"))
        };
        let mut snapshot = state.lock().expect("application state mutex poisoned");
        snapshot.status.active_profile_id = active_id;
        snapshot.status.message = message;
        let changed = snapshot.status != previous_status;
        drop(snapshot);
        if changed {
            notify(&state, &app);
        }
        match stop.recv_timeout(Duration::from_millis(150)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    if let Err(error) = native.restore_all() {
        tracing::error!(%error, "failed to restore original display state on exit");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_scope_restoration_retries_even_after_target_ids_are_updated() {
        let all = std::collections::BTreeSet::from(["one".into(), "two".into()]);
        let primary = std::collections::BTreeSet::from(["one".into()]);
        let mut pending = false;
        assert!(
            restore_changed_targets(&all, &primary, &mut pending, || Err("driver unavailable"))
                .is_err()
        );
        assert!(pending);
        let mut retried = false;
        restore_changed_targets(&primary, &primary, &mut pending, || {
            retried = true;
            Ok::<_, &str>(false)
        })
        .unwrap();
        assert!(retried);
        assert!(!pending);
    }

    #[test]
    fn changing_os_primary_restores_previous_monitor_and_keeps_disconnected_work_pending() {
        let first = std::collections::BTreeSet::from(["one".into()]);
        let second = std::collections::BTreeSet::from(["two".into()]);
        let mut pending = false;
        restore_changed_targets(&first, &second, &mut pending, || Ok::<_, &str>(true)).unwrap();
        assert!(pending);
    }

    #[test]
    fn unchanged_targets_without_pending_restores_do_not_touch_hardware() {
        let targets = std::collections::BTreeSet::from(["one".into()]);
        let mut pending = false;
        restore_changed_targets(
            &targets,
            &targets,
            &mut pending,
            || -> Result<bool, &str> { panic!("unchanged targets must not restore") },
        )
        .unwrap();
    }
    fn display(id: &str, primary: bool) -> DisplayInfo {
        DisplayInfo {
            id: id.into(),
            name: id.into(),
            adapter: "Test GPU".into(),
            primary,
            vibrance_supported: true,
            gamma_supported: true,
            current_mode: DisplayMode {
                width: 1920,
                height: 1080,
                refresh_rate: 60,
            },
        }
    }
    fn game() -> Profile {
        Profile {
            id: "game".into(),
            name: "Game".into(),
            executable_path: "C:\\game.exe".into(),
            match_by_path: false,
            color: ColorSettings {
                vibrance: 85.0,
                brightness: 60.0,
                gamma: 1.3,
            },
            resolution: Some(DisplayMode {
                width: 1280,
                height: 720,
                refresh_rate: 60,
            }),
        }
    }
    fn foreground(id: &str, name: &str) -> ForegroundApp {
        ForegroundApp {
            exe_name: name.into(),
            exe_path: format!("C:\\{name}.exe"),
            display_id: id.into(),
            pid: 1,
        }
    }

    #[test]
    fn game_color_applies_to_all_displays_but_resolution_only_to_game_display() {
        let config = Config {
            profiles: vec![game()],
            ..Config::default()
        };
        let (_, plans) = plan(
            &config,
            &[display("one", true), display("two", false)],
            Some(&foreground("two", "game")),
        );
        assert_eq!(plans[0].color, game().color);
        assert_eq!(plans[0].resolution, None);
        assert_eq!(plans[1].resolution, game().resolution);
    }

    #[test]
    fn alt_tab_to_another_display_restores_desktop_and_resolution() {
        let config = Config {
            profiles: vec![game()],
            ..Config::default()
        };
        let (active, plans) = plan(
            &config,
            &[display("one", true), display("two", false)],
            Some(&foreground("one", "explorer")),
        );
        assert_eq!(active, None);
        assert!(
            plans
                .iter()
                .all(|p| p.color == config.desktop && p.resolution.is_none())
        );
    }

    #[test]
    fn primary_only_targets_os_primary_even_when_game_is_elsewhere() {
        let config = Config {
            settings: GlobalSettings {
                primary_only: true,
                ..GlobalSettings::default()
            },
            profiles: vec![game()],
            ..Config::default()
        };
        let (_, plans) = plan(
            &config,
            &[display("one", true), display("two", false)],
            Some(&foreground("two", "game")),
        );
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].id, "one");
        assert_eq!(plans[0].resolution, None);
    }

    #[test]
    fn resolution_transition_is_independent_of_vibrance_and_global_opt_out() {
        let mut profile = game();
        profile.color = ColorSettings::default();
        let mut config = Config {
            profiles: vec![profile],
            ..Config::default()
        };
        let (_, plans) = plan(
            &config,
            &[display("one", true)],
            Some(&foreground("one", "game")),
        );
        assert!(plans[0].resolution.is_some());
        config.settings.never_change_resolution = true;
        let (_, plans) = plan(
            &config,
            &[display("one", true)],
            Some(&foreground("one", "game")),
        );
        assert!(plans[0].resolution.is_none());
    }

    #[test]
    fn desktop_controls_apply_even_without_any_profiles_or_foreground_window() {
        let config = Config {
            desktop: ColorSettings {
                brightness: 72.0,
                ..ColorSettings::default()
            },
            ..Config::default()
        };
        let (_, plans) = plan(&config, &[display("one", true)], None);
        assert_eq!(plans[0].color.brightness, 72.0);
    }
}
