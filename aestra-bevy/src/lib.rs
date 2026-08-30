//! Bevy integration for compiled Aestra effects.

mod capabilities;
pub mod gpu;

pub use aestra_compiler::{CompileError, EffectCompiler, ModuleRegistry};
pub use aestra_core::*;
pub use aestra_runtime::{
    CheckpointBackendId, CheckpointContext, CheckpointPolicy, CheckpointStore, ClockAdvance,
    CompiledEffect, DEFAULT_PLAYBACK_TICK_RATE, DispatchedChoreographyEvent, EffectInstance,
    EffectProfile, EmitterProfile, ParameterError, ParticleSample, PlaybackCheckpoint,
    PlaybackClock, ProfileValue, ProfileValueSource, RuntimeValue, SeekOrigin, SeekPlan,
    SimulationSeekMode,
};
pub use capabilities::{
    ActiveBackend, AestraRuntimeStatus, DEFAULT_GPU_PARTICLE_BUDGET, EffectRuntimeStatus,
    GpuCapabilities,
};

use bevy::asset::LoadState;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{
    App, AssetServer, Assets, Children, Color, Commands, Component, Entity, Event, Image, Plugin,
    Quat, Query, Res, Resource, Sprite, Time, Transform, Update, Vec2, Vec3, Visibility, Without,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Instant,
};

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

/// Selects how an effect's presentation geometry is shaded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EffectRenderMode {
    /// Use authored materials, textures, and blending.
    #[default]
    Rendered,
    /// Draw the particle sprite quads as unfilled outlines for editor inspection.
    Wireframe,
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
        app.init_resource::<AestraSettings>()
            .init_resource::<GpuCapabilities>()
            .init_resource::<AestraRuntimeStatus>()
            .init_resource::<TextureAssetCache>()
            .add_observer(gpu::receive_readback);
        gpu::install(app);
        app.add_systems(
            Update,
            (
                assign_effect_backends,
                prepare_effect_profiles,
                prepare_effect_players,
                gpu::prepare_gpu_players,
                update_asset_diagnostics,
                play_effects,
            )
                .chain()
                .in_set(AestraSet::Playback),
        );
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
    samples: Vec<ParticleSample>,
    gpu_samples: Vec<ParticleSample>,
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
            samples: Vec::new(),
            gpu_samples: Vec::new(),
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
        let looping = self.effect().looping;
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
        let duration = self.instance.effect().duration;
        self.instance.seek(self.clock.time(duration));
    }
}

#[derive(Component)]
struct RuntimeParticle {
    sample_index: usize,
    renderer_index: usize,
}

#[derive(Component)]
struct CpuPresentationPrepared;

#[derive(Component)]
pub(crate) struct GpuPresentationPrepared;

/// Live machine-readable profile for an [`EffectPlayer`].
#[derive(Component, Debug, Clone)]
pub struct EffectProfiler(pub EffectProfile);

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
        let renderer_capacity = cpu_renderer_capacity(player.effect());
        commands
            .entity(entity)
            .insert(CpuPresentationPrepared)
            .with_children(|parent| {
                for sample_index in 0..capacity {
                    for renderer_index in 0..renderer_capacity {
                        parent.spawn((
                            RuntimeParticle {
                                sample_index,
                                renderer_index,
                            },
                            Sprite::from_color(Color::WHITE, Vec2::ONE),
                            Transform::default(),
                            Visibility::Hidden,
                        ));
                    }
                }
            });
    }
}

fn cpu_renderer_capacity(effect: &CompiledEffect) -> usize {
    effect
        .emitters
        .iter()
        .filter(|emitter| emitter.enabled)
        .map(|emitter| emitter.renderers.len())
        .max()
        .unwrap_or(0)
}

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

#[derive(bevy::ecs::system::SystemParam)]
struct TexturePresentationAssets<'w> {
    asset_server: Res<'w, AssetServer>,
    images: Res<'w, Assets<Image>>,
    texture_cache: bevy::prelude::ResMut<'w, TextureAssetCache>,
    fallback_textures: Res<'w, gpu::GpuFallbackTextures>,
}

