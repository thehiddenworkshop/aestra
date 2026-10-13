//! Root-local shortcuts. Missing locations remain dormant so deletion Undo restores them.
#[cfg(test)]
mod tests;
use super::{state::*, *};
use aestra_project::ProjectAssetId;
#[cfg(test)]
use aestra_project::{ProjectContent, ProjectSourceId};
use std::path::PathBuf;

mod model;
pub(super) use model::{Bookmark, BrowserCollection, FAVORITES_LIMIT, RECENT_LIMIT};

/// Sent only after successful document activation, never by selection or an open attempt.
#[derive(Event)]
pub(crate) struct AssetOpened {
    pub(crate) root: PathBuf,
    pub(crate) relative: PathBuf,
}

pub(super) fn record_open(
    event: On<AssetOpened>,
    catalog: Res<ProjectEffectCatalog>,
    mut state: ResMut<AssetBrowserState>,
    mut persistence: ResMut<persistence::BrowserPersistence>,
) {
    if event.root != catalog.root() {
        return;
    }
    if persistence.needs_restore(catalog.content(), &state, catalog.content_revision()) {
        persistence.restore_root(&mut state, catalog.content(), catalog.content_revision());
    }
    if let Some(entry) = catalog
        .content()
        .source_tree()
        .at_relative_path(&event.relative)
    {
        state.record_recent(catalog.content(), entry.id);
    }
}

pub(super) fn opened_asset(
    commands: &mut Commands,
    catalog: &ProjectEffectCatalog,
    asset: ProjectAssetId,
) {
    if let [id] = catalog.content().sources_for_asset(asset)
        && let Some(entry) = catalog.content().source(*id)
    {
        commands.trigger(AssetOpened {
            root: catalog.root().to_owned(),
            relative: entry.relative_path.clone(),
        });
    }
}
