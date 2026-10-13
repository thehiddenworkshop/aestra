use super::*;
use crate::{
    history::asset_order::{self, AssetOrder, Context},
    localization::Localizer,
    material_target::MaterialEditingTarget,
};
use aestra_core::material::{MaterialFunction, MaterialProgram};
use std::{fs, path::PathBuf};

#[derive(Resource, Default)]
struct Publications {
    prepared: Vec<(PathBuf, bool, aestra_project::ProjectContentVersion)>,
    finished: usize,
}

// Only browser/dialog callbacks are observed. Catalog refresh/publication and all I/O are real.
fn reconcile(world: &mut World, prepared: &ProjectEffectCatalog, relative: &Path, deleting: bool) {
    assert_eq!(
        prepared
            .content()
            .source_tree()
            .at_relative_path(relative)
            .is_some(),
        !deleting
    );
    world.resource_mut::<Publications>().prepared.push((
        relative.into(),
        deleting,
        prepared.content_revision(),
    ));
}
fn finish(world: &mut World, relative: &Path, deleting: bool) {
    let catalog = world.resource::<ProjectEffectCatalog>();
    let publications = world.resource::<Publications>();
    assert_eq!(
        publications.prepared.last(),
        Some(&(relative.into(), deleting, catalog.content_revision()))
    );
    world.resource_mut::<Publications>().finished += 1;
}
const PUBLICATION: Publication = Publication { reconcile, finish };

struct Fixture {
    root: tempfile::TempDir,
    world: World,
    program: MaterialProgram,
    outside: MaterialProgram,
    function: MaterialFunction,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("folder/empty")).unwrap();
        fs::write(root.path().join("folder/image.png"), b"exact image bytes").unwrap();
        let program = MaterialProgram::additive_sprite("Inside").normalized();
        let outside = MaterialProgram::additive_sprite("Outside").normalized();
        program
            .save_ron(root.path().join("folder/program.aestra.material.ron"))
            .unwrap();
        outside
            .save_ron(root.path().join("outside.aestra.material.ron"))
            .unwrap();
        let function = MaterialFunction::from_ron(include_str!(
            "../../../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
        ))
        .unwrap();
        function
            .save_ron(
                root.path()
                    .join("folder/function.aestra.material-function.ron"),
            )
            .unwrap();
        let mut world = World::new();
        world.insert_resource(ProjectEffectCatalog::scan(root.path()));
        world.insert_resource(crate::test_support::session_with_timing_slack());
        world.insert_resource(Localizer::new("en-US").unwrap());
        world.init_resource::<DocumentProtectionState>();
        world.init_resource::<AssetOrder>();
        world.init_resource::<Publications>();
        Self {
            root,
            world,
            program,
            outside,
            function,
        }
    }
    fn delete(&mut self) -> DeletedSource {
        self.delete_with_data(None)
    }
    fn delete_with_data(&mut self, data: Option<Vec<u8>>) -> DeletedSource {
        // Fixture setup uses the real plan/journal and closed-document inventory.
        // The initial Delete modal is outside this history-worker gate.
        let prepared = self.world.resource::<ProjectEffectCatalog>().clone();
        let session = self.world.resource::<EditorSession>();
        let source = prepared
            .content()
            .source_tree()
            .at_relative_path("folder")
            .unwrap()
            .id;
        assert!(!active_block(&prepared, session, source));
        let closed = ClosedDocuments::capture(&prepared, session, source).unwrap();
        let for_checks = closed.without_drafts(&prepared);
        let (drafts, complete) = deletion_draft_inventory(&for_checks, session);
        let item = prepared
            .content()
            .plan_delete_source(source, &drafts, complete)
            .unwrap()
            .with_recovery_data(data.unwrap_or_else(|| closed.encode().unwrap()))
            .apply()
            .unwrap();
        asset_order::record_delete(&mut self.world, item.clone());
        closed.close(&mut self.world);
        publish(&mut self.world, prepared, &item.original, true, PUBLICATION);
        item
    }
    fn revision(&self) -> u64 {
        self.world
            .resource::<AssetOrder>()
            .deletion_snapshot_for_test()
            .0
    }
    fn begin(&mut self, item: &DeletedSource, undo: bool, revision: u64) {
        step(&mut self.world, item.clone(), undo, revision, PUBLICATION);
        self.world.flush();
    }
    fn run(&mut self, item: &DeletedSource, undo: bool) {
        self.begin(item, undo, self.revision());
        if !io::idle_world(&self.world) {
            io::drain(&mut self.world);
        }
    }
    fn top(&self, undo: bool) -> DeletedSource {
        let (_, before, after) = self
            .world
            .resource::<AssetOrder>()
            .deletion_snapshot_for_test();
        if undo { before } else { after }.expect("expected filesystem history")
    }
    fn publications(&self) -> usize {
        let p = self.world.resource::<Publications>();
        assert_eq!(p.finished, p.prepared.len());
        p.finished
    }
    fn draft_inside(&mut self) {
        let mut program = self.program.clone();
        program.name = "Unsaved program".into();
        let mut function = self.function.clone();
        function.name = "Unsaved function".into();
        self.world
            .resource_mut::<ProjectEffectCatalog>()
            .replace_material_program(&self.program, &program)
            .unwrap();
        self.world
            .resource_mut::<ProjectEffectCatalog>()
            .replace_material_function(&self.function, &function)
            .unwrap();
        let drafts = self
            .world
            .resource::<ProjectEffectCatalog>()
            .material_drafts
            .clone();
        let target = MaterialEditingTarget::Program {
            root: self.root.path().into(),
            id: self.program.id,
        };
        let mut session = self.world.resource_mut::<EditorSession>();
        session.set_material_drafts(drafts);
        session.material_target = target;
        session.material_history_active = true;
    }
}

