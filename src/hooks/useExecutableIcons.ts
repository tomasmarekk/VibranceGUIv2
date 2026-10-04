// Session-wide cache of executable icons extracted by the native layer.
// Each path is requested once; missing files and failures are remembered as null.
import { useEffect, useSyncExternalStore } from "react";
import { api } from "../api";

const icons = new Map<string, string | null>();
const requested = new Set<string>();
const listeners = new Set<() => void>();
let version = 0;

const cacheKey = (path: string) => path.toLocaleLowerCase();

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

function publish() {
  version += 1;
  for (const listener of listeners) listener();
}

function request(paths: readonly string[]) {
  const missing = [...new Set(paths)].filter((path) => !requested.has(cacheKey(path)));
  if (missing.length === 0) return;
  for (const path of missing) requested.add(cacheKey(path));
  void api.executableIcons(missing)
    .then((result) => missing.forEach((path, index) => icons.set(cacheKey(path), result[index] ?? null)))
    .catch(() => missing.forEach((path) => icons.set(cacheKey(path), null)))
    .finally(publish);
}

/**
 * Loads icons for `paths` in the background and returns a lookup for any cached path.
 * The lookup yields undefined while an icon is loading and null when none exists.
 */
export function useExecutableIcons(paths: readonly string[]): (path: string) => string | null | undefined {
  useSyncExternalStore(subscribe, () => version);
  const signature = paths.join("\n");
  useEffect(() => { request(signature ? signature.split("\n") : []); }, [signature]);
  return (path) => icons.get(cacheKey(path));
}
