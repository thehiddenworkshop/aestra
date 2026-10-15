//! Headless production controllers with explicit computed geometry and input state.
use super::super::motion_execution::{self, Field, NumberControl};
use super::super::motion_state::{Action as Motion, KeyDrag};
use super::*;
use aestra_authoring::{EffectCommand, EffectTransaction};
use aestra_core::{
    EffectAsset, EffectClipId, EffectId, EmitterTransform, HostTransformKey, HostTransformTrack,
};
use bevy::ui_widgets::ValueChange;

struct Fixture {
    app: App,
    window: Entity,
    canvas: Entity,
    tracks: Entity,
}

fn track() -> HostTransformTrack {
    HostTransformTrack::from_pose_keys(
        [0.0, 1.0, 12.0]
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

impl Fixture {
    fn new() -> Self {
        let mut effect = crate::test_support::effect_with_timing_slack();
        effect.host_transform_track = Some(track());
        let mut app = App::new();
        app.insert_resource(EditorSession::from_test_effect(effect))
            .insert_resource(TimelineState::framed(12.0))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<OverrideCursor>()
            .init_resource::<InputFocus>()
            .add_message::<MouseMotion>()
            .add_message::<MouseWheel>()
            .add_observer(motion_execution::execute)
            .add_observer(motion_execution::number_changed)
            .add_systems(
                Update,
                (
                    navigate_timeline,
                    dismiss_timeline_popovers,
                    restore_timeline_context_menu_focus,
                    motion_execution::cancel_drag,
                )
                    .chain(),
            );
        let window = app
            .world_mut()
            .spawn((
                PrimaryWindow,
                CursorIcon::System(SystemCursorIcon::ColResize),
            ))
            .id();
        let canvas = app
            .world_mut()
            .spawn((
                TimelineCanvas,
                RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::ZERO),
                },
                ComputedNode {
                    size: Vec2::new(600.0, 100.0),
                    ..default()
                },
            ))
            .id();
        let tracks = app
            .world_mut()
            .spawn((
                TimelineVerticalPane::Tracks,
                ComputedNode {
                    size: Vec2::new(600.0, 100.0),
                    content_size: Vec2::new(600.0, 400.0),
                    ..default()
                },
            ))
            .id();
        Self {
            app,
            window,
            canvas,
            tracks,
        }
    }
    fn tick(&mut self) {
        self.app.update();
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
    }
    fn state(&self) -> &TimelineState {
        self.app.world().resource::<TimelineState>()
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource::<EditorSession>()
    }
    fn wheel(&mut self, unit: MouseScrollUnit, x: f32, y: f32) {
        self.app.world_mut().write_message(MouseWheel {
            unit,
            x,
            y,
            window: self.window,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
    }
    fn key(&mut self, key: KeyCode) {
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
    }
    fn button(&mut self, button: MouseButton) {
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(button);
    }
    fn hover(&mut self, over: bool) {
        self.app
            .world_mut()
            .get_mut::<RelativeCursorPosition>(self.canvas)
            .unwrap()
            .cursor_over = over;
    }
    fn open_popups(&mut self) {
        let emitter = self.session().effect.emitters[0].id;
        let mut state = self.app.world_mut().resource_mut::<TimelineState>();
        state.color_picker_emitter = Some(emitter);
        state.context_emitter = Some(emitter);
        state.automation_menu_emitter = Some(emitter);
    }
    fn no_edit(&self, before: &EffectAsset) {
        assert_eq!(&self.session().effect, before);
        assert!(!self.session().dirty);
        assert!(!self.session().can_undo());
    }
    fn number_preview(&mut self) -> Entity {
        self.app.world_mut().trigger(Motion::Select(1));
        self.app.world_mut().flush();
        assert_eq!(self.state().host_motion.selected, Some(1));
        let effect = self.session().effect.id;
        let entity = self
            .app
            .world_mut()
            .spawn(NumberControl {
                effect,
                index: 1,
                field: Field::Position(0),
                initial: 10.0,
            })
            .id();
        self.change(entity, false);
        assert!(
            self.state().host_motion.number_preview.is_some(),
            "{:?}",
            self.state().host_motion
        );
        assert_eq!(
            self.session()
                .preview()
                .unwrap()
                .host_transform_at(1.0)
                .translation[0],
            18.0
        );
        entity
    }
    fn change(&mut self, source: Entity, is_final: bool) {
        self.app.world_mut().trigger(ValueChange {
            source,
            value: 18.0_f32,
            is_final,
        });
        self.app.world_mut().flush();
    }
    fn drag_preview(&mut self) {
        let original = self.session().effect.host_transform_track.clone().unwrap();
        let mut candidate = original.clone();
        candidate.set_pose(
            1.0,
            EmitterTransform {
                translation: [18.0, 0.0, 0.0],
                ..default()
            },
        );
        let effect = self.session().effect.id;
        assert!(
            self.app
                .world_mut()
                .resource_mut::<EditorSession>()
                .preview_interaction(EffectTransaction::single(
                    "Preview host pose",
                    EffectCommand::SetHostTransformTrack {
                        track: Some(candidate.clone()),
                    }
                ))
        );
        self.app
            .world_mut()
            .resource_mut::<TimelineState>()
            .host_motion
            .drag = Some(KeyDrag {
            effect,
            index: 1,
            original,
            candidate,
            selected: 1,
            start: 1.0,
        });
    }
}

#[test]
fn vertical_line_and_pixel_wheel_use_track_units() {
    for (unit, amount) in [
        (MouseScrollUnit::Line, -2.0),
        (MouseScrollUnit::Pixel, -42.0),
    ] {
        let mut f = Fixture::new();
        f.wheel(unit, 0.0, amount);
        f.tick();
        assert_eq!(f.state().vertical_scroll, 42.0);
        assert_eq!(f.state().view.start, 0.0);
        assert_eq!(f.state().view.end, 12.0);
    }
}

#[test]
fn track_scroll_clamps_to_content_and_ignores_header_extent() {
    let mut f = Fixture::new();
    f.app.world_mut().spawn((
        TimelineVerticalPane::Headers,
        ComputedNode {
            size: Vec2::splat(100.0),
            content_size: Vec2::splat(2000.0),
            ..default()
        },
    ));
    f.wheel(MouseScrollUnit::Pixel, 0.0, -1000.0);
    f.tick();
    assert_eq!(f.state().vertical_scroll, 300.0);
    f.wheel(MouseScrollUnit::Pixel, 0.0, 1000.0);
    f.tick();
    assert_eq!(f.state().vertical_scroll, 0.0);
}

#[test]
fn missing_or_nonoverflowing_track_pane_has_no_scroll_range() {
    for missing in [false, true] {
        let mut f = Fixture::new();
        if missing {
            f.app.world_mut().despawn(f.tracks);
        } else {
            f.app
                .world_mut()
                .get_mut::<ComputedNode>(f.tracks)
                .unwrap()
                .content_size
                .y = 50.0;
        }
        f.wheel(MouseScrollUnit::Line, 0.0, -10.0);
        f.tick();
        assert_eq!(f.state().vertical_scroll, 0.0);
    }
}

#[test]
fn horizontal_or_shift_wheel_pans_time_without_scrolling_tracks() {
    for shift in [false, true] {
        let mut f = Fixture::new();
        f.app.world_mut().resource_mut::<TimelineState>().view =
            super::super::state::TimelineView {
                start: 2.0,
                end: 6.0,
            };
        if shift {
            f.key(KeyCode::ShiftRight);
        }
        f.wheel(
            MouseScrollUnit::Line,
            if shift { 0.0 } else { -1.0 },
            if shift { -1.0 } else { 0.0 },
        );
        f.tick();
        assert!((f.state().view.start - 2.32).abs() < 0.0001);
        assert_eq!(f.state().vertical_scroll, 0.0);
    }
}

#[test]
fn control_zoom_preserves_pointer_anchor_and_wins_over_shift() {
    for key in [KeyCode::ControlLeft, KeyCode::ControlRight] {
        let mut f = Fixture::new();
        f.key(key);
        f.key(KeyCode::ShiftLeft);
        f.app
            .world_mut()
            .get_mut::<RelativeCursorPosition>(f.canvas)
            .unwrap()
            .normalized = Some(Vec2::new(-0.25, 0.0));
        f.wheel(MouseScrollUnit::Line, 0.0, 1.0);
        f.tick();
        assert!((f.state().view.span() - 12.0 * 0.82).abs() < 0.0001);
        assert!((f.state().view.time_at(0.25) - 3.0).abs() < 0.0001);
    }
}

#[test]
fn control_zoom_requires_known_pointer_position() {
    let mut f = Fixture::new();
    f.key(KeyCode::ControlLeft);
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(f.canvas)
        .unwrap()
        .normalized = None;
    f.wheel(MouseScrollUnit::Line, 0.0, 3.0);
    f.tick();
    assert_eq!(f.state().view.span(), 12.0);
}

#[test]
fn wheel_outside_canvas_is_drained_without_delayed_navigation() {
    let mut f = Fixture::new();
    f.hover(false);
    f.wheel(MouseScrollUnit::Line, 0.0, -2.0);
    f.tick();
    f.hover(true);
    f.tick();
    assert_eq!(f.state().vertical_scroll, 0.0);
}

#[test]
fn middle_pan_scales_motion_by_canvas_width_and_stops_on_release() {
    let mut f = Fixture::new();
    f.app.world_mut().resource_mut::<TimelineState>().view = super::super::state::TimelineView {
        start: 2.0,
        end: 6.0,
    };
    f.button(MouseButton::Middle);
    f.app.world_mut().write_message(MouseMotion {
        delta: Vec2::new(-60.0, 800.0),
    });
    f.tick();
    assert!(f.state().panning);
    assert!((f.state().view.start - 2.4).abs() < 0.0001);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Middle);
    f.app.world_mut().write_message(MouseMotion {
        delta: Vec2::new(-60.0, 0.0),
    });
    f.tick();
    assert!(!f.state().panning);
    assert!((f.state().view.start - 2.4).abs() < 0.0001);
}

#[test]
fn middle_press_outside_does_not_start_pan_when_pointer_enters() {
    let mut f = Fixture::new();
    f.hover(false);
    f.button(MouseButton::Middle);
    f.tick();
    f.hover(true);
    f.app.world_mut().write_message(MouseMotion {
        delta: Vec2::new(-60.0, 0.0),
    });
    f.tick();
    assert!(!f.state().panning);
    assert_eq!(f.state().view.start, 0.0);
}

#[test]
fn navigation_includes_host_motion_extent_without_editing_document() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    assert!(f.session().playback_duration() < 12.0);
    f.app.world_mut().resource_mut::<TimelineState>().view = super::super::state::TimelineView {
        start: 7.0,
        end: 11.0,
    };
    f.wheel(MouseScrollUnit::Line, -1.0, 0.0);
    f.tick();
    assert!(f.state().view.end > 11.0);
    f.no_edit(&before);
}