#[test]
fn actual_worker_round_trips_exact_files_empty_folders_catalog_and_fresh_redo_journals() {
    let mut f = Fixture::new();
    let mut item = f.delete();
    let effect = f.world.resource::<EditorSession>().effect.clone();
    let mut publications = f.publications();
    for _ in 0..3 {
        let revision = f.revision();
        f.run(&item, true);
        assert_eq!(f.revision(), revision + 1);
        assert!(f.root.path().join("folder/empty").is_dir());
        assert_eq!(
            fs::read(f.root.path().join("folder/image.png")).unwrap(),
            b"exact image bytes"
        );
        assert_eq!(
            MaterialProgram::load_ron(f.root.path().join("folder/program.aestra.material.ron"))
                .unwrap(),
            f.program
        );
        assert_eq!(f.publications(), publications + 1);
        let old = item.journal().to_owned();
        f.run(&item, false);
        item = f.top(true);
        assert_ne!(item.journal(), old);
        assert_eq!(f.revision(), revision + 2);
        assert!(!f.root.path().join("folder").exists());
        publications += 2;
        assert_eq!(f.publications(), publications);
    }
    assert_eq!(f.world.resource::<EditorSession>().effect, effect);
}

#[test]
fn prepared_completion_keeps_busy_files_order_and_publication_unchanged_until_applied() {
    let mut f = Fixture::new();
    let item = f.delete();
    let revision = f.revision();
    let publications = f.publications();
    f.begin(&item, true, revision);
    let mut completion = io::prepared_completion(&mut f.world);
    assert!(!io::idle_world(&f.world));
    assert_eq!(f.revision(), revision);
    assert!(!f.root.path().join("folder").exists());
    assert_eq!(f.publications(), publications);
    f.begin(&item, true, revision); // Busy is still authoritative after preparation.
    completion.apply(&mut f.world);
    assert!(io::idle_world(&f.world));
    assert_eq!(f.revision(), revision + 1);
    assert_eq!(f.publications(), publications + 1);
    assert!(f.root.path().join("folder/image.png").exists());
}

