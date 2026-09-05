//! Engine-neutral GPU ABI definitions and artifact lowering.
//!
//! This crate translates compiled Aestra effects into packed data suitable for
//! GPU simulation and rendering. It intentionally contains no Bevy, WGPU,
//! windowing, ECS, shader loading, dispatch, or drawing integration.

pub mod material;
pub mod mesh_bounds;
pub mod particle_attributes;
pub mod ribbon_bounds;
pub mod shader;

use aestra_core::{
    BlendMode, EmitterShape, FlipbookPlaybackMode, FlipbookTimeSource, PropertyEvaluationDomain,
    ScalarRange, Vec3Range,
};
use aestra_runtime::{
    CompiledCurve, CompiledGradient, CompiledVec3Curve, EffectInstance, ExecutionPlan, Instruction,
    MaterialColorPlan, RendererPlanKind, RuntimeValue, ScalarSource, VectorSource,
};
use encase::ShaderType;
use glam::{Mat4, Quat, UVec2, UVec3, Vec2, Vec3, Vec4};
use thiserror::Error;

pub const MAX_CURVE_KEYS: usize = 8;
/// Samples in the per-emitter inverse-emission table used to seed curve-driven
/// spawn-time reconstruction (see `aestra_simulation.wesl`). More samples give the
/// GPU a tighter starting bracket, so fewer refinement iterations are needed.
pub const SPAWN_INVERSE_SAMPLES: usize = 32;
pub const MAX_FLIPBOOK_FRAMES: usize = 64;
pub const WORKGROUP_SIZE: u32 = 64;
const INDIRECT_DRAW_WORDS: usize = 4;
pub const INDIRECT_DRAW_BYTES: u64 = (INDIRECT_DRAW_WORDS * std::mem::size_of::<u32>()) as u64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GpuArtifactError {
    #[error("emitter '{0}' has no {1} instruction")]
    MissingInstruction(String, &'static str),
    #[error("{kind} has {actual} keys; the GPU profile supports at most {maximum}")]
    KeyLimit {
        kind: &'static str,
        actual: usize,
        maximum: usize,
    },
    #[error("flipbook '{name}' has {actual} frames; the GPU profile supports at most {maximum}")]
    FlipbookFrameLimit {
        name: String,
        actual: usize,
        maximum: usize,
    },
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuCurve {
    pub keys: [Vec2; MAX_CURVE_KEYS],
    pub count: u32,
    pub _padding: Vec3,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuGradientKey {
    pub color: Vec4,
    pub time: f32,
    pub _padding: Vec3,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuGradient {
    pub keys: [GpuGradientKey; MAX_CURVE_KEYS],
    pub count: u32,
    pub _padding: Vec3,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuEmitter {
    pub slot_offset: u32,
    pub max_particles: u32,
    pub burst_count: u32,
    pub shape_kind: u32,
    pub start_time: f32,
    pub duration: f32,
    pub source_offset: f32,
    pub source_duration: f32,
    pub spawn_rate: Vec2,
    pub spawn_rate_source: u32,
    pub seed_index: u32,
    pub spawn_rate_curve: GpuCurve,
    pub shape_radius: f32,
    pub shape_depth: f32,
    pub shape_extent_z: f32,
    pub spread_radians: f32,
    pub drag: Vec2,
    pub drag_source: u32,
    /// Omitted presentation attributes; zero retains full-reference readback.
    pub omitted_attributes: u32,
    pub drag_curve: GpuCurve,
    pub direction: Vec3,
    pub _direction_padding: f32,
    pub lifetime: Vec2,
    pub speed: Vec2,
    pub angular_velocity: Vec2,
    pub _range_padding: Vec2,
    pub gravity: Vec3,
    pub gravity_source: u32,
    pub gravity_max: Vec3,
    pub _gravity_max_padding: f32,
    pub gravity_curves: [GpuCurve; 3],
    pub turbulence: Vec2,
    pub turbulence_source: u32,
    /// Nonzero enables deterministic ribbon linking after simulation (former padding).
    pub _turbulence_padding: u32,
    pub turbulence_curve: GpuCurve,
    pub translation: Vec3,
    pub max_scale: f32,
    pub rotation: Vec4,
    pub scale: Vec3,
    pub _transform_padding: f32,
    pub size: GpuCurve,
    pub opacity: GpuCurve,
    pub color: GpuGradient,
    /// Inverse-emission table for a curve-driven spawn rate: `spawn_inverse[k]` is the
    /// spawn time (in `[0, source_duration]`) at which cumulative emission reaches the
    /// fraction `k/(SPAWN_INVERSE_SAMPLES-1)` of `spawn_inverse_total`. Zeroed for
    /// non-curve spawn rates, which do not need reconstruction.
    pub spawn_inverse: [f32; SPAWN_INVERSE_SAMPLES],
    /// Total emission over the emitter's source duration — the denominator for the
    /// spawn-inverse fraction. Zero when the table is unused.
    pub spawn_inverse_total: f32,
    pub _spawn_inverse_padding: Vec3,
}

/// One authored presentation path for an emitter.
#[derive(Debug, Clone, Copy, ShaderType)]
pub struct GpuRenderer {
    pub emitter_index: u32,
    pub blend_mode: u32,
    pub softness: f32,
    pub textured: u32,
    pub uv_min: Vec2,
    pub uv_max: Vec2,
    pub tint: Vec4,
    pub particle_color: u32,
    pub renderer_kind: u32,
    pub frame_count: u32,
    pub playback_mode: u32,
    pub flipbook_flags: u32,
    pub frame_rate: f32,
    /// x: omitted particle reads; y: ribbon width (f32 bits); z: reserved.
    pub attribute_flags: UVec3,
    pub frames: [Vec4; MAX_FLIPBOOK_FRAMES],
}

/// Selects the renderer record used by one indirect draw.
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuRenderParams {
    pub renderer_index: u32,
    pub alive_offset: u32,
    pub _padding: UVec2,
    /// Emitter rotation and relative scale; particle size already includes maximum emitter scale.
    pub mesh_from_local: Mat4,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuGlobals {
    pub time: f32,
    pub total_slots: u32,
    pub seed: u32,
    pub emitter_count: u32,
    pub duration: f32,
    pub continuous: u32,
    pub _padding: UVec2,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuRenderGlobals {
    pub world_from_effect: Mat4,
    pub time: f32,
    pub seed: u32,
    pub _padding: Vec2,
}

/// Stable storage/readback ABI shared with the GPU simulation shader.
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuParticle {
    pub color: Vec4,
    pub position: Vec3,
    pub size: f32,
    pub rotation: f32,
    pub normalized_age: f32,
    pub emitter_index: u32,
    pub alive: u32,
    pub particle_index: u32,
    pub _padding_0: u32,
    pub _padding_1: u32,
    pub _padding_2: u32,
}

#[derive(Debug, Clone)]
pub struct GpuEffectArtifact {
    pub emitters: Vec<GpuEmitter>,
    pub renderers: Vec<GpuRenderer>,
    pub particles: Vec<GpuParticle>,
    pub total_slots: u32,
    pub bounds_half_extents: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum GpuBlend {
    Alpha = 0,
    Additive = 1,
    Multiply = 2,
}

/// The dynamic, per-frame portion of a GPU effect artifact: emitter and renderer
/// inputs plus the slot count and bounds, but not the capacity-sized particle
/// scratch buffer. Building this avoids allocating and zeroing `Vec<GpuParticle>`
/// on the per-frame update path, where only emitter and renderer inputs change.
#[derive(Debug, Clone)]
pub struct GpuEffectDynamics {
    /// Geometry-independent bounds inputs, indexed by compiled emitter (including disabled ones).
    pub mesh_bounds: Vec<mesh_bounds::MeshParticleBounds>,
    /// Camera-facing bounds inputs, indexed by compiled emitter (including disabled ones).
    pub ribbon_bounds: Vec<ribbon_bounds::RibbonParticleBounds>,
    pub emitters: Vec<GpuEmitter>,
    pub renderers: Vec<GpuRenderer>,
    pub total_slots: u32,
    pub bounds_half_extents: Vec3,
}

impl GpuEffectArtifact {
    /// Builds the full artifact including capacity-sized particle storage. Use this
    /// when persistent GPU particle buffers are first created or resized; the
    /// per-frame update path should prefer [`Self::dynamics_from_instance`], whose
    /// cost scales with emitter and renderer count rather than particle capacity.
    pub fn from_instance(instance: &EffectInstance) -> Result<Self, GpuArtifactError> {
        let dynamics = Self::dynamics_from_instance(instance)?;
        Ok(Self {
            particles: vec![GpuParticle::default(); dynamics.total_slots as usize],
            emitters: dynamics.emitters,
            renderers: dynamics.renderers,
            total_slots: dynamics.total_slots,
            bounds_half_extents: dynamics.bounds_half_extents,
        })
    }

    /// Builds only the dynamic emitter/renderer inputs (plus slot count and bounds)
    /// without allocating the capacity-sized particle buffer.
    pub fn dynamics_from_instance(
        instance: &EffectInstance,
    ) -> Result<GpuEffectDynamics, GpuArtifactError> {
        for flipbook in &instance.effect().flipbooks {
            if flipbook.frames.len() > MAX_FLIPBOOK_FRAMES {
                return Err(GpuArtifactError::FlipbookFrameLimit {
                    name: flipbook.name.clone(),
                    actual: flipbook.frames.len(),
                    maximum: MAX_FLIPBOOK_FRAMES,
                });
            }
        }
        let parameters = instance.parameter_values();
        let mut slot_offset = 0_u32;
        let mut bounds_half_extents = Vec3::splat(0.01);
        let mut mesh_bounds = Vec::new();
        let mut ribbon_bounds = Vec::new();
        let mut emitters = Vec::with_capacity(instance.effect().emitters.len());
        let mut renderers = Vec::new();
        for (emitter_index, emitter) in instance.effect().emitters.iter().enumerate() {
            let (spawn_rate, burst_count) =
                emission(&emitter.execution, parameters).ok_or_else(|| {
                    GpuArtifactError::MissingInstruction(emitter.name.clone(), "Emit")
                })?;
            let shape = shape(&emitter.execution, parameters).ok_or_else(|| {
                GpuArtifactError::MissingInstruction(emitter.name.clone(), "SampleShape")
            })?;
            let init = initialize(&emitter.execution, parameters).ok_or_else(|| {
                GpuArtifactError::MissingInstruction(emitter.name.clone(), "Initialize")
            })?;
            let motion = motion(&emitter.execution, parameters).unwrap_or_default();
            let appearance = appearance(&emitter.execution, parameters).ok_or_else(|| {
                GpuArtifactError::MissingInstruction(emitter.name.clone(), "Appearance")
            })?;
            let (shape_kind, shape_radius, shape_depth, shape_extent_z) = match shape {
                EmitterShape::Point => (0, 0.0, 0.0, 0.0),
                EmitterShape::Circle { radius } => (1, radius, 0.0, 0.0),
                EmitterShape::Ring { radius } => (2, radius, 0.0, 0.0),
                EmitterShape::Sphere { radius } => (3, radius, 0.0, 0.0),
                EmitterShape::Hemisphere { radius } => (4, radius, 0.0, 0.0),
                EmitterShape::Box { half_extents } => {
                    (5, half_extents[0], half_extents[1], half_extents[2])
                }
                EmitterShape::Cylinder { radius, depth } => (6, radius, depth, 0.0),
                EmitterShape::Cone { radius, depth } => (7, radius, depth, 0.0),
            };
            if emitter.enabled {
                renderers.extend(emitter.renderers.iter().map(|renderer| {
                    // Semantic material instances are bound by the concrete render adapter. The
                    // portable simulation artifact still supplies their sprite geometry and
                    // particle inputs, while legacy materials retain their existing packed data.
                    let material = instance.effect().material(renderer.material);
                    let (tint, particle_color, blend, softness, material_texture, uv) = material
                        .map_or(
                            (
                                [1.0; 4],
                                1,
                                BlendMode::Additive,
                                1.0,
                                None,
                                aestra_core::UvRect::FULL,
                            ),
                            |material| {
                                let (tint, particle_color) = match &material.color {
                                    MaterialColorPlan::ParticleColor => ([1.0; 4], 1),
                                    MaterialColorPlan::Value(value) => {
                                        (*value.resolve(parameters), 0)
                                    }
                                };
                                (
                                    tint,
                                    particle_color,
                                    material.blend,
                                    *material.softness.resolve(parameters),
                                    material.texture,
                                    material.uv,
                                )
                            },
                        );
                    let mut frames = [Vec4::new(0.0, 0.0, 1.0, 1.0); MAX_FLIPBOOK_FRAMES];
                    let (
                        renderer_kind,
                        frame_count,
                        playback_mode,
                        flipbook_flags,
                        frame_rate,
                        texture,
                    ) = match &renderer.kind {
                        RendererPlanKind::Sprite => (0, 1, 0, 0, 0.0, material_texture),
                        RendererPlanKind::Ribbon { .. } => (3, 1, 0, 0, 0.0, material_texture),
                        RendererPlanKind::Mesh { .. } => (2, 1, 0, 0, 0.0, material_texture),
                        RendererPlanKind::Flipbook {
                            flipbook,
                            time_source,
                            playback,
                            random_start,
                        } => {
                            let flipbook = instance
                                .effect()
                                .flipbook(*flipbook)
                                .expect("compiler guarantees flipbook references");
                            for (target, frame) in frames.iter_mut().zip(&flipbook.frames) {
                                *target = Vec4::new(
                                    frame.min[0],
                                    frame.min[1],
                                    frame.max[0],
                                    frame.max[1],
                                );
                            }
                            let mut flags = 0;
                            if *time_source == FlipbookTimeSource::EffectTime {
                                flags |= 1;
                            }
                            if *random_start {
                                flags |= 2;
                            }
                            if flipbook.looping {
                                flags |= 4;
                            }
                            (
                                1,
                                flipbook.frames.len() as u32,
                                match playback {
                                    FlipbookPlaybackMode::Forward => 0,
                                    FlipbookPlaybackMode::Reverse => 1,
                                    FlipbookPlaybackMode::PingPong => 2,
                                },
                                flags,
                                flipbook.frame_rate,
                                Some(flipbook.texture),
                            )
                        }
                    };
                    GpuRenderer {
                        emitter_index: emitter_index as u32,
                        blend_mode: match blend {
                            BlendMode::Alpha => GpuBlend::Alpha as u32,
                            BlendMode::Additive => GpuBlend::Additive as u32,
                            BlendMode::Multiply => GpuBlend::Multiply as u32,
                        },
                        softness,
                        textured: u32::from(texture.is_some()),
                        uv_min: Vec2::from_array(uv.min),
                        uv_max: Vec2::from_array(uv.max),
                        tint: Vec4::from_array(tint),
                        particle_color,
                        renderer_kind,
                        frame_count,
                        playback_mode,
                        flipbook_flags,
                        frame_rate,
                        attribute_flags: UVec3::new(
                            0,
                            match renderer.kind {
                                RendererPlanKind::Ribbon { width } => width.to_bits(),
                                _ => 0,
                            },
                            0,
                        ),
                        frames,
                    }
                }));
            }
            let local_bounds = emitter_bounds(
                shape,
                init.lifetime,
                init.speed,
                maximum_absolute_vector_source(motion.gravity),
                maximum_absolute_scalar_source(motion.turbulence),
                maximum_absolute_curve(appearance.size)
                    * 0.5
                    * emitter
                        .renderers
                        .iter()
                        .map(|r| match r.kind {
                            RendererPlanKind::Ribbon { width } => width,
                            _ => 1.0,
                        })
                        .fold(1.0_f32, f32::max),
            );
            bounds_half_extents = bounds_half_extents.max(transformed_emitter_bounds(
                local_bounds,
                emitter.transform.translation,
                emitter.transform.rotation,
                emitter.transform.scale,
            ));
            let rotation = Quat::from_array(emitter.transform.rotation).normalize();
            let scale = Vec3::from_array(emitter.transform.scale);
            let particle_bounds = mesh_bounds::MeshParticleBounds {
                position_half_extents: transformed_emitter_bounds(
                    emitter_bounds(
                        shape,
                        init.lifetime,
                        init.speed,
                        maximum_absolute_vector_source(motion.gravity),
                        maximum_absolute_scalar_source(motion.turbulence),
                        0.0,
                    ),
                    emitter.transform.translation,
                    emitter.transform.rotation,
                    emitter.transform.scale,
                ),
                linear_from_local: glam::Mat3::from_quat(rotation)
                    * glam::Mat3::from_diagonal(scale),
                maximum_size: maximum_absolute_curve(appearance.size),
            };
            let maximum_width = emitter
                .renderers
                .iter()
                .map(|r| match r.kind {
                    RendererPlanKind::Ribbon { width } => width,
                    _ => 0.0,
                })
                .fold(0.0_f32, f32::max);
            let ribbon = ribbon_bounds::RibbonParticleBounds {
                position_half_extents: particle_bounds.position_half_extents,
                maximum_half_width: particle_bounds.maximum_size
                    * scale.abs().max_element()
                    * maximum_width
                    * 0.5,
            };
            // The aggregate bound is also used for initial viewport framing. A
            // camera-facing ribbon cannot use emitter-axis-scaled sprite padding.
            if maximum_width > 0.0
                && let Some(bounds) = ribbon.half_extents(glam::Mat3::IDENTITY)
            {
                bounds_half_extents = bounds_half_extents.max(bounds);
            }
            ribbon_bounds.push(ribbon);
            mesh_bounds.push(particle_bounds);
            let (spawn_inverse, spawn_inverse_total) = if emitter.enabled {
                build_spawn_inverse(spawn_rate, emitter.source_duration)
            } else {
                ([0.0; SPAWN_INVERSE_SAMPLES], 0.0)
            };
            let (spawn_rate, spawn_rate_source, spawn_rate_curve) = if emitter.enabled {
                pack_scalar_source(spawn_rate)?
            } else {
                (Vec2::ZERO, 0, GpuCurve::default())
            };
            let (drag, drag_source, drag_curve) = pack_scalar_source(motion.drag)?;
            let (turbulence, turbulence_source, turbulence_curve) =
                pack_scalar_source(motion.turbulence)?;
            let (gravity, gravity_source, gravity_max, gravity_curves) =
                pack_vector_source(motion.gravity)?;
            emitters.push(GpuEmitter {
                slot_offset,
                max_particles: emitter.max_particles,
                burst_count: if emitter.enabled { burst_count } else { 0 },
                shape_kind,
                start_time: emitter.start_time,
                duration: emitter.duration,
                source_offset: emitter.source_offset,
                source_duration: emitter.source_duration,
                spawn_rate,
                spawn_rate_source,
                seed_index: emitter.seed_index,
                spawn_rate_curve,
                shape_radius,
                shape_depth,
                shape_extent_z,
                spread_radians: init.spread_degrees.to_radians(),
                drag,
                drag_source,
                omitted_attributes: 0,
                drag_curve,
                direction: Vec3::from_array(init.direction).normalize_or_zero(),
                _direction_padding: 0.0,
                lifetime: Vec2::new(init.lifetime.min, init.lifetime.max),
                speed: Vec2::new(init.speed.min, init.speed.max),
                angular_velocity: Vec2::new(init.angular_velocity.min, init.angular_velocity.max),
                _range_padding: Vec2::ZERO,
                gravity,
                gravity_source,
                gravity_max,
                _gravity_max_padding: 0.0,
                gravity_curves,
                turbulence,
                turbulence_source,
                _turbulence_padding: u32::from(
                    emitter
                        .renderers
                        .iter()
                        .any(|r| matches!(r.kind, RendererPlanKind::Ribbon { .. })),
                ),
                turbulence_curve,
                translation: Vec3::from_array(emitter.transform.translation),
                max_scale: scale.max_element(),
                rotation: Vec4::from_array(rotation.to_array()),
                scale,
                _transform_padding: 0.0,
                size: pack_curve(appearance.size)?,
                opacity: pack_curve(appearance.opacity)?,
                color: pack_gradient(appearance.color)?,
                spawn_inverse,
                spawn_inverse_total,
                _spawn_inverse_padding: Vec3::ZERO,
            });
            slot_offset = slot_offset.saturating_add(emitter.max_particles);
        }
        Ok(GpuEffectDynamics {
            mesh_bounds,
            ribbon_bounds,
            emitters,
            renderers,
            total_slots: slot_offset,
            bounds_half_extents,
        })
    }
}

pub fn fold_seed(seed: u64) -> u32 {
    seed as u32 ^ (seed >> 32) as u32
}

pub fn indirect_draw_commands(emitters: &[GpuEmitter]) -> Vec<u32> {
    emitters.iter().flat_map(|_| [6, 0, 0, 0]).collect()
}

pub const fn indirect_draw_offset(emitter_index: u32) -> u64 {
    emitter_index as u64 * INDIRECT_DRAW_BYTES
}

fn emitter_bounds(
    shape: EmitterShape,
    lifetime: ScalarRange,
    speed: ScalarRange,
    gravity: [f32; 3],
    turbulence: f32,
    size: f32,
) -> Vec3 {
    let shape_extents = match shape {
        EmitterShape::Point => Vec3::ZERO,
        EmitterShape::Circle { radius } | EmitterShape::Ring { radius } => {
            Vec3::new(radius.abs(), radius.abs(), 0.0)
        }
        EmitterShape::Sphere { radius } | EmitterShape::Hemisphere { radius } => {
            Vec3::splat(radius.abs())
        }
        EmitterShape::Box { half_extents } => Vec3::from_array(half_extents).abs(),
        EmitterShape::Cylinder { radius, depth } => {
            Vec3::new(radius.abs(), depth.abs() * 0.5, radius.abs())
        }
        EmitterShape::Cone { radius, depth } => Vec3::new(radius.abs(), depth.abs(), radius.abs()),
    };
    let lifetime = lifetime.min.abs().max(lifetime.max.abs());
    let speed = speed.min.abs().max(speed.max.abs());
    let travel = speed * lifetime;
    shape_extents
        + Vec3::new(
            travel + turbulence.abs() + gravity[0].abs() * lifetime * lifetime * 0.5 + size,
            travel + turbulence.abs() + gravity[1].abs() * lifetime * lifetime * 0.5 + size,
            travel + turbulence.abs() + gravity[2].abs() * lifetime * lifetime * 0.5 + size,
        )
}

fn transformed_emitter_bounds(
    local: Vec3,
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
) -> Vec3 {
    let scaled = local * Vec3::from_array(scale).abs();
    let rotation = Quat::from_array(rotation).normalize();
    let rotated = (rotation * Vec3::X * scaled.x).abs()
        + (rotation * Vec3::Y * scaled.y).abs()
        + (rotation * Vec3::Z * scaled.z).abs();
    Vec3::from_array(translation).abs() + rotated
}

fn pack_curve(curve: &CompiledCurve) -> Result<GpuCurve, GpuArtifactError> {
    let mut points = Vec::new();
    if let Some(first) = curve.first() {
        points.push(first);
        points.extend(
            curve
                .segments()
                .iter()
                .map(|segment| (segment.end_time, segment.end_value)),
        );
    }
    if points.len() > MAX_CURVE_KEYS {
        return Err(GpuArtifactError::KeyLimit {
            kind: "curve",
            actual: points.len(),
            maximum: MAX_CURVE_KEYS,
        });
    }
    let mut packed = GpuCurve {
        count: points.len() as u32,
        ..Default::default()
    };
    for (target, (time, value)) in packed.keys.iter_mut().zip(points) {
        *target = Vec2::new(time, value);
    }
    Ok(packed)
}

fn pack_gradient(gradient: &CompiledGradient) -> Result<GpuGradient, GpuArtifactError> {
    let mut points = Vec::new();
    if let Some(first) = gradient.first() {
        points.push(first);
        points.extend(
            gradient
                .segments()
                .iter()
                .map(|segment| (segment.end_time, segment.end_color)),
        );
    }
    if points.len() > MAX_CURVE_KEYS {
        return Err(GpuArtifactError::KeyLimit {
            kind: "gradient",
            actual: points.len(),
            maximum: MAX_CURVE_KEYS,
        });
    }
    let mut packed = GpuGradient {
        count: points.len() as u32,
        ..Default::default()
    };
    for (target, (time, color)) in packed.keys.iter_mut().zip(points) {
        target.time = time;
        target.color = Vec4::from_array(color);
    }
    Ok(packed)
}

#[derive(Clone, Copy)]
enum ResolvedGpuScalarSource<'a> {
    Constant(f32),
    RandomRange(ScalarRange),
    Curve(&'a CompiledCurve, PropertyEvaluationDomain),
}

#[derive(Clone, Copy)]
enum ResolvedGpuVectorSource<'a> {
    Constant([f32; 3]),
    RandomRange(Vec3Range),
    Curve(&'a CompiledVec3Curve, PropertyEvaluationDomain),
}

fn resolve_gpu_vector_source<'a>(
    source: &'a VectorSource,
    values: &'a [RuntimeValue],
) -> ResolvedGpuVectorSource<'a> {
    match source {
        VectorSource::Constant(value) => ResolvedGpuVectorSource::Constant(*value.resolve(values)),
        VectorSource::RandomRange(value) => {
            ResolvedGpuVectorSource::RandomRange(*value.resolve(values))
        }
        VectorSource::Curve { value, domain } => {
            ResolvedGpuVectorSource::Curve(value.resolve(values), *domain)
        }
    }
}

fn maximum_absolute_curve(curve: &CompiledCurve) -> f32 {
    curve
        .first()
        .map_or(0.0, |(_, value)| value.abs())
        .max(curve.last_value().abs())
        .max(
            curve
                .segments()
                .iter()
                .map(|segment| segment.start_value.abs().max(segment.end_value.abs()))
                .fold(0.0, f32::max),
        )
}

fn maximum_absolute_vector_source(source: ResolvedGpuVectorSource<'_>) -> [f32; 3] {
    match source {
        ResolvedGpuVectorSource::Constant(value) => value.map(f32::abs),
        ResolvedGpuVectorSource::RandomRange(range) => {
            std::array::from_fn(|axis| range.min[axis].abs().max(range.max[axis].abs()))
        }
        ResolvedGpuVectorSource::Curve(curve, _) => {
            std::array::from_fn(|axis| maximum_absolute_curve(&curve.curves[axis]))
        }
    }
}

fn pack_vector_source(
    source: ResolvedGpuVectorSource<'_>,
) -> Result<(Vec3, u32, Vec3, [GpuCurve; 3]), GpuArtifactError> {
    match source {
        ResolvedGpuVectorSource::Constant(value) => Ok((
            Vec3::from_array(value),
            0,
            Vec3::from_array(value),
            [GpuCurve::default(); 3],
        )),
        ResolvedGpuVectorSource::RandomRange(range) => Ok((
            Vec3::from_array(range.min),
            1,
            Vec3::from_array(range.max),
            [GpuCurve::default(); 3],
        )),
        ResolvedGpuVectorSource::Curve(curve, domain) => Ok((
            Vec3::ZERO,
            match domain {
                PropertyEvaluationDomain::EmitterTime => 2,
                PropertyEvaluationDomain::ParticleLife => 3,
            },
            Vec3::ZERO,
            [
                pack_curve(&curve.curves[0])?,
                pack_curve(&curve.curves[1])?,
                pack_curve(&curve.curves[2])?,
            ],
        )),
    }
}

fn resolve_gpu_scalar_source<'a>(
    source: &'a ScalarSource,
    values: &'a [RuntimeValue],
) -> ResolvedGpuScalarSource<'a> {
    match source {
        ScalarSource::Constant(value) => ResolvedGpuScalarSource::Constant(*value.resolve(values)),
        ScalarSource::RandomRange(value) => {
            ResolvedGpuScalarSource::RandomRange(*value.resolve(values))
        }
        ScalarSource::Curve { value, domain } => {
            ResolvedGpuScalarSource::Curve(value.resolve(values), *domain)
        }
    }
}

fn maximum_absolute_scalar_source(source: ResolvedGpuScalarSource<'_>) -> f32 {
    match source {
        ResolvedGpuScalarSource::Constant(value) => value.abs(),
        ResolvedGpuScalarSource::RandomRange(range) => range.min.abs().max(range.max.abs()),
        ResolvedGpuScalarSource::Curve(curve, _) => maximum_absolute_curve(curve),
    }
}

/// Builds the inverse-emission table for a curve-driven (emitter-time) spawn rate,
/// returning the table plus the total emission over `source_duration`. Other sources
/// return zeros and the shader keeps its analytic constant-rate path. Each entry is
/// the spawn time at an evenly spaced emission fraction, found by inverting the exact
/// cumulative-emission function the GPU evaluates — so the shader only needs a few
/// refinement steps to match the previous per-particle binary search.
fn build_spawn_inverse(
    source: ResolvedGpuScalarSource<'_>,
    source_duration: f32,
) -> ([f32; SPAWN_INVERSE_SAMPLES], f32) {
    let ResolvedGpuScalarSource::Curve(curve, PropertyEvaluationDomain::EmitterTime) = source
    else {
        return ([0.0; SPAWN_INVERSE_SAMPLES], 0.0);
    };
    let duration = source_duration.max(f32::EPSILON);
    let total = (duration * curve.integral(1.0)).max(0.0);
    let mut table = [0.0; SPAWN_INVERSE_SAMPLES];
    if total <= 0.0 {
        return (table, 0.0);
    }
    let denominator = (SPAWN_INVERSE_SAMPLES - 1) as f32;
    for (index, entry) in table.iter_mut().enumerate() {
        let target = (index as f32 / denominator) * total;
        let mut low = 0.0;
        let mut high = duration;
        for _ in 0..24 {
            let middle = (low + high) * 0.5;
            let emitted = (duration * curve.integral(middle / duration)).max(0.0);
            if emitted < target {
                low = middle;
            } else {
                high = middle;
            }
        }
        *entry = (low + high) * 0.5;
    }
    (table, total)
}

fn pack_scalar_source(
    source: ResolvedGpuScalarSource<'_>,
) -> Result<(Vec2, u32, GpuCurve), GpuArtifactError> {
    match source {
        ResolvedGpuScalarSource::Constant(value) => {
            Ok((Vec2::splat(value), 0, GpuCurve::default()))
        }
        ResolvedGpuScalarSource::RandomRange(range) => {
            Ok((Vec2::new(range.min, range.max), 1, GpuCurve::default()))
        }
        ResolvedGpuScalarSource::Curve(curve, PropertyEvaluationDomain::EmitterTime) => {
            Ok((Vec2::ZERO, 2, pack_curve(curve)?))
        }
        ResolvedGpuScalarSource::Curve(curve, PropertyEvaluationDomain::ParticleLife) => {
            Ok((Vec2::ZERO, 3, pack_curve(curve)?))
        }
    }
}

fn emission<'a>(
    plan: &'a ExecutionPlan,
    values: &'a [RuntimeValue],
) -> Option<(ResolvedGpuScalarSource<'a>, u32)> {
    plan.emitter_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Emit {
                spawn_rate,
                burst_count,
                ..
            } => {
                let spawn_rate = resolve_gpu_scalar_source(spawn_rate, values);
                Some((spawn_rate, *burst_count.resolve(values)))
            }
            _ => None,
        })
}

fn shape(plan: &ExecutionPlan, values: &[RuntimeValue]) -> Option<EmitterShape> {
    plan.particle_spawn
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::SampleShape { shape, .. } => Some(*shape.resolve(values)),
            _ => None,
        })
}

