//! Optional root-local UI preferences, separate from authored assets and graph layouts.
//! No source handles, document selection, or activation requests are serialized.
use super::state::*;
use crate::*;
use aestra_project::{ProjectContent, ProjectContentVersion};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

const FORMAT: u32 = 1;
const SAVE_DELAY: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct BrowserPreferences {
    format_version: u32,
    folder: PathBuf,
    expanded: BTreeSet<PathBuf>,
    view: ViewMode,
    sort: Sort,
    sources_visible: bool,
    sources_width: f32,
    query: String,
    recursive: bool,
    kinds: BTreeSet<Kind>,
}

impl Default for BrowserPreferences {
    fn default() -> Self {
        let state = AssetBrowserState::default();
        Self {
            format_version: FORMAT,
            folder: PathBuf::new(),
            expanded: BTreeSet::new(),
            view: state.view,
            sort: state.sort,
            sources_visible: state.sources_visible,
            sources_width: state.sources_width,
            query: String::new(),
            recursive: false,
            kinds: BTreeSet::new(),
        }
    }
}

fn safe_relative(path: &Path) -> bool {
    path.components()
        .all(|part| matches!(part, Component::Normal(_)))
        && !path.components().any(|part| part.as_os_str() == ".aestra")
}

impl BrowserPreferences {
    fn capture(state: &AssetBrowserState, content: &ProjectContent) -> Self {
        Self {
            format_version: FORMAT,
            folder: state.folder.clone(),
            expanded: state
                .expanded
                .iter()
                .filter_map(|id| content.source(*id).map(|entry| entry.relative_path.clone()))
                .collect(),
            view: state.view,
            sort: state.sort,
            sources_visible: state.sources_visible,
            sources_width: state.sources_width,
            query: state.query.clone(),
            recursive: state.recursive,
            kinds: state.kinds.clone(),
        }
    }

    fn restore(
        &self,
        state: &mut AssetBrowserState,
        content: &ProjectContent,
        version: ProjectContentVersion,
    ) {
        // Always start from defaults: a second project must not inherit the first one's filters.
        *state = AssetBrowserState::default();
        state.reconcile(content, version);
        let mut folder = if safe_relative(&self.folder) {
            self.folder.clone()
        } else {
            PathBuf::new()
        };
        while !folder.as_os_str().is_empty()
            && !content
                .source_tree()
                .at_relative_path(&folder)
                .is_some_and(|entry| Kind::of(entry) == Kind::Folder)
        {
            folder.pop();
        }
        state.folder = folder;
        state.expanded.extend(
            self.expanded
                .iter()
                .filter(|path| safe_relative(path))
                .filter_map(|path| content.source_tree().at_relative_path(path))
                .filter(|entry| Kind::of(entry) == Kind::Folder)
                .map(|entry| entry.id),
        );
        let mut ancestor = content.source(state.folder_id(content));
        while let Some(entry) = ancestor {
            state.expanded.insert(entry.id);
            ancestor = entry.parent.and_then(|id| content.source(id));
        }
        state.view = self.view;
        state.sort = self.sort;
        state.sources_visible = self.sources_visible;
        state.sources_width = if self.sources_width.is_finite() {
            self.sources_width.clamp(90.0, 360.0)
        } else {
            140.0
        };
        state.query = self.query.chars().take(4096).collect();
        state.recursive = self.recursive;
        state.kinds = self
            .kinds
            .iter()
            .copied()
            .filter(|kind| Kind::FILTERS.contains(kind))
            .collect();
    }

    fn path(root: &Path) -> PathBuf {
        root.join(aestra_project::PROJECT_EDITOR_LAYOUT_DIRECTORY)
            .join("asset-browser.ron")
    }

