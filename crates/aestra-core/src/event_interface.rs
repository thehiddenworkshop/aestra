//! Declared event inputs and outputs (event system E1): the events an effect promises to accept
//! from its host and to raise for it, each with a typed payload. Part of the effect's public
//! interface, next to its parameters and bindings, and just as engine-neutral: a payload field that
//! refers to a host object names one of the effect's bindings, never an engine entity.
//!
//! A declaration is a promise. The routes that raise an output or react to an input come later
//! (event system E3); until then a declared output nothing raises is listed as such, not rejected.

use crate::diagnostic::{Diagnostic, DiagnosticCode, ValidationReport};
use crate::model::register_id;
use crate::{EventDefinitionId, EventFieldId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Event names every effect already has, raised or accepted without being declared: the runtime's
/// output events, timeline cues and built-in inputs. A declaration cannot reuse one.
pub const RESERVED_EVENT_NAMES: &[&str] = &[
    "impact",
    "target_lost",
    "target_acquired",
    "finished",
    "play_sound",
    "camera_shake",
    "spawn_child_effect",
    "restart",
    "stop_emitting",
    "kill",
];

/// Whether an event travels from the host into the effect, or from the effect out to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EventDirection {
    Input,
    Output,
}

/// The type of one payload field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EventFieldType {
    Bool,
    Int,
    Float,
    Vec2,
    Vec3,
    Vec4,
    Color,
    /// One of the effect's bindings: the host resolves it to its own object (event system §12D).
    Binding,
}

impl EventFieldType {
    pub const ALL: [Self; 8] = [
        Self::Bool,
        Self::Int,
        Self::Float,
        Self::Vec2,
        Self::Vec3,
        Self::Vec4,
        Self::Color,
        Self::Binding,
    ];
}

/// One payload value, as a host sends it (event system E2).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EventValue {
    Bool(bool),
    Int(i32),
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Color([f32; 4]),
    /// One of the effect's bindings (event system §12D).
    Binding(crate::BindingId),
}

impl EventValue {
    pub fn field_type(&self) -> EventFieldType {
        match self {
            Self::Bool(_) => EventFieldType::Bool,
            Self::Int(_) => EventFieldType::Int,
            Self::Float(_) => EventFieldType::Float,
            Self::Vec2(_) => EventFieldType::Vec2,
            Self::Vec3(_) => EventFieldType::Vec3,
            Self::Vec4(_) => EventFieldType::Vec4,
            Self::Color(_) => EventFieldType::Color,
            Self::Binding(_) => EventFieldType::Binding,
        }
    }

    /// The neutral value of a type: false, zero, opaque white. `None` for a binding, which has no
    /// neutral object.
    pub fn neutral(field_type: EventFieldType) -> Option<Self> {
        Some(match field_type {
            EventFieldType::Bool => Self::Bool(false),
            EventFieldType::Int => Self::Int(0),
            EventFieldType::Float => Self::Float(0.0),
            EventFieldType::Vec2 => Self::Vec2([0.0; 2]),
            EventFieldType::Vec3 => Self::Vec3([0.0; 3]),
            EventFieldType::Vec4 => Self::Vec4([0.0; 4]),
            EventFieldType::Color => Self::Color([1.0; 4]),
            EventFieldType::Binding => return None,
        })
    }
}

/// One named, typed field of an event's payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventField {
    pub id: EventFieldId,
    pub name: String,
    pub field_type: EventFieldType,
    /// Whether the event always carries it; an optional field may be absent.
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

impl EventField {
    /// A required field.
    pub fn new(name: impl Into<String>, field_type: EventFieldType) -> Self {
        Self {
            id: EventFieldId::new(),
            name: name.into(),
            field_type,
            required: true,
        }
    }
}

/// A declared event input or output, e.g. `Detonate(position: vec3, power: float)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventDefinition {
    pub id: EventDefinitionId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<EventField>,
}

impl EventDefinition {
    /// An event without payload.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: EventDefinitionId::new(),
            name: name.into(),
            fields: Vec::new(),
        }
    }

    pub fn with_field(mut self, field: EventField) -> Self {
        self.fields.push(field);
        self
    }
}

