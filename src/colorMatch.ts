// Preview copy of the native color math: the gamma-ramp tone curve (platform.rs) and the
// color-rule overlay (color_match.rs, overlay.hlsl). Keep the constants and formulas in
// step with those files so the preview shows what the game will show.
import type { BlackEqualizer, ColorRule, ColorSettings } from "./types";

/** Match radius in OKLab units at zero tolerance. */
const MIN_RADIUS = 0.02;
/** Additional radius at full tolerance. */
const RADIUS_SPAN = 0.28;
/** Lightness differences count far less than chromaticity, so shaded edges still match. */
export const LIGHTNESS_WEIGHT = 0.15;
/** Floor for divisions by lightness, keeping near-black chromaticity stable. */
export const MIN_LIGHTNESS = 0.05;
/** Brightest shade of the target a pixel can map to, relative to the source. */
export const MAX_SHADE = 1.5;
/** OKLab lightness change at ±100 rule brightness. */
const LIGHTNESS_SPAN = 0.25;

export type Rgb = [number, number, number];
export type Lab = [number, number, number];

/** Parses `#RRGGBB` (case-insensitive) into 0–255 channels, or null. */
export function parseHex(value: string): Rgb | null {
  const match = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(value.trim());
  return match ? [parseInt(match[1], 16), parseInt(match[2], 16), parseInt(match[3], 16)] : null;
}

/** Formats 0–255 channels as uppercase `#RRGGBB`. */
export function toHex([red, green, blue]: Rgb): string {
  return `#${[red, green, blue].map((channel) => Math.round(channel).toString(16).padStart(2, "0")).join("").toUpperCase()}`;
}

export function srgbToLinear(value: number): number {
  return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
}

export function linearToSrgb(value: number): number {
  return value <= 0.0031308 ? value * 12.92 : 1.055 * value ** (1 / 2.4) - 0.055;
}

export function linearToOklab([red, green, blue]: Rgb): Lab {
  const long = Math.cbrt(0.4122214708 * red + 0.5363325363 * green + 0.0514459929 * blue);
  const medium = Math.cbrt(0.2119034982 * red + 0.6806995451 * green + 0.1073969566 * blue);
  const short = Math.cbrt(0.0883024619 * red + 0.2817188376 * green + 0.6299787005 * blue);
  return [
    0.2104542553 * long + 0.793617785 * medium - 0.0040720468 * short,
    1.9779984951 * long - 2.428592205 * medium + 0.4505937099 * short,
    0.0259040371 * long + 0.7827717662 * medium - 0.808675766 * short,
  ];
}

