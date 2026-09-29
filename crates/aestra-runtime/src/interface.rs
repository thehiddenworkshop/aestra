//! An effect's public interface (host bindings HB11): what a game sets, binds, hears and supplies to
//! play it — in the words of the effect, not of any engine. Tools show it (the editor's Interface
//! section); hosts can check against it. Derived from the compiled effect, so it always says what the
//! effect actually reads.

use crate::{
    CompiledEffect, EVENT_FINISHED, EVENT_IMPACT, EVENT_TARGET_ACQUIRED, EVENT_TARGET_LOST,
};
use aestra_core::{
    BindingUpdateMode, CollisionInputSource, EffectPlaybackMode, HostInputAvailability, ValueType,
};

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

/// A runtime event the effect raises for the game to hear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceEvent {
    /// The event's kind, e.g. `impact`.
    pub kind: String,
    /// What raises it: an emitter's or a stage's name, or the effect's.
    pub raised_by: String,
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
    /// Events the game can send the effect. None exists yet beyond playback control (restart, seek).
    pub input_events: Vec<String>,
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
        let mut raise = |kind: &str, raised_by: &str| {
            let event = InterfaceEvent {
                kind: kind.to_string(),
                raised_by: raised_by.to_string(),
            };
            if !output_events.contains(&event) {
                output_events.push(event);
            }
        };
        for emitter in self.emitters.iter().filter(|emitter| emitter.enabled) {
            if emitter.homing.is_some() {
                raise(EVENT_IMPACT, &emitter.name);
                let bound = emitter
                    .homing
                    .as_ref()
                    .is_some_and(|homing| homing.target_source.is_some());
                if bound {
                    raise(EVENT_TARGET_LOST, &emitter.name);
                    raise(EVENT_TARGET_ACQUIRED, &emitter.name);
                }
            }
        }
        for stage in self.all_extension_stages() {
            for output in &stage.block.outputs {
                if let Some(event) = &output.event {
                    raise(&event.kind, &stage.name);
                }
            }
        }
        if self.playback_mode == EffectPlaybackMode::Once {
            raise(EVENT_FINISHED, &self.name);
        }

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
            input_events: Vec::new(),
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
