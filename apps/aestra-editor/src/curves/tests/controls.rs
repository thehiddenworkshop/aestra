//! Real Curves activation, keyboard and semantic consumers on both engines.
use super::super::{EditorCurvesPlugin, keyboard::CurveGraph};
use super::*;
use crate::{
    ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
    test_support,
    timeline::TimelineState,
};
use aestra_core::{
    Curve, CurveInterpolation, CurveKey, EffectAsset, EmitterTransform, HostTransformKey,
    HostTransformTrack, PropertyEvaluationDomain, PropertySource, PropertySourceValue, Value,
    Vec3Curve,
};
use bevy::{input_focus::InputFocus, text::EditableText, ui::RelativeCursorPosition};

struct Fixture {
    app: App,
    selection: ComplexSelection,
    before: EffectAsset,
    layout: WorkspaceLayout,
    _root: tempfile::TempDir,
    path: std::path::PathBuf,
    bytes: Vec<u8>,
}
impl Fixture {
    fn new(vector: bool) -> Self {
        Self::property(if vector { "gravity" } else { "size" })
    }
    fn property(parameter: &'static str) -> Self {
        let mut session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let module = session
            .selected_layer()
            .unwrap()
            .modules
            .iter()
            .find(|m| m.parameter_value(parameter).is_some())
            .unwrap();
        let id = module.id;
        let input = registry
            .0
            .get(&module.module_type)
            .unwrap()
            .inputs
            .iter()
            .position(|i| i.name == parameter)
            .unwrap() as u8;
        let module = session
            .effect
            .emitters
            .iter_mut()
            .flat_map(|e| e.modules.iter_mut())
            .find(|m| m.id == id)
            .unwrap();
        let curve = || {
            Curve::new(vec![
                CurveKey::new(0.0, 0.2),
                CurveKey::new(0.5, 0.5),
                CurveKey::new(1.0, 0.8),
            ])
        };
        let value = if parameter == "gravity" {
            Value::Vec3Curve(Vec3Curve {
                curves: [curve(), curve(), curve()],
            })
        } else if parameter == "color" {
            Value::Gradient(aestra_core::Gradient::new(vec![
                aestra_core::ColorKey::new(0.0, [0.2; 4]),
                aestra_core::ColorKey::new(0.5, [0.5; 4]),
                aestra_core::ColorKey::new(1.0, [0.8; 4]),
            ]))
        } else {
            Value::Curve(curve())
        };
        let source = if parameter == "color" {
            PropertySource::Gradient(PropertyEvaluationDomain::ParticleLife)
        } else {
            PropertySource::Curve(PropertyEvaluationDomain::ParticleLife)
        };
        module.property_sources.insert(parameter.into(), source);
        module.property_source_values.insert(
            parameter.into(),
            vec![PropertySourceValue::new(source, value)],
        );
        session.effect.host_transform_track = Some(HostTransformTrack::from_pose_keys(
            [0.0, 1.0, 2.0]
                .into_iter()
                .map(|time| HostTransformKey {
                    time,
                    transform: EmitterTransform {
                        translation: [time * 10.0, 0.0, 0.0],
                        ..default()
                    },
                })
                .collect(),
            false,
        ));
        let session = EditorSession::from_test_effect(session.effect);
        let before = session.effect.clone();
        let selection = ComplexSelection {
            module: id,
            input,
            key: 1,
        };
        let state = CurvesState {
            complex: Some(selection),
            ..default()
        };
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("controls.aestra.ron");
        before.save_ron(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut layout = WorkspaceLayout::default();
        layout.show(ToolPanel::Curves); // Prevent tests from saving the user's workspace layout.
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(registry)
            .insert_resource(state)
            .insert_resource(layout.clone())
            .init_resource::<TimelineState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<FunctionEditor>()
            .add_plugins(EditorCurvesPlugin)
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
            layout,
            _root: root,
            path,
            bytes,
        }
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource()
    }
    fn state(&self) -> &CurvesState {
        self.app.world().resource()
    }
    fn value(&self) -> Value {
        resolve_complex_input(self.session(), self.app.world().resource(), self.selection)
            .unwrap()
            .2
    }
    fn control(&mut self, action: CurvesAction, feathers: bool) -> Entity {
        let id = self
            .app
            .world_mut()
            .spawn((
                control_input::NativeButton,
                action,
                BackgroundColor::default(),
            ))
            .id();
        control_input::set_phase(self.app.world_mut(), id, Phase::None);
        if feathers {
            self.app
                .world_mut()
                .entity_mut(id)
                .insert(FeathersActionButton);
        }
        id
    }
    fn activate(&mut self, id: Entity) {
        self.app.world_mut().trigger(Activate { entity: id });
        self.app.update();
    }
    fn action(&mut self, action: CurvesAction) -> Entity {
        let id = self.control(action, true);
        self.activate(id);
        id
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
        assert_eq!(self.session().effect, self.before);
        self.files_unchanged();
    }
    fn files_unchanged(&self) {
        assert_eq!(std::fs::read(&self.path).unwrap(), self.bytes);
        assert_eq!(self.app.world().resource::<WorkspaceLayout>(), &self.layout);
    }
    fn graph(&mut self, over: bool) -> Entity {
        self.app
            .world_mut()
            .spawn((
                CurveGraph,
                RelativeCursorPosition {
                    cursor_over: over,
                    normalized: Some(Vec2::new(-0.25, 0.0)),
                },
            ))
            .id()
    }
    fn tap(&mut self, key: KeyCode) {
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        self.app.update();
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
    }
}

