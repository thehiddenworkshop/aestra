//! Real pointer events/observers and production document/history consumers on both engines.
use super::super::TimelineSnapMode;
use super::super::{
    execution, navigation, region_gestures, state::TimelineView, timeline_pointer::tests as pointer,
};
use super::*;
use crate::{
    CurvesState, ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
};
use aestra_authoring::SemanticTarget;
use aestra_core::{
    ChoreographyEvent, ChoreographyEventId, ChoreographyEventPayload, EffectAsset, EffectMarker,
    EmitterId, EmitterRegionId, MarkerId, MarkerTimeReference,
};
use bevy::{
    input::mouse::{MouseMotion, MouseWheel},
    picking::pointer::PointerId,
};

#[derive(Clone, Copy)]
enum Kind {
    Marker,
    Event,
}
struct Fixture {
    _root: tempfile::TempDir,
    app: App,
    window: Entity,
    canvas: Entity,
    marker: MarkerId,
    event: ChoreographyEventId,
}
impl Fixture {
    fn new(scale: f32) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut effect = crate::test_support::effect_with_timing_slack();
        let marker = MarkerId::new();
        effect.markers.push(EffectMarker {
            id: marker,
            name: "Cue".into(),
            time: 1.0,
        });
        let event = ChoreographyEvent::new(
            "Sound",
            1.0,
            ChoreographyEventPayload::PlaySound {
                cue: "burst".into(),
            },
        );
        let event_id = event.id;
        effect.choreography_events.push(event);
        let mut app = App::new();
        app.insert_resource(EditorSession::from_test_effect(effect))
            .insert_resource(TimelineState::framed(4.0))
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<CurvesState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<OverrideCursor>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<FunctionEditor>()
            .add_message::<MouseMotion>()
            .add_message::<MouseWheel>()
            .add_observer(execution::execute_timeline_action)
            .add_observer(execution::execute_choreography_action)
            .add_observer(
                |action: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*action, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history expected")
                    });
                },
            )
            .add_systems(Update, navigation::navigate_timeline);
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
            .observe(seek_timeline_on_press)
            .observe(seek_timeline_on_drag)
            .id();
        Self {
            _root: root,
            app,
            window,
            canvas,
            marker,
            event: event_id,
        }
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource::<EditorSession>()
    }
    fn state(&self) -> &TimelineState {
        self.app.world().resource::<TimelineState>()
    }
    fn action<E: Event>(&mut self, action: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(action);
        self.app.world_mut().flush();
    }
    fn press(&mut self, target: Entity, button: PointerButton) {
        self.action(pointer::press(
            target,
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            button,
        ));
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
    fn move_by(&mut self, target: Entity, distance: f32) {
        self.action(pointer::motion_delta(
            target,
            self.window,
            Vec2::new(distance, 0.0),
        ));
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
    fn escape(&mut self) {
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        self.app.update();
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Escape);
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }
    fn region(&mut self, kind: TimelineDragKind) -> (Entity, EmitterId, EmitterRegionId) {
        let emitter = &self.session().effect.emitters[0];
        let id = emitter.id;
        let region = emitter.timeline_regions()[0].id;
        let entity = self
            .app
            .world_mut()
            .spawn((
                TimelineClipInteraction {
                    emitter: id,
                    region,
                    kind,
                },
                ChildOf(self.canvas),
            ))
            .observe(region_gestures::begin_timeline_clip_drag)
            .observe(region_gestures::move_timeline_clip_drag)
            .observe(region_gestures::finish_timeline_clip_drag)
            .id();
        (entity, id, region)
    }
    fn cue(&mut self, kind: Kind) -> Entity {
        match kind {
            Kind::Marker => self
                .app
                .world_mut()
                .spawn((
                    TimelineMarker {
                        marker: self.marker,
                    },
                    ChildOf(self.canvas),
                ))
                .observe(begin_timeline_marker_drag)
                .observe(move_timeline_marker_drag)
                .observe(finish_timeline_marker_drag)
                .id(),
            Kind::Event => self
                .app
                .world_mut()
                .spawn((
                    TimelineChoreographyEvent { event: self.event },
                    ChildOf(self.canvas),
                ))
                .observe(begin_choreography_event_drag)
                .observe(move_choreography_event_drag)
                .observe(finish_choreography_event_drag)
                .id(),
        }
    }
    fn time(&self, kind: Kind) -> f32 {
        match kind {
            Kind::Marker => {
                self.session()
                    .effect
                    .markers
                    .iter()
                    .find(|m| m.id == self.marker)
                    .unwrap()
                    .time
            }
            Kind::Event => {
                self.session()
                    .effect
                    .choreography_events
                    .iter()
                    .find(|e| e.id == self.event)
                    .unwrap()
                    .time
            }
        }
    }
    fn preview_time(&self, kind: Kind) -> f32 {
        match kind {
            Kind::Marker => self.state().marker_drag.unwrap().current_time,
            Kind::Event => self.state().choreography_event_drag.unwrap().current_time,
        }
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
            "One release must be one semantic history entry"
        );
        self.action(HistoryAction::Redo);
        assert_eq!(self.session().effect, after);
    }
}
fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
}

