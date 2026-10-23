//! Real snapshot payload, catalog/placement guards, pointer observers and global history.
use super::super::timeline_pointer::tests as pointer;
use super::*;
use crate::{
    CurvesState,
    asset_drop::AssetPayload,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
    persistence::DocumentProtectionState,
};
use aestra_core::EffectAsset;
use bevy::picking::pointer::PointerId;
use std::time::Duration;

struct Fixture {
    root: tempfile::TempDir,
    app: App,
    window: Entity,
    source: Entity,
    target: Entity,
    feedback: Entity,
    title: Entity,
    message: Entity,
    before: EffectAsset,
    child: EffectAsset,
    bytes: Vec<u8>,
}
impl Fixture {
    fn new(canvas: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let child = EffectAsset::new("Dragged effect", 0.6);
        let path = root.path().join("child.aestra.ron");
        child.save_ron(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let payload = AssetPayload::capture(
            &catalog,
            catalog
                .content()
                .source_tree()
                .at_relative_path("child.aestra.ron")
                .unwrap()
                .id,
        );
        let effect = crate::test_support::effect_with_timing_slack();
        let mut app = App::new();
        app.insert_resource(EditorSession::from_test_effect(effect.clone()))
            .insert_resource(catalog)
            .insert_resource(TimelineState::framed(2.0))
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<CurvesState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Time>()
            .init_resource::<DocumentProtectionState>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<FunctionEditor>()
            .add_observer(begin_project_effect_drag_preview)
            .add_observer(finish_project_effect_drag_preview)
            .add_observer(reject_project_effect_drop)
            .add_observer(
                |event: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*event, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history expected")
                    })
                },
            )
            .add_systems(
                Update,
                (
                    asset_drop::clear_cancelled_preview,
                    tick_invalid_timeline_drop_feedback,
                )
                    .chain(),
            );
        let window = app.world_mut().spawn(Window::default()).id();
        let source = app.world_mut().spawn(payload).id();
        let mut target = app.world_mut().spawn_empty();
        if canvas {
            target
                .insert((
                    TimelineCanvas,
                    RelativeCursorPosition {
                        normalized: Some(Vec2::new(-0.25, 0.0)),
                        ..default()
                    },
                ))
                .observe(drop_project_effect_on_timeline);
        } else {
            target.observe(drop_project_effect_on_track_headers);
        }
        target
            .observe(show_invalid_timeline_drop_feedback)
            .observe(hide_invalid_timeline_drop_feedback);
        let target = target.id();
        let feedback = app
            .world_mut()
            .spawn((
                TimelineInvalidDropFeedback::default(),
                Node {
                    display: Display::None,
                    ..default()
                },
            ))
            .id();
        let title = app
            .world_mut()
            .spawn((TimelineDropFeedbackTitle, Text::new("")))
            .id();
        let message = app
            .world_mut()
            .spawn((TimelineDropFeedbackMessage, Text::new("")))
            .id();
        Self {
            root,
            app,
            window,
            source,
            target,
            feedback,
            title,
            message,
            before: effect,
            child,
            bytes,
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
        self.app.world_mut().flush();
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn drop_source(&mut self, source: Entity, button: PointerButton) {
        self.action(pointer::drop_on(self.target, source, self.window, button));
    }
    fn enter(&mut self) {
        self.action(pointer::enter(
            self.target,
            self.source,
            self.window,
            PointerButton::Primary,
        ));
    }
    fn clean(&self) {
        assert_eq!(self.session().effect, self.before);
        assert!(!self.session().can_undo());
    }
    fn source_unchanged(&self) {
        assert_eq!(
            std::fs::read(self.root.path().join("child.aestra.ron")).unwrap(),
            self.bytes
        );
    }
    fn roundtrip(&mut self) {
        let after = self.session().effect.clone();
        assert_ne!(after, self.before);
        self.action(HistoryAction::Undo);
        self.clean();
        self.action(HistoryAction::Redo);
        assert_eq!(self.session().effect, after);
        self.source_unchanged();
    }
    fn rejected(&self) {
        assert!(
            self.app
                .world()
                .get::<TimelineInvalidDropFeedback>(self.feedback)
                .unwrap()
                .rejected
        );
        assert_eq!(
            self.app.world().get::<Node>(self.feedback).unwrap().display,
            Display::Flex
        );
        assert!(
            self.session()
                .status
                .starts_with("Effect clip was not added:")
        );
        self.clean();
    }
}

