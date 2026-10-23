use super::*;
use aestra_project::{
    ProjectContentVersion,
    content::operations::{AssetMoveBatchResult, SourceRelocation},
};
use std::{fs, path::Path};

const VERSION: ProjectContentVersion = ProjectContentVersion {
    generation: 1,
    revision: 1,
};
fn source(content: &ProjectContent, path: &str) -> ProjectSourceId {
    content
        .source_tree()
        .at_relative_path(Path::new(path))
        .unwrap()
        .id
}

#[test]
fn collections_are_cross_folder_filtered_deduplicated_and_recent_first() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("shaders")).unwrap();
    fs::write(root.path().join("shaders/a.wesl"), "").unwrap();
    fs::write(root.path().join("z.wesl"), "").unwrap();
    let content = ProjectContent::scan(root.path());
    let a = source(&content, "shaders/a.wesl");
    let z = source(&content, "z.wesl");
    let folder = source(&content, "shaders");
    let mut state = AssetBrowserState::default();
    state.reconcile(&content, VERSION);
    for id in [a, z, folder] {
        state.toggle_favorite(&content, id);
    }
    state.collection = BrowserCollection::Favorites;
    assert_eq!(state.filtered(&content).len(), 3);
    state.query = "a.wesl".into();
    assert_eq!(state.filtered(&content)[0].id, a);
    state.query.clear();
    state.toggle_favorite(&content, z);
    assert!(!state.is_favorite(&content, z));
    for id in [a, z, a] {
        state.record_recent(&content, id);
    }
    state.collection = BrowserCollection::Recent;
    assert_eq!(
        state
            .filtered(&content)
            .iter()
            .map(|e| e.id)
            .collect::<Vec<_>>(),
        [a, z]
    );
    state.locate(&content, a);
    assert_eq!(state.collection, BrowserCollection::Folder);
    assert_eq!(state.selected, Some(a));
    state.collection = BrowserCollection::Favorites;
    state.navigate(&content, folder); // Same directory must still leave the virtual collection.
    assert_eq!(state.collection, BrowserCollection::Folder);
}

#[test]
fn directory_relocation_remaps_generic_bookmarks_and_delete_restore_revives_them() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("old")).unwrap();
    fs::write(root.path().join("old/a.wesl"), "").unwrap();
    let before = ProjectContent::scan(root.path());
    let mut state = AssetBrowserState::default();
    state.reconcile(&before, VERSION);
    state.toggle_favorite(&before, source(&before, "old"));
    state.toggle_favorite(&before, source(&before, "old/a.wesl"));
    state.record_recent(&before, source(&before, "old/a.wesl"));
    fs::rename(root.path().join("old"), root.path().join("new")).unwrap();
    let after = ProjectContent::scan(root.path());
    state.reconcile_relocations(
        &before,
        &after,
        &AssetMoveBatchResult {
            moves: Vec::new(),
            folders: vec![SourceRelocation {
                source: root.path().join("old"),
                destination: root.path().join("new"),
                asset: None,
            }],
            journal: PathBuf::new(),
            rewritten_sources: Vec::new(),
        },
    );
    assert!(state.is_favorite(&after, source(&after, "new/a.wesl")));
    assert_eq!(state.recent[0].path, Path::new("new/a.wesl"));
    // Model recovery storage outside the scanned project, then an Undo restoring exact paths.
    let recovery = tempfile::tempdir().unwrap();
    fs::rename(root.path().join("new"), recovery.path().join("new")).unwrap();
    let deleted = ProjectContent::scan(root.path());
    state.reconcile(
        &deleted,
        ProjectContentVersion {
            revision: 2,
            ..VERSION
        },
    );
    state.collection = BrowserCollection::Favorites;
    assert!(state.filtered(&deleted).is_empty());
    assert_eq!(state.favorites.len(), 2);
    fs::rename(recovery.path().join("new"), root.path().join("new")).unwrap();
    let restored = ProjectContent::scan(root.path());
    state.reconcile(
        &restored,
        ProjectContentVersion {
            revision: 3,
            ..VERSION
        },
    );
    assert_eq!(state.filtered(&restored).len(), 2);
}

