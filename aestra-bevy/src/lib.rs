//! Bevy integration for compiled Aestra effects.

pub use aestra_bevy_render::{
    ActiveBackend, AestraRenderPlugin, AestraRenderSet, AestraRenderSettings as AestraSettings,
    AestraRuntimeStatus, BackendCapabilities, CompatibilityIssue, CompatibilityIssueCode,
    CompatibilityReport, CompatibilityTarget, DEFAULT_GPU_PARTICLE_BUDGET, EffectRenderMode,
    EffectRequirements, EffectRuntimeStatus, GpuCapabilities, PresentationMode, PresentedEffect,
    RendererCapability, gpu,
};
pub use aestra_compiler::{CompileError, EffectCompiler, ModuleRegistry};
pub use aestra_core::*;
pub use aestra_runtime::{
    CheckpointBackendId, CheckpointContext, CheckpointPolicy, CheckpointStore, ClockAdvance,
    CompiledEffect, DEFAULT_PLAYBACK_TICK_RATE, DispatchedChoreographyEvent, EffectInstance,
    EffectProfile, EmitterProfile, ParameterError, ParticleSample, PlaybackCheckpoint,
    PlaybackClock, ProfileValue, ProfileValueSource, RuntimeValue, SeekOrigin, SeekPlan,
    SimulationSeekMode,
};

use bevy::asset::LoadState;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{
    App, AssetServer, Assets, Commands, Component, Entity, Event, Image, Plugin, Query, Res,
    Resource, Time, Transform, Update, Visibility, Without,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Installs Aestra's compiled effect playback into a Bevy application.
#[derive(Default)]
pub struct AestraPlugin;

/// Public scheduling points for applications that need to coordinate editor or game state with
/// Aestra playback.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AestraSet {
    /// Advances effect clocks and updates their presentation for the current frame.
    Playback,
}

/// Fired after an [`EffectPlayer`] crosses a compiled choreography event during normal playback.
/// Applications can consume it with a Bevy observer without coupling game logic to particle
/// lifecycle links or polling the player timeline.
#[derive(Event, Debug, Clone)]
pub struct AestraChoreographyEvent {
    pub player: Entity,
    pub event: DispatchedChoreographyEvent,
}

impl Plugin for AestraPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(AestraRenderPlugin)
            .init_resource::<TextureAssetCache>()
            .configure_sets(Update, AestraRenderSet::Prepare.after(AestraSet::Playback));
        app.add_systems(
            Update,
            (
                prepare_player_presentations,
                prepare_effect_profiles,
                update_asset_diagnostics,
                play_effects,
                sync_player_presentations,
            )
                .chain()
                .in_set(AestraSet::Playback),
        );
    }
}

fn prepare_player_presentations(
    mut commands: Commands,
    players: Query<(Entity, &EffectPlayer), Without<PresentedEffect>>,
) {
    for (entity, player) in &players {
        let mut presented = PresentedEffect::new(player.effect().clone());
        presented.instance = player.instance.clone();
        presented.set_render_mode(player.render_mode());
        commands.entity(entity).insert(presented);
    }
}

fn sync_player_presentations(mut players: Query<(&EffectPlayer, &mut PresentedEffect)>) {
    for (player, mut presented) in &mut players {
        presented.instance = player.instance.clone();
        presented.set_render_mode(player.render_mode());
    }
}

#[derive(Resource, Default)]
pub(crate) struct TextureAssetCache(BTreeMap<String, bevy::prelude::Handle<Image>>);

impl TextureAssetCache {
    pub(crate) fn load(
        &mut self,
        asset_server: &AssetServer,
        path: &str,
    ) -> bevy::prelude::Handle<Image> {
        self.0
            .entry(path.to_owned())
            .or_insert_with(|| asset_server.load(path.to_owned()))
            .clone()
    }
}

