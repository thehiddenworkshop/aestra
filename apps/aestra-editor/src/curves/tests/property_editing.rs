//! Execute the same semantic editing helpers on both engines, with real history routing.
use super::*;
use crate::{
    ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
    test_support,
};
use aestra_core::{
    Curve, EffectAsset, EffectParameter, Gradient, ParameterId, PropertyEvaluationDomain,
    PropertySource, PropertySourceValue, ScalarRange, Vec3Curve,
};
use bevy::ecs::system::RunSystemOnce;

#[derive(Clone, Copy)]
enum Kind {
    Scalar,
    Vector,
    Gradient,
}

struct Fixture {
    app: App,
    selection: ComplexSelection,
    before: EffectAsset,
    parameter: &'static str,
    _root: tempfile::TempDir,
    path: std::path::PathBuf,
    bytes: Vec<u8>,
}
fn scalar() -> Curve {
    Curve::new(vec![
        CurveKey::new(0.0, 0.2),
        CurveKey::new(0.5, 0.5),
        CurveKey::new(1.0, 0.8),
    ])
}
impl Fixture {
    fn new(kind: Kind, bound: bool) -> Self {
        let mut session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let parameter = match kind {
            Kind::Scalar => "size",
            Kind::Vector => "gravity",
            Kind::Gradient => "color",
        };
        let module = session
            .selected_layer()
            .unwrap()
            .modules
            .iter()
            .find(|m| m.parameter_value(parameter).is_some())
            .unwrap();
        let module_id = module.id;
        let input = registry
            .0
            .get(&module.module_type)
            .unwrap()
            .inputs
            .iter()
            .position(|i| i.name == parameter)
            .unwrap() as u8;
        let mut value = match kind {
            Kind::Scalar => Value::Curve(scalar()),
            Kind::Vector => Value::Vec3Curve(Vec3Curve {
                curves: [scalar(), scalar(), scalar()],
            }),
            Kind::Gradient => Value::Gradient(Gradient::new(vec![
                ColorKey::new(0.0, [0.1, 0.2, 0.3, 0.4]),
                ColorKey::new(0.5, [0.5, 0.5, 0.5, 0.5]),
                ColorKey::new(1.0, [0.9, 0.8, 0.7, 0.6]),
            ])),
        };
        if let Value::Vec3Curve(curves) = &mut value {
            curves.curves[1] =
                Curve::normalized(curves.curves[1].keys.clone(), ScalarRange::new(-10.0, 30.0));
        }
        let source = match kind {
            Kind::Gradient => PropertySource::Gradient(PropertyEvaluationDomain::ParticleLife),
            _ => PropertySource::Curve(PropertyEvaluationDomain::ParticleLife),
        };
        let module = session
            .effect
            .emitters
            .iter_mut()
            .flat_map(|e| e.modules.iter_mut())
            .find(|m| m.id == module_id)
            .unwrap();
        module.property_sources.insert(parameter.into(), source);
        module.property_source_values.insert(
            parameter.into(),
            vec![PropertySourceValue::new(source, value.clone())],
        );
        if bound {
            let id = ParameterId::new();
            module.bindings.insert(parameter.into(), id);
            // Cached local values and the parameter default are distinct authored objects.
            match &mut value {
                Value::Curve(curve) => curve.id = aestra_core::CurveId::new(),
                Value::Vec3Curve(curves) => {
                    for curve in &mut curves.curves {
                        curve.id = aestra_core::CurveId::new();
                    }
                }
                Value::Gradient(gradient) => gradient.id = aestra_core::GradientId::new(),
                _ => unreachable!(),
            }
            session.effect.parameters.push(EffectParameter {
                id,
                name: "Shared shape".into(),
                default: value,
                exposed: true,
            });
        }
        let session = EditorSession::from_test_effect(session.effect);
        let selection = ComplexSelection {
            module: module_id,
            input,
            key: 1,
        };
        let state = CurvesState {
            complex: Some(selection),
            vector_channel: 1,
            ..default()
        };
        let before = session.effect.clone();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("curves.aestra.ron");
        before.save_ron(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(registry)
            .insert_resource(state)
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<FunctionEditor>()
            .add_observer(
                |event: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*event, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history expected")
                    });
                },
            );
        Self {
            app,
            selection,
            before,
            parameter,
            _root: root,
            path,
            bytes,
        }
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource()
    }
    fn value(&self) -> Value {
        resolve_complex_input(self.session(), self.app.world().resource(), self.selection)
            .unwrap()
            .2
    }
    fn curve(&self) -> Curve {
        match self.value() {
            Value::Curve(c) => c,
            Value::Vec3Curve(c) => c.curves[self
                .app
                .world()
                .resource::<CurvesState>()
                .vector_channel
                .min(2) as usize]
                .clone(),
            _ => panic!("Expected curve"),
        }
    }
    fn edit(&mut self, edit: ComplexKeyEdit) {
        self.app
            .world_mut()
            .run_system_once(
                move |mut session: ResMut<EditorSession>,
                      registry: Res<EditorModuleRegistry>,
                      mut state: ResMut<CurvesState>| {
                    edit_complex_key(&mut session, &registry.0, &mut state, edit);
                },
            )
            .unwrap();
    }
    fn pointer(&mut self, cursor: Vec2) {
        self.app
            .world_mut()
            .run_system_once(
                move |mut session: ResMut<EditorSession>,
                      registry: Res<EditorModuleRegistry>,
                      mut state: ResMut<CurvesState>| {
                    add_complex_key_at_pointer(&mut session, &registry.0, &mut state, cursor);
                },
            )
            .unwrap();
    }
    fn history(&mut self, action: HistoryAction) {
        self.app.world_mut().trigger(action);
        self.app.world_mut().flush();
    }
    fn round_trip(&mut self) {
        let after = self.session().effect.clone();
        assert_ne!(after, self.before, "{}", self.session().status);
        self.history(HistoryAction::Undo);
        assert_eq!(self.session().effect, self.before);
        self.history(HistoryAction::Redo);
        assert_eq!(self.session().effect, after);
        self.history(HistoryAction::Undo);
        self.history(HistoryAction::Undo);
        assert_eq!(
            self.session().effect,
            self.before,
            "one edit must make one history entry"
        );
        assert_eq!(
            std::fs::read(&self.path).unwrap(),
            self.bytes,
            "editing must not save source files"
        );
    }
    fn unchanged(&mut self) {
        assert_eq!(self.session().effect, self.before);
        self.history(HistoryAction::Undo);
        assert_eq!(self.session().effect, self.before);
        assert_eq!(std::fs::read(&self.path).unwrap(), self.bytes);
    }
}

