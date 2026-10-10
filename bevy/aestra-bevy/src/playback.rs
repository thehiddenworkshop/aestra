//! Canonical host clock, presentation synchronization and choreography delivery.
use crate::{AestraOutputEvent, EffectRenderMode, PresentedEffect};
use aestra_compiler::{CompileError, EffectCompiler};
use aestra_core::*;
use aestra_runtime::*;
use bevy::prelude::*;
use std::sync::Arc;

/// Public scheduling points for applications that coordinate editor or game state with playback.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AestraSet {
    /// Resolves bound host objects (`AestraBindings`) into snapshots before playback.
    ResolveHostInputs,
    /// Advances effect clocks and updates their presentation for the current frame.
    Playback,
    /// Current presentation snapshots and project totals, after render preparation.
    Profile,
    /// Optional transient-light adapter; host cue-to-light bindings run before this.
    SceneOutputs,
}

/// Fired after an [`EffectPlayer`] crosses a compiled choreography event during normal playback.
/// Applications can consume it with a Bevy observer without coupling game logic to particle
/// lifecycle links or polling the player timeline. The same cue also arrives in the one output
/// stream, [`AestraOutputEvent`] (event system E2b), which new code should prefer.
#[derive(Event, Debug, Clone)]
pub struct AestraChoreographyEvent {
    /// Root player, including when the source is a nested clip.
    pub player: Entity,
    /// Empty for a root event. Does not depend on transient child entities.
    pub clip_path: Vec<EffectClipId>,
    pub effect: EffectId,
    pub event: DispatchedChoreographyEvent,
}

pub(crate) fn prepare_player_presentations(
    mut commands: Commands,
    players: Query<(Entity, &EffectPlayer), Without<PresentedEffect>>,
) {
    for (entity, player) in &players {
        let mut presented = PresentedEffect::new(player.effect().clone());
        presented.instance = player.instance().clone();
        presented.set_render_mode(player.render_mode());
        commands.entity(entity).insert(presented);
    }
}

pub(crate) fn sync_player_presentations(mut players: Query<(&EffectPlayer, &mut PresentedEffect)>) {
    for (player, mut presented) in &mut players {
        presented.instance = player.instance().clone();
        presented.set_render_mode(player.render_mode());
        presented.set_seek_quality(player.seek_quality());
    }
}

/// A Bevy component that owns mutable state for one compiled effect instance.
#[derive(Component)]
#[require(Transform, Visibility)]
pub struct EffectPlayer {
    /// Owns the playhead clock, the simulated instance, and the optional
    /// backward-scrub checkpoint cache (shared with the editor via M-CR2).
    pub(crate) driver: PlaybackDriver,
    pub speed: f32,
    pub playing: bool,
    /// The fidelity requested of stateful seeks (hybrid roadmap M12). `Exact` by default — playback and
    /// settled positions reconstruct the authoritative state; a host sets `Preview` for the duration of
    /// a scrub gesture so rapid scrubbing stays responsive, then restores `Exact` on release.
    seek_quality: SeekQuality,
    render_mode: EffectRenderMode,
    pub(crate) choreography_events: Vec<DispatchedChoreographyEvent>,
    pub(crate) project_choreography_events: Vec<ProjectChoreographyEvent>,
    choreography_started: bool,
    project: Option<Arc<CompiledEffectProject>>,
    /// Bumped whenever the compiled effect is replaced, so cached checkpoints
    /// from a previous version are never restored.
    revision: u64,
}

impl EffectPlayer {
    /// Select host-owned checkpoint retention for this root and its nested effects.
    pub fn with_history_policy(mut self, policy: PlaybackHistoryPolicy) -> Self {
        self.set_history_policy(policy);
        self
    }

    pub fn history_policy(&self) -> PlaybackHistoryPolicy {
        self.instance().history_policy()
    }

    /// Switching to playback-only releases CPU caches now and GPU caches on the
    /// next render preparation, without resetting the live state. Replay-enabled
    /// resumes future GPU captures; CPU caching needs `enable_scrub_cache`.
    pub fn set_history_policy(&mut self, policy: PlaybackHistoryPolicy) {
        self.driver.set_history_policy(policy);
    }

    pub fn try_new(effect: &EffectAsset) -> Result<Self, CompileError> {
        let compiled = EffectCompiler::default().compile(effect)?;
        Ok(Self::from_compiled(Arc::new(compiled)))
    }

    pub fn new(effect: &EffectAsset) -> Self {
        Self::try_new(effect).expect("effect must compile before playback")
    }

    pub fn from_compiled(effect: Arc<CompiledEffect>) -> Self {
        Self {
            driver: PlaybackDriver::new(EffectInstance::new(effect)),
            speed: 1.0,
            playing: true,
            seek_quality: SeekQuality::Exact,
            render_mode: EffectRenderMode::Rendered,
            choreography_events: Vec::new(),
            project_choreography_events: Vec::new(),
            choreography_started: false,
            project: None,
            revision: 0,
        }
    }

