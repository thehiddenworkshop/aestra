//! Actual catalog-backed clip observers with upstream pointer events and global history.
use super::super::{
    execution, gestures, navigation, region_gestures, timeline_pointer::tests as pointer,
};
use super::*;
use crate::{
    CurvesState,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
};
use aestra_authoring::SemanticTarget;
use aestra_core::{
    EffectAsset, EffectClip, EffectMarker, EffectPlaybackMode, Emitter, MarkerId,
    MarkerTimeReference,
};
use bevy::{
    input::mouse::{MouseMotion, MouseWheel},
    picking::pointer::{PointerButton, PointerId},
    ui::RelativeCursorPosition,
};
use std::path::PathBuf;

struct Fixture {
    _root: tempfile::TempDir,
    source_path: PathBuf,
    source_bytes: Option<Vec<u8>>,
    app: App,
    window: Entity,
    canvas: Entity,
    clip: EffectClipId,
}
impl Fixture {
    fn new(mode: Option<EffectPlaybackMode>, scale: f32) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut source = EffectAsset::new("Clip Source", 2.0);
        source.emitters.push(Emitter::basic_sprite("Source", 2.0));
        source.playback_mode = mode.unwrap_or(EffectPlaybackMode::Once);
        let source_path = root.path().join("source.aestra.ron");
        let source_bytes = mode.map(|_| {
            source.save_ron(&source_path).unwrap();
            std::fs::read(&source_path).unwrap()
        });
        let mut effect = crate::test_support::effect_with_timing_slack();
        let mut clip = EffectClip::new(source.id, 1.0, 1.0);
        clip.source_offset = 0.5;
        let clip_id = clip.id;
        effect.effect_clips.push(clip);
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
            .observe(gestures::seek_timeline_on_drag)
            .id();
        Self {
            _root: root,
            source_path,
            source_bytes,
            app,
            window,
            canvas,
            clip: clip_id,
        }
    }
    fn once() -> Self {
        Self::new(Some(EffectPlaybackMode::Once), 1.0)
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource::<EditorSession>()
    }
    fn state(&self) -> &TimelineState {
        self.app.world().resource::<TimelineState>()
    }
    fn clip(&self) -> &EffectClip {
        self.session()
            .effect
            .effect_clips
            .iter()
            .find(|c| c.id == self.clip)
            .unwrap()
    }
    fn target(&mut self, kind: TimelineDragKind) -> Entity {
        self.app
            .world_mut()
            .spawn((
                TimelineEffectClipInteraction {
                    clip: self.clip,
                    kind,
                },
                ChildOf(self.canvas),
            ))
            .observe(begin_effect_clip_timeline_drag)
            .observe(move_effect_clip_timeline_drag)
            .observe(finish_effect_clip_timeline_drag)
            .id()
    }
    fn action<E: Event>(&mut self, action: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(action);
        self.app.world_mut().flush();
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
    fn unchanged_source(&self) {
        assert_eq!(std::fs::read(&self.source_path).ok(), self.source_bytes);
    }
    fn clean(&self, before: &EffectAsset) {
        assert_eq!(&self.session().effect, before);
        assert!(!self.session().dirty);
        assert!(!self.session().can_undo());
        self.unchanged_source();
    }
    fn roundtrip(&mut self, before: &EffectAsset) {
        let after = self.session().effect.clone();
        assert_ne!(&after, before);
        self.action(HistoryAction::Undo);
        assert_eq!(&self.session().effect, before);
        assert!(
            !self.session().can_undo(),
            "Release must create exactly one history entry"
        );
        self.action(HistoryAction::Redo);
        assert_eq!(self.session().effect, after);
        self.unchanged_source();
    }
}
fn near(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.0001, "{actual} != {expected}");
}

#[test]
fn begin_uses_actual_catalog_source_duration_and_looping_policy() {
    for mode in [
        EffectPlaybackMode::Once,
        EffectPlaybackMode::LoopRestart,
        EffectPlaybackMode::LoopContinuous,
    ] {
        let mut f = Fixture::new(Some(mode), 1.0);
        let target = f.target(TimelineDragKind::Move);
        f.begin(target);
        let drag = f.state().effect_clip_drag.unwrap();
        near(drag.source_duration, 2.0);
        assert_eq!(drag.source_looping, mode.is_looping());
        near(drag.original_start, 1.0);
        near(drag.original_source_offset, 0.5);
        near(drag.original_duration, 1.0);
        assert!(f.app.world().resource::<OverrideCursor>().0.is_some());
        f.unchanged_source();
    }
}

