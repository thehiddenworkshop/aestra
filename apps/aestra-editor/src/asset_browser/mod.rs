//! Read-only project browser in the existing Assets dock slot.
mod actions;
mod context_menu;
mod inspection;
mod panel;
mod state;
#[cfg(test)]
mod tests;

use crate::*;
pub(crate) use actions::LocateInAssets;
pub(crate) use inspection::spawn_asset_inspector;
pub(crate) use panel::BrowserSurface;
pub(crate) use panel::spawn_assets_panel;
pub(crate) use state::AssetBrowserState;

pub(crate) struct EditorAssetBrowserPlugin;

impl Plugin for EditorAssetBrowserPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AssetBrowserState>()
            .init_resource::<actions::BrowserClickState>()
            .add_observer(actions::activate_button)
            .add_observer(actions::handle_action)
            .add_observer(actions::change_search)
            .add_observer(actions::select_row)
            .add_observer(actions::click_row)
            .add_observer(actions::open_material)
            .add_observer(actions::keyboard)
            .add_observer(actions::activate_locate)
            .add_observer(actions::locate_asset)
            .add_observer(actions::resize_sources)
            .add_observer(context_menu::pointer_menu)
            .add_observer(context_menu::keyboard_menu)
            .add_observer(context_menu::close_after_action)
            .add_systems(
                PostUpdate,
                panel::scroll_to_located_row.after(bevy::ui::UiSystems::Layout),
            )
            .add_systems(
                Update,
                reconcile_snapshot
                    .after(LibrarySet::Input)
                    .before(EditorSet::UiRebuild),
            )
            .add_systems(
                Update,
                (
                    panel::sync_panel,
                    inspection::sync_panel,
                    panel::block_browser_popovers,
                    context_menu::dismiss_menu,
                )
                    .chain()
                    .after(DockingSet::Sync)
                    .before(AestraFeathersSet::Sync),
            );
    }
}

fn reconcile_snapshot(
    catalog: Res<ProjectEffectCatalog>,
    mut state: ResMut<AssetBrowserState>,
    mut clicks: ResMut<actions::BrowserClickState>,
) {
    // Watcher publication deliberately need not mark the entire catalog changed.
    if state.version != Some(catalog.content_revision()) {
        *clicks = default();
        state.reconcile(catalog.content(), catalog.content_revision());
    }
}
