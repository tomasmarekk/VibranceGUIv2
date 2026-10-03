// The public IPC contract shared with the Rust command layer.
// Color values remain driver-independent; Rust owns validation and hardware conversion.

/** Driver-independent color controls: 0–100 vibrance/brightness, 0.5–3 gamma. */
export interface ColorSettings {
  vibrance: number;
  brightness: number;
  gamma: number;
}

/** A display mode selected from modes advertised by the primary monitor. */
export interface Resolution {
  width: number;
  height: number;
  refreshRate: number;
}

/** Persisted profile; executable-name matching is the default legacy behavior. */
export interface Profile {
  id: string;
  name: string;
  executablePath: string;
  matchByPath: boolean;
  color: ColorSettings;
  resolution: Resolution | null;
}

/** Global observer and Windows launch preferences, persisted by the native layer. */
export interface Settings {
  autostart: boolean;
  primaryOnly: boolean;
  neverChangeResolution: boolean;
}

/** A real foreground-capable process returned by native enumeration. */
export interface RunningApp {
  name: string;
  executablePath: string;
  pid: number;
}

/** Authoritative application snapshot, also delivered on the state-changed event. */
export interface AppState {
  settings: Settings;
  desktop: ColorSettings;
  profiles: Profile[];
  status: {
    enabled: boolean;
    activeProfileId: string | null;
    gpuName: string;
    supportsVibrance: boolean;
    supportsGamma: boolean;
    message: string | null;
  };
  resolutions: Resolution[];
}

/** Neutral settings used by new profiles and the explicit reset action. */
export const DEFAULT_COLOR: Readonly<ColorSettings> = { vibrance: 50, brightness: 50, gamma: 1 };
