//! Shared status-bar surface.

use crate::theme;
use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct EditorStatusBar;

pub(crate) fn status_bar() -> impl Bundle {
    (
        EditorStatusBar,
        Node {
            grid_row: GridPlacement::start(4),
            width: Val::Percent(100.0),
            height: Val::Px(24.0),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(Val::Px(12.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(theme::PANEL_DARK),
    )
}