/// A Bevy component that owns mutable state for one compiled effect instance.
#[derive(Component)]
#[require(Transform, Visibility)]
pub struct EffectPlayer {
    pub instance: EffectInstance,
    pub speed: f32,
    pub playing: bool,
    render_mode: EffectRenderMode,
    clock: PlaybackClock,
    choreography_events: Vec<DispatchedChoreographyEvent>,
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
            render_mode: EffectRenderMode::Rendered,
            clock: PlaybackClock::default(),
            choreography_events: Vec::new(),
        }
    }

    pub fn effect(&self) -> &Arc<CompiledEffect> {
        self.instance.effect()
    }

    pub fn render_mode(&self) -> EffectRenderMode {
        self.render_mode
    }

    pub fn set_render_mode(&mut self, mode: EffectRenderMode) {
        self.render_mode = mode;
    }

    pub fn elapsed(&self) -> f32 {
        self.clock.time(self.effect().duration)
    }

    /// Simulation time, which remains unwrapped for seamless continuous looping.
    pub fn simulation_time(&self) -> f32 {
        if self.effect().playback_mode.is_continuous() {
            self.clock.elapsed_time()
        } else {
            self.elapsed()
        }
    }

    pub fn frame(&self) -> u64 {
        self.clock.frame()
    }

    pub fn tick_rate(&self) -> u32 {
        self.clock.tick_rate()
    }

    pub fn seek_mode(&self) -> SimulationSeekMode {
        self.effect().seek_mode
    }

    pub fn restart(&mut self) {
        self.clock.restart();
        self.instance.restart();
        self.playing = true;
    }

    pub fn seek(&mut self, time: f32) {
        let duration = self.effect().duration;
        let mut target = self.clock;
        target.seek_seconds(time, duration);
        self.seek_frame(target.frame());
    }

    /// Seeks continuous playback using absolute simulation time while leaving the playhead
    /// wrapped to the authored effect duration.
    pub fn seek_simulation_time(&mut self, time: f32) {
        let duration = self.effect().duration;
        if self.effect().playback_mode.is_continuous() {
            self.clock.seek_elapsed_seconds(time, duration);
            self.sync_instance_time();
        } else {
            self.seek(time);
        }
    }

    pub fn seek_frame(&mut self, frame: u64) {
        let duration = self.effect().duration;
        let target = frame.min(self.clock.maximum_frame(duration));
        if self.seek_mode() == SimulationSeekMode::StatelessDirect {
            self.clock.seek_frame(target, duration);
            self.sync_instance_time();
            return;
        }
        if target < self.frame() {
            self.clock.restart();
            self.instance.restart();
        }
        let tick_seconds = 1.0 / self.clock.tick_rate() as f32;
        while self.frame() < target {
            self.clock.step_forward(duration);
            self.instance.advance(tick_seconds);
        }
    }

    pub fn step_forward(&mut self) {
        self.seek_frame(self.frame().saturating_add(1));
        self.playing = false;
    }

    pub fn step_back(&mut self) {
        self.seek_frame(self.frame().saturating_sub(1));
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
        let mut target = self.clock;
        target.restore(checkpoint, duration);
        self.seek_frame(target.frame());
        self.playing = false;
    }

    pub fn set_parameter(&mut self, id: ParameterId, value: Value) -> Result<(), ParameterError> {
        self.instance.set_parameter(id, value)
    }

    pub fn clear_parameter(&mut self, id: ParameterId) -> Result<(), ParameterError> {
        self.instance.clear_parameter(id)
    }

    /// Drains choreography events produced by the most recent clock advance. The plugin drains
    /// this automatically and emits [`AestraChoreographyEvent`]; manual player integrations can
    /// use the same queue directly.
    pub fn drain_choreography_events(
        &mut self,
    ) -> impl Iterator<Item = DispatchedChoreographyEvent> + '_ {
        self.choreography_events.drain(..)
    }

    fn advance_clock(&mut self, delta_seconds: f32) -> ClockAdvance {
        let duration = self.effect().duration;
        let playback_mode = self.effect().playback_mode;
        let looping = playback_mode.is_looping();
        let previous_frame = self.clock.frame();
        let result = self
            .clock
            .advance(delta_seconds, self.speed, duration, looping);
        self.choreography_events.clear();
        let tick_seconds = 1.0 / self.clock.tick_rate() as f32;
        match self.seek_mode() {
            SimulationSeekMode::StatelessDirect => {
                self.instance.advance_with_choreography_events(
                    result.ticks as f32 * tick_seconds,
                    &mut self.choreography_events,
                );
                self.sync_instance_time();
            }
            SimulationSeekMode::CheckpointRestore | SimulationSeekMode::RestartReplay => {
                let ticks = if looping {
                    result.ticks
                } else {
                    self.clock.frame().saturating_sub(previous_frame)
                };
                let mut events = Vec::new();
                for _ in 0..ticks {
                    self.instance
                        .advance_with_choreography_events(tick_seconds, &mut events);
                    self.choreography_events.append(&mut events);
                }
            }
        }
        result
    }

    fn sync_instance_time(&mut self) {
        let time = if self.instance.effect().playback_mode.is_continuous() {
            self.clock.elapsed_time()
        } else {
            self.clock.time(self.instance.effect().duration)
        };
        self.instance.seek(time);
    }
}