/// Checks a name: not empty, no surrounding whitespace, `unique` among its siblings.
fn validate_name(
    report: &mut ValidationReport,
    path: &str,
    name: &str,
    what: &str,
    first: Option<usize>,
) {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        report.push(Diagnostic::error(
            DiagnosticCode::InvalidValue,
            format!("{path}.name"),
            format!("{what} name cannot be empty"),
        ));
    } else if trimmed != name {
        report.push(Diagnostic::error(
            DiagnosticCode::InvalidValue,
            format!("{path}.name"),
            format!("{what} name cannot have surrounding whitespace"),
        ));
    } else if let Some(first) = first {
        report.push(Diagnostic::error(
            DiagnosticCode::DuplicateId,
            format!("{path}.name"),
            format!("{what} name '{name}' is already used by entry {first}"),
        ));
    }
}

/// Structural validation of an effect's declared events: stable ids, names unique within their
/// direction and never an intrinsic event's, field names unique within their event.
pub(crate) fn validate_event_definitions(
    inputs: &[EventDefinition],
    outputs: &[EventDefinition],
    report: &mut ValidationReport,
    semantic_ids: &mut BTreeMap<u128, String>,
) {
    for (collection, definitions) in [("event_inputs", inputs), ("event_outputs", outputs)] {
        let mut names: BTreeMap<&str, usize> = BTreeMap::new();
        for (index, definition) in definitions.iter().enumerate() {
            let path = format!("effect.{collection}[{index}]");
            register_id(
                report,
                semantic_ids,
                definition.id.as_uuid().as_u128(),
                format!("{path}.id"),
            );
            let first = names.insert(definition.name.as_str(), index);
            validate_name(report, &path, &definition.name, "event", first);
            if RESERVED_EVENT_NAMES.contains(&definition.name.as_str()) {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidValue,
                    format!("{path}.name"),
                    format!(
                        "'{}' is a built-in event every effect already has",
                        definition.name
                    ),
                ));
            }
            let mut field_names: BTreeMap<&str, usize> = BTreeMap::new();
            for (field_index, field) in definition.fields.iter().enumerate() {
                let field_path = format!("{path}.fields[{field_index}]");
                register_id(
                    report,
                    semantic_ids,
                    field.id.as_uuid().as_u128(),
                    format!("{field_path}.id"),
                );
                let first = field_names.insert(field.name.as_str(), field_index);
                validate_name(report, &field_path, &field.name, "event field", first);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EffectAsset;

    fn detonate() -> EventDefinition {
        EventDefinition::new("Detonate")
            .with_field(EventField::new("position", EventFieldType::Vec3))
            .with_field(EventField::new("power", EventFieldType::Float))
    }

    fn codes_at(effect: &EffectAsset, path_prefix: &str) -> Vec<DiagnosticCode> {
        effect
            .validation_report()
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.path.starts_with(path_prefix))
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    #[test]
    fn declared_events_validate_and_round_trip() {
        let mut effect = EffectAsset::new("Fireball", 2.0);
        effect.event_inputs = vec![detonate()];
        let mut hit = EventDefinition::new("Hit")
            .with_field(EventField::new("target", EventFieldType::Binding));
        hit.fields[0].required = false;
        effect.event_outputs = vec![hit, EventDefinition::new("Detonate")];
        assert!(codes_at(&effect, "effect.event_").is_empty());

        let saved = effect.to_pretty_ron().unwrap();
        let loaded = EffectAsset::from_ron(&saved).unwrap();
        assert_eq!(loaded.event_inputs, effect.event_inputs);
        assert_eq!(loaded.event_outputs, effect.event_outputs);

        // An effect without declarations saves none.
        let plain = EffectAsset::new("Plain", 1.0).to_pretty_ron().unwrap();
        assert!(!plain.contains("event_inputs") && !plain.contains("event_outputs"));
    }

    #[test]
    fn names_are_unique_trimmed_and_never_built_in() {
        let mut effect = EffectAsset::new("Fireball", 2.0);
        let mut twice = detonate();
        twice.fields[1].name = "position".into();
        effect.event_inputs = vec![
            detonate(),
            EventDefinition::new("Detonate"),
            EventDefinition::new(" Cast"),
            EventDefinition::new("finished"),
            twice,
        ];
        let codes = codes_at(&effect, "effect.event_inputs");
        assert_eq!(
            codes,
            [
                DiagnosticCode::DuplicateId,
                DiagnosticCode::InvalidValue,
                DiagnosticCode::InvalidValue,
                DiagnosticCode::DuplicateId,
                DiagnosticCode::DuplicateId,
            ],
            "{:?}",
            effect.validation_report()
        );
    }
}
