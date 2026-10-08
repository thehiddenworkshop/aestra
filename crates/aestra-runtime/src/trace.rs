//! Recorded host input (host bindings HB8): a [`BindingTrace`] holds an effect's binding frames tick
//! by tick, so a history that read live host objects can be replayed exactly — a backward seek in an
//! editor, a capture, a deterministic test. A host records one while it plays ([`BindingRecorder`]),
//! or a tool authors one ([`BindingTrace::circle`], [`BindingTrace::from_fn`]). An instance driven by
//! a trace ([`crate::EffectInstance::set_binding_trace`]) reads its bindings from it at every time it
//! is put at, and its host input becomes [`aestra_core::HostInputAvailability::Recordable`].
//!
//! Traces are editor state and test input, **not** semantic game identity: an asset never names one.

use crate::{
    BindingFrame, BindingSlot, BindingSnapshot, CompiledEffect, SpatialBindingSnapshot,
    StatefulSimulation,
};
use aestra_core::BindingUpdateMode;

/// The fixed tick a playback time falls in, as the stateful backends count them.
/// A clock's rounded f32 representation of an exact tick boundary maps back to that
/// tick. Other sub-tick times still floor; even the previous representable float is
/// not promoted. Dividing by an f32 reciprocal loses final ticks (18s became 1079).
pub fn trace_tick(time: f32) -> u64 {
    let time = time.max(0.0);
    let rate = f64::from(crate::DEFAULT_PLAYBACK_TICK_RATE);
    let ticks = f64::from(time) * rate;
    let nearest = ticks.round();
    if time == (nearest / rate) as f32 {
        nearest as u64
    } else {
        ticks.floor() as u64
    }
}

/// An effect's binding frames, one per fixed tick from tick 0. Past its end the last frame holds.
#[derive(Debug, Clone, PartialEq)]
pub struct BindingTrace {
    frames: Vec<BindingFrame>,
    identity: u64,
}

impl BindingTrace {
    /// A trace of `frames`, frame `i` at tick `i`. Its identity hashes every value, so two traces
    /// with the same content restore each other's checkpoints and different ones never do.
    pub fn new(frames: Vec<BindingFrame>) -> Self {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        let mut mix = |word: u64| hash = (hash ^ word).wrapping_mul(0x0000_0100_0000_01b3);
        mix(frames.len() as u64);
        for frame in &frames {
            mix(frame.snapshots.len() as u64);
            for snapshot in &frame.snapshots {
                match snapshot {
                    None => mix(u64::MAX),
                    Some(snapshot) => {
                        mix(u64::from(snapshot.present));
                        for value in &snapshot.values {
                            mix(u64::from(value.to_bits()));
                        }
                    }
                }
            }
        }
        Self {
            frames,
            identity: hash,
        }
    }

    /// A trace of `ticks` frames, each made by `frame(tick)`.
    pub fn from_fn(ticks: u64, frame: impl FnMut(u64) -> BindingFrame) -> Self {
        Self::new((0..ticks).map(frame).collect())
    }

    /// A mock target for previews and tests: the spatial binding `slot` of `effect` circling
    /// `center` in the XZ plane at `radius`, `speed` radians per second, for `ticks` ticks — with
    /// its velocity when the binding declares one. Every other slot is unbound.
    pub fn circle(
        effect: &CompiledEffect,
        slot: BindingSlot,
        center: [f32; 3],
        radius: f32,
        speed: f32,
        ticks: u64,
    ) -> Result<Self, crate::BindingError> {
        let binding = effect
            .bindings
            .get(slot.0)
            .ok_or(crate::BindingError::UnknownSlot(slot.0))?;
        Ok(Self::from_fn(ticks, |tick| {
            let angle = tick as f32 * StatefulSimulation::TICK_DT * speed;
            let (sin, cos) = angle.sin_cos();
            let mut object = SpatialBindingSnapshot::at([
                center[0] + radius * cos,
                center[1],
                center[2] + radius * sin,
            ]);
            object.linear_velocity = Some([-radius * speed * sin, 0.0, radius * speed * cos]);
            let mut snapshots = vec![None; effect.bindings.len()];
            snapshots[slot.0] = Some(object.to_snapshot(&binding.layout));
            BindingFrame { snapshots }
        }))
    }

    /// The frame at `tick`: the last one past the end, `None` for an empty trace.
    pub fn frame(&self, tick: u64) -> Option<&BindingFrame> {
        let last = self.frames.len().checked_sub(1)?;
        self.frames.get((tick as usize).min(last))
    }

    /// Ticks recorded.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The content hash that identifies this trace in checkpoint contexts.
    pub fn identity(&self) -> u64 {
        self.identity
    }