#[test]
fn actual_activation_consumes_one_edit_and_resets_pressed_and_pending() {
    let mut f = Fixture::new(false);
    let id = f.action(CurvesAction::AddKey);
    let after = f.session().effect.clone();
    assert!(f.app.world().get::<PendingFeathersActivation>(id).is_none());
    assert!(!control_input::is_pressed(f.app.world(), id));
    for _ in 0..3 {
        f.app.update();
    }
    assert_eq!(f.session().effect, after);
    f.round_trip();
}
#[test]
fn feathers_press_without_activation_is_not_a_command() {
    let mut f = Fixture::new(false);
    let id = f.control(CurvesAction::AddKey, true);
    control_input::set_phase(f.app.world_mut(), id, Phase::Pressed);
    f.app.update();
    assert_eq!(f.session().effect, f.before);
    f.history(HistoryAction::Undo);
    assert_eq!(f.session().effect, f.before);
    f.files_unchanged();
}
#[test]
fn repeated_actual_activation_performs_one_edit_per_activation() {
    let mut f = Fixture::new(false);
    let id = f.action(CurvesAction::AddKey);
    let once = f.session().effect.clone();
    f.activate(id);
    assert_ne!(f.session().effect, once);
    f.history(HistoryAction::Undo);
    assert_eq!(f.session().effect, once);
    f.history(HistoryAction::Undo);
    assert_eq!(f.session().effect, f.before);
    f.files_unchanged();
}
#[test]
fn unrelated_entities_and_non_feathers_controls_do_not_queue_activation() {
    let mut f = Fixture::new(false);
    let id = f.control(CurvesAction::AddKey, false);
    let other = f
        .app
        .world_mut()
        .spawn((FeathersActionButton, BackgroundColor::default()))
        .id();
    f.activate(id);
    f.activate(other);
    assert_eq!(f.session().effect, f.before);
    assert!(f.app.world().get::<PendingFeathersActivation>(id).is_none());
    assert!(
        f.app
            .world()
            .get::<PendingFeathersActivation>(other)
            .is_none()
    );
    f.files_unchanged();
}
#[test]
fn native_hover_press_and_release_preserve_styling_and_do_not_repeat() {
    let mut f = Fixture::new(false);
    let id = f.control(CurvesAction::AddKey, false);
    f.app.update();
    assert_eq!(
        f.app.world().get::<BackgroundColor>(id).unwrap().0,
        theme::BUTTON
    );
    control_input::set_phase(f.app.world_mut(), id, Phase::Hovered);
    f.app.update();
    assert_eq!(
        f.app.world().get::<BackgroundColor>(id).unwrap().0,
        theme::BUTTON_HOVER
    );
    control_input::set_phase(f.app.world_mut(), id, Phase::Pressed);
    f.app.update();
    assert_eq!(
        f.app.world().get::<BackgroundColor>(id).unwrap().0,
        theme::ACCENT_DIM
    );
    let after = f.session().effect.clone();
    f.app.update();
    assert_eq!(f.session().effect, after);
    control_input::set_phase(f.app.world_mut(), id, Phase::None);
    f.app.update();
    assert_eq!(
        f.app.world().get::<BackgroundColor>(id).unwrap().0,
        theme::BUTTON
    );
    f.round_trip();
}
#[test]
fn feathers_hover_does_not_override_widget_theme() {
    let mut f = Fixture::new(false);
    let id = f.control(CurvesAction::AddKey, true);
    let initial = f.app.world().get::<BackgroundColor>(id).unwrap().0;
    control_input::set_phase(f.app.world_mut(), id, Phase::Hovered);
    f.app.update();
    assert_eq!(f.app.world().get::<BackgroundColor>(id).unwrap().0, initial);
    f.files_unchanged();
}
#[test]
fn open_input_uses_actual_dock_reveal_and_clears_host_selection_without_edit() {
    let mut f = Fixture::new(true);
    f.action(CurvesAction::OpenHost(0));
    assert!(f.state().host.is_some());
    f.action(CurvesAction::OpenInput(
        f.selection.module,
        f.selection.input,
    ));
    assert!(f.state().host.is_none());
    assert_eq!(f.state().complex.unwrap().key, 0);
    assert_eq!(f.state().vector_channel, 0);
    assert_eq!(f.session().effect, f.before);
    f.files_unchanged();
}
#[test]
fn channel_selection_clamps_resets_key_and_does_not_edit_document() {
    let mut f = Fixture::new(true);
    f.action(CurvesAction::SelectVectorChannel(255));
    assert_eq!(f.state().vector_channel, 2);
    assert_eq!(f.state().complex.unwrap().key, 0);
    assert_eq!(f.session().effect, f.before);
    f.files_unchanged();
}
#[test]
fn scalar_and_vector_interpolation_are_single_reversible_actions() {
    for vector in [false, true] {
        let mut f = Fixture::new(vector);
        let before = f.value();
        f.action(CurvesAction::SetInterpolation(CurveInterpolation::Step));
        match f.value() {
            Value::Curve(c) => assert_eq!(c.interpolation, CurveInterpolation::Step),
            Value::Vec3Curve(c) => {
                assert_eq!(c.curves[0].interpolation, CurveInterpolation::Step);
                let Value::Vec3Curve(original) = before else {
                    unreachable!()
                };
                assert_eq!(c.curves[1], original.curves[1]);
                assert_eq!(c.curves[2], original.curves[2]);
            }
            _ => unreachable!(),
        }
        let after = f.session().effect.clone();
        f.action(CurvesAction::SetInterpolation(CurveInterpolation::Step));
        assert_eq!(f.session().effect, after);
        f.round_trip();
    }
}
#[test]
fn property_delete_time_and_value_actions_execute_real_edits() {
    for action in [
        CurvesAction::DeleteKey,
        CurvesAction::AdjustTime(1),
        CurvesAction::AdjustCurveValue(1),
    ] {
        let mut f = Fixture::new(false);
        f.action(action);
        f.round_trip();
    }
}
#[test]
fn stale_input_interpolation_is_consumed_without_semantic_edit() {
    let mut f = Fixture::new(false);
    f.app
        .world_mut()
        .resource_mut::<CurvesState>()
        .complex
        .as_mut()
        .unwrap()
        .input = 255;
    let id = f.action(CurvesAction::SetInterpolation(CurveInterpolation::Step));
    assert!(!control_input::is_pressed(f.app.world(), id));
    assert_eq!(f.session().effect, f.before);
    f.files_unchanged();
}
#[test]
fn host_open_clamps_channel_and_clears_property_selection() {
    let mut f = Fixture::new(false);
    f.action(CurvesAction::OpenHost(255));
    assert!(f.state().complex.is_none());
    assert_eq!(f.state().host.unwrap().channel, 9);
    assert_eq!(f.session().effect, f.before);
    f.files_unchanged();
}
#[test]
fn host_scalar_and_quaternion_interpolation_use_actual_commit_and_history() {
    for channel in [0, 6] {
        let mut f = Fixture::new(false);
        f.action(CurvesAction::OpenHost(channel));
        f.action(CurvesAction::SetInterpolation(CurveInterpolation::Step));
        let track = f.session().effect.host_transform_track.as_ref().unwrap();
        if channel == 0 {
            assert_eq!(
                track.curves.translation[0].interpolation,
                CurveInterpolation::Step
            );
        } else {
            assert_eq!(
                track.curves.rotation.interpolation,
                CurveInterpolation::Step
            );
        }
        f.round_trip();
    }
}
#[test]
fn host_add_at_playhead_is_one_edit_and_existing_time_is_not_duplicated() {
    let mut f = Fixture::new(false);
    f.action(CurvesAction::OpenHost(0));
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(0.75);
    f.action(CurvesAction::HostAddKey);
    let track = f.session().effect.host_transform_track.as_ref().unwrap();
    assert!(
        track.curves.translation[0]
            .keys
            .iter()
            .any(|k| k.time == 0.75)
    );
    let after = f.session().effect.clone();
    f.action(CurvesAction::HostAddKey);
    assert_eq!(f.session().effect, after);
    f.round_trip();
}
#[test]
fn host_delete_protects_anchor_and_deletes_non_anchor_with_global_undo() {
    let mut f = Fixture::new(false);
    f.action(CurvesAction::OpenHost(0));
    f.action(CurvesAction::HostDeleteKey);
    assert_eq!(f.session().effect, f.before);
    assert!(f.session().status.contains("time-zero anchor"));
    f.app
        .world_mut()
        .resource_mut::<CurvesState>()
        .host
        .as_mut()
        .unwrap()
        .key = 1;
    f.action(CurvesAction::HostDeleteKey);
    f.round_trip();
}
#[test]
fn insert_keyboard_uses_hovered_graph_coordinates_then_delete_reverses_it() {
    let mut f = Fixture::new(false);
    f.graph(true);
    f.tap(KeyCode::Insert);
    let Value::Curve(c) = f.value() else {
        unreachable!()
    };
    assert_eq!(c.keys.len(), 4);
    assert_eq!(c.keys[1].time, 0.25);
    let after = f.session().effect.clone();
    f.tap(KeyCode::Delete);
    assert_eq!(f.session().effect, f.before);
    f.history(HistoryAction::Undo);
    assert_eq!(f.session().effect, after);
    f.history(HistoryAction::Undo);
    assert_eq!(f.session().effect, f.before);
    f.files_unchanged();
}
#[test]
fn keyboard_ignores_non_hovered_graphs_missing_graphs_and_absent_key_resource() {
    let mut f = Fixture::new(false);
    f.tap(KeyCode::Insert);
    f.graph(false);
    f.tap(KeyCode::Insert);
    f.app.world_mut().remove_resource::<ButtonInput<KeyCode>>();
    f.app.update();
    assert_eq!(f.session().effect, f.before);
    f.files_unchanged();
}
#[test]
fn keyboard_text_focus_guard_preserves_editable_text_and_document() {
    let mut f = Fixture::new(false);
    f.graph(true);
    let text = f.app.world_mut().spawn(EditableText::default()).id();
    f.app
        .world_mut()
        .resource_mut::<InputFocus>()
        .set(text, bevy::input_focus::FocusCause::Navigated);
    f.tap(KeyCode::Insert);
    f.tap(KeyCode::Delete);
    assert_eq!(f.session().effect, f.before);
    f.files_unchanged();
}

