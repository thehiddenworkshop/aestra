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
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
mod model;
mod paths;
pub(crate) use model::EditorProjectContent;
use model::PreparedProject;

impl Default for EditorProjectContent {
    fn default() -> Self {
        let root = default_project_root();
        Self::scan_project(&root, root.join("effects"))
    }
}

/// Bundled examples, relative to the executable rather than the build checkout.
pub(crate) fn default_project_root() -> PathBuf {
    let executable = std::env::current_exe().expect("editor executable location unavailable");
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from);
    let root = paths::project_root(&executable, manifest.as_deref());
    root.canonicalize().unwrap_or(root)
}
