//! Runtime events and stage outputs on the host side (fluid F11, host bindings HB9): the values a
//! stage reports ([`StageOutput`]) read out of the words the host read back, the runtime events they
//! raise, and the effect's other runtime events — a homing target lost or acquired, particles
//! reaching it, the playback finishing. Engine-neutral: a host adapter (Bevy, say) reads what the
//! backend reports after each frame's ticks and maps the events onto its own event model. Events are
//! visual outcomes for gameplay to *hear*, never state it must obey (see `docs/ARCHITECTURE.md`,
//! "Gameplay authority").

use crate::{
    CUE_CAMERA_SHAKE, CUE_PLAY_SOUND, CUE_SPAWN_CHILD_EFFECT, DispatchedChoreographyEvent,
    ExecutionBlock, OutputEvent, StageOutput, trace_tick,
};
use aestra_core::{ChoreographyEventId, ChoreographyEventPayload, ModuleId, ResourceTypeId};
use std::collections::{BTreeMap, BTreeSet};

/// One output's value as the host read it.
#[derive(Debug, Clone, PartialEq)]
pub struct StageOutputValue {
    /// The stage, as indexed in `CompiledEffect::all_extension_stages`.
    pub stage: usize,
    /// The output's place among the stage's outputs.
    pub index: usize,
    pub name: String,
    pub source: Option<ModuleId>,
    pub value: Vec<f32>,
    pub event: Option<OutputEvent>,
}

impl StageOutputValue {
    /// The value's Euclidean length.
    pub fn magnitude(&self) -> f32 {
        self.value.iter().map(|v| v * v).sum::<f32>().sqrt()
    }
}

/// Something hit: a stage output rose past its threshold (a fluid pushing a collider), or homing
/// particles reached their target. Its magnitude is the force, or the number of arrivals.
pub const EVENT_IMPACT: &str = "impact";
/// The homing target is no longer supplied; the value is where it was last seen (world space).
pub const EVENT_TARGET_LOST: &str = "target_lost";
/// The homing target is supplied again (or for the first time); the value is where (world space).
pub const EVENT_TARGET_ACQUIRED: &str = "target_acquired";
/// A play-once effect's playback reached its end. Particles may still be alive.
pub const EVENT_FINISHED: &str = "finished";

/// What raised a runtime event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventOrigin {
    /// An extension stage, as indexed in `CompiledEffect::all_extension_stages`.
    Stage(usize),
    /// An emitter, by index in `CompiledEffect::emitters`.
    Emitter(usize),
    /// The effect as a whole.
    Effect,
    /// A cue on the effect's timeline (event system E2b).
    Timeline(ChoreographyEventId),
}

/// An event an effect raised for its host — the one output stream (event system §12B): an output rose
/// past its threshold, a homing target was lost or acquired, particles reached it, the playback
/// finished (host bindings HB9), or playback crossed a timeline cue (`EventOrigin::Timeline`).
/// `kind` is one of the `EVENT_*` / `CUE_*` names, a cue's topic, or a plugin's own.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectOutputEvent {
    /// The event's kind, e.g. [`EVENT_IMPACT`].
    pub kind: String,
    /// The output that raised it, e.g. `force`; `homing` for homing events; empty otherwise.
    pub output: String,
    /// The authored module that raised it, when one did.
    pub source: Option<ModuleId>,
    pub origin: EventOrigin,
    pub value: Vec<f32>,
    pub magnitude: f32,
    /// The fixed tick it happened at, in the instance's time. A host hearing it frames later (a GPU
    /// read-back) still knows when.
    pub tick: u64,
    /// Text the event carries: a cue's sound or child effect.
    pub text: Option<String>,
}

impl EffectOutputEvent {
    /// An event with no output behind it: `kind` from `origin` at `tick`, carrying `value`.
    pub fn new(
        kind: &str,
        origin: EventOrigin,
        output: &str,
        value: Vec<f32>,
        magnitude: f32,
        tick: u64,
    ) -> Self {
        Self {
            kind: kind.into(),
            output: output.into(),
            source: None,
            origin,
            value,
            magnitude,
            tick,
            text: None,
        }
    }

