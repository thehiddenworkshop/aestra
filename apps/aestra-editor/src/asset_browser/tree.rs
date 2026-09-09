//! Tree gestures navigate on release so pressing a folder does not destroy a drag source.
use super::{actions::BrowserAction, panel::BrowserFolderButton};
use crate::*;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::ui_widgets::Activate;

#[derive(Resource, Default)]
struct TreeFocus(Option<(Entity, aestra_project::ProjectSourceId)>);
#[derive(Component, Clone, Copy)]
struct Rename(
    Entity,
    aestra_project::ProjectSourceId,
    aestra_project::ProjectContentVersion,
);
#[derive(Component)]
struct Menu;

pub(super) fn register(app: &mut App) {
    app.init_resource::<TreeFocus>()
        .add_observer(click)
        .add_observer(keyboard)
        .add_observer(rename)
        .add_observer(
            |_: On<BrowserAction>, menus: Query<Entity, With<Menu>>, mut commands: Commands| {
                for menu in &menus {
                    commands.entity(menu).try_despawn();
                }
            },
        )
        .add_systems(PostUpdate, restore_focus);
}

#[allow(clippy::too_many_arguments)]
fn click(
    mut event: On<Pointer<Click>>,
    folders: Query<&BrowserAction, With<BrowserFolderButton>>,
    parents: Query<&ChildOf>,
    editors: Query<(), With<super::operations::InlineRenameEditor>>,
    drag: Res<super::drag_drop::AssetDrag>,
    catalog: Res<ProjectEffectCatalog>,
    mut focus: ResMut<InputFocus>,
    mut tree: ResMut<TreeFocus>,
    menus: Query<Entity, With<Menu>>,
    localizer: Res<Localizer>,
    mut commands: Commands,
) {
    let ancestors: Vec<_> = std::iter::once(event.entity)
        .chain(parents.iter_ancestors(event.entity))
        .collect();
    if ancestors.iter().any(|entity| menus.contains(*entity)) {
        event.propagate(false);
        return;
    }
    if ancestors.iter().any(|id| editors.contains(*id)) {
        return;
    }
    let Some((entity, source)) = ancestors
        .iter()
        .find_map(|id| match folders.get(*id).ok()? {
            BrowserAction::Navigate(source) => Some((*id, *source)),
            _ => None,
        })
    else {
        return;
    };
    event.propagate(false);
    if drag.suppress_click {
        return;
    }
    focus.set(entity, FocusCause::Navigated);
    tree.0 = Some((entity, source));
    if event.button == PointerButton::Primary {
        commands.trigger(BrowserAction::Navigate(source));
    } else if event.button == PointerButton::Secondary {
        for menu in &menus {
            commands.entity(menu).try_despawn();
        }
        if catalog.content().source_relocation_suffix(source).is_some() {
            commands.entity(entity).with_children(|host| {
                crate::feathers::context_menu::spawn_pointer_context_menu(
                    host,
                    Vec2::new(24.0, 24.0),
                    Menu,
                    super::panel::BrowserSurface,
                    |menu| {
                        crate::feathers::context_menu::spawn_pointer_context_menu_item(
                            menu,
                            &localizer.text("browser-rename"),
                            Rename(entity, source, catalog.content_revision()),
                        );
                        crate::feathers::context_menu::spawn_pointer_context_menu_item(
                            menu,
                            &localizer.text("browser-delete"),
                            BrowserAction::Delete(source, catalog.content_revision()),
                        );
                    },
                );
            });
        }
    }
}

fn rename(
    event: On<Activate>,
    choices: Query<&Rename>,
    menus: Query<Entity, With<Menu>>,
    mut commands: Commands,
) {
    if let Ok(Rename(entity, source, version)) = choices.get(event.entity) {
        commands.trigger(super::operations::OpenFolderPrompt(
            Some((*source, *version)),
            true,
            Some(*entity),
        ));
        for menu in &menus {
            commands.entity(menu).try_despawn();
        }
    }
}

fn keyboard(
    mut event: On<FocusedInput<KeyboardInput>>,
    folders: Query<&BrowserAction, With<BrowserFolderButton>>,
    catalog: Res<ProjectEffectCatalog>,
    menus: Query<Entity, With<Menu>>,
    mut commands: Commands,
) {
    if event.input.state != ButtonState::Pressed || event.input.repeat {
        return;
    }
    if event.input.key_code == KeyCode::Escape {
        for menu in &menus {
            commands.entity(menu).try_despawn();
        }
    }
    let Ok(BrowserAction::Navigate(source)) = folders.get(event.focused_entity) else {
        return;
    };
    match event.input.key_code {
        KeyCode::F2 => commands.trigger(super::operations::OpenFolderPrompt(
            Some((*source, catalog.content_revision())),
            true,
            Some(event.focused_entity),
        )),
        KeyCode::Enter => commands.trigger(BrowserAction::Navigate(*source)),
        KeyCode::Delete => {
            commands.trigger(BrowserAction::Delete(*source, catalog.content_revision()))
        }
        _ => return,
    }
    event.propagate(false);
}

fn restore_focus(
    mut tree: ResMut<TreeFocus>,
    mut focus: ResMut<InputFocus>,
    entities: Query<Entity>,
    folders: Query<(Entity, &BrowserAction), With<BrowserFolderButton>>,
) {
    let Some((old, source)) = tree.0 else {
        return;
    };
    if entities.contains(old) {
        return;
    }
    if focus
        .get()
        .is_some_and(|entity| entity != old && entities.contains(entity))
    {
        tree.0 = None;
        return;
    }
    if let Some((entity, _)) = folders
        .iter()
        .find(|(_, action)| **action == BrowserAction::Navigate(source))
    {
        focus.set(entity, FocusCause::Navigated);
        tree.0 = Some((entity, source));
    } else {
        tree.0 = None;
    }
}
