//! Host input events for Bevy (event system E2): gameplay sends an effect one of its declared inputs
//! with an [`AestraEventInput`] message, or directly with [`EffectPlayer::send_event`]. The effect
//! checks it against its interface, stamps it with the tick it takes effect at and records it, so a
//! seek replays exactly what gameplay sent.

use crate::EffectPlayer;
use aestra_core::EventValue;
use aestra_runtime::EventInputError;
use bevy::prelude::*;

/// Sends `input` to the effect on entity `effect`, with its payload by field name. A payload field
/// typed `Binding` names one of the effect's bindings (`EventValue::Binding`).
#[derive(Message, Debug, Clone)]
pub struct AestraEventInput {
    pub effect: Entity,
    pub input: String,
    pub payload: Vec<(String, EventValue)>,
}

impl AestraEventInput {
    /// An input without payload.
    pub fn new(effect: Entity, input: impl Into<String>) -> Self {
        Self {
            effect,
            input: input.into(),
            payload: Vec::new(),
        }
    }

    pub fn with(mut self, field: impl Into<String>, value: EventValue) -> Self {
        self.payload.push((field.into(), value));
        self
    }
}

impl EffectPlayer {
    /// Sends an input event to this effect (event system E2); see
    /// [`aestra_runtime::EffectInstance::send_event`]. Returns the tick it takes effect at.
    pub fn send_event(
        &mut self,
        input: &str,
        payload: Vec<(String, EventValue)>,
    ) -> Result<u64, EventInputError> {
        self.instance_mut().send_event(input, payload)
    }
}

/// Delivers this frame's [`AestraEventInput`]s, before playback advances.
pub(crate) fn apply_event_inputs(
    mut inputs: MessageReader<AestraEventInput>,
    mut players: Query<&mut EffectPlayer>,
) {
    for input in inputs.read() {
        let Ok(mut player) = players.get_mut(input.effect) else {
            warn!(
                "aestra: input '{}' sent to {}, which plays no effect",
                input.input, input.effect
            );
            continue;
        };
        if let Err(error) = player.send_event(&input.input, input.payload.clone()) {
            warn!("aestra: {} refused an input event: {error}", input.effect);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_core::{EffectAsset, Emitter, EventDefinition, EventField, EventFieldType};

    fn player() -> EffectPlayer {
        let mut effect = EffectAsset::new("Mine", 4.0);
        effect.emitters.push(Emitter::basic_sprite("Sparks", 4.0));
        effect.event_inputs = vec![
            EventDefinition::new("Detonate")
                .with_field(EventField::new("position", EventFieldType::Vec3)),
        ];
        let compiled = aestra_compiler::EffectCompiler::with_extensions(
            aestra_compiler::ExtensionRegistry::builtin(),
        )
        .compile(&effect)
        .unwrap();
        EffectPlayer::from_compiled(compiled.into())
    }

    #[test]
    fn gameplay_sends_inputs_by_message() {
        let mut app = App::new();
        app.add_message::<AestraEventInput>()
            .add_systems(Update, apply_event_inputs);
        let effect = app.world_mut().spawn(player()).id();
        app.world_mut().write_message(
            AestraEventInput::new(effect, "Detonate")
                .with("position", EventValue::Vec3([0.0, 1.0, 0.0])),
        );
        // Refused: a missing field. The effect is unchanged.
        app.world_mut()
            .write_message(AestraEventInput::new(effect, "Detonate"));
        app.update();
        let received = app
            .world()
            .get::<EffectPlayer>(effect)
            .unwrap()
            .instance()
            .received_events()
            .to_vec();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].input, "Detonate");
        assert_eq!(received[0].tick, 1);
    }
}
