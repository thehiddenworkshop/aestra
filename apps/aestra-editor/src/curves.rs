//! Curves workspace, curve/gradient selection, and semantic key editing.

#[cfg(test)]
use crate::feathers::automation_curve::{self, AutomationCurveData};
#[cfg(test)]
use crate::*;
mod composition;
#[path = "curves/control_input.rs"]
mod control_input;
mod controls;
use crate::timeline::panel_input as cursor;
mod host_composition;
mod host_editing;
mod host_install;
mod host_motion;
mod plugin;
use crate::timeline::panel_input as pointer;
pub(crate) use composition::spawn_curves_workspace;
#[cfg(test)]
use composition::*;
pub(crate) use plugin::{CurvesSet, EditorCurvesPlugin};
mod keyboard;
#[cfg(test)]
use keyboard::CurveGraph;
mod property_editing;
mod state;
#[cfg(test)]
use aestra_compiler::InputControl;
#[cfg(test)]
use aestra_core::{CurveKey, Value};
#[cfg(test)]
use bevy::ui::RelativeCursorPosition;
#[cfg(test)]
use bevy::{input_focus::InputFocus, ui_widgets::Activate};
#[cfg(test)]
use property_editing::*;
#[cfg(test)]
use state::ComplexSelection;
pub(crate) use state::CurvesState;

