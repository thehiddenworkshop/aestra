//! Actual reorder pointers and global semantic order history on both engines.
use super::super::timeline_pointer::tests as pointer;
use super::*;
use crate::{
    ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
};
use aestra_core::{EffectAsset, EffectClip, EffectId};
use bevy::picking::pointer::{PointerButton, PointerId};

struct Fixture {
    _root: tempfile::TempDir,
    app: App,
    window: Entity,
    before: EffectAsset,
    source: Entity,
    target: Entity,
    clip: bool,
}
impl Fixture {
    fn new(clip: bool, before_target: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut effect = crate::test_support::effect_with_timing_slack();
        let child = EffectClip::new(EffectId::new(), 0.0, 1.0);
        let id = child.id;
        effect.effect_clips.push(child);
        let mut app = App::new();
        app.insert_resource(EditorSession::from_test_effect(effect.clone()))
            .insert_resource(TimelineState::framed(4.0))
            .insert_resource(Localizer::new("en-US").unwrap())
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .init_resource::<OverrideCursor>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<FunctionEditor>()
            .add_observer(
                |event: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*event, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history expected")
                    })
                },
            );
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow, CursorIcon::default()))
            .id();
        let mut source = app.world_mut().spawn_empty();
        if clip {
            source
                .insert(EffectClipTrackReorderHandle { clip: id })
                .observe(begin_effect_clip_track_reorder)
                .observe(finish_effect_clip_track_reorder);
        } else {
            source
                .insert(EmitterTrackReorderHandle {
                    emitter: effect.emitters[0].id,
                })
                .observe(begin_emitter_track_reorder)
                .observe(finish_emitter_track_reorder);
        }
        let source = source.id();
        let mut target = app.world_mut().spawn((
            TimelineTrackHeader {
                track: ChoreographyTrackId::Emitter(effect.emitters[1].id),
            },
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::new(0.0, if before_target { -0.2 } else { 0.2 })),
            },
        ));
        if clip {
            target.observe(drop_effect_clip_track_reorder);
        } else {
            target.observe(drop_emitter_track_reorder);
        }
        let target = target.id();
        Self {
            _root: root,
            app,
            window,
            before: effect,
            source,
            target,
            clip,
        }
    }
    fn action<E: Event>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().flush();
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn begin(&mut self, button: PointerButton) {
        self.action(pointer::start(
            self.source,
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            button,
        ));
    }
    fn end(&mut self) {
        self.action(pointer::end(
            self.source,
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
    }
    fn drop(&mut self, source: Entity, button: PointerButton) {
        self.action(pointer::drop_on(self.target, source, self.window, button));
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource::<EditorSession>()
    }
    fn state(&self) -> &TimelineState {
        self.app.world().resource::<TimelineState>()
    }
    fn clean(&self) {
        assert_eq!(self.session().effect, self.before);
        assert!(!self.session().can_undo());
    }
    fn dragged(&self) -> Option<ChoreographyTrackId> {
        if self.clip {
            self.state()
                .effect_clip_reorder_drag
                .map(ChoreographyTrackId::EffectClip)
        } else {
            self.state().reorder_drag.map(ChoreographyTrackId::Emitter)
        }
    }
    fn roundtrip(&mut self) {
        let after = self.session().effect.clone();
        assert_ne!(after, self.before);
        self.action(HistoryAction::Undo);
        self.clean();
        self.action(HistoryAction::Redo);
        assert_eq!(self.session().effect, after);
    }
}
#[test]
fn begin_and_end_set_real_cursor_and_state_without_editing() {
    for clip in [false, true] {
        let mut f = Fixture::new(clip, false);
        f.begin(PointerButton::Primary);
        assert!(f.dragged().is_some());
        assert_eq!(
            *f.app.world().get::<CursorIcon>(f.window).unwrap(),
            CursorIcon::System(SystemCursorIcon::Grabbing)
        );
        f.clean();
        f.end();
        assert!(f.dragged().is_none());
        assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
        assert_eq!(
            *f.app.world().get::<CursorIcon>(f.window).unwrap(),
            CursorIcon::System(SystemCursorIcon::Grab)
        );
        f.clean();
    }
}
#[test]
fn emitter_and_effect_clip_reorder_across_mixed_tracks_is_one_edit() {
    for clip in [false, true] {
        let mut f = Fixture::new(clip, false);
        f.begin(PointerButton::Primary);
        let source = f.dragged().unwrap();
        let label = f.app.world_mut().spawn(ChildOf(f.source)).id();
        f.drop(label, PointerButton::Primary);
        let order = normalized_choreography_order(&f.session().effect);
        let target = ChoreographyTrackId::Emitter(f.before.emitters[1].id);
        assert_eq!(
            order.iter().position(|x| *x == source).unwrap(),
            order.iter().position(|x| *x == target).unwrap() + 1
        );
        assert!(f.dragged().is_none());
        assert_eq!(
            *f.app.world().get::<CursorIcon>(f.window).unwrap(),
            CursorIcon::System(SystemCursorIcon::Default)
        );
        f.roundtrip();
    }
}
#[test]
fn clip_drop_before_emitter_preserves_clip_and_emitter_payloads() {
    let mut f = Fixture::new(true, true);
    f.drop(f.source, PointerButton::Primary);
    let order = normalized_choreography_order(&f.session().effect);
    let source = ChoreographyTrackId::EffectClip(f.before.effect_clips[0].id);
    let target = ChoreographyTrackId::Emitter(f.before.emitters[1].id);
    assert_eq!(
        order.iter().position(|x| *x == source).unwrap() + 1,
        order.iter().position(|x| *x == target).unwrap()
    );
    assert_eq!(f.session().effect.emitters, f.before.emitters);
    assert_eq!(f.session().effect.effect_clips, f.before.effect_clips);
    f.roundtrip();
}
#[test]
fn adjacent_noop_and_self_drop_do_not_create_history() {
    for own in [false, true] {
        let mut f = Fixture::new(false, true);
        if own {
            f.app
                .world_mut()
                .get_mut::<TimelineTrackHeader>(f.target)
                .unwrap()
                .track = ChoreographyTrackId::Emitter(f.before.emitters[0].id);
        }
        f.drop(f.source, PointerButton::Primary);
        f.clean();
    }
}
#[test]
fn stale_source_or_target_cannot_reorder() {
    for source in [false, true] {
        let mut f = Fixture::new(false, false);
        if source {
            f.app
                .world_mut()
                .get_mut::<EmitterTrackReorderHandle>(f.source)
                .unwrap()
                .emitter = EmitterId::new();
        } else {
            f.app
                .world_mut()
                .get_mut::<TimelineTrackHeader>(f.target)
                .unwrap()
                .track = ChoreographyTrackId::Emitter(EmitterId::new());
        }
        f.drop(f.source, PointerButton::Primary);
        f.clean();
    }
}
#[test]
fn missing_target_ancestry_or_unrecognized_source_does_not_edit() {
    let mut f = Fixture::new(false, false);
    let unrelated = f.app.world_mut().spawn_empty().id();
    f.drop(unrelated, PointerButton::Primary);
    f.app
        .world_mut()
        .entity_mut(f.target)
        .remove::<TimelineTrackHeader>();
    f.drop(f.source, PointerButton::Primary);
    f.clean();
}
#[test]
fn existing_secondary_reorder_policy_is_preserved() {
    let mut f = Fixture::new(false, false);
    f.begin(PointerButton::Secondary);
    assert!(f.dragged().is_some());
    f.drop(f.source, PointerButton::Secondary);
    f.roundtrip();
}
#[test]
fn descendant_target_resolves_nearest_track_header() {
    let mut f = Fixture::new(false, false);
    let target = f.app.world_mut().spawn(ChildOf(f.target)).id();
    f.action(pointer::drop_on(
        target,
        f.source,
        f.window,
        PointerButton::Primary,
    ));
    f.roundtrip();
}
#[test]
fn index_policy_preserves_boundaries_and_noop_moves() {
    assert_eq!(track_reorder_index(0, 1, true, 3), None);
    assert_eq!(track_reorder_index(0, 1, false, 3), Some(1));
    assert_eq!(track_reorder_index(2, 0, true, 3), Some(0));
    assert_eq!(track_reorder_index(3, 0, true, 3), None);
    assert_eq!(track_reorder_index(0, 0, true, 1), None);
}
