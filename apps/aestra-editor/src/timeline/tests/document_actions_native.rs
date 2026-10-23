//! Native timeline state cleanup surrounding the shared session edit branch.
use super::*;

fn fixture() -> App {
    let session = crate::test_support::session_with_timing_slack();
    let mut state = TimelineState::default();
    state.select_only_emitter(session.effect.emitters[0].id);
    let path = EffectClipPath::root_path(EffectClipId::new());
    state.inspected_child = Some(EffectClipChildSelection::EffectClip { path });
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(state)
        .add_observer(execute_timeline_action);
    app
}

#[test]
fn adding_marker_or_event_clears_emitter_selection_without_clearing_nested_inspection() {
    for action in [
        TimelineAction::AddMarker,
        TimelineAction::AddChoreographyEvent,
    ] {
        let mut app = fixture();
        app.world_mut().trigger(action);
        app.world_mut().flush();
        let state = app.world().resource::<TimelineState>();
        assert!(state.selected_emitters.is_empty());
        assert!(state.inspected_child.is_some());
        assert!(app.world().resource::<EditorSession>().can_undo());
    }
}

#[test]
fn selecting_marker_or_event_clears_emitter_selection_and_nested_inspection() {
    for marker in [true, false] {
        let mut app = fixture();
        let action = if marker {
            TimelineAction::SelectMarker(MarkerId::new())
        } else {
            TimelineAction::SelectChoreographyEvent(ChoreographyEventId::new())
        };
        app.world_mut().trigger(action);
        app.world_mut().flush();
        let state = app.world().resource::<TimelineState>();
        assert!(state.selected_emitters.is_empty());
        assert!(state.inspected_child.is_none());
        assert!(!app.world().resource::<EditorSession>().can_undo());
    }
}

#[test]
fn rejected_marker_or_event_deletion_does_not_clear_timeline_selection() {
    for action in [
        TimelineAction::DeleteMarker(MarkerId::new()),
        TimelineAction::DeleteChoreographyEvent(ChoreographyEventId::new()),
    ] {
        let mut app = fixture();
        app.world_mut().trigger(action);
        app.world_mut().flush();
        let state = app.world().resource::<TimelineState>();
        assert!(!state.selected_emitters.is_empty());
        assert!(state.inspected_child.is_some());
        assert!(!app.world().resource::<EditorSession>().can_undo());
    }
}
