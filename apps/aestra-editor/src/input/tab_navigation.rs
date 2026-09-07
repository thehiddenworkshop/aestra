//! Bevy tab order is structural; retained hidden/disabled controls must be excluded.
use bevy::{input_focus::tab_navigation::TabIndex, prelude::*, ui::InteractionDisabled};

#[derive(Component)]
pub(super) struct SuppressedTabIndex(i32);

pub(super) fn sync_tab_eligibility(
    mut commands: Commands,
    mut controls: Query<(Entity, &mut TabIndex, Option<&SuppressedTabIndex>)>,
    ancestors: Query<(
        Option<&Node>,
        Option<&Visibility>,
        Has<InteractionDisabled>,
        Option<&ChildOf>,
    )>,
) {
    for (entity, mut index, suppressed) in &mut controls {
        let mut current = Some(entity);
        let mut eligible = true;
        while let Some(target) = current {
            let Ok((node, visibility, disabled, parent)) = ancestors.get(target) else {
                break;
            };
            if disabled
                || node.is_some_and(|node| node.display == Display::None)
                || visibility.is_some_and(|visibility| *visibility == Visibility::Hidden)
            {
                eligible = false;
                break;
            }
            current = parent.map(ChildOf::parent);
        }
        if eligible {
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
}
