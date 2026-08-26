//! Engine-independent compiled effect contracts and deterministic CPU execution.

use aestra_core::{
    AssetId, BlendMode, Curve, EffectId, EmitterId, EmitterShape, Gradient, MaterialId, ModuleId,
    ParameterId, RendererId, ScalarRange, Value, ValueType,
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
        direction_degrees: Expression<f32>,
        spread_degrees: Expression<f32>,
        angular_velocity: Expression<ScalarRange>,
    },
    Motion {
        source: ModuleId,
        gravity: Expression<[f32; 2]>,
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
    pub blend: BlendMode,
    pub softness: f32,
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
    pub parameters: Vec<CompiledParameter>,
    pub parameter_slots: BTreeMap<ParameterId, ParameterSlot>,
    pub particle_layout: ParticleLayout,
    pub emitters: Vec<CompiledEmitter>,
    pub max_particles: usize,
    pub source_map: BTreeMap<ModuleId, IrLocation>,
    pub optimizations: OptimizationStats,
}

/// A renderer-neutral particle sample produced by the reference interpreter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleSample {
    pub emitter_index: usize,
    pub position: [f32; 2],
    pub size: f32,
    pub rotation: f32,
    pub color: [f32; 4],
    pub normalized_age: f32,
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
    let parameters = effect
        .parameters
        .iter()
        .map(|parameter| parameter.default.clone())
        .collect::<Vec<_>>();
    evaluate_with_parameters(effect, time, seed, &parameters, output);
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
            gravity: [0.0, 0.0],
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
            let direction = (initializer.direction_degrees
                + (hash01(index, 1, seed) - 0.5) * initializer.spread_degrees)
                .to_radians();
            let speed = initializer.speed.sample(hash01(index, 2, seed));
            let (origin_x, origin_y) = sample_shape(shape, index, seed);
            let damping = (-motion.drag.max(0.0) * age).exp();
            let travel = if motion.drag.abs() < 0.0001 {
                speed * age
            } else {
                speed * (1.0 - damping) / motion.drag.max(0.0001)
            };
            let turbulence = motion.turbulence
                * (age * 7.0 + hash01(index, 3, seed) * std::f32::consts::TAU).sin();
            let position = [
                origin_x + direction.cos() * travel + turbulence,
                origin_y
                    + direction.sin() * travel
                    + motion.gravity[1] * age * age * 0.5
                    + motion.gravity[0] * age * 0.1,
            ];
            let mut color = appearance.color.sample(normalized_age);
            color[3] *= appearance.opacity.sample(normalized_age);
            output.push(ParticleSample {
                emitter_index,
                position,
                size: appearance.size.sample(normalized_age),
                rotation: initializer.angular_velocity.sample(hash01(index, 4, seed)) * age,
                color,
                normalized_age,
            });
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
    direction_degrees: f32,
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
                direction_degrees,
                spread_degrees,
                angular_velocity,
                ..
            } => Some(Initializer {
                lifetime: *lifetime.resolve(parameters),
                speed: *speed.resolve(parameters),
                direction_degrees: *direction_degrees.resolve(parameters),
                spread_degrees: *spread_degrees.resolve(parameters),
                angular_velocity: *angular_velocity.resolve(parameters),
            }),
            _ => None,
        })
}

struct Motion {
    gravity: [f32; 2],
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

fn sample_shape(shape: EmitterShape, index: u32, seed: u64) -> (f32, f32) {
    let angle = hash01(index, 5, seed) * std::f32::consts::TAU;
    match shape {
        EmitterShape::Point => (0.0, 0.0),
        EmitterShape::Circle { radius } => {
            let radius = radius * hash01(index, 6, seed).sqrt();
            (angle.cos() * radius, angle.sin() * radius)
        }
        EmitterShape::Ring { radius } => (angle.cos() * radius, angle.sin() * radius),
        EmitterShape::Cone { radius, depth } => {
            let y = hash01(index, 6, seed) * depth;
            let x = (hash01(index, 7, seed) * 2.0 - 1.0) * radius * (y / depth.max(0.001));
            (x, y)
        }
    }
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
