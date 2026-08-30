//! Engine-independent compiled effect contracts and deterministic CPU execution.

mod checkpoint;
mod profile;

pub use checkpoint::{
    CheckpointBackendId, CheckpointContext, CheckpointPolicy, CheckpointStore, SeekOrigin,
    SeekPlan, SimulationSeekMode, StoredCheckpoint,
};
pub use profile::{EffectProfile, EmitterProfile, ProfileValue, ProfileValueSource};

use aestra_core::{
    AssetId, AssetKind, BlendMode, Curve, EffectAssetRef, EffectClipId, EffectClipSeed, EffectId,
    EmitterId, EmitterShape, EmitterTransform, FlipbookPlaybackMode, FlipbookTimeSource, Gradient,
    MaterialId, ModuleId, ParameterId, RendererId, ScalarRange, UvRect, Value, ValueType,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use thiserror::Error;

/// A logical particle field retained by the compiler for this effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParticleAttribute {
    Position,
    Velocity,
    Age,
    Lifetime,
    NormalizedAge,
    Rotation,
    AngularVelocity,
    Size,
    Color,
}

/// Runtime particle storage selected from compiler attribute-liveness analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParticleLayout {
    pub attributes: Vec<ParticleAttribute>,
    pub transient_attributes: Vec<ParticleAttribute>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParameterSlot(pub usize);

/// A constant-folded value or an indexed runtime parameter read.
#[derive(Debug, Clone, PartialEq)]
pub enum Expression<T> {
    Constant(T),
    Parameter(ParameterSlot),
}

impl<T> Expression<T> {
    pub fn constant(value: T) -> Self {
        Self::Constant(value)
    }

    pub fn parameter(slot: ParameterSlot) -> Self {
        Self::Parameter(slot)
    }

    pub fn constant_value(&self) -> Option<&T> {
        match self {
            Self::Constant(value) => Some(value),
            Self::Parameter(_) => None,
        }
    }
}

