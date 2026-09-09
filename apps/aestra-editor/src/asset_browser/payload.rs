//! Shared source identity for authoring drops and, later, compatible asset pickers.
//! A drag is a snapshot-bound intent, never authority to mutate a file/document.
use crate::*;
use aestra_project::{ProjectAssetId, ProjectContentVersion, ProjectSourceId};
use std::path::PathBuf;

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AssetPayload {
    root: PathBuf,
    version: ProjectContentVersion,
    source: ProjectSourceId,
    asset: Option<ProjectAssetId>,
}

impl AssetPayload {
    pub(crate) fn capture(catalog: &ProjectEffectCatalog, source: ProjectSourceId) -> Self {
        Self {
            root: catalog.root().to_path_buf(),
            version: catalog.content_revision(),
            source,
            asset: catalog.content().asset_for_source(source),
        }
    }

    pub(crate) fn resolve(
        &self,
        catalog: &ProjectEffectCatalog,
    ) -> Result<Option<ProjectAssetId>, String> {
        if self.root != catalog.root() || self.version != catalog.content_revision() {
            return Err("Project changed; drag the asset again".into());
        }
        catalog
            .content()
            .source(self.source)
            .ok_or("Source no longer exists")?;
        if catalog.content().asset_for_source(self.source) != self.asset {
            return Err("Asset identity changed; drag the asset again".into());
        }
        if let Some(asset) = self.asset {
            let source = catalog
                .content()
                .unique_source_for_asset(asset)
                .map_err(|error| error.to_string())?;
            if source.id != self.source {
                return Err("Asset source changed; drag the asset again".into());
            }
        }
        Ok(self.asset)
    }

    pub(crate) fn timeline_effect(
        &self,
        catalog: &ProjectEffectCatalog,
    ) -> Result<(aestra_core::EffectAssetRef, String), String> {
        let Some(ProjectAssetId::Effect(id)) = self.resolve(catalog)? else {
            return Err("The timeline accepts effect assets, not this asset type".into());
        };
        let reference = aestra_core::EffectAssetRef::new(id);
        let effect = catalog.cached_effect(reference)?;
        Ok((reference, effect.name))
    }
}
