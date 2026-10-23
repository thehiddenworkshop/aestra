//! Native widget values cross the real inspector semantic/undo boundary.
use super::*;
use crate::feathers::{
    number_input::{NumberScrubPlugin, arrows::NumberArrowsPlugin},
    text_input::tests::{app_with, focus, tap},
};
use bevy::{feathers::controls::FeathersNumberInput, input::keyboard::Key};

fn fixture(control: EmitterNumberControl) -> (App, Entity, Entity, Entity, f32) {
    let session = crate::test_support::session_with_timing_slack();
    let initial = emitter_number_input_value(&session, control);
    let mut app = app_with(|app| {
        app.insert_resource(session)
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<NumericScrubState>()
            .add_plugins((NumberScrubPlugin, NumberArrowsPlugin))
            .add_observer(handle_native_numeric_edit)
            .add_systems(
                Update,
                (
                    decorate_numeric_scrub_inputs,
                    configure_native_numeric_inputs,
                    cleanup_native_numeric_edit,
                )
                    .chain(),
            );
    });
    let owner = app
        .world_mut()
        .commands()
        .spawn(control)
        .apply_scene(ui_shell::feathers_scalar_input())
        .id();
    app.world_mut().flush();
    app.world_mut().trigger(UpdateNumberInput {
        entity: owner,
        value: NumberInputValue::F32(initial),
    });
    settle(&mut app);
    let native = app
        .world()
        .get::<Children>(owner)
        .unwrap()
        .iter()
        .find(|&e| app.world().get::<FeathersNumberInput>(e).is_some())
        .unwrap();
    let text = app
        .world()
        .get::<Children>(native)
        .unwrap()
        .iter()
        .find(|&e| app.world().get::<EditableText>(e).is_some())
        .unwrap();
    (app, owner, native, text, initial)
}

fn settle(app: &mut App) {
    for _ in 0..5 {
        app.update();
    }
}
fn change(app: &mut App, native: Entity, value: f32, is_final: bool) {
    app.world_mut().trigger(ValueChange {
        source: native,
        value,
        is_final,
    });
    settle(app);
}

#[test]
fn native_translation_rotation_scale_and_timing_preview_then_commit_one_undo() {
    for control in [
        EmitterNumberControl::Translation(0),
        EmitterNumberControl::Rotation(1),
        EmitterNumberControl::Scale(2),
        EmitterNumberControl::Duration,
    ] {
        let (mut app, _, native, _, initial) = fixture(control);
        let before = app.world().resource::<EditorSession>().effect.clone();
        let revision = app.world().resource::<EditorSession>().document_revision();
        change(&mut app, native, initial + 0.2, false);
        change(&mut app, native, initial + 0.4, false);
        assert_eq!(
            app.world().resource::<EditorSession>().effect,
            before,
            "draft must not mutate the document"
        );
        assert_eq!(
            app.world().resource::<EditorSession>().document_revision(),
            revision
        );
        change(&mut app, native, initial + 0.4, true);
        assert_eq!(
            app.world().resource::<EditorSession>().document_revision(),
            revision + 1
        );
        app.world_mut().resource_mut::<EditorSession>().undo();
        assert_eq!(
            app.world().resource::<EditorSession>().effect,
            before,
            "one undo must restore the whole native gesture"
        );
    }
}

#[test]
fn native_escape_restores_preview_without_dirtying_and_late_final_is_rejected() {
    let (mut app, _, native, text, initial) = fixture(EmitterNumberControl::Translation(0));
    let before = app.world().resource::<EditorSession>().effect.clone();
    let revision = app.world().resource::<EditorSession>().document_revision();
    focus(&mut app, text);
    change(&mut app, native, initial + 5.0, false);
    tap(&mut app, KeyCode::Escape, Key::Escape, None);
    assert!(app.world().resource::<NumericScrubState>().active.is_none());
    change(&mut app, native, initial + 5.0, true);
    assert_eq!(app.world().resource::<EditorSession>().effect, before);
    assert_eq!(
        app.world().resource::<EditorSession>().document_revision(),
        revision
    );
}

#[test]
fn rebuilding_inspector_during_native_preview_clears_transaction_without_commit() {
    let (mut app, owner, native, _, initial) = fixture(EmitterNumberControl::Translation(0));
    let revision = app.world().resource::<EditorSession>().document_revision();
    change(&mut app, native, initial + 5.0, false);
    assert!(app.world().resource::<NumericScrubState>().active.is_some());
    app.world_mut().despawn(owner);
    settle(&mut app);
    assert!(app.world().resource::<NumericScrubState>().active.is_none());
    assert_eq!(
        app.world().resource::<EditorSession>().document_revision(),
        revision
    );
}

#[test]
fn changing_document_revision_rejects_native_final_instead_of_editing_new_state() {
    let (mut app, _, native, text, initial) = fixture(EmitterNumberControl::Translation(0));
    focus(&mut app, text);
    change(&mut app, native, initial + 5.0, false);
    let mut transform = app
        .world()
        .resource::<EditorSession>()
        .selected_layer()
        .unwrap()
        .transform;
    transform.translation[0] += 2.0;
    app.world_mut()
        .resource_mut::<EditorSession>()
        .set_selected_emitter_transform(transform, true);
    let before = app.world().resource::<EditorSession>().effect.clone();
    let revision = app.world().resource::<EditorSession>().document_revision();
    change(&mut app, native, initial + 5.0, true);
    assert_eq!(app.world().resource::<EditorSession>().effect, before);
    assert_eq!(
        app.world().resource::<EditorSession>().document_revision(),
        revision
    );
}
