use super::*;
use crate::history::{HistoryAction, document_execution::execute_document_history};
use crate::material_function_editor::FunctionEditor;
use crate::material_target::MaterialEditingTarget;
use aestra_core::{
    MaterialExpressionId, MaterialFunctionId, MaterialFunctionOutputId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialFunctionOutput,
        MaterialProgram, MaterialSchemaVersion, MaterialValue, MaterialValueType,
    },
};
use bevy::ecs::system::RunSystemOnce;
use std::fs;

struct Fixture {
    root: tempfile::TempDir,
    world: World,
    programs: [MaterialProgram; 2],
    function: MaterialFunction,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let programs = [
            MaterialProgram::additive_sprite("First").normalized(),
            MaterialProgram::additive_sprite("Second").normalized(),
        ];
        for (index, program) in programs.iter().enumerate() {
            program
                .save_ron(
                    root.path()
                        .join(format!("program-{index}.aestra.material.ron")),
                )
                .unwrap();
        }
        let expression = MaterialExpressionId::new();
        let function = MaterialFunction {
            schema_version: MaterialSchemaVersion::CURRENT,
            id: MaterialFunctionId::new(),
            name: "Function".into(),
            inputs: vec![],
            outputs: vec![MaterialFunctionOutput {
                id: MaterialFunctionOutputId::new(),
                name: "Value".into(),
                value_type: MaterialValueType::Float,
                expression,
            }],
            expressions: vec![MaterialExpression {
                id: expression,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
            }],
            custom_wesl: None,
        };
        function
            .save_ron(root.path().join("function.aestra.material-function.ron"))
            .unwrap();
        let mut world = World::new();
        world.insert_resource(ProjectEffectCatalog::scan(root.path()));
        world.insert_resource(crate::test_support::session_with_timing_slack());
        world.init_resource::<EditorHistoryLedger>();
        world.init_resource::<MaterialProgramEditHistory>();
        world.init_resource::<FunctionEditor>();
        world.run_system_once(sync_effect_history_ledger).unwrap();
        Self {
            root,
            world,
            programs,
            function,
        }
    }

    fn material_redo(&mut self, index: usize) {
        let mut session = self.world.remove_resource::<EditorSession>().unwrap();
        let mut catalog = self
            .world
            .remove_resource::<ProjectEffectCatalog>()
            .unwrap();
        let mut history = self
            .world
            .remove_resource::<MaterialProgramEditHistory>()
            .unwrap();
        let mut ledger = self.world.remove_resource::<EditorHistoryLedger>().unwrap();
        let mut functions = self.world.remove_resource::<FunctionEditor>().unwrap();
        let before = self.programs[index].clone();
        session.open_material_program(&catalog, before.id).unwrap();
        let mut after = before.clone();
        after.name = "Edited program".into();
        history
            .execute_replacement(&mut session, &mut catalog, "Edit", before, after)
            .unwrap();
        assert!(execute_document_history(
            HistoryAction::Undo,
            &mut session,
            &mut catalog,
            &mut history,
            &mut ledger,
            &mut functions
        ));
        self.world.insert_resource(session);
        self.world.insert_resource(catalog);
        self.world.insert_resource(history);
        self.world.insert_resource(ledger);
        self.world.insert_resource(functions);
    }

    fn function_redo(&mut self) {
        let mut session = self.world.remove_resource::<EditorSession>().unwrap();
        let mut catalog = self
            .world
            .remove_resource::<ProjectEffectCatalog>()
            .unwrap();
        let mut functions = self.world.remove_resource::<FunctionEditor>().unwrap();
        session
            .open_material_function(&catalog, self.function.id)
            .unwrap();
        let mut after = self.function.clone();
        after.name = "Edited function".into();
        functions.edit(&mut session, &mut catalog, after).unwrap();
        functions.step(&mut session, &mut catalog, true).unwrap();
        self.world.insert_resource(session);
        self.world.insert_resource(catalog);
        self.world.insert_resource(functions);
    }

    fn effect_redo(&mut self) {
        let mut session = self.world.resource_mut::<EditorSession>();
        session.material_target = MaterialEditingTarget::EffectInstance;
        session.material_history_active = false;
        session.set_effect_name("Edited effect");
        session.undo();
        assert!(session.can_redo());
    }

    fn target(&self, index: usize) -> MaterialEditingTarget {
        MaterialEditingTarget::Program {
            root: self.root.path().into(),
            id: self.programs[index].id,
        }
    }
    fn program_redo_available(&self, index: usize) -> bool {
        self.world
            .resource::<MaterialProgramEditHistory>()
            .available_for_program(self.root.path(), self.programs[index].id, false)
    }
    fn function_redo_available(&self) -> bool {
        self.world
            .resource::<FunctionEditor>()
            .available_for_function(self.root.path(), self.function.id, false)
    }
    fn delete(&self) -> aestra_project::content::operations::DeletedSource {
        fs::write(self.root.path().join("unused.png"), b"recoverable bytes").unwrap();
        let content = aestra_project::ProjectContent::scan(self.root.path());
        let source = content
            .source_tree()
            .at_relative_path("unused.png")
            .unwrap()
            .id;
        content
            .plan_delete_source(source, &[], true)
            .unwrap()
            .apply()
            .unwrap()
    }
    fn histories_with_redo(&mut self) {
        self.material_redo(0);
        self.material_redo(1);
        self.function_redo();
        self.effect_redo();
        assert!(
            self.program_redo_available(0)
                && self.program_redo_available(1)
                && self.function_redo_available()
        );
    }
}