#[test]
fn missing_source_uses_existing_conservative_extent_without_loading_or_writing() {
    let mut f = Fixture::new(None, 1.0);
    let target = f.target(TimelineDragKind::TrimEnd);
    f.begin(target);
    let drag = f.state().effect_clip_drag.unwrap();
    near(drag.source_duration, 1.5);
    assert!(!drag.source_looping);
    f.move_by(target, 1000.0);
    near(f.state().effect_clip_drag.unwrap().current_duration, 1.0);
    f.unchanged_source();
}

#[test]
fn stale_and_unrelated_drag_start_leave_cursor_and_state_unchanged() {
    let mut f = Fixture::once();
    let target = f.target(TimelineDragKind::Move);
    f.app
        .world_mut()
        .get_mut::<TimelineEffectClipInteraction>(target)
        .unwrap()
        .clip = EffectClipId::new();
    f.begin(target);
    let unrelated = f
        .app
        .world_mut()
        .spawn_empty()
        .observe(begin_effect_clip_timeline_drag)
        .id();
    f.begin(unrelated);
    assert!(f.state().effect_clip_drag.is_none());
    assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
}

#[test]
fn clip_begin_replaces_active_emitter_region_drag() {
    let mut f = Fixture::once();
    let emitter = &f.session().effect.emitters[0];
    let control = TimelineClipInteraction {
        emitter: emitter.id,
        region: emitter.timeline_regions()[0].id,
        kind: TimelineDragKind::Move,
    };
    let region = f
        .app
        .world_mut()
        .spawn(control)
        .observe(region_gestures::begin_timeline_clip_drag)
        .id();
    f.begin(region);
    assert!(f.state().drag.is_some());
    let target = f.target(TimelineDragKind::Move);
    f.begin(target);
    assert!(f.state().drag.is_none());
    assert!(f.state().effect_clip_drag.is_some());
}

#[test]
fn move_previews_without_edit_then_releases_one_selected_reversible_commit() {
    let mut f = Fixture::once();
    let target = f.target(TimelineDragKind::Move);
    let before = f.session().effect.clone();
    f.begin(target);
    f.move_by(target, 25.0);
    let drag = f.state().effect_clip_drag.unwrap();
    near(drag.current_start, 1.25);
    near(drag.current_source_offset, 0.5);
    near(drag.current_duration, 1.0);
    f.clean(&before);
    near(f.session().time(), 0.0);
    f.end(target);
    near(f.clip().start_time, 1.25);
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::EffectClip(f.clip)
    );
    assert!(f.state().effect_clip_drag.is_none());
    assert!(f.state().snap_guide.is_none());
    assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
    f.roundtrip(&before);
}

#[test]
fn start_trim_advances_source_offset_and_preserves_parent_end() {
    let mut f = Fixture::once();
    let target = f.target(TimelineDragKind::TrimStart);
    let before = f.session().effect.clone();
    f.begin(target);
    f.move_by(target, 25.0);
    let drag = f.state().effect_clip_drag.unwrap();
    near(drag.current_start, 1.25);
    near(drag.current_source_offset, 0.75);
    near(drag.current_duration, 0.75);
    f.clean(&before);
    f.end(target);
    near(f.clip().start_time + f.clip().duration, 2.0);
    near(f.clip().source_offset, 0.75);
    f.roundtrip(&before);
}

#[test]
fn one_shot_start_expansion_stops_when_source_offset_reaches_zero() {
    let mut f = Fixture::once();
    let target = f.target(TimelineDragKind::TrimStart);
    let before = f.session().effect.clone();
    f.begin(target);
    f.move_by(target, -1000.0);
    let drag = f.state().effect_clip_drag.unwrap();
    near(drag.current_start, 0.5);
    near(drag.current_source_offset, 0.0);
    near(drag.current_duration, 1.5);
    f.end(target);
    near(f.clip().source_offset, 0.0);
    f.roundtrip(&before);
}

#[test]
fn looping_start_expansion_wraps_negative_source_offset() {
    for mode in [
        EffectPlaybackMode::LoopRestart,
        EffectPlaybackMode::LoopContinuous,
    ] {
        let mut f = Fixture::new(Some(mode), 1.0);
        let target = f.target(TimelineDragKind::TrimStart);
        let before = f.session().effect.clone();
        f.begin(target);
        f.move_by(target, -1000.0);
        let drag = f.state().effect_clip_drag.unwrap();
        near(drag.current_start, 0.0);
        near(drag.current_source_offset, 1.5);
        near(drag.current_duration, 2.0);
        f.end(target);
        near(f.clip().source_offset, 1.5);
        f.roundtrip(&before);
    }
}

