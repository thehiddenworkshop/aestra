//! Shared deterministic instance scheduling for editor and engine hosts.
use crate::{
    CompiledEffect, CompiledEffectClip, CompiledEffectProject, CompiledParameterOverride,
    HostTransformContext, InheritedHostTransform,
};
use aestra_core::{EffectClipId, EffectId, EffectPlaybackMode};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ScheduledEffectInstance {
    /// Stable identity within one root player (empty for the root).
    pub path: Vec<EffectClipId>,
    pub effect: Arc<CompiledEffect>,
    pub time: f32,
    pub seed: u64,
    pub inherited: Arc<InheritedHostTransform>,
    pub parameter_overrides: Vec<CompiledParameterOverride>,
}

/// A notification crossed by a project clock, independent of presentation entities.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectChoreographyEvent {
    /// Empty for the root; otherwise the stable path from the root to the source.
    pub path: Vec<EffectClipId>,
    pub effect: EffectId,
    /// Crossing time in the root clock supplied to `choreography_events_between`.
    pub root_time: f64,
    /// Authored source-local event (its time is not rewritten to root time).
    pub event: crate::DispatchedChoreographyEvent,
}

#[derive(Clone, Copy)]
struct EventWindow {
    start: f64,
    end: f64,
    include_start: bool,
    root_offset: f64,
}

impl CompiledEffectProject {
    /// Enumerate normal forward playback crossings, including clips that start
    /// and finish within one update. Both looping modes repeat notifications.
    /// Use `include_start` only on fresh playback/restart, never after a seek.
    /// Source-offset pre-roll is silent. Clip ends are inclusive; a loop boundary
    /// emits the old cycle's end followed by the new cycle's time-zero events.
    pub fn choreography_events_between(
        &self,
        start: f32,
        end: f32,
        include_start: bool,
    ) -> Vec<ProjectChoreographyEvent> {
        self.events_between(start, end, include_start)
    }

    /// Enumerate one fixed-clock advance. Restart playback wraps at the clock's
    /// final frame (which can round duration upward); continuous playback keeps
    /// the authored unwrapped clock. Clock repositioning must not call this API.
    pub fn choreography_events_for_clock_advance(
        &self,
        previous: crate::PlaybackClock,
        current: crate::PlaybackClock,
        include_start: bool,
    ) -> Vec<ProjectChoreographyEvent> {
        if self.root.playback_mode != EffectPlaybackMode::LoopRestart {
            return self.events_between(
                previous.elapsed_time(),
                current.elapsed_time(),
                include_start,
            );
        }
        let mut output = Vec::new();
        let frames = current.maximum_frame(self.root.duration);
        if frames == 0
            || previous.tick_rate() != current.tick_rate()
            || current.elapsed_frame <= previous.elapsed_frame
        {
            return output;
        }
        // Split on integer frame boundaries, not rounded floating-point periods.
        let first = previous.elapsed_frame / frames;
        let last = current.elapsed_frame / frames;
        for cycle in first..=last {
            let base = cycle * frames;
            let from = previous.elapsed_frame.saturating_sub(base).min(frames);
            let to = current.elapsed_frame.saturating_sub(base).min(frames);
            self.collect_events(
                &self.root,
                &mut Vec::new(),
                EventWindow {
                    start: f64::from(current.time_for_frame(from, self.root.duration)),
                    end: f64::from(current.time_for_frame(to, self.root.duration)),
                    include_start: cycle > first || include_start,
                    root_offset: base as f64 / f64::from(current.tick_rate()),
                },
                false,
                &mut output,
            );
        }
        sort_events(&mut output);
        output
    }

    fn events_between(
        &self,
        start: f32,
        end: f32,
        include_start: bool,
    ) -> Vec<ProjectChoreographyEvent> {
        let mut output = Vec::new();
        if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start {
            return output;
        }
        self.collect_events(
            &self.root,
            &mut Vec::new(),
            EventWindow {
                start: start.into(),
                end: end.into(),
                include_start,
                root_offset: 0.0,
            },
            true,
            &mut output,
        );
        sort_events(&mut output);
        output
    }