#[test]
fn outside_primary_click_dismisses_all_popups_once_and_restores_emitter_focus() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    let emitter = before.emitters[0].id;
    let header = f.app.world_mut().spawn(EmitterTrackHeader { emitter }).id();
    f.open_popups();
    let revision = f.session().ui_revision;
    f.button(MouseButton::Left);
    f.tick();
    assert!(f.state().color_picker_emitter.is_none());
    assert!(f.state().context_emitter.is_none());
    assert!(f.state().automation_menu_emitter.is_none());
    assert_eq!(f.session().ui_revision, revision + 1);
    assert_eq!(f.app.world().resource::<InputFocus>().get(), Some(header));
    f.no_edit(&before);
}

#[test]
fn pointer_over_each_popup_surface_protects_its_popup() {
    let mut f = Fixture::new();
    f.open_popups();
    let cursor = RelativeCursorPosition {
        cursor_over: true,
        normalized: Some(Vec2::ZERO),
    };
    f.app.world_mut().spawn((EmitterTrackColorChip, cursor));
    f.app
        .world_mut()
        .spawn((EffectClipTrackContextMenu, cursor));
    f.app
        .world_mut()
        .spawn((EmitterAutomationMenuButton, cursor));
    let revision = f.session().ui_revision;
    f.button(MouseButton::Left);
    f.tick();
    assert!(f.state().color_picker_emitter.is_some());
    assert!(f.state().context_emitter.is_some());
    assert!(f.state().automation_menu_emitter.is_some());
    assert_eq!(f.session().ui_revision, revision);
}

