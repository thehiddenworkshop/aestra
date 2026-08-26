//! Bevy integration and deterministic reference playback for Aestra effects.

pub use aestra_core::*;

use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{
    Added, App, Children, Color, Commands, Component, Entity, Plugin, Quat, Query, Res, Sprite,
    Time, Transform, Update, Vec2, Vec3, Visibility,
};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleSample {
    pub emitter_index: usize,
    pub position: [f32; 2],
    pub size: f32,
    pub rotation: f32,
    pub color: [f32; 4],
    pub normalized_age: f32,
}

/// Deterministically evaluates all live particles at a choreography time.
///
/// This remains the semantic reference while the compiler and GPU runtime are
/// introduced. It consumes the v2 module stack rather than a renderer-specific
/// particle structure.
pub fn evaluate(asset: &EffectAsset, time: f32, output: &mut Vec<ParticleSample>) {
    output.clear();
    let effect_time = if asset.looping {
        time.rem_euclid(asset.duration)
    } else {
        time.clamp(0.0, asset.duration)
    };

    for (emitter_index, emitter) in asset.emitters.iter().enumerate() {
        if !emitter.enabled {
            continue;
        }
        let local_time = effect_time - emitter.start_time;
        if local_time < 0.0 || local_time > emitter.duration {
            continue;
        }

        let spawn_rate = emitter.spawn_rate();
        let burst_count = emitter.burst_count();
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
            let life = emitter.lifetime().sample(hash01(index, 0));
            if age < 0.0 || age >= life || life <= 0.0 {
                continue;
            }

            let normalized_age = age / life;
            let direction = (emitter.direction_degrees()
                + (hash01(index, 1) - 0.5) * emitter.spread_degrees())
            .to_radians();
            let speed = emitter.speed().sample(hash01(index, 2));
            let (origin_x, origin_y) = sample_shape(emitter.shape(), index);
            let drag = emitter.drag();
            let damping = (-drag.max(0.0) * age).exp();
            let travel = if drag.abs() < 0.0001 {
                speed * age
            } else {
                speed * (1.0 - damping) / drag.max(0.0001)
            };
            let turbulence =
                emitter.turbulence() * (age * 7.0 + hash01(index, 3) * std::f32::consts::TAU).sin();
            let gravity = emitter.gravity();
            let position = [
                origin_x + direction.cos() * travel + turbulence,
                origin_y
                    + direction.sin() * travel
                    + gravity[1] * age * age * 0.5
                    + gravity[0] * age * 0.1,
            ];
            let mut color = emitter.color_gradient().sample(normalized_age);
            color[3] *= emitter.opacity_curve().sample(normalized_age);
            output.push(ParticleSample {
                emitter_index,
                position,
                size: emitter.size_curve().sample(normalized_age),
                rotation: emitter.angular_velocity().sample(hash01(index, 4)) * age,
                color,
                normalized_age,
            });
        }
    }
}

/// Installs Aestra's CPU reference playback runtime into a Bevy application.
#[derive(Default)]
pub struct AestraPlugin;

impl Plugin for AestraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (prepare_effect_players, play_effects).chain());
    }
}

/// A playing instance of an authored effect.
///
/// This component is the current integration surface. It will later contain a
/// compiled-effect handle rather than authored source data.
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
            .emitters
            .iter()
            .map(|emitter| emitter.max_particles as usize)
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
            let Ok((slot, mut sprite, mut transform, mut visibility)) = particles.get_mut(*child)
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
            let radius = radius * hash01(index, 6).sqrt();
            (angle.cos() * radius, angle.sin() * radius)
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
    let mut value = index.wrapping_mul(0x9E37_79B9) ^ channel.wrapping_mul(0x85EB_CA6B);
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

    fn test_asset() -> EffectAsset {
        let mut asset = EffectAsset::new("Test", 2.0);
        let mut emitter = Emitter::basic_sprite("Sparks", 2.0);
        emitter.max_particles = 64;
        *emitter.spawn_rate_mut() = 10.0;
        *emitter.burst_count_mut() = 2;
        *emitter.lifetime_mut() = ScalarRange::new(1.0, 1.0);
        if let ModuleParameters::Initialize {
            speed,
            direction_degrees,
            spread_degrees,
            angular_velocity,
            ..
        } = &mut emitter
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == MODULE_INITIALIZE)
            .expect("basic emitter has initialize module")
            .parameters
        {
            *speed = ScalarRange::new(10.0, 10.0);
            *direction_degrees = 90.0;
            *spread_degrees = 0.0;
            *angular_velocity = ScalarRange::new(0.0, 0.0);
        }
        emitter.modules[1].parameters = ModuleParameters::Shape {
            shape: EmitterShape::Point,
        };
        if let ModuleParameters::Motion {
            gravity,
            drag,
            turbulence,
        } = &mut emitter.modules[3].parameters
        {
            *gravity = [0.0, 0.0];
            *drag = 0.0;
            *turbulence = 0.0;
        }
        *emitter.size_curve_mut() = Curve::new(vec![CurveKey::new(0.0, 4.0)]);
        *emitter.opacity_curve_mut() = Curve::new(vec![CurveKey::new(0.0, 1.0)]);
        *emitter.color_gradient_mut() = Gradient::new(vec![ColorKey::new(0.0, [1.0; 4])]);
        asset.emitters.push(emitter);
        asset
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
    fn v2_round_trip_preserves_asset() {
        let asset = test_asset();
        let source = asset.to_pretty_ron().unwrap();
        assert_eq!(EffectAsset::from_ron(&source).unwrap(), asset);
    }
}
