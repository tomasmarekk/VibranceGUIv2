//! Color-rule math shared by the overlay shader constants and its CPU reference.
//! Matching compares OKLab chromaticity (a/L, b/L), which stays constant when a color
//! is only shaded darker, so a whole outline matches including its darker edges while
//! duller scenery of a similar hue does not. `overlay.hlsl` and `src/colorMatch.ts`
//! mirror these formulas; change all three together.

use crate::settings::{ColorRule, parse_hex_color};

/// Match radius in OKLab units at zero tolerance; a little above exact-match noise.
const MIN_RADIUS: f64 = 0.02;
/// Additional radius at full tolerance, wide enough to take in a whole hue family.
const RADIUS_SPAN: f64 = 0.28;
#[cfg(test)]
/// Lightness differences count far less than chromaticity, so shaded edges still match.
const LIGHTNESS_WEIGHT: f64 = 0.15;
#[cfg(test)]
/// Floor for divisions by lightness, keeping near-black chromaticity stable.
const MIN_LIGHTNESS: f64 = 0.05;
#[cfg(test)]
/// Brightest shade of the target a pixel can map to, relative to the source.
const MAX_SHADE: f64 = 1.5;
/// OKLab lightness change at ±100 brightness.
const LIGHTNESS_SPAN: f64 = 0.25;

/// Decodes one sRGB-encoded channel in 0–1 to linear light.
pub(crate) fn srgb_to_linear(value: f64) -> f64 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
/// Encodes one linear channel in 0–1 as sRGB.
pub(crate) fn linear_to_srgb(value: f64) -> f64 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

/// Converts linear sRGB to OKLab (Björn Ottosson's reference matrices).
pub(crate) fn linear_to_oklab([red, green, blue]: [f64; 3]) -> [f64; 3] {
    let long = (0.412_221_470_8 * red + 0.536_332_536_3 * green + 0.051_445_992_9 * blue).cbrt();
    let medium = (0.211_903_498_2 * red + 0.680_699_545_1 * green + 0.107_396_956_6 * blue).cbrt();
    let short = (0.088_302_461_9 * red + 0.281_718_837_6 * green + 0.629_978_700_5 * blue).cbrt();
    [
        0.210_454_255_3 * long + 0.793_617_785 * medium - 0.004_072_046_8 * short,
        1.977_998_495_1 * long - 2.428_592_205 * medium + 0.450_593_709_9 * short,
        0.025_904_037_1 * long + 0.782_771_766_2 * medium - 0.808_675_766 * short,
    ]
}

#[cfg(test)]
/// Converts OKLab back to linear sRGB; out-of-gamut results are left for the caller to clamp.
pub(crate) fn oklab_to_linear([lightness, a, b]: [f64; 3]) -> [f64; 3] {
    let long = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let medium = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let short = (lightness - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
    [
        4.076_741_662_1 * long - 3.307_711_591_3 * medium + 0.230_969_929_2 * short,
        -1.268_438_004_6 * long + 2.609_757_401_1 * medium - 0.341_319_396_5 * short,
        -0.004_196_086_3 * long - 0.703_418_614_7 * medium + 1.707_614_701 * short,
    ]
}

/// OKLab coordinates of an sRGB byte triple.
pub(crate) fn rgb_to_oklab(rgb: [u8; 3]) -> [f64; 3] {
    linear_to_oklab(rgb.map(|channel| srgb_to_linear(f64::from(channel) / 255.0)))
}

/// One enabled rule in the form the shader consumes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RuleParams {
    /// OKLab color to find.
    pub source: [f64; 3],
    /// OKLab color matched pixels take on, shaded like the pixel.
    pub target: [f64; 3],
    /// Match radius; pixels beyond it are untouched.
    pub radius: f64,
    /// Opacity of the change, 0–1.
    pub strength: f64,
    /// Chroma multiplier applied after the shift.
    pub chroma: f64,
    /// Lightness offset applied after the shift.
    pub lightness: f64,
}

/// Converts the rules that draw something; invalid colors are skipped.
pub(crate) fn rule_params(rules: &[ColorRule]) -> Vec<RuleParams> {
    rules
        .iter()
        .filter(|rule| rule.is_active())
        .filter_map(|rule| {
            Some(RuleParams {
                source: rgb_to_oklab(parse_hex_color(&rule.source)?),
                target: rgb_to_oklab(parse_hex_color(&rule.target)?),
                radius: MIN_RADIUS + RADIUS_SPAN * rule.tolerance / 100.0,
                strength: rule.strength / 100.0,
                chroma: 1.0 + rule.saturation / 100.0,
                lightness: LIGHTNESS_SPAN * rule.brightness / 100.0,
            })
        })
        .collect()
}

