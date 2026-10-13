use crate::docking::ToolPanel;
use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuKind {
    File,
    Edit,
    View,
    Help,
}

#[derive(Resource)]
pub(crate) struct MenuState {
    pub(crate) open: Option<MenuKind>,
    pub(crate) panels_open: bool,
    pub(crate) tab_context: Option<TabContextMenu>,
    pub(crate) show_grid: bool,
    pub(crate) show_about: bool,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            open: None,
            panels_open: false,
            tab_context: None,
            show_grid: true,
            show_about: false,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct TabContextMenu {
    pub(crate) panel: ToolPanel,
    pub(crate) position: [f32; 2],
}

#[derive(Component)]
pub(crate) struct UndoMenuItem;

#[derive(Component)]
pub(crate) struct RedoMenuItem;

#[derive(Component)]
pub(crate) struct AboutDescription;
