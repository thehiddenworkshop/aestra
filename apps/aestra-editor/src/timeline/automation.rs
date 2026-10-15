pub(super) use super::automation_data::AutomationLaneKeys;
use super::automation_data::bound_automation_parameter;
use super::{TimelineState, interaction_state::AutomationLaneId};
use crate::{
    EditorModuleRegistry, localization::Localizer, localized_properties_input, module_parameter,
};
#[cfg(test)]
use aestra_core::{CurveKey, ModuleParameters};
use aestra_core::{EffectAsset, Emitter, Value};
use bevy::prelude::Component;

#[derive(Component)]
pub(super) struct EmitterAutomationVisibilityMenuAnchor;

#[derive(Clone)]
pub(super) struct AutomationLaneProjection {
    pub(super) id: AutomationLaneId,
    pub(super) label: String,
    pub(super) keys: AutomationLaneKeys,
}

pub(super) fn emitter_automation_lanes(
    effect: &EffectAsset,
    emitter: &Emitter,
    registry: &EditorModuleRegistry,
    localizer: &Localizer,
) -> Vec<AutomationLaneProjection> {
    let mut lanes = Vec::new();
    for module in &emitter.modules {
        let Some(metadata) = registry.0.get(&module.module_type) else {
            continue;
        };
        for (input, input_metadata) in metadata.inputs.iter().enumerate() {
            let source = module.property_source(input_metadata.name);
            let value = bound_automation_parameter(effect, module, input_metadata.name)
                .map(|parameter| parameter.default.clone())
                .or_else(|| module_parameter(module, input_metadata.name));
            let display_name = localized_properties_input(
                localizer,
                input_metadata.name,
                input_metadata.display_name,
                false,
            );
            let lane_id = |channel| AutomationLaneId {
                emitter: emitter.id,
                module: module.id,
                input: input as u8,
                parameter: input_metadata.name.into(),
                channel,
            };
            match (source, value) {
                (Some(aestra_core::PropertySource::Curve(_)), Some(Value::Curve(curve))) => {
                    lanes.push(AutomationLaneProjection {
                        id: lane_id(None),
                        label: display_name,
                        keys: AutomationLaneKeys::Curve(curve.keys, curve.interpolation),
                    });
                }
                (Some(aestra_core::PropertySource::Curve(_)), Some(Value::Vec3Curve(curves))) => {
                    for (channel, curve) in curves.curves.into_iter().enumerate() {
                        lanes.push(AutomationLaneProjection {
                            id: lane_id(Some(channel as u8)),
                            label: format!("{display_name} {}", ["X", "Y", "Z"][channel]),
                            keys: AutomationLaneKeys::Curve(curve.keys, curve.interpolation),
                        });
                    }
                }
                (
                    Some(aestra_core::PropertySource::Gradient(_)),
                    Some(Value::Gradient(gradient)),
                ) => lanes.push(AutomationLaneProjection {
                    id: lane_id(None),
                    label: display_name,
                    keys: AutomationLaneKeys::Gradient(gradient.keys),
                }),
                _ => {}
            }
        }
    }
    lanes
}

#[cfg(test)]
pub(super) fn automation_lane_count(emitter: &Emitter) -> usize {
    emitter
        .modules
        .iter()
        .map(|module| {
            let parameters: Vec<&str> = match &module.parameters {
                ModuleParameters::Emission { .. } => vec!["spawn_rate", "burst_count"],
                ModuleParameters::Shape { .. } => vec!["shape"],
                ModuleParameters::Initialize { .. } => vec![
                    "lifetime",
                    "speed",
                    "direction",
                    "spread_degrees",
                    "angular_velocity",
                ],
                ModuleParameters::Motion { .. } => vec!["gravity", "drag", "turbulence"],
                ModuleParameters::Appearance { .. } => vec!["size", "opacity", "color"],
                ModuleParameters::Persistent { .. } => Vec::new(),
                ModuleParameters::Collision { .. }
                | ModuleParameters::FollowField { .. }
                | ModuleParameters::SpawnFromDomain { .. } => Vec::new(),
                ModuleParameters::Homing { .. } => {
                    vec!["speed", "acceleration", "turn_rate", "arrival_radius"]
                }
                ModuleParameters::Custom(values) => values.keys().map(String::as_str).collect(),
            };
            parameters
                .into_iter()
                .map(|parameter| {
                    if !module
                        .property_source(parameter)
                        .is_some_and(source_is_automation)
                    {
                        return 0;
                    }
                    if matches!(
                        module.active_parameter_value(parameter),
                        Some(Value::Vec3Curve(_))
                    ) {
                        3
                    } else {
                        1
                    }
                })
                .sum::<usize>()
        })
        .sum()
}

