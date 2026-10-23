//! Real session-backed document edits with production history routing on both engines.
use super::*;
use crate::{
    ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
};
use aestra_authoring::SemanticTarget;
use aestra_core::EffectAsset;

#[derive(Resource, Default)]
struct Dispatch(Vec<(TimelineAction, bool)>);

struct Fixture {
    _root: tempfile::TempDir,
    app: App,
    original: EffectAsset,
}
impl Fixture {
    fn new(locale: Option<&str>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let session = crate::test_support::session_with_timing_slack();
        let original = session.effect.clone();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .init_resource::<Dispatch>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<FunctionEditor>()
            .add_observer(
                |action: On<TimelineAction>,
                 mut session: ResMut<EditorSession>,
                 localizer: Option<Res<Localizer>>,
                 mut dispatch: ResMut<Dispatch>| {
                    let handled = apply(*action, &mut session, localizer.as_deref());
                    dispatch.0.push((*action, handled));
                },
            )
            .add_observer(
                |action: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*action, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history in this fixture")
                    });
                },
            );
        if let Some(locale) = locale {
            app.insert_resource(Localizer::new(locale).unwrap());
        }
        Self {
            _root: root,
            app,
            original,
        }
    }
    fn action(&mut self, action: TimelineAction) {
        self.app.world_mut().trigger(action);
        self.app.world_mut().flush();
    }
    fn step(&mut self, action: HistoryAction) {
        self.app.world_mut().trigger(action);
        self.app.world_mut().flush();
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource::<EditorSession>()
    }
    fn seek(&mut self, time: f32) {
        self.app
            .world_mut()
            .resource_mut::<EditorSession>()
            .seek_time(time);
    }
    fn assert_no_edit(&self) {
        assert_eq!(self.session().effect, self.original);
        assert!(!self.session().can_undo());
        assert!(!self.session().dirty);
    }
}

#[test]
fn add_marker_uses_playhead_and_fallback_label_then_selects_and_dirties_once() {
    let mut f = Fixture::new(None);
    f.seek(0.75);
    let index = f.original.markers.len();
    f.action(TimelineAction::AddMarker);
    let marker = &f.session().effect.markers[index];
    assert_eq!(marker.time, 0.75);
    assert_eq!(marker.name, format!("Marker {}", index + 1));
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::Marker(marker.id)
    );
    assert!(f.session().dirty);
    assert!(f.session().can_undo());
    assert_eq!(
        f.app.world().resource::<Dispatch>().0,
        [(TimelineAction::AddMarker, true)]
    );
    f.step(HistoryAction::Undo);
    f.assert_no_edit();
}

#[test]
fn marker_labels_use_the_actual_locale_in_both_languages() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::new(Some(locale));
        let prefix = f
            .app
            .world()
            .resource::<Localizer>()
            .text("timeline-marker");
        f.action(TimelineAction::AddMarker);
        assert_eq!(
            f.session().effect.markers.last().unwrap().name,
            format!("{prefix} {}", f.original.markers.len() + 1)
        );
    }
}

#[test]
fn multiple_marker_actions_append_fresh_identities_in_dispatch_order() {
    let mut f = Fixture::new(None);
    for time in [0.25, 0.75, 1.25] {
        f.seek(time);
        f.action(TimelineAction::AddMarker);
    }
    let added = &f.session().effect.markers[f.original.markers.len()..];
    assert_eq!(
        added.iter().map(|marker| marker.time).collect::<Vec<_>>(),
        [0.25, 0.75, 1.25]
    );
    assert_eq!(
        added
            .iter()
            .map(|marker| marker.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    let final_effect = f.session().effect.clone();
    for _ in 0..3 {
        f.step(HistoryAction::Undo);
    }
    f.assert_no_edit();
    for _ in 0..3 {
        f.step(HistoryAction::Redo);
    }
    assert_eq!(f.session().effect, final_effect);
}

#[test]
fn marker_selection_is_not_a_semantic_edit_and_stale_ids_do_not_change_selection() {
    let mut f = Fixture::new(None);
    f.action(TimelineAction::AddMarker);
    let id = f.session().effect.markers.last().unwrap().id;
    let serial = f.session().operation_order.edit_serial();
    let effect = f.session().effect.clone();
    f.action(TimelineAction::SelectMarker(id));
    assert_eq!(f.session().selection.primary, SemanticTarget::Marker(id));
    f.action(TimelineAction::SelectMarker(MarkerId::new()));
    assert_eq!(f.session().selection.primary, SemanticTarget::Marker(id));
    assert_eq!(f.session().effect, effect);
    assert_eq!(f.session().operation_order.edit_serial(), serial);
}

#[test]
fn marker_deletion_is_one_reversible_transaction() {
    let mut f = Fixture::new(Some("fr-FR"));
    f.action(TimelineAction::AddMarker);
    let effect = f.session().effect.clone();
    let id = effect.markers.last().unwrap().id;
    f.action(TimelineAction::DeleteMarker(id));
    assert_eq!(f.session().effect, f.original);
    f.step(HistoryAction::Undo);
    assert_eq!(f.session().effect, effect);
    f.step(HistoryAction::Redo);
    assert_eq!(f.session().effect, f.original);
}

#[test]
fn stale_marker_deletion_rejects_without_a_history_entry_or_document_mutation() {
    let mut f = Fixture::new(None);
    f.action(TimelineAction::DeleteMarker(MarkerId::new()));
    f.assert_no_edit();
}

#[test]
fn add_event_uses_playhead_and_existing_gameplay_notify_payload() {
    let mut f = Fixture::new(None);
    f.seek(0.5);
    f.action(TimelineAction::AddChoreographyEvent);
    let event = f.session().effect.choreography_events.last().unwrap();
    assert_eq!(event.time, 0.5);
    assert_eq!(
        event.name,
        format!("Event {}", f.original.choreography_events.len() + 1)
    );
    assert_eq!(
        event.payload,
        ChoreographyEventPayload::GameplayNotify {
            topic: String::new()
        }
    );
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::ChoreographyEvent(event.id)
    );
    assert!(f.session().dirty);
    assert!(f.session().can_undo());
    f.step(HistoryAction::Undo);
    f.assert_no_edit();
}

#[test]
fn event_selection_changes_only_the_selection_not_document_history() {
    let mut f = Fixture::new(None);
    f.action(TimelineAction::AddChoreographyEvent);
    let id = f.session().effect.choreography_events.last().unwrap().id;
    let effect = f.session().effect.clone();
    let serial = f.session().operation_order.edit_serial();
    f.action(TimelineAction::SelectChoreographyEvent(id));
    f.action(TimelineAction::SelectChoreographyEvent(
        ChoreographyEventId::new(),
    ));
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::ChoreographyEvent(id)
    );
    assert_eq!(f.session().effect, effect);
    assert_eq!(f.session().operation_order.edit_serial(), serial);
}

