//! Bevy integration for compiled Aestra effects.

mod capabilities;
pub mod gpu;

pub use aestra_compiler::{CompileError, EffectCompiler, ModuleRegistry};
pub use aestra_core::*;
pub use aestra_runtime::{
    ClockAdvance, CompiledEffect, DEFAULT_PLAYBACK_TICK_RATE, EffectInstance, ParameterError,
    ParticleSample, PlaybackCheckpoint, PlaybackClock, RuntimeValue,
};
pub use capabilities::{
    ActiveBackend, AestraRuntimeStatus, DEFAULT_GPU_PARTICLE_BUDGET, EffectRuntimeStatus,
    GpuCapabilities,
};

use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{
    App, Children, Color, Commands, Component, Entity, Plugin, Quat, Query, Res, Resource, Sprite,
    Time, Transform, Update, Vec2, Vec3, Visibility, Without,
};
use std::sync::Arc;

/// Selects the presentation path used by [`AestraPlugin`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PresentationMode {
    /// Select the best supported backend and fall back without panicking.
    #[default]
    Auto,
    /// Simulate and render particles entirely on the GPU.
    Gpu,
    /// Use the deterministic CPU interpreter and pooled Bevy sprites.
    CpuReference,
    /// Simulate on the GPU, read particles back, and present them as Bevy sprites.
    GpuReadback,
}

#[derive(Resource, Debug, Clone, Copy)]
pub struct AestraSettings {
    pub presentation: PresentationMode,
    /// Application budget applied in addition to physical device limits.
    pub max_gpu_particles: u32,
}

impl Default for AestraSettings {
    fn default() -> Self {
        Self {
            presentation: PresentationMode::Auto,
            max_gpu_particles: DEFAULT_GPU_PARTICLE_BUDGET,
        }
    }
}

/// Installs Aestra's compiled effect playback into a Bevy application.
#[derive(Default)]
pub struct AestraPlugin;

impl Plugin for AestraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AestraSettings>()
            .init_resource::<GpuCapabilities>()
            .init_resource::<AestraRuntimeStatus>()
            .add_observer(gpu::receive_readback);
        gpu::install(app);
        app.add_systems(
            Update,
            (
                assign_effect_backends,
                prepare_effect_players,
                gpu::prepare_gpu_players,
                play_effects,
            )
                .chain(),
        );
    }
}

/// A Bevy component that owns mutable state for one compiled effect instance.
#[derive(Component)]
#[require(Transform, Visibility)]
pub struct EffectPlayer {
    pub instance: EffectInstance,
    pub speed: f32,
    pub playing: bool,
    clock: PlaybackClock,
    samples: Vec<ParticleSample>,
    gpu_samples: Vec<ParticleSample>,
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
            clock: PlaybackClock::default(),
            samples: Vec::new(),
            gpu_samples: Vec::new(),
        }
    }

    pub fn effect(&self) -> &Arc<CompiledEffect> {
        self.instance.effect()
    }

    pub fn elapsed(&self) -> f32 {
        self.instance.time()
    }

    pub fn frame(&self) -> u64 {
        self.clock.frame()
    }

    pub fn tick_rate(&self) -> u32 {
        self.clock.tick_rate()
    }

    pub fn restart(&mut self) {
        self.clock.restart();
        self.instance.restart();
        self.playing = true;
    }

    pub fn seek(&mut self, time: f32) {
        let duration = self.effect().duration;
        self.clock.seek_seconds(time, duration);
        self.sync_instance_time();
    }

    pub fn seek_frame(&mut self, frame: u64) {
        let duration = self.effect().duration;
        self.clock.seek_frame(frame, duration);
        self.sync_instance_time();
    }

    pub fn step_forward(&mut self) {
        let duration = self.effect().duration;
        self.clock.step_forward(duration);
        self.sync_instance_time();
        self.playing = false;
    }

    pub fn step_back(&mut self) {
        let duration = self.effect().duration;
        self.clock.step_back(duration);
        self.sync_instance_time();
        self.playing = false;
    }

    pub fn set_seed(&mut self, seed: u64) {
        self.instance.set_seed(seed);
    }

    pub fn checkpoint(&self) -> PlaybackCheckpoint {
        self.clock.checkpoint()
    }

    pub fn restore_checkpoint(&mut self, checkpoint: PlaybackCheckpoint) {
        let duration = self.effect().duration;
        self.clock.restore(checkpoint, duration);
        self.sync_instance_time();
        self.playing = false;
    }

    pub fn set_parameter(&mut self, id: ParameterId, value: Value) -> Result<(), ParameterError> {
        self.instance.set_parameter(id, value)
    }

    pub fn clear_parameter(&mut self, id: ParameterId) -> Result<(), ParameterError> {
        self.instance.clear_parameter(id)
    }

    fn advance_clock(&mut self, delta_seconds: f32) -> ClockAdvance {
        let duration = self.effect().duration;
        let looping = self.effect().looping;
        let result = self
            .clock
            .advance(delta_seconds, self.speed, duration, looping);
        self.sync_instance_time();
        result
    }

    fn sync_instance_time(&mut self) {
        let duration = self.instance.effect().duration;
        self.instance.seek(self.clock.time(duration));
    }
}

