//! Shared playback driver: clock + instance + optional scrub-checkpoint cache,
//! with the seek/replay/checkpoint logic that both the runtime `EffectPlayer`
//! and the editor's preview duplicate today (client-runtime unification, M-CR2).
//!
//! Duration, playback mode, seek mode, and the checkpoint context are supplied
//! by the caller on each call, so each host keeps its own policy — e.g. the
//! editor's pending-edit-aware duration — without this type knowing about it.

use crate::{
    CheckpointContext, CheckpointPolicy, CheckpointStore, ClockAdvance, EffectInstance,
    PlaybackClock, SeekOrigin, SeekPlan, SimulationSeekMode,
};

/// Owns the playhead clock, the simulated instance, and an optional backward-scrub
/// checkpoint cache. Choreography-event dispatch and status/UI concerns stay with
/// the caller; this type only advances, seeks, and (optionally) checkpoints.
#[derive(Debug, Clone)]
pub struct PlaybackDriver {
    pub clock: PlaybackClock,
    pub instance: EffectInstance,
    checkpoints: Option<CheckpointStore<EffectInstance>>,
}

impl PlaybackDriver {
    /// A driver at frame zero for `instance`, with no scrub cache.
    pub fn new(instance: EffectInstance) -> Self {
        Self {
            clock: PlaybackClock::default(),
            instance,
            checkpoints: None,
        }
    }

    /// Replaces the instance and resets the clock, clearing any scrub cache.
    pub fn reset_instance(&mut self, instance: EffectInstance) {
        self.instance = instance;
        self.clock.restart();
        self.clear_checkpoints();
    }

    pub fn frame(&self) -> u64 {
        self.clock.frame()
    }

    pub fn tick_rate(&self) -> u32 {
        self.clock.tick_rate()
    }

    /// Enables the backward-scrub checkpoint cache with the given policy.
    pub fn enable_checkpoints(&mut self, policy: CheckpointPolicy) {
        self.checkpoints = Some(CheckpointStore::new(policy));
    }

    pub fn disable_checkpoints(&mut self) {
        self.checkpoints = None;
    }

    pub fn checkpoints(&self) -> Option<&CheckpointStore<EffectInstance>> {
        self.checkpoints.as_ref()
    }

    pub fn clear_checkpoints(&mut self) {
        if let Some(store) = &mut self.checkpoints {
            store.clear();
        }
    }

    /// Restarts the clock and the instance to frame zero.
    pub fn restart(&mut self) {
        self.clock.restart();
        self.instance.restart();
    }

    /// Advances the clock, then (for stateful modes) the instance by the elapsed
    /// ticks, recording a scrub checkpoint at the new frame. Choreography events
    /// are the caller's responsibility. Returns the tick result.
    pub fn advance(
        &mut self,
        delta_seconds: f32,
        speed: f32,
        duration: f32,
        looping: bool,
        seek_mode: SimulationSeekMode,
        context: &CheckpointContext,
    ) -> ClockAdvance {
        let previous_frame = self.clock.frame();
        let result = self.clock.advance(delta_seconds, speed, duration, looping);
        if result.ticks == 0 {
            return result;
        }
        if seek_mode != SimulationSeekMode::StatelessDirect {
            let tick_seconds = 1.0 / self.clock.tick_rate() as f32;
            let ticks = if looping {
                result.ticks
            } else {
                self.clock.frame().saturating_sub(previous_frame)
            };
            for _ in 0..ticks {
                self.instance.advance(tick_seconds);
            }
            self.record_checkpoint(seek_mode, context, self.clock.frame());
        }
        result
    }

