//! Presentation stress probes, not authored shells or finale certification.
//! A stationary cohort makes every measured frame do the same particle work.
use aestra_bevy::{
    ColorKey, Curve, CurveKey, EffectAsset, EffectId, EffectPlaybackMode, Emitter, EmitterShape,
    Gradient, ModuleInstance, ScalarRange,
};
use bevy::prelude::*;
use serde::Serialize;

pub const PARTICLES: u32 = 65_536;
const QUAD_PIXELS: f32 = 0.25;
const PATCH_PIXELS: f32 = 16.0;
const DEPTH: f32 = 48.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Fill,
    Offscreen,
}

impl Probe {
    pub const ALL: [Self; 2] = [Self::Fill, Self::Offscreen];

    pub fn name(self) -> &'static str {
        match self {
            Self::Fill => "f4-sprite-fill",
            Self::Offscreen => "f4-sprite-offscreen",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|probe| probe.name() == value)
    }

    fn center_ndc_x(self) -> f32 {
        match self {
            Self::Fill => 0.0,
            Self::Offscreen => 2.0,
        }
    }

    pub fn setup(self) -> RasterProbeSetup {
        RasterProbeSetup {
            particles: PARTICLES,
            nominal_quad_pixels: QUAD_PIXELS,
            nominal_patch_pixels: PATCH_PIXELS,
            reference_physical_viewport: [super::VIEW_WIDTH, super::VIEW_HEIGHT],
            center_ndc_x: self.center_ndc_x(),
        }
    }
}

/// Nominal calibration, not a claim about a resized/HiDPI actual viewport.
#[derive(Serialize)]
pub struct RasterProbeSetup {
    particles: u32,
    nominal_quad_pixels: f32,
    nominal_patch_pixels: f32,
    reference_physical_viewport: [u32; 2],
    center_ndc_x: f32,
}

pub fn effect(probe: Probe, camera: super::FireworksCamera) -> EffectAsset {
    let mut effect = EffectAsset::new(probe.name(), 30.0);
    // The pair intentionally shares simulation identities/seed. Only its host placement/name differs.
    effect.id = EffectId::from_u128(0xf400);
    effect.playback_mode = EffectPlaybackMode::Once;
    let mut emitter = Emitter::basic_sprite("Stationary overlap cohort", 30.0);
    emitter.max_particles = PARTICLES;
    // Match the viewer's default perspective projection at its reference physical resolution.
    // A shallow camera-aligned box retains fractional pixel phases and bounds for CPU culling.
    let half_height = DEPTH * (PerspectiveProjection::default().fov * 0.5).tan();
    let units_per_pixel = 2.0 * half_height / super::VIEW_HEIGHT as f32;
    let transform = camera.transform();
    emitter.transform.rotation = transform.rotation.to_array();
    emitter.modules = vec![
        ModuleInstance::emission(0.0, PARTICLES),
        ModuleInstance::shape(EmitterShape::Box {
            half_extents: [
                PATCH_PIXELS * units_per_pixel * 0.5,
                PATCH_PIXELS * units_per_pixel * 0.5,
                0.001,
            ],
        }),
        ModuleInstance::initialize(
            ScalarRange::new(30.0, 30.0),
            ScalarRange::new(0.0, 0.0),
            [0.0, 1.0, 0.0],
            0.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0; 3], 0.0, 0.0),
        ModuleInstance::appearance(
            Curve::new(vec![
                CurveKey::new(0.0, QUAD_PIXELS * units_per_pixel),
                CurveKey::new(1.0, QUAD_PIXELS * units_per_pixel),
            ]),
            Curve::new(vec![CurveKey::new(0.0, 1.0), CurveKey::new(1.0, 1.0)]),
            Gradient::new(vec![
                ColorKey::new(0.0, [8.0, 6.0, 2.0, 1.0]),
                ColorKey::new(1.0, [8.0, 6.0, 2.0, 1.0]),
            ]),
        ),
    ];
    super::fireworks_f0::fix_emitter_ids(&mut emitter, 0xf410);
    effect.emitters.push(emitter);
    effect
}