export function oklabToLinear([lightness, a, b]: Lab): Rgb {
  const long = (lightness + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const medium = (lightness - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const short = (lightness - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * long - 3.3077115913 * medium + 0.2309699292 * short,
    -1.2684380046 * long + 2.6097574011 * medium - 0.3413193965 * short,
    -0.0041960863 * long - 0.7034186147 * medium + 1.707614701 * short,
  ];
}

/** OKLab coordinates of 0–255 sRGB channels. */
export function rgbToOklab(rgb: Rgb): Lab {
  return linearToOklab(rgb.map((channel) => srgbToLinear(channel / 255)) as Rgb);
}

/** One active rule in the form the shaders consume. */
export interface RuleParams {
  source: Lab;
  target: Lab;
  radius: number;
  strength: number;
  chroma: number;
  lightness: number;
}

/** Converts the rules that draw something; invalid colors are skipped. */
export function ruleParams(rules: readonly ColorRule[]): RuleParams[] {
  return rules.flatMap((rule) => {
    const source = parseHex(rule.source);
    const target = parseHex(rule.target);
    if (!rule.enabled || rule.strength <= 0 || !source || !target) return [];
    return [{
      source: rgbToOklab(source),
      target: rgbToOklab(target),
      radius: MIN_RADIUS + RADIUS_SPAN * rule.tolerance / 100,
      strength: rule.strength / 100,
      chroma: 1 + rule.saturation / 100,
      lightness: LIGHTNESS_SPAN * rule.brightness / 100,
    }];
  });
}

function smoothstep(edge0: number, edge1: number, value: number): number {
  const t = Math.min(1, Math.max(0, (value - edge0) / (edge1 - edge0)));
  return t * t * (3 - 2 * t);
}

/** How strongly a pixel belongs to a rule: 1 inside half the radius, fading to 0 at it. */
export function matchWeight(lab: Lab, rule: RuleParams): number {
  const sourceLightness = Math.max(rule.source[0], MIN_LIGHTNESS);
  const pixelLightness = Math.max(lab[0], MIN_LIGHTNESS);
  const a = (lab[1] / pixelLightness - rule.source[1] / sourceLightness) * sourceLightness;
  const b = (lab[2] / pixelLightness - rule.source[2] / sourceLightness) * sourceLightness;
  const lightness = (lab[0] - rule.source[0]) * LIGHTNESS_WEIGHT;
  return 1 - smoothstep(rule.radius * 0.5, rule.radius, Math.hypot(lightness, a, b));
}

/** Recolored pixel (0–1 sRGB) and its opacity, exactly as the overlay shader draws it. */
export function recolor(rgb: Rgb, rules: readonly RuleParams[]): { color: Rgb; opacity: number } {
  const lab = rgbToOklab(rgb);
  let best = 0;
  let color: Rgb = [0, 0, 0];
  let opacity = 0;
  for (const rule of rules) {
    const weight = matchWeight(lab, rule);
    if (weight > best) {
      best = weight;
      const shade = Math.min(MAX_SHADE, Math.max(0, lab[0] / Math.max(rule.source[0], MIN_LIGHTNESS)));
      const shifted: Lab = [rule.target[0] * shade + rule.lightness, rule.target[1] * shade * rule.chroma, rule.target[2] * shade * rule.chroma];
      color = oklabToLinear(shifted).map((channel) => linearToSrgb(Math.min(1, Math.max(0, channel)))) as Rgb;
      opacity = weight * rule.strength;
    }
  }
  return { color, opacity };
}

/** Black-equalizer curve on a 0–1 tone (platform.rs `black_lift`). */
export function blackLift(value: number, strength: number, range: number): number {
  const reach = 0.1 + 0.5 * range / 100;
  if (strength <= 0 || value >= reach) return value;
  const remaining = 1 - value / reach;
  return value + 3 * strength / 100 * value * remaining * remaining;
}

/** Gamma-ramp value for a 0–1 input tone with an identity calibration (platform.rs `compose_gamma`). */
export function toneCurve(value: number, color: ColorSettings, black: BlackEqualizer): number {
  const gain = 2 ** ((color.brightness - 50) / 50);
  const corrected = Math.min(1, Math.max(0, value ** (1 / color.gamma) * gain));
  return Math.min(1, Math.max(0, blackLift(corrected, black.strength, black.range)));
}

/**
 * Chroma multiplier used to approximate digital vibrance in the preview. Drivers do not
 * publish their saturation math; 50 is neutral, 0 is gray and 100 roughly doubles chroma.
 */
export function vibranceChroma(vibrance: number): number {
  return vibrance <= 50 ? vibrance / 50 : 1 + (vibrance - 50) / 50;
}

/** WebGL 2 fragment shader for the preview: rules, then approximate vibrance, then tone. */
export const PREVIEW_FRAGMENT_SHADER = `#version 300 es
precision highp float;
const int MAX_RULES = 4;
const float LIGHTNESS_WEIGHT = ${LIGHTNESS_WEIGHT.toFixed(4)};
const float MIN_LIGHTNESS = ${MIN_LIGHTNESS.toFixed(4)};
const float MAX_SHADE = ${MAX_SHADE.toFixed(4)};
uniform sampler2D uImage;
uniform bool uOriginal;
uniform int uRuleCount;
uniform vec3 uSource[MAX_RULES];
uniform vec3 uTarget[MAX_RULES];
uniform vec4 uParams[MAX_RULES];
uniform float uVibrance;
uniform vec4 uTone; // x: gamma, y: gain, z: black strength, w: black range
in vec2 vUv;
out vec4 outColor;

vec3 srgbToLinear(vec3 c) { return mix(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), step(0.04045, c)); }
vec3 linearToSrgb(vec3 c) { return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(0.0031308, c)); }
vec3 linearToOklab(vec3 c) {
  vec3 lms = vec3(
    0.4122214708 * c.r + 0.5363325363 * c.g + 0.0514459929 * c.b,
    0.2119034982 * c.r + 0.6806995451 * c.g + 0.1073969566 * c.b,
    0.0883024619 * c.r + 0.2817188376 * c.g + 0.6299787005 * c.b);
  lms = pow(max(lms, 0.0), vec3(1.0 / 3.0));
  return vec3(
    0.2104542553 * lms.x + 0.7936177850 * lms.y - 0.0040720468 * lms.z,
    1.9779984951 * lms.x - 2.4285922050 * lms.y + 0.4505937099 * lms.z,
    0.0259040371 * lms.x + 0.7827717662 * lms.y - 0.8086757660 * lms.z);
}
vec3 oklabToLinear(vec3 lab) {
  vec3 lms = vec3(
    lab.x + 0.3963377774 * lab.y + 0.2158037573 * lab.z,
    lab.x - 0.1055613458 * lab.y - 0.0638541728 * lab.z,
    lab.x - 0.0894841775 * lab.y - 1.2914855480 * lab.z);
  lms = lms * lms * lms;
  return vec3(
    4.0767416621 * lms.x - 3.3077115913 * lms.y + 0.2309699292 * lms.z,
    -1.2684380046 * lms.x + 2.6097574011 * lms.y - 0.3413193965 * lms.z,
    -0.0041960863 * lms.x - 0.7034186147 * lms.y + 1.7076147010 * lms.z);
}
float blackLift(float value) {
  float reach = 0.1 + 0.5 * uTone.w / 100.0;
  if (uTone.z <= 0.0 || value >= reach) return value;
  float remaining = 1.0 - value / reach;
  return value + 3.0 * uTone.z / 100.0 * value * remaining * remaining;
}
float tone(float value) {
  float corrected = clamp(pow(value, 1.0 / uTone.x) * uTone.y, 0.0, 1.0);
  return clamp(blackLift(corrected), 0.0, 1.0);
}

void main() {
  vec3 srgb = texture(uImage, vUv).rgb;
  if (uOriginal) { outColor = vec4(srgb, 1.0); return; }
  vec3 lab = linearToOklab(srgbToLinear(srgb));
  float best = 0.0;
  float opacity = 0.0;
  vec3 adjusted = lab;
  for (int i = 0; i < MAX_RULES; i++) {
    if (i >= uRuleCount) break;
    vec3 source = uSource[i];
    float sourceLightness = max(source.x, MIN_LIGHTNESS);
    vec2 chroma = (lab.yz / max(lab.x, MIN_LIGHTNESS) - source.yz / sourceLightness) * sourceLightness;
    float lightness = (lab.x - source.x) * LIGHTNESS_WEIGHT;
    float radius = uParams[i].x;
    float weight = 1.0 - smoothstep(radius * 0.5, radius, length(vec3(lightness, chroma)));
    if (weight > best) {
      best = weight;
      float shade = clamp(lab.x / sourceLightness, 0.0, MAX_SHADE);
      vec3 shaded = uTarget[i] * shade;
      adjusted = vec3(shaded.x + uParams[i].w, shaded.yz * uParams[i].z);
      opacity = weight * uParams[i].y;
    }
  }
  vec3 overlay = linearToSrgb(clamp(oklabToLinear(adjusted), 0.0, 1.0));
  vec3 composed = mix(srgb, overlay, opacity);
  vec3 vivid = linearToOklab(srgbToLinear(composed));
  vivid.yz *= uVibrance;
  vec3 shown = linearToSrgb(clamp(oklabToLinear(vivid), 0.0, 1.0));
  outColor = vec4(tone(shown.r), tone(shown.g), tone(shown.b), 1.0);
}
`;

/** Full-screen triangle for the preview. */
export const PREVIEW_VERTEX_SHADER = `#version 300 es
out vec2 vUv;
void main() {
  vec2 corner = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  // The triangle spans twice the viewport, so UV 0–1 lands exactly on the canvas;
  // the top-left corner samples the first uploaded image row, so no flip is needed.
  vUv = corner;
  gl_Position = vec4(corner * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.0, 1.0);
}
`;
