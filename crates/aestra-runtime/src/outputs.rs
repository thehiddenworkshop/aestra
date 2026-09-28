//! Stage outputs on the host side (fluid F11, host bindings HB9): the values a stage reports
//! ([`StageOutput`]) read out of the words the host read back, and the runtime events they raise.
//! Engine-neutral: a host adapter (Bevy, say) reads the output resources back after each frame's
//! ticks, hands the words here, and maps the events onto its own event model.

use crate::{ExecutionBlock, OutputEvent, StageOutput};
use aestra_core::{ModuleId, ResourceTypeId};
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

/// A runtime event an output raised: its magnitude rose past its event's threshold.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectOutputEvent {
    /// The event's kind, e.g. `impact`.
    pub kind: String,
    /// The output that raised it, e.g. `force`.
    pub output: String,
    pub source: Option<ModuleId>,
    pub stage: usize,
    pub value: Vec<f32>,
    pub magnitude: f32,
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
    /// Observes one read of outputs; returns the events it raises.
    pub fn observe(&mut self, values: &[StageOutputValue]) -> Vec<EffectOutputEvent> {
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
                        stage: value.stage,
                        value: value.value.clone(),
                        magnitude,
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
            tracker.observe(&values).is_empty(),
            "at the threshold: none"
        );
        let events = tracker.observe(&read([0.0, 12.0, 5.0]));
        assert_eq!(events.len(), 1);
        assert_eq!(
            (
                events[0].kind.as_str(),
                events[0].stage,
                events[0].magnitude
            ),
            ("impact", 2, 13.0)
        );
        assert!(
            tracker.observe(&read([0.0, 20.0, 0.0])).is_empty(),
            "still pushing"
        );
        assert!(tracker.observe(&read([0.0; 3])).is_empty());
        assert_eq!(
            tracker.observe(&read([30.0, 0.0, 0.0])).len(),
            1,
            "a new impact"
        );
        tracker.reset();
        assert_eq!(tracker.observe(&read([30.0, 0.0, 0.0])).len(), 1);
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
}