/// Live machine-readable profile for an [`EffectPlayer`].
#[derive(Component, Debug, Clone)]
pub struct EffectProfiler(pub EffectProfile);

fn prepare_effect_profiles(
    mut commands: Commands,
    capabilities: Res<GpuCapabilities>,
    players: Query<(Entity, &EffectPlayer, &EffectRuntimeStatus), Without<EffectProfiler>>,
) {
    for (entity, player, runtime) in &players {
        commands.entity(entity).insert(EffectProfiler(bevy_profile(
            player.effect(),
            &capabilities,
            runtime,
        )));
    }
}

fn update_asset_diagnostics(
    asset_server: Res<AssetServer>,
    images: Res<Assets<Image>>,
    mut texture_cache: bevy::prelude::ResMut<TextureAssetCache>,
    mut players: Query<(&EffectPlayer, &mut EffectProfiler)>,
) {
    const PREFIX: &str = "texture asset '";
    for (player, mut profiler) in &mut players {
        profiler
            .0
            .platform_warnings
            .retain(|warning| !warning.starts_with(PREFIX));
        let referenced = player
            .effect()
            .emitters
            .iter()
            .flat_map(|emitter| emitter.renderers.iter())
            .filter_map(|renderer| player.effect().material(renderer.material))
            .filter_map(|material| material.texture)
            .chain(
                player
                    .effect()
                    .flipbooks
                    .iter()
                    .map(|flipbook| flipbook.texture),
            )
            .collect::<BTreeSet<_>>();
        let mut texture_bytes = 0_u64;
        let mut all_loaded = true;
        for asset in &player.effect().assets {
            if asset.kind != AssetKind::Texture {
                continue;
            }
            if !referenced.contains(&asset.source) {
                continue;
            }
            let handle = texture_cache.load(&asset_server, &asset.path);
            match asset_server.get_load_state(handle.id()) {
                Some(LoadState::Failed(error)) => {
                    all_loaded = false;
                    profiler.0.platform_warnings.push(format!(
                        "texture asset '{}' ({}) failed to load: {error}; using the missing-texture fallback",
                        asset.name, asset.path
                    ));
                }
                Some(LoadState::Loaded) => {
                    let Some(image) = images.get(&handle) else {
                        all_loaded = false;
                        continue;
                    };
                    texture_bytes = texture_bytes
                        .saturating_add(image.data.as_ref().map_or(0, |data| data.len()) as u64);
                }
                Some(LoadState::NotLoaded | LoadState::Loading) | None => {
                    all_loaded = false;
                }
            }
        }
        profiler.0.texture_memory_bytes = if referenced.is_empty() {
            ProfileValue::Estimated(0)
        } else if all_loaded {
            ProfileValue::Measured(texture_bytes)
        } else {
            ProfileValue::Unavailable
        };
    }
}

fn bevy_profile(
    effect: &CompiledEffect,
    capabilities: &GpuCapabilities,
    runtime: &EffectRuntimeStatus,
) -> EffectProfile {
    let mut profile = EffectProfile::from_compiled(effect);
    profile.platform_warnings = capabilities.limitations.clone();
    if runtime.active == ActiveBackend::CpuReference
        && runtime.reason.contains("fallback")
        && !profile.platform_warnings.contains(&runtime.reason)
    {
        profile.platform_warnings.push(runtime.reason.clone());
    }
    profile
}

