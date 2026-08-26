//! Aestra's shared effect asset model, deterministic evaluator, and Bevy playback plugin.

use serde::{Deserialize, Serialize};
use std::{fs, path::Path, sync::Arc};
use thiserror::Error;

use bevy::prelude::*;

pub const CURRENT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffectAsset {
    pub format_version: u32,
    pub id: String,
    pub name: String,
    pub duration: f32,
    pub looping: bool,
    #[serde(default)]
    pub layers: Vec<EffectLayer>,
    #[serde(default)]
    pub events: Vec<EventLink>,
}

impl EffectAsset {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.format_version != CURRENT_FORMAT_VERSION {
            return Err(ValidationError::UnsupportedVersion(self.format_version));
        }
        if self.id.trim().is_empty() {
            return Err(ValidationError::EmptyId);
        }
        if !self.duration.is_finite() || self.duration <= 0.0 {
            return Err(ValidationError::InvalidDuration(self.duration));
        }
        for layer in &self.layers {
            layer.validate()?;
        }
        Ok(())
    }

    pub fn from_ron(source: &str) -> Result<Self, AssetError> {
        let asset: Self = ron::from_str(source)?;
        asset.validate()?;
        Ok(asset)
    }

    pub fn load_ron(path: impl AsRef<Path>) -> Result<Self, AssetError> {
        Self::from_ron(&fs::read_to_string(path)?)
    }

    pub fn to_pretty_ron(&self) -> Result<String, AssetError> {
        self.validate()?;
        Ok(ron::ser::to_string_pretty(
            self,
            ron::ser::PrettyConfig::new().depth_limit(8),
        )?)
    }

    pub fn save_ron(&self, path: impl AsRef<Path>) -> Result<(), AssetError> {
        fs::write(path, self.to_pretty_ron()?)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffectLayer {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub start_time: f32,
    pub duration: f32,
    pub blend: BlendMode,
    pub emitter: Emitter,
    pub renderer: Renderer,
}

impl EffectLayer {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.id.trim().is_empty() {
            return Err(ValidationError::EmptyLayerId);
        }
        if self.start_time < 0.0 || !self.duration.is_finite() || self.duration <= 0.0 {
            return Err(ValidationError::InvalidLayerTiming(self.id.clone()));
        }
        if self.emitter.spawn_rate < 0.0 || self.emitter.max_particles == 0 {
            return Err(ValidationError::InvalidEmitter(self.id.clone()));
        }
        self.emitter.lifetime.validate(&self.id, "lifetime")?;
        self.emitter.speed.validate(&self.id, "speed")?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BlendMode {
    Alpha,
    Additive,
    Multiply,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Emitter {
    pub shape: EmitterShape,
    pub spawn_rate: f32,
    pub burst_count: u32,
    pub max_particles: u32,
    pub lifetime: ScalarRange,
    pub speed: ScalarRange,
    pub direction_degrees: f32,
    pub spread_degrees: f32,
    pub gravity: [f32; 2],
    pub drag: f32,
    pub turbulence: f32,
    pub angular_velocity: ScalarRange,
    pub size: Curve,
    pub opacity: Curve,
    pub color: Gradient,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EmitterShape {
    Point,
    Circle { radius: f32 },
    Ring { radius: f32 },
    Cone { radius: f32, depth: f32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Renderer {
    Billboard { softness: f32 },
    Ribbon { width: f32 },
    Mesh { asset: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventLink {
    pub source_layer: String,
    pub trigger: EventTrigger,
    pub target_layer: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventTrigger {
    OnSpawn,
    OnDeath,
    OnCollision,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ScalarRange {
    pub min: f32,
    pub max: f32,
}

impl ScalarRange {
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    pub fn sample(self, random: f32) -> f32 {
        self.min + (self.max - self.min) * random.clamp(0.0, 1.0)
    }

    fn validate(self, layer: &str, field: &'static str) -> Result<(), ValidationError> {
        if !self.min.is_finite() || !self.max.is_finite() || self.min > self.max {
            return Err(ValidationError::InvalidRange {
                layer: layer.to_owned(),
                field,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Curve {
    pub keys: Vec<CurveKey>,
}

impl Curve {
    pub fn sample(&self, time: f32) -> f32 {
        let Some(first) = self.keys.first() else {
            return 0.0;
        };
        let t = time.clamp(0.0, 1.0);
        if t <= first.time {
            return first.value;
        }
        for pair in self.keys.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if t <= b.time {
                let span = (b.time - a.time).max(f32::EPSILON);
                let x = ((t - a.time) / span).clamp(0.0, 1.0);
                let smooth = x * x * (3.0 - 2.0 * x);
                return a.value + (b.value - a.value) * smooth;
            }
        }
        self.keys.last().map_or(0.0, |key| key.value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct CurveKey {
    pub time: f32,
    pub value: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Gradient {
    pub keys: Vec<ColorKey>,
}

impl Gradient {
    pub fn sample(&self, time: f32) -> [f32; 4] {
        let Some(first) = self.keys.first() else {
            return [1.0; 4];
        };
        let t = time.clamp(0.0, 1.0);
        if t <= first.time {
            return first.color;
        }
        for pair in self.keys.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if t <= b.time {
                let x = ((t - a.time) / (b.time - a.time).max(f32::EPSILON)).clamp(0.0, 1.0);
                return std::array::from_fn(|i| a.color[i] + (b.color[i] - a.color[i]) * x);
            }
        }
        self.keys.last().map_or([1.0; 4], |key| key.color)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ColorKey {
    pub time: f32,
    pub color: [f32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleSample {
    pub layer_index: usize,
    pub position: [f32; 2],
    pub size: f32,
    pub rotation: f32,
    pub color: [f32; 4],
    pub normalized_age: f32,
}

/// Deterministically evaluates all live particles at a choreography time.
///
/// The evaluator is deliberately allocation-friendly and renderer-neutral. It provides a stable
/// CPU reference implementation; a future GPU runtime can conform to the same behavior.
pub fn evaluate(asset: &EffectAsset, time: f32, output: &mut Vec<ParticleSample>) {
    output.clear();
    let effect_time = if asset.looping {
        time.rem_euclid(asset.duration)
    } else {
        time.clamp(0.0, asset.duration)
    };

    for (layer_index, layer) in asset.layers.iter().enumerate() {
        if !layer.enabled {
            continue;
        }
        let local_time = effect_time - layer.start_time;
        if local_time < 0.0 || local_time > layer.duration {
            continue;
        }

        let emission_count = layer
            .emitter
            .burst_count
            .saturating_add((local_time * layer.emitter.spawn_rate).floor().max(0.0) as u32);
        let count = emission_count.min(layer.emitter.max_particles);
        for index in 0..count {
            let spawn_time = if index < layer.emitter.burst_count {
                0.0
            } else if layer.emitter.spawn_rate > 0.0 {
                (index - layer.emitter.burst_count) as f32 / layer.emitter.spawn_rate
            } else {
                continue;
            };
            let age = local_time - spawn_time;
            let life = layer.emitter.lifetime.sample(hash01(index, 0));
            if age < 0.0 || age >= life || life <= 0.0 {
                continue;
            }

            let normalized_age = age / life;
            let direction = (layer.emitter.direction_degrees
                + (hash01(index, 1) - 0.5) * layer.emitter.spread_degrees)
                .to_radians();
            let speed = layer.emitter.speed.sample(hash01(index, 2));
            let (origin_x, origin_y) = sample_shape(&layer.emitter.shape, index);
            let damping = (-layer.emitter.drag.max(0.0) * age).exp();
            let travel = if layer.emitter.drag.abs() < 0.0001 {
                speed * age
            } else {
                speed * (1.0 - damping) / layer.emitter.drag.max(0.0001)
            };
            let turbulence = layer.emitter.turbulence
                * (age * 7.0 + hash01(index, 3) * std::f32::consts::TAU).sin();
            let position = [
                origin_x + direction.cos() * travel + turbulence,
                origin_y
                    + direction.sin() * travel
                    + layer.emitter.gravity[1] * age * age * 0.5
                    + layer.emitter.gravity[0] * age * 0.1,
            ];
            let mut color = layer.emitter.color.sample(normalized_age);
            color[3] *= layer.emitter.opacity.sample(normalized_age);
            output.push(ParticleSample {
                layer_index,
                position,
                size: layer.emitter.size.sample(normalized_age),
                rotation: layer.emitter.angular_velocity.sample(hash01(index, 4)) * age,
                color,
                normalized_age,
            });
        }
    }
}

/// Installs Aestra's CPU reference playback runtime into a Bevy application.
///
/// Add this plugin once, then spawn an [`EffectPlayer`]. The plugin creates and reuses a bounded
/// sprite pool beneath that entity. The API is intentionally small so a GPU-backed implementation
/// can preserve the same integration surface later.
#[derive(Default)]
pub struct AestraPlugin;

impl Plugin for AestraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (prepare_effect_players, play_effects).chain());
    }
}

/// A playing instance of an Aestra effect.
#[derive(Component)]
#[require(Transform, Visibility)]
pub struct EffectPlayer {
    pub effect: Arc<EffectAsset>,
    pub elapsed: f32,
    pub speed: f32,
    pub playing: bool,
    samples: Vec<ParticleSample>,
}

impl EffectPlayer {
    pub fn new(effect: EffectAsset) -> Self {
        Self {
            effect: Arc::new(effect),
            elapsed: 0.0,
            speed: 1.0,
            playing: true,
            samples: Vec::new(),
        }
    }

    pub fn from_shared(effect: Arc<EffectAsset>) -> Self {
        Self {
            effect,
            elapsed: 0.0,
            speed: 1.0,
            playing: true,
            samples: Vec::new(),
        }
    }

    pub fn restart(&mut self) {
        self.elapsed = 0.0;
        self.playing = true;
    }

    pub fn seek(&mut self, time: f32) {
        self.elapsed = time.clamp(0.0, self.effect.duration);
    }
}

#[derive(Component)]
struct RuntimeParticle(usize);

fn prepare_effect_players(
    mut commands: Commands,
    players: Query<(Entity, &EffectPlayer), Added<EffectPlayer>>,
) {
    for (entity, player) in &players {
        let capacity = player
            .effect
            .layers
            .iter()
            .map(|layer| layer.emitter.max_particles as usize)
            .sum::<usize>()
            .min(4096);
        commands.entity(entity).with_children(|parent| {
            for slot in 0..capacity {
                parent.spawn((
                    RuntimeParticle(slot),
                    Sprite::from_color(Color::WHITE, Vec2::ONE),
                    Transform::default(),
                    Visibility::Hidden,
                ));
            }
        });
    }
}

fn play_effects(
    time: Res<Time>,
    mut players: Query<(&mut EffectPlayer, &Children)>,
    mut particles: Query<(
        &RuntimeParticle,
        &mut Sprite,
        &mut Transform,
        &mut Visibility,
    )>,
) {
    for (mut player, children) in &mut players {
        if player.playing {
            player.elapsed += time.delta_secs() * player.speed;
            if player.elapsed > player.effect.duration {
                if player.effect.looping {
                    player.elapsed = player.elapsed.rem_euclid(player.effect.duration);
                } else {
                    player.elapsed = player.effect.duration;
                    player.playing = false;
                }
            }
        }

        let elapsed = player.elapsed;
        let mut samples = std::mem::take(&mut player.samples);
        evaluate(&player.effect, elapsed, &mut samples);

        for child in children.iter() {
            let Ok((slot, mut sprite, mut transform, mut visibility)) = particles.get_mut(child)
            else {
                continue;
            };
            let Some(sample) = samples.get(slot.0) else {
                *visibility = Visibility::Hidden;
                continue;
            };
            let size = sample.size.max(0.01);
            sprite.color = Color::srgba(
                sample.color[0],
                sample.color[1],
                sample.color[2],
                sample.color[3],
            );
            sprite.custom_size = Some(Vec2::splat(size));
            transform.translation = Vec3::new(sample.position[0], sample.position[1], 0.0);
            transform.rotation = Quat::from_rotation_z(sample.rotation);
            *visibility = Visibility::Visible;
        }
        player.samples = samples;
    }
}

fn sample_shape(shape: &EmitterShape, index: u32) -> (f32, f32) {
    let angle = hash01(index, 5) * std::f32::consts::TAU;
    match shape {
        EmitterShape::Point => (0.0, 0.0),
        EmitterShape::Circle { radius } => {
            let r = radius * hash01(index, 6).sqrt();
            (angle.cos() * r, angle.sin() * r)
        }
        EmitterShape::Ring { radius } => (angle.cos() * radius, angle.sin() * radius),
        EmitterShape::Cone { radius, depth } => {
            let y = hash01(index, 6) * depth;
            let x = (hash01(index, 7) * 2.0 - 1.0) * radius * (y / depth.max(0.001));
            (x, y)
        }
    }
}

fn hash01(index: u32, channel: u32) -> f32 {
    let mut x = index.wrapping_mul(0x9E37_79B9) ^ channel.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x as f64 / u32::MAX as f64) as f32
}

#[derive(Debug, Error)]
pub enum AssetError {
    #[error("could not read or write the effect asset: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not parse the effect asset: {0}")]
    Parse(#[from] ron::error::SpannedError),
    #[error("could not serialize the effect asset: {0}")]
    Serialize(#[from] ron::Error),
    #[error(transparent)]
    Validation(#[from] ValidationError),
}

#[derive(Debug, Error, PartialEq)]
pub enum ValidationError {
    #[error("effect format version {0} is not supported")]
    UnsupportedVersion(u32),
    #[error("effect id cannot be empty")]
    EmptyId,
    #[error("effect duration must be a positive finite number, got {0}")]
    InvalidDuration(f32),
    #[error("layer id cannot be empty")]
    EmptyLayerId,
    #[error("layer '{0}' has invalid timing")]
    InvalidLayerTiming(String),
    #[error("layer '{0}' has invalid emitter settings")]
    InvalidEmitter(String),
    #[error("layer '{layer}' has an invalid {field} range")]
    InvalidRange { layer: String, field: &'static str },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_asset() -> EffectAsset {
        EffectAsset {
            format_version: CURRENT_FORMAT_VERSION,
            id: "test".into(),
            name: "Test".into(),
            duration: 2.0,
            looping: true,
            events: vec![],
            layers: vec![EffectLayer {
                id: "sparks".into(),
                name: "Sparks".into(),
                enabled: true,
                start_time: 0.0,
                duration: 2.0,
                blend: BlendMode::Additive,
                renderer: Renderer::Billboard { softness: 0.5 },
                emitter: Emitter {
                    shape: EmitterShape::Point,
                    spawn_rate: 10.0,
                    burst_count: 2,
                    max_particles: 64,
                    lifetime: ScalarRange::new(1.0, 1.0),
                    speed: ScalarRange::new(10.0, 10.0),
                    direction_degrees: 90.0,
                    spread_degrees: 0.0,
                    gravity: [0.0, 0.0],
                    drag: 0.0,
                    turbulence: 0.0,
                    angular_velocity: ScalarRange::new(0.0, 0.0),
                    size: Curve {
                        keys: vec![CurveKey {
                            time: 0.0,
                            value: 4.0,
                        }],
                    },
                    opacity: Curve {
                        keys: vec![CurveKey {
                            time: 0.0,
                            value: 1.0,
                        }],
                    },
                    color: Gradient {
                        keys: vec![ColorKey {
                            time: 0.0,
                            color: [1.0; 4],
                        }],
                    },
                },
            }],
        }
    }

    #[test]
    fn evaluation_is_deterministic() {
        let asset = test_asset();
        let mut first = vec![];
        let mut second = vec![];
        evaluate(&asset, 0.75, &mut first);
        evaluate(&asset, 0.75, &mut second);
        assert_eq!(first, second);
        assert!(!first.is_empty());
    }

    #[test]
    fn ron_round_trip_preserves_asset() {
        let asset = test_asset();
        let source = asset.to_pretty_ron().unwrap();
        assert_eq!(EffectAsset::from_ron(&source).unwrap(), asset);
    }

    #[test]
    fn invalid_duration_is_rejected() {
        let mut asset = test_asset();
        asset.duration = 0.0;
        assert_eq!(asset.validate(), Err(ValidationError::InvalidDuration(0.0)));
    }
}