#[test]
fn primary_press_seeks_in_the_current_view_without_semantic_edits() {
    let mut f = Fixture::new(1.0);
    let before = f.session().effect.clone();
    f.app.world_mut().resource_mut::<TimelineState>().view = TimelineView {
        start: 1.0,
        end: 3.0,
    };
    f.press(f.canvas, PointerButton::Primary);
    near(f.session().time(), 2.0);
    f.clean(&before);
}

#[test]
fn seek_clamps_relative_cursor_to_view_edges() {
    let mut f = Fixture::new(1.0);
    for (x, expected) in [(-2.0, 0.0), (2.0, 4.0)] {
        f.app
            .world_mut()
            .get_mut::<RelativeCursorPosition>(f.canvas)
            .unwrap()
            .normalized = Some(Vec2::new(x, 0.0));
        f.press(f.canvas, PointerButton::Primary);
        near(f.session().time(), expected);
    }
}

#[test]
fn secondary_press_and_drag_do_not_seek() {
    let mut f = Fixture::new(1.0);
    f.press(f.canvas, PointerButton::Secondary);
    let event = pointer::motion_button(
        f.canvas,
        f.window,
        Vec2::splat(50.0),
        PointerId::Mouse,
        PointerButton::Secondary,
    );
    f.action(event);
    near(f.session().time(), 0.0);
}

#[test]
fn seek_requires_matching_canvas_and_known_cursor() {
    let mut f = Fixture::new(1.0);
    let other = f
        .app
        .world_mut()
        .spawn_empty()
        .observe(seek_timeline_on_press)
        .id();
    f.press(other, PointerButton::Primary);
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(f.canvas)
        .unwrap()
        .normalized = None;
    f.press(f.canvas, PointerButton::Primary);
    f.move_by(f.canvas, 100.0);
    near(f.session().time(), 0.0);
}

#[test]
fn canvas_drag_seeks_using_cursor_not_distance() {
    let mut f = Fixture::new(1.0);
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(f.canvas)
        .unwrap()
        .normalized = Some(Vec2::new(-0.25, 0.0));
    f.move_by(f.canvas, 300.0);
    near(f.session().time(), 1.0);
}

#[test]
fn primary_seek_closes_existing_context_and_color_popups_once() {
    let mut f = Fixture::new(1.0);
    let emitter = f.session().effect.emitters[0].id;
    {
        let mut s = f.app.world_mut().resource_mut::<TimelineState>();
        s.context_emitter = Some(emitter);
        s.context_effect_clip = Some(aestra_core::EffectClipId::new());
        s.color_picker_emitter = Some(emitter);
    }
    let revision = f.session().ui_revision;
    f.press(f.canvas, PointerButton::Primary);
    assert!(f.state().context_emitter.is_none());
    assert!(f.state().context_effect_clip.is_none());
    assert!(f.state().color_picker_emitter.is_none());
    assert_eq!(f.session().ui_revision, revision + 1);
}

#[test]
fn timeline_control_stoppers_block_parent_seek_on_press_and_drag() {
    let mut f = Fixture::new(1.0);
    let control = f
        .app
        .world_mut()
        .spawn(ChildOf(f.canvas))
        .observe(stop_timeline_control_press)
        .observe(stop_timeline_control_drag)
        .id();
    f.press(control, PointerButton::Primary);
    f.move_by(control, 100.0);
    near(f.session().time(), 0.0);
}

#[test]
fn region_move_previews_then_commits_once_with_selection_and_undo_redo() {
    let mut f = Fixture::new(1.0);
    let (target, emitter, region) = f.region(TimelineDragKind::Move);
    let before = f.session().effect.clone();
    f.begin(target);
    f.move_by(target, 50.0);
    near(f.state().drag.unwrap().current_start, 1.0);
    f.clean(&before);
    near(f.session().time(), 0.0);
    f.end(target);
    near(
        f.session().effect.emitters[0]
            .timeline_region(region)
            .unwrap()
            .start_time,
        1.0,
    );
    assert_eq!(f.session().selected_emitter_region, Some(region));
    assert_eq!(f.state().selected_emitter_region, Some((emitter, region)));
    assert!(f.state().drag.is_none());
    assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
    f.roundtrip(&before);
}

