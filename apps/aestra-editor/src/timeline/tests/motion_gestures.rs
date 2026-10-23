//! Actual host-pose observers with upstream pointers, real previews and global history.
use super::super::{motion_execution, timeline_pointer::tests as pointer};
use super::*;
use crate::{
    CurvesState, ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    localization::Localizer,
    material_function_editor::FunctionEditor,
};
use aestra_core::{EffectAsset, EmitterTransform, HostTransformKey, HostTransformTrack};
use bevy::picking::pointer::PointerId;

fn track() -> HostTransformTrack {
    HostTransformTrack::from_pose_keys(
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
    )
}

struct Fixture {
    _root: tempfile::TempDir,
    app: App,
    window: Entity,
    canvas: Entity,
    keys: [Entity; 3],
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut effect = crate::test_support::effect_with_timing_slack();
        effect.host_transform_track = Some(track());
        let id = effect.id;
        let mut state = TimelineState::framed(4.0);
        state.snap = super::super::TimelineSnapMode::None;
        let mut app = App::new();
        app.insert_resource(EditorSession::from_test_effect(effect))
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .insert_resource(Localizer::new("en-US").unwrap())
            .insert_resource(state)
            .init_resource::<CurvesState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<FunctionEditor>()
            .add_observer(motion_execution::execute)
            .add_observer(motion_execution::number_changed)
            .add_observer(
                |action: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*action, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history expected")
                    });
                },
            )
            .add_systems(Update, (motion_execution::cancel_drag, update_keys).chain());
        let window = app.world_mut().spawn(Window::default()).id();
        let canvas = app
            .world_mut()
            .spawn((
                TimelineCanvas,
                ComputedNode {
                    size: Vec2::new(400.0, 100.0),
                    ..default()
                },
            ))
            .id();
        let keys = std::array::from_fn(|index| {
            app.world_mut()
                .spawn((
                    KeyControl { effect: id, index },
                    Node::default(),
                    BackgroundColor(theme::ACCENT_DIM),
                    ChildOf(canvas),
                ))
                .observe(click_key)
                .observe(begin_drag)
                .observe(drag_key)
                .observe(end_drag)
                .id()
        });
        Self {
            _root: root,
            app,
            window,
            canvas,
            keys,
        }
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource::<EditorSession>()
    }
    fn state(&self) -> &TimelineState {
        self.app.world().resource::<TimelineState>()
    }
    fn action<E: Event>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn begin(&mut self, index: usize) {
        self.action(pointer::start(
            self.keys[index],
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
    }
    fn move_by(&mut self, distance: f32) {
        self.action(pointer::motion_delta(
            self.keys[1],
            self.window,
            Vec2::new(distance, 0.0),
        ));
    }
    fn end(&mut self) {
        self.action(pointer::end(
            self.keys[1],
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
    }
    fn candidate_time(&self) -> f32 {
        let drag = self.state().host_motion.drag.as_ref().unwrap();
        drag.candidate.keys()[drag.selected].time
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
            "One release must create one edit"
        );
        self.action(HistoryAction::Redo);
        assert_eq!(self.session().effect, after);
    }
}
fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
}

#[test]
fn primary_click_selects_without_editing() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    f.action(pointer::click(
        f.keys[2],
        f.window,
        PointerButton::Primary,
        1,
    ));
    assert_eq!(f.state().host_motion.selected, Some(2));
    assert!(active(f.session(), f.state()));
    f.clean(&before);
}

#[test]
fn secondary_events_do_not_select_start_move_or_release() {
    let mut f = Fixture::new();
    f.action(pointer::click(
        f.keys[1],
        f.window,
        PointerButton::Secondary,
        1,
    ));
    f.action(pointer::start(
        f.keys[1],
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Secondary,
    ));
    assert!(f.state().host_motion.selected.is_none());
    assert!(f.state().host_motion.drag.is_none());
    f.begin(1);
    f.action(pointer::motion_button(
        f.keys[1],
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Secondary,
    ));
    f.action(pointer::end(
        f.keys[1],
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Secondary,
    ));
    near(f.candidate_time(), 1.0);
    assert!(f.state().host_motion.released_key.is_none());
}

#[test]
fn anchor_cannot_begin_retiming() {
    let mut f = Fixture::new();
    f.begin(0);
    assert!(f.state().host_motion.drag.is_none());
    assert!(f.state().host_motion.selected.is_none());
}

