use super::*;
use crate::history::{HistoryAction, queue_history_action_activation};
use bevy::{ecs::system::RunSystemOnce, ui::InteractionDisabled};

#[derive(Resource, Default)]
struct Received(Vec<HistoryAction>);

fn app() -> App {
    let mut app = App::new();
    app.init_resource::<Received>()
        .add_observer(queue_history_action_activation)
        .add_observer(
            |action: On<HistoryAction>, mut received: ResMut<Received>| {
                received.0.push(*action);
            },
        );
    app
}

fn dispatch(app: &mut App, entity: Entity) -> bool {
    let world = app.world();
    let action = *world.get::<HistoryAction>(entity).unwrap();
    let feathers = world.get::<FeathersActionButton>(entity).is_some();
    let pending = world.get::<PendingFeathersActivation>(entity).is_some();
    let disabled = world.get::<InteractionDisabled>(entity).is_some();
    app.world_mut()
        .run_system_once(move |mut commands: Commands| {
            dispatch_action(&mut commands, entity, action, feathers, pending, disabled)
        })
        .unwrap()
}

#[test]
fn activate_queues_only_classified_actions_in_its_domain() {
    let mut app = app();
    let valid = app
        .world_mut()
        .spawn((HistoryAction::Undo, FeathersActionButton))
        .id();
    let unclassified = app.world_mut().spawn(HistoryAction::Undo).id();
    let other_domain = app
        .world_mut()
        .spawn((EditorNativeControl, FeathersActionButton))
        .id();
    for entity in [valid, unclassified, other_domain] {
        app.world_mut().trigger(Activate { entity });
    }
    app.update();
    assert!(
        app.world()
            .get::<PendingFeathersActivation>(valid)
            .is_some()
    );
    assert!(button_activation::is_pressed(app.world(), valid));
    for entity in [unclassified, other_domain] {
        assert!(
            app.world()
                .get::<PendingFeathersActivation>(entity)
                .is_none()
        );
        assert!(!button_activation::is_pressed(app.world(), entity));
    }
    assert!(app.world().resource::<Received>().0.is_empty());
}

#[test]
fn history_event_dispatch_consumes_pending_and_pressed_once() {
    let mut app = app();
    let entity = app
        .world_mut()
        .spawn((HistoryAction::Undo, FeathersActionButton))
        .id();
    app.world_mut().trigger(Activate { entity });
    app.update();
    assert!(dispatch(&mut app, entity));
    assert!(
        app.world()
            .get::<PendingFeathersActivation>(entity)
            .is_none()
    );
    assert!(!button_activation::is_pressed(app.world(), entity));
    assert!(!dispatch(&mut app, entity));
    assert_eq!(app.world().resource::<Received>().0, [HistoryAction::Undo]);
}

#[test]
fn pressed_without_activate_cannot_dispatch_a_feathers_action() {
    let mut app = app();
    let entity = app
        .world_mut()
        .spawn((
            HistoryAction::Redo,
            FeathersActionButton,
            button_activation::pressed_state(),
        ))
        .id();
    assert!(!dispatch(&mut app, entity));
    assert!(app.world().resource::<Received>().0.is_empty());
}

#[test]
fn disabled_action_does_not_dispatch_or_consume_its_pending_activation() {
    let mut app = app();
    let entity = app
        .world_mut()
        .spawn((
            HistoryAction::Undo,
            FeathersActionButton,
            InteractionDisabled,
        ))
        .id();
    app.world_mut().trigger(Activate { entity });
    app.update();
    assert!(!dispatch(&mut app, entity));
    assert!(app.world().resource::<Received>().0.is_empty());
    assert!(
        app.world()
            .get::<PendingFeathersActivation>(entity)
            .is_some()
    );
}

#[test]
fn independent_buttons_dispatch_their_actual_undo_redo_payloads() {
    let mut app = app();
    let undo = app
        .world_mut()
        .spawn((HistoryAction::Undo, FeathersActionButton))
        .id();
    let redo = app
        .world_mut()
        .spawn((HistoryAction::Redo, FeathersActionButton))
        .id();
    for entity in [undo, redo, undo] {
        app.world_mut().trigger(Activate { entity });
        app.update();
        assert!(dispatch(&mut app, entity));
    }
    assert_eq!(
        app.world().resource::<Received>().0,
        [
            HistoryAction::Undo,
            HistoryAction::Redo,
            HistoryAction::Undo
        ]
    );
}

#[test]
fn legacy_control_dispatches_without_feathers_pending_but_respects_disabled() {
    let mut app = app();
    let entity = app
        .world_mut()
        .spawn((HistoryAction::Redo, EditorNativeControl))
        .id();
    assert!(dispatch(&mut app, entity));
    app.world_mut()
        .entity_mut(entity)
        .insert(InteractionDisabled);
    assert!(!dispatch(&mut app, entity));
    assert_eq!(app.world().resource::<Received>().0, [HistoryAction::Redo]);
}