pub fn placement(probe: Probe, camera: super::FireworksCamera) -> Transform {
    let half_height = DEPTH * (PerspectiveProjection::default().fov * 0.5).tan();
    let half_width = half_height * super::VIEW_WIDTH as f32 / super::VIEW_HEIGHT as f32;
    // Keep the authored bounds near local zero; baking this offset into the emitter would
    // make the origin-centered conservative effect AABB span the camera frustum too.
    Transform::from_translation(camera.transform().transform_point(Vec3::new(
        probe.center_ndc_x() * half_width,
        0.0,
        -DEPTH,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_bevy::{EffectCompiler, EffectInstance, PlaybackHistoryPolicy};
    use bevy::camera::{CameraProjection, primitives::Aabb};
    use std::sync::Arc;

    #[test]
    fn stress_pair_is_deterministic_stationary_and_differs_only_in_placement() {
        for camera in [
            super::super::FireworksCamera::Close,
            super::super::FireworksCamera::Audience,
            super::super::FireworksCamera::Wide,
        ] {
            let fill = effect(Probe::Fill, camera);
            let mut offscreen = effect(Probe::Offscreen, camera);
            assert_eq!(
                fill.to_pretty_ron().unwrap(),
                effect(Probe::Fill, camera).to_pretty_ron().unwrap()
            );
            assert!(fill.events.is_empty());
            assert_eq!(fill.emitters.len(), 1);
            assert_eq!(fill.emitters[0].renderers.len(), 1);
            assert_eq!(fill.emitters[0].max_particles, PARTICLES);
            EffectCompiler::default().compile(&offscreen).unwrap();
            offscreen.name.clone_from(&fill.name);
            assert_eq!(fill, offscreen);
            assert_ne!(
                placement(Probe::Fill, camera),
                placement(Probe::Offscreen, camera)
            );
            let compiled = Arc::new(EffectCompiler::default().compile(&fill).unwrap());
            let mut instance =
                EffectInstance::with_seed(compiled, super::super::fireworks_f0::SEED)
                    .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
            instance.seek(2.0);
            let mut before = Vec::new();
            instance.evaluate(&mut before);
            assert_eq!(before.len(), PARTICLES as usize);
            instance.seek(12.0);
            let mut after = Vec::new();
            instance.evaluate(&mut after);
            assert_eq!(after.len(), before.len());
            for (a, b) in before.iter().zip(&after) {
                assert_eq!(a.position, b.position);
                assert_eq!(a.size, b.size);
                assert_eq!(a.color, b.color);
            }
        }
    }

    #[test]
    fn probe_projection_matches_nominal_footprint_and_offscreen_margin() {
        for camera in [
            super::super::FireworksCamera::Close,
            super::super::FireworksCamera::Audience,
            super::super::FireworksCamera::Wide,
        ] {
            let view = camera.transform().to_matrix().inverse();
            let projection = Mat4::perspective_infinite_reverse_rh(
                PerspectiveProjection::default().fov,
                super::super::VIEW_WIDTH as f32 / super::super::VIEW_HEIGHT as f32,
                0.1,
            );
            let frustum = PerspectiveProjection {
                aspect_ratio: super::super::VIEW_WIDTH as f32 / super::super::VIEW_HEIGHT as f32,
                ..default()
            }
            .compute_frustum(&GlobalTransform::from(camera.transform()));
            for probe in Probe::ALL {
                let source = effect(probe, camera);
                let emitter = &source.emitters[0];
                let center = placement(probe, camera).translation;
                let clip = projection * view * center.extend(1.0);
                assert!((clip.x / clip.w - probe.center_ndc_x()).abs() < 1e-5);
                let aestra_bevy::ModuleParameters::Appearance { size, .. } =
                    &emitter.modules[4].parameters
                else {
                    panic!("appearance")
                };
                let axis = camera.transform().rotation * Vec3::X * size.keys[0].value;
                let end = projection * view * (center + axis).extend(1.0);
                let pixels =
                    (end.x / end.w - clip.x / clip.w).abs() * super::super::VIEW_WIDTH as f32 * 0.5;
                assert!((pixels - QUAD_PIXELS).abs() < 0.001);
                let instance = EffectInstance::new(Arc::new(
                    EffectCompiler::default().compile(&source).unwrap(),
                ));
                let dynamics =
                    aestra_gpu::GpuEffectArtifact::dynamics_from_instance(&instance).unwrap();
                let bounds = Aabb {
                    center: bevy::math::Vec3A::ZERO,
                    half_extents: bevy::math::Vec3A::from(dynamics.bounds_half_extents),
                };
                assert_eq!(
                    frustum.intersects_obb(
                        &bounds,
                        &GlobalTransform::from(placement(probe, camera)).affine(),
                        true,
                        true
                    ),
                    probe == Probe::Fill,
                    "fixture must isolate real CPU culling, not a bounds span enclosing the camera"
                );
                // Even the 8-pixel maximum treatment is nowhere near the right viewport edge.
                if probe == Probe::Offscreen {
                    assert!(
                        (clip.x / clip.w - 1.0) * super::super::VIEW_WIDTH as f32 * 0.5
                            > PATCH_PIXELS + 8.0
                    );
                }
            }
        }
    }
}
