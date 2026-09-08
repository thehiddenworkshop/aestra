//! Read-only project content. Paths locate sources; typed asset IDs identify semantic assets.
mod classification;
pub mod operations;
pub(crate) mod queries;
mod refresh;
mod relations;
mod source_tree;

use crate::{ProjectAssetId, ProjectAssetIndex, ProjectSourceId};
pub use classification::*;
pub use refresh::*;
pub use relations::*;
pub use source_tree::*;
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone)]
pub struct ProjectContent {
    source_tree: ProjectSourceTree,
    asset_index: ProjectAssetIndex,
    documents: BTreeMap<ProjectSourceId, ProjectSourceDocument>,
    sources_by_asset: BTreeMap<ProjectAssetId, Vec<ProjectSourceId>>,
    relations: relations::RelationIndex,
}

impl ProjectContent {
    pub fn scan(root: impl AsRef<Path>) -> Self {
        Self::from_source_tree(ProjectSourceTree::scan(root))
    }

    /// Build the semantic join from an existing discovery, without walking the root again.
    pub fn from_source_tree(mut source_tree: ProjectSourceTree) -> Self {
        let (asset_index, documents) = ProjectAssetIndex::from_source_tree(&source_tree, false);
        for entry in asset_index.effects() {
            source_tree
                .file_mut(entry.id)
                .expect("indexed effect source exists")
                .classification = ProjectFileClassification::Effect;
        }
        let mut sources_by_asset = BTreeMap::<_, Vec<_>>::new();
        let bindings = asset_index
            .effects()
            .iter()
            .map(|entry| {
                (
                    entry.id,
                    entry.reference.map(|r| ProjectAssetId::Effect(r.id)),
                )
            })
            .chain(asset_index.material_programs().iter().map(|entry| {
                (
                    entry.id,
                    entry
                        .reference
                        .map(|r| ProjectAssetId::MaterialProgram(r.id())),
                )
            }))
            .chain(asset_index.material_functions().iter().map(|entry| {
                (
                    entry.id,
                    entry
                        .reference
                        .map(|r| ProjectAssetId::MaterialFunction(r.id())),
                )
            }))
            .chain(
                asset_index
                    .material_presets()
                    .iter()
                    .map(|entry| (entry.id, entry.preset.map(ProjectAssetId::MaterialPreset))),
            );
        for (source, asset) in bindings {
            if let Some(asset) = asset {
                let file = source_tree
                    .file_mut(source)
                    .expect("index sources originate in this discovery");
                file.semantic_asset = Some(asset);
                sources_by_asset.entry(asset).or_default().push(source);
            }
        }
        for sources in sources_by_asset.values_mut() {
            sources.sort_by_key(|id| source_tree.source(*id).unwrap().relative_path.clone());
        }
        let relations = relations::RelationIndex::build(&documents);
        Self {
            source_tree,
            asset_index,
            documents,
            sources_by_asset,
            relations,
        }
    }

    pub fn source_tree(&self) -> &ProjectSourceTree {
        &self.source_tree
    }
    pub fn asset_index(&self) -> &ProjectAssetIndex {
        &self.asset_index
    }
    pub fn source(&self, id: ProjectSourceId) -> Option<&ProjectSourceEntry> {
        self.source_tree.source(id)
    }
    pub fn asset_for_source(&self, id: ProjectSourceId) -> Option<ProjectAssetId> {
        match &self.source(id)?.kind {
            ProjectSourceKind::File(file) => file.semantic_asset,
            _ => None,
        }
    }
    pub fn sources_for_asset(&self, asset: ProjectAssetId) -> &[ProjectSourceId] {
        self.sources_by_asset
            .get(&asset)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    /// Unique location lookup only. Cached typed reads validate snapshot identity/status;
    /// index loaders additionally check current disk contents.
    pub fn unique_source_for_asset(
        &self,
        asset: ProjectAssetId,
    ) -> Result<&ProjectSourceEntry, ProjectContentResolveError> {
        if let crate::ProjectAssetIndexAvailability::Unavailable { root, message } =
            self.asset_index.availability()
        {
            return Err(ProjectContentResolveError::Unavailable {
                root: root.clone(),
                message: message.clone(),
            });
        }
        match self.sources_for_asset(asset) {
            [] => Err(ProjectContentResolveError::Missing { asset }),
            [id] => Ok(self.source(*id).expect("joined source exists")),
            sources => Err(ProjectContentResolveError::Ambiguous {
                asset,
                sources: sources.to_vec(),
            }),
        }
    }
}

/// Documents are retained by source, not a second semantic registry. All identity/ambiguity
/// decisions go through the single index built from these exact parses.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ProjectSourceDocument {
    Effect(Box<aestra_core::EffectAsset>),
    MaterialProgram(Box<aestra_core::material::MaterialProgram>),
    MaterialFunction(Box<aestra_core::material::MaterialFunction>),
    MaterialPreset(Box<aestra_core::material::MaterialPresetDescriptor>),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectContentResolveError {
    #[error("project root {root:?} is unavailable: {message}")]
    Unavailable {
        root: std::path::PathBuf,
        message: String,
    },
    #[error("project asset {asset:?} has no source")]
    Missing { asset: ProjectAssetId },
    #[error("project asset {asset:?} has multiple sources: {sources:?}")]
    Ambiguous {
        asset: ProjectAssetId,
        sources: Vec<ProjectSourceId>,
    },
}