#[test]
fn region_trim_start_updates_source_offset_and_preserves_end() {
    let mut f = Fixture::new(1.0);
    let (target, _, region) = f.region(TimelineDragKind::TrimStart);
    let before = f.session().effect.clone();
    f.begin(target);
    f.move_by(target, 50.0);
    let drag = f.state().drag.unwrap();
    near(drag.current_start, 1.0);
    near(drag.current_source_offset, 0.5);
    near(drag.current_duration, 2.0);
    f.clean(&before);
    f.end(target);
    let edited = f.session().effect.emitters[0]
        .timeline_region(region)
        .unwrap();
    near(edited.source_offset, 0.5);
    near(edited.end_time(), 3.0);
    f.roundtrip(&before);
}

#[test]
fn region_trim_end_keeps_start_and_source_offset() {
    let mut f = Fixture::new(1.0);
    let (target, _, region) = f.region(TimelineDragKind::TrimEnd);
    let before = f.session().effect.clone();
    f.begin(target);
    f.move_by(target, -50.0);
    f.clean(&before);
    f.end(target);
    let edited = f.session().effect.emitters[0]
        .timeline_region(region)
        .unwrap();
    near(edited.start_time, 0.5);
    near(edited.source_offset, 0.0);
    near(edited.duration, 2.0);
    f.roundtrip(&before);
}

#[test]
fn region_move_uses_window_scale_and_largest_canvas_width() {
    let mut f = Fixture::new(2.0);
    f.app.world_mut().spawn((
        TimelineCanvas,
        ComputedNode {
            size: Vec2::new(100.0, 100.0),
            ..default()
        },
    ));
    let (target, _, _) = f.region(TimelineDragKind::Move);
    f.begin(target);
    f.move_by(target, 100.0);
    near(f.state().drag.unwrap().current_start, 1.0);
}

#[test]
fn unchanged_region_release_does_not_create_history() {
    let mut f = Fixture::new(1.0);
    let (target, _, _) = f.region(TimelineDragKind::Move);
    let before = f.session().effect.clone();
    f.begin(target);
    f.end(target);
    f.clean(&before);
}

#[test]
fn escape_region_drag_prevents_late_release_from_committing() {
    let mut f = Fixture::new(1.0);
    let (target, _, _) = f.region(TimelineDragKind::Move);
    let before = f.session().effect.clone();
    f.begin(target);
    f.move_by(target, 50.0);
    f.escape();
    f.end(target);
    assert!(f.state().drag.is_none());
    f.clean(&before);
}

#[test]
fn mismatched_region_motion_does_not_change_the_active_preview() {
    let mut f = Fixture::new(1.0);
    let (target, emitter, _) = f.region(TimelineDragKind::Move);
    let other = f
        .app
        .world_mut()
        .spawn(TimelineClipInteraction {
            emitter,
            region: EmitterRegionId::new(),
            kind: TimelineDragKind::Move,
        })
        .observe(region_gestures::move_timeline_clip_drag)
        .id();
    f.begin(target);
    f.move_by(other, 50.0);
    near(f.state().drag.unwrap().current_start, 0.5);
}

#[test]
fn stale_region_drag_start_is_ignored() {
    let mut f = Fixture::new(1.0);
    let (target, _, _) = f.region(TimelineDragKind::Move);
    f.app
        .world_mut()
        .get_mut::<TimelineClipInteraction>(target)
        .unwrap()
        .region = EmitterRegionId::new();
    f.begin(target);
    assert!(f.state().drag.is_none());
    assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
}