#[test]
fn canvas_and_header_drops_from_nested_source_are_one_global_transaction() {
    for canvas in [false, true] {
        let mut f = Fixture::new(canvas);
        f.app
            .world_mut()
            .resource_mut::<EditorSession>()
            .material_history_active = true;
        let graph = f.session().material_target.clone();
        let label = f.app.world_mut().spawn(ChildOf(f.source)).id();
        f.drop_source(label, PointerButton::Primary);
        let clip = f.session().effect.effect_clips.last().unwrap();
        assert_eq!(clip.source.id, f.child.id);
        assert_eq!(clip.duration, 0.6);
        assert_eq!(clip.start_time, if canvas { 0.5 } else { 0.0 });
        assert!(!f.session().material_history_active);
        assert_eq!(f.session().material_target, graph);
        assert!(
            !f.app
                .world()
                .get::<TimelineInvalidDropFeedback>(f.feedback)
                .unwrap()
                .rejected
        );
        f.roundtrip();
    }
}
#[test]
fn drop_inserts_relative_to_mixed_track_order_without_rewriting_source() {
    let mut f = Fixture::new(false);
    let target = ChoreographyTrackId::Emitter(f.before.emitters[0].id);
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .effect_drop_insertion = Some((target, true));
    f.drop_source(f.source, PointerButton::Primary);
    let id = f.session().effect.effect_clips.last().unwrap().id;
    assert_eq!(
        f.session().effect.choreography_order[0],
        ChoreographyTrackId::EffectClip(id)
    );
    assert!(f.state().effect_drop_insertion.is_none());
    f.roundtrip();
}
#[test]
fn header_drop_uses_playhead_and_source_placement_clamps_to_owner() {
    let mut f = Fixture::new(false);
    let end = f.session().playback_duration();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(end);
    f.drop_source(f.source, PointerButton::Primary);
    assert!(
        (f.session().effect.effect_clips.last().unwrap().start_time - (end - 0.6)).abs() < 0.0001
    );
    f.roundtrip();
}
#[test]
fn secondary_and_nonpayload_drops_do_not_edit_or_reject() {
    let mut f = Fixture::new(true);
    f.drop_source(f.source, PointerButton::Secondary);
    let unrelated = f.app.world_mut().spawn_empty().id();
    f.drop_source(unrelated, PointerButton::Primary);
    f.clean();
    assert!(
        !f.app
            .world()
            .get::<TimelineInvalidDropFeedback>(f.feedback)
            .unwrap()
            .rejected
    );
}
#[test]
fn unknown_or_ambiguous_canvas_position_rejects_without_editing() {
    for multiple in [false, true] {
        let mut f = Fixture::new(true);
        if multiple {
            f.app
                .world_mut()
                .spawn((TimelineCanvas, RelativeCursorPosition::default()));
        } else {
            f.app
                .world_mut()
                .get_mut::<RelativeCursorPosition>(f.target)
                .unwrap()
                .normalized = None;
        }
        f.drop_source(f.source, PointerButton::Primary);
        f.rejected();
    }
}
#[test]
fn escape_at_release_rejects_and_keeps_document_work() {
    let mut f = Fixture::new(false);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    f.drop_source(f.source, PointerButton::Primary);
    f.rejected();
    assert!(f.session().status.contains("cancelled"));
}
#[test]
fn modal_and_pending_proposal_guards_preserve_original_work() {
    for pending in [false, true] {
        let mut f = Fixture::new(false);
        if pending {
            assert!(
                f.app
                    .world_mut()
                    .resource_mut::<EditorSession>()
                    .preview_transaction(EffectTransaction::single(
                        "Proposal",
                        EffectCommand::SetEffectName {
                            name: "Keep proposal".into()
                        }
                    ))
            );
        } else {
            f.app
                .world_mut()
                .resource_mut::<DocumentProtectionState>()
                .asset_delete_open = true;
        }
        f.drop_source(f.source, PointerButton::Primary);
        f.rejected();
        assert_eq!(f.session().pending_change.is_some(), pending);
    }
}
#[test]
fn stale_snapshot_and_duplicate_source_identity_reject() {
    for duplicate in [false, true] {
        let mut f = Fixture::new(false);
        f.child
            .save_ron(f.root.path().join(if duplicate {
                "duplicate.aestra.ron"
            } else {
                "renamed.aestra.ron"
            }))
            .unwrap();
        f.app
            .world_mut()
            .resource_mut::<ProjectEffectCatalog>()
            .refresh();
        if duplicate {
            let catalog = f.app.world().resource::<ProjectEffectCatalog>();
            let id = catalog
                .content()
                .source_tree()
                .at_relative_path("child.aestra.ron")
                .unwrap()
                .id;
            let payload = AssetPayload::capture(catalog, id);
            f.app.world_mut().entity_mut(f.source).insert(payload);
        }
        f.drop_source(f.source, PointerButton::Primary);
        f.rejected();
    }
}
#[test]
fn self_reference_rejects_before_creating_a_clip() {
    let mut f = Fixture::new(false);
    f.before.id = f.child.id;
    f.app
        .insert_resource(EditorSession::from_test_effect(f.before.clone()));
    f.drop_source(f.source, PointerButton::Primary);
    f.rejected();
}