    /// A timeline cue as an output (event system E2b): a notification by its topic (its name when
    /// the topic is empty), a sound with its cue, a camera shake with its intensity as the value, a
    /// child effect with its path. `tick` is when playback crossed it.
    pub fn from_cue(cue: &DispatchedChoreographyEvent, tick: u64) -> Self {
        let (kind, value, text) = match &cue.payload {
            ChoreographyEventPayload::GameplayNotify { topic } if !topic.trim().is_empty() => {
                (topic.as_str(), Vec::new(), None)
            }
            ChoreographyEventPayload::GameplayNotify { .. } => {
                (cue.name.as_str(), Vec::new(), None)
            }
            ChoreographyEventPayload::PlaySound { cue: sound } => {
                (CUE_PLAY_SOUND, Vec::new(), Some(sound.clone()))
            }
            ChoreographyEventPayload::CameraShake { intensity } => {
                (CUE_CAMERA_SHAKE, vec![*intensity], None)
            }
            ChoreographyEventPayload::SpawnChildEffect { effect } => {
                (CUE_SPAWN_CHILD_EFFECT, Vec::new(), Some(effect.clone()))
            }
        };
        let magnitude = value.iter().map(|v| v * v).sum::<f32>().sqrt();
        Self {
            text,
            ..Self::new(
                kind,
                EventOrigin::Timeline(cue.source),
                &cue.name,
                value,
                magnitude,
                tick,
            )
        }
    }
}

/// The tick playback crossed a cue at `cue_time` (effect time), seen at instance time `now`: in
/// continuous playback the cue repeats each `duration`, so the latest crossing at or before `now`.
pub fn cue_crossing_tick(cue_time: f32, now: f32, duration: f32, continuous: bool) -> u64 {
    let crossing = if continuous && duration > 0.0 {
        cue_time + ((now - cue_time) / duration).floor().max(0.0) * duration
    } else {
        cue_time
    };
    trace_tick(crossing)
}

/// Raises `finished` once when a play-once effect's playback reaches its end, and again after each
/// restart that reaches it again.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FinishedTracker {
    finished: bool,
}

impl FinishedTracker {
    /// Observes `instance`'s playback; returns the event when it has just finished.
    pub fn observe(&mut self, instance: &crate::EffectInstance) -> Option<EffectOutputEvent> {
        let effect = instance.effect();
        let done = effect.playback_mode == aestra_core::EffectPlaybackMode::Once
            && instance.time() >= effect.duration;
        let raised = done && !self.finished;
        self.finished = done;
        raised.then(|| {
            EffectOutputEvent::new(
                EVENT_FINISHED,
                EventOrigin::Effect,
                "",
                Vec::new(),
                0.0,
                trace_tick(effect.duration),
            )
        })
    }
}

/// Reads stage `stage`'s outputs from `words`, the read-back contents of its output resources.
/// Outputs whose resource was not read are skipped.
pub fn read_stage_outputs(
    stage: usize,
    block: &ExecutionBlock,
    words: &BTreeMap<ResourceTypeId, Vec<u32>>,
) -> Vec<StageOutputValue> {
    block
        .outputs
        .iter()
        .enumerate()
        .filter_map(|(index, output): (usize, &StageOutput)| {
            let resource = words.get(&output.resource)?;
            let start = output.word as usize;
            let value = resource
                .get(start..start + output.components as usize)?
                .iter()
                .map(|word| f32::from_bits(*word))
                .collect();
            Some(StageOutputValue {
                stage,
                index,
                name: output.name.clone(),
                source: output.source,
                value,
                event: output.event.clone(),
            })
        })
        .collect()
}

/// Turns successive reads of an effect's outputs into runtime events: an output raises its event when
/// its magnitude is above the threshold and was not at the previous read (edge-triggered, so a lasting
/// push raises one event, not one a frame).
#[derive(Debug, Clone, Default)]
pub struct OutputEventTracker {
    above: BTreeSet<(usize, usize)>,
}

impl OutputEventTracker {
    /// Observes one read of outputs, taken at `tick`; returns the events it raises.
    pub fn observe(&mut self, values: &[StageOutputValue], tick: u64) -> Vec<EffectOutputEvent> {
        let mut events = Vec::new();
        for value in values {
            let Some(event) = &value.event else {
                continue;
            };
            let key = (value.stage, value.index);
            let magnitude = value.magnitude();
            if magnitude > event.threshold {
                if self.above.insert(key) {
                    events.push(EffectOutputEvent {
                        kind: event.kind.clone(),
                        output: value.name.clone(),
                        source: value.source,
                        origin: EventOrigin::Stage(value.stage),
                        value: value.value.clone(),
                        magnitude,
                        tick,
                        text: None,
                    });
                }
            } else {
                self.above.remove(&key);
            }
        }
        events
    }

    /// Forgets what was above its threshold (a restart or a seek): the next read raises afresh.
    pub fn reset(&mut self) {
        self.above.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResourceDescriptor, ResourceLifetime};

