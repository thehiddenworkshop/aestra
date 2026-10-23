//! Data-driven editor combo boxes and compact action menus.
mod selection;
#[path = "combo_box/selection_input.rs"]
mod selection_input;
pub(super) use selection::sync_action_menu_availability;
pub(crate) use selection::{ComboOption, spawn_action_combo_control, spawn_combo_control};
mod actions;
pub(crate) use actions::{
    spawn_action_menu, spawn_compact_action_menu, spawn_icon_action_menu,
    spawn_searchable_action_list, spawn_searchable_icon_action_menu,
};