    /// What readers of `effect` see at each tick from 0 to `ticks` (exclusive): `Live` slots the
    /// tick's value, `SnapshotOnSpawn` slots the first value the trace supplied — the per-tick view a
    /// stateful backend replays with.
    pub fn resolved(
        &self,
        effect: &CompiledEffect,
        ticks: u64,
    ) -> Vec<Vec<Option<BindingSnapshot>>> {
        let mut latched: Vec<Option<BindingSnapshot>> = vec![None; effect.bindings.len()];
        (0..ticks)
            .map(|tick| {
                let frame = self.frame(tick);
                effect
                    .bindings
                    .iter()
                    .enumerate()
                    .map(|(index, binding)| {
                        let value = frame
                            .and_then(|frame| frame.snapshots.get(index))
                            .cloned()
                            .flatten();
                        if latched[index].is_none() {
                            latched[index] = value.clone();
                        }
                        match binding.update_mode {
                            BindingUpdateMode::Live => value,
                            BindingUpdateMode::SnapshotOnSpawn => latched[index].clone(),
                        }
                    })
                    .collect()
            })
            .collect()
    }
}

/// Records the binding frames a host pushes while an effect plays forward, one per tick. A tick the
/// host skipped (a slow frame) repeats the previous frame; recording at an earlier tick (the user
/// seeked back) drops everything from there, so the trace is always one forward history.
#[derive(Debug, Clone, Default)]
pub struct BindingRecorder {
    frames: Vec<BindingFrame>,
}

impl BindingRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `frame` as the host input at `tick`.
    pub fn record(&mut self, tick: u64, frame: &BindingFrame) {
        let tick = tick as usize;
        self.frames.truncate(tick);
        while self.frames.len() < tick {
            let held = self.frames.last().cloned().unwrap_or_else(|| BindingFrame {
                snapshots: vec![None; frame.snapshots.len()],
            });
            self.frames.push(held);
        }
        self.frames.push(frame.clone());
    }

    /// Ticks recorded so far.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The trace recorded so far.
    pub fn trace(&self) -> BindingTrace {
        BindingTrace::new(self.frames.clone())
    }
}

impl crate::CompiledHoming {
    /// The target each tick from 0 to `ticks` steers toward under `trace` (host bindings HB8): the
    /// trace's input resolved into effect space by `world_to_effect`, losses resolved by a tracker in
    /// tick order — exactly what a live run over the recorded input saw, so a replay from any tick
    /// steers the same way.
    pub fn schedule(
        &self,
        effect: &CompiledEffect,
        trace: &BindingTrace,
        ticks: u64,
        world_to_effect: [[f32; 4]; 3],
    ) -> Vec<Option<crate::HomingTarget>> {
        let mut tracker = crate::HomingTracker::default();
        trace
            .resolved(effect, ticks)
            .iter()
            .map(|values| {
                let input = self.resolve_with(
                    effect,
                    |slot| values.get(slot.0).and_then(Option::as_ref),
                    world_to_effect,
                );
                tracker.resolve(self.config.lost, input)
            })
            .collect()
    }
}

impl crate::CompiledAttachment {
    /// The attached emitter's transform each tick from 0 to `ticks` under `trace` (host bindings
    /// HB8): the bound pose composed with `authored`, holding the last one while the trace supplies
    /// none (`authored` before the first).
    pub fn schedule(
        &self,
        effect: &CompiledEffect,
        trace: &BindingTrace,
        ticks: u64,
        world_to_effect: [[f32; 4]; 3],
        authored: aestra_core::EmitterTransform,
    ) -> Vec<aestra_core::EmitterTransform> {
        let mut last = authored;
        trace
            .resolved(effect, ticks)
            .iter()
            .map(|values| {
                if let Some(transform) = self.resolve_with(
                    effect,
                    |slot| values.get(slot.0).and_then(Option::as_ref),
                    world_to_effect,
                    authored,
                ) {
                    last = transform;
                }
                last
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_boundaries_round_trip_without_promoting_subtick_times() {
        let clock = crate::PlaybackClock::default();
        for tick in 1..=216_000 {
            let time = clock.time_for_frame(tick, 3600.0);
            assert_eq!(trace_tick(time), tick, "tick {tick}, time {time}");
            assert_eq!(trace_tick(time.next_down()), tick - 1, "before tick {tick}");
            assert_eq!(trace_tick(time.next_up()), tick, "after tick {tick}");
            assert_eq!(trace_tick((tick as f64 / 60.0 + 0.5 / 60.0) as f32), tick);
        }
        assert_eq!(trace_tick(18.0), 1080);
        assert_eq!(trace_tick(-1.0), 0);
        assert_eq!(trace_tick(0.0), 0);
    }
}
