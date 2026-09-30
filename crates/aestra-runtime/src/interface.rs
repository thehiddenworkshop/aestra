//! An effect's public interface (host bindings HB11): what a game sets, binds, hears and supplies to
//! play it — in the words of the effect, not of any engine. Tools show it (the editor's Interface
//! section); hosts can check against it. Derived from the compiled effect, so it always says what the
//! effect actually reads.

use crate::{
    CompiledEffect, EVENT_FINISHED, EVENT_IMPACT, EVENT_TARGET_ACQUIRED, EVENT_TARGET_LOST,
};
use aestra_core::{
    BindingUpdateMode, ChoreographyEventPayload, CollisionInputSource, EffectPlaybackMode,
    EventDefinition, EventFieldType, HostInputAvailability, ValueType,
};

/// The input every effect accepts: play again from the start (event system E0.5).
pub const INPUT_RESTART: &str = "restart";
/// The input stopping an effect's emission; live particles finish their lives (event system E2b).
pub const INPUT_STOP_EMITTING: &str = "stop_emitting";
/// The input retiring every particle at once, without death events; emission stops too (E2b).
pub const INPUT_KILL: &str = "kill";
/// A timeline cue asking the host to play a sound (`ChoreographyEventPayload::PlaySound`).
pub const CUE_PLAY_SOUND: &str = "play_sound";
/// A timeline cue asking the host to shake the camera (`ChoreographyEventPayload::CameraShake`).
pub const CUE_CAMERA_SHAKE: &str = "camera_shake";
/// A timeline cue asking the host to spawn another effect (`ChoreographyEventPayload::SpawnChildEffect`).
pub const CUE_SPAWN_CHILD_EFFECT: &str = "spawn_child_effect";

/// An effect parameter a game may set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceParameter {
    pub name: String,
    pub value_type: ValueType,
}

/// One field a binding reads from its object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceField {
    /// Its id, e.g. `aestra.field.linear_velocity`.
    pub id: String,
    /// A readable name, e.g. `Linear Velocity` (see [`field_label`]).
    pub label: String,
    /// Whether the object must supply it; an optional field falls back when absent.
    pub required: bool,
}

/// An object slot a game fills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceBinding {
    pub name: String,
    /// Its kind's id, e.g. `aestra.binding.spatial`.
    pub kind: String,
    /// A readable kind name, e.g. `Spatial`.
    pub kind_label: String,
    pub fields: Vec<InterfaceField>,
    pub update_mode: BindingUpdateMode,
    /// Whether the effect needs it bound to play as authored.
    pub required: bool,
    /// Whether anything in the effect reads it.
    pub read: bool,
}

/// How the host hears an output event (event system E0.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventChannel {
    /// Raised by the simulation: an `EffectOutputEvent`.
    Runtime,
    /// A cue authored on the timeline, delivered as playback crosses it.
    Timeline,
    /// Declared on the effect's interface (event system E1); routes raise it (E3).
    Declared,
}

/// One field of a declared event's payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceEventField {
    pub name: String,
    pub field_type: EventFieldType,
    pub required: bool,
}

fn payload(definition: &EventDefinition) -> Vec<InterfaceEventField> {
    definition
        .fields
        .iter()
        .map(|field| InterfaceEventField {
            name: field.name.clone(),
            field_type: field.field_type,
            required: field.required,
        })
        .collect()
}

/// An event the game can send the effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceInput {
    pub name: String,
    /// Its payload, when declared (event system E1); built-in inputs carry none.
    pub fields: Vec<InterfaceEventField>,
    /// Whether every effect has it (`restart`), rather than this effect declaring it.
    pub built_in: bool,
}

/// An event the effect raises for the game to hear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceEvent {
    /// The event's kind, e.g. `impact`, or a timeline cue's topic.
    pub kind: String,
    /// What raises it: an emitter's or a stage's name, the effect's, or a timeline cue's. Empty for a
    /// declared output nothing raises yet.
    pub raised_by: String,
    pub channel: EventChannel,
    /// Its payload, when declared (event system E1).
    pub fields: Vec<InterfaceEventField>,
}