#[test]
fn resolves_authored_and_bound_scalar_vector_gradient_values() {
    for kind in [Kind::Scalar, Kind::Vector, Kind::Gradient] {
        for bound in [false, true] {
            let f = Fixture::new(kind, bound);
            let (module, metadata, value) =
                resolve_complex_input(f.session(), f.app.world().resource(), f.selection).unwrap();
            assert_eq!(module.id, f.selection.module);
            assert_eq!(metadata.name, f.parameter);
            if bound {
                assert_eq!(value, f.before.parameters[0].default);
            }
            assert_eq!(
                f.app.world().resource::<CurvesState>().selected_key(),
                Some(f.selection)
            );
        }
    }
}

#[test]
fn scalar_add_samples_and_inserts_one_globally_reversible_key() {
    for bound in [false, true] {
        let mut f = Fixture::new(Kind::Scalar, bound);
        let original = f.curve();
        f.edit(ComplexKeyEdit::Add);
        let c = f.curve();
        assert_eq!(c.keys.len(), 4);
        assert_eq!(c.id, original.id);
        assert_eq!(c.keys[2], CurveKey::new(0.75, original.sample(0.75)));
        assert_eq!(
            f.app
                .world()
                .resource::<CurvesState>()
                .selected_key()
                .unwrap()
                .key,
            2
        );
        f.round_trip();
    }
}

