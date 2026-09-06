//! Shared ownership checks for editor-wide keyboard shortcuts.
use bevy::{ecs::system::SystemParam, input_focus::InputFocus, prelude::*, text::EditableText};

#[derive(SystemParam)]
pub(crate) struct ShortcutContext<'w, 's> {
    focus: Option<Res<'w, InputFocus>>,
    editable: Query<'w, 's, (), With<EditableText>>,
    parents: Query<'w, 's, &'static ChildOf>,
    menu_items: Query<'w, 's, (), With<bevy::ui_widgets::MenuItem>>,
    protection: Option<Res<'w, crate::persistence::DocumentProtectionState>>,
    library: Option<Res<'w, crate::library::LibraryAssetOperationState>>,
    menus: Option<Res<'w, crate::menus::MenuState>>,
    material_palette: Option<Res<'w, crate::material_graph::MaterialGraphPaletteState>>,
    windows: Query<'w, 's, &'static Window>,
}

impl ShortcutContext<'_, '_> {
    pub(crate) fn blocked(&self) -> bool {
        if self
            .protection
            .as_ref()
            .is_some_and(|state| state.is_open())
            || self.library.as_ref().is_some_and(|state| state.is_open())
            || self
                .material_palette
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
            if self.editable.contains(entity) || self.menu_items.contains(entity) {
                return true;
            }
            focused = self.parents.get(entity).ok().map(ChildOf::parent);
        }
        false
    }
}
