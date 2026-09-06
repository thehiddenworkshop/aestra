//! Synchronous worker-side discovery and pure reconciliation. Scheduling belongs to the host.
use super::{ProjectContent, ProjectSourceKind, ProjectSourceMetadata, ProjectSourceTree};
use crate::{ProjectAssetIndexAvailability, is_project_asset_source};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSourceStamp {
    pub kind: ProjectSourceKind,
    pub metadata: Option<ProjectSourceMetadata>,
    pub error: Option<String>,
    /// Change detection only, never an authorization to overwrite a file.
    pub fingerprint: Option<Result<u64, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTreeStamp {
    pub root: PathBuf,
    pub availability: ProjectAssetIndexAvailability,
    pub sources: BTreeMap<PathBuf, ProjectSourceStamp>,
}

impl ProjectTreeStamp {
    pub fn scan(root: &Path) -> Self {
        Self::from_tree(&ProjectSourceTree::scan(root), None, true)
    }

    fn from_tree(tree: &ProjectSourceTree, previous: Option<&Self>, force: bool) -> Self {
        let sources = tree
            .entries()
            .map(|source| {
                let mut stamp = ProjectSourceStamp {
                    kind: source.kind.clone(),
                    metadata: source.metadata.clone(),
                    error: source.error.clone(),
                    fingerprint: None,
                };
                // The observation describes disk discovery, not the subsequently parsed semantic join.
                if let ProjectSourceKind::File(file) = &mut stamp.kind {
                    file.semantic_asset = None;
                    file.classification = super::ProjectFileClassification::for_path(&source.path);
                    let cached = previous
                        .and_then(|previous| previous.sources.get(&source.path))
                        .filter(|old| {
                            !force
                                && old.metadata == stamp.metadata
                                && old.kind == stamp.kind
                                && old.error == stamp.error
                                && matches!(old.fingerprint, Some(Ok(_)))
                        });
                    stamp.fingerprint = cached
                        .and_then(|old| old.fingerprint.clone())
                        .or_else(|| Some(fingerprint(&source.path)));
                }
                (source.path.clone(), stamp)
            })
            .collect();
        Self {
            root: tree.root_path().to_owned(),
            availability: tree.availability().clone(),
            sources,
        }
    }

    pub fn file(&self, path: &Path) -> Option<&ProjectSourceStamp> {
        self.sources
            .get(path)
            .or_else(|| {
                self.sources
                    .iter()
                    .find(|(candidate, _)| same_project_source_location(candidate, path))
                    .map(|(_, stamp)| stamp)
            })
            .filter(|source| matches!(source.kind, ProjectSourceKind::File(_)))
    }

    pub fn changes_from(&self, previous: &Self) -> ProjectContentChanges {
        let added = self
            .sources
            .keys()
            .filter(|path| !previous.sources.contains_key(*path))
            .cloned()
            .collect();
        let removed = previous
            .sources
            .keys()
            .filter(|path| !self.sources.contains_key(*path))
            .cloned()
            .collect();
        let modified = self
            .sources
            .iter()
            .filter(|(path, stamp)| previous.sources.get(*path).is_some_and(|old| old != *stamp))
            .map(|(path, _)| path.clone())
            .collect();
        let availability_changed = self.root != previous.root
            || self.availability != previous.availability
            || self.sources.iter().any(|(path, source)| {
                previous
                    .sources
                    .get(path)
                    .and_then(|old| old.error.as_ref())
                    != source.error.as_ref()
            })
            || previous
                .sources
                .iter()
                .any(|(path, old)| old.error.is_some() && !self.sources.contains_key(path));
        let mut changes = ProjectContentChanges {
            added,
            removed,
            modified,
            availability_changed,
            semantic_changed: availability_changed,
        };
        let semantic_source_changed = changes.paths().any(|path| is_project_asset_source(path));
        changes.semantic_changed |= semantic_source_changed;
        changes
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectContentChanges {
    pub added: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
    pub modified: Vec<PathBuf>,
    pub availability_changed: bool,
    pub semantic_changed: bool,
}

impl ProjectContentChanges {
    pub fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.added.iter().chain(&self.removed).chain(&self.modified)
    }
    pub fn is_empty(&self) -> bool {
        !self.availability_changed && self.paths().next().is_none()
    }
}

#[derive(Debug, Clone)]
pub struct ProjectContentSnapshot {
    pub content: Arc<ProjectContent>,
    pub stamp: ProjectTreeStamp,
}

impl ProjectContentSnapshot {
    pub fn scan(root: &Path) -> Self {
        let tree = ProjectSourceTree::scan(root);
        let stamp = ProjectTreeStamp::from_tree(&tree, None, true);
        Self {
            content: Arc::new(ProjectContent::from_source_tree(tree)),
            stamp,
        }
    }

    /// Worker-only polling. Reuse unchanged file fingerprints; explicit refresh hashes all files.
    /// A changed candidate is checked again after parsing. An unsettled scan is never published.
    pub fn poll(&self, force: bool) -> Option<Self> {
        let tree = ProjectSourceTree::scan(&self.stamp.root);
        let mut stamp = ProjectTreeStamp::from_tree(&tree, Some(&self.stamp), force);
        if stamp == self.stamp {
            return Some(self.clone());
        }
        // Parsing the index reads semantic files again. Reconcile every fingerprint whenever
        // building a candidate, including same-size edits masked by coarse timestamps.
        stamp = ProjectTreeStamp::from_tree(&tree, None, true);
        let content = Arc::new(ProjectContent::from_source_tree(tree));
        let after = ProjectTreeStamp::from_tree(
            &ProjectSourceTree::scan(&self.stamp.root),
            Some(&self.stamp),
            true,
        );
        (after == stamp).then_some(Self { content, stamp })
    }
}

/// A root/session generation plus a published-content/internal-write revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectContentVersion {
    pub generation: u64,
    pub revision: u64,
}

#[derive(Debug, Clone)]
pub struct ProjectContentRefresh {
    version: ProjectContentVersion,
    committed: ProjectTreeStamp,
    pending: Option<ProjectTreeStamp>,
}

impl ProjectContentRefresh {
    pub fn new(version: ProjectContentVersion, committed: ProjectTreeStamp) -> Self {
        Self {
            version,
            committed,
            pending: None,
        }
    }
    pub fn version(&self) -> ProjectContentVersion {
        self.version
    }
    pub fn reset(&mut self, version: ProjectContentVersion, committed: ProjectTreeStamp) {
        *self = Self::new(version, committed);
    }
    pub fn unsettled(&mut self) {
        self.pending = None;
    }

    /// Two consecutive equal observations are required. No clocks, sleeping, or I/O here.
    /// The host resets to a new version only after atomically installing the accepted snapshot.
    pub fn observe(
        &mut self,
        version: ProjectContentVersion,
        current: &ProjectTreeStamp,
    ) -> Option<ProjectContentChanges> {
        if version != self.version || current.root != self.committed.root {
            return None;
        }
        if current == &self.committed {
            self.pending = None;
            return None;
        }
        if self.pending.as_ref() != Some(current) {
            self.pending = Some(current.clone());
            return None;
        }
        let changes = current.changes_from(&self.committed);
        self.committed = current.clone();
        self.pending = None;
        Some(changes)
    }
}

fn fingerprint(path: &Path) -> Result<u64, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hash = 0xcbf29ce484222325_u64;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            return Ok(hash);
        }
        for byte in &buffer[..count] {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
}

/// Pure lexical lookup compatibility, not filesystem identity or mutation containment. Windows
/// dialogs and canonical project roots may differ in ASCII case and verbatim prefix. Preserve
/// native code units (including unpaired surrogates); never match through lossy display strings.
pub fn same_project_source_location(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        fn key(path: &Path) -> Vec<u16> {
            let mut units = path
                .as_os_str()
                .encode_wide()
                .map(|unit| match unit {
                    65..=90 => unit + 32,
                    47 => 92,
                    _ => unit,
                })
                .collect::<Vec<_>>();
            if units.starts_with(&[92, 92, 63, 92, 117, 110, 99, 92]) {
                units.splice(..8, [92, 92]);
            } else if units.starts_with(&[92, 92, 63, 92]) {
                units.drain(..4);
            }
            units
        }
        left == right || key(left) == key(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}
