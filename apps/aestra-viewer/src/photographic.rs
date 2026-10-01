//! Opt-in, fixed photographic response. This is host presentation, not simulation state.
use bevy::{
    camera::Hdr,
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    post_process::bloom::Bloom,
    prelude::*,
    render::view::ColorGrading,
};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayTransform {
    Tony,
    Aces,
    Reinhard,
}

impl DisplayTransform {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "tony" => Ok(Self::Tony),
            "aces" => Ok(Self::Aces),
            "reinhard" => Ok(Self::Reinhard),
            _ => Err("--tonemapping requires tony, aces or reinhard".into()),
        }
    }

    fn component(self) -> Tonemapping {
        match self {
            Self::Tony => Tonemapping::TonyMcMapface,
            Self::Aces => Tonemapping::AcesFitted,
            Self::Reinhard => Tonemapping::Reinhard,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhotographicPreview {
    pub exposure_stops: f32,
    pub tonemapping: DisplayTransform,
    pub bloom_intensity: f32,
}

impl Default for PhotographicPreview {
    fn default() -> Self {
        Self {
            exposure_stops: 0.0,
            tonemapping: DisplayTransform::Tony,
            bloom_intensity: Bloom::NATURAL.intensity,
        }
    }
}

impl PhotographicPreview {
    pub fn apply(self, camera: &mut EntityCommands) {
        // Camera Exposure is consumed by PBR shaders, not Aestra's unlit shaders. ColorGrading
        // applies relative stops to the entire HDR scene in the display transform instead.
        let mut grading = ColorGrading::default();
        grading.global.exposure = self.exposure_stops;
        camera.insert((
            Hdr,
            grading,
            self.tonemapping.component(),
            DebandDither::Disabled,
        ));
        if self.bloom_intensity > 0.0 {
            camera.insert(Bloom {
                intensity: self.bloom_intensity,
                ..Bloom::NATURAL
            });
        } else {
            camera.remove::<Bloom>();
        }
    }
}

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
}

impl CaptureResponse {
    pub fn new(settings: Option<PhotographicPreview>) -> Self {
        Self {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_response_is_attached_only_to_the_requested_camera() {
        let mut world = World::new();
        let photo = world.spawn(Camera3d::default()).id();
        let legacy = world.spawn(Camera3d::default()).id();
        let mut commands = world.commands();
        PhotographicPreview {
            exposure_stops: 2.0,
            ..default()
        }
        .apply(&mut commands.entity(photo));
        world.flush();
        assert!(world.get::<Hdr>(photo).is_some());
        assert_eq!(
            world.get::<ColorGrading>(photo).unwrap().global.exposure,
            2.0
        );
        assert_eq!(
            *world.get::<Tonemapping>(photo).unwrap(),
            Tonemapping::TonyMcMapface
        );
        assert_eq!(
            *world.get::<DebandDither>(photo).unwrap(),
            DebandDither::Disabled
        );
        let bloom = world.get::<Bloom>(photo).unwrap();
        assert_eq!(bloom.intensity, Bloom::NATURAL.intensity);
        assert_eq!(bloom.prefilter.threshold, 0.0);
        assert_eq!(bloom.prefilter.threshold_softness, 0.0);
        assert_eq!(bloom.max_mip_dimension, 512);
        assert!(matches!(
            bloom.composite_mode,
            bevy::post_process::bloom::BloomCompositeMode::EnergyConserving
        ));
        assert!(world.get::<Hdr>(legacy).is_none());
        assert!(world.get::<Bloom>(legacy).is_none());
    }

    #[test]
    fn zero_bloom_still_uses_hdr_and_fixed_exposure() {
        let mut world = World::new();
        let camera = world.spawn(Camera2d).id();
        PhotographicPreview {
            bloom_intensity: 0.0,
            tonemapping: DisplayTransform::Reinhard,
            ..default()
        }
        .apply(&mut world.commands().entity(camera));
        world.flush();
        assert!(world.get::<Hdr>(camera).is_some());
        assert!(world.get::<Bloom>(camera).is_none());
        assert_eq!(
            *world.get::<Tonemapping>(camera).unwrap(),
            Tonemapping::Reinhard
        );
    }
}
