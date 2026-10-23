use super::*;
use crate::{
    history::asset_order::Context, material_function_editor::FunctionEditor,
    material_target::MaterialEditingTarget,
};
use aestra_core::{
    MaterialId,
    material::{
        MaterialFunction, MaterialInstance, MaterialProgram, MaterialProgramRef,
        MaterialRenderState,
    },
};
use std::sync::Arc;

struct Fixture {
    root: tempfile::TempDir,
    catalog: ProjectEffectCatalog,
    session: EditorSession,
    history: MaterialProgramEditHistory,
    ledger: EditorHistoryLedger,
    functions: FunctionEditor,
    program: MaterialProgram,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Original").normalized();
        program
            .save_ron(root.path().join("material.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session.effect.material_instances.push(MaterialInstance {
            id: MaterialId::new(),
            program: MaterialProgramRef::Project(program.id),
            values: Default::default(),
            render_state: MaterialRenderState::additive_sprite(),
        });
        let compiled = catalog.compile_project(&session.effect).unwrap().root;
        session.install_compiled_project_root(compiled).unwrap();
        Self {
            root,
            catalog,
            session,
            program,
            history: Default::default(),
            ledger: Default::default(),
            functions: Default::default(),
        }
    }
    fn edit(&mut self, before: &MaterialProgram, name: &str) -> MaterialProgram {
        let mut after = before.clone();
        after.name = name.into();
        self.history
            .execute_replacement(
                &mut self.session,
                &mut self.catalog,
                name,
                before.clone(),
                after.clone(),
            )
            .unwrap();
        self.ledger.record_material_edit(&mut self.session);
        after
    }
    fn step(&mut self, action: HistoryAction) -> bool {
        execute_document_history(
            action,
            &mut self.session,
            &mut self.catalog,
            &mut self.history,
            &mut self.ledger,
            &mut self.functions,
        )
    }
    fn disk(&self) -> MaterialProgram {
        MaterialProgram::load_ron(self.root.path().join("material.aestra.material.ron")).unwrap()
    }
}

#[test]
fn real_history_interleaves_effect_and_material_edits_and_keeps_compiled_materials() {
    let mut f = Fixture::new();
    let original_effect = f.session.effect.name.clone();
    f.session.set_effect_name("Effect edit");
    let before = f.program.clone();
    let after = f.edit(&before, "Material edit");
    f.session.adjust_effect_duration(0.25);
    let duration = f.session.effect.duration;
    assert!(f.step(HistoryAction::Undo));
    assert_ne!(f.session.effect.duration, duration);
    assert_eq!(f.catalog.material_program(before.id).unwrap(), after);
    assert!(f.step(HistoryAction::Undo));
    assert_eq!(f.catalog.material_program(before.id).unwrap(), before);
    assert_eq!(
        f.session
            .preview()
            .unwrap()
            .effect()
            .material_program(before.id),
        Some(&before)
    );
    assert!(f.step(HistoryAction::Undo));
    assert_eq!(f.session.effect.name, original_effect);
    for _ in 0..3 {
        assert!(f.step(HistoryAction::Redo));
    }
    assert_eq!(f.session.effect.name, "Effect edit");
    assert_eq!(f.session.effect.duration, duration);
    assert_eq!(
        f.session
            .preview()
            .unwrap()
            .effect()
            .material_program(after.id),
        Some(&after)
    );
    assert_eq!(f.disk(), before);
    assert!(f.session.dirty);
}

#[test]
fn standalone_undo_is_asset_scoped_and_does_not_install_or_touch_an_invalid_effect() {
    let mut f = Fixture::new();
    let second = MaterialProgram::additive_sprite("Second").normalized();
    second
        .save_ron(f.root.path().join("second.aestra.material.ron"))
        .unwrap();
    f.catalog.refresh();
    f.session.seek_time(0.75);
    f.session.effect.duration = -1.0;
    let effect = f.session.effect.clone();
    let clock = f.session.driver.clock;
    let preview = f.session.preview().unwrap().effect().clone();
    f.session
        .open_material_program(&f.catalog, f.program.id)
        .unwrap();
    let before = f.program.clone();
    let after = f.edit(&before, "First edited");
    f.session
        .open_material_program(&f.catalog, second.id)
        .unwrap();
    let second_after = f.edit(&second, "Second edited");
    f.session
        .open_material_program(&f.catalog, before.id)
        .unwrap();
    assert!(f.step(HistoryAction::Undo));
    assert_eq!(f.catalog.material_program(before.id).unwrap(), before);
    assert_eq!(f.catalog.material_program(second.id).unwrap(), second_after);
    assert!(f.step(HistoryAction::Redo));
    assert_eq!(f.catalog.material_program(before.id).unwrap(), after);
    assert_eq!(f.session.effect, effect);
    assert_eq!(f.session.driver.clock, clock);
    assert!(Arc::ptr_eq(f.session.preview().unwrap().effect(), &preview));
    assert_eq!(
        Context::current(&f.session),
        Context::Material(f.session.material_target.clone())
    );
    assert_eq!(f.disk(), before);
}