    fn collect_events(
        &self,
        effect: &CompiledEffect,
        path: &mut Vec<EffectClipId>,
        window: EventWindow,
        allow_looping: bool,
        output: &mut Vec<ProjectChoreographyEvent>,
    ) {
        let duration = f64::from(effect.duration);
        if !duration.is_finite() || duration <= 0.0 || path.len() >= 64 {
            return;
        }
        let looping = allow_looping && effect.playback_mode.is_looping();
        let first = if looping {
            (window.start / duration).floor() as u64
        } else {
            0
        };
        let last = if looping {
            (window.end / duration).floor() as u64
        } else {
            0
        };
        for cycle in first..=last {
            let base = cycle as f64 * duration;
            let start = (window.start - base).max(0.0);
            let end = (window.end - base).min(duration);
            let include_start = cycle > first || window.include_start;
            if start > end || (start == end && !include_start) {
                continue;
            }
            let root_offset = window.root_offset + base;
            for event in &effect.choreography_events {
                let time = f64::from(event.time);
                if (time > start || (include_start && time == start)) && time <= end {
                    output.push(ProjectChoreographyEvent {
                        path: path.clone(),
                        effect: effect.source,
                        root_time: root_offset + time,
                        event: event.into(),
                    });
                }
            }
            for clip in &effect.effect_clips {
                let Some(child) = self.effect(clip.source.id) else {
                    continue;
                };
                let clip_start = f64::from(clip.start_time);
                let from = start.max(clip_start);
                let to = end.min(clip_start + f64::from(clip.duration));
                let include_from = from > start || include_start;
                if from > to || (from == to && !include_from) {
                    continue;
                }
                let offset = f64::from(clip.source_offset);
                path.push(clip.source_clip);
                self.collect_events(
                    child,
                    path,
                    EventWindow {
                        start: offset + from - clip_start,
                        end: offset + to - clip_start,
                        include_start: include_from,
                        root_offset: root_offset + clip_start - offset,
                    },
                    true,
                    output,
                );
                path.pop();
            }
        }
    }

    pub fn instances(&self, time: f32, seed: u64) -> Vec<ScheduledEffectInstance> {
        let time = if self.root.playback_mode == EffectPlaybackMode::LoopRestart {
            time.rem_euclid(self.root.duration)
        } else {
            time
        };
        self.instances_with(
            time,
            seed,
            HostTransformContext {
                motion: self.root.host_transform_track.clone(),
                inherited: Arc::default(),
            },
            |_, clip| Some(clip.clone()),
        )
    }

    /// Host policy may suppress a clip/subtree or preview an edited clip. `path`
    /// identifies the parent. The root context allows runtime motion overrides.
    /// `time` is the host's simulation time; an explicit seek to the last frame
    /// of restart playback is preserved, while child scheduling uses loop phase.
    pub fn instances_with(
        &self,
        time: f32,
        seed: u64,
        root_context: HostTransformContext,
        mut resolve_clip: impl FnMut(&[EffectClipId], &CompiledEffectClip) -> Option<CompiledEffectClip>,
    ) -> Vec<ScheduledEffectInstance> {
        let time = match self.root.playback_mode {
            EffectPlaybackMode::Once | EffectPlaybackMode::LoopRestart => {
                time.clamp(0.0, self.root.duration)
            }
            EffectPlaybackMode::LoopContinuous => time.max(0.0),
        };
        let mut result = vec![ScheduledEffectInstance {
            path: vec![],
            effect: self.root.clone(),
            time,
            seed,
            inherited: root_context.inherited,
            parameter_overrides: vec![],
        }];
        // Parent-before-child order, bounded like reference evaluation even for
        // manually assembled invalid projects. Validated projects are acyclic.
        let mut i = 0;
        while i < result.len() {
            let parent = result[i].clone();
            i += 1;
            if parent.path.len() >= 63 {
                continue;
            }
            let motion = if parent.path.is_empty() {
                root_context.motion.clone()
            } else {
                parent.effect.host_transform_track.clone()
            };
            for source in &parent.effect.effect_clips {
                let Some(clip) = resolve_clip(&parent.path, source) else {
                    continue;
                };
                let Some(child) = self.effect(clip.source.id) else {
                    continue;
                };
                let Some((time, offset)) =
                    clip.map_instance_time(parent.time, &parent.effect, child)
                else {
                    continue;
                };
                let mut path = parent.path.clone();
                path.push(clip.source_clip);
                result.push(ScheduledEffectInstance {
                    path,
                    effect: child.clone(),
                    time,
                    seed: clip.seed.resolve(parent.seed, clip.source_clip),
                    inherited: Arc::new(parent.inherited.for_child(
                        motion.clone(),
                        clip.transform,
                        offset,
                    )),
                    parameter_overrides: clip.parameter_overrides,
                });
            }
        }
        result
    }
}

fn sort_events(events: &mut [ProjectChoreographyEvent]) {
    // Stable sorting preserves authored order and end-before-start order for
    // simultaneous events on the same path.
    events.sort_by(|a, b| {
        a.root_time
            .total_cmp(&b.root_time)
            .then_with(|| a.path.cmp(&b.path))
    });
}
