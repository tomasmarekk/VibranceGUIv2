// Presentation helpers shared by the main window, the program editor and the app picker.
// Values follow the IPC contract in types.ts; nothing here changes stored data.
import { DEFAULT_COLOR } from "./types";
import type { ColorSettings, Resolution } from "./types";

/** Returns the last segment of a Windows or POSIX path. */
export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/** Windows paths are case-insensitive, so profiles compare them the same way. */
export function samePath(left: string, right: string): boolean {
  return left.toLocaleLowerCase() === right.toLocaleLowerCase();
}

/** Formats a percentage control value without a decimal part. */
export function formatPercent(value: number): string {
  return `${Math.round(value)}%`;
}

/** Formats gamma with the two decimals its 0.05 step needs. */
export function formatGamma(value: number): string {
  return value.toFixed(2);
}

/** Stable select key for a display mode. */
export function resolutionKey(mode: Resolution): string {
  return `${mode.width}x${mode.height}@${mode.refreshRate}`;
}

/** Human-readable display mode, for example "2560 × 1440 · 144 Hz". */
export function formatResolution(mode: Resolution): string {
  return `${mode.width} × ${mode.height} · ${mode.refreshRate} Hz`;
}

/** True when every control is at its neutral value, so the profile changes nothing. */
export function isNeutral(color: ColorSettings): boolean {
  return color.vibrance === DEFAULT_COLOR.vibrance && color.brightness === DEFAULT_COLOR.brightness && color.gamma === DEFAULT_COLOR.gamma;
}