#[test]
fn same_program_id_in_another_project_has_independent_history_and_wrong_root_cannot_publish() {
    let mut f = Fixture::new();
    f.session
        .open_material_program(&f.catalog, f.program.id)
        .unwrap();
    let before = f.program.clone();
    let after = f.edit(&before, "Project one");
    let other = tempfile::tempdir().unwrap();
    before
        .save_ron(other.path().join("same.aestra.material.ron"))
        .unwrap();
    let mut other_catalog = ProjectEffectCatalog::scan(other.path());
    let serial = f.session.operation_order.edit_serial();
    assert!(
        f.history
            .execute_replacement(
                &mut f.session,
                &mut other_catalog,
                "Wrong root",
                before.clone(),
                after.clone()
            )
            .is_err()
    );
    assert_eq!(f.session.operation_order.edit_serial(), serial);
    f.session
        .open_material_program(&other_catalog, before.id)
        .unwrap();
    let mut other_after = before.clone();
    other_after.name = "Project two".into();
    f.history
        .execute_replacement(
            &mut f.session,
            &mut other_catalog,
            "Project two",
            before.clone(),
            other_after.clone(),
        )
        .unwrap();
    assert!(execute_document_history(
        HistoryAction::Undo,
        &mut f.session,
        &mut other_catalog,
        &mut f.history,
        &mut f.ledger,
        &mut f.functions
    ));
    assert_eq!(other_catalog.material_program(before.id).unwrap(), before);
    f.session
        .open_material_program(&f.catalog, before.id)
        .unwrap();
    assert!(f.step(HistoryAction::Undo));
    assert_eq!(f.catalog.material_program(before.id).unwrap(), before);
    assert!(f.step(HistoryAction::Redo));
    assert_eq!(f.catalog.material_program(before.id).unwrap(), after);
    f.session
        .open_material_program(&other_catalog, before.id)
        .unwrap();
    assert!(execute_document_history(
        HistoryAction::Redo,
        &mut f.session,
        &mut other_catalog,
        &mut f.history,
        &mut f.ledger,
        &mut f.functions
    ));
    assert_eq!(
        other_catalog.material_program(before.id).unwrap(),
        other_after
    );
}

#[test]
fn failed_undo_keeps_publication_preview_and_both_history_stacks_retryable() {
    let mut f = Fixture::new();
    let before = f.program.clone();
    let after = f.edit(&before, "Applied");
    let mut newer = after.clone();
    newer.name = "External draft".into();
    f.catalog.replace_material_program(&after, &newer).unwrap();
    let preview = f.session.preview().unwrap().effect().clone();
    assert!(!f.step(HistoryAction::Undo));
    assert!(f.session.status.contains("Material undo failed"));
    assert!(f.ledger.can_undo());
    assert!(!f.ledger.can_redo());
    assert!(Arc::ptr_eq(f.session.preview().unwrap().effect(), &preview));
    assert_eq!(f.catalog.material_program(before.id).unwrap(), newer);
    f.catalog.replace_material_program(&newer, &after).unwrap();
    assert!(f.step(HistoryAction::Undo));
    assert_eq!(f.catalog.material_program(before.id).unwrap(), before);
    assert!(f.step(HistoryAction::Redo));
    assert_eq!(f.catalog.material_program(before.id).unwrap(), after);
}