#[test]
fn right_click_and_idle_do_not_dismiss_popups() {
    let mut f = Fixture::new();
    f.open_popups();
    f.tick();
    f.button(MouseButton::Right);
    f.tick();
    assert!(f.state().color_picker_emitter.is_some());
    assert!(f.state().context_emitter.is_some());
    assert!(f.state().automation_menu_emitter.is_some());
}

#[test]
fn escape_dismisses_popups_once_and_restores_clip_focus() {
    let mut f = Fixture::new();
    let clip = EffectClipId::new();
    let header = f.app.world_mut().spawn(EffectClipTrackHeader { clip }).id();
    f.open_popups();
    {
        let mut s = f.app.world_mut().resource_mut::<TimelineState>();
        s.context_emitter = None;
        s.context_effect_clip = Some(clip);
    }
    let revision = f.session().ui_revision;
    f.key(KeyCode::Escape);
    f.tick();
    assert!(f.state().context_effect_clip.is_none());
    assert!(f.state().color_picker_emitter.is_none());
    assert!(f.state().automation_menu_emitter.is_none());
    assert_eq!(f.session().ui_revision, revision + 1);
    assert_eq!(f.app.world().resource::<InputFocus>().get(), Some(header));
}

#[test]
fn focus_restoration_waits_for_real_header_and_focus_resource() {
    let mut f = Fixture::new();
    let emitter = f.session().effect.emitters[0].id;
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .restore_context_emitter_focus = Some(emitter);
    f.tick();
    assert_eq!(f.state().restore_context_emitter_focus, Some(emitter));
    f.app.world_mut().remove_resource::<InputFocus>();
    let header = f.app.world_mut().spawn(EmitterTrackHeader { emitter }).id();
    f.tick();
    assert_eq!(f.state().restore_context_emitter_focus, Some(emitter));
    f.app.world_mut().init_resource::<InputFocus>();
    f.tick();
    assert_eq!(f.app.world().resource::<InputFocus>().get(), Some(header));
    assert!(f.state().restore_context_emitter_focus.is_none());
}