    /// The simulated effect instance (read).
    pub fn instance(&self) -> &EffectInstance {
        &self.driver.instance
    }

    /// The simulated effect instance (mutable), for hosts driving parameters etc.
    pub fn instance_mut(&mut self) -> &mut EffectInstance {
        &mut self.driver.instance
    }

    /// Own one root clock; the plugin reconciles active child presentations.
    pub fn from_project(project: Arc<CompiledEffectProject>) -> Self {
        let mut player = Self::from_compiled(project.root.clone());
        player.project = Some(project);
        player
    }

    pub fn project(&self) -> Option<&Arc<CompiledEffectProject>> {
        self.project.as_ref()
    }

    pub fn effect(&self) -> &Arc<CompiledEffect> {
        self.driver.instance.effect()
    }

    /// Motion relative to this entity's stable placement. Changes invalidate trail checkpoints.
    pub fn set_host_transform_track(
        &mut self,
        track: Option<Arc<aestra_runtime::CompiledHostTransformTrack>>,
    ) {
        self.driver.instance.set_host_transform_track(track);
    }

    /// Historical ancestor motion and clip placements, supplied by a project host.
    pub fn set_inherited_host_transform(
        &mut self,
        inherited: Arc<aestra_runtime::InheritedHostTransform>,
    ) {
        self.driver.instance.set_inherited_host_transform(inherited);
    }

    pub fn render_mode(&self) -> EffectRenderMode {
        self.render_mode
    }

    pub fn set_render_mode(&mut self, mode: EffectRenderMode) {
        self.render_mode = mode;
    }

    pub fn elapsed(&self) -> f32 {
        self.driver.clock.time(self.effect().duration)
    }

    /// Simulation time, which remains unwrapped for seamless continuous looping.
    pub fn simulation_time(&self) -> f32 {
        if self.effect().playback_mode.is_continuous() {
            self.driver.clock.elapsed_time()
        } else {
            self.elapsed()
        }
    }

    pub fn frame(&self) -> u64 {
        self.driver.clock.frame()
    }

    pub fn tick_rate(&self) -> u32 {
        self.driver.clock.tick_rate()
    }

    pub fn seek_mode(&self) -> SimulationSeekMode {
        self.effect().seek_mode
    }

    /// The fidelity requested of stateful seeks this frame (hybrid roadmap M12).
    pub fn seek_quality(&self) -> SeekQuality {
        self.seek_quality
    }

    /// How the effect's host input can be recovered for a past tick (host bindings HB8):
    /// `ForwardOnly` while the simulation reads live bindings nobody records.
    pub fn host_input_availability(&self) -> aestra_core::HostInputAvailability {
        self.driver.instance.host_input_availability()
    }

    /// Whether seeking backward reproduces the history exactly (host bindings HB8). False while a
    /// stateful effect reads live host objects that are not recorded: the replay then uses the
    /// present input for the past. Record them (AestraBindingRecorder) and replay the trace
    /// (AestraBindingTrace) for exact scrubbing.
    pub fn supports_exact_backward_seek(&self) -> bool {
        self.driver.instance.supports_exact_backward_seek()
    }

    /// Requests a stateful-seek fidelity. A host sets [`SeekQuality::Preview`] while the user is
    /// actively scrubbing the timeline (bounded reconstruction keeps the editor responsive), and
    /// restores [`SeekQuality::Exact`] when the cursor settles so the authoritative state is
    /// reconstructed. Only affects stateful effects; analytic seeks are already direct.
    pub fn set_seek_quality(&mut self, quality: SeekQuality) {
        self.seek_quality = quality;
    }

    pub fn restart(&mut self) {
        self.silence_choreography_events();
        self.choreography_started = false;
        self.driver.clock.restart();
        self.driver.instance.restart();
        self.playing = true;
    }

    /// Swaps in a freshly compiled version of the effect (e.g. after a live edit),
    /// keeping the current seed. With `preserve_position` the new effect is
    /// replayed forward to the current frame so the playhead does not jump;
    /// otherwise playback restarts at zero. The running/paused state is untouched.
    pub fn replace_effect(&mut self, effect: Arc<CompiledEffect>, preserve_position: bool) {
        let seed = self.driver.instance.seed();
        let history_policy = self.history_policy();
        let target = if preserve_position {
            self.driver.clock.frame()
        } else {
            0
        };
        self.silence_choreography_events();
        self.choreography_started = false;
        self.driver.instance =
            EffectInstance::with_seed(effect, seed).with_history_policy(history_policy);
        // Old checkpoints describe the previous effect; drop them and bump the
        // revision so any that linger are never restored.
        self.revision += 1;
        self.driver.clear_checkpoints();
        self.driver.clock.restart();
        if target > 0 {
            // Replays the new instance from zero up to the retained frame.
            self.seek_frame(target);
        }
    }

