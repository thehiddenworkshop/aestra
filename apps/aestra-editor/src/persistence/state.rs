//! Shared document actions and modal protection state; no native dialog implementation.
use aestra_core::{EffectClipId, EmitterId};
use aestra_project::EffectAssetRef;
use bevy::prelude::*;

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocumentAction {
    New,
    NewInFolder(
        aestra_project::ProjectSourceId,
        aestra_project::ProjectContentVersion,
    ),
    NewProject,
    Open,
    OpenProject,
    OpenRecentProject(usize),
    ExploreExamples,
    ShowProjectLauncher,
    OpenCatalog(EffectAssetRef),
    OpenCatalogClip(EffectAssetRef, EffectClipId),
    OpenSource(EffectAssetRef),
    OpenSourceEmitter(EffectAssetRef, EmitterId),
    BackToSource,
    ForwardToSource,
    NavigateSourceAncestor(usize),
    Save,
    SaveAs,
    SaveAll,
    ReloadMaterial,
    Exit,
}

#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DocumentProtectionState {
    pub(super) recovery_open: bool,
    pub(crate) asset_recovery_open: bool,
    pub(crate) asset_delete_open: bool,
    pub(crate) asset_create_open: bool,
    pub(crate) transaction_open: bool,
    pub(crate) creation_open: bool,
    pub(crate) project_launcher_open: bool,
    pub(super) pending: Option<DocumentAction>,
    pub(super) reload_target: Option<crate::material_document::MaterialEditingTarget>,
    /// The editor view awaiting a dirty-close decision (Save / Discard / Cancel), if any.
    pub(crate) pending_editor_close: Option<crate::docking::EditorViewId>,
}

impl DocumentProtectionState {
    #[cfg(test)]
    pub(crate) fn blocking_states_for_test() -> Vec<Self> {
        vec![
            Self {
                pending: Some(DocumentAction::Save),
                ..default()
            },
            Self {
                pending_editor_close: Some(crate::docking::EditorViewId(1)),
                ..default()
            },
            Self {
                recovery_open: true,
                ..default()
            },
            Self {
                asset_recovery_open: true,
                ..default()
            },
            Self {
                asset_delete_open: true,
                ..default()
            },
            Self {
                asset_create_open: true,
                ..default()
            },
            Self {
                transaction_open: true,
                ..default()
            },
            Self {
                creation_open: true,
                ..default()
            },
            Self {
                project_launcher_open: true,
                ..default()
            },
        ]
    }
    pub(crate) fn is_open(&self) -> bool {
        self.pending.is_some()
            || self.pending_editor_close.is_some()
            || self.recovery_open
            || self.asset_recovery_open
            || self.asset_delete_open
            || self.asset_create_open
            || self.transaction_open
            || self.creation_open
            || self.project_launcher_open
    }
}