    fn load(root: &Path) -> io::Result<Self> {
        check_metadata_path(root)?;
        match fs::read_to_string(Self::path(root)) {
            Ok(source) => {
                let preferences: Self = ron::from_str(&source).map_err(io::Error::other)?;
                if preferences.format_version != FORMAT {
                    return Err(io::Error::other(
                        "unsupported asset-browser settings version",
                    ));
                }
                Ok(preferences)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    fn save(&self, root: &Path) -> io::Result<()> {
        // A disappearing project must not be recreated merely to save UI preferences.
        if !root.is_dir() {
            return Err(io::Error::other("asset root is unavailable"));
        }
        check_metadata_path(root)?;
        let path = Self::path(root);
        let parent = path.parent().expect("root-local metadata path");
        fs::create_dir_all(parent)?;
        let source = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(io::Error::other)?;
        let mut staged = tempfile::Builder::new()
            .prefix(".asset-browser-")
            .tempfile_in(parent)?;
        staged.write_all(source.as_bytes())?;
        staged.as_file().sync_all()?;
        staged
            .persist(path)
            .map_err(|error| error.error)?
            .sync_all()
    }
}

fn check_metadata_path(root: &Path) -> io::Result<()> {
    for path in [
        root.join(aestra_project::PROJECT_EDITOR_LAYOUT_DIRECTORY),
        BrowserPreferences::path(root),
    ] {
        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                #[cfg(windows)]
                let is_link = {
                    use std::os::windows::fs::MetadataExt;
                    metadata.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let is_link = metadata.is_symlink();
                if is_link {
                    return Err(io::Error::other("linked browser metadata is not supported"));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[derive(Resource, Default)]
pub(super) struct BrowserPersistence {
    root: Option<PathBuf>,
    current: BrowserPreferences,
    saved: BrowserPreferences,
    changed_at: Option<Instant>,
    writable: bool,
    last_error: Option<String>,
}

impl BrowserPersistence {
    pub(super) fn needs_restore(
        &self,
        content: &ProjectContent,
        state: &AssetBrowserState,
        version: ProjectContentVersion,
    ) -> bool {
        self.root.as_deref() != Some(content.source_tree().root_path())
            || state
                .version
                .is_some_and(|previous| previous.generation != version.generation)
    }

    pub(super) fn restore_root(
        &mut self,
        state: &mut AssetBrowserState,
        content: &ProjectContent,
        version: ProjectContentVersion,
    ) {
        let root = content.source_tree().root_path();
        if self.root.as_deref() == Some(root) {
            // A guarded reopen can publish a new generation of the same project.
            // Rebind relative locations from memory; don't erase its navigation or reread disk.
            let legacy = state.legacy;
            self.current.restore(state, content, version);
            state.legacy = legacy;
            return;
        }
        // Flush the cached old-root projection before accepting the new snapshot.
        self.flush();
        let (preferences, writable) = match BrowserPreferences::load(root) {
            Ok(preferences) => (preferences, true),
            Err(error) => {
                warn!(
                    "Asset Browser settings at {} are preserved without overwriting: {error}",
                    root.display()
                );
                (BrowserPreferences::default(), false)
            }
        };
        preferences.restore(state, content, version);
        self.root = Some(root.to_owned());
        self.current = BrowserPreferences::capture(state, content);
        self.saved = self.current.clone();
        self.changed_at = None;
        self.writable = writable;
        self.last_error = None;
    }

    fn capture(&mut self, state: &AssetBrowserState, content: &ProjectContent) {
        if self.root.as_deref() != Some(content.source_tree().root_path()) {
            return;
        }
        let next = BrowserPreferences::capture(state, content);
        if next != self.current {
            self.current = next;
            self.changed_at = Some(Instant::now());
        }
    }

    fn flush(&mut self) {
        if !self.writable || self.current == self.saved {
            return;
        }
        let Some(root) = &self.root else {
            return;
        };
        match self.current.save(root) {
            Ok(()) => {
                self.saved = self.current.clone();
                self.changed_at = None;
                self.last_error = None;
            }
            Err(error) => {
                let message = error.to_string();
                if self.last_error.as_ref() != Some(&message) {
                    warn!(
                        "Could not save Asset Browser settings at {}: {message}",
                        root.display()
                    );
                }
                self.last_error = Some(message);
                self.changed_at = Some(Instant::now());
            }
        }
    }
}

pub(super) fn persist_preferences(
    state: Res<AssetBrowserState>,
    catalog: Res<ProjectEffectCatalog>,
    mut persistence: ResMut<BrowserPersistence>,
    exits: Option<Res<Messages<AppExit>>>,
) {
    // Selection/hover can mark the state changed, but identical preferences never write.
    if state.is_changed() && state.scope == SourceScope::Project {
        persistence.capture(&state, catalog.content());
    }
    if exits.is_some_and(|exits| !exits.is_empty())
        || persistence
            .changed_at
            .is_some_and(|changed| changed.elapsed() >= SAVE_DELAY)
    {
        persistence.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERSION: ProjectContentVersion = ProjectContentVersion {
        generation: 1,
        revision: 1,
    };

    #[test]
    fn restart_restores_preferences_but_never_selection_history_or_inspection() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("textures/nested")).unwrap();
        let content = ProjectContent::scan(root.path());
        let mut state = AssetBrowserState::default();
        state.reconcile(&content, VERSION);
        let folder = content
            .source_tree()
            .at_relative_path(Path::new("textures/nested"))
            .unwrap()
            .id;
        state.navigate(&content, folder);
        state.view = ViewMode::Grid;
        state.sort = Sort::Type;
        state.query = "smoke".into();
        state.recursive = true;
        state.sources_visible = false;
        state.sources_width = 240.0;
        state.kinds.insert(Kind::Texture);
        state.selected = Some(folder);
        state.inspected = Some(folder);
        state.page = 3;
        let preferences = BrowserPreferences::capture(&state, &content);
        preferences.save(root.path()).unwrap();
        let loaded = BrowserPreferences::load(root.path()).unwrap();
        assert_eq!(loaded, preferences);
        let source = fs::read_to_string(BrowserPreferences::path(root.path())).unwrap();
        assert!(!source.contains("ProjectSourceId"));
        assert!(!source.contains("selected"));
        loaded.restore(&mut state, &content, VERSION);
        assert_eq!(BrowserPreferences::capture(&state, &content), preferences);
        assert!(state.selected.is_none() && state.inspected.is_none());
        assert!(state.back.is_empty() && state.forward.is_empty());
        assert_eq!(state.page, 0);
        // The existing graph layout is a separate file, never rewritten by the browser.
        assert!(!root.path().join(".aestra/editor-layout.ron").exists());
    }

    #[test]
    fn restore_validates_paths_and_clamps_dimensions() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("textures")).unwrap();
        fs::write(root.path().join("file.png"), []).unwrap();
        let content = ProjectContent::scan(root.path());
        let mut preferences = BrowserPreferences {
            folder: "textures/deleted/deeper".into(),
            expanded: ["textures", "missing", "file.png", "../outside"]
                .into_iter()
                .map(PathBuf::from)
                .collect(),
            sources_width: f32::NAN,
            ..default()
        };
        let mut state = AssetBrowserState::default();
        preferences.restore(&mut state, &content, VERSION);
        assert_eq!(state.folder, Path::new("textures"));
        assert_eq!(state.sources_width, 140.0);
        assert_eq!(state.expanded.len(), 2);
        for invalid in [
            "../outside",
            "/absolute",
            ".aestra",
            "textures/../../outside",
        ] {
            preferences.folder = invalid.into();
            preferences.sources_width = 10000.0;
            preferences.restore(&mut state, &content, VERSION);
            assert!(state.folder.as_os_str().is_empty());
            assert_eq!(state.sources_width, 360.0);
        }
        #[cfg(windows)]
        assert!(!safe_relative(Path::new(r"C:\outside")));
    }

    #[test]
    fn root_switch_flushes_old_project_and_restores_each_root_independently() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let a = ProjectContent::scan(first.path());
        let b = ProjectContent::scan(second.path());
        let mut state = AssetBrowserState::default();
        let mut persistence = BrowserPersistence::default();
        persistence.restore_root(&mut state, &a, VERSION);
        state.view = ViewMode::Grid;
        state.query = "first root".into();
        persistence.capture(&state, &a);
        persistence.restore_root(&mut state, &b, VERSION);
        assert_eq!(state.view, ViewMode::List);
        assert!(state.query.is_empty());
        assert_eq!(
            BrowserPreferences::load(first.path()).unwrap().query,
            "first root"
        );
        state.sources_width = 330.0;
        persistence.capture(&state, &b);
        persistence.restore_root(&mut state, &a, VERSION);
        assert_eq!(state.view, ViewMode::Grid);
        assert_eq!(state.query, "first root");
        assert_eq!(state.sources_width, 140.0);
        assert_eq!(
            BrowserPreferences::load(second.path())
                .unwrap()
                .sources_width,
            330.0
        );
    }

    #[test]
    fn malformed_future_and_unknown_settings_are_never_overwritten() {
        for source in [
            "not ron",
            "(format_version: 999)",
            "(unknown_future_field: true)",
        ] {
            let root = tempfile::tempdir().unwrap();
            fs::create_dir(root.path().join(".aestra")).unwrap();
            let path = BrowserPreferences::path(root.path());
            fs::write(&path, source).unwrap();
            let content = ProjectContent::scan(root.path());
            let mut state = AssetBrowserState::default();
            let mut persistence = BrowserPersistence::default();
            persistence.restore_root(&mut state, &content, VERSION);
            state.query = "change".into();
            persistence.capture(&state, &content);
            persistence.flush();
            assert_eq!(fs::read_to_string(path).unwrap(), source);
            assert!(!persistence.writable);
        }
    }

    #[test]
    fn idle_and_selection_do_not_write_and_failed_saves_can_retry() {
        let root = tempfile::tempdir().unwrap();
        let content = ProjectContent::scan(root.path());
        let mut state = AssetBrowserState::default();
        let mut persistence = BrowserPersistence::default();
        persistence.restore_root(&mut state, &content, VERSION);
        state.selected = Some(content.source_tree().root());
        persistence.capture(&state, &content);
        persistence.flush();
        assert!(!BrowserPreferences::path(root.path()).exists());
        state.query = "changed".into();
        persistence.capture(&state, &content);
        fs::write(root.path().join(".aestra"), "blocks directory creation").unwrap();
        persistence.flush();
        assert_ne!(persistence.current, persistence.saved);
        assert!(persistence.last_error.is_some());
        fs::remove_file(root.path().join(".aestra")).unwrap();
        persistence.flush();
        assert_eq!(persistence.current, persistence.saved);
        assert!(persistence.last_error.is_none());
        assert_eq!(
            fs::read_dir(root.path().join(".aestra")).unwrap().count(),
            1
        );
    }

    #[test]
    fn app_exit_flushes_latest_preferences_and_new_app_restores_them() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("textures")).unwrap();
        let mut app = super::super::tests::browser_app(root.path());
        app.add_message::<AppExit>();
        let original_effect = app.world().resource::<EditorSession>().effect.clone();
        {
            let mut state = app.world_mut().resource_mut::<AssetBrowserState>();
            state.folder = "textures".into();
            state.view = ViewMode::Grid;
            state.query = "latest edit".into();
            state.sources_width = 210.0;
        }
        // No sleep: the normal exit path must capture the final frame before flushing.
        app.world_mut().write_message(AppExit::Success);
        app.update();
        assert_eq!(
            BrowserPreferences::load(root.path()).unwrap().query,
            "latest edit"
        );
        drop(app);
        let mut restarted = super::super::tests::browser_app(root.path());
        let state = restarted.world().resource::<AssetBrowserState>();
        assert_eq!(state.folder, Path::new("textures"));
        assert_eq!(state.view, ViewMode::Grid);
        assert_eq!(state.sources_width, 210.0);
        assert!(state.selected.is_none());
        assert_eq!(
            restarted.world().resource::<EditorSession>().effect,
            original_effect
        );
        restarted.update();
        assert!(
            restarted
                .world()
                .resource::<BrowserPersistence>()
                .changed_at
                .is_none()
        );
    }

    #[test]
    fn plugin_root_switch_does_not_leak_filters_and_returns_to_saved_folder() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        fs::create_dir(first.path().join("textures")).unwrap();
        let mut app = super::super::tests::browser_app(first.path());
        {
            let mut state = app.world_mut().resource_mut::<AssetBrowserState>();
            state.folder = "textures".into();
            state.query = "first".into();
            state.recursive = true;
        }
        app.update();
        app.insert_resource(ProjectEffectCatalog::scan(second.path()));
        app.update();
        let state = app.world().resource::<AssetBrowserState>();
        assert!(state.query.is_empty() && !state.recursive);
        assert!(state.folder.as_os_str().is_empty());
        app.insert_resource(ProjectEffectCatalog::scan(first.path()));
        app.update();
        let state = app.world().resource::<AssetBrowserState>();
        assert_eq!(state.folder, Path::new("textures"));
        assert_eq!(state.query, "first");
        assert!(state.recursive);
        app.insert_resource(ProjectEffectCatalog::scan(first.path()));
        app.update();
        let state = app.world().resource::<AssetBrowserState>();
        assert_eq!(state.folder, Path::new("textures"));
        assert_eq!(state.query, "first");
    }
}
