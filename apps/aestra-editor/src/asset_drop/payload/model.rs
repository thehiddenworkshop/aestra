//! Actual immutable asset-drop identity payload.
use aestra_project::{ProjectAssetId, ProjectContentVersion, ProjectSourceId};
use bevy::prelude::*;
use std::path::PathBuf;

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AssetPayload {
    pub(super) root: PathBuf,
    pub(super) version: ProjectContentVersion,
    pub(super) source: Option<ProjectSourceId>,
    pub(super) asset: Option<ProjectAssetId>,
    pub(super) virtual_asset: Option<VirtualAsset>,
    pub(super) document: Option<(aestra_core::EffectId, u64, u64)>,
}

/// Virtual resources have semantic identities, never filesystem source identities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VirtualAsset {
    BuiltInPreset(aestra_core::MaterialPresetId),
    Material(aestra_core::MaterialId),
    Texture(aestra_core::AssetId),
    Mesh(aestra_core::AssetId),
    Flipbook(aestra_core::AssetId),
    /// Legacy declaration without atlas metadata: inspectable, not assignable.
    FlipbookDeclaration(aestra_core::AssetId),
}