#[test]
fn typed_bookmarks_follow_unique_identity_but_not_replacement_or_ambiguous_moves() {
    let root = tempfile::tempdir().unwrap();
    let program = aestra_core::material::MaterialProgram::additive_sprite("Original").normalized();
    let old = root.path().join("old.aestra.material.ron");
    program.save_ron(&old).unwrap();
    let content = ProjectContent::scan(root.path());
    let item = Bookmark::new(&content, source(&content, "old.aestra.material.ron")).unwrap();
    fs::rename(&old, root.path().join("new.aestra.material.ron")).unwrap();
    aestra_core::material::MaterialProgram::additive_sprite("Replacement")
        .normalized()
        .save_ron(&old)
        .unwrap();
    let moved = ProjectContent::scan(root.path());
    assert_eq!(
        item.resolve(&moved).unwrap().relative_path,
        Path::new("new.aestra.material.ron")
    );
    program
        .save_ron(root.path().join("duplicate.aestra.material.ron"))
        .unwrap();
    assert!(item.resolve(&ProjectContent::scan(root.path())).is_none());
}

#[test]
fn shortcut_storage_is_bounded_and_root_changes_clear_it() {
    let root = tempfile::tempdir().unwrap();
    for i in 0..FAVORITES_LIMIT + 2 {
        fs::create_dir(root.path().join(format!("f{i}"))).unwrap();
    }
    let content = ProjectContent::scan(root.path());
    let mut state = AssetBrowserState::default();
    state.reconcile(&content, VERSION);
    for entry in content.source_tree().entries() {
        state.toggle_favorite(&content, entry.id);
        state.record_recent(&content, entry.id);
    }
    assert_eq!(state.favorites.len(), FAVORITES_LIMIT);
    assert_eq!(state.recent.len(), RECENT_LIMIT);
    let other = tempfile::tempdir().unwrap();
    state.reconcile(&ProjectContent::scan(other.path()), VERSION);
    assert!(state.favorites.is_empty() && state.recent.is_empty());
}

#[test]
fn successful_open_records_recent_but_failed_open_and_selection_do_not() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.wesl"), "fn foo() {}").unwrap();
    let mut app = super::super::tests::browser_app(root.path());
    app.insert_resource(WorkspaceLayout::default());
    let content = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .content()
        .clone();
    let id = source(&content, "a.wesl");
    app.world_mut().resource_mut::<AssetBrowserState>().selected = Some(id);
    app.update();
    assert!(
        app.world()
            .resource::<AssetBrowserState>()
            .recent
            .is_empty()
    );
    app.world_mut()
        .trigger(super::super::actions::OpenWeslSource {
            relative: "missing.wesl".into(),
            new_view: false,
        });
    app.world_mut().flush();
    assert!(
        app.world()
            .resource::<AssetBrowserState>()
            .recent
            .is_empty()
    );
    app.world_mut()
        .trigger(super::super::actions::OpenWeslSource {
            relative: "a.wesl".into(),
            new_view: false,
        });
    app.world_mut().flush();
    let state = app.world().resource::<AssetBrowserState>();
    assert_eq!(state.recent.len(), 1);
    assert_eq!(state.recent[0].path, Path::new("a.wesl"));
    app.world_mut()
        .trigger(super::super::actions::BrowserAction::ClearRecent);
    app.world_mut().flush();
    assert!(
        app.world()
            .resource::<AssetBrowserState>()
            .recent
            .is_empty()
    );
}

#[test]
fn favorites_actions_reuse_browser_rows_and_reject_stale_requests() {
    use super::super::{actions::BrowserAction, panel::BrowserRow};
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.wesl"), "").unwrap();
    fs::write(root.path().join("b.wesl"), "").unwrap();
    let mut app = super::super::tests::browser_app(root.path());
    let catalog = app.world().resource::<ProjectEffectCatalog>();
    let id = source(catalog.content(), "a.wesl");
    let version = catalog.content_revision();
    let effect = app.world().resource::<EditorSession>().effect.clone();
    app.world_mut().trigger(BrowserAction::ToggleFavorite(
        id,
        ProjectContentVersion {
            revision: version.revision.wrapping_add(1),
            ..version
        },
    ));
    assert!(
        app.world()
            .resource::<AssetBrowserState>()
            .favorites
            .is_empty()
    );
    app.world_mut()
        .trigger(BrowserAction::ToggleFavorite(id, version));
    app.world_mut()
        .trigger(BrowserAction::Collection(BrowserCollection::Favorites));
    app.update();
    let world = app.world_mut();
    let visible: Vec<_> = world
        .query::<(&BrowserRow, &Node)>()
        .iter(world)
        .filter(|(_, node)| node.display != Display::None)
        .map(|(row, _)| row.0)
        .collect();
    assert_eq!(visible, [id]);
    assert_eq!(world.resource::<EditorSession>().effect, effect);
    app.world_mut()
        .trigger(BrowserAction::ToggleFavorite(id, version));
    app.update();
    assert!(
        app.world()
            .resource::<AssetBrowserState>()
            .favorites
            .is_empty()
    );
    assert!(
        app.world()
            .resource::<AssetBrowserState>()
            .recent
            .is_empty()
    );
}
