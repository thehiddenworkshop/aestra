//! Shared editor identities; no document manager, docking plugin or filesystem ownership.
use aestra_core::{MaterialFunctionId, MaterialProgramId};
use serde::{Deserialize, Serialize};
use std::{
    hash::{Hash, Hasher},
    path::Path,
};

/// Stable identity of one open document within an editor session. Distinct from the asset it
/// authors ([`DocumentKey`]) and from the dock tabs / editor views that display it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct DocumentId(pub(crate) u64);

/// Monotonic per-document edit counter. Async work (compilation, previews) is tagged with the
/// revision it started from, so a stale result can be discarded once the document changes again.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DocumentRevision(pub(crate) u64);

/// Identifies the project asset a document authors. One asset corresponds to one open document by
/// default, so opening the same asset twice never creates two independent drafts.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) enum DocumentKey {
    MaterialProgram(MaterialProgramId),
    MaterialFunction(MaterialFunctionId),
    /// A standalone WESL source module, identified by its project-relative path (Milestone 7).
    WeslSource(WeslSourceId),
}

/// Stable identity of a WESL source document: a deterministic hash of its project-relative path.
/// Serializable so it can persist in the editor-document manifest, and `Copy` so [`DocumentKey`]
/// stays `Copy`. Like the source-tree ids, it changes if the file is moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct WeslSourceId {
    // Two u64 halves rather than a u128 so the id round-trips cleanly through RON in the manifest.
    high: u64,
    low: u64,
}

impl WeslSourceId {
    pub(crate) fn for_relative_path(relative: &Path) -> Self {
        // Normalize separators so the same file yields the same id on every platform.
        let normalized = relative.to_string_lossy().replace('\\', "/");
        let hash = |salt: u64| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            salt.hash(&mut hasher);
            normalized.hash(&mut hasher);
            hasher.finish()
        };
        Self {
            high: hash(0),
            low: hash(0xa5a5_5a5a),
        }
    }
}

impl std::fmt::Display for WeslSourceId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:016x}{:016x}", self.high, self.low)
    }
}

/// Identity of one open editor-view instance (a dockable asset editor). Distinct from the editor's
/// kind and from its document, so several editors of the same kind can be docked at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct EditorViewId(pub(crate) u64);