#[path = "curves/action.rs"]
mod action;
pub(crate) use action::CurvesAction;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use crate::timeline::TimelineState;

    fn first_curve_selection(
        session: &EditorSession,
        registry: &EditorModuleRegistry,
    ) -> ComplexSelection {
        for module in &session.selected_layer().unwrap().modules {
            let Some(metadata) = registry.0.get(&module.module_type) else {
                continue;
            };
            for (input, metadata) in metadata.inputs.iter().enumerate() {
                if matches!(
                    module_parameter(module, metadata.name),
                    Some(Value::Curve(_))
                ) && complex_input_is_visible(module, metadata)
                {
                    return ComplexSelection {
                        module: module.id,
                        input: input as u8,
                        key: 0,
                    };
                }
            }
        }
        panic!("test fixture should expose a curve input");
    }

    fn curve_key_count(
        session: &EditorSession,
        registry: &EditorModuleRegistry,
        selection: ComplexSelection,
    ) -> usize {
        let module = session
            .selected_layer()
            .unwrap()
            .modules
            .iter()
            .find(|module| module.id == selection.module)
            .unwrap();
        let input = &registry.0.get(&module.module_type).unwrap().inputs[selection.input as usize];
        let Some(Value::Curve(curve)) = module_parameter(module, input.name) else {
            panic!("selection should resolve to a curve");
        };
        curve.keys.len()
    }

    #[test]
    fn curves_plugin_owns_feathers_activation_and_selection() {
        let session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let selection = first_curve_selection(&session, &registry);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(registry)
            .init_resource::<WorkspaceLayout>()
            .init_resource::<TimelineState>()
            .add_plugins(EditorCurvesPlugin);
        let control = app
            .world_mut()
            .spawn((
                Button,
                FeathersActionButton,
                Interaction::None,
                CurvesAction::OpenInput(selection.module, selection.input),
                BackgroundColor::default(),
            ))
            .id();

        app.world_mut().trigger(Activate { entity: control });
        app.update();

        assert!(app.world().resource::<CurvesState>().has_selection());
        assert!(
            !app.world()
                .entity(control)
                .contains::<PendingFeathersActivation>()
        );
    }

    #[test]
    fn adding_a_curve_key_is_one_undoable_semantic_edit() {
        let session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let selection = first_curve_selection(&session, &registry);
        let initial = curve_key_count(&session, &registry, selection);
        let mut state = CurvesState::default();
        state.select_for_test(selection.module, selection.input, selection.key);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(registry)
            .insert_resource(state)
            .init_resource::<WorkspaceLayout>()
            .init_resource::<TimelineState>()
            .add_plugins(EditorCurvesPlugin);
        app.world_mut().spawn((
            Button,
            bevy::ui::Pressed,
            CurvesAction::AddKey,
            BackgroundColor::default(),
        ));

        app.update();

        let registry = app.world().resource::<EditorModuleRegistry>();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(curve_key_count(session, registry, selection), initial + 1);
        app.world_mut().resource_mut::<EditorSession>().undo();
        let registry = app.world().resource::<EditorModuleRegistry>();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(curve_key_count(session, registry, selection), initial);
    }

    #[test]
    fn property_curve_interpolation_is_an_undoable_semantic_edit() {
        let session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let selection = first_curve_selection(&session, &registry);
        let original = resolve_complex_input(&session, &registry, selection)
            .unwrap()
            .2;
        let mut state = CurvesState::default();
        state.select_for_test(selection.module, selection.input, 0);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(registry)
            .insert_resource(state)
            .init_resource::<WorkspaceLayout>()
            .init_resource::<TimelineState>()
            .add_plugins(EditorCurvesPlugin);
        app.world_mut().spawn((
            Button,
            bevy::ui::Pressed,
            CurvesAction::SetInterpolation(aestra_core::CurveInterpolation::Step),
            BackgroundColor::default(),
        ));
        app.update();
        let value = resolve_complex_input(
            app.world().resource::<EditorSession>(),
            app.world().resource::<EditorModuleRegistry>(),
            selection,
        )
        .unwrap()
        .2;
        let Value::Curve(curve) = value else {
            panic!("curve fixture");
        };
        assert_eq!(curve.interpolation, aestra_core::CurveInterpolation::Step);
        app.world_mut().resource_mut::<EditorSession>().undo();
        let value = resolve_complex_input(
            app.world().resource::<EditorSession>(),
            app.world().resource::<EditorModuleRegistry>(),
            selection,
        )
        .unwrap()
        .2;
        assert_eq!(value, original);
    }

    #[test]
    fn insert_and_delete_shortcuts_edit_the_curve_under_the_pointer() {
        let session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let selection = first_curve_selection(&session, &registry);
        let initial = curve_key_count(&session, &registry, selection);
        let mut state = CurvesState::default();
        state.select_for_test(selection.module, selection.input, selection.key);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(registry)
            .insert_resource(state)
            .init_resource::<WorkspaceLayout>()
            .init_resource::<TimelineState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(EditorCurvesPlugin);
        app.world_mut().spawn((
            CurveGraph,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::new(0.17, -0.1)),
            },
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Insert);

        app.update();

        let inserted_selection = app
            .world()
            .resource::<CurvesState>()
            .selected_key()
            .unwrap();
        let registry = app.world().resource::<EditorModuleRegistry>();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(curve_key_count(session, registry, selection), initial + 1);
        let (_, _, Value::Curve(curve)) =
            resolve_complex_input(session, registry, inserted_selection).unwrap()
        else {
            panic!("expected curve input");
        };
        assert!((curve.keys[inserted_selection.key].time - 0.67).abs() < 0.0001);

        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.release(KeyCode::Insert);
            keys.press(KeyCode::Delete);
        }
        app.update();

        let registry = app.world().resource::<EditorModuleRegistry>();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(curve_key_count(session, registry, selection), initial);
        app.world_mut().resource_mut::<EditorSession>().undo();
        let registry = app.world().resource::<EditorModuleRegistry>();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(curve_key_count(session, registry, selection), initial + 1);
    }

    #[test]
    fn curves_workspace_projects_keys_into_shared_feather_data() {
        let session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let selection = first_curve_selection(&session, &registry);
        let (_, _, Value::Curve(curve)) =
            resolve_complex_input(&session, &registry, selection).unwrap()
        else {
            panic!("expected curve input");
        };

        let AutomationCurveData::Curve { points, .. } = curve_graph_data(&curve) else {
            panic!("expected Feather curve data");
        };
        assert_eq!(points.len(), curve.keys.len());
        assert_eq!(points[selection.key].time, curve.keys[selection.key].time);
        assert_eq!(points[selection.key].value, curve.keys[selection.key].value);
    }

    #[test]
    fn vector_curve_edits_only_the_selected_gravity_channel_and_is_undoable() {
        let mut session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let module = session
            .selected_layer()
            .unwrap()
            .modules
            .iter()
            .find(|module| module.parameter_value("gravity").is_some())
            .unwrap();
        let module_id = module.id;
        let input = registry
            .0
            .get(&module.module_type)
            .unwrap()
            .inputs
            .iter()
            .position(|input| input.name == "gravity")
            .unwrap() as u8;
        let source =
            aestra_core::PropertySource::Curve(aestra_core::PropertyEvaluationDomain::ParticleLife);
        let module = session
            .effect
            .emitters
            .iter_mut()
            .flat_map(|emitter| emitter.modules.iter_mut())
            .find(|module| module.id == module_id)
            .unwrap();
        module.property_sources.insert("gravity".into(), source);
        module.property_source_values.insert(
            "gravity".into(),
            vec![aestra_core::PropertySourceValue::new(
                source,
                Value::Vec3Curve(aestra_core::Vec3Curve::constant([1.0, 2.0, 3.0])),
            )],
        );
        let selection = ComplexSelection {
            module: module_id,
            input,
            key: 0,
        };
        let mut state = CurvesState {
            complex: Some(selection),
            vector_channel: 1,
            ..default()
        };

        edit_complex_key(&mut session, &registry.0, &mut state, ComplexKeyEdit::Add);

        let (_, _, Value::Vec3Curve(curves)) =
            resolve_complex_input(&session, &registry, selection).unwrap()
        else {
            panic!("gravity should remain an XYZ curve source");
        };
        assert_eq!(curves.curves[0].keys.len(), 2);
        assert_eq!(curves.curves[1].keys.len(), 3, "{}", session.status);
        assert_eq!(curves.curves[2].keys.len(), 2);
        session.undo();
        let (_, _, Value::Vec3Curve(curves)) =
            resolve_complex_input(&session, &registry, selection).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(curves.curves[1].keys.len(), 2);
    }

    #[test]
    fn normalized_curve_graph_uses_fixed_shape_bounds_and_real_output_labels() {
        let curve = aestra_core::Curve::normalized(
            vec![CurveKey::new(0.0, 0.25), CurveKey::new(1.0, 0.75)],
            aestra_core::ScalarRange::new(5.8, 32.0),
        );
        let AutomationCurveData::Curve { value_bounds, .. } = curve_graph_data(&curve) else {
            panic!("expected Feather curve data");
        };

        assert_eq!(value_bounds, Some((0.0, 1.0)));
        assert_eq!(
            formatted_curve_output_value(curve.output_range().min),
            "5.8"
        );
        assert_eq!(formatted_curve_output_value(curve.output_range().max), "32");
        assert_eq!(
            formatted_curve_output_value(curve_key_output_value(&curve, curve.keys[1])),
            "25.45"
        );
    }

    #[test]
    fn curves_workspace_visibility_uses_the_explicit_source_not_key_count() {
        let mut session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let selection = first_curve_selection(&session, &registry);
        let (_, input, Value::Curve(mut curve)) =
            resolve_complex_input(&session, &registry, selection).unwrap()
        else {
            panic!("expected curve input");
        };
        let parameter = input.name;
        curve.keys.truncate(1);
        session.set_module_parameter(selection.module, parameter, Value::Curve(curve));
        let module = session
            .effect
            .emitters
            .iter_mut()
            .flat_map(|emitter| emitter.modules.iter_mut())
            .find(|module| module.id == selection.module)
            .unwrap();
        module.property_sources.insert(
            parameter.into(),
            aestra_core::PropertySource::Curve(aestra_core::PropertyEvaluationDomain::ParticleLife),
        );
        let input = &registry.0.get(&module.module_type).unwrap().inputs[selection.input as usize];
        assert!(complex_input_is_visible(module, input));

        module
            .property_sources
            .insert(parameter.into(), aestra_core::PropertySource::Constant);

        let module = session
            .selected_layer()
            .unwrap()
            .modules
            .iter()
            .find(|module| module.id == selection.module)
            .unwrap();
        let input = &registry.0.get(&module.module_type).unwrap().inputs[selection.input as usize];

        assert!(!complex_input_is_visible(module, input));
    }

    #[test]
    fn curve_drag_preview_updates_the_shared_raster_data() {
        let session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let selection = first_curve_selection(&session, &registry);
        let (_, input, Value::Curve(curve)) =
            resolve_complex_input(&session, &registry, selection).unwrap()
        else {
            panic!("expected curve input");
        };
        let InputControl::Curve { min, max, .. } = input.control else {
            panic!("expected curve metadata");
        };

        let (key, preview) = curve_drag_preview(
            &curve,
            selection.key,
            Vec2::new(12.0, 8.0),
            Vec2::new(120.0, 100.0),
            min,
            max,
        )
        .unwrap();
        let AutomationCurveData::Curve { points, .. } = preview else {
            panic!("expected curve preview");
        };
        assert_eq!(points[selection.key].time, key.time);
        assert_eq!(points[selection.key].value, key.value);
        assert_ne!(key.time, curve.keys[selection.key].time);
    }

    #[test]
    fn key_time_helpers_preserve_ordering() {
        assert_eq!(insertion_time(&[0.0, 0.4, 1.0], 1), (2, 0.7));
        assert_eq!(insertion_time(&[0.0, 1.0], 1), (1, 0.5));
        assert_eq!(bounded_key_time(&[0.0, 0.4, 1.0], 1, 2.0), 0.999);
        assert_eq!(bounded_key_time(&[0.0, 0.4, 1.0], 1, -1.0), 0.001);
    }
}