struct GpuInitialize {
    lifetime: ScalarRange,
    speed: ScalarRange,
    direction: [f32; 3],
    spread_degrees: f32,
    angular_velocity: ScalarRange,
}

fn initialize(plan: &ExecutionPlan, values: &[RuntimeValue]) -> Option<GpuInitialize> {
    plan.particle_spawn
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Initialize {
                lifetime,
                speed,
                direction,
                spread_degrees,
                angular_velocity,
                ..
            } => Some(GpuInitialize {
                lifetime: *lifetime.resolve(values),
                speed: *speed.resolve(values),
                direction: *direction.resolve(values),
                spread_degrees: *spread_degrees.resolve(values),
                angular_velocity: *angular_velocity.resolve(values),
            }),
            _ => None,
        })
}

struct GpuMotion<'a> {
    gravity: ResolvedGpuVectorSource<'a>,
    drag: ResolvedGpuScalarSource<'a>,
    turbulence: ResolvedGpuScalarSource<'a>,
}

impl Default for GpuMotion<'_> {
    fn default() -> Self {
        Self {
            gravity: ResolvedGpuVectorSource::Constant([0.0; 3]),
            drag: ResolvedGpuScalarSource::Constant(0.0),
            turbulence: ResolvedGpuScalarSource::Constant(0.0),
        }
    }
}

