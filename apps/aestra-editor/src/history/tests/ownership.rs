use super::*;
use crate::{
    localization::Localizer, project_content::EditorProjectContent, session::EditorSession,
};

#[derive(Resource, Default)]
struct Observed {
    routes: Vec<Route>,
    blocked: bool,
}

fn observe(action: On<HistoryAction>, mut ownership: Ownership, mut observed: ResMut<Observed>) {
    observed.routes.push(ownership.route(*action));
}

fn observe_gate(gate: Gate, mut observed: ResMut<Observed>) {
    observed.blocked = gate.blocked();
}

fn fixture() -> App {
    let mut app = App::new();
    let mut session = crate::test_support::session_with_timing_slack();
    session.adjust_effect_duration(0.25);
    app.insert_resource(session)
        .init_resource::<Observed>()
        .add_observer(observe)
        .add_systems(Update, observe_gate);
    app
}

fn trigger(app: &mut App, action: HistoryAction) -> Route {
    app.world_mut().trigger(action);
    app.world_mut().flush();
    app.world_mut()
        .resource_mut::<Observed>()
        .routes
        .pop()
        .unwrap()
}

fn editor(app: &mut App) -> Entity {
    let mut editor = CodeEditor::new("original".into());
    editor.insert_for_test(" + edited");
    app.world_mut().spawn(editor).id()
}

#[test]
fn absent_optional_resources_leave_document_history_authoritative() {
    let mut app = fixture();
    app.update();
    assert!(!app.world().resource::<Observed>().blocked);
    assert_eq!(trigger(&mut app, HistoryAction::Undo), Route::Document);
    assert!(app.world().resource::<EditorSession>().can_undo());
}

#[test]
fn active_code_undo_redo_uses_real_buffer_without_touching_effect_history() {
    let mut app = fixture();
    let entity = editor(&mut app);
    let original_effect = app.world().resource::<EditorSession>().effect.clone();
    let original_revision = app.world().resource::<EditorSession>().document_revision();
    let original_serial = app.world().resource::<EditorSession>().effect_edit_serial();
    app.insert_resource(ActiveCodeEditor(Some(entity)));
    assert_eq!(
        trigger(&mut app, HistoryAction::Undo),
        Route::Code("Undid edit")
    );
    assert_eq!(
        app.world().get::<CodeEditor>(entity).unwrap().text,
        "original"
    );
    assert_eq!(
        trigger(&mut app, HistoryAction::Redo),
        Route::Code("Redid edit")
    );
    assert_eq!(
        app.world().get::<CodeEditor>(entity).unwrap().text,
        "original + edited"
    );
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.effect, original_effect);
    assert_eq!(session.document_revision(), original_revision);
    assert_eq!(session.effect_edit_serial(), original_serial);
    assert!(session.can_undo());
    assert!(!session.can_redo());
}

#[test]
fn empty_active_buffer_history_never_falls_through_to_effect_undo() {
    let mut app = fixture();
    let entity = app
        .world_mut()
        .spawn(CodeEditor::new("unchanged".into()))
        .id();
    app.insert_resource(ActiveCodeEditor(Some(entity)));
    assert_eq!(
        trigger(&mut app, HistoryAction::Undo),
        Route::Code("Nothing to undo")
    );
    assert_eq!(
        trigger(&mut app, HistoryAction::Redo),
        Route::Code("Nothing to redo")
    );
    assert!(app.world().resource::<EditorSession>().can_undo());
}

#[test]
fn missing_removed_and_recycled_active_entities_cannot_steal_history() {
    let mut app = fixture();
    let entity = editor(&mut app);
    app.insert_resource(ActiveCodeEditor(Some(entity)));
    app.world_mut().entity_mut(entity).remove::<CodeEditor>();
    assert_eq!(trigger(&mut app, HistoryAction::Undo), Route::Document);
    app.world_mut().entity_mut(entity).despawn();
    let replacement = editor(&mut app);
    assert_ne!(replacement, entity);
    assert_eq!(trigger(&mut app, HistoryAction::Undo), Route::Document);
    assert_eq!(
        app.world().get::<CodeEditor>(replacement).unwrap().text,
        "original + edited"
    );
    app.insert_resource(ActiveCodeEditor(None));
    assert_eq!(trigger(&mut app, HistoryAction::Undo), Route::Document);
}

#[test]
fn every_modal_branch_blocks_code_and_document_routing_and_availability() {
    let mut app = fixture();
    let entity = editor(&mut app);
    for protection in DocumentProtectionState::blocking_states_for_test() {
        assert!(protection.is_open());
        app.insert_resource(protection);
        for active in [None, Some(entity)] {
            app.insert_resource(ActiveCodeEditor(active));
            assert_eq!(trigger(&mut app, HistoryAction::Undo), Route::Blocked);
            assert_eq!(trigger(&mut app, HistoryAction::Redo), Route::Blocked);
            app.update();
            assert!(app.world().resource::<Observed>().blocked);
        }
        assert_eq!(
            app.world().get::<CodeEditor>(entity).unwrap().text,
            "original + edited"
        );
        assert!(app.world().resource::<EditorSession>().can_undo());
    }
    app.insert_resource(DocumentProtectionState::default());
    app.update();
    assert!(!app.world().resource::<Observed>().blocked);
    assert_eq!(
        trigger(&mut app, HistoryAction::Undo),
        Route::Code("Undid edit")
    );
}

#[test]
fn completed_unapplied_io_blocks_both_routes_until_real_completion_is_applied() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = fixture();
    let entity = editor(&mut app);
    app.insert_resource(EditorProjectContent::scan(directory.path()));
    app.insert_resource(Localizer::new("en-US").unwrap());
    let guard = io::IoGuard::capture(
        app.world().resource::<EditorProjectContent>(),
        app.world().resource::<EditorSession>(),
    );
    io::enqueue(&mut app.world_mut().commands(), guard, || {
        io::completion(|_| {})
    });
    let mut completion = io::prepared_completion(app.world_mut());
    for active in [None, Some(entity)] {
        app.insert_resource(ActiveCodeEditor(active));
        assert_eq!(trigger(&mut app, HistoryAction::Undo), Route::Blocked);
        app.update();
        assert!(app.world().resource::<Observed>().blocked);
    }
    completion.apply(app.world_mut());
    app.update();
    assert!(!app.world().resource::<Observed>().blocked);
    assert_eq!(
        trigger(&mut app, HistoryAction::Undo),
        Route::Code("Undid edit")
    );
    app.world_mut().remove_resource::<ActiveCodeEditor>();
    assert_eq!(trigger(&mut app, HistoryAction::Undo), Route::Document);
}

#[test]
fn switching_active_code_buffers_keeps_independent_histories() {
    let mut app = fixture();
    let first = editor(&mut app);
    let second = editor(&mut app);
    app.insert_resource(ActiveCodeEditor(Some(first)));
    assert_eq!(
        trigger(&mut app, HistoryAction::Undo),
        Route::Code("Undid edit")
    );
    app.insert_resource(ActiveCodeEditor(Some(second)));
    assert_eq!(
        trigger(&mut app, HistoryAction::Redo),
        Route::Code("Nothing to redo")
    );
    assert_eq!(
        app.world().get::<CodeEditor>(second).unwrap().text,
        "original + edited"
    );
    app.insert_resource(ActiveCodeEditor(Some(first)));
    assert_eq!(
        trigger(&mut app, HistoryAction::Redo),
        Route::Code("Redid edit")
    );
}
