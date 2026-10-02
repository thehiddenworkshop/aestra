//! Viewer CLI validation and additive schema-1 capture metadata.
pub use aestra_bevy::preview::{DisplayTransform, PhotographicPreview};
use serde::Serialize;

pub fn bounded_number(value: &str, option: &str, min: f32, max: f32) -> Result<f32, String> {
    let number = value.parse::<f32>().ok();
    number
        .filter(|v| v.is_finite() && (min..=max).contains(v))
        .ok_or_else(|| format!("{option} requires a finite number in {min}..={max}"))
}

/// Additive report metadata; absence in older schema-1 reports means the legacy camera defaults.
#[derive(Serialize)]
pub struct CaptureResponse {
    hdr: bool,
    exposure_stops: f32,
    tonemapping: Option<DisplayTransform>,
    bloom_intensity: f32,
    bloom_preset: &'static str,
    deband_dither: &'static str,
    sprite_minimum_pixels: f32,
    trail_minimum_pixels: f32,
}

impl CaptureResponse {
    pub fn new(settings: Option<PhotographicPreview>, sprite_minimum_pixels: f32) -> Self {
        let settings = settings.map(PhotographicPreview::normalized);
        Self {
            trail_minimum_pixels: 0.0,
            sprite_minimum_pixels: aestra_bevy::SpriteSampling {
                minimum_pixels: sprite_minimum_pixels,
            }
            .normalized()
            .minimum_pixels,
            hdr: settings.is_some(),
            exposure_stops: settings.map_or(0.0, |s| s.exposure_stops),
            tonemapping: settings.map(|s| s.tonemapping),
            bloom_intensity: settings.map_or(0.0, |s| s.bloom_intensity),
            bloom_preset: if settings.is_some_and(|s| s.bloom_intensity > 0.0) {
                "natural_energy_conserving_threshold_zero"
            } else {
                "disabled"
            },
            deband_dither: if settings.is_some() {
                "disabled"
            } else {
                "camera_default"
            },
        }
    }

    pub fn with_trail_sampling(mut self, minimum_pixels: f32) -> Self {
        self.trail_minimum_pixels = aestra_bevy::TrailRasterSampling { minimum_pixels }
            .normalized()
            .minimum_pixels;
        self
    }
}
