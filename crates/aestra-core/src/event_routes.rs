//! Effect-scale event routes (event system E3): what a declared event input does inside the effect.
//! Two specialized routes come before any generic event model (roadmap §23, E3). The first is
//! [`InputSpawnRoute`]: a host input spawns a burst of an emitter's particles.

use crate::diagnostic::{Diagnostic, DiagnosticCode, ValidationReport};
use crate::model::register_id;
use crate::{
    EffectAsset, EmitterId, EventDefinitionId, EventFieldId, EventFieldType, EventRouteId,
    MAX_EVENT_LINK_COUNT,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A declared input spawning a burst (event system E3): each time the host sends `input`, `target`
/// spawns `count` particles at the input's `position` field, in effect space, or at the effect's
/// origin without one. They start with the target's own launch velocity and lifetime. Like an event
/// link's target, an emitter a route targets is a *sub-emitter*: it spawns only from its routes and
/// links, and simulates persistent particles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputSpawnRoute {
    pub id: EventRouteId,
    /// The declared input (in `EffectAsset::event_inputs`) that fires the route.
    pub input: EventDefinitionId,
    pub target: EmitterId,
    /// Particles spawned per input event, 1 to [`MAX_EVENT_LINK_COUNT`].
    #[serde(default = "default_route_count")]
    pub count: u32,
    /// The input's `vec3` field the burst is centered on; none spawns at the effect origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<EventFieldId>,
}

fn default_route_count() -> u32 {
    1
}

impl InputSpawnRoute {
    /// A route spawning one particle per event at the effect origin.
    pub fn new(input: EventDefinitionId, target: EmitterId) -> Self {
        Self {
            id: EventRouteId::new(),
            input,
            target,
            count: 1,
            position: None,
        }
    }
}

/// Structural validation of an effect's input spawn routes: stable ids, a count in range, an input
/// and an emitter the effect has, and a position field that is one of the input's `vec3` fields.
pub(crate) fn validate_input_spawns(
    effect: &EffectAsset,
    report: &mut ValidationReport,
    semantic_ids: &mut BTreeMap<u128, String>,
) {
    for (index, route) in effect.input_spawns.iter().enumerate() {
        let path = format!("effect.input_spawns[{index}]");
        register_id(
            report,
            semantic_ids,
            route.id.as_uuid().as_u128(),
            format!("{path}.id"),
        );
        if route.count == 0 || route.count > MAX_EVENT_LINK_COUNT {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidValue,
                format!("{path}.count"),
                format!("an input route spawns 1 to {MAX_EVENT_LINK_COUNT} particles per event"),
            ));
        }
        if !effect
            .emitters
            .iter()
            .any(|emitter| emitter.id == route.target)
        {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidReference,
                format!("{path}.target"),
                format!("input route references missing emitter {}", route.target),
            ));
        }
        let Some(input) = effect
            .event_inputs
            .iter()
            .find(|definition| definition.id == route.input)
        else {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidReference,
                format!("{path}.input"),
                format!("input route references missing event input {}", route.input),
            ));
            continue;
        };
        if let Some(position) = route.position
            && !input
                .fields
                .iter()
                .any(|field| field.id == position && field.field_type == EventFieldType::Vec3)
        {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidReference,
                format!("{path}.position"),
                format!("'{}' has no vec3 field {position}", input.name),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Emitter, EventDefinition, EventField};

    fn detonation() -> (EffectAsset, InputSpawnRoute) {
        let mut effect = EffectAsset::new("Mine", 2.0);
        let burst = Emitter::basic_sprite("Burst", 2.0);
        let detonate = EventDefinition::new("Detonate")
            .with_field(EventField::new("position", EventFieldType::Vec3))
            .with_field(EventField::new("power", EventFieldType::Float));
        let mut route = InputSpawnRoute::new(detonate.id, burst.id);
        route.count = 64;
        route.position = Some(detonate.fields[0].id);
        effect.emitters.push(burst);
        effect.event_inputs.push(detonate);
        effect.input_spawns.push(route.clone());
        (effect, route)
    }

    fn codes(effect: &EffectAsset) -> Vec<(String, DiagnosticCode)> {
        effect
            .validation_report()
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.path.starts_with("effect.input_spawns"))
            .map(|diagnostic| (diagnostic.path, diagnostic.code))
            .collect()
    }

    #[test]
    fn input_spawn_routes_validate_and_round_trip() {
        let (effect, route) = detonation();
        assert!(codes(&effect).is_empty(), "{:?}", codes(&effect));
        let saved = effect.to_pretty_ron().unwrap();
        let loaded = EffectAsset::from_ron(&saved).unwrap();
        assert_eq!(loaded.input_spawns, vec![route]);
        // An effect without routes saves none.
        let plain = EffectAsset::new("Plain", 1.0).to_pretty_ron().unwrap();
        assert!(!plain.contains("input_spawns"));
    }

    #[test]
    fn a_route_needs_its_input_emitter_and_a_vec3_position() {
        let (effect, _) = detonation();
        let mut bad = effect.clone();
        bad.input_spawns[0].count = 0;
        // The float field is no position.
        bad.input_spawns[0].position = Some(bad.event_inputs[0].fields[1].id);
        bad.input_spawns[0].target = EmitterId::new();
        assert_eq!(
            codes(&bad),
            [
                (
                    "effect.input_spawns[0].count".to_string(),
                    DiagnosticCode::InvalidValue
                ),
                (
                    "effect.input_spawns[0].target".to_string(),
                    DiagnosticCode::InvalidReference
                ),
                (
                    "effect.input_spawns[0].position".to_string(),
                    DiagnosticCode::InvalidReference
                ),
            ]
        );
        let mut orphan = effect;
        orphan.event_inputs.clear();
        assert_eq!(
            codes(&orphan),
            [(
                "effect.input_spawns[0].input".to_string(),
                DiagnosticCode::InvalidReference
            )]
        );
    }
}