    fn block() -> ExecutionBlock {
        ExecutionBlock {
            resources: vec![ResourceDescriptor {
                id: ResourceTypeId::new("org.x::resource/outputs"),
                bytes: 32,
                lifetime: ResourceLifetime::Persistent,
            }],
            ops: Vec::new(),
            constants: Vec::new(),
            fields: Vec::new(),
            emissions: Vec::new(),
            outputs: vec![StageOutput {
                name: "force".into(),
                source: None,
                resource: ResourceTypeId::new("org.x::resource/outputs"),
                word: 4,
                components: 3,
                event: Some(OutputEvent {
                    kind: "impact".into(),
                    threshold: 10.0,
                }),
            }],
        }
    }

    fn read(force: [f32; 3]) -> Vec<StageOutputValue> {
        let mut words = vec![0u32; 8];
        for (axis, value) in force.iter().enumerate() {
            words[4 + axis] = value.to_bits();
        }
        let words = BTreeMap::from([(ResourceTypeId::new("org.x::resource/outputs"), words)]);
        read_stage_outputs(2, &block(), &words)
    }

    #[test]
    fn an_output_raises_its_event_once_each_time_it_rises_past_the_threshold() {
        assert!(block().validate().is_ok());
        let values = read([6.0, 8.0, 0.0]);
        assert_eq!(values[0].value, [6.0, 8.0, 0.0]);
        assert_eq!(values[0].magnitude(), 10.0);
        let mut tracker = OutputEventTracker::default();
        assert!(
            tracker.observe(&values, 7).is_empty(),
            "at the threshold: none"
        );
        let events = tracker.observe(&read([0.0, 12.0, 5.0]), 7);
        assert!(events.iter().all(|event| event.tick == 7));
        assert_eq!(events.len(), 1);
        assert_eq!(
            (
                events[0].kind.as_str(),
                events[0].origin,
                events[0].magnitude
            ),
            ("impact", EventOrigin::Stage(2), 13.0)
        );
        assert!(
            tracker.observe(&read([0.0, 20.0, 0.0]), 7).is_empty(),
            "still pushing"
        );
        assert!(tracker.observe(&read([0.0; 3]), 7).is_empty());
        assert_eq!(
            tracker.observe(&read([30.0, 0.0, 0.0]), 7).len(),
            1,
            "a new impact"
        );
        tracker.reset();
        assert_eq!(tracker.observe(&read([30.0, 0.0, 0.0]), 7).len(), 1);
    }

    #[test]
    fn an_output_that_does_not_fit_a_persistent_resource_is_invalid() {
        let mut bad = block();
        bad.outputs[0].word = 7;
        assert!(bad.validate().is_err());
        let mut transient = block();
        transient.resources[0].lifetime = ResourceLifetime::Transient;
        assert!(transient.validate().is_err());
        let mut negative = block();
        negative.outputs[0].event.as_mut().unwrap().threshold = -1.0;
        assert!(negative.validate().is_err());
    }

    #[test]
    fn timeline_cues_become_outputs_at_their_crossing() {
        let cue = |payload| DispatchedChoreographyEvent {
            source: ChoreographyEventId::new(),
            name: "Launch".into(),
            time: 0.5,
            payload,
        };
        let notify = EffectOutputEvent::from_cue(
            &cue(ChoreographyEventPayload::GameplayNotify {
                topic: "whoosh".into(),
            }),
            30,
        );
        assert_eq!((notify.kind.as_str(), notify.tick), ("whoosh", 30));
        assert!(matches!(notify.origin, EventOrigin::Timeline(_)));
        let sound = EffectOutputEvent::from_cue(
            &cue(ChoreographyEventPayload::PlaySound { cue: "boom".into() }),
            30,
        );
        assert_eq!(sound.kind, CUE_PLAY_SOUND);
        assert_eq!(sound.text.as_deref(), Some("boom"));
        let shake = EffectOutputEvent::from_cue(
            &cue(ChoreographyEventPayload::CameraShake { intensity: 2.0 }),
            30,
        );
        assert_eq!((shake.value.as_slice(), shake.magnitude), (&[2.0][..], 2.0));

        // Continuous playback crosses a cue once per cycle: the latest crossing.
        assert_eq!(cue_crossing_tick(0.5, 0.52, 2.0, false), trace_tick(0.5));
        assert_eq!(cue_crossing_tick(0.5, 4.52, 2.0, true), trace_tick(4.5));
        assert_eq!(cue_crossing_tick(0.5, 0.52, 2.0, true), trace_tick(0.5));
    }
}
