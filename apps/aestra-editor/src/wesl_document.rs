//! Standalone WESL source documents (Milestone 7).
//!
//! WESL modules are plain `.wesl` text files, distinct from the semantic material assets. They are
//! identified by their project-relative path (hashed into a stable, serializable [`WeslSourceId`] so
//! it can key a [`crate::document::DocumentKey`]) and their text lives in an in-memory buffer here.
//!
//! Milestone 7-1 opens them read-only; editing, dirty tracking, save, and diagnostics are layered on
//! in later slices.
#![allow(dead_code)] // Editing/save/diagnostics consume the rest from Milestone 7-2+.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

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

/// One open WESL source buffer. `disk` is the last text read from (or written to) the file, so a
/// future editing slice can compare it against `text` for dirty state.
#[derive(Debug, Clone)]
struct WeslBuffer {
    relative_path: PathBuf,
    text: String,
    disk: String,
    revision: u64,
}

/// In-memory registry of open WESL source buffers, keyed by [`WeslSourceId`].
#[derive(Resource, Debug, Default)]
pub(crate) struct WeslDocuments {
    buffers: BTreeMap<WeslSourceId, WeslBuffer>,
}

impl WeslDocuments {
    /// Opens (or refreshes) a WESL buffer for `relative_path`, returning its id. Opening the same
    /// file again refreshes the on-disk baseline without discarding an existing buffer identity.
    pub(crate) fn open(&mut self, relative_path: PathBuf, text: String) -> WeslSourceId {
        let id = WeslSourceId::for_relative_path(&relative_path);
        self.buffers
            .entry(id)
            .and_modify(|buffer| buffer.disk = text.clone())
            .or_insert_with(|| WeslBuffer {
                relative_path,
                text: text.clone(),
                disk: text,
                revision: 0,
            });
        id
    }

    pub(crate) fn text(&self, id: WeslSourceId) -> Option<&str> {
        self.buffers.get(&id).map(|buffer| buffer.text.as_str())
    }

    pub(crate) fn relative_path(&self, id: WeslSourceId) -> Option<&Path> {
        self.buffers
            .get(&id)
            .map(|buffer| buffer.relative_path.as_path())
    }

    pub(crate) fn is_open(&self, id: WeslSourceId) -> bool {
        self.buffers.contains_key(&id)
    }

    /// Read-only in Milestone 7-1, so a buffer is never dirty yet.
    pub(crate) fn is_dirty(&self, id: WeslSourceId) -> bool {
        self.buffers
            .get(&id)
            .is_some_and(|buffer| buffer.text != buffer.disk)
    }

    pub(crate) fn close(&mut self, id: WeslSourceId) {
        self.buffers.remove(&id);
    }

    pub(crate) fn len(&self) -> usize {
        self.buffers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_relative_path_yields_a_stable_id_across_separators() {
        let a = WeslSourceId::for_relative_path(Path::new("shaders/noise.wesl"));
        let b = WeslSourceId::for_relative_path(Path::new("shaders\\noise.wesl"));
        assert_eq!(a, b);
        let other = WeslSourceId::for_relative_path(Path::new("shaders/other.wesl"));
        assert_ne!(a, other);
    }

    #[test]
    fn opening_registers_text_and_is_idempotent_by_path() {
        let mut documents = WeslDocuments::default();
        let id = documents.open(PathBuf::from("shaders/noise.wesl"), "fn main() {}".into());
        assert_eq!(documents.text(id), Some("fn main() {}"));
        assert_eq!(
            documents.relative_path(id),
            Some(Path::new("shaders/noise.wesl"))
        );
        assert!(!documents.is_dirty(id));
        // Reopening the same path returns the same id and refreshes the baseline.
        let again = documents.open(PathBuf::from("shaders/noise.wesl"), "fn main() { }".into());
        assert_eq!(again, id);
        assert_eq!(documents.len(), 1);
    }

    #[test]
    fn round_trips_through_ron_for_the_manifest() {
        let id = WeslSourceId::for_relative_path(Path::new("shaders/noise.wesl"));
        let encoded = ron::to_string(&id).unwrap();
        assert_eq!(ron::from_str::<WeslSourceId>(&encoded).unwrap(), id);
    }
}
