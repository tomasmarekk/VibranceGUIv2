//! Native display and process boundary used by the profile engine.
//! Percentage values are vendor independent; Windows adapters translate them to
//! driver ranges and retain the original state for pause and shutdown restoration.

use serde::{Deserialize, Serialize};

#[cfg(windows)]
mod executable;
#[cfg(windows)]
mod vendor;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use executable::{executable_description, executable_icon_png};
#[cfg(windows)]
pub(crate) use windows::{NativeController, display_modes, foreground_app, running_apps};

/// One selectable Windows mode; refresh rates are the integer values from GDI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh_rate: u32,
}

/// Active desktop display and capabilities actually reported by its driver.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DisplayInfo {
    pub id: String,
    pub name: String,
    pub adapter: String,
    pub primary: bool,
    pub vibrance_supported: bool,
    pub gamma_supported: bool,
    pub current_mode: DisplayMode,
}

/// Foreground executable and the desktop display containing its window.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ForegroundApp {
    pub exe_name: String,
    pub exe_path: String,
    pub display_id: String,
    pub pid: u32,
}

/// A visible, titled application window that can be added to a profile.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunningApp {
    pub exe_name: String,
    pub exe_path: String,
    pub title: String,
    pub pid: u32,
}

/// A native failure suitable for conversion at the command boundary.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PlatformError {
    #[error("invalid {0}")]
    InvalidValue(&'static str),
    #[error("display is no longer connected: {0}")]
    DisplayMissing(String),
    #[error("{operation} failed: {source}")]
    Windows {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("{vendor} {operation} failed with driver status {status}")]
    Driver {
        vendor: &'static str,
        operation: &'static str,
        status: i32,
    },
    #[error("{0}")]
    Unavailable(String),
    #[error("{0}")]
    Partial(String),
}

type GammaRamp = [[u16; 256]; 3];

fn validate_adjustments(vibrance: f64, brightness: f64, gamma: f64) -> Result<(), PlatformError> {
    if !vibrance.is_finite() || !(0.0..=100.0).contains(&vibrance) {
        return Err(PlatformError::InvalidValue(
            "digital vibrance (expected 0–100)",
        ));
    }
    if !brightness.is_finite() || !(0.0..=100.0).contains(&brightness) {
        return Err(PlatformError::InvalidValue("brightness (expected 0–100)"));
    }
    if !gamma.is_finite() || !(0.5..=3.0).contains(&gamma) {
        return Err(PlatformError::InvalidValue("gamma (expected 0.5–3.0)"));
    }
    Ok(())
}

fn compose_gamma(baseline: &GammaRamp, brightness: f64, gamma: f64) -> GammaRamp {
    if brightness == 50.0 && gamma == 1.0 {
        return *baseline;
    }
    let gain = 2.0_f64.powf((brightness - 50.0) / 50.0);
    baseline.map(|channel| {
        channel.map(|sample| {
            let corrected = (f64::from(sample) / 65535.0).powf(1.0 / gamma) * gain;
            // GDI requires unsigned 16-bit samples; clamp before the deliberate rounding cast.
            (corrected.clamp(0.0, 1.0) * 65535.0).round() as u16
        })
    })
}

fn driver_level(percent: f64, min: i32, neutral: i32, max: i32, step: i32) -> i32 {
    let neutral = neutral.clamp(min, max);
    let level = if percent <= 50.0 {
        f64::from(min) + (f64::from(neutral) - f64::from(min)) * percent / 50.0
    } else {
        f64::from(neutral) + (f64::from(max) - f64::from(neutral)) * (percent - 50.0) / 50.0
    };
    let step = f64::from(step.max(1));
    let snapped = f64::from(min) + ((level - f64::from(min)) / step).round() * step;
    // The validated driver bounds are i32, so the clamped result cannot overflow.
    snapped.clamp(f64::from(min), f64::from(max)).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calibrated_ramp() -> GammaRamp {
        std::array::from_fn(|channel| {
            std::array::from_fn(|index| u16::try_from(index * (255 - channel * 20)).unwrap())
        })
    }

    #[test]
    fn neutral_preserves_each_calibrated_channel_exactly() {
        let ramp = calibrated_ramp();
        assert_eq!(compose_gamma(&ramp, 50.0, 1.0), ramp);
    }

    #[test]
    fn brightness_and_gamma_are_monotonic_and_retain_black() {
        let ramp = calibrated_ramp();
        for brightness in [0.0, 25.0, 50.0, 100.0] {
            for gamma in [0.5, 1.0, 2.2, 3.0] {
                let adjusted = compose_gamma(&ramp, brightness, gamma);
                for channel in adjusted {
                    assert_eq!(channel[0], 0);
                    assert!(channel.windows(2).all(|pair| pair[0] <= pair[1]));
                }
            }
        }
    }

    #[test]
    fn gamma_above_one_lifts_midtones_and_brightness_is_independent() {
        let ramp = calibrated_ramp();
        assert!(compose_gamma(&ramp, 50.0, 2.0)[0][128] > ramp[0][128]);
        assert!(compose_gamma(&ramp, 50.0, 0.5)[0][128] < ramp[0][128]);
        assert!(compose_gamma(&ramp, 25.0, 1.0)[0][128] < ramp[0][128]);
        assert!(compose_gamma(&ramp, 75.0, 1.0)[0][128] > ramp[0][128]);
    }

    #[test]
    fn vendor_scaling_preserves_asymmetric_defaults_and_step() {
        assert_eq!(driver_level(0.0, 0, 100, 200, 1), 0);
        assert_eq!(driver_level(50.0, 0, 100, 200, 1), 100);
        assert_eq!(driver_level(100.0, 0, 100, 200, 1), 200);
        assert_eq!(driver_level(75.0, -50, 0, 100, 1), 50);
        assert_eq!(driver_level(51.0, 0, 100, 200, 5), 100);
        assert_eq!(driver_level(100.0, i32::MIN, 0, i32::MAX, 1), i32::MAX);
    }

    #[test]
    fn invalid_inputs_are_rejected_before_touching_drivers() {
        for value in [f64::NAN, f64::INFINITY, -1.0, 101.0] {
            assert!(validate_adjustments(value, 50.0, 1.0).is_err());
            assert!(validate_adjustments(50.0, value, 1.0).is_err());
        }
        for value in [f64::NAN, f64::INFINITY, 0.49, 3.01] {
            assert!(validate_adjustments(50.0, 50.0, value).is_err());
        }
        assert!(validate_adjustments(0.0, 100.0, 3.0).is_ok());
    }
}
