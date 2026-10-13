use super::*;
use bevy::{ecs::system::RunSystemOnce, input_focus::InputFocus};
use std::path::PathBuf;

fn focus(app: &mut App, target: Entity) {
    app.insert_resource(InputFocus::from_entity(target));
    app.world_mut()
        .run_system_once(capture_history_keyboard_focus)
        .unwrap();
}

#[test]
fn nearest_neutral_scope_preserves_context_instead_of_inheriting_effect_scope() {
    let mut app = App::new();
    let mut session = crate::test_support::session_with_timing_slack();
    session.material_history_active = true;
    let original = session.effect.clone();
    let revision = session.ui_revision;
    app.insert_resource(session);
    let effect = app.world_mut().spawn(HistoryScope::Effect).id();
    let menu = app
        .world_mut()
        .spawn((HistoryScope::Neutral, ChildOf(effect)))
        .id();
    let item = app.world_mut().spawn(ChildOf(menu)).id();
    focus(&mut app, item);
    assert!(
        app.world()
            .resource::<EditorSession>()
            .material_history_active
    );
    focus(&mut app, effect);
    assert!(
        !app.world()
            .resource::<EditorSession>()
            .material_history_active
    );
    assert_eq!(app.world().resource::<EditorSession>().effect, original);
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision
    );
}

#[test]
fn material_scope_requires_a_standalone_program_or_function_target() {
    use crate::material_document::MaterialEditingTarget;
    let mut app = App::new();
    app.insert_resource(crate::test_support::session_with_timing_slack());
    for (target, expected) in [
        (MaterialEditingTarget::EffectInstance, false),
        (
            MaterialEditingTarget::Program {
                root: "project".into(),
                id: aestra_core::MaterialProgramId::new(),
            },
            true,
        ),
        (
            MaterialEditingTarget::Function {
                root: "project".into(),
                id: aestra_core::MaterialFunctionId::new(),
            },
            true,
        ),
    ] {
        app.world_mut()
            .resource_mut::<EditorSession>()
            .material_target = target;
        let panel = app.world_mut().spawn(HistoryScope::Material).id();
        let child = app.world_mut().spawn(ChildOf(panel)).id();
        focus(&mut app, child);
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .material_history_active,
            expected
        );
    }
}

#[test]
fn missing_or_cleared_keyboard_focus_does_not_change_the_editing_scope() {
    let mut app = App::new();
    let mut session = crate::test_support::session_with_timing_slack();
    session.material_history_active = true;
    app.insert_resource(session);
    app.world_mut()
        .run_system_once(capture_history_keyboard_focus)
        .unwrap();
    app.insert_resource(InputFocus::default());
    app.world_mut()
        .run_system_once(capture_history_keyboard_focus)
        .unwrap();
    let removed = app.world_mut().spawn(HistoryScope::Effect).id();
    app.world_mut().entity_mut(removed).despawn();
    focus(&mut app, removed);
    assert!(
        app.world()
            .resource::<EditorSession>()
            .material_history_active
    );
}

#[test]
fn pointer_focus_walks_real_ancestors_without_changing_document_or_ui_revision() {
    let mut app = App::new();
    let mut session = crate::test_support::session_with_timing_slack();
    session.material_target = crate::material_document::MaterialEditingTarget::Program {
        root: "project".into(),
        id: aestra_core::MaterialProgramId::new(),
    };
    let revision = session.ui_revision;
    app.insert_resource(session)
        .add_observer(capture_history_focus);
    for (scope, expected) in [
        (HistoryScope::Material, true),
        (HistoryScope::Neutral, true),
        (HistoryScope::Effect, false),
    ] {
        let parent = app.world_mut().spawn(scope).id();
        let child = app.world_mut().spawn(ChildOf(parent)).id();
        app.world_mut()
            .trigger(super::super::focus_pointer::press(child));
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .material_history_active,
            expected
        );
    }
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision
    );
}

#[test]
fn keyboard_focus_routes_history_and_neutral_panels_keep_the_last_edit_scope() {
    use bevy::{ecs::system::RunSystemOnce, input_focus::InputFocus};
    let mut app = App::new();
    let mut session = crate::test_support::session_with_timing_slack();
    session.material_target = crate::material_document::MaterialEditingTarget::Program {
        root: PathBuf::from("project"),
        id: aestra_core::MaterialProgramId::new(),
    };
    let revision = session.ui_revision;
    app.insert_resource(session);
    for (scope, expected) in [
        (HistoryScope::Material, true),
        (HistoryScope::Neutral, true),
        (HistoryScope::Effect, false),
        (HistoryScope::Neutral, false),
    ] {
        let parent = app.world_mut().spawn(scope).id();
        let child = app.world_mut().spawn(ChildOf(parent)).id();
        app.insert_resource(InputFocus::from_entity(child));
        app.world_mut()
            .run_system_once(capture_history_keyboard_focus)
            .unwrap();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .material_history_active,
            expected
        );
    }
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision,
        "focus-only navigation must not rebuild graph panels"
    );
}
