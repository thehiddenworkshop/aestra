//! Host input events (event system E2): the events a host sends an effect — a declared input such as
//! `Detonate(position)`, or the built-in `restart`. Each is checked against the effect's interface,
//! stamped with the fixed tick it takes effect at (the next one), and recorded, so a backward seek
//! replays exactly what the host sent (event system §12C): the received events are host input, like
//! a binding trace.
//!
//! What a declared input *does* — spawn a burst, raise an output — comes with event routes (E3);
//! until then an instance records its inputs and reports which ones each tick crosses.

use crate::{EffectInstance, INPUT_RESTART, trace_tick};
use aestra_core::{EventFieldType, EventValue};
use std::ops::Range;

/// An input event an instance received: which input, the tick it takes effect at, and its payload
/// by field name.
#[derive(Debug, Clone, PartialEq)]
pub struct HostInputEvent {
    pub input: String,
    pub tick: u64,
    pub payload: Vec<(String, EventValue)>,
}

/// Why an input event was refused. The instance is left unchanged.
#[derive(Debug, Clone, PartialEq)]
pub enum EventInputError {
    /// The effect declares no input of that name.
    UnknownInput(String),
    /// The payload names a field the input does not declare.
    UnknownField { input: String, field: String },
    /// A required field is missing.
    MissingField { input: String, field: String },
    /// A field carries a value of another type, or appears twice.
    InvalidField { input: String, field: String },
    /// A binding field names no binding of the effect.
    UnknownBinding { input: String, field: String },
}

impl std::fmt::Display for EventInputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownInput(input) => write!(formatter, "the effect has no input '{input}'"),
            Self::UnknownField { input, field } => {
                write!(formatter, "input '{input}' has no field '{field}'")
            }
            Self::MissingField { input, field } => {
                write!(formatter, "input '{input}' needs field '{field}'")
            }
            Self::InvalidField { input, field } => write!(
                formatter,
                "field '{field}' of input '{input}' has the wrong type or appears twice"
            ),
            Self::UnknownBinding { input, field } => write!(
                formatter,
                "field '{field}' of input '{input}' names no binding of the effect"
            ),
        }
    }
}

impl std::error::Error for EventInputError {}

/// A content hash of received events, part of the instance's host input identity.
pub(crate) fn events_identity(events: &[HostInputEvent]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut mix = |word: u64| hash = (hash ^ word).wrapping_mul(0x0000_0100_0000_01b3);
    for event in events {
        mix(event.tick);
        event.input.bytes().for_each(|byte| mix(u64::from(byte)));
        for (field, value) in &event.payload {
            field.bytes().for_each(|byte| mix(u64::from(byte)));
            let words: Vec<u64> = match value {
                EventValue::Bool(value) => vec![u64::from(*value)],
                EventValue::Int(value) => vec![*value as u64],
                EventValue::Float(value) => vec![u64::from(value.to_bits())],
                EventValue::Vec2(values) => values.iter().map(|v| u64::from(v.to_bits())).collect(),
                EventValue::Vec3(values) => values.iter().map(|v| u64::from(v.to_bits())).collect(),
                EventValue::Vec4(values) | EventValue::Color(values) => {
                    values.iter().map(|v| u64::from(v.to_bits())).collect()
                }
                EventValue::Binding(id) => {
                    let bits = id.as_uuid().as_u128();
                    vec![bits as u64, (bits >> 64) as u64]
                }
            };
            words.into_iter().for_each(&mut mix);
        }
    }
    hash
}

impl EffectInstance {
    /// Receives an input event (event system E2). A declared input is checked against its payload
    /// schema, stamped with the next fixed tick and recorded; sending at an earlier tick than events
    /// already recorded (after a backward seek) replaces that future, so the record is always one
    /// history. `restart` restarts playback at once and is not recorded. Returns the tick the event
    /// takes effect at.
    pub fn send_event(
        &mut self,
        input: &str,
        payload: Vec<(String, EventValue)>,
    ) -> Result<u64, EventInputError> {
        if input == INPUT_RESTART {
            self.restart();
            return Ok(0);
        }
        let definition = self
            .effect
            .event_inputs
            .iter()
            .find(|definition| definition.name == input)
            .ok_or_else(|| EventInputError::UnknownInput(input.to_string()))?;
        let error = |make: fn(String, String) -> EventInputError, field: &str| {
            make(input.to_string(), field.to_string())
        };
        for (index, (name, value)) in payload.iter().enumerate() {
            let Some(field) = definition.fields.iter().find(|field| &field.name == name) else {
                return Err(error(
                    |input, field| EventInputError::UnknownField { input, field },
                    name,
                ));
            };
            let repeated = payload[..index].iter().any(|(earlier, _)| earlier == name);
            if value.field_type() != field.field_type || repeated {
                return Err(error(
                    |input, field| EventInputError::InvalidField { input, field },
                    name,
                ));
            }
            if let EventValue::Binding(binding) = value
                && !self.effect.binding_slots.contains_key(binding)
            {
                return Err(error(
                    |input, field| EventInputError::UnknownBinding { input, field },
                    name,
                ));
            }
        }
        if let Some(missing) = definition
            .fields
            .iter()
            .find(|field| field.required && !payload.iter().any(|(name, _)| name == &field.name))
        {
            return Err(error(
                |input, field| EventInputError::MissingField { input, field },
                &missing.name,
            ));
        }
        let tick = trace_tick(self.time) + 1;
        self.input_events.retain(|event| event.tick <= tick);
        self.input_events.push(HostInputEvent {
            input: input.to_string(),
            tick,
            payload,
        });
        Ok(tick)
    }

    /// Every input event received, in tick order.
    pub fn received_events(&self) -> &[HostInputEvent] {
        &self.input_events
    }

    /// The received events taking effect in `ticks`.
    pub fn events_in_ticks(&self, ticks: Range<u64>) -> impl Iterator<Item = &HostInputEvent> {
        self.input_events
            .iter()
            .filter(move |event| ticks.contains(&event.tick))
    }

    /// Replaces the received events with a recorded history — an editor's preview inputs, a replay.
    /// Events are kept in tick order; a different history invalidates checkpoints taken under the
    /// old one.
    pub fn set_received_events(&mut self, mut events: Vec<HostInputEvent>) {
        events.sort_by_key(|event| event.tick);
        if self.input_events != events {
            self.input_events = events;
            self.invalidate_history();
        }
    }

    /// Forgets every received event: a new play of the effect.
    pub fn clear_received_events(&mut self) {
        self.set_received_events(Vec::new());
    }
}

/// The neutral payload of a declared input: every field at its neutral value, a binding field at the
/// first binding the effect declares (left out when it has none).
pub fn neutral_payload(
    effect: &crate::CompiledEffect,
    input: &str,
) -> Option<Vec<(String, EventValue)>> {
    let definition = effect
        .event_inputs
        .iter()
        .find(|definition| definition.name == input)?;
    Some(
        definition
            .fields
            .iter()
            .filter_map(|field| {
                let value = match field.field_type {
                    EventFieldType::Binding => effect
                        .bindings
                        .first()
                        .map(|binding| EventValue::Binding(binding.source))?,
                    other => EventValue::neutral(other)?,
                };
                Some((field.name.clone(), value))
            })
            .collect(),
    )
}
