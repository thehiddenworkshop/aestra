//! Production automation observers, presentation systems and global history on both engines.
use super::super::{execution, gestures, timeline_pointer::tests as pointer};
use super::*;
use crate::{
    ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
};
use aestra_core::{
    ColorKey, Curve, CurveKey, EffectParameter, EmitterRegionId, ModuleParameters, ParameterId,
    PropertyEvaluationDomain, PropertySource, PropertySourceValue, Vec3Curve,
};
use bevy::picking::pointer::PointerId;

#[derive(Clone, Copy)]
enum Flavor {
    Scalar,
    Gradient,
    Vector,
    Bound,
    SourceBank,
}
#[derive(Resource, Default)]
struct ParentEvents {
    clicks: usize,
    starts: usize,
    moves: usize,
    ends: usize,
}
struct Fixture {
    _root: tempfile::TempDir,
    app: App,
    window: Entity,
    canvas: Entity,
    lane: AutomationLaneId,
}
fn curve() -> Curve {
    Curve::new(vec![
        CurveKey::new(0.0, 1.0),
        CurveKey::new(0.5, 2.0),
        CurveKey::new(1.0, 3.0),
    ])
}
impl Fixture {
    fn new(flavor: Flavor, scale: f32) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut effect = crate::test_support::effect_with_timing_slack();
        let emitter = effect.emitters[0].id;
        let appearance = effect.emitters[0]
            .modules
            .iter_mut()
            .find(|m| matches!(m.parameters, ModuleParameters::Appearance { .. }))
            .unwrap();
        let ModuleParameters::Appearance { size, color, .. } = &mut appearance.parameters else {
            unreachable!()
        };
        *size = curve();
        color.keys = vec![
            ColorKey::new(0.0, [1.0, 0.0, 0.0, 1.0]),
            ColorKey::new(0.5, [0.0, 1.0, 0.0, 1.0]),
            ColorKey::new(1.0, [0.0, 0.0, 1.0, 1.0]),
        ];
        let mut lane = AutomationLaneId {
            emitter,
            module: appearance.id,
            input: 0,
            parameter: "size".into(),
            channel: None,
        };
        match flavor {
            Flavor::Scalar => {}
            Flavor::Gradient => {
                lane.parameter = "color".into();
                lane.input = 2;
            }
            Flavor::Bound => {
                let id = ParameterId::new();
                appearance.bindings.insert("size".into(), id);
                effect.parameters.push(EffectParameter {
                    id,
                    name: "Size".into(),
                    exposed: true,
                    default: Value::Curve(curve()),
                });
            }
            Flavor::SourceBank => {
                let source = PropertySource::Curve(PropertyEvaluationDomain::ParticleLife);
                appearance.property_sources.insert("size".into(), source);
                appearance.property_source_values.insert(
                    "size".into(),
                    vec![PropertySourceValue::new(source, Value::Curve(curve()))],
                );
            }
            Flavor::Vector => {
                let module = effect.emitters[0]
                    .modules
                    .iter_mut()
                    .find(|m| m.module_type.0 == aestra_core::MODULE_MOTION)
                    .unwrap();
                let source = PropertySource::Curve(PropertyEvaluationDomain::ParticleLife);
                module.property_sources.insert("gravity".into(), source);
                module.property_source_values.insert(
                    "gravity".into(),
                    vec![PropertySourceValue::new(
                        source,
                        Value::Vec3Curve(Vec3Curve {
                            curves: std::array::from_fn(|axis| {
                                Curve::new(vec![
                                    CurveKey::new(0.0, 0.1 + axis as f32 * 0.1),
                                    CurveKey::new(0.5, 0.2 + axis as f32 * 0.1),
                                    CurveKey::new(1.0, 0.3 + axis as f32 * 0.1),
                                ])
                            }),
                        }),
                    )],
                );
                lane.module = module.id;
                lane.parameter = "gravity".into();
                lane.channel = Some(1);
            }
        }
        let mut app = App::new();
        app.insert_resource(EditorSession::from_test_effect(effect))
            .insert_resource(TimelineState::framed(4.0))
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<CurvesState>()
            .init_resource::<ParentEvents>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<OverrideCursor>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<FunctionEditor>()
            .add_observer(execution::execute_timeline_action)
            .add_observer(execution::execute_choreography_action)
            .add_observer(
                |action: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*action, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history expected")
                    });
                },
            )
            .add_systems(
                Update,
                (
                    update_automation_lane_graph_visuals,
                    update_automation_key_visuals,
                    update_automation_curve_drag_preview,
                )
                    .chain(),
            );
        app.world_mut().resource_mut::<TimelineState>().snap = TimelineSnapMode::None;
        let mut window = Window::default();
        window.resolution.set_scale_factor_override(Some(scale));
        let window = app
            .world_mut()
            .spawn((window, PrimaryWindow, CursorIcon::default()))
            .id();
        let canvas = app
            .world_mut()
            .spawn((
                TimelineCanvas,
                ComputedNode {
                    size: Vec2::new(400.0, 100.0),
                    ..default()
                },
                RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::ZERO),
                },
            ))
            .observe(gestures::seek_timeline_on_drag)
            .observe(|_: On<PointerClick>, mut events: ResMut<ParentEvents>| events.clicks += 1)
            .observe(|_: On<PointerDragStart>, mut events: ResMut<ParentEvents>| events.starts += 1)
            .observe(|_: On<PointerDrag>, mut events: ResMut<ParentEvents>| events.moves += 1)
            .observe(|_: On<PointerDragEnd>, mut events: ResMut<ParentEvents>| events.ends += 1)
            .id();
        Self {
            _root: root,
            app,
            window,
            canvas,
            lane,
        }
    }
    fn scalar() -> Self {
        Self::new(Flavor::Scalar, 1.0)
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource::<EditorSession>()
    }
    fn state(&self) -> &TimelineState {
        self.app.world().resource::<TimelineState>()
    }
    fn data(&self) -> AutomationCurveData {
        automation_lane_keys(&self.session().effect, &self.lane)
            .unwrap()
            .graph_data()
    }
    fn value(&self) -> Value {
        automation_lane_value(&self.session().effect, &self.lane).unwrap()
    }
    fn key(&mut self, index: usize) -> Entity {
        self.app
            .world_mut()
            .spawn((
                TimelineAutomationKey,
                TimelineAutomationKeySelection {
                    lane: self.lane.clone(),
                    key: index,
                },
                Node::default(),
                ChildOf(self.canvas),
            ))
            .observe(select_automation_key)
            .observe(begin_automation_key_drag)
            .observe(move_automation_key_drag)
            .observe(finish_automation_key_drag)
            .id()
    }
    fn graph(&mut self, position: Option<Vec2>) -> (Entity, Entity) {
        let data = self.data();
        let graph = self
            .app
            .world_mut()
            .spawn((
                TimelineAutomationLaneGraph(self.lane.clone()),
                Node::default(),
                RelativeCursorPosition {
                    cursor_over: true,
                    normalized: position,
                },
                ChildOf(self.canvas),
            ))
            .observe(add_automation_key_from_graph)
            .id();
        self.app
            .world_mut()
            .commands()
            .entity(graph)
            .with_children(|parent| automation_curve::spawn_automation_curve(parent, &data));
        self.app.world_mut().flush();
        let raster = self
            .app
            .world()
            .get::<Children>(graph)
            .unwrap()
            .iter()
            .next()
            .unwrap();
        (graph, raster)
    }
    fn resize_handle(&mut self) -> (Entity, Entity) {
        let row = self
            .app
            .world_mut()
            .spawn((
                TimelineAutomationLane,
                self.lane.clone(),
                Node {
                    height: Val::Px(automation_curve::DEFAULT_HEIGHT),
                    ..default()
                },
            ))
            .id();
        let handle = self
            .app
            .world_mut()
            .spawn((
                TimelineAutomationLaneResizeHandle,
                self.lane.clone(),
                ChildOf(self.canvas),
            ))
            .observe(begin_automation_lane_resize)
            .observe(move_automation_lane_resize)
            .observe(finish_automation_lane_resize)
            .id();
        (handle, row)
    }
    fn action<E: Event>(&mut self, action: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(action);
        self.app.world_mut().flush();
    }
    fn click(&mut self, target: Entity, button: PointerButton, count: u8) {
        self.action(pointer::click(target, self.window, button, count));
    }
    fn begin(&mut self, target: Entity) {
        self.action(pointer::start(
            target,
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
    }
    fn motion(&mut self, target: Entity, x: f32, y: f32) {
        self.action(pointer::motion_delta(target, self.window, Vec2::new(x, y)));
    }
    fn end(&mut self, target: Entity) {
        self.action(pointer::end(
            target,
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
    }
    fn clean(&self, before: &EffectAsset) {
        assert_eq!(&self.session().effect, before);
        assert!(!self.session().dirty);
        assert!(!self.session().can_undo());
    }
    fn roundtrip(&mut self, before: &EffectAsset) {
        let after = self.session().effect.clone();
        assert_ne!(&after, before);
        self.action(HistoryAction::Undo);
        assert_eq!(&self.session().effect, before);
        assert!(
            !self.session().can_undo(),
            "Exactly one semantic release commit"
        );
        self.action(HistoryAction::Redo);
        assert_eq!(self.session().effect, after);
    }
}
fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
}

#[test]
fn primary_key_click_selects_real_timeline_and_curves_consumers_without_edit() {
    let mut f = Fixture::scalar();
    let target = f.key(1);
    let before = f.session().effect.clone();
    f.click(target, PointerButton::Primary, 1);
    assert_eq!(f.state().selected_automation_key.as_ref().unwrap().key, 1);
    assert_eq!(
        f.app
            .world()
            .resource::<CurvesState>()
            .selected_key()
            .unwrap()
            .module,
        f.lane.module
    );
    assert_eq!(f.app.world().resource::<ParentEvents>().clicks, 0);
    f.clean(&before);
}

#[test]
fn secondary_key_click_does_not_select_or_edit() {
    let mut f = Fixture::scalar();
    let target = f.key(1);
    let before = f.session().effect.clone();
    f.click(target, PointerButton::Secondary, 1);
    assert!(f.state().selected_automation_key.is_none());
    f.clean(&before);
}

#[test]
fn double_click_graph_inserts_projected_scalar_key_as_one_undoable_edit() {
    let mut f = Fixture::scalar();
    let (graph, _) = f.graph(Some(Vec2::new(-0.25, 0.0)));
    let before = f.session().effect.clone();
    f.click(graph, PointerButton::Primary, 2);
    let Value::Curve(curve) = f.value() else {
        unreachable!()
    };
    assert_eq!(curve.keys.len(), 4);
    assert!(curve.keys.iter().any(|key| key.time == 0.25));
    assert_eq!(f.app.world().resource::<ParentEvents>().clicks, 0);
    f.roundtrip(&before);
}

#[test]
fn graph_insert_requires_primary_double_click_and_known_cursor() {
    for cause in 0..3 {
        let mut f = Fixture::scalar();
        let (graph, _) = f.graph(if cause == 2 { None } else { Some(Vec2::ZERO) });
        let before = f.session().effect.clone();
        f.click(
            graph,
            if cause == 1 {
                PointerButton::Secondary
            } else {
                PointerButton::Primary
            },
            if cause == 0 { 1 } else { 2 },
        );
        f.clean(&before);
    }
}

#[test]
fn gradient_graph_double_click_samples_existing_colors_and_roundtrips() {
    let mut f = Fixture::new(Flavor::Gradient, 1.0);
    let Value::Gradient(original) = f.value() else {
        unreachable!()
    };
    let (graph, _) = f.graph(Some(Vec2::new(-0.25, 0.4)));
    let before = f.session().effect.clone();
    f.click(graph, PointerButton::Primary, 2);
    let Value::Gradient(edited) = f.value() else {
        unreachable!()
    };
    assert!(
        edited
            .keys
            .iter()
            .any(|key| key.time == 0.25 && key.color == original.sample(0.25))
    );
    f.roundtrip(&before);
}

#[test]
fn key_drag_previews_source_time_and_value_without_changing_document() {
    let mut f = Fixture::scalar();
    let target = f.key(1);
    let before = f.session().effect.clone();
    f.begin(target);
    near(
        f.state()
            .automation_key_drag
            .as_ref()
            .unwrap()
            .original_time,
        1.75,
    );
    f.motion(target, 40.0, -10.0);
    let drag = f.state().automation_key_drag.as_ref().unwrap();
    near(drag.current_time, 2.15);
    assert!(drag.current_value.unwrap() > 2.0);
    near(f.session().time(), 0.0);
    let parent = f.app.world().resource::<ParentEvents>();
    assert_eq!((parent.starts, parent.moves), (0, 0));
    f.clean(&before);
}

#[test]
fn scalar_release_commits_both_axes_once_and_selects_the_key() {
    let mut f = Fixture::scalar();
    let target = f.key(1);
    let before = f.session().effect.clone();
    f.begin(target);
    f.motion(target, 40.0, -10.0);
    let expected = f
        .state()
        .automation_key_drag
        .as_ref()
        .unwrap()
        .current_value
        .unwrap();
    f.end(target);
    let Value::Curve(curve) = f.value() else {
        unreachable!()
    };
    near(curve.keys[1].time, 0.66);
    near(curve.keys[1].value, expected);
    assert!(f.state().automation_key_drag.is_none());
    assert!(f.state().snap_guide.is_none());
    assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
    assert_eq!(f.state().selected_automation_key.as_ref().unwrap().key, 1);
    assert_eq!(f.app.world().resource::<ParentEvents>().ends, 0);
    f.roundtrip(&before);
}

#[test]
fn unchanged_release_has_no_history_or_implicit_key_selection() {
    let mut f = Fixture::scalar();
    let target = f.key(1);
    let before = f.session().effect.clone();
    f.begin(target);
    f.end(target);
    assert!(f.state().selected_automation_key.is_none());
    f.clean(&before);
}

#[test]
fn gradient_drag_changes_time_not_color_or_value() {
    let mut f = Fixture::new(Flavor::Gradient, 1.0);
    let target = f.key(1);
    let before = f.session().effect.clone();
    let Value::Gradient(original) = f.value() else {
        unreachable!()
    };
    f.begin(target);
    f.motion(target, 40.0, 1000.0);
    assert!(
        f.state()
            .automation_key_drag
            .as_ref()
            .unwrap()
            .current_value
            .is_none()
    );
    f.end(target);
    let Value::Gradient(edited) = f.value() else {
        unreachable!()
    };
    near(edited.keys[1].time, 0.66);
    assert_eq!(edited.keys[1].color, original.keys[1].color);
    assert_eq!(
        *f.app.world().get::<CursorIcon>(f.window).unwrap(),
        CursorIcon::System(SystemCursorIcon::EwResize)
    );
    f.roundtrip(&before);
}

#[test]
fn vector_drag_changes_only_selected_channel_and_selects_it_in_curves() {
    let mut f = Fixture::new(Flavor::Vector, 1.0);
    let target = f.key(1);
    let before = f.session().effect.clone();
    let Value::Vec3Curve(original) = f.value() else {
        unreachable!()
    };
    f.begin(target);
    f.motion(target, 40.0, -10.0);
    f.end(target);
    let Value::Vec3Curve(edited) = f.value() else {
        unreachable!()
    };
    assert_eq!(edited.curves[0], original.curves[0]);
    assert_eq!(edited.curves[2], original.curves[2]);
    near(edited.curves[1].keys[1].time, 0.66);
    assert_eq!(
        f.app
            .world()
            .resource::<CurvesState>()
            .selected_vector_channel(),
        1
    );
    f.roundtrip(&before);
}

#[test]
fn bound_parameter_drag_edits_parameter_without_rewriting_module() {
    let mut f = Fixture::new(Flavor::Bound, 1.0);
    let target = f.key(1);
    let before = f.session().effect.clone();
    f.begin(target);
    f.motion(target, 40.0, 0.0);
    f.end(target);
    assert_eq!(f.session().effect.emitters, before.emitters);
    assert_ne!(f.session().effect.parameters, before.parameters);
    f.roundtrip(&before);
}

#[test]
fn source_bank_drag_preserves_legacy_parameter_payload_and_source_identity() {
    let mut f = Fixture::new(Flavor::SourceBank, 1.0);
    let target = f.key(1);
    let before = f.session().effect.clone();
    f.begin(target);
    f.motion(target, 40.0, 0.0);
    f.end(target);
    let old = before.emitters[0]
        .modules
        .iter()
        .find(|m| m.id == f.lane.module)
        .unwrap();
    let edited = f.session().effect.emitters[0]
        .modules
        .iter()
        .find(|m| m.id == f.lane.module)
        .unwrap();
    assert_eq!(edited.parameters, old.parameters);
    assert_eq!(edited.property_sources, old.property_sources);
    assert_ne!(edited.property_source_values, old.property_source_values);
    f.roundtrip(&before);
}

#[test]
fn key_time_clamps_between_neighbors_with_epsilon() {
    let mut f = Fixture::scalar();
    let target = f.key(1);
    f.begin(target);
    f.motion(target, -10000.0, 0.0);
    near(
        f.state().automation_key_drag.as_ref().unwrap().current_time,
        0.50125,
    );
    f.motion(target, 10000.0, 0.0);
    near(
        f.state().automation_key_drag.as_ref().unwrap().current_time,
        2.99875,
    );
}

#[test]
fn motion_scales_by_window_width_view_span_and_lane_height() {
    let mut f = Fixture::new(Flavor::Scalar, 2.0);
    f.app.world_mut().spawn((
        TimelineCanvas,
        ComputedNode {
            size: Vec2::new(800.0, 100.0),
            ..default()
        },
    ));
    f.app.world_mut().resource_mut::<TimelineState>().view = TimelineView {
        start: 1.0,
        end: 3.0,
    };
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .automation_lane_heights
        .insert(f.lane.clone(), 144.0);
    let target = f.key(1);
    let graph = f.data();
    f.begin(target);
    f.motion(target, 100.0, 20.0);
    let drag = f.state().automation_key_drag.as_ref().unwrap();
    near(drag.current_time, 1.875);
    let expected = graph
        .value_for_top_percent(graph.top_percent_for_value(2.0).unwrap() + 10.0 / 144.0 * 100.0)
        .unwrap();
    near(drag.current_value.unwrap(), expected);
}

#[test]
fn selected_region_source_offset_drives_key_time_projection() {
    let mut f = Fixture::scalar();
    let mut effect = f.session().effect.clone();
    let emitter = &mut effect.emitters[0];
    let id = emitter.id;
    let first = emitter.implicit_region_id();
    let second = EmitterRegionId::new();
    emitter.regions = emitter.split_timeline_region(first, 1.5, second).unwrap();
    emitter.regions[1].start_time = 2.0;
    f.app
        .insert_resource(EditorSession::from_test_effect(effect));
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .select_only_emitter_region(id, second);
    let target = f.key(1);
    f.begin(target);
    let drag = f.state().automation_key_drag.as_ref().unwrap();
    near(drag.source_start, 1.0);
    near(drag.original_time, 2.25);
}

#[test]
fn stale_lane_or_key_drag_start_is_ignored() {
    for cause in 0..3 {
        let mut f = Fixture::scalar();
        let target = f.key(if cause == 0 { 99 } else { 1 });
        if cause == 1 {
            f.app
                .world_mut()
                .get_mut::<TimelineAutomationKeySelection>(target)
                .unwrap()
                .lane
                .module = aestra_core::ModuleId::new();
        }
        if cause == 2 {
            f.app
                .world_mut()
                .get_mut::<TimelineAutomationKeySelection>(target)
                .unwrap()
                .lane
                .emitter = EmitterId::new();
        }
        f.begin(target);
        assert!(f.state().automation_key_drag.is_none());
        assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
    }
}

#[test]
fn mismatched_key_motion_and_release_preserve_active_gesture() {
    let mut f = Fixture::scalar();
    let target = f.key(1);
    let wrong = f.key(0);
    let before = f.session().effect.clone();
    f.begin(target);
    f.motion(wrong, 40.0, 0.0);
    f.end(wrong);
    near(
        f.state().automation_key_drag.as_ref().unwrap().current_time,
        1.75,
    );
    f.clean(&before);
    f.motion(target, 40.0, 0.0);
    f.end(target);
    f.roundtrip(&before);
}

#[test]
fn stale_value_at_release_is_safe_without_semantic_commit() {
    let mut f = Fixture::scalar();
    let target = f.key(1);
    f.begin(target);
    f.motion(target, 40.0, 0.0);
    let mut effect = f.session().effect.clone();
    let m = effect.emitters[0]
        .modules
        .iter_mut()
        .find(|m| m.id == f.lane.module)
        .unwrap();
    let ModuleParameters::Appearance { size, .. } = &mut m.parameters else {
        unreachable!()
    };
    size.keys.truncate(1);
    f.app
        .insert_resource(EditorSession::from_test_effect(effect.clone()));
    f.end(target);
    assert!(f.state().automation_key_drag.is_none());
    f.clean(&effect);
}

#[test]
fn frame_seconds_and_smart_snapping_publish_existing_guides() {
    for mode in [
        TimelineSnapMode::Frames,
        TimelineSnapMode::Seconds,
        TimelineSnapMode::Smart,
    ] {
        let mut f = Fixture::scalar();
        f.app.world_mut().resource_mut::<TimelineState>().snap = mode;
        let target = f.key(1);
        f.begin(target);
        f.motion(target, 12.3, 0.0);
        let interval = if mode == TimelineSnapMode::Seconds {
            nice_timeline_step(4.0, 400.0) / 5.0
        } else {
            1.0 / f.session().tick_rate() as f32
        };
        let expected = (1.873 / interval).round() * interval;
        near(
            f.state().automation_key_drag.as_ref().unwrap().current_time,
            expected,
        );
        assert_eq!(f.state().snap_guide, Some(expected));
    }
}

#[test]
fn curve_preview_updates_real_raster_and_key_nodes_without_semantic_edit() {
    let mut f = Fixture::scalar();
    let (graph, raster) = f.graph(Some(Vec2::ZERO));
    let target = f.key(1);
    let before = f.session().effect.clone();
    let original = f.data();
    f.begin(target);
    f.motion(target, 40.0, -10.0);
    f.app.update();
    let drag = f.state().automation_key_drag.as_ref().unwrap();
    let preview = automation_key_drag_preview_data(&before, drag).unwrap();
    assert_ne!(preview, original);
    assert_eq!(
        f.app
            .world()
            .get::<automation_curve::AutomationCurveRaster>(raster)
            .unwrap()
            .data(),
        &preview
    );
    let node = f.app.world().get::<Node>(target).unwrap();
    assert_eq!(node.display, Display::Flex);
    assert_eq!(
        node.left,
        Val::Percent(
            f.state()
                .view
                .normalized_time(drag.current_time)
                .clamp(0.0, 1.0)
                * 100.0
        )
    );
    assert_eq!(node.top, Val::Percent(preview.key_top_percent(1)));
    assert_eq!(
        f.app.world().get::<Node>(graph).unwrap().left,
        Val::Percent(12.5)
    );
    f.clean(&before);
}

#[test]
fn gradient_preview_changes_only_time_and_does_not_update_key_vertical_position() {
    let mut f = Fixture::new(Flavor::Gradient, 1.0);
    let (_, raster) = f.graph(Some(Vec2::ZERO));
    let target = f.key(1);
    f.app.world_mut().get_mut::<Node>(target).unwrap().top = Val::Percent(37.0);
    f.begin(target);
    f.motion(target, 40.0, 30.0);
    f.app.update();
    let AutomationCurveData::Gradient(points) = f
        .app
        .world()
        .get::<automation_curve::AutomationCurveRaster>(raster)
        .unwrap()
        .data()
    else {
        unreachable!()
    };
    near(points[1].time, 0.66);
    assert_eq!(points[1].color, [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(
        f.app.world().get::<Node>(target).unwrap().top,
        Val::Percent(37.0)
    );
}

#[test]
fn missing_emitter_and_outside_keys_hide_actual_visual_nodes() {
    let mut f = Fixture::scalar();
    let (graph, _) = f.graph(Some(Vec2::ZERO));
    let target = f.key(1);
    f.app.world_mut().resource_mut::<TimelineState>().view = TimelineView {
        start: 3.2,
        end: 4.0,
    };
    f.app.update();
    assert_eq!(
        f.app.world().get::<Node>(graph).unwrap().display,
        Display::None
    );
    assert_eq!(
        f.app.world().get::<Node>(target).unwrap().display,
        Display::None
    );
    f.app
        .world_mut()
        .get_mut::<TimelineAutomationLaneGraph>(graph)
        .unwrap()
        .0
        .emitter = EmitterId::new();
    f.app.update();
    assert_eq!(
        f.app.world().get::<Node>(graph).unwrap().display,
        Display::None
    );
}

#[test]
fn graph_geometry_keeps_source_window_and_view_intersection_policy() {
    let mut node = Node::default();
    let view = TimelineView {
        start: 1.0,
        end: 3.0,
    };
    apply_automation_graph_geometry(&mut node, 0.5, 2.5, view);
    assert_eq!(node.display, Display::Flex);
    assert_eq!(node.left, Val::Percent(-25.0));
    assert_eq!(node.width, Val::Percent(125.0));
    apply_automation_graph_geometry(&mut node, 3.0, 2.5, view);
    assert_eq!(node.display, Display::None);
}

#[test]
fn lane_resize_updates_matching_nodes_with_scaled_height_but_no_history() {
    let mut f = Fixture::new(Flavor::Scalar, 2.0);
    let (handle, row) = f.resize_handle();
    let before = f.session().effect.clone();
    f.begin(handle);
    f.motion(handle, 0.0, 100.0);
    near(f.state().automation_lane_height(&f.lane), 122.0);
    assert_eq!(
        f.app.world().get::<Node>(row).unwrap().height,
        Val::Px(122.0)
    );
    let revision = f.session().ui_revision;
    f.end(handle);
    assert!(f.state().automation_lane_resize.is_none());
    assert_eq!(f.session().ui_revision, revision + 1);
    assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
    near(f.session().time(), 0.0);
    let events = f.app.world().resource::<ParentEvents>();
    assert_eq!((events.starts, events.moves, events.ends), (0, 0, 0));
    f.clean(&before);
}

#[test]
fn lane_resize_clamps_minimum_and_maximum_height() {
    let mut f = Fixture::scalar();
    let (handle, _) = f.resize_handle();
    f.begin(handle);
    f.motion(handle, 0.0, -1000.0);
    near(
        f.state().automation_lane_height(&f.lane),
        automation_curve::MIN_HEIGHT,
    );
    f.motion(handle, 0.0, 1000.0);
    near(
        f.state().automation_lane_height(&f.lane),
        automation_curve::MAX_HEIGHT,
    );
}

#[test]
fn lane_resize_starts_from_saved_height_and_ignores_mismatched_motion() {
    let mut f = Fixture::scalar();
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .automation_lane_heights
        .insert(f.lane.clone(), 100.0);
    let (handle, _) = f.resize_handle();
    let (wrong, _) = f.resize_handle();
    f.app
        .world_mut()
        .get_mut::<AutomationLaneId>(wrong)
        .unwrap()
        .parameter = "opacity".into();
    f.begin(handle);
    f.motion(wrong, 0.0, 50.0);
    near(f.state().automation_lane_height(&f.lane), 100.0);
    f.motion(handle, 0.0, 20.0);
    near(f.state().automation_lane_height(&f.lane), 120.0);
}

#[test]
fn mixed_automation_pointer_and_timeline_edits_follow_global_history_order() {
    let mut f = Fixture::scalar();
    let target = f.key(1);
    let before = f.session().effect.clone();
    f.begin(target);
    f.motion(target, 40.0, 0.0);
    f.end(target);
    let moved = f.session().effect.clone();
    f.action(super::super::TimelineAction::AddMarker);
    let both = f.session().effect.clone();
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, moved);
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, before);
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, moved);
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, both);
}
