use super::ProjectFileClassification;
use crate::{
    ProjectAssetDiagnostic, ProjectAssetDiagnosticCode, ProjectAssetId,
    ProjectAssetIndexAvailability, ProjectSourceId,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFileInfo {
    pub classification: ProjectFileClassification,
    /// Filled by ProjectContent; source discovery alone never invents semantic identities.
    pub semantic_asset: Option<ProjectAssetId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectSourceKind {
    Directory,
    File(ProjectFileInfo),
    /// Symlinks and Windows reparse points are shown but never traversed or parsed.
    Link,
    Other,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSourceMetadata {
    pub bytes: u64,
    pub modified: Option<SystemTime>,
    pub readonly: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSourceEntry {
    pub id: ProjectSourceId,
    pub parent: Option<ProjectSourceId>,
    pub path: PathBuf,
    pub relative_path: PathBuf,
    /// Native filename; convert lossily only for display, never filesystem operations.
    pub name: OsString,
    pub kind: ProjectSourceKind,
    pub metadata: Option<ProjectSourceMetadata>,
    /// A failed directory read is not an empty directory. Retain its row and the error.
    pub error: Option<String>,
}

/// One read-only discovery result shared by generic content and semantic indexing.
#[derive(Debug, Clone)]
pub struct ProjectSourceTree {
    root: PathBuf,
    root_id: ProjectSourceId,
    entries: BTreeMap<ProjectSourceId, ProjectSourceEntry>,
    paths: BTreeMap<PathBuf, ProjectSourceId>,
    children: BTreeMap<ProjectSourceId, Vec<ProjectSourceId>>,
    availability: ProjectAssetIndexAvailability,
    diagnostics: Vec<ProjectAssetDiagnostic>,
}

impl ProjectSourceTree {
    /// Validate an explicitly selected root before a host canonicalizes it (which loses link
    /// provenance). This checks the selected entry, not concurrent replacement or ancestor links.
    pub fn validate_root(root: impl AsRef<Path>) -> Result<(), String> {
        let metadata = fs::symlink_metadata(root.as_ref()).map_err(|error| error.to_string())?;
        if !metadata.is_dir() || is_link(&metadata) {
            return Err("Project root must be a directory, not a file or link".into());
        }
        Ok(())
    }

    /// The caller supplies the asset root, not a directory to search for a project root.
    /// This performs no writes and does not follow links, including a linked root.
    pub fn scan(root: impl AsRef<Path>) -> Self {
        Self::scan_with(root.as_ref(), &|path| fs::read_dir(path))
    }

    fn scan_with(root: &Path, read_dir: &dyn Fn(&Path) -> std::io::Result<fs::ReadDir>) -> Self {
        let root = root.to_owned();
        let mut tree = Self {
            root: root.clone(),
            root_id: crate::source_id(&root, &root),
            entries: BTreeMap::new(),
            paths: BTreeMap::new(),
            children: BTreeMap::new(),
            availability: ProjectAssetIndexAvailability::Ready,
            diagnostics: Vec::new(),
        };
        let mut ids = BTreeSet::new();
        tree.root_id = tree.discover(root.clone(), None, &mut ids, read_dir);
        if tree.entries[&tree.root_id].kind != ProjectSourceKind::Directory
            && tree.entries[&tree.root_id].error.is_none()
        {
            tree.fail(
                tree.root_id,
                "Project root must be a directory, not a file or link".into(),
            );
        }
        if let Some(message) = tree.entries[&tree.root_id].error.clone() {
            tree.availability = ProjectAssetIndexAvailability::Unavailable { root, message };
        }
        tree
    }

    pub fn root_path(&self) -> &Path {
        &self.root
    }
    pub fn root(&self) -> ProjectSourceId {
        self.root_id
    }
    pub fn availability(&self) -> &ProjectAssetIndexAvailability {
        &self.availability
    }
    pub fn diagnostics(&self) -> &[ProjectAssetDiagnostic] {
        &self.diagnostics
    }
    pub fn source(&self, id: ProjectSourceId) -> Option<&ProjectSourceEntry> {
        self.entries.get(&id)
    }
    /// Deterministic native relative-path order, independent of filesystem enumeration order.
    pub fn entries(&self) -> impl Iterator<Item = &ProjectSourceEntry> {
        self.paths.values().map(|id| &self.entries[id])
    }
    pub fn children(&self, parent: ProjectSourceId) -> impl Iterator<Item = &ProjectSourceEntry> {
        self.children
            .get(&parent)
            .into_iter()
            .flatten()
            .map(|id| &self.entries[id])
    }
    /// Exact relative lookup; no I/O, canonicalization, parent traversal or display-string matching.
    pub fn at_relative_path(&self, path: impl AsRef<Path>) -> Option<&ProjectSourceEntry> {
        self.paths
            .get(path.as_ref())
            .and_then(|id| self.entries.get(id))
    }
    pub(super) fn file_mut(&mut self, id: ProjectSourceId) -> Option<&mut ProjectFileInfo> {
        match &mut self.entries.get_mut(&id)?.kind {
            ProjectSourceKind::File(file) => Some(file),
            _ => None,
        }
    }

    fn discover(
        &mut self,
        path: PathBuf,
        parent: Option<ProjectSourceId>,
        ids: &mut BTreeSet<ProjectSourceId>,
        read_dir: &dyn Fn(&Path) -> std::io::Result<fs::ReadDir>,
    ) -> ProjectSourceId {
        let mut id = crate::source_id(&self.root, &path);
        while !ids.insert(id) {
            id.0 = id.0.wrapping_add(1);
        }
        let relative_path = path
            .strip_prefix(&self.root)
            .expect("discovery remains under root")
            .to_owned();
        let metadata = fs::symlink_metadata(&path);
        let kind = match &metadata {
            Ok(meta) if is_link(meta) => ProjectSourceKind::Link,
            Ok(meta) if meta.is_dir() => ProjectSourceKind::Directory,
            Ok(meta) if meta.is_file() => ProjectSourceKind::File(ProjectFileInfo {
                classification: ProjectFileClassification::for_path(&path),
                semantic_asset: None,
            }),
            Ok(_) => ProjectSourceKind::Other,
            Err(_) => ProjectSourceKind::Unavailable,
        };
        let is_directory = kind == ProjectSourceKind::Directory;
        self.paths.insert(relative_path.clone(), id);
        self.entries.insert(
            id,
            ProjectSourceEntry {
                id,
                parent,
                name: path.file_name().unwrap_or(path.as_os_str()).to_owned(),
                path: path.clone(),
                relative_path,
                kind,
                error: None,
                metadata: metadata.as_ref().ok().map(|meta| ProjectSourceMetadata {
                    bytes: meta.len(),
                    modified: meta.modified().ok(),
                    readonly: meta.permissions().readonly(),
                }),
            },
        );
        if let Err(error) = metadata {
            self.fail(id, error.to_string());
        }
        if is_directory {
            match read_dir(&path) {
                Ok(entries) => {
                    let mut paths = Vec::new();
                    for entry in entries {
                        match entry {
                            Ok(entry) if !excluded(&entry.file_name()) => paths.push(entry.path()),
                            Ok(_) => {}
                            Err(error) => self.fail(id, error.to_string()),
                        }
                    }
                    paths.sort();
                    let mut children = paths
                        .into_iter()
                        .map(|path| self.discover(path, Some(id), ids, read_dir))
                        .collect::<Vec<_>>();
                    children.sort_by(|a, b| {
                        let a = &self.entries[a];
                        let b = &self.entries[b];
                        (a.kind != ProjectSourceKind::Directory)
                            .cmp(&(b.kind != ProjectSourceKind::Directory))
                            .then_with(|| a.name.cmp(&b.name))
                    });
                    self.children.insert(id, children);
                }
                Err(error) => self.fail(id, error.to_string()),
            }
        }
        id
    }

    fn fail(&mut self, id: ProjectSourceId, message: String) {
        let source = self.entries.get_mut(&id).unwrap();
        source.error = Some(message.clone());
        self.diagnostics.push(ProjectAssetDiagnostic {
            code: ProjectAssetDiagnosticCode::SourceUnavailable,
            path: Some(source.path.clone()),
            message,
        });
    }
}

fn excluded(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        [".aestra", ".git", ".hg", ".svn"]
            .iter()
            .any(|hidden| name.eq_ignore_ascii_case(hidden))
    })
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Includes junctions, not just symbolic-link tags recognized by FileType.
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreadable_subtree_is_not_mistaken_for_an_empty_folder() {
        let temporary = tempfile::tempdir().unwrap();
        let denied = temporary.path().join("denied");
        fs::create_dir(&denied).unwrap();
        fs::write(temporary.path().join("visible.txt"), "visible").unwrap();
        let tree = ProjectSourceTree::scan_with(temporary.path(), &|path| {
            if path == denied {
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            } else {
                fs::read_dir(path)
            }
        });
        assert_eq!(tree.availability(), &ProjectAssetIndexAvailability::Ready);
        let source = tree.at_relative_path("denied").unwrap();
        assert_eq!(source.kind, ProjectSourceKind::Directory);
        assert!(source.error.is_some());
        assert_eq!(tree.children(source.id).count(), 0);
        assert!(tree.at_relative_path("visible.txt").is_some());
        assert_eq!(tree.diagnostics().len(), 1);
        let index = crate::ProjectAssetIndex::from_source_tree(&tree, false);
        assert_eq!(index.diagnostics(), tree.diagnostics());
    }

    #[test]
    fn root_read_failure_propagates_unavailability_to_the_index() {
        let temporary = tempfile::tempdir().unwrap();
        let tree = ProjectSourceTree::scan_with(temporary.path(), &|_| {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        });
        assert!(matches!(
            tree.availability(),
            ProjectAssetIndexAvailability::Unavailable { .. }
        ));
        assert_eq!(
            crate::ProjectAssetIndex::from_source_tree(&tree, true).availability(),
            tree.availability()
        );
    }
}
