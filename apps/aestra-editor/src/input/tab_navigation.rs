//! Bevy tab order is structural; retained hidden/disabled controls must be excluded.
use bevy::{
    input_focus::tab_navigation::TabIndex, prelude::*, ui::InteractionDisabled,
    ui_widgets::MenuFocusState,
};

#[derive(Component)]
pub(super) struct SuppressedTabIndex(i32);

type EligibilityAncestors = (
    Option<&'static Node>,
    Option<&'static Visibility>,
    Has<InteractionDisabled>,
    Option<&'static ChildOf>,
);

fn eligible(entity: Entity, ancestors: &Query<EligibilityAncestors>) -> bool {
    let mut current = Some(entity);
    while let Some(target) = current {
        let Ok((node, visibility, disabled, parent)) = ancestors.get(target) else {
            break;
        };
        if disabled
            || node.is_some_and(|node| node.display == Display::None)
            || visibility.is_some_and(|visibility| *visibility == Visibility::Hidden)
        {
            return false;
        }
        current = parent.map(ChildOf::parent);
    }
    true
}

/// Opening a retained popup happens after the PreUpdate eligibility pass. Restore its children
/// synchronously with that transition, before Bevy's Update menu focus acquisition can reject
/// their suppressed indices and close the popup. Still respect disabled/hidden ancestors.
pub(super) fn restore_opening_menu_tab_indices(
    event: crate::input_api::MenuFocusInsert,
    menus: Query<&MenuFocusState, With<bevy::ui_widgets::MenuPopup>>,
    children: Query<&Children>,
    mut controls: Query<(&mut TabIndex, &SuppressedTabIndex)>,
    ancestors: Query<EligibilityAncestors>,
    mut commands: Commands,
) {
    if !menus
        .get(event.entity)
        .is_ok_and(|state| matches!(state, MenuFocusState::Opening(_) | MenuFocusState::Open))
    {
        return;
    }
    for entity in children.iter_descendants(event.entity) {
        if eligible(entity, &ancestors)
            && let Ok((mut index, original)) = controls.get_mut(entity)
        {
            if index.0 == -1 {
                index.0 = original.0;
            }
            commands.entity(entity).remove::<SuppressedTabIndex>();
        }
    }
}

