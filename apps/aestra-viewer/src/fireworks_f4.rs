//! Presentation stress probes, not authored shells or finale certification.
//! Constant cohorts isolate raster policies from event/occupancy changes.
use aestra_bevy::{
    ColorKey, Curve, CurveKey, EffectAsset, EffectId, EffectPlaybackMode, Emitter, EmitterShape,
    Gradient, ModuleInstance, RendererProperties, RendererTypeId, ScalarRange, TrailEndCap,
    TrailSamplingMode, TrailUvMode,
};
use bevy::prelude::*;
use serde::Serialize;

pub const PARTICLES: u32 = 65_536;
const TRAIL_PARTICLES: u32 = 8_192;
const TRAIL_POINTS: u32 = 8;
const TRAIL_SAMPLE_SECONDS: f32 = 1.0 / 60.0;
const TRAIL_LIFETIME_SECONDS: f32 = 0.08;
const TRAIL_SPEED_PIXELS: f32 = 15.0;
const QUAD_PIXELS: f32 = 0.25;
const PATCH_PIXELS: f32 = 16.0;
const DEPTH: f32 = 48.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Fill,
    Offscreen,
    TrailFill,
    TrailOffscreen,
}

impl Probe {
    pub const ALL: [Self; 4] = [
        Self::Fill,
        Self::Offscreen,
        Self::TrailFill,
        Self::TrailOffscreen,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Fill => "f4-sprite-fill",
            Self::Offscreen => "f4-sprite-offscreen",
            Self::TrailFill => "f4-trail-fill",
            Self::TrailOffscreen => "f4-trail-offscreen",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|probe| probe.name() == value)
    }

    fn center_ndc_x(self) -> f32 {
        match self {
            Self::Fill | Self::TrailFill => 0.0,
            Self::Offscreen | Self::TrailOffscreen => 2.0,
        }
    }

    pub fn is_trail(self) -> bool {
        matches!(self, Self::TrailFill | Self::TrailOffscreen)
    }

    pub fn particles(self) -> u32 {
        if self.is_trail() {
            TRAIL_PARTICLES
        } else {
            PARTICLES
        }
    }

    pub fn setup(self) -> RasterProbeSetup {
        RasterProbeSetup {
            particles: self.particles(),
            nominal_quad_pixels: (!self.is_trail()).then_some(QUAD_PIXELS),
            nominal_patch_pixels: PATCH_PIXELS,
            reference_physical_viewport: [super::VIEW_WIDTH, super::VIEW_HEIGHT],
            center_ndc_x: self.center_ndc_x(),
            trail: self.is_trail().then_some(TrailProbeSetup {
                max_points: TRAIL_POINTS,
                max_trails: TRAIL_PARTICLES,
                nominal_head_width_pixels: QUAD_PIXELS,
                nominal_speed_pixels_per_second: TRAIL_SPEED_PIXELS,
                sample_interval_seconds: TRAIL_SAMPLE_SECONDS,
                lifetime_seconds: TRAIL_LIFETIME_SECONDS,
                end_cap: "flat",
            }),
        }
    }
}

/// Nominal calibration, not a claim about a resized/HiDPI actual viewport.
#[derive(Serialize)]
pub struct RasterProbeSetup {
    particles: u32,
    nominal_quad_pixels: Option<f32>,
    nominal_patch_pixels: f32,
    reference_physical_viewport: [u32; 2],
    center_ndc_x: f32,
    trail: Option<TrailProbeSetup>,
}

#[derive(Serialize)]
struct TrailProbeSetup {
    max_points: u32,
    max_trails: u32,
    nominal_head_width_pixels: f32,
    nominal_speed_pixels_per_second: f32,
    sample_interval_seconds: f32,
    lifetime_seconds: f32,
    end_cap: &'static str,
}

