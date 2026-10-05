// Eyedropper glyph drawn on the 16-unit grid and stroke weight of the Gravity UI icons,
// which ship no color-picker symbol.
import type { SVGProps } from "react";

/** Pipette icon for picking a color from the preview image. */
export function EyedropperIcon(props: SVGProps<SVGSVGElement>) {
  return <svg xmlns="http://www.w3.org/2000/svg" width={16} height={16} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.5} strokeLinecap="round" strokeLinejoin="round" {...props}>
    <circle cx="11.75" cy="4.25" r="2" />
    <path d="M7.75 5.75l2.5 2.5" />
    <path d="M9 7 3.5 12.5 2.25 13.75" />
  </svg>;
}
