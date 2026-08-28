//! Native Feathers scroll areas with overflow-aware scrollbars.

use super::scenes;
use crate::ScrollMemoryKey;
use bevy::{
    prelude::*,
    ui_widgets::{ScrollArea, Scrollbar},
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

fn spawn_vertical_scrollbar(parent: &mut ChildSpawnerCommands, target: Entity) {
    parent
        .spawn_empty()
        .apply_scene(scenes::feathers_vertical_scrollbar(target))
        .insert(Node {
            width: Val::Px(10.0),
            height: Val::Percent(100.0),
            display: Display::None,
            padding: UiRect::horizontal(Val::Px(3.0)),
            ..default()
        });
}

pub(crate) fn vertical_scrollbar_needed(viewport_height: f32, content_height: f32) -> bool {
    content_height > viewport_height + 0.5
}

pub(crate) fn update_scrollbar_visibility(
    scroll_areas: Query<&ComputedNode, With<ScrollArea>>,
    mut scrollbars: Query<(&Scrollbar, &mut Node), Without<ScrollArea>>,
) {
    for (scrollbar, mut node) in &mut scrollbars {
        let Ok(viewport) = scroll_areas.get(scrollbar.target) else {
            continue;
        };
        node.display = if vertical_scrollbar_needed(viewport.size().y, viewport.content_size().y) {
            Display::Flex
        } else {
            Display::None
        };
    }
}