#[cfg(test)]
pub(super) fn source_is_automation(source: aestra_core::PropertySource) -> bool {
    matches!(
        source,
        aestra_core::PropertySource::Curve(_) | aestra_core::PropertySource::Gradient(_)
    )
}

pub(super) fn automation_lane_is_visible(state: &TimelineState, lane: &AutomationLaneId) -> bool {
    state.expanded_automation_emitters.contains(&lane.emitter)
        && state.visible_automation_lanes.contains(lane)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn lanes_project_existing_curve_and_gradient_parameters() {
        let session = test_support::session_with_timing_slack();
        let emitter = &session.effect.emitters[0];
        let registry = EditorModuleRegistry::default();
        let localizer = Localizer::new("en-US").unwrap();

        let lanes = emitter_automation_lanes(&session.effect, emitter, &registry, &localizer);

        assert_eq!(lanes.len(), automation_lane_count(emitter));
        assert!(lanes.iter().any(|lane| lane.id.parameter == "size"));
        assert!(lanes.iter().any(|lane| lane.id.parameter == "opacity"));
        assert!(lanes.iter().any(|lane| lane.id.parameter == "color"));
        assert!(lanes.iter().all(|lane| lane.keys.len() >= 2));

        let mut constant_size = emitter.clone();
        let appearance = constant_size
            .modules
            .iter_mut()
            .find(|module| matches!(&module.parameters, ModuleParameters::Appearance { .. }))
            .unwrap();
        appearance.property_sources.insert(
            "size".into(),
            aestra_core::PropertySource::Curve(aestra_core::PropertyEvaluationDomain::ParticleLife),
        );
        let ModuleParameters::Appearance { size, .. } = &mut appearance.parameters else {
            panic!("fixture emitter should have appearance automation");
        };
        size.keys.truncate(1);
        let one_key_lanes =
            emitter_automation_lanes(&session.effect, &constant_size, &registry, &localizer);
        assert!(one_key_lanes.iter().any(|lane| lane.id.parameter == "size"));
        constant_size
            .modules
            .iter_mut()
            .find(|module| matches!(&module.parameters, ModuleParameters::Appearance { .. }))
            .unwrap()
            .property_sources
            .insert("size".into(), aestra_core::PropertySource::Constant);
        let constant_lanes =
            emitter_automation_lanes(&session.effect, &constant_size, &registry, &localizer);
        assert!(
            !constant_lanes
                .iter()
                .any(|lane| lane.id.parameter == "size")
        );
        assert_eq!(constant_lanes.len(), automation_lane_count(&constant_size));
    }

    #[test]
    fn emitter_time_curve_projects_as_an_automation_lane() {
        let mut session = test_support::session_with_timing_slack();
        let emitter = &mut session.effect.emitters[0];
        let emission = emitter
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == aestra_core::MODULE_EMISSION)
            .unwrap();
        let source =
            aestra_core::PropertySource::Curve(aestra_core::PropertyEvaluationDomain::EmitterTime);
        emission
            .property_sources
            .insert("spawn_rate".into(), source);
        emission.property_source_values.insert(
            "spawn_rate".into(),
            vec![aestra_core::PropertySourceValue::new(
                source,
                Value::Curve(aestra_core::Curve::new(vec![
                    CurveKey::new(0.0, 4.0),
                    CurveKey::new(1.0, 24.0),
                ])),
            )],
        );
        let registry = EditorModuleRegistry::default();
        let localizer = Localizer::new("en-US").unwrap();

        let lanes = emitter_automation_lanes(
            &session.effect,
            &session.effect.emitters[0],
            &registry,
            &localizer,
        );

        assert!(lanes.iter().any(|lane| lane.id.parameter == "spawn_rate"));
        assert_eq!(
            lanes.len(),
            automation_lane_count(&session.effect.emitters[0])
        );
    }
}
