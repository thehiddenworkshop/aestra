//! Editor adapter for one coherent project snapshot and unsaved shared-source drafts.
pub(crate) mod io;
mod plugin;
pub(crate) use plugin::EditorProjectContentPlugin;
mod services;
pub(crate) use services::ProjectContentSet;
#[cfg(test)]
mod catalog_tests;
mod refresh;
use crate::*;
#[cfg(test)]
use aestra_core::{EffectAssetRef, material::MaterialProgram};
#[cfg(test)]
use aestra_project::{ProjectAssetIndexAvailability, ProjectEffectStatus};
pub(crate) use refresh::ProjectEffectWatchState;
#[cfg(test)]
pub(crate) use refresh::{apply_project_effect_catalog_refresh, poll_project_effect_catalog};
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};
mod model;
pub(crate) use model::EditorProjectContent;
use model::PreparedProject;

impl Default for EditorProjectContent {
    fn default() -> Self {
        let root = default_project_root();
        Self::scan_project(&root, root.join("effects"))
    }
}

/// The project the editor opens by default: the curated sample project at the repo root. Editor UI
/// assets live under `assets/`; workspace test fixtures live under `assets/test/`.
pub(crate) fn default_project_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-project");
    root.canonicalize().unwrap_or(root)
}
