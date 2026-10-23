//! Real project files and the complete production layout registrar; no wall-clock sleeps.
use super::*;
use crate::{
    Localizer,
    feathers::node_graph::geometry::{
        GraphDocumentKey, GraphGeometryRegistry, GraphGeometryView, GraphViewKey,
    },
};

struct Fixture {
    root: tempfile::TempDir,
    program: MaterialProgram,
    app: App,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Persistence registrar").normalized();
        program
            .save_ron(root.path().join("test.aestra.material.ron"))
            .unwrap();
        let mut app = App::new();
        app.add_message::<AppExit>()
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .insert_resource(crate::test_support::session_with_timing_slack())
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<GraphViewportMemory>()
            .init_resource::<GraphGeometryRegistry>()
            .configure_sets(Startup, PersistenceSet::Startup)
            .configure_sets(
                Update,
                (
                    PersistenceSet::Actions,
                    PersistenceSet::Lifecycle,
                    EditorSet::MainUpdate,
                    EditorSet::UiRebuild,
                )
                    .chain(),
            );
        register(&mut app);
        Self { root, program, app }
    }
    fn path(&self) -> PathBuf {
        self.root.path().join(".aestra/editor-layout.ron")
    }
    fn baseline(&mut self) {
        self.app.update();
        save_material_graph_layout(
            &mut self
                .app
                .world_mut()
                .resource_mut::<MaterialGraphLayoutPersistence>(),
        );
    }
    fn edit(&mut self, position: Vec2) {
        self.app
            .world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_node(
                material_graph_view_key(self.program.id),
                MATERIAL_GRAPH_OUTPUT_NODE_KEY,
                position,
                true,
            );
        self.app.update();
    }
    fn output(&self) -> [f32; 2] {
        ProjectEditorLayout::load(self.root.path())
            .unwrap()
            .material_graphs[&self.program.id]
            .output
            .unwrap()
            .position
    }
}

#[test]
fn startup_restores_saved_layout_before_rebuild_without_semantic_edits() {
    let mut f = Fixture::new();
    let mut saved = ProjectEditorLayout::default();
    saved
        .material_graphs
        .entry(f.program.id)
        .or_default()
        .output = Some(MaterialGraphNodeLayout {
        position: [44.0, 88.0],
        collapsed: true,
        pinned: true,
    });
    saved.save(f.root.path()).unwrap();
    f.app.update();
    let memory = f.app.world().resource::<GraphViewportMemory>();
    let key = material_graph_view_key(f.program.id);
    assert_eq!(
        memory.node(&key, MATERIAL_GRAPH_OUTPUT_NODE_KEY),
        Some((Vec2::new(44.0, 88.0), true))
    );
    assert!(memory.is_pinned(&key, MATERIAL_GRAPH_OUTPUT_NODE_KEY));
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        0
    );
}

#[test]
fn actual_exit_flushes_pending_bases_not_temporary_offsets() {
    let mut f = Fixture::new();
    f.baseline();
    f.edit(Vec2::new(10.0, 20.0));
    f.app
        .world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_temporary_offset(
            &material_graph_view_key(f.program.id),
            MATERIAL_GRAPH_OUTPUT_NODE_KEY,
            Vec2::splat(900.0),
        );
    f.app.world_mut().write_message(AppExit::Success);
    f.app.update();
    assert_eq!(f.output(), [10.0, 20.0]);
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        0
    );
}

#[test]
fn debounce_saves_after_pending_deadline_without_requiring_another_edit() {
    let mut f = Fixture::new();
    f.baseline();
    f.edit(Vec2::new(70.0, 80.0));
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphLayoutPersistence>()
        .changed_at = Some(Instant::now() - Duration::from_secs(1));
    f.app.update();
    assert_eq!(f.output(), [70.0, 80.0]);
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphLayoutPersistence>()
            .changed_at
            .is_none()
    );
}

#[test]
fn external_corruption_blocks_exit_save_and_preserves_the_exact_file() {
    let mut f = Fixture::new();
    f.baseline();
    f.edit(Vec2::splat(99.0));
    let corrupt = "this is not a layout and must survive";
    std::fs::write(f.path(), corrupt).unwrap();
    f.app.world_mut().write_message(AppExit::Success);
    f.app.update();
    let persistence = f.app.world().resource::<MaterialGraphLayoutPersistence>();
    assert!(persistence.write_blocked && persistence.last_error.is_some());
    assert_eq!(std::fs::read_to_string(f.path()).unwrap(), corrupt);
}