#[test]
fn stale_effect_control_cannot_select_or_begin() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .get_mut::<KeyControl>(f.keys[1])
        .unwrap()
        .effect = EffectId::new();
    f.action(pointer::click(
        f.keys[1],
        f.window,
        PointerButton::Primary,
        1,
    ));
    f.begin(1);
    assert!(f.state().host_motion.drag.is_none());
    assert!(f.state().host_motion.selected.is_none());
}

#[test]
fn missing_track_and_outdated_index_cannot_begin() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .get_mut::<KeyControl>(f.keys[1])
        .unwrap()
        .index = 99;
    f.begin(1);
    assert!(f.state().host_motion.drag.is_none());
    f.app
        .world_mut()
        .get_mut::<KeyControl>(f.keys[1])
        .unwrap()
        .index = 1;
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .effect
        .host_transform_track = None;
    f.begin(1);
    assert!(f.state().host_motion.drag.is_none());
}

#[test]
fn begin_clears_region_selection_without_rebuilding_target() {
    let mut f = Fixture::new();
    let emitter = f.session().effect.emitters[0].clone();
    let region = emitter.timeline_regions()[0].id;
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .select_emitter_region(emitter.id, region);
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .select_only_emitter_region(emitter.id, region);
    let revision = f.session().ui_revision;
    f.begin(1);
    assert_eq!(f.session().ui_revision, revision);
    assert!(f.state().selected_emitter_regions.is_empty());
    assert!(f.session().selected_emitter_region.is_none());
    assert!(active(f.session(), f.state()));
    assert!(f.app.world().get_entity(f.keys[1]).is_ok());
}

#[test]
fn movement_previews_real_pose_without_semantic_edit() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    f.begin(1);
    f.move_by(50.0);
    near(f.candidate_time(), 1.5);
    near(
        f.session()
            .preview()
            .unwrap()
            .host_transform_at(1.5)
            .translation[0],
        10.0,
    );
    f.clean(&before);
}

#[test]
fn release_creates_one_global_undoable_edit() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    f.begin(1);
    f.move_by(20.0);
    f.move_by(50.0);
    f.end();
    near(
        f.session()
            .effect
            .host_transform_track
            .as_ref()
            .unwrap()
            .keys()[1]
            .time,
        1.5,
    );
    assert!(f.state().host_motion.drag.is_none());
    f.roundtrip(&before);
}

#[test]
fn cross_retime_keeps_sorted_selection_and_consumes_trailing_click_once() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    f.begin(1);
    f.move_by(200.0);
    assert_eq!(f.state().host_motion.drag.as_ref().unwrap().selected, 2);
    f.end();
    assert_eq!(f.state().host_motion.selected, Some(2));
    f.action(pointer::click(
        f.keys[1],
        f.window,
        PointerButton::Primary,
        1,
    ));
    assert_eq!(f.state().host_motion.selected, Some(2));
    assert!(f.state().host_motion.released_key.is_none());
    f.action(pointer::click(
        f.keys[1],
        f.window,
        PointerButton::Primary,
        1,
    ));
    assert_eq!(f.state().host_motion.selected, Some(1));
    f.roundtrip(&before);
}

#[test]
fn secondary_click_does_not_consume_release_suppression() {
    let mut f = Fixture::new();
    f.begin(1);
    f.move_by(200.0);
    f.end();
    f.action(pointer::click(
        f.keys[1],
        f.window,
        PointerButton::Secondary,
        1,
    ));
    assert_eq!(f.state().host_motion.released_key, Some(f.keys[1]));
    assert_eq!(f.state().host_motion.selected, Some(2));
}

#[test]
fn unchanged_release_refreshes_inspector_once_without_history() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    f.begin(1);
    let revision = f.session().ui_revision;
    f.end();
    assert_eq!(f.session().ui_revision, revision + 1);
    f.end();
    assert_eq!(f.session().ui_revision, revision + 1);
    f.clean(&before);
}

#[test]
fn collision_is_rejected_and_next_valid_motion_recovers() {
    let mut f = Fixture::new();
    f.begin(1);
    f.move_by(100.0);
    near(f.candidate_time(), 1.0);
    f.move_by(-1000.0);
    near(f.candidate_time(), 1.0);
    f.move_by(50.0);
    near(f.candidate_time(), 1.5);
}

