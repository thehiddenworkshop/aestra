//! Bevy integration for compiled Aestra effects.

pub use aestra_compiler::{CompileError, EffectCompiler, ModuleRegistry};
pub use aestra_core::*;
pub use aestra_runtime::{
    CompiledEffect, EffectInstance, ParameterError, ParticleSample, RuntimeValue,
};

use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{
    Added, App, Children, Color, Commands, Component, Entity, Plugin, Quat, Query, Res, Sprite,
    Time, Transform, Update, Vec2, Vec3, Visibility,
};
use std::sync::Arc;

/// Installs Aestra's compiled CPU playback adapter into a Bevy application.
#[derive(Default)]
pub struct AestraPlugin;

impl Plugin for AestraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (prepare_effect_players, play_effects).chain());
    }
}

/// A Bevy component that owns mutable state for one compiled effect instance.
#[derive(Component)]
#[require(Transform, Visibility)]
pub struct EffectPlayer {
    pub instance: EffectInstance,
    pub speed: f32,
    pub playing: bool,
    samples: Vec<ParticleSample>,
}

impl EffectPlayer {
    pub fn try_new(effect: &EffectAsset) -> Result<Self, CompileError> {
        let compiled = EffectCompiler::default().compile(effect)?;
        Ok(Self::from_compiled(Arc::new(compiled)))
    }

    pub fn new(effect: &EffectAsset) -> Self {
        Self::try_new(effect).expect("effect must compile before playback")
    }

    pub fn from_compiled(effect: Arc<CompiledEffect>) -> Self {
        Self {
            instance: EffectInstance::new(effect),
            speed: 1.0,
            playing: true,
            samples: Vec::new(),
        }
    }

    pub fn effect(&self) -> &Arc<CompiledEffect> {
        self.instance.effect()
    }

    pub fn elapsed(&self) -> f32 {
        self.instance.time()
    }

    pub fn restart(&mut self) {
        self.instance.restart();
        self.playing = true;
    }

    pub fn seek(&mut self, time: f32) {
        self.instance.seek(time);
    }

    pub fn set_parameter(&mut self, id: ParameterId, value: Value) -> Result<(), ParameterError> {
        self.instance.set_parameter(id, value)
    }

    pub fn clear_parameter(&mut self, id: ParameterId) -> Result<(), ParameterError> {
        self.instance.clear_parameter(id)
    }
}

#[derive(Component)]
struct RuntimeParticle(usize);

fn prepare_effect_players(
    mut commands: Commands,
    players: Query<(Entity, &EffectPlayer), Added<EffectPlayer>>,
) {
    for (entity, player) in &players {
        let capacity = player.effect().max_particles.min(4096);
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
            let delta = time.delta_secs() * player.speed;
            player.instance.advance(delta);
            if !player.effect().looping && player.elapsed() >= player.effect().duration {
                player.playing = false;
            }
        }

        let mut samples = std::mem::take(&mut player.samples);
        player.instance.evaluate(&mut samples);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_compiles_authored_effect() {
        let mut effect = EffectAsset::new("Test", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Sparks", 2.0));
        let player = EffectPlayer::try_new(&effect).unwrap();
        assert_eq!(player.effect().source, effect.id);
        assert_eq!(player.effect().emitters.len(), 1);
    }

    #[test]
    fn player_forwards_runtime_parameter_overrides() {
        let mut effect = EffectAsset::new("Parameterized", 2.0);
        let parameter = EffectParameter {
            id: ParameterId::new(),
            name: "Spawn Rate".into(),
            default: Value::Scalar(4.0),
            exposed: true,
        };
        let parameter_id = parameter.id;
        let mut emitter = Emitter::basic_sprite("Emitter", 2.0);
        emitter
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == MODULE_EMISSION)
            .unwrap()
            .bindings
            .insert("spawn_rate".into(), parameter_id);
        effect.parameters.push(parameter);
        effect.emitters.push(emitter);

        let mut player = EffectPlayer::try_new(&effect).unwrap();
        player
            .set_parameter(parameter_id, Value::Scalar(40.0))
            .unwrap();
        assert!(matches!(
            player.instance.parameter(parameter_id),
            Some(RuntimeValue::Scalar(40.0))
        ));
    }
}
