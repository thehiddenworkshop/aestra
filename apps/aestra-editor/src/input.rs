//! Shared ownership checks for editor-wide keyboard shortcuts.
#[path = "input/keypresses.rs"]
pub(crate) mod keypresses;
#[path = "input/tab_navigation.rs"]
mod tab_navigation;
pub(crate) use keypresses::ShortcutKeys;
#[cfg(test)]
use keypresses::{KeyboardPresses, shortcut_key_state};
#[cfg(test)]
#[path = "input/tests/ownership.rs"]
mod ownership_tests;
#[cfg(test)]
#[path = "input/tests/input.rs"]
pub(crate) mod tests;
use bevy::{ecs::system::SystemParam, input_focus::InputFocus, prelude::*, text::EditableText};

pub(crate) struct EditorKeyboardPlugin;

impl Plugin for EditorKeyboardPlugin {
    fn build(&self, app: &mut App) {
        keypresses::register(app);
        app.add_observer(tab_navigation::restore_opening_menu_tab_indices)
            .add_systems(
                PreUpdate,
                tab_navigation::sync_tab_eligibility
                    .before(bevy::input_focus::InputFocusSystems::Dispatch),
            );
    }
}

#[derive(SystemParam)]
pub(crate) struct ShortcutContext<'w, 's> {
    focus: Option<Res<'w, InputFocus>>,
    editable: Query<'w, 's, (), With<EditableText>>,
    code_editors: Query<'w, 's, (), With<crate::feathers::code_editor::CodeEditor>>,
    parents: Query<'w, 's, &'static ChildOf>,
    menu_items: Query<'w, 's, (), With<bevy::ui_widgets::MenuItem>>,
    asset_surfaces: Query<'w, 's, (), With<crate::asset_browser::BrowserSurface>>,
    protection: Option<Res<'w, crate::persistence::DocumentProtectionState>>,
    asset_operation: Option<Res<'w, crate::asset_actions::AssetOperationState>>,
    menus: Option<Res<'w, crate::menus::MenuState>>,
    material_palette: Option<Res<'w, crate::material_graph::MaterialGraphPaletteState>>,
    function_graph_menus:
        Option<Res<'w, crate::material_function_editor::graph::FunctionGraphMenuState>>,
    windows: Query<'w, 's, &'static Window>,
}

impl ShortcutContext<'_, '_> {
    /// Panel-local shortcuts must not also edit the timeline behind Assets.
    /// Keep global history/save shortcuts available by not folding this into blocked().
    pub(crate) fn asset_browser_focused(&self) -> bool {
        let mut focused = self.focus.as_ref().and_then(|focus| focus.get());
        while let Some(entity) = focused {
            if self.asset_surfaces.contains(entity) {
                return true;
            }
            focused = self.parents.get(entity).ok().map(ChildOf::parent);
        }
        false
    }

    pub(crate) fn blocked(&self) -> bool {
        if self
            .protection
            .as_ref()
            .is_some_and(|state| state.is_open())
            || self
                .asset_operation
                .as_ref()
                .is_some_and(|state| state.is_open())
            || self
                .material_palette
                .as_ref()
                .is_some_and(|state| state.is_open())
            || self
                .function_graph_menus
                .as_ref()
                .is_some_and(|state| state.is_open())
            || self.menus.as_ref().is_some_and(|state| {
                state.open.is_some()
                    || state.panels_open
                    || state.tab_context.is_some()
                    || state.show_about
            })
            || (!self.windows.is_empty() && !self.windows.iter().any(|window| window.focused))
        {
            return true;
        }
        let mut focused = self.focus.as_ref().and_then(|focus| focus.get());
        while let Some(entity) = focused {
            if self.editable.contains(entity)
                || self.code_editors.contains(entity)
                || self.menu_items.contains(entity)
            {
                return true;
            }
            focused = self.parents.get(entity).ok().map(ChildOf::parent);
        }
        false
    }
}