#[test]
fn stale_revision_wrong_direction_and_each_modal_branch_reject_before_queueing() {
    let mut f = Fixture::new();
    let item = f.delete();
    let revision = f.revision();
    let publications = f.publications();
    f.begin(&item, true, revision + 1);
    f.begin(&item, false, revision);
    for protection in DocumentProtectionState::blocking_states_for_test() {
        f.world.insert_resource(protection);
        f.begin(&item, true, revision);
        assert!(io::idle_world(&f.world));
        assert_eq!(f.revision(), revision);
        assert_eq!(f.publications(), publications);
        assert!(!f.root.path().join("folder").exists());
    }
    f.world.insert_resource(DocumentProtectionState::default());
    f.run(&item, true);
    assert!(f.root.path().join("folder").exists());
}

#[test]
fn completion_rechecks_document_target_catalog_order_and_modal_before_any_disk_write() {
    for change in 0..5 {
        let mut f = Fixture::new();
        let item = f.delete();
        let publications = f.publications();
        f.begin(&item, true, f.revision());
        let mut completion = io::prepared_completion(&mut f.world);
        match change {
            0 => {
                f.world
                    .resource_mut::<EditorSession>()
                    .set_effect_name("Changed during I/O");
            }
            1 => {
                f.world.resource_mut::<EditorSession>().material_target =
                    MaterialEditingTarget::Program {
                        root: f.root.path().into(),
                        id: f.outside.id,
                    };
            }
            2 => {
                f.world.resource_mut::<ProjectEffectCatalog>().refresh();
            }
            3 => {
                f.world
                    .resource_mut::<AssetOrder>()
                    .clear_context(&Context::Effect);
            }
            _ => {
                f.world
                    .resource_mut::<DocumentProtectionState>()
                    .asset_delete_open = true;
            }
        }
        let revision = f.revision();
        completion.apply(&mut f.world);
        assert!(io::idle_world(&f.world));
        assert_eq!(f.revision(), revision);
        assert_eq!(f.publications(), publications);
        assert!(!f.root.path().join("folder").exists());
        assert_eq!(f.top(true).journal(), item.journal());
        assert!(
            f.world
                .resource::<EditorSession>()
                .status
                .contains("cancelled")
        );
    }
}

#[test]
fn filesystem_conflicts_never_overwrite_and_keep_the_entry_retryable() {
    let mut f = Fixture::new();
    let item = f.delete();
    let revision = f.revision();
    let publications = f.publications();
    fs::create_dir(f.root.path().join("folder")).unwrap();
    fs::write(f.root.path().join("folder/external.txt"), b"keep external").unwrap();
    f.run(&item, true);
    assert_eq!(
        fs::read(f.root.path().join("folder/external.txt")).unwrap(),
        b"keep external"
    );
    assert!(!f.root.path().join("folder/image.png").exists());
    assert_eq!(f.revision(), revision);
    assert_eq!(f.publications(), publications);
    assert!(
        f.world
            .resource::<EditorSession>()
            .status
            .contains("blocked")
    );
    fs::remove_file(f.root.path().join("folder/external.txt")).unwrap();
    fs::remove_dir(f.root.path().join("folder")).unwrap();
    f.run(&item, true);
    assert!(f.root.path().join("folder/image.png").exists());
}

#[test]
fn redo_rechecks_exact_restored_bytes_before_deleting_and_keeps_external_edits() {
    let mut f = Fixture::new();
    let item = f.delete();
    f.run(&item, true);
    let revision = f.revision();
    let publications = f.publications();
    fs::write(f.root.path().join("folder/image.png"), b"external bytes").unwrap();
    f.run(&item, false);
    assert_eq!(
        fs::read(f.root.path().join("folder/image.png")).unwrap(),
        b"external bytes"
    );
    assert_eq!(f.revision(), revision);
    assert_eq!(f.publications(), publications);
    assert!(
        f.world
            .resource::<EditorSession>()
            .status
            .contains("blocked")
    );
}