#[test]
fn emitter_focus_has_priority_over_clip_focus_and_clears_both_targets() {
    let mut f = Fixture::new();
    let emitter = f.session().effect.emitters[0].id;
    let clip = EffectClipId::new();
    let header = f.app.world_mut().spawn(EmitterTrackHeader { emitter }).id();
    f.app.world_mut().spawn(EffectClipTrackHeader { clip });
    {
        let mut s = f.app.world_mut().resource_mut::<TimelineState>();
        s.restore_context_emitter_focus = Some(emitter);
        s.restore_context_effect_clip_focus = Some(clip);
    }
    f.tick();
    assert_eq!(f.app.world().resource::<InputFocus>().get(), Some(header));
    assert!(f.state().restore_context_emitter_focus.is_none());
    assert!(f.state().restore_context_effect_clip_focus.is_none());
}

#[test]
fn escape_clears_all_timeline_drags_snap_and_cursor_without_history() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    let emitter = &before.emitters[0];
    {
        let mut s = f.app.world_mut().resource_mut::<TimelineState>();
        s.drag = Some(TimelineDrag {
            emitter: emitter.id,
            region: aestra_core::EmitterRegionId::new(),
            kind: TimelineDragKind::Move,
            pointer_start: 0.0,
            original_start: 0.0,
            original_duration: 1.0,
            original_source_offset: 0.0,
            current_start: 1.0,
            current_duration: 1.0,
            current_source_offset: 0.0,
            source_duration: 1.0,
        });
        s.effect_clip_drag = Some(EffectClipTimelineDrag {
            clip: EffectClipId::new(),
            kind: TimelineDragKind::Move,
            pointer_start: 0.0,
            original_start: 0.0,
            original_source_offset: 0.0,
            original_duration: 1.0,
            current_start: 1.0,
            current_source_offset: 0.0,
            current_duration: 1.0,
            source_duration: 1.0,
            source_looping: false,
        });
        s.marker_drag = Some(TimelineMarkerDrag {
            marker: aestra_core::MarkerId::new(),
            original_time: 0.0,
            current_time: 1.0,
        });
        s.choreography_event_drag = Some(TimelineChoreographyEventDrag {
            event: aestra_core::ChoreographyEventId::new(),
            original_time: 0.0,
            current_time: 1.0,
        });
        s.snap_guide = Some(1.0);
    }
    f.app.world_mut().resource_mut::<OverrideCursor>().0 = Some(default());
    f.key(KeyCode::Escape);
    f.tick();
    assert!(f.state().drag.is_none());
    assert!(f.state().effect_clip_drag.is_none());
    assert!(f.state().marker_drag.is_none());
    assert!(f.state().choreography_event_drag.is_none());
    assert!(f.state().snap_guide.is_none());
    assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
    assert_eq!(
        *f.app.world().get::<CursorIcon>(f.window).unwrap(),
        CursorIcon::System(SystemCursorIcon::Default)
    );
    f.no_edit(&before);
}

#[test]
fn live_host_drag_is_retained_until_escape_and_preview_is_restored() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    f.drag_preview();
    f.tick();
    assert!(f.state().host_motion.drag.is_some());
    assert_eq!(
        f.session()
            .preview()
            .unwrap()
            .host_transform_at(1.0)
            .translation[0],
        18.0
    );
    f.key(KeyCode::Escape);
    f.tick();
    assert!(f.state().host_motion.drag.is_none());
    assert_eq!(
        f.session()
            .preview()
            .unwrap()
            .host_transform_at(1.0)
            .translation[0],
        10.0
    );
    f.no_edit(&before);
}