#[test]
fn unsupported_startup_layout_stays_read_only_through_the_complete_registrar() {
    let mut f = Fixture::new();
    std::fs::create_dir_all(f.root.path().join(".aestra")).unwrap();
    let future = "(format_version: 999, material_graphs: {})";
    std::fs::write(f.path(), future).unwrap();
    f.app.update();
    f.edit(Vec2::splat(42.0));
    f.app.world_mut().write_message(AppExit::Success);
    f.app.update();
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphLayoutPersistence>()
            .write_blocked
    );
    assert_eq!(std::fs::read_to_string(f.path()).unwrap(), future);
}

#[test]
fn stable_saved_layout_does_not_dirty_persistence_on_idle_frames() {
    let mut f = Fixture::new();
    f.baseline();
    f.app.update();
    let before = f
        .app
        .world()
        .get_resource_ref::<MaterialGraphLayoutPersistence>()
        .unwrap()
        .last_changed();
    f.app.update();
    assert_eq!(
        f.app
            .world()
            .get_resource_ref::<MaterialGraphLayoutPersistence>()
            .unwrap()
            .last_changed(),
        before
    );
}

#[test]
fn clean_exit_does_not_reserialize_the_layout_file() {
    let mut f = Fixture::new();
    f.baseline();
    f.app.update();
    let bytes =
        std::fs::read_to_string(f.path()).unwrap() + "\n// preserve original bytes on clean exit\n";
    std::fs::write(f.path(), &bytes).unwrap();
    f.app.world_mut().write_message(AppExit::Success);
    f.app.update();
    assert_eq!(std::fs::read_to_string(f.path()).unwrap(), bytes);
}

#[test]
fn late_catalog_change_never_flushes_one_projects_layout_to_another() {
    let mut f = Fixture::new();
    let other = tempfile::tempdir().unwrap();
    f.baseline();
    let before = std::fs::read(f.path()).unwrap();
    f.edit(Vec2::new(25.0, 50.0));
    let path = other.path().to_owned();
    f.app.add_systems(
        Last,
        (move |mut commands: Commands, mut once: Local<bool>| {
            if !*once {
                commands.insert_resource(ProjectEffectCatalog::scan(&path));
                *once = true;
            }
        })
        .before(persist_material_graph_layout),
    );
    f.app.world_mut().write_message(AppExit::Success);
    f.app.update();
    assert_eq!(std::fs::read(f.path()).unwrap(), before);
    assert!(!other.path().join(".aestra/editor-layout.ron").exists());
}

#[test]
fn lifecycle_switch_retires_old_views_and_queued_transients_before_rebuild() {
    use crate::material_graph::MaterialGraphPaletteState;
    let mut f = Fixture::new();
    let other = tempfile::tempdir().unwrap();
    f.app.update();
    let project = f
        .app
        .world()
        .resource::<ProjectEffectCatalog>()
        .root()
        .to_owned();
    let old_view = f
        .app
        .world_mut()
        .spawn(GraphGeometryView {
            key: GraphViewKey {
                document: GraphDocumentKey {
                    project,
                    asset: crate::document::DocumentKey::MaterialProgram(f.program.id),
                },
                view: None,
            },
            nodes: BTreeSet::new(),
        })
        .id();
    f.app
        .insert_resource(MaterialGraphPaletteState::blocking_states_for_test().remove(0));
    let path = other.path().to_owned();
    f.app
        .add_systems(
            Update,
            (move |mut commands: Commands, mut once: Local<bool>| {
                if !*once {
                    commands.insert_resource(ProjectEffectCatalog::scan(&path));
                    *once = true;
                }
            })
            .in_set(PersistenceSet::Lifecycle),
        )
        .add_systems(
            Update,
            (move |views: Query<&GraphGeometryView>, palette: Res<MaterialGraphPaletteState>| {
                assert!(!views.contains(old_view));
                assert!(!palette.is_open());
            })
            .in_set(EditorSet::UiRebuild),
        );
    f.app.update();
    assert_eq!(
        f.app
            .world()
            .resource::<MaterialGraphLayoutPersistence>()
            .root
            .as_deref(),
        Some(f.app.world().resource::<ProjectEffectCatalog>().root())
    );
}
