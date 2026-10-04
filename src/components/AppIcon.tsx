// Program artwork: the executable's own icon, or a soft monogram when Windows has none.
// Monogram tints derive from the name, so a program keeps its color between sessions.
import { Avatar } from "@heroui/react";
import type { CSSProperties } from "react";

interface AppIconProps {
  name: string;
  /** Icon data URL; undefined while loading (renders a quiet placeholder), null when absent. */
  src: string | null | undefined;
  size?: "sm" | "md" | "lg";
  className?: string;
}

function nameHue(name: string): number {
  let hash = 0;
  for (const char of name) hash = (hash * 31 + (char.codePointAt(0) ?? 0)) % 360;
  return hash;
}

/** Square program icon used by the program grid, the editor and the running-app picker. */
export function AppIcon({ name, src, size = "md", className = "" }: AppIconProps) {
  const initial = [...name.trim()][0]?.toLocaleUpperCase() ?? "?";
  const kind = src ? "image" : src === null ? "monogram" : "loading";
  return <Avatar aria-hidden="true" className={`app-icon app-icon--${size} app-icon--${kind} ${className}`} style={{ "--app-hue": nameHue(name) } as CSSProperties}>
    {src && <Avatar.Image src={src} alt="" draggable={false} />}
    <Avatar.Fallback className="app-icon__monogram">{kind === "monogram" ? initial : null}</Avatar.Fallback>
  </Avatar>;
}
