// Routes UI requests through Tauri without allowing browser previews to touch hardware.
// Preview state is deliberately ephemeral and visibly identified by the interface.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { DEFAULT_COLOR } from "./types";
import type { AppState, ColorSettings, Profile, RunningApp, Settings } from "./types";

/** True only outside the native webview; never indicates a connected GPU. */
export const isPreview = !isTauri();

/** The frameless native window driven by the custom title bar; null in the browser preview. */
export const appWindow = isPreview ? null : getCurrentWindow();

let previewState: AppState = {
  settings: { autostart: false, primaryOnly: false, neverChangeResolution: false },
  desktop: { ...DEFAULT_COLOR },
  profiles: [],
  status: {
    enabled: true,
    activeProfileId: null,
    gpuName: "Browser preview",
    supportsVibrance: true,
    supportsGamma: true,
    message: null,
  },
  resolutions: [
    { width: 2560, height: 1440, refreshRate: 144 },
    { width: 1920, height: 1080, refreshRate: 144 },
    { width: 1920, height: 1080, refreshRate: 60 },
  ],
};
const subscribers = new Set<(state: AppState) => void>();

function previewSnapshot(): AppState {
  return structuredClone(previewState);
}

function publishPreview(): AppState {
  const state = previewSnapshot();
  for (const subscriber of subscribers) subscriber(state);
  return state;
}

/** Native commands and matching preview operations; rejected commands surface as UI errors. */
export const api = {
  /** Reads the current persisted configuration and live observer capabilities. */
  async getState(): Promise<AppState> {
    return isPreview ? previewSnapshot() : invoke("get_state");
  },
  /** Persists global preferences; native validation failures reject the promise. */
  async saveSettings(settings: Settings): Promise<AppState> {
    if (!isPreview) return invoke("save_settings", { settings });
    previewState.settings = { ...settings };
    return publishPreview();
  },
  /** Persists the baseline and requests application when no profile is active. */
  async saveDesktop(color: ColorSettings): Promise<AppState> {
    if (!isPreview) return invoke("save_desktop", { color });
    previewState.desktop = { ...color };
    return publishPreview();
  },
  /** Creates or replaces a profile by ID after native validation succeeds. */
  async saveProfile(profile: Profile): Promise<AppState> {
    if (!isPreview) return invoke("save_profile", { profile });
    const index = previewState.profiles.findIndex((item) => item.id === profile.id);
    if (index < 0) previewState.profiles.push(structuredClone(profile));
    else previewState.profiles[index] = structuredClone(profile);
    return publishPreview();
  },
  /** Removes only the saved profile, never the executable it refers to. */
  async removeProfile(id: string): Promise<AppState> {
    if (!isPreview) return invoke("remove_profile", { id });
    previewState.profiles = previewState.profiles.filter((profile) => profile.id !== id);
    if (previewState.status.activeProfileId === id) previewState.status.activeProfileId = null;
    return publishPreview();
  },
  /** Pauses or resumes the observer; native pause restores original display state. */
  async setEnabled(enabled: boolean): Promise<AppState> {
    if (!isPreview) return invoke("set_enabled", { enabled });
    previewState.status.enabled = enabled;
    return publishPreview();
  },
  /** Lists visible native applications, or clearly labelled preview fixtures. */
  async listRunningApps(): Promise<RunningApp[]> {
    if (!isPreview) return invoke("list_running_apps");
    return [
      { name: "Example game", executablePath: "C:\\Preview\\ExampleGame.exe", pid: 100 },
      { name: "Example editor", executablePath: "C:\\Preview\\ExampleEditor.exe", pid: 101 },
      { name: "Example launcher", executablePath: "C:\\Preview\\Launcher\\ExampleLauncher.exe", pid: 102 },
    ];
  },
  /** Opens the native executable picker; cancellation resolves to null. */
  async pickExecutable(): Promise<Pick<RunningApp, "name" | "executablePath"> | null> {
    if (!isPreview) return invoke("pick_executable");
    return { name: "Example game", executablePath: "C:\\Preview\\ExampleGame.exe" };
  },
  /** Returns PNG data URLs aligned with `paths`; null marks files without a readable icon. */
  async executableIcons(paths: string[]): Promise<(string | null)[]> {
    if (!isPreview) return invoke("executable_icons", { paths });
    return paths.map(() => null);
  },
  /** Subscribes to snapshots and returns the cleanup function once listening starts. */
  async subscribe(callback: (state: AppState) => void): Promise<() => void> {
    if (!isPreview) return listen<AppState>("state-changed", (event) => callback(event.payload));
    subscribers.add(callback);
    return () => { subscribers.delete(callback); };
  },
};