#[test]
fn one_shot_end_expansion_stops_at_source_extent() {
    let mut f = Fixture::once();
    let target = f.target(TimelineDragKind::TrimEnd);
    let before = f.session().effect.clone();
    f.begin(target);
    f.move_by(target, 1000.0);
    near(f.state().effect_clip_drag.unwrap().current_duration, 1.5);
    f.end(target);
    near(f.clip().start_time, 1.0);
    near(f.clip().source_offset, 0.5);
    near(f.clip().duration, 1.5);
    f.roundtrip(&before);
}

#[test]
fn looping_end_expansion_can_exceed_source_length_but_not_parent_duration() {
    let mut f = Fixture::new(Some(EffectPlaybackMode::LoopRestart), 1.0);
    let target = f.target(TimelineDragKind::TrimEnd);
    let before = f.session().effect.clone();
    f.begin(target);
    f.move_by(target, 1000.0);
    near(f.state().effect_clip_drag.unwrap().current_duration, 3.0);
    f.end(target);
    near(f.clip().start_time + f.clip().duration, 4.0);
    f.roundtrip(&before);
}

#[test]
fn trimming_keeps_at_least_one_frame_for_both_source_policies() {
    for mode in [EffectPlaybackMode::Once, EffectPlaybackMode::LoopRestart] {
        for (kind, distance) in [
            (TimelineDragKind::TrimStart, 1000.0),
            (TimelineDragKind::TrimEnd, -1000.0),
        ] {
            let mut f = Fixture::new(Some(mode), 1.0);
            let target = f.target(kind);
            let before = f.session().effect.clone();
            f.begin(target);
            f.move_by(target, distance);
            near(
                f.state().effect_clip_drag.unwrap().current_duration,
                1.0 / f.session().tick_rate() as f32,
            );
            f.end(target);
            f.roundtrip(&before);
        }
    }
}

#[test]
fn move_clamps_parent_bounds_without_changing_source_window() {
    let mut f = Fixture::once();
    let target = f.target(TimelineDragKind::Move);
    f.begin(target);
    f.move_by(target, -1000.0);
    near(f.state().effect_clip_drag.unwrap().current_start, 0.0);
    f.move_by(target, 1000.0);
    let drag = f.state().effect_clip_drag.unwrap();
    near(drag.current_start, 3.0);
    near(drag.current_duration, 1.0);
    near(drag.current_source_offset, 0.5);
}

#[test]
fn motion_uses_window_scale_largest_canvas_and_current_view_span() {
    let mut f = Fixture::new(Some(EffectPlaybackMode::Once), 2.0);
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
    let target = f.target(TimelineDragKind::Move);
    f.begin(target);
    f.move_by(target, 100.0);
    near(f.state().effect_clip_drag.unwrap().current_start, 1.125);
    near(f.session().time(), 0.0);
}

#[test]
fn unchanged_release_only_selects_for_each_gesture_kind() {
    for kind in [
        TimelineDragKind::Move,
        TimelineDragKind::TrimStart,
        TimelineDragKind::TrimEnd,
    ] {
        let mut f = Fixture::once();
        let target = f.target(kind);
        let before = f.session().effect.clone();
        f.begin(target);
        f.end(target);
        f.clean(&before);
        assert_eq!(
            f.session().selection.primary,
            SemanticTarget::EffectClip(f.clip)
        );
    }
}

#[test]
fn escape_cancels_each_kind_and_late_release_cannot_commit() {
    for kind in [
        TimelineDragKind::Move,
        TimelineDragKind::TrimStart,
        TimelineDragKind::TrimEnd,
    ] {
        let mut f = Fixture::once();
        let target = f.target(kind);
        let before = f.session().effect.clone();
        f.begin(target);
        f.move_by(target, 25.0);
        f.escape();
        assert!(f.state().effect_clip_drag.is_none());
        assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
        f.end(target);
        f.clean(&before);
    }
}

#[test]
fn mismatched_clip_or_kind_cannot_move_or_finish_the_original_gesture() {
    for wrong_clip in [false, true] {
        let mut f = Fixture::once();
        let target = f.target(TimelineDragKind::Move);
        let wrong = f.target(if wrong_clip {
            TimelineDragKind::Move
        } else {
            TimelineDragKind::TrimEnd
        });
        if wrong_clip {
            f.app
                .world_mut()
                .get_mut::<TimelineEffectClipInteraction>(wrong)
                .unwrap()
                .clip = EffectClipId::new();
        }
        let before = f.session().effect.clone();
        f.begin(target);
        f.move_by(wrong, 25.0);
        f.end(wrong);
        near(f.state().effect_clip_drag.unwrap().current_start, 1.0);
        f.clean(&before);
        f.move_by(target, 25.0);
        f.end(target);
        near(f.clip().start_time, 1.25);
        f.roundtrip(&before);
    }
}