impl<T: RuntimeParameterValue> Expression<T> {
    /// Resolves a compiler expression against an instance's packed parameter table.
    pub fn resolve<'a>(&'a self, parameters: &'a [RuntimeValue]) -> &'a T {
        match self {
            Self::Constant(value) => value,
            Self::Parameter(slot) => T::from_runtime(
                parameters
                    .get(slot.0)
                    .expect("compiled parameter slot must exist"),
            )
            .expect("compiler guarantees expression parameter types"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CurveSegment {
    pub start_time: f32,
    pub end_time: f32,
    pub start_value: f32,
    pub end_value: f32,
}

/// Curve data stripped of authoring IDs and lowered into interpolation segments.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledCurve {
    first: Option<(f32, f32)>,
    last_value: f32,
    segments: Vec<CurveSegment>,
}

impl CompiledCurve {
    pub fn compile(curve: &Curve) -> Self {
        Self {
            first: curve.keys.first().map(|key| (key.time, key.value)),
            last_value: curve.keys.last().map_or(0.0, |key| key.value),
            segments: curve
                .keys
                .windows(2)
                .map(|pair| CurveSegment {
                    start_time: pair[0].time,
                    end_time: pair[1].time,
                    start_value: pair[0].value,
                    end_value: pair[1].value,
                })
                .collect(),
        }
    }

    pub fn sample(&self, time: f32) -> f32 {
        let Some((first_time, first_value)) = self.first else {
            return 0.0;
        };
        let time = time.clamp(0.0, 1.0);
        if time <= first_time {
            return first_value;
        }
        for segment in &self.segments {
            if time <= segment.end_time {
                let span = (segment.end_time - segment.start_time).max(f32::EPSILON);
                let x = ((time - segment.start_time) / span).clamp(0.0, 1.0);
                let smooth = x * x * (3.0 - 2.0 * x);
                return segment.start_value + (segment.end_value - segment.start_value) * smooth;
            }
        }
        self.last_value
    }

    pub fn first(&self) -> Option<(f32, f32)> {
        self.first
    }

    pub fn last_value(&self) -> f32 {
        self.last_value
    }

    pub fn segments(&self) -> &[CurveSegment] {
        &self.segments
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GradientSegment {
    pub start_time: f32,
    pub end_time: f32,
    pub start_color: [f32; 4],
    pub end_color: [f32; 4],
}

/// Gradient data stripped of authoring IDs and lowered into interpolation segments.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledGradient {
    first: Option<(f32, [f32; 4])>,
    last_color: [f32; 4],
    segments: Vec<GradientSegment>,
}

impl CompiledGradient {
    pub fn compile(gradient: &Gradient) -> Self {
        Self {
            first: gradient.keys.first().map(|key| (key.time, key.color)),
            last_color: gradient.keys.last().map_or([1.0; 4], |key| key.color),
            segments: gradient
                .keys
                .windows(2)
                .map(|pair| GradientSegment {
                    start_time: pair[0].time,
                    end_time: pair[1].time,
                    start_color: pair[0].color,
                    end_color: pair[1].color,
                })
                .collect(),
        }
    }

    pub fn sample(&self, time: f32) -> [f32; 4] {
        let Some((first_time, first_color)) = self.first else {
            return [1.0; 4];
        };
        let time = time.clamp(0.0, 1.0);
        if time <= first_time {
            return first_color;
        }
        for segment in &self.segments {
            if time <= segment.end_time {
                let x = ((time - segment.start_time)
                    / (segment.end_time - segment.start_time).max(f32::EPSILON))
                .clamp(0.0, 1.0);
                return std::array::from_fn(|index| {
                    segment.start_color[index]
                        + (segment.end_color[index] - segment.start_color[index]) * x
                });
            }
        }
        self.last_color
    }

    pub fn first(&self) -> Option<(f32, [f32; 4])> {
        self.first
    }

    pub fn last_color(&self) -> [f32; 4] {
        self.last_color
    }

    pub fn segments(&self) -> &[GradientSegment] {
        &self.segments
    }
}

/// Runtime-ready parameter value. Curves and gradients are compiled on ingress.
#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeValue {
    Bool(bool),
    U32(u32),
    Scalar(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Text(String),
    Range(ScalarRange),
    Curve(CompiledCurve),
    Gradient(CompiledGradient),
    Shape(EmitterShape),
    Asset(AssetId),
    Material(MaterialId),
}

impl RuntimeValue {
    pub fn compile(value: &Value) -> Option<Self> {
        Some(match value {
            Value::Bool(value) => Self::Bool(*value),
            Value::U32(value) => Self::U32(*value),
            Value::Scalar(value) => Self::Scalar(*value),
            Value::Vec2(value) => Self::Vec2(*value),
            Value::Vec3(value) => Self::Vec3(*value),
            Value::Vec4(value) => Self::Vec4(*value),
            Value::Text(value) => Self::Text(value.clone()),
            Value::Range(value) => Self::Range(*value),
            Value::Curve(value) => Self::Curve(CompiledCurve::compile(value)),
            Value::Gradient(value) => Self::Gradient(CompiledGradient::compile(value)),
            Value::Shape(value) => Self::Shape(*value),
            Value::Parameter(_) => return None,
            Value::Asset(value) => Self::Asset(*value),
            Value::Material(value) => Self::Material(*value),
        })
    }

    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Bool(_) => ValueType::Bool,
            Self::U32(_) => ValueType::U32,
            Self::Scalar(_) => ValueType::Scalar,
            Self::Vec2(_) => ValueType::Vec2,
            Self::Vec3(_) => ValueType::Vec3,
            Self::Vec4(_) => ValueType::Vec4,
            Self::Text(_) => ValueType::Text,
            Self::Range(_) => ValueType::Range,
            Self::Curve(_) => ValueType::Curve,
            Self::Gradient(_) => ValueType::Gradient,
            Self::Shape(_) => ValueType::Shape,
            Self::Asset(_) => ValueType::Asset,
            Self::Material(_) => ValueType::Material,
        }
    }
}

pub trait RuntimeParameterValue: Sized {
    fn from_runtime(value: &RuntimeValue) -> Option<&Self>;
}

macro_rules! runtime_parameter_value {
    ($type:ty, $variant:ident) => {
        impl RuntimeParameterValue for $type {
            fn from_runtime(value: &RuntimeValue) -> Option<&Self> {
                match value {
                    RuntimeValue::$variant(value) => Some(value),
                    _ => None,
                }
            }
        }
    };
}

runtime_parameter_value!(bool, Bool);
runtime_parameter_value!(u32, U32);
runtime_parameter_value!(f32, Scalar);
runtime_parameter_value!([f32; 2], Vec2);
runtime_parameter_value!([f32; 3], Vec3);
runtime_parameter_value!([f32; 4], Vec4);
runtime_parameter_value!(String, Text);
runtime_parameter_value!(ScalarRange, Range);
runtime_parameter_value!(CompiledCurve, Curve);
runtime_parameter_value!(CompiledGradient, Gradient);
runtime_parameter_value!(EmitterShape, Shape);
runtime_parameter_value!(AssetId, Asset);
runtime_parameter_value!(MaterialId, Material);

/// A typed operation in a compiled emitter execution plan.
#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    Emit {
        source: ModuleId,
        spawn_rate: Expression<f32>,
        burst_count: Expression<u32>,
    },
    SampleShape {
        source: ModuleId,
        shape: Expression<EmitterShape>,
    },
    Initialize {
        source: ModuleId,
        lifetime: Expression<ScalarRange>,
        speed: Expression<ScalarRange>,
        direction: Expression<[f32; 3]>,
        spread_degrees: Expression<f32>,
        angular_velocity: Expression<ScalarRange>,
    },
    Motion {
        source: ModuleId,
        gravity: Expression<[f32; 3]>,
        drag: Expression<f32>,
        turbulence: Expression<f32>,
    },
    Appearance {
        source: ModuleId,
        size: Expression<CompiledCurve>,
        opacity: Expression<CompiledCurve>,
        color: Expression<CompiledGradient>,
    },
}

impl Instruction {
    pub fn source(&self) -> ModuleId {
        match self {
            Self::Emit { source, .. }
            | Self::SampleShape { source, .. }
            | Self::Initialize { source, .. }
            | Self::Motion { source, .. }
            | Self::Appearance { source, .. } => *source,
        }
    }
}