#[derive(Component)]
struct RuntimeParticle(usize);

#[derive(Component)]
struct CpuPresentationPrepared;

#[derive(Component)]
pub(crate) struct GpuPresentationPrepared;

fn assign_effect_backends(
    mut commands: Commands,
    settings: Res<AestraSettings>,
    runtime: Res<AestraRuntimeStatus>,
    capabilities: Res<GpuCapabilities>,
    players: Query<(Entity, &EffectPlayer), Without<EffectRuntimeStatus>>,
) {
    if runtime.active == ActiveBackend::Pending {
        return;
    }
    let particle_budget = capabilities.max_particles.min(settings.max_gpu_particles) as usize;
    for (entity, player) in &players {
        commands
            .entity(entity)
            .insert(capabilities::select_effect_backend(
                &runtime,
                player.effect().max_particles,
                particle_budget,
            ));
    }
}

fn prepare_effect_players(
    mut commands: Commands,
    players: Query<
        (Entity, &EffectPlayer, &EffectRuntimeStatus),
        (Without<CpuPresentationPrepared>,),
    >,
) {
    for (entity, player, runtime) in &players {
        if !matches!(
            runtime.active,
            ActiveBackend::CpuReference | ActiveBackend::GpuReadback
        ) {
            continue;
        }
        let capacity = player.effect().max_particles.min(4096);
        commands
            .entity(entity)
            .insert(CpuPresentationPrepared)
            .with_children(|parent| {
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
    mut players: Query<(&mut EffectPlayer, Option<&Children>, &EffectRuntimeStatus)>,
    mut particles: Query<(
        &RuntimeParticle,
        &mut Sprite,
        &mut Transform,
        &mut Visibility,
    )>,
) {
    for (mut player, children, runtime) in &mut players {
        if player.playing {
            let advance = player.advance_clock(time.delta_secs());
            if advance.reached_end {
                player.playing = false;
            }
        }

        if runtime.active == ActiveBackend::Gpu {
            continue;
        }

        let uses_gpu_readback =
            runtime.active == ActiveBackend::GpuReadback && !player.gpu_samples.is_empty();
        let samples = if uses_gpu_readback {
            std::mem::take(&mut player.gpu_samples)
        } else {
            let mut samples = std::mem::take(&mut player.samples);
            player.instance.evaluate(&mut samples);
            samples
        };

        let Some(children) = children else {
            continue;
        };
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
        if uses_gpu_readback {
            player.gpu_samples = samples;
        } else {
            player.samples = samples;
        }
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
    fn automatic_presentation_is_the_default() {
        assert_eq!(
            AestraSettings::default().presentation,
            PresentationMode::Auto
        );
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

    #[test]
    fn fixed_step_players_match_across_render_delta_sequences() {
        let mut effect = EffectAsset::new("Deterministic", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
        let mut fine = EffectPlayer::new(&effect);
        let mut coarse = EffectPlayer::new(&effect);
        fine.set_seed(42);
        coarse.set_seed(42);

        for _ in 0..120 {
            fine.advance_clock(1.0 / 120.0);
        }
        for _ in 0..10 {
            coarse.advance_clock(0.1);
        }

        assert_eq!(fine.frame(), 60);
        assert_eq!(fine.frame(), coarse.frame());
        assert_eq!(fine.elapsed(), coarse.elapsed());
        let mut fine_samples = Vec::new();
        let mut coarse_samples = Vec::new();
        fine.instance.evaluate(&mut fine_samples);
        coarse.instance.evaluate(&mut coarse_samples);
        assert_eq!(fine_samples, coarse_samples);
    }

    #[test]
    fn frame_controls_pause_and_reproduce_exact_time() {
        let mut effect = EffectAsset::new("Frame Controls", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
        let mut player = EffectPlayer::new(&effect);
        player.seek_frame(30);
        assert_eq!(player.elapsed(), 0.5);
        player.step_forward();
        assert_eq!(player.frame(), 31);
        assert!(!player.playing);
        let checkpoint = player.checkpoint();
        player.step_back();
        assert_eq!(player.frame(), 30);
        player.restore_checkpoint(checkpoint);
        assert_eq!(player.frame(), 31);
        player.restart();
        assert_eq!(player.frame(), 0);
        assert_eq!(player.elapsed(), 0.0);
    }
}
