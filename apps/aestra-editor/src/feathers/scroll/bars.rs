//! Real Feathers scrollbar spawning and overflow visibility.
use crate::feathers::scenes;
use bevy::{
    prelude::*,
    ui_widgets::{ControlOrientation, ScrollArea, Scrollbar},
};

pub(crate) fn spawn_vertical_scrollbar(
    parent: &mut ChildSpawnerCommands,
    target: Entity,
) -> Entity {
    parent
        .spawn_empty()
        .apply_scene(scenes::feathers_vertical_scrollbar(target))
        .insert(Node {
            width: Val::Px(10.0),
            height: Val::Percent(100.0),
            display: Display::None,
            padding: UiRect::horizontal(Val::Px(3.0)),
            ..default()
        })
        .id()
}

pub(crate) fn spawn_horizontal_scrollbar(
    parent: &mut ChildSpawnerCommands,
    target: Entity,
) -> Entity {
    parent
        .spawn_empty()
        .apply_scene(scenes::feathers_horizontal_scrollbar(target))
        .insert(Node {
            width: Val::Percent(100.0),
            height: Val::Px(10.0),
            display: Display::None,
            padding: UiRect::vertical(Val::Px(3.0)),
            ..default()
        })
        .id()
}

pub(crate) fn scrollbar_needed(viewport_extent: f32, content_extent: f32) -> bool {
    content_extent > viewport_extent + 0.5
}

pub(crate) fn update_scrollbar_visibility(
    scroll_targets: Query<&ComputedNode>,
    mut scrollbars: Query<(&Scrollbar, &mut Node), Without<ScrollArea>>,
) {
    for (scrollbar, mut node) in &mut scrollbars {
        let Ok(viewport) = scroll_targets.get(scrollbar.target) else {
            continue;
        };
        let needed = match scrollbar.orientation {
            ControlOrientation::Vertical => {
                scrollbar_needed(viewport.size().y, viewport.content_size().y)
            }
            ControlOrientation::Horizontal => {
                scrollbar_needed(viewport.size().x, viewport.content_size().x)
            }
        };
        node.display = if needed { Display::Flex } else { Display::None };
    }
}