/// Ordered operations grouped by semantic execution stage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExecutionPlan {
    pub emitter_update: Vec<Instruction>,
    pub particle_spawn: Vec<Instruction>,
    pub particle_update: Vec<Instruction>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RendererPlan {
    pub source: RendererId,
    pub material: MaterialId,
    pub kind: RendererPlanKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RendererPlanKind {
    Sprite,
    Flipbook {
        flipbook: AssetId,
        time_source: FlipbookTimeSource,
        playback: FlipbookPlaybackMode,
        random_start: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum MaterialColorPlan {
    ParticleColor,
    Value(Expression<[f32; 4]>),
}

/// One compiled material shared by every renderer that references its stable ID.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledMaterial {
    pub source: MaterialId,
    pub name: String,
    pub blend: BlendMode,
    pub softness: Expression<f32>,
    pub color: MaterialColorPlan,
    pub texture: Option<AssetId>,
    pub uv: UvRect,
}

/// One resolved entry in the compiled effect's renderer asset registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledAsset {
    pub source: AssetId,
    pub name: String,
    pub kind: AssetKind,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledFlipbook {
    pub source: AssetId,
    pub name: String,
    pub texture: AssetId,
    pub frames: Vec<UvRect>,
    pub frame_rate: f32,
    pub looping: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStage {
    EmitterUpdate,
    ParticleSpawn,
    ParticleUpdate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrLocation {
    pub emitter_index: usize,
    pub stage: RuntimeStage,
    pub instruction_index: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledEmitter {
    pub source: EmitterId,
    pub name: String,
    pub enabled: bool,
    pub transform: EmitterTransform,
    pub start_time: f32,
    pub duration: f32,
    pub max_particles: u32,
    pub execution: ExecutionPlan,
    pub renderers: Vec<RendererPlan>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledParameter {
    pub source: ParameterId,
    pub name: String,
    pub value_type: ValueType,
    pub default: RuntimeValue,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OptimizationStats {
    pub constant_expressions: usize,
    pub runtime_parameter_reads: usize,
    pub eliminated_attributes: usize,
}

/// Immutable, engine-independent output of the Aestra compiler.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledEffect {
    pub source: EffectId,
    pub name: String,
    pub duration: f32,
    pub looping: bool,
    pub seek_mode: SimulationSeekMode,
    pub assets: Vec<CompiledAsset>,
    pub flipbooks: Vec<CompiledFlipbook>,
    pub materials: Vec<CompiledMaterial>,
    pub parameters: Vec<CompiledParameter>,
    pub parameter_slots: BTreeMap<ParameterId, ParameterSlot>,
    pub particle_layout: ParticleLayout,
    pub emitters: Vec<CompiledEmitter>,
    pub effect_clips: Vec<CompiledEffectClip>,
    pub max_particles: usize,
    pub source_map: BTreeMap<ModuleId, IrLocation>,
    pub optimizations: OptimizationStats,
}

/// Compiled timing and deterministic-instance metadata for one reusable child effect.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledEffectClip {
    pub source_clip: EffectClipId,
    pub source: EffectAssetRef,
    pub start_time: f32,
    pub source_offset: f32,
    pub duration: f32,
    pub transform: EmitterTransform,
    pub seed: EffectClipSeed,
    pub parameter_overrides: Vec<CompiledParameterOverride>,
}

/// A validated, packed value replacing one exposed parameter on a child instance.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledParameterOverride {
    pub source: ParameterId,
    pub slot: ParameterSlot,
    pub value: RuntimeValue,
}

impl CompiledEffectClip {
    pub fn map_time(&self, parent_time: f32, child: &CompiledEffect) -> Option<f32> {
        let elapsed = parent_time - self.start_time;
        if elapsed < 0.0 || elapsed > self.duration {
            return None;
        }
        let local = self.source_offset + elapsed;
        Some(if child.looping && child.duration > 0.0 {
            local.rem_euclid(child.duration)
        } else {
            local.clamp(0.0, child.duration.max(0.0))
        })
    }
}

/// A compiled root plus the unique effects needed to execute all of its reusable clips.
#[derive(Debug, Clone)]
pub struct CompiledEffectProject {
    pub root: Arc<CompiledEffect>,
    pub dependencies: BTreeMap<EffectId, Arc<CompiledEffect>>,
}

impl CompiledEffectProject {
    pub fn effect(&self, id: EffectId) -> Option<&Arc<CompiledEffect>> {
        if self.root.source == id {
            Some(&self.root)
        } else {
            self.dependencies.get(&id)
        }
    }

    /// Deterministically evaluates the root and every active nested clip.
    pub fn evaluate(&self, time: f32, seed: u64, output: &mut Vec<ProjectParticleSample>) {
        output.clear();
        let mut path = Vec::new();
        let parameters = default_parameter_values(&self.root);
        evaluate_project_effect(self, &self.root, time, seed, &parameters, &mut path, output);
    }
}

/// One reference-runtime sample with enough provenance to render or profile a nested instance.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectParticleSample {
    pub effect: EffectId,
    pub instance_path: Vec<EffectClipId>,
    pub particle: ParticleSample,
}

fn evaluate_project_effect(
    project: &CompiledEffectProject,
    effect: &CompiledEffect,
    time: f32,
    seed: u64,
    parameters: &[RuntimeValue],
    path: &mut Vec<EffectClipId>,
    output: &mut Vec<ProjectParticleSample>,
) {
    // Project compilation rejects dependency cycles. Keep manually assembled runtime projects
    // bounded as a final defense against malformed external data.
    if path.len() >= 64 {
        return;
    }
    let effect_time = if effect.looping {
        time.rem_euclid(effect.duration)
    } else {
        time.clamp(0.0, effect.duration)
    };
    let mut local_samples = Vec::new();
    evaluate_with_parameters(effect, effect_time, seed, parameters, &mut local_samples);
    output.extend(
        local_samples
            .into_iter()
            .map(|particle| ProjectParticleSample {
                effect: effect.source,
                instance_path: path.clone(),
                particle,
            }),
    );

    for clip in &effect.effect_clips {
        let Some(child) = project.dependencies.get(&clip.source.id) else {
            continue;
        };
        let Some(child_time) = clip.map_time(effect_time, child) else {
            continue;
        };
        let mut child_parameters = default_parameter_values(child);
        apply_compiled_parameter_overrides(child, &clip.parameter_overrides, &mut child_parameters);
        path.push(clip.source_clip);
        evaluate_project_effect(
            project,
            child,
            child_time,
            clip.seed.resolve(seed, clip.source_clip),
            &child_parameters,
            path,
            output,
        );
        path.pop();
    }
}

impl CompiledEffect {
    pub fn material(&self, id: MaterialId) -> Option<&CompiledMaterial> {
        self.materials.iter().find(|material| material.source == id)
    }

    pub fn flipbook(&self, id: AssetId) -> Option<&CompiledFlipbook> {
        self.flipbooks.iter().find(|flipbook| flipbook.source == id)
    }
}

/// A renderer-neutral particle sample produced by the reference interpreter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleSample {
    pub emitter_index: usize,
    pub particle_index: u32,
    pub position: [f32; 3],
    pub size: f32,
    pub rotation: f32,
    pub color: [f32; 4],
    pub normalized_age: f32,
}

pub const DEFAULT_PLAYBACK_TICK_RATE: u32 = 60;

/// Exact timeline position shared by editor, viewer, and game playback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackClock {
    tick_rate: u32,
    frame: u64,
    accumulator: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaybackCheckpoint {
    pub tick_rate: u32,
    pub frame: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClockAdvance {
    pub ticks: u64,
    pub reached_end: bool,
}

impl Default for PlaybackClock {
    fn default() -> Self {
        Self::new(DEFAULT_PLAYBACK_TICK_RATE)
    }
}

impl PlaybackClock {
    pub fn new(tick_rate: u32) -> Self {
        Self {
            tick_rate: tick_rate.max(1),
            frame: 0,
            accumulator: 0.0,
        }
    }

    pub fn tick_rate(&self) -> u32 {
        self.tick_rate
    }

    pub fn frame(&self) -> u64 {
        self.frame
    }

    pub fn maximum_frame(&self, duration: f32) -> u64 {
        (f64::from(duration.max(0.0)) * f64::from(self.tick_rate)).ceil() as u64
    }

    pub fn time(&self, duration: f32) -> f32 {
        self.time_for_frame(self.frame, duration)
    }

    pub fn time_for_frame(&self, frame: u64, duration: f32) -> f32 {
        (frame as f64 / f64::from(self.tick_rate)).min(f64::from(duration.max(0.0))) as f32
    }

    pub fn restart(&mut self) {
        self.frame = 0;
        self.accumulator = 0.0;
    }

    pub fn seek_frame(&mut self, frame: u64, duration: f32) {
        self.frame = frame.min(self.maximum_frame(duration));
        self.accumulator = 0.0;
    }

    pub fn seek_seconds(&mut self, time: f32, duration: f32) {
        let time = time.clamp(0.0, duration.max(0.0));
        let frame = (f64::from(time) * f64::from(self.tick_rate)).round() as u64;
        self.seek_frame(frame, duration);
    }

    pub fn step_forward(&mut self, duration: f32) {
        self.seek_frame(self.frame.saturating_add(1), duration);
    }

    pub fn step_back(&mut self, duration: f32) {
        self.seek_frame(self.frame.saturating_sub(1), duration);
    }

    pub fn advance(
        &mut self,
        delta_seconds: f32,
        speed: f32,
        duration: f32,
        looping: bool,
    ) -> ClockAdvance {
        let scaled_delta = f64::from(delta_seconds.max(0.0)) * f64::from(speed.max(0.0));
        if !scaled_delta.is_finite() || scaled_delta == 0.0 {
            return ClockAdvance::default();
        }
        let tick_duration = 1.0 / f64::from(self.tick_rate);
        self.accumulator += scaled_delta;
        let ticks = ((self.accumulator + tick_duration * 1.0e-9) / tick_duration).floor() as u64;
        if ticks == 0 {
            return ClockAdvance::default();
        }
        self.accumulator = (self.accumulator - ticks as f64 * tick_duration).max(0.0);
        let maximum = self.maximum_frame(duration);
        if looping {
            self.frame = if maximum == 0 {
                0
            } else {
                self.frame.wrapping_add(ticks) % maximum
            };
            ClockAdvance {
                ticks,
                reached_end: false,
            }
        } else {
            let next = self.frame.saturating_add(ticks);
            self.frame = next.min(maximum);
            let reached_end = next >= maximum;
            if reached_end {
                self.accumulator = 0.0;
            }
            ClockAdvance { ticks, reached_end }
        }
    }

    pub fn checkpoint(&self) -> PlaybackCheckpoint {
        PlaybackCheckpoint {
            tick_rate: self.tick_rate,
            frame: self.frame,
        }
    }

    pub fn restore(&mut self, checkpoint: PlaybackCheckpoint, duration: f32) {
        self.tick_rate = checkpoint.tick_rate.max(1);
        self.seek_frame(checkpoint.frame, duration);
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParameterError {
    #[error("compiled effect has no runtime parameter {0}")]
    Unknown(ParameterId),
    #[error("parameter {id} expected {expected:?}, got {actual:?}")]
    TypeMismatch {
        id: ParameterId,
        expected: ValueType,
        actual: ValueType,
    },
}

/// Mutable playback state for one immutable compiled effect.
#[derive(Debug, Clone)]
pub struct EffectInstance {
    effect: Arc<CompiledEffect>,
    time: f32,
    seed: u64,
    parameters: Vec<RuntimeValue>,
    overridden: BTreeSet<ParameterSlot>,
}

impl EffectInstance {
    pub fn new(effect: Arc<CompiledEffect>) -> Self {
        let parameters = effect
            .parameters
            .iter()
            .map(|parameter| parameter.default.clone())
            .collect();
        Self {
            effect,
            time: 0.0,
            seed: 0,
            parameters,
            overridden: BTreeSet::new(),
        }
    }

    pub fn with_seed(effect: Arc<CompiledEffect>, seed: u64) -> Self {
        Self {
            seed,
            ..Self::new(effect)
        }
    }

    pub fn effect(&self) -> &Arc<CompiledEffect> {
        &self.effect
    }

    pub fn time(&self) -> f32 {
        self.time
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn set_seed(&mut self, seed: u64) {
        self.seed = seed;
    }

    pub fn set_parameter(&mut self, id: ParameterId, value: Value) -> Result<(), ParameterError> {
        let Some(slot) = self.effect.parameter_slots.get(&id).copied() else {
            return Err(ParameterError::Unknown(id));
        };
        let expected = self.effect.parameters[slot.0].value_type;
        let actual = value.value_type();
        if actual != expected {
            return Err(ParameterError::TypeMismatch {
                id,
                expected,
                actual,
            });
        }
        let compiled = RuntimeValue::compile(&value).ok_or(ParameterError::TypeMismatch {
            id,
            expected,
            actual,
        })?;
        self.parameters[slot.0] = compiled;
        self.overridden.insert(slot);
        Ok(())
    }

    /// Applies compiler-validated values authored on a reusable effect clip.
    pub fn apply_compiled_parameter_overrides(&mut self, overrides: &[CompiledParameterOverride]) {
        apply_compiled_parameter_overrides(&self.effect, overrides, &mut self.parameters);
        self.overridden
            .extend(overrides.iter().map(|parameter| parameter.slot));
    }

    pub fn clear_parameter(&mut self, id: ParameterId) -> Result<(), ParameterError> {
        let Some(slot) = self.effect.parameter_slots.get(&id).copied() else {
            return Err(ParameterError::Unknown(id));
        };
        self.parameters[slot.0] = self.effect.parameters[slot.0].default.clone();
        self.overridden.remove(&slot);
        Ok(())
    }

    pub fn parameter(&self, id: ParameterId) -> Option<&RuntimeValue> {
        self.effect
            .parameter_slots
            .get(&id)
            .and_then(|slot| self.parameters.get(slot.0))
    }

    /// Packed values used by compiled expressions and GPU artifact generation.
    pub fn parameter_values(&self) -> &[RuntimeValue] {
        &self.parameters
    }

    pub fn overridden_parameters(
        &self,
    ) -> impl Iterator<Item = (&CompiledParameter, &RuntimeValue)> {
        self.overridden
            .iter()
            .map(|slot| (&self.effect.parameters[slot.0], &self.parameters[slot.0]))
    }

    pub fn seek(&mut self, time: f32) {
        self.time = time.clamp(0.0, self.effect.duration);
    }

    pub fn restart(&mut self) {
        self.time = 0.0;
    }

    pub fn advance(&mut self, delta_seconds: f32) {
        let next = self.time + delta_seconds;
        self.time = if self.effect.looping {
            next.rem_euclid(self.effect.duration)
        } else {
            next.clamp(0.0, self.effect.duration)
        };
    }

    pub fn evaluate(&self, output: &mut Vec<ParticleSample>) {
        evaluate_with_parameters(&self.effect, self.time, self.seed, &self.parameters, output);
    }
}

/// Executes a compiled effect with its default parameter values.
pub fn evaluate(effect: &CompiledEffect, time: f32, seed: u64, output: &mut Vec<ParticleSample>) {
    let parameters = default_parameter_values(effect);
    evaluate_with_parameters(effect, time, seed, &parameters, output);
}

fn default_parameter_values(effect: &CompiledEffect) -> Vec<RuntimeValue> {
    effect
        .parameters
        .iter()
        .map(|parameter| parameter.default.clone())
        .collect()
}

fn apply_compiled_parameter_overrides(
    effect: &CompiledEffect,
    overrides: &[CompiledParameterOverride],
    parameters: &mut [RuntimeValue],
) {
    for parameter in overrides {
        debug_assert_eq!(effect.parameters[parameter.slot.0].source, parameter.source);
        parameters[parameter.slot.0] = parameter.value.clone();
    }
}

fn evaluate_with_parameters(
    effect: &CompiledEffect,
    time: f32,
    seed: u64,
    parameters: &[RuntimeValue],
    output: &mut Vec<ParticleSample>,
) {
    output.clear();
    let effect_time = if effect.looping {
        time.rem_euclid(effect.duration)
    } else {
        time.clamp(0.0, effect.duration)
    };

    for (emitter_index, emitter) in effect.emitters.iter().enumerate() {
        if !emitter.enabled {
            continue;
        }
        let local_time = effect_time - emitter.start_time;
        if local_time < 0.0 || local_time > emitter.duration {
            continue;
        }

        let Some((spawn_rate, burst_count)) = emission(&emitter.execution, parameters) else {
            continue;
        };
        let Some(shape) = shape(&emitter.execution, parameters) else {
            continue;
        };
        let Some(initializer) = initializer(&emitter.execution, parameters) else {
            continue;
        };
        let motion = motion(&emitter.execution, parameters).unwrap_or(Motion {
            gravity: [0.0, 0.0, 0.0],
            drag: 0.0,
            turbulence: 0.0,
        });
        let Some(appearance) = appearance(&emitter.execution, parameters) else {
            continue;
        };

        let emission_count =
            burst_count.saturating_add((local_time * spawn_rate).floor().max(0.0) as u32);
        let count = emission_count.min(emitter.max_particles);
        for index in 0..count {
            let spawn_time = if index < burst_count {
                0.0
            } else if spawn_rate > 0.0 {
                (index - burst_count) as f32 / spawn_rate
            } else {
                continue;
            };
            let age = local_time - spawn_time;
            let life = initializer.lifetime.sample(hash01(index, 0, seed));
            if age < 0.0 || age >= life || life <= 0.0 {
                continue;
            }

            let normalized_age = age / life;
            let direction = sample_direction(
                initializer.direction,
                initializer.spread_degrees,
                index,
                seed,
            );
            let speed = initializer.speed.sample(hash01(index, 2, seed));
            let origin = sample_shape(shape, index, seed);
            let damping = (-motion.drag.max(0.0) * age).exp();
            let travel = if motion.drag.abs() < 0.0001 {
                speed * age
            } else {
                speed * (1.0 - damping) / motion.drag.max(0.0001)
            };
            let turbulence = [
                motion.turbulence
                    * (age * 7.0 + hash01(index, 3, seed) * std::f32::consts::TAU).sin(),
                motion.turbulence
                    * (age * 6.3 + hash01(index, 8, seed) * std::f32::consts::TAU).sin(),
                motion.turbulence
                    * (age * 7.7 + hash01(index, 10, seed) * std::f32::consts::TAU).sin(),
            ];
            let local_position = [
                origin[0]
                    + direction[0] * travel
                    + motion.gravity[0] * age * age * 0.5
                    + turbulence[0],
                origin[1]
                    + direction[1] * travel
                    + motion.gravity[1] * age * age * 0.5
                    + turbulence[1],
                origin[2]
                    + direction[2] * travel
                    + motion.gravity[2] * age * age * 0.5
                    + turbulence[2],
            ];
            let position = transform_emitter_position(emitter.transform, local_position);
            let mut color = appearance.color.sample(normalized_age);
            color[3] *= appearance.opacity.sample(normalized_age);
            output.push(ParticleSample {
                emitter_index,
                particle_index: index,
                position,
                size: appearance.size.sample(normalized_age)
                    * emitter.transform.scale.into_iter().fold(0.0_f32, f32::max),
                rotation: initializer.angular_velocity.sample(hash01(index, 4, seed)) * age,
                color,
                normalized_age,
            });
        }
    }
}

fn transform_emitter_position(transform: EmitterTransform, position: [f32; 3]) -> [f32; 3] {
    let scaled = [
        position[0] * transform.scale[0],
        position[1] * transform.scale[1],
        position[2] * transform.scale[2],
    ];
    let [qx, qy, qz, qw] = transform.rotation;
    let length = (qx * qx + qy * qy + qz * qz + qw * qw).sqrt();
    let (qx, qy, qz, qw) = (qx / length, qy / length, qz / length, qw / length);
    let tx = 2.0 * (qy * scaled[2] - qz * scaled[1]);
    let ty = 2.0 * (qz * scaled[0] - qx * scaled[2]);
    let tz = 2.0 * (qx * scaled[1] - qy * scaled[0]);
    [
        scaled[0] + qw * tx + (qy * tz - qz * ty) + transform.translation[0],
        scaled[1] + qw * ty + (qz * tx - qx * tz) + transform.translation[1],
        scaled[2] + qw * tz + (qx * ty - qy * tx) + transform.translation[2],
    ]
}

/// Selects a frame deterministically for every presentation backend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlipbookFrameContext {
    pub time_source: FlipbookTimeSource,
    pub playback: FlipbookPlaybackMode,
    pub random_start: bool,
    pub effect_time: f32,
    pub normalized_age: f32,
    pub particle_index: u32,
    pub seed: u64,
}

pub fn flipbook_frame_index(flipbook: &CompiledFlipbook, context: FlipbookFrameContext) -> usize {
    let count = flipbook.frames.len();
    if count <= 1 {
        return 0;
    }
    let seconds = match context.time_source {
        FlipbookTimeSource::ParticleAge => {
            context.normalized_age.clamp(0.0, 1.0) * count as f32 / flipbook.frame_rate
        }
        FlipbookTimeSource::EffectTime => context.effect_time.max(0.0),
    };
    let mut frame = (seconds * flipbook.frame_rate).floor() as usize;
    if context.random_start {
        frame = frame.wrapping_add(
            (hash01(context.particle_index, 9, context.seed) * count as f32) as usize,
        );
    }
    let forward = if flipbook.looping {
        frame % count
    } else {
        frame.min(count - 1)
    };
    match context.playback {
        FlipbookPlaybackMode::Forward => forward,
        FlipbookPlaybackMode::Reverse => count - 1 - forward,
        FlipbookPlaybackMode::PingPong => {
            let period = (count - 1) * 2;
            let value = if flipbook.looping {
                frame % period
            } else {
                frame.min(period)
            };
            if value < count { value } else { period - value }
        }
    }
}

fn emission(plan: &ExecutionPlan, parameters: &[RuntimeValue]) -> Option<(f32, u32)> {
    plan.emitter_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Emit {
                spawn_rate,
                burst_count,
                ..
            } => Some((
                *spawn_rate.resolve(parameters),
                *burst_count.resolve(parameters),
            )),
            _ => None,
        })
}

fn shape(plan: &ExecutionPlan, parameters: &[RuntimeValue]) -> Option<EmitterShape> {
    plan.particle_spawn
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::SampleShape { shape, .. } => Some(*shape.resolve(parameters)),
            _ => None,
        })
}

struct Initializer {
    lifetime: ScalarRange,
    speed: ScalarRange,
    direction: [f32; 3],
    spread_degrees: f32,
    angular_velocity: ScalarRange,
}

fn initializer(plan: &ExecutionPlan, parameters: &[RuntimeValue]) -> Option<Initializer> {
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
            } => Some(Initializer {
                lifetime: *lifetime.resolve(parameters),
                speed: *speed.resolve(parameters),
                direction: *direction.resolve(parameters),
                spread_degrees: *spread_degrees.resolve(parameters),
                angular_velocity: *angular_velocity.resolve(parameters),
            }),
            _ => None,
        })
}