#[test]
fn scalar_delete_selects_a_surviving_key_and_round_trips() {
    let mut f = Fixture::new(Kind::Scalar, false);
    f.edit(ComplexKeyEdit::Delete);
    assert_eq!(f.curve().keys.len(), 2);
    assert_eq!(
        f.app
            .world()
            .resource::<CurvesState>()
            .selected_key()
            .unwrap()
            .key,
        1
    );
    f.round_trip();
}

#[test]
fn vector_add_preserves_other_channels_and_normalized_stored_sample() {
    for bound in [false, true] {
        let mut f = Fixture::new(Kind::Vector, bound);
        let Value::Vec3Curve(before) = f.value() else {
            unreachable!()
        };
        f.edit(ComplexKeyEdit::Add);
        let Value::Vec3Curve(after) = f.value() else {
            unreachable!()
        };
        assert_eq!(after.curves[0], before.curves[0]);
        assert_eq!(after.curves[2], before.curves[2]);
        assert_eq!(after.curves[1].keys[2], CurveKey::new(0.75, 0.65));
        assert_eq!(after.curves[1].output_range, before.curves[1].output_range);
        assert_eq!(after.curves[1].id, before.curves[1].id);
        f.round_trip();
    }
}

#[test]
fn vector_delete_and_value_edits_are_channel_scoped() {
    for edit in [
        ComplexKeyEdit::Delete,
        ComplexKeyEdit::CurveValue(1),
        ComplexKeyEdit::Time(-1),
    ] {
        let mut f = Fixture::new(Kind::Vector, false);
        let Value::Vec3Curve(before) = f.value() else {
            unreachable!()
        };
        f.edit(edit);
        let Value::Vec3Curve(after) = f.value() else {
            unreachable!()
        };
        assert_eq!(after.curves[0], before.curves[0]);
        assert_eq!(after.curves[2], before.curves[2]);
        assert_ne!(after.curves[1], before.curves[1]);
        f.round_trip();
    }
}

#[test]
fn vector_channel_is_clamped_to_z() {
    let mut f = Fixture::new(Kind::Vector, false);
    f.app
        .world_mut()
        .resource_mut::<CurvesState>()
        .vector_channel = 255;
    let Value::Vec3Curve(before) = f.value() else {
        unreachable!()
    };
    f.edit(ComplexKeyEdit::Add);
    let Value::Vec3Curve(after) = f.value() else {
        unreachable!()
    };
    assert_eq!(after.curves[0], before.curves[0]);
    assert_eq!(after.curves[1], before.curves[1]);
    assert_eq!(after.curves[2].keys.len(), 4);
    f.round_trip();
}

#[test]
fn gradient_add_samples_color_and_preserves_identity() {
    for bound in [false, true] {
        let mut f = Fixture::new(Kind::Gradient, bound);
        let Value::Gradient(before) = f.value() else {
            unreachable!()
        };
        f.edit(ComplexKeyEdit::Add);
        let Value::Gradient(after) = f.value() else {
            unreachable!()
        };
        assert_eq!(after.id, before.id);
        assert_eq!(after.keys[2], ColorKey::new(0.75, before.sample(0.75)));
        f.round_trip();
    }
}

#[test]
fn gradient_delete_and_time_adjustment_use_semantic_history() {
    for edit in [ComplexKeyEdit::Delete, ComplexKeyEdit::Time(1)] {
        let mut f = Fixture::new(Kind::Gradient, false);
        f.edit(edit);
        f.round_trip();
    }
}

#[test]
fn gradient_rgba_adjustments_are_independent_and_reversible() {
    for channel in 0..4 {
        let mut f = Fixture::new(Kind::Gradient, true);
        let Value::Gradient(before) = f.value() else {
            unreachable!()
        };
        f.edit(ComplexKeyEdit::GradientChannel(channel, 1));
        let Value::Gradient(after) = f.value() else {
            unreachable!()
        };
        for i in 0..4 {
            assert_eq!(
                after.keys[1].color[i],
                before.keys[1].color[i] + if i == channel as usize { 0.05 } else { 0.0 }
            );
        }
        assert_eq!(after.keys[0], before.keys[0]);
        assert_eq!(after.keys[2], before.keys[2]);
        f.round_trip();
    }
}

