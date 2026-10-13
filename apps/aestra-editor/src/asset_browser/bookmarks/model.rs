//! Actual root-local bookmark identity and bounded collection model.
use crate::asset_browser::state::AssetBrowserState;
use aestra_project::{ProjectAssetId, ProjectContent, ProjectSourceEntry, ProjectSourceId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(crate) const FAVORITES_LIMIT: usize = 256;
pub(crate) const RECENT_LIMIT: usize = 64;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum BrowserCollection {
    #[default]
    Folder,
    Favorites,
    Recent,
}

impl BrowserCollection {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Folder => "browser-collection-folder",
            Self::Favorites => "browser-collection-favorites",
            Self::Recent => "browser-collection-recent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Bookmark {
    pub(crate) path: PathBuf,
    asset: Option<ProjectAssetId>,
}

impl Bookmark {
    pub(crate) fn new(content: &ProjectContent, id: ProjectSourceId) -> Option<Self> {
        let entry = content.source(id)?;
        Some(Self {
            path: entry.relative_path.clone(),
            asset: content.asset_for_source(id),
        })
    }

    pub(crate) fn resolve<'a>(
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
    pub(crate) fn is_favorite(&self, content: &ProjectContent, id: ProjectSourceId) -> bool {
        self.favorites
            .iter()
            .any(|item| item.resolve(content).is_some_and(|entry| entry.id == id))
    }

    pub(crate) fn toggle_favorite(&mut self, content: &ProjectContent, id: ProjectSourceId) {
        if self.is_favorite(content, id) {
            self.favorites
                .retain(|item| item.resolve(content).is_none_or(|entry| entry.id != id));
        } else if self.favorites.len() < FAVORITES_LIMIT
            && let Some(item) = Bookmark::new(content, id)
        {
            self.favorites.push(item);
        }
    }

    pub(crate) fn record_recent(&mut self, content: &ProjectContent, id: ProjectSourceId) {
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
