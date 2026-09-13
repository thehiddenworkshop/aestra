//! Root-local shortcuts. Missing locations remain dormant so deletion Undo restores them.
#[cfg(test)]
mod tests;
use super::{state::*, *};
use aestra_project::{ProjectAssetId, ProjectContent, ProjectSourceEntry, ProjectSourceId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(super) const FAVORITES_LIMIT: usize = 256;
pub(super) const RECENT_LIMIT: usize = 64;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum BrowserCollection {
    #[default]
    Folder,
    Favorites,
    Recent,
}

impl BrowserCollection {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Folder => "browser-collection-folder",
            Self::Favorites => "browser-collection-favorites",
            Self::Recent => "browser-collection-recent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Bookmark {
    pub(super) path: PathBuf,
    asset: Option<ProjectAssetId>,
}

impl Bookmark {
    fn new(content: &ProjectContent, id: ProjectSourceId) -> Option<Self> {
        let entry = content.source(id)?;
        Some(Self {
            path: entry.relative_path.clone(),
            asset: content.asset_for_source(id),
        })
    }

    pub(super) fn resolve<'a>(
        &self,
        content: &'a ProjectContent,
    ) -> Option<&'a ProjectSourceEntry> {
        // Prefer the exact location, including duplicate IDs. Do not bind an old typed bookmark
        // to a different asset that happens to occupy its former filename.
        if let Some(entry) = content.source_tree().at_relative_path(&self.path)
            && (self.asset.is_none() || content.asset_for_source(entry.id) == self.asset)
        {
            return Some(entry);
        }
        let [id] = content.sources_for_asset(self.asset?) else {
            return None;
        };
        content.source(*id)
    }
}

impl AssetBrowserState {
    pub(super) fn is_favorite(&self, content: &ProjectContent, id: ProjectSourceId) -> bool {
        self.favorites
            .iter()
            .any(|item| item.resolve(content).is_some_and(|entry| entry.id == id))
    }

    pub(super) fn toggle_favorite(&mut self, content: &ProjectContent, id: ProjectSourceId) {
        if self.is_favorite(content, id) {
            self.favorites
                .retain(|item| item.resolve(content).is_none_or(|entry| entry.id != id));
        } else if self.favorites.len() < FAVORITES_LIMIT
            && let Some(item) = Bookmark::new(content, id)
        {
            self.favorites.push(item);
        }
    }

    pub(super) fn record_recent(&mut self, content: &ProjectContent, id: ProjectSourceId) {
        let Some(item) = Bookmark::new(content, id) else {
            return;
        };
        self.recent.retain(|previous| {
            previous.path != item.path
                && previous.resolve(content).is_none_or(|entry| entry.id != id)
        });
        self.recent.insert(0, item);
        self.recent.truncate(RECENT_LIMIT);
    }
}

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