#[test]
fn gradient_channels_clamp_and_invalid_channel_does_not_edit() {
    for direction in [-127, 127] {
        let mut f = Fixture::new(Kind::Gradient, false);
        f.edit(ComplexKeyEdit::GradientChannel(3, direction));
        let Value::Gradient(after) = f.value() else {
            unreachable!()
        };
        assert_eq!(
            after.keys[1].color[3],
            if direction < 0 { 0.0 } else { 1.0 }
        );
        f.round_trip();
    }
    let mut f = Fixture::new(Kind::Gradient, false);
    f.edit(ComplexKeyEdit::GradientChannel(4, 1));
    f.unchanged();
}

#[test]
fn all_property_kinds_keep_at_least_two_keys() {
    for kind in [Kind::Scalar, Kind::Vector, Kind::Gradient] {
        let mut f = Fixture::new(kind, false);
        f.edit(ComplexKeyEdit::Delete);
        let two = f.session().effect.clone();
        f.edit(ComplexKeyEdit::Delete);
        assert_eq!(f.session().effect, two);
        assert!(f.session().status.contains("at least two keys"));
        f.round_trip();
    }
}

#[test]
fn time_adjustments_keep_neighbors_ordered() {
    for kind in [Kind::Scalar, Kind::Vector, Kind::Gradient] {
        for direction in [-127, 127] {
            let mut f = Fixture::new(kind, false);
            f.edit(ComplexKeyEdit::Time(direction));
            let time = match f.value() {
                Value::Gradient(g) => g.keys[1].time,
                _ => f.curve().keys[1].time,
            };
            assert_eq!(time, if direction < 0 { 0.001 } else { 0.999 });
            f.round_trip();
        }
    }
}

#[test]
fn pointer_insert_uses_graph_coordinates_and_selects_new_key() {
    let mut f = Fixture::new(Kind::Scalar, false);
    let mut expected = f.curve();
    let control = resolve_complex_input(f.session(), f.app.world().resource(), f.selection)
        .unwrap()
        .1
        .control;
    let (_, min, max) = curve_value_bounds(&control, &expected).unwrap();
    let value = curve_graph_data(&expected)
        .value_for_top_percent(25.0)
        .unwrap()
        .clamp(min, max);
    expected.keys.insert(1, CurveKey::new(0.25, value));
    f.pointer(Vec2::new(-0.25, -0.25));
    assert_eq!(f.curve(), expected);
    assert_eq!(
        f.app
            .world()
            .resource::<CurvesState>()
            .selected_key()
            .unwrap()
            .key,
        1
    );
    f.round_trip();
}

#[test]
fn pointer_on_existing_key_only_changes_selection() {
    for kind in [Kind::Scalar, Kind::Vector, Kind::Gradient] {
        let mut f = Fixture::new(kind, false);
        let revision = f.session().ui_revision;
        f.pointer(Vec2::new(0.4999, 99.0));
        assert_eq!(
            f.app
                .world()
                .resource::<CurvesState>()
                .selected_key()
                .unwrap()
                .key,
            2
        );
        assert_eq!(f.session().ui_revision, revision + 1);
        f.unchanged();
    }
}

#[test]
fn pointer_outside_graph_clamps_to_existing_endpoints_without_history() {
    let mut f = Fixture::new(Kind::Scalar, false);
    f.pointer(Vec2::new(-20.0, 30.0));
    assert_eq!(
        f.app
            .world()
            .resource::<CurvesState>()
            .selected_key()
            .unwrap()
            .key,
        0
    );
    f.pointer(Vec2::new(20.0, -30.0));
    assert_eq!(
        f.app
            .world()
            .resource::<CurvesState>()
            .selected_key()
            .unwrap()
            .key,
        2
    );
    f.unchanged();
}

