//! Engine-independent compiled effect contracts and deterministic CPU execution.

use aestra_core::{
    BlendMode, Curve, EffectId, EmitterId, EmitterShape, Gradient, ModuleId, ParameterId,
    RendererId, ScalarRange, Value,
};
use std::{collections::BTreeMap, sync::Arc};

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
}

/// A typed operation in a compiled emitter execution plan.
#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    Emit {
        source: ModuleId,
        spawn_rate: f32,
        burst_count: u32,
    },
    SampleShape {
        source: ModuleId,
        shape: EmitterShape,
    },
    Initialize {
        source: ModuleId,
        lifetime: ScalarRange,
        speed: ScalarRange,
        direction_degrees: f32,
        spread_degrees: f32,
        angular_velocity: ScalarRange,
    },
    Motion {
        source: ModuleId,
        gravity: [f32; 2],
        drag: f32,
        turbulence: f32,
    },
    Appearance {
        source: ModuleId,
        size: Curve,
        opacity: Curve,
        color: Gradient,
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

/// Immutable, engine-independent output of the Aestra compiler.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledEffect {
    pub source: EffectId,
    pub name: String,
    pub duration: f32,
    pub looping: bool,
    pub particle_layout: ParticleLayout,
    pub emitters: Vec<CompiledEmitter>,
    pub max_particles: usize,
    pub source_map: BTreeMap<ModuleId, IrLocation>,
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

/// Mutable playback state for one immutable compiled effect.
#[derive(Debug, Clone)]
pub struct EffectInstance {
    effect: Arc<CompiledEffect>,
    time: f32,
    seed: u64,
    parameter_overrides: BTreeMap<ParameterId, Value>,
}

impl EffectInstance {
    pub fn new(effect: Arc<CompiledEffect>) -> Self {
        Self {
            effect,
            time: 0.0,
            seed: 0,
            parameter_overrides: BTreeMap::new(),
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

    pub fn parameter_overrides(&self) -> &BTreeMap<ParameterId, Value> {
        &self.parameter_overrides
    }

    pub fn set_parameter(&mut self, id: ParameterId, value: Value) -> Option<Value> {
        self.parameter_overrides.insert(id, value)
    }

    pub fn clear_parameter(&mut self, id: ParameterId) -> Option<Value> {
        self.parameter_overrides.remove(&id)
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
        evaluate(&self.effect, self.time, self.seed, output);
    }
}

/// Executes a compiled effect at an arbitrary time using the deterministic CPU backend.
pub fn evaluate(effect: &CompiledEffect, time: f32, seed: u64, output: &mut Vec<ParticleSample>) {
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

        let Some((spawn_rate, burst_count)) = emission(&emitter.execution) else {
            continue;
        };
        let Some(shape) = shape(&emitter.execution) else {
            continue;
        };
        let Some(initializer) = initializer(&emitter.execution) else {
            continue;
        };
        let motion = motion(&emitter.execution).unwrap_or(Motion {
            gravity: [0.0, 0.0],
            drag: 0.0,
            turbulence: 0.0,
        });
        let Some(appearance) = appearance(&emitter.execution) else {
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

fn emission(plan: &ExecutionPlan) -> Option<(f32, u32)> {
    plan.emitter_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Emit {
                spawn_rate,
                burst_count,
                ..
            } => Some((*spawn_rate, *burst_count)),
            _ => None,
        })
}

fn shape(plan: &ExecutionPlan) -> Option<EmitterShape> {
    plan.particle_spawn
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::SampleShape { shape, .. } => Some(*shape),
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

fn initializer(plan: &ExecutionPlan) -> Option<Initializer> {
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
                lifetime: *lifetime,
                speed: *speed,
                direction_degrees: *direction_degrees,
                spread_degrees: *spread_degrees,
                angular_velocity: *angular_velocity,
            }),
            _ => None,
        })
}

struct Motion {
    gravity: [f32; 2],
    drag: f32,
    turbulence: f32,
}

fn motion(plan: &ExecutionPlan) -> Option<Motion> {
    plan.particle_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Motion {
                gravity,
                drag,
                turbulence,
                ..
            } => Some(Motion {
                gravity: *gravity,
                drag: *drag,
                turbulence: *turbulence,
            }),
            _ => None,
        })
}

struct Appearance<'a> {
    size: &'a Curve,
    opacity: &'a Curve,
    color: &'a Gradient,
}

fn appearance(plan: &ExecutionPlan) -> Option<Appearance<'_>> {
    plan.particle_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Appearance {
                size,
                opacity,
                color,
                ..
            } => Some(Appearance {
                size,
                opacity,
                color,
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