#[test]
fn active_and_proposed_effect_references_block_redo_including_texture_subresources() {
    use aestra_authoring::{EffectCommand, EffectTransaction};
    for proposed in [false, true] {
        let mut f = Fixture::new();
        let item = f.delete();
        f.run(&item, true);
        let asset = aestra_core::AssetDefinition::texture("In use", "folder/image.png#Layer0");
        if proposed {
            f.world
                .resource_mut::<EditorSession>()
                .preview_transaction(EffectTransaction::single(
                    "Proposal",
                    EffectCommand::AddAsset { asset, index: 0 },
                ));
        } else {
            // Model a live unsaved owner without recording an unrelated global history item.
            f.world
                .resource_mut::<EditorSession>()
                .effect
                .assets
                .push(asset);
        }
        let revision = f.revision();
        let publications = f.publications();
        f.run(&item, false);
        assert!(io::idle_world(&f.world));
        assert_eq!(f.revision(), revision);
        assert_eq!(f.publications(), publications);
        assert!(f.root.path().join("folder/image.png").exists());
        assert!(
            f.world
                .resource::<EditorSession>()
                .status
                .contains("referenced")
        );
    }
}

#[test]
fn saved_surviving_owner_references_block_real_redo_planning() {
    let mut f = Fixture::new();
    let item = f.delete();
    f.run(&item, true);
    let mut owner = aestra_core::EffectAsset::new("Saved owner", 1.0);
    owner.assets.push(aestra_core::AssetDefinition::texture(
        "Used",
        "folder/image.png",
    ));
    owner
        .save_ron(f.root.path().join("owner.aestra.ron"))
        .unwrap();
    // The worker plans against the real disk tree, not only the published snapshot.
    let revision = f.revision();
    let publications = f.publications();
    f.run(&item, false);
    assert_eq!(f.revision(), revision);
    assert_eq!(f.publications(), publications);
    assert!(f.root.path().join("folder/image.png").exists());
    assert!(
        f.world
            .resource::<EditorSession>()
            .status
            .contains("blocked")
    );
}

#[test]
fn real_program_function_drafts_target_and_exact_disk_baselines_restore_together() {
    let mut f = Fixture::new();
    f.draft_inside();
    let drafts = f
        .world
        .resource::<ProjectEffectCatalog>()
        .material_drafts
        .clone();
    let target = f.world.resource::<EditorSession>().material_target.clone();
    let item = f.delete();
    assert!(
        f.world
            .resource::<ProjectEffectCatalog>()
            .material_drafts
            .is_empty()
    );
    assert_eq!(
        f.world.resource::<EditorSession>().material_target,
        MaterialEditingTarget::EffectInstance
    );
    f.run(&item, true);
    assert_eq!(
        f.world.resource::<ProjectEffectCatalog>().material_drafts,
        drafts
    );
    assert_eq!(f.world.resource::<EditorSession>().material_target, target);
    assert!(f.world.resource::<EditorSession>().material_history_active);
    assert_eq!(
        MaterialProgram::load_ron(f.root.path().join("folder/program.aestra.material.ron"))
            .unwrap(),
        f.program
    );
    f.run(&item, false);
    assert!(
        f.world
            .resource::<ProjectEffectCatalog>()
            .material_drafts
            .is_empty()
    );
    assert!(!f.root.path().join("folder").exists());
    let redone = f.top(true);
    f.run(&redone, true);
    assert_eq!(
        f.world.resource::<ProjectEffectCatalog>().material_drafts,
        drafts
    );
}