#[test]
fn event_delete_undo_redo_preserve_exact_event_identity_and_payload() {
    let mut f = Fixture::new(Some("en-US"));
    f.action(TimelineAction::AddChoreographyEvent);
    let effect = f.session().effect.clone();
    let id = effect.choreography_events.last().unwrap().id;
    f.action(TimelineAction::DeleteChoreographyEvent(id));
    assert_eq!(f.session().effect, f.original);
    f.step(HistoryAction::Undo);
    assert_eq!(f.session().effect, effect);
    f.step(HistoryAction::Redo);
    assert_eq!(f.session().effect, f.original);
}

#[test]
fn stale_event_deletion_rejects_without_a_history_entry_or_document_mutation() {
    let mut f = Fixture::new(Some("fr-FR"));
    f.action(TimelineAction::DeleteChoreographyEvent(
        ChoreographyEventId::new(),
    ));
    f.assert_no_edit();
}

#[test]
fn mixed_marker_and_event_actions_undo_and_redo_in_actual_global_order() {
    let mut f = Fixture::new(None);
    f.action(TimelineAction::AddMarker);
    let marker = f.session().effect.markers.last().unwrap().id;
    let marker_only = f.session().effect.clone();
    f.action(TimelineAction::AddChoreographyEvent);
    let both = f.session().effect.clone();
    f.action(TimelineAction::DeleteMarker(marker));
    let event_only = f.session().effect.clone();
    f.step(HistoryAction::Undo);
    assert_eq!(f.session().effect, both);
    f.step(HistoryAction::Undo);
    assert_eq!(f.session().effect, marker_only);
    f.step(HistoryAction::Undo);
    f.assert_no_edit();
    for _ in 0..3 {
        f.step(HistoryAction::Redo);
    }
    assert_eq!(f.session().effect, event_only);
}

#[test]
fn new_document_action_forks_redo_using_the_existing_session_history() {
    let mut f = Fixture::new(None);
    f.action(TimelineAction::AddMarker);
    f.step(HistoryAction::Undo);
    assert!(f.session().can_redo());
    f.action(TimelineAction::AddChoreographyEvent);
    assert!(!f.session().can_redo());
    assert_eq!(f.session().effect.markers, f.original.markers);
}

#[test]
fn view_duration_and_region_actions_remain_owned_by_the_native_adapter() {
    let mut f = Fixture::new(None);
    for action in [
        TimelineAction::AdjustEffectDuration(1.0),
        TimelineAction::SetSnap(TimelineSnapMode::None),
        TimelineAction::FrameAll,
        TimelineAction::SplitEmitterRegion,
        TimelineAction::JoinEmitterRegion,
    ] {
        f.action(action);
    }
    f.assert_no_edit();
    assert!(
        f.app
            .world()
            .resource::<Dispatch>()
            .0
            .iter()
            .all(|(_, handled)| !handled)
    );
}

#[test]
fn snap_contract_preserves_all_modes_and_localization_message_ids() {
    assert_eq!(
        TimelineSnapMode::ALL,
        [
            TimelineSnapMode::None,
            TimelineSnapMode::Frames,
            TimelineSnapMode::Seconds,
            TimelineSnapMode::Smart
        ]
    );
    assert_eq!(TimelineSnapMode::default(), TimelineSnapMode::Smart);
    assert_eq!(
        TimelineSnapMode::ALL.map(TimelineSnapMode::message_id),
        [
            "timeline-snap-off",
            "timeline-snap-frames",
            "timeline-snap-time",
            "timeline-snap-smart"
        ]
    );
}
