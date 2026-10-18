//! Constructed Curves panel and its actual pointer observers on both engines.
use super::super::{
    EditorCurvesPlugin, control_input,
    host_composition::{Graph as HostGraph, Key as HostKey},
    pointer::tests as pointer,
    state::Selection,
};
use super::*;
use crate::{
    ProjectEffectCatalog, ToolPanel, WorkspaceLayout,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
    test_support,
    timeline::TimelineState,
};
use aestra_core::{
    ColorKey, Curve, CurveKey, EffectAsset, EmitterTransform, Gradient, HostTransformKey,
    HostTransformTrack, PropertyEvaluationDomain, PropertySource, PropertySourceValue, ScalarRange,
    Vec3Curve,
};
use bevy::{
    asset::AssetPlugin,
    input_focus::InputFocus,
    picking::pointer::PointerId,
    scene::ScenePlugin,
    text::TextPlugin,
    ui_widgets::{Activate, ScrollArea, Scrollbar},
};

#[derive(Clone, Copy)]
enum Mode {
    Scalar,
    Normalized,
    Vector,
    Gradient,
    Host(u8),
    None,
    Missing,
    Hidden,
    Empty,
}
#[derive(Resource)]
struct Built(Entity);
fn build(
    mut commands: Commands,
    session: Res<EditorSession>,
    registry: Res<EditorModuleRegistry>,
    state: Res<CurvesState>,
    localizer: Res<Localizer>,
) {
    let root = commands
        .spawn(Node::default())
        .with_children(|p| spawn_curves_workspace(p, &session, &registry, &state, &localizer))
        .id();
    commands.insert_resource(Built(root));
}
struct Fixture {
    app: App,
    selection: Option<ComplexSelection>,
    before: EffectAsset,
    _root: tempfile::TempDir,
    path: std::path::PathBuf,
    bytes: Vec<u8>,
}
impl Fixture {
    fn new(mode: Mode) -> Self {
        Self::localized(mode, "en-US")
    }
    fn localized(mode: Mode, locale: &str) -> Self {
        let mut effect = if matches!(mode, Mode::Empty) {
            EffectAsset::new("Empty", 4.0)
        } else {
            test_support::effect_with_timing_slack()
        };
        let registry = EditorModuleRegistry::default();
        let mut state = CurvesState::default();
        let mut selection = None;
        if !effect.emitters.is_empty() {
            let parameter = match mode {
                Mode::Vector => "gravity",
                Mode::Gradient => "color",
                _ => "size",
            };
            let module = effect.emitters[0]
                .modules
                .iter_mut()
                .find(|m| m.parameter_value(parameter).is_some())
                .unwrap();
            let input = registry
                .0
                .get(&module.module_type)
                .unwrap()
                .inputs
                .iter()
                .position(|i| i.name == parameter)
                .unwrap() as u8;
            let curve = || {
                Curve::new(vec![
                    CurveKey::new(0.0, 0.2),
                    CurveKey::new(0.5, 0.5),
                    CurveKey::new(1.0, 0.8),
                ])
            };
            let value = match mode {
                Mode::Normalized => Value::Curve(Curve::normalized(
                    curve().keys,
                    ScalarRange::new(10.0, 30.0),
                )),
                Mode::Vector => Value::Vec3Curve(Vec3Curve {
                    curves: [curve(), curve(), curve()],
                }),
                Mode::Gradient => Value::Gradient(Gradient::new(vec![
                    ColorKey::new(0.0, [0.2; 4]),
                    ColorKey::new(0.5, [0.5; 4]),
                    ColorKey::new(1.0, [0.8; 4]),
                ])),
                _ => Value::Curve(curve()),
            };
            let source = if matches!(mode, Mode::Gradient) {
                PropertySource::Gradient(PropertyEvaluationDomain::ParticleLife)
            } else {
                PropertySource::Curve(PropertyEvaluationDomain::ParticleLife)
            };
            module.property_sources.insert(
                parameter.into(),
                if matches!(mode, Mode::Hidden) {
                    PropertySource::Constant
                } else {
                    source
                },
            );
            module.property_source_values.insert(
                parameter.into(),
                vec![PropertySourceValue::new(source, value)],
            );
            let chosen = ComplexSelection {
                module: module.id,
                input: if matches!(mode, Mode::Missing) {
                    255
                } else {
                    input
                },
                key: 1,
            };
            selection = Some(chosen);
            state.complex = if matches!(mode, Mode::None | Mode::Host(_)) {
                None
            } else {
                Some(chosen)
            };
            state.vector_channel = if matches!(mode, Mode::Vector) { 1 } else { 0 };
        }
        if let Mode::Host(channel) = mode {
            effect.host_transform_track = Some(HostTransformTrack::from_pose_keys(
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
            state.host = Some(Selection {
                effect: effect.id,
                channel,
                key: 1,
            });
        }
        let before = effect.clone();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("curves.aestra.ron");
        before.save_ron(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut session = EditorSession::from_test_effect(effect);
        session.playing = false;
        let mut layout = WorkspaceLayout::default();
        layout.show(ToolPanel::Curves);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<crate::feathers::icon::SvgFile>()
        .insert_resource(session)
        .insert_resource(registry)
        .insert_resource(state)
        .insert_resource(layout)
        .insert_resource(Localizer::new(locale).unwrap())
        .insert_resource(ProjectEffectCatalog::scan(root.path()))
        .init_resource::<TimelineState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputFocus>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<FunctionEditor>()
        .add_plugins(EditorCurvesPlugin)
        .add_observer(|e: On<HistoryAction>, r: Routing, mut c: Commands| {
            r.execute(*e, &mut c, |_, _, _, _| {
                panic!("No filesystem history expected")
            })
        })
        .add_systems(Startup, build);
        app.update();
        Self {
            app,
            selection,
            before,
            _root: root,
            path,
            bytes,
        }
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource()
    }
    fn root(&self) -> Entity {
        self.app.world().resource::<Built>().0
    }
    fn event<E: Event>(&mut self, e: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(e);
        self.app.world_mut().flush();
    }
    fn graph(&mut self) -> Entity {
        let w = self.app.world_mut();
        w.query_filtered::<Entity, With<CurveGraph>>()
            .single(w)
            .unwrap()
    }
    fn host_graph(&mut self) -> Entity {
        let w = self.app.world_mut();
        w.query_filtered::<Entity, With<HostGraph>>()
            .single(w)
            .unwrap()
    }
    fn key(&mut self, index: usize) -> Entity {
        let w = self.app.world_mut();
        w.query::<(Entity, &CurveGraphKey)>()
            .iter(w)
            .find(|(_, k)| k.0.key == index)
            .unwrap()
            .0
    }
    fn host_key(&mut self, index: usize) -> Entity {
        let w = self.app.world_mut();
        w.query::<(Entity, &HostKey)>()
            .iter(w)
            .find(|(_, k)| k.0 == index)
            .unwrap()
            .0
    }
    fn raster(&mut self) -> AutomationCurveData {
        let w = self.app.world_mut();
        w.query::<&automation_curve::AutomationCurveRaster>()
            .single(w)
            .unwrap()
            .data()
            .clone()
    }
    fn texts(&mut self) -> Vec<String> {
        let w = self.app.world_mut();
        w.query::<&Text>().iter(w).map(|t| t.0.clone()).collect()
    }
    fn actions(&mut self) -> Vec<CurvesAction> {
        let w = self.app.world_mut();
        w.query::<&CurvesAction>().iter(w).copied().collect()
    }
    fn control(&mut self, action: CurvesAction) -> Entity {
        let w = self.app.world_mut();
        w.query::<(Entity, &CurvesAction)>()
            .iter(w)
            .find(|(_, a)| **a == action)
            .unwrap()
            .0
    }
    fn geometry(&mut self, host: bool, scale: f32) {
        let graph = if host {
            self.host_graph()
        } else {
            self.graph()
        };
        self.app.world_mut().entity_mut(graph).insert(ComputedNode {
            size: Vec2::new(200.0, 100.0) * scale,
            inverse_scale_factor: 1.0 / scale,
            ..default()
        });
    }
    fn click(&mut self, key: Entity, button: PointerButton) {
        self.event(pointer::click(key, Entity::PLACEHOLDER, button, 1));
    }
    fn start(&mut self, key: Entity, button: PointerButton) {
        self.event(pointer::start(
            key,
            Entity::PLACEHOLDER,
            Vec2::ZERO,
            PointerId::Mouse,
            button,
        ));
    }
    fn drag(&mut self, key: Entity, distance: Vec2) {
        self.event(pointer::motion_delta(key, Entity::PLACEHOLDER, distance));
    }
    fn end(&mut self, key: Entity, distance: Vec2, button: PointerButton) {
        self.event(pointer::end_distance(
            key,
            Entity::PLACEHOLDER,
            distance,
            button,
        ));
    }
    fn history(&mut self, a: HistoryAction) {
        self.event(a);
    }
    fn unchanged(&self) {
        assert_eq!(self.session().effect, self.before);
        assert_eq!(std::fs::read(&self.path).unwrap(), self.bytes);
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
        self.unchanged();
    }
    fn value(&self) -> Value {
        resolve_complex_input(
            self.session(),
            self.app.world().resource(),
            self.selection.unwrap(),
        )
        .unwrap()
        .2
    }
}

#[test]
fn scalar_panel_constructs_real_graph_keys_controls_and_scroll_targets() {
    let mut f = Fixture::new(Mode::Scalar);
    let g = f.graph();
    assert!(f.app.world().get::<RelativeCursorPosition>(g).is_some());
    let w = f.app.world_mut();
    assert_eq!(w.query::<&CurveGraphKey>().iter(w).count(), 3);
    assert_eq!(w.query::<&CurveGraphValueLabel>().iter(w).count(), 3);
    assert_eq!(w.query::<&ScrollArea>().iter(w).count(), 1);
    for bar in w.query::<&Scrollbar>().iter(w) {
        assert!(w.get::<ScrollArea>(bar.target).is_some());
    }
    let actions = f.actions();
    assert!(actions.contains(&CurvesAction::AddKey));
    assert!(actions.contains(&CurvesAction::DeleteKey));
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a, CurvesAction::SetInterpolation(_)))
            .count(),
        3
    );
    f.unchanged();
}
#[test]
fn empty_missing_hidden_and_unselected_panels_use_localized_feedback() {
    for mode in [Mode::Empty, Mode::None, Mode::Missing, Mode::Hidden] {
        for locale in ["en-US", "fr-FR"] {
            let mut f = Fixture::localized(mode, locale);
            let id = if matches!(mode, Mode::Missing) {
                "curves-property-missing"
            } else {
                "curves-choose-property"
            };
            let expected = f.app.world().resource::<Localizer>().text(id);
            assert!(f.texts().contains(&expected));
            let w = f.app.world_mut();
            assert_eq!(w.query::<&CurveGraph>().iter(w).count(), 0);
            f.unchanged();
        }
    }
}
#[test]
fn normalized_panel_labels_show_output_units_and_keys_keep_normalized_data() {
    let mut f = Fixture::new(Mode::Normalized);
    let labels = f.texts();
    assert!(labels.contains(&"10".into()));
    assert!(labels.contains(&"30".into()));
    let AutomationCurveData::Curve {
        value_bounds,
        points,
        ..
    } = f.raster()
    else {
        unreachable!()
    };
    assert_eq!(value_bounds, Some((0.0, 1.0)));
    assert_eq!(points[1].value, 0.5);
    f.unchanged();
}
#[test]
fn vector_panel_constructs_xyz_selectors_for_selected_channel() {
    let mut f = Fixture::new(Mode::Vector);
    for i in 0..3 {
        let e = f.control(CurvesAction::SelectVectorChannel(i));
        assert!(f.app.world().get::<EditorNativeControl>(e).is_some());
    }
    let e = f.control(CurvesAction::SelectVectorChannel(2));
    control_input::set_phase(f.app.world_mut(), e, super::super::controls::Phase::Pressed);
    f.app.update();
    assert_eq!(
        f.app
            .world()
            .resource::<CurvesState>()
            .selected_vector_channel(),
        2
    );
    f.unchanged();
}
#[test]
fn constructed_feathers_add_button_reaches_real_dispatch_and_global_history() {
    let mut f = Fixture::new(Mode::Scalar);
    let e = f.control(CurvesAction::AddKey);
    assert!(
        f.app
            .world()
            .get::<crate::feathers::action_activation::FeathersActionButton>(e)
            .is_some()
    );
    assert_eq!(f.app.world().get::<Node>(e).unwrap().width, Val::Px(56.0));
    f.event(Activate { entity: e });
    f.app.update();
    f.round_trip();
}
#[test]
fn constructed_property_list_native_button_opens_actual_selection() {
    let mut f = Fixture::new(Mode::None);
    let s = f.selection.unwrap();
    let e = f.control(CurvesAction::OpenInput(s.module, s.input));
    control_input::set_phase(f.app.world_mut(), e, super::super::controls::Phase::Pressed);
    f.app.update();
    assert_eq!(
        f.app
            .world()
            .resource::<CurvesState>()
            .selected_key()
            .unwrap()
            .key,
        0
    );
    f.unchanged();
}
#[test]
fn scalar_key_click_selects_only_on_primary_without_semantic_edit() {
    let mut f = Fixture::new(Mode::Scalar);
    let e = f.key(2);
    f.click(e, PointerButton::Secondary);
    assert_eq!(
        f.app
            .world()
            .resource::<CurvesState>()
            .selected_key()
            .unwrap()
            .key,
        1
    );
    f.click(e, PointerButton::Primary);
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
fn scalar_drag_updates_raster_nodes_and_label_without_mutating_then_commits_once() {
    for scale in [1.0, 2.0] {
        let mut f = Fixture::new(Mode::Scalar);
        f.geometry(false, scale);
        let e = f.key(1);
        let initial = f.raster();
        f.drag(e, Vec2::new(20.0, -5.0));
        assert_ne!(f.raster(), initial);
        let Val::Percent(left) = f.app.world().get::<Node>(e).unwrap().left else {
            unreachable!()
        };
        assert!((left - 60.0).abs() < 0.001);
        let w = f.app.world_mut();
        assert_eq!(
            w.query::<(&CurveGraphValueLabel, &Visibility)>()
                .iter(w)
                .filter(|(_, v)| **v == Visibility::Visible)
                .count(),
            1
        );
        f.unchanged();
        f.end(e, Vec2::new(20.0, -5.0), PointerButton::Primary);
        let Value::Curve(c) = f.value() else {
            unreachable!()
        };
        assert!((c.keys[1].time - 0.6).abs() < 0.00001);
        let w = f.app.world_mut();
        assert!(
            w.query::<(&CurveGraphValueLabel, &Visibility)>()
                .iter(w)
                .all(|(_, v)| *v == Visibility::Hidden)
        );
        f.round_trip();
    }
}
#[test]
fn vector_key_drag_commits_only_the_selected_channel() {
    let mut f = Fixture::new(Mode::Vector);
    f.geometry(false, 1.0);
    let Value::Vec3Curve(before) = f.value() else {
        unreachable!()
    };
    let e = f.key(1);
    f.drag(e, Vec2::new(20.0, 0.0));
    f.unchanged();
    f.end(e, Vec2::new(20.0, 0.0), PointerButton::Primary);
    let Value::Vec3Curve(after) = f.value() else {
        unreachable!()
    };
    assert_eq!(after.curves[0], before.curves[0]);
    assert_eq!(after.curves[2], before.curves[2]);
    assert_ne!(after.curves[1], before.curves[1]);
    f.round_trip();
}
#[test]
fn secondary_scalar_release_cannot_commit_a_preview() {
    let mut f = Fixture::new(Mode::Scalar);
    f.geometry(false, 1.0);
    let e = f.key(1);
    f.drag(e, Vec2::new(20.0, 0.0));
    f.end(e, Vec2::new(20.0, 0.0), PointerButton::Secondary);
    f.unchanged();
    f.history(HistoryAction::Undo);
    f.unchanged();
}
#[test]
fn gradient_panel_constructs_rgba_controls_and_attached_key_handles() {
    let mut f = Fixture::new(Mode::Gradient);
    let graph = f.graph();
    assert_eq!(
        f.app.world().get::<Node>(graph).unwrap().height,
        Val::Px(82.0)
    );
    for channel in 0..4 {
        assert!(
            f.actions()
                .contains(&CurvesAction::AdjustGradientChannel(channel, 1))
        );
    }
    let AutomationCurveData::Gradient(points) = f.raster() else {
        unreachable!()
    };
    assert_eq!(points.len(), 3);
    f.unchanged();
}
#[test]
fn gradient_drag_uses_attached_observer_and_preserves_color() {
    let mut f = Fixture::new(Mode::Gradient);
    f.geometry(false, 2.0);
    let graph = f.graph();
    let e = f
        .app
        .world()
        .get::<Children>(graph)
        .unwrap()
        .iter()
        .filter(|e| f.app.world().get::<EditorNativeControl>(*e).is_some())
        .nth(1)
        .unwrap();
    f.drag(e, Vec2::new(20.0, 50.0));
    f.unchanged();
    f.end(e, Vec2::new(20.0, 50.0), PointerButton::Primary);
    let Value::Gradient(g) = f.value() else {
        unreachable!()
    };
    assert!((g.keys[1].time - 0.6).abs() < 0.00001);
    assert_eq!(g.keys[1].color, [0.5; 4]);
    f.round_trip();
}
#[test]
fn host_panel_constructs_ten_channels_and_actual_host_controls() {
    let mut f = Fixture::new(Mode::Host(0));
    let _ = f.host_graph();
    let actions = f.actions();
    assert_eq!(
        actions
            .iter()
            .filter(|a| matches!(a, CurvesAction::OpenHost(_)))
            .count(),
        10
    );
    assert!(actions.contains(&CurvesAction::HostAddKey));
    assert!(actions.contains(&CurvesAction::HostDeleteKey));
    let w = f.app.world_mut();
    assert_eq!(w.query::<&HostKey>().iter(w).count(), 3);
    assert_eq!(w.query::<&CurveGraph>().iter(w).count(), 0);
    f.unchanged();
}
#[test]
fn host_drag_is_temporary_until_release_and_trailing_click_is_consumed() {
    for scale in [1.0, 2.0] {
        let mut f = Fixture::new(Mode::Host(0));
        f.geometry(true, scale);
        let e = f.host_key(1);
        let before = f.raster();
        f.start(e, PointerButton::Primary);
        f.drag(e, Vec2::new(20.0, -5.0));
        assert_ne!(f.raster(), before);
        f.unchanged();
        assert!(f.app.world().resource::<CurvesState>().host_drag.is_some());
        f.end(e, Vec2::ZERO, PointerButton::Primary);
        let after = f.session().effect.clone();
        assert!(f.app.world().resource::<CurvesState>().host_drag.is_none());
        f.click(e, PointerButton::Primary);
        assert_eq!(f.session().effect, after);
        assert!(
            f.app
                .world()
                .resource::<CurvesState>()
                .host_released
                .is_none()
        );
        f.round_trip();
    }
}
#[test]
fn quaternion_host_drag_retimes_linked_key_without_changing_orientations() {
    let mut f = Fixture::new(Mode::Host(6));
    f.geometry(true, 1.0);
    let original = f.before.host_transform_track.as_ref().unwrap().clone();
    let e = f.host_key(1);
    f.start(e, PointerButton::Primary);
    f.drag(e, Vec2::new(20.0, 50.0));
    f.end(e, Vec2::ZERO, PointerButton::Primary);
    let track = f.session().effect.host_transform_track.as_ref().unwrap();
    assert_eq!(track.curves.translation, original.curves.translation);
    assert_eq!(
        track.curves.rotation.keys[1].value,
        original.curves.rotation.keys[1].value
    );
    assert!((track.curves.rotation.keys[1].time - 1.2).abs() < 0.00001);
    f.round_trip();
}
#[test]
fn escape_cancels_constructed_host_drag_and_late_release_does_not_commit() {
    let mut f = Fixture::new(Mode::Host(0));
    f.geometry(true, 1.0);
    let e = f.host_key(1);
    f.start(e, PointerButton::Primary);
    f.drag(e, Vec2::new(20.0, -5.0));
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    f.app.update();
    assert!(f.app.world().resource::<CurvesState>().host_drag.is_none());
    f.end(e, Vec2::ZERO, PointerButton::Primary);
    f.history(HistoryAction::Undo);
    f.unchanged();
}
#[test]
fn secondary_host_pointer_cannot_start_or_commit_a_drag() {
    let mut f = Fixture::new(Mode::Host(0));
    f.geometry(true, 1.0);
    let e = f.host_key(1);
    f.start(e, PointerButton::Secondary);
    f.drag(e, Vec2::new(20.0, -5.0));
    f.end(e, Vec2::ZERO, PointerButton::Secondary);
    assert!(f.app.world().resource::<CurvesState>().host_drag.is_none());
    f.unchanged();
}
#[test]
fn retained_panel_is_stable_on_idle_and_despawning_root_retires_all_children() {
    let mut f = Fixture::new(Mode::Host(0));
    let root = f.root();
    let graph = f.host_graph();
    let count = f.app.world().entities().len();
    for _ in 0..3 {
        f.app.update();
    }
    assert_eq!(f.host_graph(), graph);
    assert_eq!(f.app.world().entities().len(), count);
    f.app.world_mut().despawn(root);
    let w = f.app.world_mut();
    assert_eq!(w.query::<&HostKey>().iter(w).count(), 0);
    assert_eq!(w.query::<&CurvesAction>().iter(w).count(), 0);
    assert_eq!(
        w.query::<&automation_curve::AutomationCurveRaster>()
            .iter(w)
            .count(),
        0
    );
    f.unchanged();
}