pub fn effect(probe: Probe, camera: super::FireworksCamera) -> EffectAsset {
    let mut effect = EffectAsset::new(probe.name(), 30.0);
    // The pair intentionally shares simulation identities/seed. Only its host placement/name differs.
    effect.id = EffectId::from_u128(if probe.is_trail() { 0xf500 } else { 0xf400 });
    effect.playback_mode = EffectPlaybackMode::Once;
    let mut emitter = Emitter::basic_sprite("Stationary overlap cohort", 30.0);
    emitter.max_particles = probe.particles();
    // Match the viewer's default perspective projection at its reference physical resolution.
    // A shallow camera-aligned box retains fractional pixel phases and bounds for CPU culling.
    let half_height = DEPTH * (PerspectiveProjection::default().fov * 0.5).tan();
    let units_per_pixel = 2.0 * half_height / super::VIEW_HEIGHT as f32;
    let transform = camera.transform();
    emitter.transform.rotation = transform.rotation.to_array();
    emitter.modules = vec![
        ModuleInstance::emission(0.0, probe.particles()),
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
    if probe.is_trail() {
        emitter.name = "Constant moving trail cohort".into();
        let speed = TRAIL_SPEED_PIXELS * units_per_pixel;
        if let aestra_bevy::ModuleParameters::Initialize {
            speed: value,
            direction,
            ..
        } = &mut emitter.modules[2].parameters
        {
            *value = ScalarRange::new(speed, speed);
            *direction = [1.0, 0.0, 0.0];
        }
        if let aestra_bevy::ModuleParameters::Appearance { size, .. } =
            &mut emitter.modules[4].parameters
        {
            for key in &mut size.keys {
                key.value = 1.0;
            }
        }
        // No head sprite: the pass measures history strips alone. Flat caps isolate
        // one-dimensional widening from the already-tested round-cap footprint.
        emitter.renderers[0].renderer_type = RendererTypeId::new(aestra_bevy::RENDERER_TRAIL);
        emitter.renderers[0].properties = RendererProperties::Trail {
            width: QUAD_PIXELS * units_per_pixel,
            sample_interval: TRAIL_SAMPLE_SECONDS,
            // Keep room for a boundary anchor and live head. A one-second tail at
            // 60 Hz would truncate in this eight-point pool and invalidate the comparison.
            lifetime: TRAIL_LIFETIME_SECONDS,
            max_points: TRAIL_POINTS,
            max_trails: TRAIL_PARTICLES,
            sampling: TrailSamplingMode::Time,
            sample_distance: 0.001,
            curve_tolerance: 0.001,
            uv_mode: TrailUvMode::Stretch,
            tile_length: 1.0,
            end_cap: TrailEndCap::Flat,
        };
    }
    super::fireworks_f0::fix_emitter_ids(
        &mut emitter,
        if probe.is_trail() { 0xf510 } else { 0xf410 },
    );
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
    fn trail_probe_history_fits_and_measured_trajectories_stay_in_the_intended_view() {
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
            for probe in [Probe::TrailFill, Probe::TrailOffscreen] {
                let source = effect(probe, camera);
                let RendererProperties::Trail {
                    max_points,
                    sample_interval,
                    lifetime,
                    max_trails,
                    sampling,
                    end_cap,
                    ..
                } = source.emitters[0].renderers[0].properties
                else {
                    panic!("trail-only fixture")
                };
                assert_eq!(sampling, TrailSamplingMode::Time);
                assert_eq!(end_cap, TrailEndCap::Flat);
                assert_eq!(max_trails, TRAIL_PARTICLES);
                // Retained samples plus the boundary anchor and live head must fit.
                assert!((lifetime / sample_interval).ceil() as u32 + 2 <= max_points);
                let mut instance = EffectInstance::with_seed(
                    Arc::new(EffectCompiler::default().compile(&source).unwrap()),
                    super::super::fireworks_f0::SEED,
                );
                for seconds in [2.0, 12.0] {
                    instance.seek(seconds);
                    let mut samples = Vec::new();
                    instance.evaluate(&mut samples);
                    for sample in samples {
                        let center = placement(probe, camera)
                            .transform_point(Vec3::from_array(sample.position));
                        let clip = projection * view * center.extend(1.0);
                        let ndc = clip.truncate() / clip.w;
                        if probe == Probe::TrailFill {
                            assert!(ndc.x.abs() < 0.8 && ndc.y.abs() < 0.1);
                        } else {
                            assert!(
                                (ndc.x - 1.0) * super::super::VIEW_WIDTH as f32 * 0.5
                                    > PATCH_PIXELS + 8.0
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn stress_pairs_preserve_full_cohorts_and_differ_only_in_placement() {
        for camera in [
            super::super::FireworksCamera::Close,
            super::super::FireworksCamera::Audience,
            super::super::FireworksCamera::Wide,
        ] {
            for (fill_probe, offscreen_probe) in [
                (Probe::Fill, Probe::Offscreen),
                (Probe::TrailFill, Probe::TrailOffscreen),
            ] {
                let fill = effect(fill_probe, camera);
                let mut offscreen = effect(offscreen_probe, camera);
                assert_eq!(
                    fill.to_pretty_ron().unwrap(),
                    effect(fill_probe, camera).to_pretty_ron().unwrap()
                );
                assert!(fill.events.is_empty());
                assert_eq!(fill.emitters.len(), 1);
                assert_eq!(fill.emitters[0].renderers.len(), 1);
                assert_eq!(fill.emitters[0].max_particles, fill_probe.particles());
                EffectCompiler::default().compile(&offscreen).unwrap();
                offscreen.name.clone_from(&fill.name);
                assert_eq!(fill, offscreen);
                assert_ne!(
                    placement(fill_probe, camera),
                    placement(offscreen_probe, camera)
                );
                let compiled = Arc::new(EffectCompiler::default().compile(&fill).unwrap());
                let mut instance =
                    EffectInstance::with_seed(compiled, super::super::fireworks_f0::SEED)
                        .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
                instance.seek(2.0);
                let mut before = Vec::new();
                instance.evaluate(&mut before);
                assert_eq!(before.len(), fill_probe.particles() as usize);
                instance.seek(12.0);
                let mut after = Vec::new();
                instance.evaluate(&mut after);
                assert_eq!(after.len(), before.len());
                for (a, b) in before.iter().zip(&after) {
                    if fill_probe.is_trail() {
                        let expected = camera.transform().rotation
                            * Vec3::X
                            * (TRAIL_SPEED_PIXELS
                                * (12.0 - 2.0)
                                * 2.0
                                * DEPTH
                                * (PerspectiveProjection::default().fov * 0.5).tan()
                                / super::super::VIEW_HEIGHT as f32);
                        assert!(
                            (Vec3::from_array(b.position)
                                - Vec3::from_array(a.position)
                                - expected)
                                .length()
                                < 0.001
                        );
                    } else {
                        assert_eq!(a.position, b.position);
                    }
                    assert_eq!(a.size, b.size);
                    assert_eq!(a.color, b.color);
                }
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
                let width = match &emitter.renderers[0].properties {
                    RendererProperties::Trail { width, .. } => *width * size.keys[0].value,
                    _ => size.keys[0].value,
                };
                let axis = camera.transform().rotation * Vec3::X * width;
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
                if !probe.is_trail() {
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
                }
                // Even the 8-pixel maximum treatment is nowhere near the right viewport edge.
                if matches!(probe, Probe::Offscreen | Probe::TrailOffscreen) {
                    assert!(
                        (clip.x / clip.w - 1.0) * super::super::VIEW_WIDTH as f32 * 0.5
                            > PATCH_PIXELS + 8.0
                    );
                }
            }
        }
    }
}