#[test]
fn selected_region_group_moves_together_and_preserves_multi_selection() {
    let mut f = Fixture::new(1.0);
    let mut effect = f.session().effect.clone();
    let emitter = &mut effect.emitters[0];
    let id = emitter.id;
    let first = emitter.implicit_region_id();
    let second = EmitterRegionId::new();
    emitter.regions = emitter.split_timeline_region(first, 1.5, second).unwrap();
    f.app
        .insert_resource(EditorSession::from_test_effect(effect));
    let (target, _, _) = f.region(TimelineDragKind::Move);
    {
        let mut s = f.app.world_mut().resource_mut::<TimelineState>();
        s.select_only_emitter_region(id, first);
        s.selected_emitter_regions.insert((id, second));
    }
    let before = f.session().effect.clone();
    f.begin(target);
    f.move_by(target, 50.0);
    assert_eq!(f.state().drag_regions.len(), 2);
    f.move_by(target, 1000.0);
    near(f.state().drag.unwrap().current_start, 1.5);
    f.move_by(target, -1000.0);
    near(f.state().drag.unwrap().current_start, 0.0);
    f.move_by(target, 50.0);
    let original = before.emitters[0].timeline_region(second).unwrap();
    let preview = region_gestures::timeline_region_preview_timing(f.state(), id, original);
    near(preview.0, original.start_time + 0.5);
    f.end(target);
    assert_eq!(f.state().selected_emitter_regions.len(), 2);
    for region in &before.emitters[0].regions {
        near(
            f.session().effect.emitters[0]
                .timeline_region(region.id)
                .unwrap()
                .start_time,
            region.start_time + 0.5,
        );
    }
    f.roundtrip(&before);
}

#[test]
fn single_region_move_clamps_to_effect_bounds() {
    let mut f = Fixture::new(1.0);
    let (target, _, _) = f.region(TimelineDragKind::Move);
    f.begin(target);
    f.move_by(target, -1000.0);
    near(f.state().drag.unwrap().current_start, 0.0);
    f.move_by(target, 1000.0);
    near(f.state().drag.unwrap().current_start, 1.5);
}

#[test]
fn region_trims_keep_at_least_one_frame_and_remain_inside_effect() {
    for (kind, distance) in [
        (TimelineDragKind::TrimStart, 1000.0),
        (TimelineDragKind::TrimEnd, -1000.0),
    ] {
        let mut f = Fixture::new(1.0);
        let (target, _, _) = f.region(kind);
        f.begin(target);
        f.move_by(target, distance);
        let drag = f.state().drag.unwrap();
        near(drag.current_duration, 1.0 / f.session().tick_rate() as f32);
        assert!(drag.current_start >= 0.0);
        assert!(drag.current_start + drag.current_duration <= 4.0);
    }
}

#[test]
fn marker_and_event_drags_preview_then_commit_once_without_parent_seek() {
    for kind in [Kind::Marker, Kind::Event] {
        let mut f = Fixture::new(1.0);
        let target = f.cue(kind);
        let before = f.session().effect.clone();
        f.begin(target);
        assert!(f.app.world().resource::<OverrideCursor>().0.is_some());
        f.move_by(target, 50.0);
        near(f.preview_time(kind), 1.5);
        f.clean(&before);
        near(f.session().time(), 0.0);
        f.end(target);
        near(f.time(kind), 1.5);
        let selected = match kind {
            Kind::Marker => SemanticTarget::Marker(f.marker),
            Kind::Event => SemanticTarget::ChoreographyEvent(f.event),
        };
        assert_eq!(f.session().selection.primary, selected);
        assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
        f.roundtrip(&before);
    }
}

#[test]
fn marker_and_event_drag_time_is_scaled_and_clamped() {
    for kind in [Kind::Marker, Kind::Event] {
        let mut f = Fixture::new(2.0);
        let target = f.cue(kind);
        f.begin(target);
        f.move_by(target, 100.0);
        near(f.preview_time(kind), 1.5);
        f.move_by(target, -1000.0);
        near(f.preview_time(kind), 0.0);
        f.move_by(target, 1000.0);
        near(f.preview_time(kind), 4.0);
    }
}

#[test]
fn unchanged_marker_and_event_release_only_selects() {
    for kind in [Kind::Marker, Kind::Event] {
        let mut f = Fixture::new(1.0);
        let target = f.cue(kind);
        let before = f.session().effect.clone();
        f.begin(target);
        f.end(target);
        f.clean(&before);
    }
}

#[test]
fn escape_marker_and_event_drag_prevents_late_commit() {
    for kind in [Kind::Marker, Kind::Event] {
        let mut f = Fixture::new(1.0);
        let target = f.cue(kind);
        let before = f.session().effect.clone();
        f.begin(target);
        f.move_by(target, 50.0);
        f.escape();
        f.end(target);
        f.clean(&before);
    }
}