struct Motion {
    gravity: [f32; 3],
    drag: f32,
    turbulence: f32,
}

fn motion(plan: &ExecutionPlan, parameters: &[RuntimeValue]) -> Option<Motion> {
    plan.particle_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Motion {
                gravity,
                drag,
                turbulence,
                ..
            } => Some(Motion {
                gravity: *gravity.resolve(parameters),
                drag: *drag.resolve(parameters),
                turbulence: *turbulence.resolve(parameters),
            }),
            _ => None,
        })
}

struct Appearance<'a> {
    size: &'a CompiledCurve,
    opacity: &'a CompiledCurve,
    color: &'a CompiledGradient,
}

fn appearance<'a>(
    plan: &'a ExecutionPlan,
    parameters: &'a [RuntimeValue],
) -> Option<Appearance<'a>> {
    plan.particle_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Appearance {
                size,
                opacity,
                color,
                ..
            } => Some(Appearance {
                size: size.resolve(parameters),
                opacity: opacity.resolve(parameters),
                color: color.resolve(parameters),
            }),
            _ => None,
        })
}

fn sample_shape(shape: EmitterShape, index: u32, seed: u64) -> [f32; 3] {
    let angle = hash01(index, 5, seed) * std::f32::consts::TAU;
    match shape {
        EmitterShape::Point => [0.0; 3],
        EmitterShape::Circle { radius } => {
            let radius = radius * hash01(index, 6, seed).sqrt();
            [angle.cos() * radius, angle.sin() * radius, 0.0]
        }
        EmitterShape::Ring { radius } => [angle.cos() * radius, angle.sin() * radius, 0.0],
        EmitterShape::Sphere { radius } => {
            let direction = sample_unit_sphere(index, seed, 6, 5);
            scale3(direction, radius * hash01(index, 8, seed).cbrt())
        }
        EmitterShape::Hemisphere { radius } => {
            let phi = std::f32::consts::TAU * hash01(index, 5, seed);
            let y = hash01(index, 6, seed);
            let radial = (1.0 - y * y).sqrt();
            scale3(
                [phi.cos() * radial, y, phi.sin() * radial],
                radius * hash01(index, 8, seed).cbrt(),
            )
        }
        EmitterShape::Box { half_extents } => [
            (hash01(index, 5, seed) * 2.0 - 1.0) * half_extents[0],
            (hash01(index, 6, seed) * 2.0 - 1.0) * half_extents[1],
            (hash01(index, 7, seed) * 2.0 - 1.0) * half_extents[2],
        ],
        EmitterShape::Cylinder { radius, depth } => {
            let sampled_radius = radius * hash01(index, 6, seed).sqrt();
            [
                angle.cos() * sampled_radius,
                (hash01(index, 7, seed) - 0.5) * depth,
                angle.sin() * sampled_radius,
            ]
        }
        EmitterShape::Cone { radius, depth } => {
            let y = hash01(index, 6, seed) * depth;
            let sampled_radius = radius * (y / depth.max(0.001)) * hash01(index, 7, seed).sqrt();
            [
                angle.cos() * sampled_radius,
                y,
                angle.sin() * sampled_radius,
            ]
        }
    }
}