    pub fn seek(&mut self, time: f32) {
        let duration = self.effect().duration;
        let mut target = self.driver.clock;
        target.seek_seconds(time, duration);
        self.seek_frame(target.frame());
    }

    /// Seeks continuous playback using absolute simulation time while leaving the playhead
    /// wrapped to the authored effect duration.
    pub fn seek_simulation_time(&mut self, time: f32) {
        self.silence_choreography_events();
        self.driver.instance.mark_history_discontinuity_at(time);
        let duration = self.effect().duration;
        if self.effect().playback_mode.is_continuous() {
            self.driver.clock.seek_elapsed_seconds(time, duration);
            self.sync_instance_time();
        } else {
            self.seek(time);
        }
    }

    /// Synchronize sequential playback driven by an external clock without
    /// treating every frame as a seek. Use `seek_simulation_time` for jumps.
    pub fn set_playback_time(&mut self, time: f32) {
        self.silence_choreography_events();
        let duration = self.effect().duration;
        if self.effect().playback_mode.is_continuous() {
            self.driver.clock.seek_elapsed_seconds(time, duration);
        } else {
            self.driver.clock.seek_seconds(time, duration);
        }
        self.sync_instance_time();
    }

    pub fn seek_frame(&mut self, frame: u64) {
        self.silence_choreography_events();
        let duration = self.effect().duration;
        let seek_mode = self.seek_mode();
        if seek_mode == SimulationSeekMode::StatelessDirect {
            // Stateless positioning is continuous-aware here (see sync_instance_time),
            // which the shared driver does not do — keep it in the player.
            let target = frame.min(self.driver.clock.maximum_frame(duration));
            self.driver
                .instance
                .mark_history_discontinuity_at(target as f32 / self.tick_rate() as f32);
            self.driver.clock.seek_frame(target, duration);
            self.sync_instance_time();
            return;
        }
        let context = self.scrub_context();
        self.driver.seek_frame(frame, duration, seek_mode, &context);
    }

    fn scrub_context(&self) -> CheckpointContext {
        CheckpointContext {
            effect: self.effect().source,
            revision: self.revision,
            seed: self.driver.instance.seed(),
            backend: CheckpointBackendId::new("cpu-reference"),
            host_input: self.driver.instance.host_input_epoch(),
        }
    }

    /// Enables the backward-scrub checkpoint cache with the given policy. Off by
    /// default; hosts that scrub (e.g. an editor timeline) opt in.
    pub fn enable_scrub_cache(&mut self, policy: CheckpointPolicy) {
        self.driver.enable_checkpoints(policy);
    }

    /// Disables and drops the scrub cache.
    pub fn disable_scrub_cache(&mut self) {
        self.driver.disable_checkpoints();
    }

    /// The scrub cache, if enabled (for status/introspection).
    pub fn scrub_cache(&self) -> Option<&CheckpointStore<EffectInstance>> {
        self.driver.checkpoints()
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
        self.driver.instance.set_seed(seed);
    }

    pub fn checkpoint(&self) -> PlaybackCheckpoint {
        self.driver.clock.checkpoint()
    }

    pub fn restore_checkpoint(&mut self, checkpoint: PlaybackCheckpoint) {
        let duration = self.effect().duration;
        let mut target = self.driver.clock;
        target.restore(checkpoint, duration);
        self.seek_frame(target.frame());
        self.playing = false;
    }

    pub fn set_parameter(&mut self, id: ParameterId, value: Value) -> Result<(), ParameterError> {
        self.driver.instance.set_parameter(id, value)
    }

    pub fn clear_parameter(&mut self, id: ParameterId) -> Result<(), ParameterError> {
        self.driver.instance.clear_parameter(id)
    }