fn play_effects(
    mut commands: Commands,
    time: Res<Time>,
    capabilities: Res<GpuCapabilities>,
    mut players: Query<(
        Entity,
        &mut EffectPlayer,
        &PresentedEffect,
        &mut EffectProfiler,
        &EffectRuntimeStatus,
    )>,
) {
    for (player_entity, mut player, presented, mut profiler, runtime) in &mut players {
        if !profiler.0.matches_compiled(player.effect()) {
            profiler.0 = bevy_profile(player.effect(), &capabilities, runtime);
        }
        if player.playing {
            let advance = player.advance_clock(time.delta_secs());
            if advance.reached_end {
                player.playing = false;
            }
        }
        for event in player.drain_choreography_events() {
            commands.trigger(AestraChoreographyEvent {
                player: player_entity,
                event,
            });
        }
        if let Some(elapsed) = presented.cpu_evaluation_time() {
            profiler.0.record_cpu_frame(elapsed, presented.samples());
        } else {
            profiler.0.record_particle_frame(presented.samples());
        }
        profiler
            .0
            .record_submitted_frame(player.effect(), presented.samples());
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
    fn player_accepts_a_reloaded_compiled_artifact() {
        let mut effect = EffectAsset::new("Artifact playback", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Sparks", 2.0));
        let compiled = EffectCompiler::default().compile(&effect).unwrap();
        let bytes = aestra_artifact::encode_effect(&compiled).unwrap();
        let reloaded = aestra_artifact::decode_effect(&bytes).unwrap();
        let mut player = EffectPlayer::from_compiled(Arc::new(reloaded));

        player.seek(0.75);
        let mut samples = Vec::new();
        player.instance.evaluate(&mut samples);
        assert_eq!(player.effect().source, effect.id);
        assert!(!samples.is_empty());
    }

    #[test]
    fn player_render_mode_can_switch_to_wireframe() {
        let mut effect = EffectAsset::new("Wireframe", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
        let mut player = EffectPlayer::new(&effect);

        assert_eq!(player.render_mode(), EffectRenderMode::Rendered);
        player.set_render_mode(EffectRenderMode::Wireframe);
        assert_eq!(player.render_mode(), EffectRenderMode::Wireframe);
    }

    #[test]
    fn automatic_presentation_is_the_default() {
        assert_eq!(
            AestraSettings::default().presentation,
            PresentationMode::Auto
        );
    }

    #[test]
    fn players_receive_a_machine_readable_profile() {
        let mut effect = EffectAsset::new("Profiled", 2.0);
        let mut emitter = Emitter::basic_sprite("Emitter", 2.0);
        let material = MaterialDefinition::sprite("Alpha", BlendMode::Alpha, 0.2);
        let material_id = material.id;
        effect.materials.push(material);
        emitter
            .renderers
            .push(RendererInstance::sprite(material_id));
        effect.emitters.push(emitter);
        let mut app = App::new();
        app.insert_resource(GpuCapabilities::default())
            .add_systems(Update, prepare_effect_profiles);
        let entity = app
            .world_mut()
            .spawn((
                EffectPlayer::new(&effect),
                EffectRuntimeStatus {
                    active: ActiveBackend::CpuReference,
                    reason: "CPU reference requested".into(),
                    compatibility: CompatibilityReport::compatible(
                        CompatibilityTarget::CpuReference,
                    ),
                },
            ))
            .id();

        app.update();

        let profile = &app.world().get::<EffectProfiler>(entity).unwrap().0;
        assert_eq!(profile.emitter_count, ProfileValue::Measured(1));
        assert_eq!(profile.gpu_time_ns, ProfileValue::Unavailable);
        assert_eq!(profile.submitted_instances, ProfileValue::Unavailable);
        assert!(matches!(
            profile.buffer_memory_bytes,
            ProfileValue::Estimated(bytes) if bytes > 0
        ));
        assert_eq!(profile.draw_calls, ProfileValue::Estimated(2));
        assert!(profile.platform_warnings.is_empty());
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
    fn players_surface_choreography_events_crossed_by_fixed_step_playback() {
        let mut effect = EffectAsset::new("Choreography", 2.0);
        effect.choreography_events = vec![
            ChoreographyEvent::new(
                "Begin",
                0.0,
                ChoreographyEventPayload::GameplayNotify {
                    topic: "begin".into(),
                },
            ),
            ChoreographyEvent::new(
                "Impact",
                0.5,
                ChoreographyEventPayload::PlaySound {
                    cue: "impact".into(),
                },
            ),
        ];
        let mut player = EffectPlayer::new(&effect);

        player.advance_clock(0.75);
        assert_eq!(
            player
                .drain_choreography_events()
                .map(|event| event.name)
                .collect::<Vec<_>>(),
            ["Begin", "Impact"]
        );
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

    #[test]
    fn snapshotless_stateful_player_restarts_and_replays_backward_seeks() {
        let mut effect = EffectAsset::new("Stateful Fallback", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
        let mut compiled = EffectCompiler::default().compile(&effect).unwrap();
        compiled.seek_mode = SimulationSeekMode::RestartReplay;
        let mut player = EffectPlayer::from_compiled(Arc::new(compiled));

        player.seek_frame(60);
        assert_eq!(player.elapsed(), 1.0);
        player.seek_frame(30);
        assert_eq!(player.frame(), 30);
        assert_eq!(player.elapsed(), 0.5);
    }
}