#[test]
fn pointer_vector_insert_keeps_normalized_shape_and_other_channels() {
    let mut f = Fixture::new(Kind::Vector, true);
    let Value::Vec3Curve(before) = f.value() else {
        unreachable!()
    };
    f.pointer(Vec2::new(-0.25, -0.25));
    let Value::Vec3Curve(after) = f.value() else {
        unreachable!()
    };
    assert_eq!(after.curves[0], before.curves[0]);
    assert_eq!(after.curves[2], before.curves[2]);
    // The production raster's ordinate mapping includes its existing plot padding.
    let expected = curve_graph_data(&before.curves[1])
        .value_for_top_percent(25.0)
        .unwrap();
    assert_eq!(after.curves[1].keys[1], CurveKey::new(0.25, expected));
    assert_eq!(after.curves[1].output_range, before.curves[1].output_range);
    f.round_trip();
}

#[test]
fn pointer_gradient_insert_samples_existing_ramp_not_vertical_coordinate() {
    let mut f = Fixture::new(Kind::Gradient, false);
    let Value::Gradient(before) = f.value() else {
        unreachable!()
    };
    f.pointer(Vec2::new(-0.25, 100.0));
    let Value::Gradient(after) = f.value() else {
        unreachable!()
    };
    assert_eq!(after.keys[1], ColorKey::new(0.25, before.sample(0.25)));
    f.round_trip();
}

#[test]
fn stale_module_and_input_are_non_mutating_and_actionable() {
    for missing_module in [true, false] {
        let mut f = Fixture::new(Kind::Scalar, false);
        let mut selection = f.selection;
        if missing_module {
            selection.module = ModuleId::new();
        } else {
            selection.input = 255;
        }
        f.app.world_mut().resource_mut::<CurvesState>().complex = Some(selection);
        assert!(resolve_complex_input(f.session(), f.app.world().resource(), selection).is_none());
        f.edit(ComplexKeyEdit::Add);
        assert!(f.session().status.contains("no longer exists"));
        f.pointer(Vec2::ZERO);
        f.unchanged();
    }
}

#[test]
fn missing_selection_is_safe_for_edit_and_pointer_insert() {
    let mut f = Fixture::new(Kind::Scalar, false);
    f.app.world_mut().resource_mut::<CurvesState>().complex = None;
    f.edit(ComplexKeyEdit::Add);
    assert_eq!(f.session().status, "Select a curve or gradient first");
    f.pointer(Vec2::ZERO);
    f.unchanged();
}

#[test]
fn dangling_binding_does_not_fall_back_to_local_authored_value() {
    let mut f = Fixture::new(Kind::Scalar, true);
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .effect
        .parameters
        .clear();
    f.before = f.session().effect.clone();
    assert!(resolve_complex_input(f.session(), f.app.world().resource(), f.selection).is_none());
    f.edit(ComplexKeyEdit::Add);
    assert_eq!(
        f.session().status,
        "The selected authored value no longer exists"
    );
    f.pointer(Vec2::ZERO);
    f.unchanged();
}

#[test]
fn incompatible_edit_kinds_do_not_create_history() {
    for (kind, edit) in [
        (Kind::Scalar, ComplexKeyEdit::GradientChannel(0, 1)),
        (Kind::Gradient, ComplexKeyEdit::CurveValue(1)),
    ] {
        let mut f = Fixture::new(kind, false);
        f.edit(edit);
        assert_eq!(
            f.session().status,
            "This edit does not apply to the selected property"
        );
        f.unchanged();
    }
}

#[test]
fn preview_math_is_non_mutating_and_clamps_normalized_shapes() {
    let mut f = Fixture::new(Kind::Vector, false);
    let c = f.curve();
    let (key, preview) = curve_drag_preview(
        &c,
        1,
        Vec2::new(999.0, -999.0),
        Vec2::new(100.0, 100.0),
        0.0,
        1.0,
    )
    .unwrap();
    assert_eq!(key, CurveKey::new(0.999, 1.0));
    let AutomationCurveData::Curve {
        points,
        value_bounds,
        ..
    } = preview
    else {
        unreachable!()
    };
    assert_eq!(value_bounds, Some((0.0, 1.0)));
    assert_eq!(points[1].time, key.time);
    assert_eq!(points[1].value, key.value);
    assert!(curve_drag_preview(&c, 99, Vec2::ZERO, Vec2::ONE, 0.0, 1.0).is_none());
    f.unchanged();
}