fn sample_direction(direction: [f32; 3], spread_degrees: f32, index: u32, seed: u64) -> [f32; 3] {
    let forward = normalize3(direction);
    let half_angle = (spread_degrees.abs() * 0.5)
        .to_radians()
        .min(std::f32::consts::PI);
    if half_angle <= f32::EPSILON {
        return forward;
    }
    let cos_theta = 1.0 - hash01(index, 1, seed) * (1.0 - half_angle.cos());
    let sin_theta = (1.0 - cos_theta * cos_theta).sqrt();
    let phi = hash01(index, 11, seed) * std::f32::consts::TAU;
    let helper = if forward[1].abs() < 0.999 {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let tangent = normalize3(cross3(helper, forward));
    let bitangent = cross3(forward, tangent);
    add3(
        scale3(forward, cos_theta),
        add3(
            scale3(tangent, sin_theta * phi.cos()),
            scale3(bitangent, sin_theta * phi.sin()),
        ),
    )
}

fn sample_unit_sphere(index: u32, seed: u64, z_channel: u32, angle_channel: u32) -> [f32; 3] {
    let y = hash01(index, z_channel, seed) * 2.0 - 1.0;
    let angle = hash01(index, angle_channel, seed) * std::f32::consts::TAU;
    let radial = (1.0 - y * y).sqrt();
    [angle.cos() * radial, y, angle.sin() * radial]
}

fn normalize3(value: [f32; 3]) -> [f32; 3] {
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    if length <= f32::EPSILON {
        [0.0, 1.0, 0.0]
    } else {
        scale3(value, 1.0 / length)
    }
}

fn cross3(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn add3(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn scale3(value: [f32; 3], scale: f32) -> [f32; 3] {
    [value[0] * scale, value[1] * scale, value[2] * scale]
}

fn hash01(index: u32, channel: u32, seed: u64) -> f32 {
    let seed = (seed as u32) ^ ((seed >> 32) as u32);
    let mut value = index.wrapping_mul(0x9E37_79B9) ^ channel.wrapping_mul(0x85EB_CA6B) ^ seed;
    value ^= value >> 16;
    value = value.wrapping_mul(0x7FEB_352D);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846C_A68B);
    value ^= value >> 16;
    (value as f64 / u32::MAX as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumetric_shapes_sample_all_three_axes() {
        let sphere = (0..32)
            .map(|index| sample_shape(EmitterShape::Sphere { radius: 4.0 }, index, 42))
            .collect::<Vec<_>>();
        assert!(sphere.iter().any(|position| position[2].abs() > 0.01));
        assert!(sphere.iter().all(|position| {
            position[0] * position[0] + position[1] * position[1] + position[2] * position[2]
                <= 16.0001
        }));

        let box_sample = sample_shape(
            EmitterShape::Box {
                half_extents: [2.0, 3.0, 4.0],
            },
            7,
            42,
        );
        assert!(box_sample[0].abs() <= 2.0);
        assert!(box_sample[1].abs() <= 3.0);
        assert!(box_sample[2].abs() <= 4.0);
    }

    #[test]
    fn emitter_transform_scales_rotates_and_translates_particle_positions() {
        let transform = EmitterTransform {
            translation: [3.0, 4.0, 5.0],
            rotation: [
                0.0,
                0.0,
                std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
            scale: [2.0, 1.0, 1.0],
        };
        let transformed = transform_emitter_position(transform, [1.0, 0.0, 0.0]);
        assert!((transformed[0] - 3.0).abs() < 0.0001);
        assert!((transformed[1] - 6.0).abs() < 0.0001);
        assert!((transformed[2] - 5.0).abs() < 0.0001);
    }

    #[test]
    fn launch_direction_uses_a_three_dimensional_solid_angle() {
        assert_eq!(
            sample_direction([0.0, 2.0, 0.0], 0.0, 0, 42),
            [0.0, 1.0, 0.0]
        );
        let directions = (0..32)
            .map(|index| sample_direction([0.0, 1.0, 0.0], 180.0, index, 42))
            .collect::<Vec<_>>();
        assert!(directions.iter().any(|direction| direction[2].abs() > 0.01));
        assert!(directions.iter().all(|direction| {
            let length = (direction[0] * direction[0]
                + direction[1] * direction[1]
                + direction[2] * direction[2])
                .sqrt();
            (length - 1.0).abs() < 0.0001
        }));
    }

    #[test]
    fn fixed_clock_is_independent_of_render_delta_partitioning() {
        let mut fine = PlaybackClock::default();
        let mut coarse = PlaybackClock::default();
        for _ in 0..120 {
            fine.advance(1.0 / 120.0, 1.0, 4.0, false);
        }
        for _ in 0..10 {
            coarse.advance(0.1, 1.0, 4.0, false);
        }
        assert_eq!(fine.frame(), 60);
        assert_eq!(fine.frame(), coarse.frame());
        assert_eq!(fine.time(4.0), 1.0);
    }

    #[test]
    fn clock_steps_seeks_and_restores_exact_frames() {
        let mut clock = PlaybackClock::default();
        clock.seek_seconds(0.126, 2.0);
        assert_eq!(clock.frame(), 8);
        assert_eq!(clock.time(2.0), 8.0 / 60.0);
        let checkpoint = clock.checkpoint();
        clock.step_forward(2.0);
        clock.step_back(2.0);
        assert_eq!(clock.frame(), checkpoint.frame);
        clock.restart();
        clock.restore(checkpoint, 2.0);
        assert_eq!(clock.frame(), 8);
    }

    #[test]
    fn non_looping_clock_stops_while_looping_clock_wraps() {
        let mut stopped = PlaybackClock::default();
        let result = stopped.advance(2.0, 1.0, 1.0, false);
        assert_eq!(stopped.frame(), 60);
        assert!(result.reached_end);

        let mut looping = PlaybackClock::default();
        let result = looping.advance(1.25, 1.0, 1.0, true);
        assert_eq!(looping.frame(), 15);
        assert!(!result.reached_end);
    }

    #[test]
    fn flipbook_frame_selection_supports_modes_and_is_deterministic() {
        let flipbook = CompiledFlipbook {
            source: AssetId::new(),
            name: "Test".into(),
            texture: AssetId::new(),
            frames: vec![UvRect::FULL; 4],
            frame_rate: 4.0,
            looping: true,
        };
        let frame = |playback, random_start| {
            flipbook_frame_index(
                &flipbook,
                FlipbookFrameContext {
                    time_source: FlipbookTimeSource::EffectTime,
                    playback,
                    random_start,
                    effect_time: 0.5,
                    normalized_age: 0.0,
                    particle_index: 7,
                    seed: 42,
                },
            )
        };
        assert_eq!(frame(FlipbookPlaybackMode::Forward, false), 2);
        assert_eq!(frame(FlipbookPlaybackMode::Reverse, false), 1);
        assert_eq!(frame(FlipbookPlaybackMode::PingPong, false), 2);
        assert_eq!(
            frame(FlipbookPlaybackMode::Forward, true),
            frame(FlipbookPlaybackMode::Forward, true)
        );
    }
}
