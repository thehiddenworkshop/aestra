//! Native Feathers scroll areas with overflow-aware scrollbars.

use super::scenes;
use crate::ScrollMemoryKey;
use bevy::{
    prelude::*,
    ui_widgets::{ControlOrientation, ScrollArea, Scrollbar},
};

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct PersistedScroll(pub(crate) ScrollMemoryKey);

pub(crate) fn spawn_vertical_scroll_area(
    parent: &mut ChildSpawnerCommands,
    memory: ScrollMemoryKey,
    mut viewport: Node,
    content: impl FnOnce(&mut ChildSpawnerCommands),
) -> Entity {
    viewport.overflow = Overflow::scroll_y();
    viewport.scrollbar_width = 0.0;
    let target = parent
        .spawn((viewport, ScrollArea, PersistedScroll(memory)))
        .with_children(content)
        .id();
    spawn_vertical_scrollbar(parent, target);
    target
}

/// A scroll area that scrolls both axes, with a vertical scrollbar on the right and a horizontal
/// scrollbar below. Used by the code editor, whose lines do not wrap and so overflow horizontally.
/// The `viewport` node's overflow is forced to scroll on both axes; the returned entity is the
/// scrollable viewport that `content` is spawned into.
pub(crate) fn spawn_scroll_area_xy(
    parent: &mut ChildSpawnerCommands,
    memory: ScrollMemoryKey,
    mut viewport: Node,
    content: impl FnOnce(&mut ChildSpawnerCommands),
) -> Entity {
    viewport.overflow = Overflow::scroll();
    viewport.scrollbar_width = 0.0;
    let mut target = Entity::PLACEHOLDER;
    parent
        .spawn(Node {
            flex_grow: 1.0,
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|column| {
            column
                .spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    ..default()
                })
                .with_children(|row| {
                    target = row
                        .spawn((viewport, ScrollArea, PersistedScroll(memory)))
                        .with_children(content)
                        .id();
                    spawn_vertical_scrollbar(row, target);
                });
            spawn_horizontal_scrollbar(column, target);
        });
    target
}

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