/// Something from the game's world the effect collides with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceWorldRequirement {
    pub source: CollisionInputSource,
    /// A readable name, e.g. `Physics Query`.
    pub label: String,
    /// How its past can be recovered (a seek's exactness).
    pub availability: HostInputAvailability,
}

/// An effect's public interface: its parameters, bindings, events and world requirements.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectInterface {
    pub parameters: Vec<InterfaceParameter>,
    pub bindings: Vec<InterfaceBinding>,
    /// Events the game can send the effect: the built-in [`INPUT_RESTART`], then declared ones.
    pub input_events: Vec<InterfaceInput>,
    pub output_events: Vec<InterfaceEvent>,
    pub world: Vec<InterfaceWorldRequirement>,
}

/// A readable name for a namespaced id: its last segment, words capitalized —
/// `aestra.field.linear_velocity` reads `Linear Velocity`.
pub fn field_label(id: &str) -> String {
    let last = id.rsplit(['.', ':', '/']).next().unwrap_or(id);
    last.split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A readable name for a collision input source.
pub fn world_label(source: CollisionInputSource) -> &'static str {
    match source {
        CollisionInputSource::AuthoredColliders => "Authored Colliders",
        CollisionInputSource::SignedDistanceField => "World Geometry (SDF)",
        CollisionInputSource::DepthBuffer => "Depth Buffer",
        CollisionInputSource::EnginePhysicsQuery => "Physics Query",
        CollisionInputSource::MeshAccelerationStructure => "Mesh Acceleration Structure",
    }
}