#[test]
fn motion_and_release_without_an_active_gesture_are_nonsemantic() {
    let mut f = Fixture::once();
    let target = f.target(TimelineDragKind::Move);
    let before = f.session().effect.clone();
    f.move_by(target, 25.0);
    f.end(target);
    near(f.session().time(), 0.0);
    f.clean(&before);
}

#[test]
fn frames_and_seconds_snap_both_move_and_trim_boundaries() {
    for mode in [TimelineSnapMode::Frames, TimelineSnapMode::Seconds] {
        for kind in [
            TimelineDragKind::Move,
            TimelineDragKind::TrimStart,
            TimelineDragKind::TrimEnd,
        ] {
            let mut f = Fixture::once();
            f.app.world_mut().resource_mut::<TimelineState>().snap = mode;
            let target = f.target(kind);
            f.begin(target);
            f.move_by(target, 12.3);
            let drag = f.state().effect_clip_drag.unwrap();
            let interval = match mode {
                TimelineSnapMode::Frames => 1.0 / f.session().tick_rate() as f32,
                _ => nice_timeline_step(4.0, 400.0) / 5.0,
            };
            let original = if kind == TimelineDragKind::TrimEnd {
                2.0
            } else {
                1.0
            };
            let expected = ((original + 0.123) / interval).round() * interval;
            let boundary = if kind == TimelineDragKind::TrimEnd {
                drag.current_start + drag.current_duration
            } else {
                drag.current_start
            };
            near(boundary, expected);
            assert_eq!(f.state().snap_guide, Some(expected));
        }
    }
}

#[test]
fn smart_move_can_choose_the_end_boundary_over_the_start() {
    let mut f = Fixture::once();
    let mut effect = f.session().effect.clone();
    let other = EffectClip::new(f.clip().source, 2.004, 1.0);
    effect.effect_clips.push(other);
    f.app
        .insert_resource(EditorSession::from_test_effect(effect));
    f.app.world_mut().resource_mut::<TimelineState>().snap = TimelineSnapMode::Smart;
    let target = f.target(TimelineDragKind::Move);
    f.begin(target);
    f.move_by(target, 0.3);
    near(f.state().effect_clip_drag.unwrap().current_start, 1.004);
    assert_eq!(f.state().snap_guide, Some(2.004));
}

#[test]
fn unsnapped_motion_clears_old_snap_guide() {
    let mut f = Fixture::once();
    let target = f.target(TimelineDragKind::Move);
    f.begin(target);
    f.app.world_mut().resource_mut::<TimelineState>().snap_guide = Some(3.0);
    f.move_by(target, 12.3);
    near(f.state().effect_clip_drag.unwrap().current_start, 1.123);
    assert!(f.state().snap_guide.is_none());
}

#[test]
fn moving_marker_anchored_clip_updates_offset_not_marker_as_one_edit() {
    let mut f = Fixture::once();
    let mut effect = f.session().effect.clone();
    let marker = MarkerId::new();
    effect.markers.push(EffectMarker {
        id: marker,
        name: "Anchor".into(),
        time: 0.5,
    });
    effect.effect_clips[0].start_reference = Some(MarkerTimeReference::new(marker, 0.5));
    f.app
        .insert_resource(EditorSession::from_test_effect(effect));
    let target = f.target(TimelineDragKind::Move);
    let before = f.session().effect.clone();
    f.begin(target);
    f.move_by(target, 25.0);
    f.end(target);
    near(f.clip().start_reference.unwrap().offset, 0.75);
    near(f.session().effect.markers[0].time, 0.5);
    f.roundtrip(&before);
}

#[test]
fn deleting_clip_during_drag_prevents_release_from_resurrecting_it() {
    let mut f = Fixture::once();
    let target = f.target(TimelineDragKind::Move);
    f.begin(target);
    f.move_by(target, 25.0);
    f.action(ChoreographyAction::DeleteEffectClip(f.clip));
    let after_delete = f.session().effect.clone();
    assert!(after_delete.effect_clips.is_empty());
    f.end(target);
    assert_eq!(f.session().effect, after_delete);
    f.action(HistoryAction::Undo);
    near(f.clip().start_time, 1.0);
    assert!(!f.session().can_undo());
    f.unchanged_source();
}

#[test]
fn clip_release_and_timeline_action_share_global_history_order() {
    let mut f = Fixture::once();
    let target = f.target(TimelineDragKind::Move);
    let before = f.session().effect.clone();
    f.begin(target);
    f.move_by(target, 25.0);
    f.end(target);
    let moved = f.session().effect.clone();
    f.action(super::super::TimelineAction::AddMarker);
    let with_marker = f.session().effect.clone();
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, moved);
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, before);
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, moved);
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, with_marker);
    f.unchanged_source();
}