#[test]
fn presentation_fork_is_program_scoped_and_does_not_publish_semantics() {
    let mut f = Fixture::new();
    f.histories_with_redo();
    let effect = f.world.resource::<EditorSession>().effect.clone();
    let revision = f.world.resource::<EditorSession>().document_revision();
    let preview = f
        .world
        .resource::<EditorSession>()
        .preview()
        .unwrap()
        .effect()
        .clone();
    let drafts = f
        .world
        .resource::<ProjectEffectCatalog>()
        .material_drafts
        .clone();
    let target = f.target(0);
    clear_presentation_redo(&mut f.world, &asset_order::Context::Material(target));
    assert!(!f.program_redo_available(0));
    assert!(f.program_redo_available(1) && f.function_redo_available());
    let session = f.world.resource::<EditorSession>();
    assert!(session.can_redo());
    assert_eq!(session.effect, effect);
    assert_eq!(session.document_revision(), revision);
    assert!(std::sync::Arc::ptr_eq(
        &preview,
        session.preview().unwrap().effect()
    ));
    assert_eq!(
        f.world.resource::<ProjectEffectCatalog>().material_drafts,
        drafts
    );
    assert_eq!(
        MaterialProgram::load_ron(f.root.path().join("program-0.aestra.material.ron")).unwrap(),
        f.programs[0]
    );
}

#[test]
fn function_presentation_fork_preserves_other_assets_and_effect_redo() {
    let mut f = Fixture::new();
    f.histories_with_redo();
    let context = asset_order::Context::Material(MaterialEditingTarget::Function {
        root: f.root.path().into(),
        id: f.function.id,
    });
    clear_presentation_redo(&mut f.world, &context);
    assert!(!f.function_redo_available());
    assert!(f.program_redo_available(0) && f.program_redo_available(1));
    assert!(f.world.resource::<EditorSession>().can_redo());
}

#[test]
fn effect_presentation_fork_preserves_standalone_asset_branches() {
    let mut f = Fixture::new();
    f.histories_with_redo();
    clear_presentation_redo(&mut f.world, &asset_order::Context::Effect);
    assert!(!f.world.resource::<EditorSession>().can_redo());
    assert!(
        f.program_redo_available(0) && f.program_redo_available(1) && f.function_redo_available()
    );
    assert!(!f.world.resource::<EditorHistoryLedger>().can_redo());
}

#[test]
fn recording_a_delete_forks_all_redo_branches_but_retains_recovery_and_document() {
    let mut f = Fixture::new();
    f.histories_with_redo();
    let effect = f.world.resource::<EditorSession>().effect.clone();
    let revision = f.world.resource::<EditorSession>().document_revision();
    let item = f.delete();
    asset_order::record_delete(&mut f.world, item.clone());
    assert!(f.world.resource::<asset_order::AssetOrder>().active());
    assert!(
        !f.program_redo_available(0)
            && !f.program_redo_available(1)
            && !f.function_redo_available()
    );
    assert!(!f.world.resource::<EditorSession>().can_redo());
    assert_eq!(f.world.resource::<EditorSession>().effect, effect);
    assert_eq!(
        f.world.resource::<EditorSession>().document_revision(),
        revision
    );
    assert!(item.journal().exists());
    item.restore(&[], true).unwrap();
}

#[test]
fn synchronization_forks_all_redo_after_a_new_document_edit_in_filesystem_order() {
    let mut f = Fixture::new();
    let item = f.delete();
    asset_order::record_delete(&mut f.world, item.clone());
    f.histories_with_redo();
    let context = asset_order::Context::Material(f.target(1));
    f.world
        .resource_mut::<EditorSession>()
        .operation_order
        .record(context);
    f.world.run_system_once(sync_effect_history_ledger).unwrap();
    assert!(!f.world.resource::<EditorSession>().can_redo());
    assert!(
        !f.program_redo_available(0)
            && !f.program_redo_available(1)
            && !f.function_redo_available()
    );
    assert!(f.world.resource::<asset_order::AssetOrder>().active());
    item.restore(&[], true).unwrap();
}

#[test]
fn clearing_document_order_preserves_other_contexts_and_recoverable_deletion() {
    let mut f = Fixture::new();
    let first = asset_order::Context::Material(f.target(0));
    let second = asset_order::Context::Material(f.target(1));
    f.world
        .resource_mut::<EditorSession>()
        .operation_order
        .record(first);
    f.world
        .resource_mut::<EditorSession>()
        .operation_order
        .record(second);
    let item = f.delete();
    asset_order::record_delete(&mut f.world, item.clone());
    let serial = f
        .world
        .resource::<EditorSession>()
        .operation_order
        .edit_serial();
    let target = f.target(0);
    clear_document_order(&mut f.world, target);
    assert_eq!(
        f.world
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        serial
    );
    let order = f.world.resource::<asset_order::AssetOrder>();
    assert!(order.active());
    assert!(order.matches_delete(true, order.revision));
    assert!(item.journal().exists());
    item.restore(&[], true).unwrap();
}

#[test]
fn ordinary_effect_sync_preserves_unrelated_standalone_redo_and_retiring_effect_clears_material_history()
 {
    let mut f = Fixture::new();
    f.material_redo(0);
    f.world
        .resource_mut::<EditorSession>()
        .set_effect_name("Effect edit");
    f.world.run_system_once(sync_effect_history_ledger).unwrap();
    assert!(f.program_redo_available(0));
    let session = crate::test_support::session_with_timing_slack();
    f.world.insert_resource(session);
    f.world.run_system_once(sync_effect_history_ledger).unwrap();
    assert!(!f.program_redo_available(0));
}