#[test]
fn editing_a_restored_draft_blocks_redo_without_losing_unsaved_work() {
    let mut f = Fixture::new();
    f.draft_inside();
    let item = f.delete();
    f.run(&item, true);
    let current = f
        .world
        .resource::<ProjectEffectCatalog>()
        .material_program(f.program.id)
        .unwrap();
    let mut newer = current.clone();
    newer.name = "Newer unsaved work".into();
    f.world
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(&current, &newer)
        .unwrap();
    let revision = f.revision();
    let publications = f.publications();
    f.run(&item, false);
    assert_eq!(
        f.world
            .resource::<ProjectEffectCatalog>()
            .material_program(f.program.id)
            .unwrap(),
        newer
    );
    assert_eq!(f.revision(), revision);
    assert_eq!(f.publications(), publications);
    assert!(
        f.root
            .path()
            .join("folder/program.aestra.material.ron")
            .exists()
    );
    assert!(
        f.world
            .resource::<EditorSession>()
            .status
            .contains("restored drafts changed")
    );
}

#[test]
fn conflicting_draft_restore_is_rejected_before_any_filesystem_work() {
    let mut f = Fixture::new();
    f.draft_inside();
    let drafts = f
        .world
        .resource::<ProjectEffectCatalog>()
        .material_drafts
        .clone();
    let item = f.delete();
    f.world
        .resource_mut::<ProjectEffectCatalog>()
        .material_drafts = drafts.clone();
    let revision = f.revision();
    f.run(&item, true);
    assert!(io::idle_world(&f.world));
    assert_eq!(f.revision(), revision);
    assert!(!f.root.path().join("folder").exists());
    assert_eq!(
        f.world.resource::<ProjectEffectCatalog>().material_drafts,
        drafts
    );
    assert!(
        f.world
            .resource::<EditorSession>()
            .status
            .contains("draft conflicts")
    );
}

#[test]
fn incomplete_unrelated_draft_inventory_blocks_restore_before_queueing() {
    let mut f = Fixture::new();
    let mut edited = f.outside.clone();
    edited.name = "Outside draft".into();
    f.world
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(&f.outside, &edited)
        .unwrap();
    let item = f.delete();
    f.world
        .resource_mut::<ProjectEffectCatalog>()
        .material_drafts
        .root = Some(f.root.path().join("another"));
    let revision = f.revision();
    f.run(&item, true);
    assert!(io::idle_world(&f.world));
    assert_eq!(f.revision(), revision);
    assert!(!f.root.path().join("folder").exists());
    assert!(
        f.world
            .resource::<EditorSession>()
            .status
            .contains("inventory is incomplete")
    );
}

#[test]
fn malformed_recovery_data_keeps_disk_and_history_unchanged() {
    let mut f = Fixture::new();
    let item = f.delete_with_data(Some(b"not a valid recovery document".to_vec()));
    let revision = f.revision();
    let publications = f.publications();
    f.run(&item, true);
    assert_eq!(f.revision(), revision);
    assert_eq!(f.publications(), publications);
    assert!(!f.root.path().join("folder").exists());
    assert!(io::idle_world(&f.world));
    assert!(
        f.world
            .resource::<EditorSession>()
            .status
            .contains("Invalid editor recovery")
    );
}

#[test]
fn deleted_active_effect_is_untitled_until_guarded_restore_rebinds_its_original_path() {
    let mut f = Fixture::new();
    let path = f.root.path().join("folder/current.aestra.ron");
    f.world
        .resource::<EditorSession>()
        .effect
        .save_ron(&path)
        .unwrap();
    f.world.resource_mut::<ProjectEffectCatalog>().refresh();
    f.world
        .resource_mut::<EditorSession>()
        .accept_external_source_path(path.clone());
    let item = f.delete();
    assert!(f.world.resource::<EditorSession>().source_path.is_none());
    let effect = f.world.resource::<EditorSession>().effect.clone();
    f.run(&item, true);
    assert_eq!(f.world.resource::<EditorSession>().source_path, Some(path));
    assert_eq!(f.world.resource::<EditorSession>().effect, effect);
    f.run(&item, false);
    assert!(f.world.resource::<EditorSession>().source_path.is_none());
    assert_eq!(f.world.resource::<EditorSession>().effect, effect);
}

