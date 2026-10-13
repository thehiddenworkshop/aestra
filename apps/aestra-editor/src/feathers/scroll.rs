//! Native Feathers scroll areas with overflow-aware scrollbars.

use crate::ScrollMemoryKey;
use bevy::{prelude::*, ui_widgets::ScrollArea};

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

#[path = "scroll/bars.rs"]
mod bars;
#[cfg(test)]
pub(crate) use bars::scrollbar_needed;
pub(crate) use bars::{
    spawn_horizontal_scrollbar, spawn_vertical_scrollbar, update_scrollbar_visibility,
};
