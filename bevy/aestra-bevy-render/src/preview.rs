//! Shared opt-in photographic camera profile for hosts, the editor and deterministic captures.
//! This is presentation, not simulation state; no component is added to cameras automatically.
use bevy::{
    camera::Hdr,
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    post_process::bloom::Bloom,
    prelude::*,
    render::view::ColorGrading,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
/// Fixed HDR response for a host-owned effect camera. RGB is composited in an HDR intermediate,
/// bloom precedes display exposure, and screenshots remain tonemapped SDR. Defaults match the
/// viewer's `--hdr` profile. Applying this replaces grading, tonemapping and bloom on that camera.
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
    /// Sanitizes persisted or host-provided values. CLI callers may reject invalid values instead.
    pub fn normalized(mut self) -> Self {
        let defaults = Self::default();
        self.exposure_stops = if self.exposure_stops.is_finite() {
            self.exposure_stops.clamp(-8.0, 8.0)
        } else {
            defaults.exposure_stops
        };
        self.bloom_intensity = if self.bloom_intensity.is_finite() {
            self.bloom_intensity.clamp(0.0, 1.0)
        } else {
            defaults.bloom_intensity
        };
        self
    }

    /// Applies the fixed profile only to the selected camera; never changes effect playback.
    pub fn apply(self, camera: &mut EntityCommands) {
        let settings = self.normalized();
        // Camera Exposure is consumed by PBR shaders, not Aestra's unlit shaders. ColorGrading
        // applies relative stops to the entire HDR scene in the display transform instead.
        let mut grading = ColorGrading::default();
        grading.global.exposure = settings.exposure_stops;
        camera.insert((
            Hdr,
            grading,
            settings.tonemapping.component(),
            DebandDither::Disabled,
        ));
        if settings.bloom_intensity > 0.0 {
            camera.insert(Bloom {
                intensity: settings.bloom_intensity,
                ..Bloom::NATURAL
            });
        } else {
            camera.remove::<Bloom>();
        }
    }
}

/// Restores the legacy **3D** camera defaults after applying a photographic profile.
/// This owns/replaces HDR, bloom, grading and display-transform settings on that camera;
/// hosts with custom baseline components should restore their own snapshot instead.
pub fn restore_legacy_3d_response(camera: &mut EntityCommands) {
    camera.remove::<(Hdr, Bloom)>().insert((
        ColorGrading::default(),
        Tonemapping::default(),
        DebandDither::Enabled,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_sanitize_nonfinite_and_out_of_range_host_values() {
        let normalized = PhotographicPreview {
            exposure_stops: f32::NAN,
            bloom_intensity: f32::INFINITY,
            ..default()
        }
        .normalized();
        assert_eq!(normalized, PhotographicPreview::default());
        let normalized = PhotographicPreview {
            exposure_stops: -20.0,
            bloom_intensity: 3.0,
            ..default()
        }
        .normalized();
        assert_eq!(normalized.exposure_stops, -8.0);
        assert_eq!(normalized.bloom_intensity, 1.0);
    }

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