fn play_effects(
    mut commands: Commands,
    time: Res<Time>,
    capabilities: Res<GpuCapabilities>,
    mut textures: TexturePresentationAssets,
    mut players: Query<(
        Entity,
        &mut EffectPlayer,
        &mut EffectProfiler,
        Option<&Children>,
        &EffectRuntimeStatus,
    )>,
    mut particles: Query<(
        &RuntimeParticle,
        &mut Sprite,
        &mut Transform,
        &mut Visibility,
    )>,
) {
    for (player_entity, mut player, mut profiler, children, runtime) in &mut players {
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

        if runtime.active == ActiveBackend::Gpu {
            continue;
        }

        let uses_gpu_readback =
            runtime.active == ActiveBackend::GpuReadback && !player.gpu_samples.is_empty();
        let samples = if uses_gpu_readback {
            std::mem::take(&mut player.gpu_samples)
        } else {
            let mut samples = std::mem::take(&mut player.samples);
            let started = Instant::now();
            player.instance.evaluate(&mut samples);
            profiler.0.record_cpu_frame(started.elapsed(), &samples);
            profiler.0.record_submitted_frame(player.effect(), &samples);
            samples
        };
        if uses_gpu_readback {
            profiler.0.record_particle_frame(&samples);
            profiler.0.record_submitted_frame(player.effect(), &samples);
        }

        let Some(children) = children else {
            continue;
        };
        for child in children.iter() {
            let Ok((slot, mut sprite, mut transform, mut visibility)) = particles.get_mut(*child)
            else {
                continue;
            };
            let Some(sample) = samples.get(slot.sample_index) else {
                *visibility = Visibility::Hidden;
                continue;
            };
            let Some(emitter) = player.effect().emitters.get(sample.emitter_index) else {
                *visibility = Visibility::Hidden;
                continue;
            };
            let Some(renderer) = emitter.renderers.get(slot.renderer_index) else {
                *visibility = Visibility::Hidden;
                continue;
            };
            let Some(material) = player.effect().material(renderer.material) else {
                *visibility = Visibility::Hidden;
                continue;
            };
            let (texture, uv) = match &renderer.kind {
                aestra_runtime::RendererPlanKind::Sprite => (material.texture, material.uv),
                aestra_runtime::RendererPlanKind::Flipbook {
                    flipbook,
                    time_source,
                    playback,
                    random_start,
                } => {
                    let Some(flipbook) = player.effect().flipbook(*flipbook) else {
                        *visibility = Visibility::Hidden;
                        continue;
                    };
                    let frame = aestra_runtime::flipbook_frame_index(
                        flipbook,
                        aestra_runtime::FlipbookFrameContext {
                            time_source: *time_source,
                            playback: *playback,
                            random_start: *random_start,
                            effect_time: player.elapsed(),
                            normalized_age: sample.normalized_age,
                            particle_index: sample.particle_index,
                            seed: player.instance.seed(),
                        },
                    );
                    (Some(flipbook.texture), flipbook.frames[frame])
                }
            };
            sprite.rect = None;
            sprite.image = textures.fallback_textures.white.clone();
            if let Some(texture) = texture
                && let Some(asset) = player
                    .effect()
                    .assets
                    .iter()
                    .find(|asset| asset.source == texture)
            {
                let handle = textures
                    .texture_cache
                    .load(&textures.asset_server, &asset.path);
                if let Some(image) = textures.images.get(&handle) {
                    let image_size = image.size_f32();
                    sprite.rect = Some(bevy::math::Rect::from_corners(
                        Vec2::from_array(uv.min) * image_size,
                        Vec2::from_array(uv.max) * image_size,
                    ));
                    sprite.image = handle;
                } else {
                    sprite.image = textures.fallback_textures.missing.clone();
                }
            }
            let size = sample.size.max(0.01);
            let color = match &material.color {
                aestra_runtime::MaterialColorPlan::ParticleColor => sample.color,
                aestra_runtime::MaterialColorPlan::Value(value) => {
                    *value.resolve(player.instance.parameter_values())
                }
            };
            sprite.color = Color::srgba(color[0], color[1], color[2], color[3]);
            sprite.custom_size = Some(Vec2::splat(size));
            transform.translation = Vec3::from_array(sample.position);
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
        assert_eq!(cpu_renderer_capacity(player_effect(&app, entity)), 2);
    }

    fn player_effect(app: &App, entity: Entity) -> &CompiledEffect {
        app.world().get::<EffectPlayer>(entity).unwrap().effect()
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