#[test]
fn function_actions_publish_scoped_history_without_consuming_effect_or_program_history() {
    let mut f = Fixture::new();
    let function = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    function
        .save_ron(f.root.path().join("function.aestra.material-function.ron"))
        .unwrap();
    f.catalog.refresh();
    f.session.set_effect_name("Effect edit");
    let before = f.program.clone();
    let after = f.edit(&before, "Material edit");
    let effect_history = f.session.effect_undo_len();
    let preview = f.session.preview().unwrap().effect().clone();
    f.session
        .open_material_function(&f.catalog, function.id)
        .unwrap();
    // Custom WESL functions are read-only. Use a validated body, not a fake catalog.
    let mut editable = function.clone();
    editable.custom_wesl = None;
    editable
        .expressions
        .push(aestra_core::material::MaterialExpression {
            id: editable.outputs[0].expression,
            kind: aestra_core::material::MaterialExpressionKind::FunctionInput(
                editable.inputs[0].id,
            ),
        });
    f.catalog
        .replace_material_function(&function, &editable)
        .unwrap();
    let mut renamed = editable.clone();
    renamed.name = "Function edit".into();
    f.functions
        .edit(&mut f.session, &mut f.catalog, renamed.clone())
        .unwrap();
    assert!(f.step(HistoryAction::Undo));
    assert_eq!(f.session.graph_function(&f.catalog).unwrap(), editable);
    assert!(f.step(HistoryAction::Redo));
    assert_eq!(f.session.graph_function(&f.catalog).unwrap(), renamed);
    assert_eq!(f.catalog.material_program(before.id).unwrap(), after);
    assert_eq!(f.session.effect_undo_len(), effect_history);
    assert!(Arc::ptr_eq(f.session.preview().unwrap().effect(), &preview));
    Context::Effect.select(&mut f.session);
    assert_eq!(
        f.session.material_target,
        MaterialEditingTarget::EffectInstance
    );
    assert!(f.step(HistoryAction::Undo));
    assert_eq!(f.catalog.material_program(before.id).unwrap(), before);
}

#[test]
fn a_new_effect_resets_old_global_material_history_instead_of_applying_it() {
    let mut f = Fixture::new();
    let before = f.program.clone();
    let after = f.edit(&before, "Old document edit");
    f.session.new_effect();
    assert!(!f.step(HistoryAction::Undo));
    assert_eq!(f.session.status, "Nothing to undo");
    assert_eq!(f.catalog.material_program(before.id).unwrap(), after);
    assert!(!f.ledger.can_undo());
    assert!(!f.ledger.can_redo());
    assert_eq!(f.disk(), before);
}

#[test]
fn real_extraction_rewiring_is_published_compiled_saved_and_reversed_together() {
    use aestra_authoring::{MaterialCommandExecutor, MaterialToolCommand, MaterialToolPlanner};
    let mut f = Fixture::new();
    let document = f.session.graph_authoring_document(&f.catalog).unwrap();
    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::ExtractMaterialFunction {
            program: f.program.id,
            function: aestra_core::MaterialFunctionId::new(),
            name: "Extracted color".into(),
            expressions: vec![f.program.outputs.color],
        },
    )
    .unwrap();
    let function = plan.created_function().unwrap().clone();
    let mut candidate = document;
    MaterialCommandExecutor::execute(&mut candidate, &plan.transaction).unwrap();
    let after = candidate
        .programs
        .into_iter()
        .find(|program| program.id == f.program.id)
        .unwrap()
        .normalized();
    assert_ne!(after.expressions, f.program.expressions);
    f.history
        .execute_extraction(
            &mut f.session,
            &mut f.catalog,
            "Extract color",
            f.program.clone(),
            after.clone(),
            function.clone(),
        )
        .unwrap();
    f.ledger.record_material_edit(&mut f.session);
    assert!(f.session.preview().is_some());
    assert_eq!(
        f.catalog.material_functions().unwrap().as_slice(),
        std::slice::from_ref(&function)
    );
    assert_eq!(f.catalog.material_program(f.program.id).unwrap(), after);
    assert!(f.step(HistoryAction::Undo));
    assert!(f.catalog.material_functions().unwrap().is_empty());
    assert_eq!(f.catalog.material_program(f.program.id).unwrap(), f.program);
    assert!(f.step(HistoryAction::Redo));
    f.catalog.save_material_drafts().unwrap();
    f.session
        .set_material_drafts(f.catalog.material_drafts.clone());
    assert_eq!(f.disk(), after);
    assert!(f.step(HistoryAction::Undo));
    assert!(f.catalog.material_functions().unwrap().is_empty());
    assert_eq!(f.catalog.material_program(f.program.id).unwrap(), f.program);
    assert_eq!(f.disk(), after);
    assert!(f.session.dirty);
    assert!(f.step(HistoryAction::Redo));
    assert_eq!(f.catalog.material_functions().unwrap(), [function]);
    assert!(f.catalog.material_drafts.is_empty());
}