#[test]
fn gradient_preview_retimes_without_changing_rgba() {
    let mut f = Fixture::new(Kind::Gradient, false);
    let Value::Gradient(g) = f.value() else {
        unreachable!()
    };
    let (key, preview) = gradient_drag_preview(&g, 1, 10.0, 100.0).unwrap();
    assert_eq!(key.time, 0.6);
    assert_eq!(key.color, g.keys[1].color);
    let AutomationCurveData::Gradient(points) = preview else {
        unreachable!()
    };
    assert_eq!(points[1].time, key.time);
    assert_eq!(points[1].color, key.color);
    assert!(gradient_drag_preview(&g, 99, 0.0, 1.0).is_none());
    f.unchanged();
}

#[test]
fn normalized_output_labels_and_degenerate_range_sampling_keep_stored_values() {
    let mut c = Curve::normalized(
        vec![CurveKey::new(0.0, 0.25), CurveKey::new(1.0, 0.75)],
        ScalarRange::new(5.8, 32.0),
    );
    assert_eq!(
        formatted_curve_output_value(curve_key_output_value(&c, c.keys[1])),
        "25.45"
    );
    assert!((curve_stored_sample(&c, 0.5) - 0.5).abs() < 0.00001);
    c.output_range = Some(ScalarRange::new(4.0, 4.0));
    assert_eq!(curve_stored_sample(&c, 0.5), 0.25);
    assert_eq!(
        curve_value_bounds(&InputControl::Gradient, &c),
        Some((0.01, 0.0, 1.0))
    );
}

#[test]
fn editable_curve_resolves_only_the_matching_scalar_or_vector_channel() {
    for kind in [Kind::Scalar, Kind::Vector, Kind::Gradient] {
        let f = Fixture::new(kind, true);
        let scalar = editable_curve(f.session(), f.selection.module, f.parameter, None);
        let vector = editable_curve(f.session(), f.selection.module, f.parameter, Some(255));
        match kind {
            Kind::Scalar => {
                assert_eq!(scalar, Some(f.curve()));
                assert!(vector.is_none());
            }
            Kind::Vector => {
                let Value::Vec3Curve(c) = f.value() else {
                    unreachable!()
                };
                assert_eq!(vector, Some(c.curves[2].clone()));
                assert!(scalar.is_none());
            }
            Kind::Gradient => {
                assert!(scalar.is_none());
                assert!(vector.is_none());
            }
        }
        assert!(editable_curve(f.session(), ModuleId::new(), f.parameter, None).is_none());
    }
}

#[test]
fn invalid_vector_key_indices_do_not_commit_changes() {
    let mut f = Fixture::new(Kind::Vector, false);
    let module = f.selection.module;
    f.app
        .world_mut()
        .run_system_once(move |mut session: ResMut<EditorSession>| {
            set_editable_curve_key(
                &mut session,
                module,
                "gravity",
                Some(1),
                99,
                CurveKey::new(0.2, 0.4),
            );
            insert_vector_curve_key(
                &mut session,
                module,
                "gravity",
                1,
                99,
                CurveKey::new(0.2, 0.4),
            );
            remove_vector_curve_key(&mut session, module, "gravity", 1, 99);
        })
        .unwrap();
    f.unchanged();
}

#[test]
fn bounds_use_authored_range_when_metadata_has_no_limits() {
    let c = Curve::new(vec![CurveKey::new(0.0, -3.0), CurveKey::new(1.0, 7.0)]);
    assert_eq!(
        curve_value_bounds(
            &InputControl::Number {
                step: 0.5,
                min: None,
                max: None
            },
            &c
        ),
        Some((0.5, -3.0, 7.0))
    );
    assert_eq!(curve_value_bounds(&InputControl::Gradient, &c), None);
    assert_eq!(insertion_time(&[], 99), (0, 0.5));
    assert_eq!(insertion_time(&[0.0, 1.0], 99), (1, 0.5));
    assert_eq!(bounded_key_time(&[0.0, 0.5, 1.0], 1, 9.0), 0.999);
}
