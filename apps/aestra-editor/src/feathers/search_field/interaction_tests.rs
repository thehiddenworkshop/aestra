use super::*;
use crate::feathers::text_input::tests::{Changes, app_with, focus, tap, value};
use bevy::input::keyboard::Key;

fn app() -> App {
    app_with(|app| {
        app.add_observer(clear_search_field)
            .add_systems(Update, sync_search_clear_visibility);
    })
}

fn field(app: &mut App, initial: &str) -> (Entity, Entity, Entity, Entity) {
    let mut container = Entity::PLACEHOLDER;
    let parent = app
        .world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|parent| {
            container = spawn_search_field(parent, initial, "Search effects", "Clear search", ());
        })
        .id();
    app.world_mut().flush();
    for _ in 0..3 {
        app.update();
    }
    let input = app.world().get::<Children>(container).unwrap()[0];
    let clear = {
        let world = app.world_mut();
        world
            .query::<(Entity, &SearchFieldClear)>()
            .iter(world)
            .find(|(_, clear)| clear.input == container)
            .unwrap()
            .0
    };
    app.world_mut().resource_mut::<Changes>().0.clear();
    (parent, container, input, clear)
}

#[test]
fn clear_emits_exactly_one_live_container_change_through_the_real_text_bridge() {
    let mut app = app();
    let (_, container, input, clear) = field(&mut app, "spark");
    app.world_mut().trigger(Activate { entity: clear });
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(value(&app, input), "");
    assert_eq!(
        app.world().resource::<Changes>().0,
        [(container, String::new(), false)]
    );
    assert_eq!(
        app.world().get::<Node>(clear).unwrap().display,
        Display::None
    );
}

#[test]
fn empty_clear_and_disabled_ancestors_cannot_emit_or_mutate_search() {
    let mut app = app();
    let (_, _, empty, empty_clear) = field(&mut app, "");
    app.world_mut().trigger(Activate {
        entity: empty_clear,
    });
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(value(&app, empty), "");
    assert!(app.world().resource::<Changes>().0.is_empty());
    let (parent, _, input, clear) = field(&mut app, "spark");
    app.world_mut()
        .entity_mut(parent)
        .insert(bevy::ui::InteractionDisabled);
    app.world_mut().trigger(Activate { entity: clear });
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(value(&app, input), "spark");
    assert!(app.world().resource::<Changes>().0.is_empty());
}

#[test]
fn search_typing_after_clear_preserves_identity_and_reveals_the_clear_button() {
    let mut app = app();
    let (_, container, input, clear) = field(&mut app, "spark");
    app.world_mut().trigger(Activate { entity: clear });
    for _ in 0..3 {
        app.update();
    }
    focus(&mut app, input);
    tap(
        &mut app,
        KeyCode::KeyA,
        Key::Character("火".into()),
        Some("火"),
    );
    assert_eq!(value(&app, input), "火");
    assert_eq!(
        app.world().get::<Node>(clear).unwrap().display,
        Display::Flex
    );
    assert!(
        app.world()
            .resource::<Changes>()
            .0
            .iter()
            .all(|change| change.0 == container)
    );
}