    /// Drains root choreography events produced by the most recent clock advance. The plugin drains
    /// this automatically and emits [`AestraChoreographyEvent`]; manual player integrations can
    /// use the same queue directly.
    pub fn drain_choreography_events(
        &mut self,
    ) -> impl Iterator<Item = DispatchedChoreographyEvent> + '_ {
        self.choreography_events.drain(..)
    }

    /// Project notifications, including the root, ordered by crossing time then
    /// clip path. Empty for single-effect players. Use this instead of the root
    /// queue for manual project integrations to avoid dispatching roots twice.
    pub fn drain_project_choreography_events(
        &mut self,
    ) -> impl Iterator<Item = ProjectChoreographyEvent> + '_ {
        self.project_choreography_events.drain(..)
    }

    fn silence_choreography_events(&mut self) {
        self.choreography_events.clear();
        self.project_choreography_events.clear();
        self.choreography_started = true;
    }

    pub(crate) fn advance_clock(&mut self, delta_seconds: f32) -> ClockAdvance {
        let duration = self.effect().duration;
        let playback_mode = self.effect().playback_mode;
        let looping = playback_mode.is_looping();
        let previous_frame = self.driver.clock.frame();
        let previous_clock = self.driver.clock;
        let result = self
            .driver
            .clock
            .advance(delta_seconds, self.speed, duration, looping);
        self.choreography_events.clear();
        self.project_choreography_events.clear();
        if result.ticks == 0 {
            return result;
        }
        if let Some(project) = &self.project {
            self.project_choreography_events = project.choreography_events_for_clock_advance(
                previous_clock,
                self.driver.clock,
                !self.choreography_started,
            );
        }
        self.choreography_started = true;
        match self.seek_mode() {
            SimulationSeekMode::StatelessDirect => {
                self.driver.instance.advance_clock_with_choreography_events(
                    previous_clock,
                    self.driver.clock,
                    &mut self.choreography_events,
                );
            }
            SimulationSeekMode::CheckpointRestore | SimulationSeekMode::RestartReplay => {
                let ticks = if looping {
                    result.ticks
                } else {
                    self.driver.clock.frame().saturating_sub(previous_frame)
                };
                let mut events = Vec::new();
                let mut tick_clock = previous_clock;
                for _ in 0..ticks {
                    let previous_tick = tick_clock;
                    tick_clock.advance_ticks(1, duration, looping);
                    self.driver.instance.advance_clock_with_choreography_events(
                        previous_tick,
                        tick_clock,
                        &mut events,
                    );
                    self.choreography_events.append(&mut events);
                }
            }
        }
        if self.project.is_some() {
            self.choreography_events = self
                .project_choreography_events
                .iter()
                .filter(|event| event.path.is_empty())
                .map(|event| event.event.clone())
                .collect();
        }
        // Capture a scrub checkpoint at the new position so later backward seeks
        // can restore instead of replaying from zero.
        let seek_mode = self.seek_mode();
        let context = self.scrub_context();
        self.driver
            .record_checkpoint(seek_mode, &context, self.driver.clock.frame());
        result
    }

    fn sync_instance_time(&mut self) {
        let time = if self.driver.instance.effect().playback_mode.is_continuous() {
            self.driver.clock.elapsed_time()
        } else {
            self.driver
                .clock
                .time(self.driver.instance.effect().duration)
        };
        self.driver.instance.set_playback_time(time);
    }
}

/// Delivers the cues playback crossed: as `AestraChoreographyEvent`s, and in the one output stream
/// as `AestraOutputEvent`s dated by their crossing tick (event system E2b).
pub(crate) fn dispatch_choreography_events(
    commands: &mut Commands,
    outputs: &mut bevy::prelude::MessageWriter<AestraOutputEvent>,
    root: Entity,
    player: &mut EffectPlayer,
) {
    let playback_epoch = player.instance().history_epoch();
    if player.project().is_some() {
        player.choreography_events.clear();
        for event in player.drain_project_choreography_events() {
            outputs.write(AestraOutputEvent {
                effect: root,
                clip_path: event.path.clone(),
                playback_epoch: Some(playback_epoch),
                particle: None,
                event: EffectOutputEvent::from_cue(
                    &event.event,
                    trace_tick(event.root_time as f32),
                ),
            });
            commands.trigger(AestraChoreographyEvent {
                player: root,
                clip_path: event.path,
                effect: event.effect,
                event: event.event,
            });
        }
    } else {
        let effect = player.effect().source;
        let (now, duration) = (player.instance().time(), player.effect().duration);
        let continuous = player.effect().playback_mode.is_continuous();
        for event in player.drain_choreography_events() {
            let tick = aestra_runtime::cue_crossing_tick(event.time, now, duration, continuous);
            outputs.write(
                AestraOutputEvent::root(root, EffectOutputEvent::from_cue(&event, tick))
                    .in_epoch(playback_epoch),
            );
            commands.trigger(AestraChoreographyEvent {
                player: root,
                clip_path: Vec::new(),
                effect,
                event,
            });
        }
    }
}

/// Advances every player exactly once, independently of renderer/profile readiness.
pub(crate) fn advance_players(
    mut commands: Commands,
    time: Res<Time>,
    mut players: Query<(Entity, &mut EffectPlayer)>,
    mut outputs: MessageWriter<AestraOutputEvent>,
) {
    for (entity, mut player) in &mut players {
        if player.playing && player.advance_clock(time.delta_secs()).reached_end {
            player.playing = false;
        }
        dispatch_choreography_events(&mut commands, &mut outputs, entity, &mut player);
    }
}