#[test]
fn stale_host_drag_effect_or_track_cancels_without_committing() {
    for stale_effect in [false, true] {
        let mut f = Fixture::new();
        let before = f.session().effect.clone();
        f.drag_preview();
        {
            let mut s = f.app.world_mut().resource_mut::<TimelineState>();
            let drag = s.host_motion.drag.as_mut().unwrap();
            if stale_effect {
                drag.effect = EffectId::new();
            } else {
                drag.original.repeat = true;
            }
        }
        f.tick();
        assert!(f.state().host_motion.drag.is_none());
        assert_eq!(
            f.session()
                .preview()
                .unwrap()
                .host_transform_at(1.0)
                .translation[0],
            10.0
        );
        f.no_edit(&before);
    }
}

#[test]
fn numeric_preview_survives_idle_then_escape_blocks_late_final_value() {
    let mut f = Fixture::new();
    let source = f.number_preview();
    let before = f.session().effect.clone();
    f.tick();
    assert!(f.state().host_motion.number_preview.is_some());
    let revision = f.session().ui_revision;
    f.key(KeyCode::Escape);
    f.tick();
    assert!(f.state().host_motion.number_preview.is_none());
    assert_eq!(f.state().host_motion.cancelled_input, Some(source));
    assert_eq!(f.session().ui_revision, revision + 1);
    f.change(source, true);
    assert_eq!(
        f.session()
            .preview()
            .unwrap()
            .host_transform_at(1.0)
            .translation[0],
        10.0
    );
    f.no_edit(&before);
}

#[test]
fn numeric_preview_cancels_for_selection_effect_track_or_inactive_owner() {
    for cause in 0..5 {
        let mut f = Fixture::new();
        let source = f.number_preview();
        let before = f.session().effect.clone();
        {
            let mut s = f.app.world_mut().resource_mut::<TimelineState>();
            match cause {
                0 => s.host_motion.selected = Some(0),
                1 => s.host_motion.number_preview.as_mut().unwrap().effect = EffectId::new(),
                2 => {
                    s.host_motion
                        .number_preview
                        .as_mut()
                        .unwrap()
                        .original
                        .repeat = true
                }
                3 => s.host_motion.effect = None,
                _ => {
                    s.inspected_child = Some(EffectClipChildSelection::EffectClip {
                        path: EffectClipPath::root_path(EffectClipId::new()),
                    })
                }
            }
        }
        f.tick();
        assert!(f.state().host_motion.number_preview.is_none());
        assert_eq!(f.state().host_motion.cancelled_input, Some(source));
        assert_eq!(
            f.session()
                .preview()
                .unwrap()
                .host_transform_at(1.0)
                .translation[0],
            10.0
        );
        f.change(source, true);
        f.no_edit(&before);
    }
}

#[test]
fn removing_numeric_control_cancels_preview_and_clears_cancelled_identity() {
    let mut f = Fixture::new();
    let source = f.number_preview();
    let before = f.session().effect.clone();
    f.app.world_mut().despawn(source);
    f.tick();
    assert!(f.state().host_motion.number_preview.is_none());
    assert!(f.state().host_motion.cancelled_input.is_none());
    assert_eq!(
        f.session()
            .preview()
            .unwrap()
            .host_transform_at(1.0)
            .translation[0],
        10.0
    );
    f.no_edit(&before);
}

#[test]
fn cancelled_numeric_identity_is_retired_when_control_is_removed() {
    let mut f = Fixture::new();
    let source = f.number_preview();
    f.key(KeyCode::Escape);
    f.tick();
    assert_eq!(f.state().host_motion.cancelled_input, Some(source));
    f.app.world_mut().despawn(source);
    f.tick();
    assert!(f.state().host_motion.cancelled_input.is_none());
}

#[test]
fn wheel_intent_and_cursor_fraction_keep_existing_boundary_policy() {
    assert_eq!(timeline_cursor_fraction(-2.0), 0.0);
    assert_eq!(timeline_cursor_fraction(2.0), 1.0);
    assert_eq!(
        timeline_wheel_intent(Vec2::new(3.0, -2.0), -42.0, true, true),
        TimelineWheelIntent::ZoomTime(3.0)
    );
    assert_eq!(
        timeline_wheel_intent(Vec2::new(2.0, -2.0), -42.0, false, false),
        TimelineWheelIntent::ScrollTracks(-42.0)
    );
    assert!(should_dismiss_pointer_context_menu(true, false, true, true));
    assert!(!should_dismiss_pointer_context_menu(
        false, true, true, false
    ));
    assert!(!should_dismiss_timeline_popover(true, true, true));
}