#[test]
fn unrelated_drafts_survive_real_catalog_publication_and_both_history_directions() {
    let mut f = Fixture::new();
    let mut edited = f.outside.clone();
    edited.name = "Keep outside draft".into();
    f.world
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(&f.outside, &edited)
        .unwrap();
    let drafts = f
        .world
        .resource::<ProjectEffectCatalog>()
        .material_drafts
        .clone();
    let item = f.delete();
    f.run(&item, true);
    assert_eq!(
        f.world.resource::<ProjectEffectCatalog>().material_drafts,
        drafts
    );
    f.run(&item, false);
    assert_eq!(
        f.world.resource::<ProjectEffectCatalog>().material_drafts,
        drafts
    );
    assert_eq!(
        MaterialProgram::load_ron(f.root.path().join("outside.aestra.material.ron")).unwrap(),
        f.outside
    );
}

#[test]
fn application_router_drives_real_worker_between_document_undo_and_redo_steps() {
    use crate::history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory};
    fn dispatch(world: &mut World, item: DeletedSource, undo: bool, revision: u64) {
        step(world, item, undo, revision, PUBLICATION);
    }
    fn observe(
        action: On<HistoryAction>,
        routing: crate::history::routing::Routing,
        mut commands: Commands,
    ) {
        routing.execute(*action, &mut commands, dispatch);
    }
    let mut f = Fixture::new();
    f.delete();
    f.world.init_resource::<EditorHistoryLedger>();
    f.world.init_resource::<MaterialProgramEditHistory>();
    f.world
        .init_resource::<crate::material_function_editor::FunctionEditor>();
    f.world.add_observer(observe);
    let original = f.world.resource::<EditorSession>().effect.name.clone();
    f.world
        .resource_mut::<EditorSession>()
        .set_effect_name("After deletion");
    for (action, restored, name) in [
        (HistoryAction::Undo, false, original.as_str()),
        (HistoryAction::Undo, true, original.as_str()),
        (HistoryAction::Redo, false, original.as_str()),
        (HistoryAction::Redo, false, "After deletion"),
    ] {
        f.world.trigger(action);
        f.world.flush();
        if !io::idle_world(&f.world) {
            io::drain(&mut f.world);
        }
        assert_eq!(f.root.path().join("folder/image.png").exists(), restored);
        assert_eq!(f.world.resource::<EditorSession>().effect.name, name);
    }
    assert_eq!(f.publications(), 3);
}

#[test]
fn unsaved_surviving_program_dependencies_block_redo_and_preserve_the_draft() {
    use aestra_core::material::{MaterialExpressionKind, MaterialFunctionRef};
    let mut f = Fixture::new();
    let item = f.delete();
    f.run(&item, true);
    let mut edited = f.outside.clone();
    edited.expressions[0].kind = MaterialExpressionKind::FunctionCall {
        function: MaterialFunctionRef::Project(f.function.id),
        output: f.function.outputs[0].id,
        arguments: Default::default(),
    };
    f.world
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(&f.outside, &edited)
        .unwrap();
    let revision = f.revision();
    let publications = f.publications();
    f.run(&item, false);
    assert_eq!(f.revision(), revision);
    assert_eq!(f.publications(), publications);
    assert!(
        f.root
            .path()
            .join("folder/function.aestra.material-function.ron")
            .exists()
    );
    assert_eq!(
        f.world
            .resource::<ProjectEffectCatalog>()
            .material_program(f.outside.id)
            .unwrap(),
        edited
    );
    assert_eq!(
        MaterialProgram::load_ron(f.root.path().join("outside.aestra.material.ron")).unwrap(),
        f.outside
    );
    assert!(
        f.world
            .resource::<EditorSession>()
            .status
            .contains("blocked")
    );
}