#[test]
fn gradient_channel_action_reaches_real_semantic_edit_and_global_history() {
    let mut f = Fixture::property("color");
    f.action(CurvesAction::AdjustGradientChannel(3, 1));
    let Value::Gradient(g) = f.value() else {
        unreachable!()
    };
    assert_eq!(g.keys[1].color, [0.5, 0.5, 0.5, 0.55]);
    f.round_trip();
}
#[test]
fn gradient_interpolation_is_consumed_without_changing_ramp_or_history() {
    let mut f = Fixture::property("color");
    f.action(CurvesAction::SetInterpolation(CurveInterpolation::Step));
    assert_eq!(f.session().effect, f.before);
    f.history(HistoryAction::Undo);
    assert_eq!(f.session().effect, f.before);
    f.files_unchanged();
}
#[test]
fn host_actions_ignore_stale_effect_selection() {
    let mut f = Fixture::new(false);
    f.action(CurvesAction::OpenHost(0));
    f.app
        .world_mut()
        .resource_mut::<CurvesState>()
        .host
        .as_mut()
        .unwrap()
        .effect = aestra_core::EffectId::new();
    for action in [
        CurvesAction::HostAddKey,
        CurvesAction::HostDeleteKey,
        CurvesAction::SetInterpolation(CurveInterpolation::Step),
    ] {
        f.action(action);
    }
    assert_eq!(f.session().effect, f.before);
    f.files_unchanged();
}
#[test]
fn production_dock_model_restores_closed_curves_without_duplicate_tabs() {
    let mut layout = WorkspaceLayout::default();
    assert!(layout.close(ToolPanel::Curves));
    assert!(!layout.is_visible(ToolPanel::Curves));
    assert!(layout.show(ToolPanel::Curves));
    assert!(layout.is_active(ToolPanel::Curves));
    let after = layout.clone();
    assert!(!layout.show(ToolPanel::Curves));
    assert_eq!(layout, after);
    let encoded = ron::to_string(&layout).unwrap();
    assert_eq!(ron::from_str::<WorkspaceLayout>(&encoded).unwrap(), layout);
}
#[test]
fn production_dock_model_preserves_floating_curves_when_revealed() {
    let mut layout = WorkspaceLayout::default();
    assert!(layout.float_panel(ToolPanel::Curves, [30.0, 50.0], [1280.0, 720.0]));
    let before = layout.clone();
    assert!(!layout.show(ToolPanel::Curves));
    assert!(layout.is_active(ToolPanel::Curves));
    assert_eq!(layout, before);
}
