//! Snapshot-only browsing. Source IDs are UI identities, never authored dependencies.
use aestra_project::{
    ProjectContent, ProjectContentVersion, ProjectFileClassification, ProjectSourceEntry,
    ProjectSourceId, ProjectSourceKind,
};
use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};

pub(super) const PAGE_SIZE: usize = 96;
pub(super) const HISTORY_LIMIT: usize = 64;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum ViewMode {
    #[default]
    List,
    Grid,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Sort {
    #[default]
    Name,
    Type,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum InspectionTab {
    #[default]
    Details,
    Dependencies,
    Usages,
    Preflight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) enum Kind {
    Folder,
    Effect,
    Material,
    Function,
    Preset,
    Texture,
    Mesh,
    Shader,
    File,
    Link,
    Unavailable,
}

impl Kind {
    pub(super) const FILTERS: [Self; 10] = [
        Self::Effect,
        Self::Material,
        Self::Function,
        Self::Preset,
        Self::Texture,
        Self::Mesh,
        Self::Shader,
        Self::File,
        Self::Link,
        Self::Unavailable,
    ];

    pub(super) fn of(entry: &ProjectSourceEntry) -> Self {
        match &entry.kind {
            ProjectSourceKind::Directory => Self::Folder,
            ProjectSourceKind::Link => Self::Link,
            ProjectSourceKind::Unavailable => Self::Unavailable,
            ProjectSourceKind::Other => Self::File,
            ProjectSourceKind::File(file) => match file.classification {
                ProjectFileClassification::Effect => Self::Effect,
                ProjectFileClassification::MaterialProgram => Self::Material,
                ProjectFileClassification::MaterialFunction => Self::Function,
                ProjectFileClassification::MaterialPreset => Self::Preset,
                ProjectFileClassification::Texture => Self::Texture,
                ProjectFileClassification::Mesh => Self::Mesh,
                ProjectFileClassification::Shader => Self::Shader,
                ProjectFileClassification::Generic => Self::File,
            },
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Folder => "browser-kind-folder",
            Self::Effect => "browser-kind-effect",
            Self::Material => "browser-kind-material",
            Self::Function => "browser-kind-function",
            Self::Preset => "browser-kind-preset",
            Self::Texture => "browser-kind-texture",
            Self::Mesh => "browser-kind-mesh",
            Self::Shader => "browser-kind-shader",
            Self::File => "browser-kind-file",
            Self::Link => "browser-kind-link",
            Self::Unavailable => "browser-kind-unavailable",
        }
    }

    pub(super) fn icon(self) -> &'static str {
        match self {
            Self::Folder => "icons/folder.svg",
            Self::Texture => "icons/image.svg",
            Self::Effect => "icons/event.svg",
            Self::Material | Self::Preset => "icons/solid.svg",
            Self::Function | Self::Shader => "icons/source-curve.svg",
            Self::Mesh => "icons/wireframe.svg",
            _ => "icons/file.svg",
        }
    }
}

#[derive(Resource, Debug, Clone, PartialEq)]
pub(crate) struct AssetBrowserState {
    pub(super) legacy: bool,
    pub(super) view: ViewMode,
    pub(super) sort: Sort,
    pub(super) sources_visible: bool,
    pub(super) sources_width: f32,
    pub(super) query: String,
    pub(super) recursive: bool,
    pub(super) kinds: BTreeSet<Kind>,
    pub(super) selected: Option<ProjectSourceId>,
    pub(super) inspected: Option<ProjectSourceId>,
    pub(super) inspection_tab: InspectionTab,
    pub(super) inspection_page: usize,
    pub(super) locate_revision: u64,
    pub(super) page: usize,
    pub(super) tree_page: usize,
    pub(super) folder: PathBuf,
    pub(super) back: Vec<PathBuf>,
    pub(super) forward: Vec<PathBuf>,
    pub(super) expanded: BTreeSet<ProjectSourceId>,
    pub(super) version: Option<ProjectContentVersion>,
    root: PathBuf,
}

impl Default for AssetBrowserState {
    fn default() -> Self {
        Self {
            legacy: false,
            view: ViewMode::List,
            sort: Sort::Name,
            sources_visible: true,
            sources_width: 140.0,
            query: String::new(),
            recursive: false,
            kinds: BTreeSet::new(),
            selected: None,
            inspected: None,
            inspection_tab: InspectionTab::Details,
            inspection_page: 0,
            locate_revision: 0,
            page: 0,
            tree_page: 0,
            folder: PathBuf::new(),
            back: Vec::new(),
            forward: Vec::new(),
            expanded: BTreeSet::new(),
            version: None,
            root: PathBuf::new(),
        }
    }
}

impl AssetBrowserState {
    /// Retarget browsing history and inspection before the old path-derived IDs vanish.
    pub(super) fn reconcile_relocations(
        &mut self,
        before: &ProjectContent,
        after: &ProjectContent,
        result: &aestra_project::content::operations::AssetMoveBatchResult,
    ) {
        let map = |path: &std::path::Path| -> PathBuf {
            let absolute = before.source_tree().root_path().join(path);
            let relocated = result
                .folders
                .iter()
                .find_map(|item| {
                    absolute
                        .strip_prefix(&item.source)
                        .ok()
                        .map(|tail| item.destination.join(tail))
                })
                .or_else(|| {
                    result
                        .moves
                        .iter()
                        .find(|item| item.source == absolute)
                        .map(|item| item.destination.clone())
                })
                .unwrap_or(absolute);
            relocated
                .strip_prefix(after.source_tree().root_path())
                .unwrap_or(path)
                .to_owned()
        };
        let map_id = |id| {
            before
                .source(id)
                .and_then(|entry| {
                    after
                        .source_tree()
                        .at_relative_path(map(&entry.relative_path))
                })
                .map(|entry| entry.id)
        };
        self.folder = map(&self.folder);
        self.back = self.back.iter().map(|path| map(path)).collect();
        self.forward = self.forward.iter().map(|path| map(path)).collect();
        self.expanded = self.expanded.iter().filter_map(|id| map_id(*id)).collect();
        self.selected = self.selected.and_then(map_id);
        self.inspected = self.inspected.and_then(map_id);
    }

    pub(super) fn reconcile(&mut self, content: &ProjectContent, version: ProjectContentVersion) {
        if self.version == Some(version) && self.root == content.source_tree().root_path() {
            return;
        }
        if self.root != content.source_tree().root_path()
            || self
                .version
                .is_some_and(|previous| previous.generation != version.generation)
        {
            self.folder.clear();
            self.back.clear();
            self.forward.clear();
            self.expanded.clear();
            self.query.clear();
            self.kinds.clear();
            self.selected = None;
            self.page = 0;
            self.tree_page = 0;
            self.inspection_page = 0;
            self.inspected = None;
        }
        self.root = content.source_tree().root_path().to_owned();
        self.version = Some(version);
        while !self.folder.as_os_str().is_empty() && !is_folder(content, &self.folder) {
            self.folder.pop();
            self.selected = None;
            self.page = 0;
        }
        self.back.retain(|path| is_folder(content, path));
        self.forward.retain(|path| is_folder(content, path));
        self.expanded.retain(|id| {
            content
                .source(*id)
                .is_some_and(|e| Kind::of(e) == Kind::Folder)
        });
        self.expanded.insert(content.source_tree().root());
        if self.selected.is_some_and(|id| content.source(id).is_none()) {
            self.selected = None;
        }
    }

    pub(super) fn folder_id(&self, content: &ProjectContent) -> ProjectSourceId {
        content
            .source_tree()
            .at_relative_path(&self.folder)
            .map_or(content.source_tree().root(), |e| e.id)
    }

    pub(super) fn navigate(&mut self, content: &ProjectContent, id: ProjectSourceId) {
        let Some(entry) = content.source(id).filter(|e| Kind::of(e) == Kind::Folder) else {
            return;
        };
        if self.folder == entry.relative_path {
            return;
        }
        self.back.push(self.folder.clone());
        if self.back.len() > HISTORY_LIMIT {
            self.back.remove(0);
        }
        self.forward.clear();
        self.set_folder(content, entry.relative_path.clone());
    }

    pub(super) fn history(&mut self, content: &ProjectContent, forward: bool) {
        let path = if forward {
            self.forward.pop()
        } else {
            self.back.pop()
        };
        if let Some(path) = path {
            if forward {
                self.back.push(self.folder.clone());
            } else {
                self.forward.push(self.folder.clone());
            }
            self.set_folder(content, path);
        }
    }

    /// Reveal a source independently from the active effect/emitter document.
    pub(super) fn locate(&mut self, content: &ProjectContent, id: ProjectSourceId) -> bool {
        let Some(entry) = content.source(id) else {
            return false;
        };
        let folder = entry.parent.unwrap_or(content.source_tree().root());
        self.navigate(content, folder);
        self.query.clear();
        self.kinds.clear();
        self.recursive = false;
        self.sources_visible = true;
        let mut ancestor = content.source(folder);
        while let Some(entry) = ancestor {
            self.expanded.insert(entry.id);
            ancestor = entry.parent.and_then(|id| content.source(id));
        }
        self.page = self
            .filtered(content)
            .iter()
            .position(|entry| entry.id == id)
            .unwrap_or(0)
            / PAGE_SIZE;
        self.tree_page = self
            .folders(content)
            .iter()
            .position(|(id, _)| *id == folder)
            .unwrap_or(0)
            / PAGE_SIZE;
        self.selected = (id != content.source_tree().root()).then_some(id);
        self.locate_revision = self.locate_revision.wrapping_add(1);
        true
    }

    fn set_folder(&mut self, content: &ProjectContent, path: PathBuf) {
        self.folder = path;
        self.page = 0;
        self.tree_page = 0;
        self.selected = None;
        let mut current = content.source(self.folder_id(content));
        while let Some(entry) = current {
            self.expanded.insert(entry.id);
            current = entry.parent.and_then(|id| content.source(id));
        }
    }

    pub(super) fn filtered<'a>(&self, content: &'a ProjectContent) -> Vec<&'a ProjectSourceEntry> {
        let folder = self.folder_id(content);
        let query = self.query.trim().to_lowercase();
        let mut entries = content
            .source_tree()
            .entries()
            .filter(|entry| {
                let in_scope = if self.recursive {
                    entry.id != folder && entry.relative_path.starts_with(&self.folder)
                } else {
                    entry.parent == Some(folder)
                };
                let kind = Kind::of(entry);
                in_scope
                    && (self.kinds.is_empty() || kind == Kind::Folder || self.kinds.contains(&kind))
                    && (query.is_empty()
                        || entry.name.to_string_lossy().to_lowercase().contains(&query))
            })
            .collect::<Vec<_>>();
        entries.sort_by_cached_key(|entry| {
            (
                Kind::of(entry) != Kind::Folder,
                (self.sort == Sort::Type).then(|| Kind::of(entry)),
                entry.name.to_string_lossy().to_lowercase(),
                entry.relative_path.clone(),
            )
        });
        entries
    }

    /// Only expanded directories are projected, and the UI pages this list before spawning.
    pub(super) fn folders(&self, content: &ProjectContent) -> Vec<(ProjectSourceId, usize)> {
        let mut result = Vec::new();
        let mut pending = vec![(content.source_tree().root(), 0)];
        while let Some((id, depth)) = pending.pop() {
            result.push((id, depth));
            if self.expanded.contains(&id) {
                let children = content
                    .source_tree()
                    .children(id)
                    .filter(|e| Kind::of(e) == Kind::Folder)
                    .map(|e| (e.id, depth + 1))
                    .collect::<Vec<_>>();
                pending.extend(children.into_iter().rev());
            }
        }
        result
    }
}

fn is_folder(content: &ProjectContent, path: &std::path::Path) -> bool {
    content
        .source_tree()
        .at_relative_path(path)
        .is_some_and(|e| Kind::of(e) == Kind::Folder)
}
