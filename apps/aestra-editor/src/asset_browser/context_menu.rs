//! Explicit asset operations; opening a menu never opens the underlying document.
use super::{
    actions::BrowserAction,
    panel::{BrowserItems, BrowserRow},
    state::*,
};
use crate::{feathers::context_menu::*, *};
use aestra_project::{ProjectContentVersion, ProjectSourceId};
use bevy::{
    input_focus::{FocusCause, FocusedInput, InputFocus},
    ui_widgets::ActiveDescendant,
};

#[derive(Component)]
pub(super) struct BrowserContextAnchor {
    list: Entity,
    version: ProjectContentVersion,
}
#[derive(Component)]
pub(super) struct BrowserContextMenu;

fn spawn_menu(
    commands: &mut Commands,
    parent: Entity,
    list: Entity,
    position: Vec2,
    source: Option<ProjectSourceId>,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) {
    commands.entity(parent).with_children(|host| {
        spawn_pointer_context_menu(
            host,
            position,
            BrowserContextAnchor {
                list,
                version: catalog.content_revision(),
            },
            BrowserContextMenu,
            |menu| {
                if let Some(source) = source {
                    let openable = catalog.content().source(source).is_some_and(|entry| {
                        matches!(
                            Kind::of(entry),
                            Kind::Folder | Kind::Effect | Kind::Material
                        )
                    });
                    if openable {
                        spawn_pointer_context_menu_item(
                            menu,
                            &localizer.text("browser-open-selected"),
                            BrowserAction::OpenSelected,
                        );
                    }
                    if matches!(
                        catalog.content().asset_for_source(source),
                        Some(
                            aestra_project::ProjectAssetId::MaterialProgram(_)
                                | aestra_project::ProjectAssetId::MaterialFunction(_)
                        )
                    ) {
                        spawn_pointer_context_menu_item(
                            menu,
                            &localizer.text("browser-duplicate"),
                            BrowserAction::Duplicate(source, catalog.content_revision()),
                        );
                    }
                    for (key, tab) in [
                        ("browser-asset-details", InspectionTab::Details),
                        ("browser-references", InspectionTab::Dependencies),
                    ] {
                        spawn_pointer_context_menu_item(
                            menu,
                            &localizer.text(key),
                            BrowserAction::InspectSource(source, catalog.content_revision(), tab),
                        );
                    }
                } else {
                    spawn_pointer_context_menu_item(
                        menu,
                        &localizer.text("browser-open-project"),
                        BrowserAction::OpenProject,
                    );
                    spawn_pointer_context_menu_item(
                        menu,
                        &localizer.text("browser-refresh"),
                        BrowserAction::Refresh,
                    );
                }
            },
        );
    });
}

pub(super) fn pointer_menu(
    mut event: On<Pointer<Click>>,
    rows: Query<(&BrowserRow, &ComputedNode, &UiGlobalTransform), With<ListItem>>,
    lists: Query<(&ComputedNode, &UiGlobalTransform), With<BrowserItems>>,
    parents: Query<&ChildOf>,
    menus: Query<Entity, With<BrowserContextAnchor>>,
    catalog: Res<ProjectEffectCatalog>,
    localizer: Res<Localizer>,
    mut state: ResMut<AssetBrowserState>,
    mut clicks: ResMut<super::actions::BrowserClickState>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Secondary {
        return;
    }
    let ancestry = std::iter::once(event.entity)
        .chain(parents.iter_ancestors(event.entity))
        .collect::<Vec<_>>();
    let Some(list) = ancestry.iter().copied().find(|id| lists.contains(*id)) else {
        return;
    };
    *clicks = default();
    for menu in &menus {
        commands.entity(menu).despawn();
    }
    let row = ancestry.iter().copied().find(|id| rows.contains(*id));
    let (parent, source, node, transform) = if let Some(row) = row {
        let (source, node, transform) = rows.get(row).unwrap();
        state.selected = Some(source.0);
        commands.entity(list).insert(ActiveDescendant(Some(row)));
        (row, Some(source.0), node, transform)
    } else {
        let (node, transform) = lists.get(list).unwrap();
        (list, None, node, transform)
    };
    focus.set(list, FocusCause::Navigated);
    let position = pointer_position_in_node(event.pointer_location.position, node, transform)
        * node.inverse_scale_factor();
    spawn_menu(
        &mut commands,
        parent,
        list,
        position,
        source,
        &catalog,
        &localizer,
    );
    event.propagate(false);
}

pub(super) fn keyboard_menu(
    mut event: On<FocusedInput<KeyboardInput>>,
    lists: Query<&ActiveDescendant, With<BrowserItems>>,
    rows: Query<(&BrowserRow, &ComputedNode)>,
    menus: Query<Entity, With<BrowserContextAnchor>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    catalog: Res<ProjectEffectCatalog>,
    localizer: Res<Localizer>,
    mut state: ResMut<AssetBrowserState>,
    mut clicks: ResMut<super::actions::BrowserClickState>,
    mut commands: Commands,
) {
    let Ok(active) = lists.get(event.focused_entity) else {
        return;
    };
    if event.input.state != ButtonState::Pressed || event.input.repeat {
        return;
    }
    let requested = event.input.key_code == KeyCode::ContextMenu
        || (event.input.key_code == KeyCode::F10
            && keys.as_deref().is_some_and(|keys| {
                keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight)
            }));
    if !requested {
        return;
    }
    *clicks = default();
    let Some(row) = active.0 else {
        return;
    };
    let Ok((source, node)) = rows.get(row) else {
        return;
    };
    for menu in &menus {
        commands.entity(menu).despawn();
    }
    state.selected = Some(source.0);
    spawn_menu(
        &mut commands,
        row,
        event.focused_entity,
        Vec2::new(8.0, node.size().y * node.inverse_scale_factor()),
        Some(source.0),
        &catalog,
        &localizer,
    );
    event.propagate(false);
}

pub(super) fn close_after_action(
    event: On<bevy::ui_widgets::Activate>,
    actions: Query<&BrowserAction>,
    menus: Query<(Entity, &BrowserContextAnchor)>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
    mut focus: ResMut<InputFocus>,
) {
    if !actions.contains(event.entity) {
        return;
    }
    for (menu, anchor) in &menus {
        if parents.iter_ancestors(event.entity).any(|id| id == menu) {
            focus.set(anchor.list, FocusCause::Navigated);
            commands.entity(menu).despawn();
        }
    }
}

pub(super) fn dismiss_menu(
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    surfaces: Query<&RelativeCursorPosition, With<BrowserContextMenu>>,
    menus: Query<(Entity, &BrowserContextAnchor)>,
    catalog: Res<ProjectEffectCatalog>,
    mut commands: Commands,
    mut focus: ResMut<InputFocus>,
) {
    let escape = keys
        .as_deref()
        .is_some_and(|keys| keys.just_pressed(KeyCode::Escape));
    let outside = should_dismiss_pointer_context_menu(
        true,
        buttons
            .as_deref()
            .is_some_and(|buttons| buttons.just_pressed(MouseButton::Left)),
        escape,
        surfaces.iter().any(RelativeCursorPosition::cursor_over),
    );
    for (menu, anchor) in &menus {
        if outside || catalog.content_revision() != anchor.version {
            if escape {
                focus.set(anchor.list, FocusCause::Navigated);
            }
            commands.entity(menu).despawn();
        }
    }
}