#[cfg(test)]
fn smoothstep(edge0: f64, edge1: f64, value: f64) -> f64 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
/// How strongly a pixel belongs to a rule: 1 inside half the radius, fading to 0 at it.
pub(crate) fn match_weight(lab: [f64; 3], rule: &RuleParams) -> f64 {
    let source_lightness = rule.source[0].max(MIN_LIGHTNESS);
    let pixel_lightness = lab[0].max(MIN_LIGHTNESS);
    // Chromaticity differences are scaled back to the source lightness so the radius
    // keeps OKLab-like units.
    let a = (lab[1] / pixel_lightness - rule.source[1] / source_lightness) * source_lightness;
    let b = (lab[2] / pixel_lightness - rule.source[2] / source_lightness) * source_lightness;
    let lightness = (lab[0] - rule.source[0]) * LIGHTNESS_WEIGHT;
    let distance = (lightness * lightness + a * a + b * b).sqrt();
    1.0 - smoothstep(rule.radius * 0.5, rule.radius, distance)
}

#[cfg(test)]
/// CPU reference of the overlay pixel shader: the recolored sRGB pixel (0–1) and its
/// opacity. The best-matching rule wins; unmatched pixels return zero opacity.
pub(crate) fn recolor(rgb: [u8; 3], rules: &[RuleParams]) -> ([f64; 3], f64) {
    let lab = rgb_to_oklab(rgb);
    let mut best = 0.0;
    let mut color = [0.0; 3];
    let mut opacity = 0.0;
    for rule in rules {
        let weight = match_weight(lab, rule);
        if weight > best {
            best = weight;
            // The target takes on the shading of the pixel relative to the source.
            let shade = (lab[0] / rule.source[0].max(MIN_LIGHTNESS)).clamp(0.0, MAX_SHADE);
            let shifted = [
                rule.target[0] * shade + rule.lightness,
                rule.target[1] * shade * rule.chroma,
                rule.target[2] * shade * rule.chroma,
            ];
            color = oklab_to_linear(shifted).map(|channel| linear_to_srgb(channel.clamp(0.0, 1.0)));
            opacity = weight * rule.strength;
        }
    }
    (color, opacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(source: &str, target: &str, tolerance: f64) -> ColorRule {
        ColorRule {
            id: "rule".into(),
            enabled: true,
            source: source.into(),
            target: target.into(),
            tolerance,
            strength: 100.0,
            saturation: 0.0,
            brightness: 0.0,
        }
    }

    #[test]
    fn oklab_round_trips_and_matches_reference_values() {
        let white = rgb_to_oklab([255, 255, 255]);
        assert!((white[0] - 1.0).abs() < 1e-4 && white[1].abs() < 1e-4 && white[2].abs() < 1e-4);
        // Reference value for the in-game yellow outline used by the frontend tests.
        let yellow = rgb_to_oklab([254, 254, 57]);
        for (actual, expected) in yellow.iter().zip([0.966_449, -0.066_179, 0.185_985]) {
            assert!((actual - expected).abs() < 1e-5, "{yellow:?}");
        }
        for rgb in [[0, 0, 0], [254, 254, 57], [12, 200, 90], [210, 180, 140]] {
            let back =
                oklab_to_linear(rgb_to_oklab(rgb)).map(|c| (linear_to_srgb(c) * 255.0).round());
            assert_eq!(back, rgb.map(f64::from));
        }
    }

    #[test]
    fn outline_yellow_matches_while_beige_scenery_does_not() {
        let params = rule_params(&[rule("#FEFE39", "#FF00FF", 30.0)]);
        let (color, opacity) = recolor([254, 254, 57], &params);
        assert!((opacity - 1.0).abs() < 1e-9);
        assert_eq!(color.map(|c| (c * 255.0).round()), [255.0, 0.0, 255.0]);
        assert!(
            recolor([250, 248, 70], &params).1 > 0.9,
            "near-identical yellow"
        );
        assert!(
            recolor([150, 150, 30], &params).1 > 0.5,
            "shaded outline edge"
        );
        assert_eq!(recolor([216, 196, 160], &params).1, 0.0, "beige wall");
        assert_eq!(recolor([224, 128, 64], &params).1, 0.0, "orange floor line");
        assert_eq!(recolor([255, 255, 255], &params).1, 0.0, "white");
        assert_eq!(recolor([20, 20, 22], &params).1, 0.0, "near black");
    }

    #[test]
    fn disabled_rules_and_zero_strength_draw_nothing() {
        let mut disabled = rule("#FEFE39", "#FF00FF", 30.0);
        disabled.enabled = false;
        let mut invisible = rule("#FEFE39", "#FF00FF", 30.0);
        invisible.strength = 0.0;
        assert!(rule_params(&[disabled, invisible]).is_empty());
    }

    #[test]
    fn saturation_and_brightness_change_only_matched_pixels() {
        let mut highlight = rule("#C8A020", "#C8A020", 20.0);
        highlight.saturation = 50.0;
        highlight.brightness = 40.0;
        let params = rule_params(&[highlight]);
        let (color, opacity) = recolor([200, 160, 32], &params);
        assert!(opacity > 0.99);
        let original = rgb_to_oklab([200, 160, 32]);
        let adjusted = linear_to_oklab(color.map(srgb_to_linear));
        assert!(adjusted[0] > original[0]);
        assert!(adjusted[1].hypot(adjusted[2]) > original[1].hypot(original[2]));
        assert_eq!(recolor([40, 60, 200], &params).1, 0.0);
    }
}