#[test]
fn mismatched_marker_and_event_release_retains_the_original_gesture() {
    for kind in [Kind::Marker, Kind::Event] {
        let mut f = Fixture::new(1.0);
        let target = f.cue(kind);
        let wrong = f.cue(kind);
        match kind {
            Kind::Marker => {
                f.app
                    .world_mut()
                    .get_mut::<TimelineMarker>(wrong)
                    .unwrap()
                    .marker = MarkerId::new()
            }
            Kind::Event => {
                f.app
                    .world_mut()
                    .get_mut::<TimelineChoreographyEvent>(wrong)
                    .unwrap()
                    .event = ChoreographyEventId::new()
            }
        }
        f.begin(target);
        f.move_by(wrong, 50.0);
        near(f.preview_time(kind), 1.0);
        f.end(wrong);
        near(f.preview_time(kind), 1.0);
        f.move_by(target, 50.0);
        f.end(target);
        near(f.time(kind), 1.5);
    }
}

#[test]
fn stale_marker_and_event_drag_start_is_ignored() {
    for kind in [Kind::Marker, Kind::Event] {
        let mut f = Fixture::new(1.0);
        let target = f.cue(kind);
        match kind {
            Kind::Marker => {
                f.app
                    .world_mut()
                    .get_mut::<TimelineMarker>(target)
                    .unwrap()
                    .marker = MarkerId::new()
            }
            Kind::Event => {
                f.app
                    .world_mut()
                    .get_mut::<TimelineChoreographyEvent>(target)
                    .unwrap()
                    .event = ChoreographyEventId::new()
            }
        }
        f.begin(target);
        assert!(f.state().marker_drag.is_none());
        assert!(f.state().choreography_event_drag.is_none());
        assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
    }
}

#[test]
fn frame_and_seconds_snapping_match_existing_marker_event_and_region_policy() {
    for mode in [TimelineSnapMode::Frames, TimelineSnapMode::Seconds] {
        for kind in [Kind::Marker, Kind::Event] {
            let mut f = Fixture::new(1.0);
            f.app.world_mut().resource_mut::<TimelineState>().snap = mode;
            let target = f.cue(kind);
            f.begin(target);
            f.move_by(target, 12.3);
            let actual = f.preview_time(kind);
            let interval = match mode {
                TimelineSnapMode::Frames => 1.0 / f.session().tick_rate() as f32,
                _ => nice_timeline_step(4.0, 400.0) / 5.0,
            };
            near(actual, (1.123 / interval).round() * interval);
            assert_eq!(f.state().snap_guide, Some(actual));
        }
        let mut f = Fixture::new(1.0);
        f.app.world_mut().resource_mut::<TimelineState>().snap = mode;
        let (target, _, _) = f.region(TimelineDragKind::Move);
        f.begin(target);
        f.move_by(target, 12.3);
        assert!(f.state().snap_guide.is_some());
    }
}

#[test]
fn smart_snapping_publishes_nearby_frame_guide_without_editing() {
    for kind in [Kind::Marker, Kind::Event] {
        let mut f = Fixture::new(1.0);
        f.app.world_mut().resource_mut::<TimelineState>().snap = TimelineSnapMode::Smart;
        let target = f.cue(kind);
        let before = f.session().effect.clone();
        f.begin(target);
        f.move_by(target, 100.3);
        near(f.preview_time(kind), 2.0);
        assert_eq!(f.state().snap_guide, Some(2.0));
        f.clean(&before);
    }
}

#[test]
fn moving_marker_updates_anchored_event_and_roundtrips_as_one_edit() {
    let mut f = Fixture::new(1.0);
    let mut effect = f.session().effect.clone();
    effect.choreography_events[0].time_reference = Some(MarkerTimeReference::new(f.marker, 0.0));
    f.app
        .insert_resource(EditorSession::from_test_effect(effect));
    let target = f.cue(Kind::Marker);
    let before = f.session().effect.clone();
    f.begin(target);
    f.move_by(target, 50.0);
    f.end(target);
    near(f.time(Kind::Marker), 1.5);
    near(f.time(Kind::Event), 1.5);
    f.roundtrip(&before);
}

#[test]
fn moving_anchored_event_preserves_marker_and_updates_its_offset() {
    let mut f = Fixture::new(1.0);
    let mut effect = f.session().effect.clone();
    effect.choreography_events[0].time_reference = Some(MarkerTimeReference::new(f.marker, 0.0));
    f.app
        .insert_resource(EditorSession::from_test_effect(effect));
    let target = f.cue(Kind::Event);
    let before = f.session().effect.clone();
    f.begin(target);
    f.move_by(target, 50.0);
    f.end(target);
    near(f.time(Kind::Marker), 1.0);
    near(
        f.session().effect.choreography_events[0]
            .time_reference
            .unwrap()
            .offset,
        0.5,
    );
    f.roundtrip(&before);
}