impl CompiledEffect {
    /// This effect's public interface (host bindings HB11).
    pub fn interface(&self) -> EffectInterface {
        let parameters = self
            .parameters
            .iter()
            .map(|parameter| InterfaceParameter {
                name: parameter.name.clone(),
                value_type: parameter.value_type,
            })
            .collect();
        let read_slots = self.read_binding_slots();
        let bindings = self
            .bindings
            .iter()
            .enumerate()
            .map(|(slot, binding)| InterfaceBinding {
                name: binding.name.clone(),
                kind: binding.kind.as_str().to_string(),
                kind_label: field_label(binding.kind.as_str()),
                fields: binding
                    .layout
                    .fields
                    .iter()
                    .map(|field| InterfaceField {
                        id: field.field.as_str().to_string(),
                        label: field_label(field.field.as_str()),
                        required: binding.required_fields.contains(&field.field),
                    })
                    .collect(),
                update_mode: binding.update_mode,
                required: binding.required,
                read: read_slots.contains(&slot),
            })
            .collect();

        let mut output_events = Vec::new();
        let mut raise = |kind: &str, raised_by: &str, channel: EventChannel| {
            let event = InterfaceEvent {
                kind: kind.to_string(),
                raised_by: raised_by.to_string(),
                channel,
                fields: Vec::new(),
            };
            if !output_events.contains(&event) {
                output_events.push(event);
            }
        };
        for emitter in self.emitters.iter().filter(|emitter| emitter.enabled) {
            if emitter.homing.is_some() {
                raise(EVENT_IMPACT, &emitter.name, EventChannel::Runtime);
                let bound = emitter
                    .homing
                    .as_ref()
                    .is_some_and(|homing| homing.target_source.is_some());
                if bound {
                    raise(EVENT_TARGET_LOST, &emitter.name, EventChannel::Runtime);
                    raise(EVENT_TARGET_ACQUIRED, &emitter.name, EventChannel::Runtime);
                }
            }
        }
        for stage in self.all_extension_stages() {
            for output in &stage.block.outputs {
                if let Some(event) = &output.event {
                    raise(&event.kind, &stage.name, EventChannel::Runtime);
                }
            }
        }
        if self.playback_mode == EffectPlaybackMode::Once {
            raise(EVENT_FINISHED, &self.name, EventChannel::Runtime);
        }
        // Timeline cues reach the host too (event system E0.5): a notification by its topic.
        for cue in &self.choreography_events {
            let kind = match &cue.payload {
                ChoreographyEventPayload::GameplayNotify { topic } if !topic.trim().is_empty() => {
                    topic.as_str()
                }
                ChoreographyEventPayload::GameplayNotify { .. } => cue.name.as_str(),
                ChoreographyEventPayload::PlaySound { .. } => CUE_PLAY_SOUND,
                ChoreographyEventPayload::CameraShake { .. } => CUE_CAMERA_SHAKE,
                ChoreographyEventPayload::SpawnChildEffect { .. } => CUE_SPAWN_CHILD_EFFECT,
            };
            raise(kind, &cue.name, EventChannel::Timeline);
        }
        // Declared outputs (event system E1): listed even before anything raises them.
        for definition in &self.event_outputs {
            output_events.push(InterfaceEvent {
                kind: definition.name.clone(),
                raised_by: String::new(),
                channel: EventChannel::Declared,
                fields: payload(definition),
            });
        }
        let mut input_events: Vec<InterfaceInput> =
            [INPUT_RESTART, INPUT_STOP_EMITTING, INPUT_KILL]
                .into_iter()
                .map(|name| InterfaceInput {
                    name: name.to_string(),
                    fields: Vec::new(),
                    built_in: true,
                })
                .collect();
        input_events.extend(self.event_inputs.iter().map(|definition| InterfaceInput {
            name: definition.name.clone(),
            fields: payload(definition),
            built_in: false,
        }));

        let collision = self.collision_inputs();
        let mut world: Vec<InterfaceWorldRequirement> = collision
            .sources()
            .iter()
            .filter(|source| **source != CollisionInputSource::AuthoredColliders)
            .map(|source| InterfaceWorldRequirement {
                source: *source,
                label: world_label(*source).to_string(),
                availability: if *source == CollisionInputSource::EnginePhysicsQuery {
                    HostInputAvailability::ForwardOnly
                } else {
                    HostInputAvailability::TimeAddressable
                },
            })
            .collect();
        // Stages colliding with the world SDF (fluid World Colliders) need it too.
        let stages_read_world = self.all_extension_stages().any(|stage| {
            stage
                .block
                .resources
                .iter()
                .any(|resource| resource.id.as_str() == crate::AESTRA_RESOURCE_WORLD_SDF)
        });
        if stages_read_world
            && !world
                .iter()
                .any(|requirement| requirement.source == CollisionInputSource::SignedDistanceField)
        {
            world.push(InterfaceWorldRequirement {
                source: CollisionInputSource::SignedDistanceField,
                label: world_label(CollisionInputSource::SignedDistanceField).to_string(),
                availability: HostInputAvailability::TimeAddressable,
            });
        }

        EffectInterface {
            parameters,
            bindings,
            input_events,
            output_events,
            world,
        }
    }

    /// The binding slots something reads: module inputs, plugin modules, homing targets and
    /// emitter attachments.
    fn read_binding_slots(&self) -> Vec<usize> {
        let mut slots: Vec<usize> = self
            .host_fields
            .iter()
            .map(|field| field.source.binding.0)
            .collect();
        for emitter in &self.emitters {
            if let Some(homing) = &emitter.homing {
                slots.extend(
                    homing
                        .target_source
                        .iter()
                        .chain(&homing.velocity_source)
                        .map(|source| source.binding.0),
                );
            }
            if let Some(attachment) = &emitter.attachment {
                slots.push(attachment.position.binding.0);
            }
            for stage in &emitter.extension_stages {
                for module in &stage.modules {
                    slots.extend(module.host_fields.values().map(|field| field.binding.0));
                }
            }
        }
        for stage in &self.extension_stages {
            for module in &stage.modules {
                slots.extend(module.host_fields.values().map(|field| field.binding.0));
            }
        }
        slots.sort_unstable();
        slots.dedup();
        slots
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_read_as_words() {
        assert_eq!(
            field_label("aestra.field.linear_velocity"),
            "Linear Velocity"
        );
        assert_eq!(field_label("aestra.binding.spatial"), "Spatial");
        assert_eq!(field_label("org.example::resource/heat_map"), "Heat Map");
    }
}