    /// Seeks to `target_frame`, restoring the nearest usable checkpoint when the
    /// cache is enabled and the mode is `CheckpointRestore`, otherwise replaying
    /// forward (restarting first on a backward jump). Returns the plan taken (for
    /// status/UI). The instance is left positioned at the target frame.
    pub fn seek_frame(
        &mut self,
        target_frame: u64,
        duration: f32,
        seek_mode: SimulationSeekMode,
        context: &CheckpointContext,
    ) -> SeekPlan {
        self.instance.mark_history_discontinuity();
        let target = target_frame.min(self.clock.maximum_frame(duration));
        if seek_mode == SimulationSeekMode::StatelessDirect {
            self.clock.seek_frame(target, duration);
            self.instance.set_playback_time(self.clock.time(duration));
            return SeekPlan {
                target_frame: target,
                origin: SeekOrigin::Direct,
                replay_ticks: 0,
            };
        }
        let current = self.clock.frame();
        let plan = self
            .checkpoints
            .as_ref()
            .map(|store| store.plan_seek(seek_mode, context, current, target))
            .unwrap_or_else(|| cache_less_plan(current, target));
        match plan.origin {
            SeekOrigin::Direct => {
                // StatelessDirect handled above; reachable only if a cache ever
                // returns Direct for a stateful mode, which it does not.
                self.clock.seek_frame(target, duration);
                self.instance.set_playback_time(self.clock.time(duration));
            }
            SeekOrigin::Current => self.replay_ticks(plan.replay_ticks, duration, seek_mode, context),
            SeekOrigin::Checkpoint { frame } => {
                let restored = self
                    .checkpoints
                    .as_ref()
                    .and_then(|store| store.nearest_at_or_before(context, frame))
                    .map(|entry| entry.state.clone());
                if let Some(state) = restored {
                    self.instance = state;
                    self.clock.seek_frame(frame, duration);
                    self.replay_ticks(target - frame, duration, seek_mode, context);
                } else {
                    self.restart();
                    self.replay_ticks(target, duration, seek_mode, context);
                }
            }
            SeekOrigin::Restart => {
                self.restart();
                self.replay_ticks(plan.replay_ticks, duration, seek_mode, context);
            }
        }
        self.instance.set_playback_time(self.instance.time());
        plan
    }

    /// Steps the clock and instance forward `ticks`, recording checkpoints.
    pub fn replay_ticks(
        &mut self,
        ticks: u64,
        duration: f32,
        seek_mode: SimulationSeekMode,
        context: &CheckpointContext,
    ) {
        let tick_seconds = 1.0 / self.clock.tick_rate() as f32;
        for _ in 0..ticks {
            self.clock.step_forward(duration);
            self.instance.advance(tick_seconds);
            self.record_checkpoint(seek_mode, context, self.clock.frame());
        }
    }

    /// Captures the instance state at `frame` when the cache is enabled, the mode
    /// is `CheckpointRestore`, and the policy cadence is due.
    pub fn record_checkpoint(
        &mut self,
        seek_mode: SimulationSeekMode,
        context: &CheckpointContext,
        frame: u64,
    ) {
        if seek_mode != SimulationSeekMode::CheckpointRestore {
            return;
        }
        let due = self
            .checkpoints
            .as_ref()
            .is_some_and(|store| store.policy().should_capture(frame));
        if !due {
            return;
        }
        let state = self.instance.clone();
        let estimated_bytes = std::mem::size_of::<EffectInstance>()
            + std::mem::size_of_val(state.parameter_values());
        if let Some(store) = self.checkpoints.as_mut() {
            store.insert(context.clone(), frame, state, estimated_bytes);
        }
    }
}

/// Plan a seek when there is no checkpoint cache: forward replays from the
/// current frame, backward restarts and replays from zero.
fn cache_less_plan(current: u64, target: u64) -> SeekPlan {
    if target >= current {
        SeekPlan {
            target_frame: target,
            origin: SeekOrigin::Current,
            replay_ticks: target - current,
        }
    } else {
        SeekPlan {
            target_frame: target,
            origin: SeekOrigin::Restart,
            replay_ticks: target,
        }
    }
}