#[test]
fn escape_restores_preview_and_ignores_late_release() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    f.begin(1);
    f.move_by(50.0);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    f.app.update();
    assert!(f.state().host_motion.drag.is_none());
    near(
        f.session()
            .preview()
            .unwrap()
            .host_transform_at(1.5)
            .translation[0],
        15.0,
    );
    f.end();
    f.clean(&before);
}

#[test]
fn canonical_clear_during_drag_is_not_overwritten_on_release() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    f.begin(1);
    f.move_by(50.0);
    f.action(Action::Clear);
    assert!(f.session().effect.host_transform_track.is_none());
    f.end();
    assert!(f.session().effect.host_transform_track.is_none());
    f.roundtrip(&before);
}

#[test]
fn changed_effect_cancels_without_editing_replacement() {
    let mut f = Fixture::new();
    f.begin(1);
    f.move_by(50.0);
    let replacement = crate::test_support::effect_with_timing_slack();
    f.app
        .insert_resource(EditorSession::from_test_effect(replacement.clone()));
    f.app.update();
    assert!(f.state().host_motion.drag.is_none());
    f.end();
    f.clean(&replacement);
}

#[test]
fn floating_window_scale_and_canvas_ancestry_drive_motion_not_ui_scale() {
    let mut f = Fixture::new();
    f.app.insert_resource(UiScale(1.5));
    let mut window = Window::default();
    window.resolution.set_scale_factor_override(Some(2.0));
    let floating = f.app.world_mut().spawn(window).id();
    let canvas = f
        .app
        .world_mut()
        .spawn((
            TimelineCanvas,
            ComputedNode {
                size: Vec2::new(800.0, 100.0),
                ..default()
            },
            ChildOf(f.canvas),
        ))
        .id();
    let wrapper = f.app.world_mut().spawn(ChildOf(canvas)).id();
    f.app
        .world_mut()
        .entity_mut(f.keys[1])
        .insert(ChildOf(wrapper));
    f.begin(1);
    f.action(pointer::motion_delta(
        f.keys[1],
        floating,
        Vec2::new(50.0, 0.0),
    ));
    near(f.candidate_time(), 1.5);
}

#[test]
fn missing_pointer_window_defaults_to_unit_scale() {
    let mut f = Fixture::new();
    f.begin(1);
    f.action(pointer::motion_delta(
        f.keys[1],
        Entity::PLACEHOLDER,
        Vec2::new(50.0, 0.0),
    ));
    near(f.candidate_time(), 1.5);
}

#[test]
fn missing_canvas_ancestry_ignores_motion() {
    let mut f = Fixture::new();
    f.app.world_mut().entity_mut(f.keys[1]).remove::<ChildOf>();
    f.begin(1);
    f.move_by(50.0);
    near(f.candidate_time(), 1.0);
}

#[test]
fn frame_snap_uses_production_snap_policy() {
    let mut f = Fixture::new();
    f.app.world_mut().resource_mut::<TimelineState>().snap = super::super::TimelineSnapMode::Frames;
    f.begin(1);
    f.move_by(51.0);
    let time = f.candidate_time();
    near(time * 60.0, (time * 60.0).round());
    assert_ne!(time, 1.51);
}

#[test]
fn live_nodes_keep_original_binding_until_release() {
    let mut f = Fixture::new();
    f.begin(1);
    f.move_by(200.0);
    f.app.update();
    assert_eq!(
        f.app.world().get::<Node>(f.keys[1]).unwrap().left,
        Val::Percent(75.0)
    );
    assert_eq!(
        f.app.world().get::<Node>(f.keys[2]).unwrap().left,
        Val::Percent(50.0)
    );
    assert_eq!(
        f.app.world().get::<BackgroundColor>(f.keys[1]).unwrap().0,
        theme::ACCENT
    );
    assert!(
        f.keys
            .into_iter()
            .all(|e| f.app.world().get_entity(e).is_ok())
    );
}

#[test]
fn idle_visual_pass_does_not_dirty_node_or_color_components() {
    let mut f = Fixture::new();
    f.app.update();
    f.app.world_mut().clear_trackers();
    f.app.update();
    let mut nodes = f
        .app
        .world_mut()
        .query_filtered::<(Ref<Node>, Ref<BackgroundColor>), With<KeyControl>>();
    for (node, color) in nodes.iter(f.app.world()) {
        assert!(!node.is_changed());
        assert!(!color.is_changed());
    }
}