pub(super) fn sync_tab_eligibility(
    mut commands: Commands,
    mut controls: Query<(Entity, &mut TabIndex, Option<&SuppressedTabIndex>)>,
    ancestors: Query<EligibilityAncestors>,
) {
    for (entity, mut index, suppressed) in &mut controls {
        if eligible(entity, &ancestors) {
            if let Some(original) = suppressed {
                if index.0 == -1 {
                    index.0 = original.0;
                }
                commands.entity(entity).remove::<SuppressedTabIndex>();
            }
        } else if index.0 >= 0 {
            commands.entity(entity).insert(SuppressedTabIndex(index.0));
            index.0 = -1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input_focus::{
        InputFocus,
        tab_navigation::{NavAction, TabGroup, TabNavigation},
    };

    #[test]
    fn opening_menu_restores_only_eligible_children_and_original_indices() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_systems(Update, sync_tab_eligibility)
            .add_observer(restore_opening_menu_tab_indices);
        let popup = app
            .world_mut()
            .spawn((bevy::ui_widgets::MenuPopup::default(), Visibility::Hidden))
            .id();
        let visible = app.world_mut().spawn((TabIndex(5), ChildOf(popup))).id();
        let disabled = app
            .world_mut()
            .spawn((TabIndex(2), InteractionDisabled, ChildOf(popup)))
            .id();
        let hidden = app
            .world_mut()
            .spawn((TabIndex(3), Visibility::Hidden, ChildOf(popup)))
            .id();
        let unrelated = app
            .world_mut()
            .spawn((TabIndex(4), Visibility::Hidden))
            .id();
        app.update();
        for entity in [visible, disabled, hidden, unrelated] {
            assert_eq!(app.world().get::<TabIndex>(entity).unwrap().0, -1);
        }
        app.world_mut().entity_mut(popup).insert((
            Visibility::Visible,
            MenuFocusState::Opening(NavAction::First),
        ));
        app.world_mut().flush();
        // Restoration happens at the transition, not at the next frame's sync.
        assert_eq!(app.world().get::<TabIndex>(visible).unwrap().0, 5);
        assert!(!app.world().entity(visible).contains::<SuppressedTabIndex>());
        for entity in [disabled, hidden, unrelated] {
            assert_eq!(app.world().get::<TabIndex>(entity).unwrap().0, -1);
            assert!(app.world().entity(entity).contains::<SuppressedTabIndex>());
        }
    }

    #[test]
    fn retained_hidden_and_disabled_controls_are_skipped_and_restore_their_order() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::scene::ScenePlugin,
        ));
        app.add_systems(Update, sync_tab_eligibility);
        let root = app
            .world_mut()
            .commands()
            .spawn_empty()
            .apply_scene(crate::feathers::scenes::editor_root())
            .id();
        app.world_mut().flush();
        assert!(
            app.world().get::<TabGroup>(root).is_some(),
            "the real editor root must establish tab navigation"
        );
        let panel = app
            .world_mut()
            .spawn((
                Node {
                    display: Display::None,
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        let hidden = app.world_mut().spawn((TabIndex(3), ChildOf(panel))).id();
        let disabled = app
            .world_mut()
            .spawn((TabIndex(2), InteractionDisabled, ChildOf(root)))
            .id();
        let visible = app.world_mut().spawn((TabIndex(1), ChildOf(root))).id();
        app.update();
        use bevy::ecs::system::RunSystemOnce;
        let next = app
            .world_mut()
            .run_system_once(|nav: TabNavigation| {
                nav.navigate(&InputFocus::default(), NavAction::Next)
                    .unwrap()
            })
            .unwrap();
        assert_eq!(next, visible);
        assert_eq!(app.world().get::<TabIndex>(hidden).unwrap().0, -1);
        assert_eq!(app.world().get::<TabIndex>(disabled).unwrap().0, -1);
        app.world_mut().get_mut::<Node>(panel).unwrap().display = Display::Flex;
        app.world_mut()
            .entity_mut(disabled)
            .remove::<InteractionDisabled>();
        app.update();
        assert_eq!(app.world().get::<TabIndex>(hidden).unwrap().0, 3);
        assert_eq!(app.world().get::<TabIndex>(disabled).unwrap().0, 2);
        app.world_mut().entity_mut(panel).insert(Visibility::Hidden);
        app.update();
        assert_eq!(app.world().get::<TabIndex>(hidden).unwrap().0, -1);
    }

    #[test]
    fn nested_ancestors_and_reparenting_restore_exact_indices_without_changing_opt_out() {
        let mut app = App::new();
        app.add_systems(Update, sync_tab_eligibility);
        // The intermediate node deliberately has no Node or Visibility component.
        let hidden = app.world_mut().spawn(Visibility::Hidden).id();
        let middle = app.world_mut().spawn(ChildOf(hidden)).id();
        let positive = app.world_mut().spawn((TabIndex(7), ChildOf(middle))).id();
        let zero = app.world_mut().spawn((TabIndex(0), ChildOf(middle))).id();
        let opt_out = app.world_mut().spawn((TabIndex(-1), ChildOf(middle))).id();
        app.update();
        for entity in [positive, zero, opt_out] {
            assert_eq!(app.world().get::<TabIndex>(entity).unwrap().0, -1);
        }
        assert!(!app.world().entity(opt_out).contains::<SuppressedTabIndex>());
        let neutral = app.world_mut().spawn_empty().id();
        app.world_mut().entity_mut(middle).insert(ChildOf(neutral));
        app.update();
        assert_eq!(app.world().get::<TabIndex>(positive).unwrap().0, 7);
        assert_eq!(app.world().get::<TabIndex>(zero).unwrap().0, 0);
        assert_eq!(app.world().get::<TabIndex>(opt_out).unwrap().0, -1);
        assert!(
            !app.world()
                .entity(positive)
                .contains::<SuppressedTabIndex>()
        );
        assert!(!app.world().entity(zero).contains::<SuppressedTabIndex>());
    }

    #[test]
    fn reopening_does_not_overwrite_an_explicit_tab_order_change_while_suppressed() {
        let mut app = App::new();
        app.add_systems(Update, sync_tab_eligibility);
        let owner = app.world_mut().spawn(InteractionDisabled).id();
        let control = app.world_mut().spawn((TabIndex(4), ChildOf(owner))).id();
        app.update();
        assert_eq!(app.world().get::<TabIndex>(control).unwrap().0, -1);
        app.world_mut().get_mut::<TabIndex>(control).unwrap().0 = 9;
        app.world_mut()
            .entity_mut(owner)
            .remove::<InteractionDisabled>();
        app.update();
        assert_eq!(app.world().get::<TabIndex>(control).unwrap().0, 9);
        assert!(!app.world().entity(control).contains::<SuppressedTabIndex>());
    }

    #[test]
    fn closing_popup_does_not_restore_indices_and_open_transition_still_respects_ancestors() {
        let mut app = App::new();
        app.add_systems(Update, sync_tab_eligibility)
            .add_observer(restore_opening_menu_tab_indices);
        let ancestor = app
            .world_mut()
            .spawn(Node {
                display: Display::None,
                ..default()
            })
            .id();
        let popup = app
            .world_mut()
            .spawn((bevy::ui_widgets::MenuPopup::default(), ChildOf(ancestor)))
            .id();
        let item = app.world_mut().spawn((TabIndex(6), ChildOf(popup))).id();
        app.update();
        for state in [
            MenuFocusState::Closed,
            MenuFocusState::Opening(NavAction::First),
        ] {
            app.world_mut().entity_mut(popup).insert(state);
            app.world_mut().flush();
            assert_eq!(app.world().get::<TabIndex>(item).unwrap().0, -1);
            assert!(app.world().entity(item).contains::<SuppressedTabIndex>());
        }
        app.world_mut().get_mut::<Node>(ancestor).unwrap().display = Display::Flex;
        app.world_mut()
            .entity_mut(popup)
            .insert(MenuFocusState::Open);
        app.world_mut().flush();
        assert_eq!(
            app.world().get::<TabIndex>(item).unwrap().0,
            6,
            "opening restoration must be synchronous, before the next PreUpdate"
        );
    }

    #[test]
    fn real_tab_and_shift_tab_dispatch_observe_same_frame_eligibility_changes() {
        use bevy::{
            input::{ButtonState, InputPlugin, keyboard::Key},
            input_focus::{
                InputDispatchPlugin, InputFocusPlugin, tab_navigation::TabNavigationPlugin,
            },
            window::PrimaryWindow,
        };
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputPlugin,
            InputFocusPlugin,
            InputDispatchPlugin,
            TabNavigationPlugin,
            super::super::EditorKeyboardPlugin,
        ));
        // Use actual visibility propagation; Node's required visibility starts hidden until
        // that production plugin runs, even in this headless keyboard fixture.
        app.add_plugins((
            bevy::asset::AssetPlugin::default(),
            bevy::mesh::MeshPlugin,
            bevy::transform::TransformPlugin,
            bevy::camera::visibility::VisibilityPlugin,
        ));
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let root = app
            .world_mut()
            .spawn((TabGroup::new(0), ChildOf(window)))
            .id();
        let first = app.world_mut().spawn((TabIndex(0), ChildOf(root))).id();
        let disabled = app
            .world_mut()
            .spawn((TabIndex(1), InteractionDisabled, ChildOf(root)))
            .id();
        let hidden_panel = app
            .world_mut()
            .spawn((
                Node {
                    display: Display::None,
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        let hidden = app
            .world_mut()
            .spawn((TabIndex(2), ChildOf(hidden_panel)))
            .id();
        let last = app.world_mut().spawn((TabIndex(3), ChildOf(root))).id();
        app.update();
        app.insert_resource(InputFocus::from_entity(first));
        super::super::tests::tap(&mut app, window, KeyCode::Tab, Key::Tab);
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(last));
        assert_eq!(app.world().get::<TabIndex>(disabled).unwrap().0, -1);
        assert_eq!(app.world().get::<TabIndex>(hidden).unwrap().0, -1);

        // Restoration must precede keyboard dispatch in this very PreUpdate.
        app.world_mut()
            .entity_mut(disabled)
            .remove::<InteractionDisabled>();
        app.world_mut()
            .get_mut::<Node>(hidden_panel)
            .unwrap()
            .display = Display::Flex;
        app.insert_resource(InputFocus::from_entity(first));
        super::super::tests::tap(&mut app, window, KeyCode::Tab, Key::Tab);
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(disabled));
        super::super::tests::tap(&mut app, window, KeyCode::Tab, Key::Tab);
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(hidden));
        super::super::tests::key(
            &mut app,
            window,
            KeyCode::ShiftRight,
            Key::Shift,
            ButtonState::Pressed,
        );
        super::super::tests::tap(&mut app, window, KeyCode::Tab, Key::Tab);
        app.update();
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(disabled));
    }
}
