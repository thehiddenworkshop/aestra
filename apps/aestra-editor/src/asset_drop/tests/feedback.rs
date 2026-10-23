use super::*;
use bevy::ecs::system::RunSystemOnce;

fn fixture() -> (tempfile::TempDir, ProjectEffectCatalog, AssetPayload) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("source.png"), b"test source").unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let source = catalog
        .content()
        .source_tree()
        .at_relative_path("source.png")
        .unwrap()
        .id;
    let payload = AssetPayload::capture(&catalog, source);
    (root, catalog, payload)
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
enum Target {
    Renderer,
    Texture,
}

#[test]
fn child_routing_chooses_nearest_target_and_only_accepts_primary_drags() {
    let (_root, _catalog, payload) = fixture();
    let mut world = World::new();
    let source = world.spawn(payload.clone()).id();
    let source_label = world.spawn(ChildOf(source)).id();
    let card = world.spawn(Target::Renderer).id();
    let input = world.spawn((Target::Texture, ChildOf(card))).id();
    let label = world.spawn(ChildOf(input)).id();
    world
        .run_system_once(
            move |sources: Query<&AssetPayload>,
                  parents: Query<&ChildOf>,
                  targets: Query<&Target>| {
                let resolved = resolve(
                    PointerButton::Primary,
                    source_label,
                    label,
                    &sources,
                    &parents,
                    |entity| targets.get(entity).ok().copied(),
                )
                .unwrap();
                assert_eq!(resolved, (payload.clone(), input, Target::Texture));
                // Type compatibility is checked after routing: rejection must not assign to the card.
                assert_ne!(resolved.1, card);
                for button in [PointerButton::Secondary, PointerButton::Middle] {
                    assert!(
                        resolve(button, source_label, label, &sources, &parents, |entity| {
                            targets.get(entity).ok().copied()
                        })
                        .is_none()
                    );
                }
                assert!(
                    resolve(
                        PointerButton::Primary,
                        card,
                        label,
                        &sources,
                        &parents,
                        |entity| targets.get(entity).ok().copied()
                    )
                    .is_none()
                );
            },
        )
        .unwrap();
}

#[test]
fn feedback_is_owner_scoped_and_registration_is_idempotent() {
    let (_root, catalog, payload) = fixture();
    let mut app = App::new();
    register_feedback::<Target>(&mut app);
    register_feedback::<Target>(&mut app);
    register_feedback::<()>(&mut app);
    app.insert_resource(catalog);
    app.world_mut().spawn(payload.clone());
    let target = app.world_mut().spawn_empty().id();
    app.world_mut()
        .run_system_once(move |mut commands: Commands| {
            show_feedback::<Target>(
                &mut commands,
                target,
                payload.clone(),
                "Assign".into(),
                true,
            );
            show_feedback::<()>(
                &mut commands,
                target,
                payload.clone(),
                "Other consumer".into(),
                false,
            );
        })
        .unwrap();
    app.update();
    app.world_mut()
        .run_system_once(
            |feedback: Query<Entity, With<Feedback<Target>>>, mut commands: Commands| {
                clear_feedback(&feedback, &mut commands);
            },
        )
        .unwrap();
    assert_eq!(
        app.world_mut()
            .query::<&Feedback<Target>>()
            .iter(app.world())
            .count(),
        0
    );
    assert_eq!(
        app.world_mut()
            .query::<&Feedback<()>>()
            .iter(app.world())
            .count(),
        1
    );
}

#[test]
fn feedback_cleans_up_on_cancel_stale_source_drag_end_and_missing_target() {
    for case in 0..5 {
        let (_root, catalog, payload) = fixture();
        let mut app = App::new();
        register_feedback::<Target>(&mut app);
        app.insert_resource(catalog)
            .init_resource::<ButtonInput<KeyCode>>();
        let source = app.world_mut().spawn(payload.clone()).id();
        let target = app.world_mut().spawn_empty().id();
        app.world_mut()
            .run_system_once(move |mut commands: Commands| {
                show_feedback::<Target>(
                    &mut commands,
                    target,
                    payload.clone(),
                    "Assign".into(),
                    true,
                );
            })
            .unwrap();
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&Feedback<Target>>()
                .iter(app.world())
                .count(),
            1
        );
        match case {
            0 => app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape),
            1 => {
                app.world_mut().entity_mut(source).remove::<AssetPayload>();
            }
            2 => {
                app.world_mut().despawn(target);
            }
            3 => {
                app.world_mut().remove_resource::<ProjectEffectCatalog>();
            }
            _ => {
                app.world_mut()
                    .resource_mut::<ProjectEffectCatalog>()
                    .refresh();
            }
        }
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&Feedback<Target>>()
                .iter(app.world())
                .count(),
            0,
            "case {case}"
        );
    }
}

#[test]
fn release_guard_rechecks_escape_without_requiring_keyboard_resources() {
    let mut world = World::new();
    assert!(
        world
            .run_system_once(|guard: AuthoringDropGuard| guard.check_release())
            .unwrap()
            .is_ok()
    );
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::Escape);
    world.insert_resource(keys);
    assert_eq!(
        world
            .run_system_once(|guard: AuthoringDropGuard| guard.check_release())
            .unwrap(),
        Err("Asset drop cancelled".into())
    );
}