#[test]
fn invalid_effect_and_out_of_view_nodes_hide() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .get_mut::<KeyControl>(f.keys[0])
        .unwrap()
        .effect = EffectId::new();
    f.app.world_mut().resource_mut::<TimelineState>().view = super::super::state::TimelineView {
        start: 0.0,
        end: 1.5,
    };
    f.app.update();
    assert_eq!(
        f.app.world().get::<Node>(f.keys[0]).unwrap().display,
        Display::None
    );
    assert_eq!(
        f.app.world().get::<Node>(f.keys[1]).unwrap().display,
        Display::Flex
    );
    assert_eq!(
        f.app.world().get::<Node>(f.keys[2]).unwrap().display,
        Display::None
    );
}

#[test]
fn key_events_stop_real_hierarchy_propagation() {
    #[derive(Resource, Default)]
    struct Seen(usize);
    let mut f = Fixture::new();
    f.app.init_resource::<Seen>();
    f.app
        .world_mut()
        .entity_mut(f.canvas)
        .observe(|_: On<PointerClick>, mut seen: ResMut<Seen>| seen.0 += 1)
        .observe(|_: On<PointerDragStart>, mut seen: ResMut<Seen>| seen.0 += 1)
        .observe(|_: On<PointerDrag>, mut seen: ResMut<Seen>| seen.0 += 1)
        .observe(|_: On<PointerDragEnd>, mut seen: ResMut<Seen>| seen.0 += 1);
    f.action(pointer::click(
        f.keys[1],
        f.window,
        PointerButton::Primary,
        1,
    ));
    f.begin(1);
    f.move_by(50.0);
    f.end();
    assert_eq!(f.app.world().resource::<Seen>().0, 0);
    f.begin(0);
    assert_eq!(f.app.world().resource::<Seen>().0, 0);
}

#[test]
fn repeating_interior_cannot_cross_endpoint_but_valid_drag_commits() {
    let mut f = Fixture::new();
    f.action(Action::Inspect);
    f.action(Action::CloseLoop);
    let before = f.session().effect.clone();
    f.begin(1);
    f.move_by(200.0);
    near(f.candidate_time(), 1.0);
    f.move_by(50.0);
    f.end();
    let track = f.session().effect.host_transform_track.as_ref().unwrap();
    assert!(track.repeat);
    assert!(track.validate().is_ok());
    near(track.keys()[1].time, 1.5);
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, before);
}

#[test]
fn repeating_endpoint_cannot_precede_interior_key() {
    let mut f = Fixture::new();
    f.action(Action::Inspect);
    f.action(Action::CloseLoop);
    f.begin(2);
    f.action(pointer::motion_delta(
        f.keys[2],
        f.window,
        Vec2::new(-150.0, 0.0),
    ));
    near(f.candidate_time(), 2.0);
    f.action(pointer::motion_delta(
        f.keys[2],
        f.window,
        Vec2::new(50.0, 0.0),
    ));
    near(f.candidate_time(), 2.5);
    f.action(pointer::end(
        f.keys[2],
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    let track = f.session().effect.host_transform_track.as_ref().unwrap();
    near(track.end_time(), 2.5);
    assert!(track.validate().is_ok());
}

#[test]
fn seconds_and_smart_snapping_use_shared_marker_targets() {
    let mut f = Fixture::new();
    f.app.world_mut().resource_mut::<TimelineState>().snap =
        super::super::TimelineSnapMode::Seconds;
    f.begin(1);
    f.move_by(51.0);
    near(f.candidate_time(), 1.6);
    f.app.world_mut().resource_mut::<TimelineState>().snap = super::super::TimelineSnapMode::Smart;
    f.move_by(51.0);
    near(f.candidate_time(), 1.5166667);
}

#[test]
fn motion_and_marker_actions_share_global_history_order() {
    let mut f = Fixture::new();
    f.app
        .add_observer(super::super::execution::execute_timeline_action);
    let before = f.session().effect.clone();
    f.action(super::super::TimelineAction::AddMarker);
    let marked = f.session().effect.clone();
    assert_eq!(marked.markers.len(), before.markers.len() + 1);
    f.begin(1);
    f.move_by(50.0);
    f.end();
    let moved = f.session().effect.clone();
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, marked);
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, before);
    assert!(!f.session().can_undo());
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, marked);
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, moved);
}