fn motion<'a>(plan: &'a ExecutionPlan, values: &'a [RuntimeValue]) -> Option<GpuMotion<'a>> {
    plan.particle_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Motion {
                gravity,
                drag,
                turbulence,
                ..
            } => Some(GpuMotion {
                gravity: resolve_gpu_vector_source(gravity, values),
                drag: resolve_gpu_scalar_source(drag, values),
                turbulence: resolve_gpu_scalar_source(turbulence, values),
            }),
            _ => None,
        })
}

struct GpuAppearance<'a> {
    size: &'a CompiledCurve,
    opacity: &'a CompiledCurve,
    color: &'a CompiledGradient,
}

fn appearance<'a>(
    plan: &'a ExecutionPlan,
    values: &'a [RuntimeValue],
) -> Option<GpuAppearance<'a>> {
    plan.particle_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Appearance {
                size,
                opacity,
                color,
                ..
            } => Some(GpuAppearance {
                size: size.resolve(values),
                opacity: opacity.resolve(values),
                color: color.resolve(values),
            }),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_compiler::EffectCompiler;
    use aestra_core::{
        Curve, CurveKey, EffectAsset, Emitter, MODULE_EMISSION, PropertyEvaluationDomain,
        PropertySource, PropertySourceValue, ScalarRange, Value,
    };
    use std::sync::Arc;

    #[test]
    fn artifact_capacity_matches_authored_bounds() {
        let mut effect = EffectAsset::new("GPU", 2.0);
        let mut first = Emitter::basic_sprite("First", 2.0);
        first.max_particles = 17;
        let mut second = Emitter::basic_sprite("Second", 2.0);
        second.max_particles = 23;
        effect.emitters.extend([first, second]);
        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let artifact = GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap();

        assert_eq!(artifact.total_slots, 40);
        assert_eq!(artifact.emitters[0].slot_offset, 0);
        assert_eq!(artifact.emitters[1].slot_offset, 17);
        assert_eq!(artifact.particles.len(), 40);
    }

    #[test]
    fn mesh_bounds_enclose_evaluated_particles_and_geometry() {
        let mut effect = EffectAsset::new("Mesh bounds", 2.0);
        let mut disabled = Emitter::basic_sprite("Disabled", 2.0);
        disabled.enabled = false;
        let mut emitter = Emitter::basic_sprite("Moving mesh", 2.0);
        emitter.transform.translation = [100.0, 20.0, -30.0];
        emitter.transform.rotation =
            Quat::from_euler(glam::EulerRot::XYZ, 0.5, 0.9, -0.3).to_array();
        emitter.transform.scale = [2.0, 0.3, 4.0];
        effect.emitters.extend([disabled, emitter]);
        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let mut instance = EffectInstance::new(compiled);
        let dynamics = GpuEffectArtifact::dynamics_from_instance(&instance).unwrap();
        assert_eq!(dynamics.mesh_bounds.len(), 2);
        let source = dynamics.mesh_bounds[1];
        let bounds = source
            .half_extents(Vec3::new(-30.0, -4.0, -1.0), Vec3::new(10.0, 20.0, 8.0))
            .unwrap();
        let mut particles = Vec::new();
        let mut sampled = 0;
        for frame in 0..120 {
            instance.seek(frame as f32 / 60.0);
            instance.evaluate(&mut particles);
            for particle in &particles {
                assert_eq!(particle.emitter_index, 1);
                for vertex in [Vec3::new(-30.0, -4.0, -1.0), Vec3::new(10.0, 20.0, 8.0)] {
                    // Same emitter scale compensation as the native mesh vertex shader.
                    let vertex = Vec3::from_array(particle.position)
                        + source.linear_from_local
                            * (Quat::from_rotation_z(particle.rotation) * vertex)
                            * (particle.size / dynamics.emitters[1].max_scale);
                    assert!(
                        vertex.abs().cmple(bounds).all(),
                        "{vertex:?} outside {bounds:?}"
                    );
                    sampled += 1;
                }
            }
        }
        assert!(sampled > 0);
    }

    #[test]
    fn dynamics_match_the_full_artifact_without_allocating_particles() {
        let mut effect = EffectAsset::new("GPU", 2.0);
        let mut first = Emitter::basic_sprite("First", 2.0);
        first.max_particles = 17;
        let mut second = Emitter::basic_sprite("Second", 2.0);
        second.max_particles = 23;
        effect.emitters.extend([first, second]);
        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let instance = EffectInstance::new(compiled);

        let artifact = GpuEffectArtifact::from_instance(&instance).unwrap();
        let dynamics = GpuEffectArtifact::dynamics_from_instance(&instance).unwrap();

        // The dynamics builder must produce the same emitter/renderer inputs and
        // slot count as the full builder — it only omits the particle scratch, which
        // the full builder sizes from exactly this slot count.
        assert_eq!(dynamics.total_slots, artifact.total_slots);
        assert_eq!(dynamics.bounds_half_extents, artifact.bounds_half_extents);
        assert_eq!(dynamics.emitters.len(), artifact.emitters.len());
        assert_eq!(dynamics.renderers.len(), artifact.renderers.len());
        assert_eq!(artifact.particles.len(), dynamics.total_slots as usize);
        for (dynamic, full) in dynamics.emitters.iter().zip(&artifact.emitters) {
            assert_eq!(dynamic.slot_offset, full.slot_offset);
            assert_eq!(dynamic.max_particles, full.max_particles);
        }
    }

    #[test]
    fn indirect_draw_contract_is_isolated_per_emitter() {
        let emitters = [GpuEmitter::default(), GpuEmitter::default()];
        assert_eq!(
            indirect_draw_commands(&emitters),
            vec![6, 0, 0, 0, 6, 0, 0, 0]
        );
        assert_eq!(indirect_draw_offset(0), 0);
        assert_eq!(indirect_draw_offset(1), INDIRECT_DRAW_BYTES);
    }

    #[test]
    fn seed_fold_matches_the_runtime_contract() {
        assert_eq!(fold_seed(0x1234_5678_9abc_def0), 0x8888_8888);
    }

    #[test]
    fn artifact_lowering_preserves_emitter_time_curves_without_bevy() {
        let mut effect = EffectAsset::new("GPU curve rate", 2.0);
        effect
            .emitters
            .push(Emitter::basic_sprite("Emitter", effect.duration));
        let emission = effect.emitters[0]
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == MODULE_EMISSION)
            .unwrap();
        let source = PropertySource::Curve(PropertyEvaluationDomain::EmitterTime);
        emission
            .property_sources
            .insert("spawn_rate".into(), source);
        emission.property_source_values.insert(
            "spawn_rate".into(),
            vec![PropertySourceValue::new(
                source,
                Value::Curve(Curve::normalized(
                    vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 1.0)],
                    ScalarRange::new(2.0, 20.0),
                )),
            )],
        );

        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let artifact = GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap();
        let emitter = artifact.emitters[0];

        assert_eq!(emitter.spawn_rate_source, 2);
        assert_eq!(emitter.spawn_rate_curve.count, 2);
        assert_eq!(emitter.spawn_rate_curve.keys[0], Vec2::new(0.0, 2.0));
        assert_eq!(emitter.spawn_rate_curve.keys[1], Vec2::new(1.0, 20.0));
    }
}