#[test]
fn cyclic_source_graph_rejects_without_editing_owner() {
    let mut f = Fixture::new(false);
    f.before
        .save_ron(f.root.path().join("owner.aestra.ron"))
        .unwrap();
    f.child
        .effect_clips
        .push(EffectClip::new(f.before.id, 0.0, 0.5));
    f.child
        .save_ron(f.root.path().join("child.aestra.ron"))
        .unwrap();
    f.app
        .world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    let catalog = f.app.world().resource::<ProjectEffectCatalog>();
    let id = catalog
        .content()
        .source_tree()
        .at_relative_path("child.aestra.ron")
        .unwrap()
        .id;
    let payload = AssetPayload::capture(catalog, id);
    f.app.world_mut().entity_mut(f.source).insert(payload);
    f.drop_source(f.source, PointerButton::Primary);
    f.rejected();
}

#[test]
fn incompatible_file_payload_rejects_with_actionable_feedback() {
    let mut f = Fixture::new(false);
    std::fs::write(f.root.path().join("note.txt"), b"not an effect").unwrap();
    f.app
        .world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    let catalog = f.app.world().resource::<ProjectEffectCatalog>();
    let id = catalog
        .content()
        .source_tree()
        .at_relative_path("note.txt")
        .unwrap()
        .id;
    let payload = AssetPayload::capture(catalog, id);
    f.app.world_mut().entity_mut(f.source).insert(payload);
    f.drop_source(f.source, PointerButton::Primary);
    f.rejected();
    assert!(
        f.app
            .world()
            .get::<Text>(f.message)
            .unwrap()
            .0
            .contains("effect assets")
    );
}
#[test]
fn drag_preview_and_enter_leave_are_nonsemantic() {
    let mut f = Fixture::new(true);
    f.action(pointer::start(
        f.source,
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    assert_eq!(
        f.state()
            .effect_drop_preview
            .as_ref()
            .unwrap()
            .source_duration,
        0.6
    );
    f.enter();
    assert!(f.state().browser_drop.is_some());
    f.clean();
    f.action(pointer::leave(
        f.target,
        f.source,
        f.window,
        PointerButton::Primary,
    ));
    assert!(f.state().browser_drop.is_none() && f.state().effect_drop_preview.is_none());
    f.clean();
}
#[test]
fn escape_or_removed_source_clears_hover_preview_and_insertion() {
    for escape in [false, true] {
        let mut f = Fixture::new(true);
        f.enter();
        f.app
            .world_mut()
            .resource_mut::<TimelineState>()
            .effect_drop_insertion =
            Some((ChoreographyTrackId::Emitter(f.before.emitters[0].id), true));
        if escape {
            f.app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
        } else {
            f.app.world_mut().despawn(f.source);
        }
        f.app.update();
        assert!(
            f.state().browser_drop.is_none()
                && f.state().effect_drop_preview.is_none()
                && f.state().effect_drop_insertion.is_none()
        );
        f.clean();
    }
}
#[test]
fn rejection_feedback_survives_leave_then_expires() {
    let mut f = Fixture::new(true);
    f.action(RejectProjectEffectDrop {
        reason: "Test rejection".into(),
    });
    f.action(pointer::leave(
        f.target,
        f.source,
        f.window,
        PointerButton::Primary,
    ));
    assert_eq!(
        f.app.world().get::<Text>(f.message).unwrap().0,
        "Test rejection"
    );
    assert_eq!(
        f.app.world().get::<Text>(f.title).unwrap().0,
        f.app
            .world()
            .resource::<Localizer>()
            .text("timeline-drop-effect-rejected-title")
    );
    f.rejected();
    f.app
        .world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(2));
    f.app.update();
    assert_eq!(
        f.app.world().get::<Node>(f.feedback).unwrap().display,
        Display::None
    );
    assert!(
        f.app
            .world()
            .get::<TimelineInvalidDropFeedback>(f.feedback)
            .unwrap()
            .timer
            .is_paused()
    );
}
#[test]
fn valid_enter_clears_prior_rejection_and_drag_end_clears_source_state() {
    let mut f = Fixture::new(true);
    f.action(RejectProjectEffectDrop {
        reason: "Old rejection".into(),
    });
    f.enter();
    assert_eq!(
        f.app.world().get::<Node>(f.feedback).unwrap().display,
        Display::None
    );
    f.action(pointer::end(
        f.source,
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    assert!(f.state().effect_drop_preview.is_none() && f.state().browser_drop.is_none());
    f.clean();
}
#[test]
fn placement_rejects_invalid_duration_and_insertion_falls_back_to_append() {
    for duration in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(effect_clip_placement(0.0, duration, 1.0).is_none());
        assert!(effect_clip_placement(0.0, 1.0, duration).is_none());
    }
    assert_eq!(effect_clip_placement(-3.0, 2.0, 5.0), Some((0.0, 2.0)));
    assert_eq!(
        choreography_insertion_index(
            &[],
            ChoreographyTrackId::EffectClip(aestra_core::EffectClipId::new()),
            false
        ),
        0
    );
}
