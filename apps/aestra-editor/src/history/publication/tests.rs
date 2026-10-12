use super::*;
use crate::history::material_program::{MaterialProgramEditHistory, MaterialProgramHistoryEntry};
use aestra_core::{
    EffectAsset, EffectAssetRef, EffectClip, EffectId, Emitter, MaterialExpressionId, MaterialId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunctionRef, MaterialInstance,
        MaterialProgramRef, MaterialRenderState, MaterialValue,
    },
};
use bevy::prelude::World;
use std::{collections::BTreeMap, fs};

struct Fixture {
    root: tempfile::TempDir,
    catalog: EditorProjectContent,
    program: MaterialProgram,
    effect: EffectAsset,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Publication").normalized();
        program
            .save_ron(root.path().join("program.aestra.material.ron"))
            .unwrap();
        let catalog = EditorProjectContent::scan(root.path());
        let mut effect = EffectAsset::new("Publication", 4.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 4.0));
        effect.material_instances.push(MaterialInstance {
            id: MaterialId::new(),
            program: MaterialProgramRef::Project(program.id),
            values: BTreeMap::new(),
            render_state: MaterialRenderState::additive_sprite(),
        });
        Self {
            root,
            catalog,
            program,
            effect,
        }
    }
    fn document(&self, standalone: bool) -> MaterialAuthoringDocument {
        let programs = vec![self.catalog.material_program(self.program.id).unwrap()];
        let document = if standalone {
            MaterialAuthoringDocument::standalone(programs)
        } else {
            MaterialAuthoringDocument::new(self.effect.clone(), programs)
        };
        document.with_material_functions(self.catalog.material_functions().unwrap())
    }
    fn disk(&self) -> MaterialProgram {
        MaterialProgram::load_ron(self.root.path().join("program.aestra.material.ron")).unwrap()
    }
}

fn function() -> MaterialFunction {
    let mut function = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    function.custom_wesl = None;
    function.expressions.push(MaterialExpression {
        id: function.outputs[0].expression,
        kind: MaterialExpressionKind::FunctionInput(function.inputs[0].id),
    });
    for input in &mut function.inputs {
        input.default = Some(MaterialValue::Float(0.25));
    }
    function.normalized()
}

fn changed(before: &MaterialProgram) -> MaterialProgram {
    let mut after = before.clone();
    after.name = "Published draft".into();
    after
}

fn standalone(
    catalog: &mut EditorProjectContent,
    label: &str,
    before: &MaterialProgram,
    after: &MaterialProgram,
) -> Result<(), String> {
    let document =
        MaterialAuthoringDocument::standalone(vec![catalog.material_program(before.id)?])
            .with_material_functions(catalog.material_functions()?);
    replace_program(catalog, document, label, before, after, |_| {
        panic!("Standalone must not install a preview")
    })
}

#[test]
fn actual_catalog_resource_publishes_standalone_draft_without_changing_disk_or_preview() {
    let mut fixture = Fixture::new();
    let after = changed(&fixture.program);
    let document = fixture.document(true);
    replace_program(
        &mut fixture.catalog,
        document,
        "Edit",
        &fixture.program,
        &after,
        |_| panic!("No effect context"),
    )
    .unwrap();
    let mut world = World::new();
    world.insert_resource(fixture.catalog);
    let catalog = world.resource::<EditorProjectContent>();
    assert_eq!(catalog.material_program(after.id).unwrap(), after);
    assert_eq!(
        MaterialProgram::load_ron(fixture.root.path().join("program.aestra.material.ron")).unwrap(),
        fixture.program
    );
}

#[test]
fn effect_publication_compiles_the_real_project_and_installs_exactly_once() {
    let mut fixture = Fixture::new();
    let after = changed(&fixture.program);
    let document = fixture.document(false);
    let mut installed = None;
    replace_program(
        &mut fixture.catalog,
        document,
        "Edit",
        &fixture.program,
        &after,
        |compiled| {
            assert_eq!(compiled.source, fixture.effect.id);
            assert_eq!(
                compiled.material_program(after.id).unwrap().name,
                after.name
            );
            installed = Some(compiled);
            Ok(())
        },
    )
    .unwrap();
    assert!(installed.is_some());
    assert_eq!(fixture.catalog.material_program(after.id).unwrap(), after);
    assert_eq!(fixture.disk(), fixture.program);
}

#[test]
fn invalid_authoring_and_stale_draft_fail_before_installation_without_overwriting_work() {
    let mut fixture = Fixture::new();
    let mut invalid = changed(&fixture.program);
    invalid.outputs.alpha = MaterialExpressionId::new();
    let document = fixture.document(true);
    assert!(
        replace_program(
            &mut fixture.catalog,
            document,
            "Invalid",
            &fixture.program,
            &invalid,
            |_| panic!("Invalid document")
        )
        .is_err()
    );
    assert!(fixture.catalog.material_drafts.programs.is_empty());
    let after = changed(&fixture.program);
    standalone(&mut fixture.catalog, "First", &fixture.program, &after).unwrap();
    let original = fixture.catalog.material_drafts.clone();
    let document = fixture.document(true);
    assert!(
        replace_program(
            &mut fixture.catalog,
            document,
            "Stale",
            &fixture.program,
            &fixture.program,
            |_| panic!("Stale draft")
        )
        .is_err()
    );
    assert_eq!(fixture.catalog.material_drafts, original);
    assert_eq!(fixture.disk(), fixture.program);
}

#[test]
fn project_dependency_compile_failure_rolls_back_the_published_program() {
    let mut fixture = Fixture::new();
    fixture.effect.effect_clips.push(EffectClip::new(
        EffectAssetRef::new(EffectId::new()),
        0.0,
        1.0,
    ));
    let document = fixture.document(false);
    let after = changed(&fixture.program);
    assert!(
        replace_program(
            &mut fixture.catalog,
            document,
            "Missing dependency",
            &fixture.program,
            &after,
            |_| panic!("Project compile must fail first")
        )
        .is_err()
    );
    assert_eq!(
        fixture.catalog.material_program(after.id).unwrap(),
        fixture.program
    );
    assert!(fixture.catalog.material_drafts.programs.is_empty());
    assert_eq!(fixture.disk(), fixture.program);
}

#[test]
fn failed_preview_installation_restores_existing_draft_and_keeps_history_retryable() {
    let mut fixture = Fixture::new();
    let before = changed(&fixture.program);
    standalone(
        &mut fixture.catalog,
        "Existing draft",
        &fixture.program,
        &before,
    )
    .unwrap();
    let baseline = fixture.catalog.material_drafts.clone();
    let mut after = before.clone();
    after.name = "Next draft".into();
    let mut history = MaterialProgramEditHistory::default();
    history.record_applied(MaterialProgramHistoryEntry {
        label: "Next draft".into(),
        before: before.clone(),
        after: after.clone(),
        created_function: None,
    });
    // Exercise the actual redo stack: the original semantic publication is undone already.
    history.step_applied(true, |_| Ok(())).unwrap();
    let document = fixture.document(false);
    assert_eq!(
        history.step_applied(false, |entry| replace_program(
            &mut fixture.catalog,
            document,
            &entry.label,
            &entry.before,
            &entry.after,
            |_| Err("preview installation failed".into())
        )),
        Err("preview installation failed".into())
    );
    assert_eq!(fixture.catalog.material_drafts, baseline);
    assert_eq!(history.redo.len(), 1);
    let document = fixture.document(false);
    history
        .step_applied(false, |entry| {
            replace_program(
                &mut fixture.catalog,
                document,
                &entry.label,
                &entry.before,
                &entry.after,
                |_| Ok(()),
            )
        })
        .unwrap();
    assert_eq!(fixture.catalog.material_program(after.id).unwrap(), after);
    assert!(history.redo.is_empty());
    assert_eq!(fixture.disk(), fixture.program);
}

#[test]
fn rollback_conflicts_preserve_the_newer_draft_and_report_both_failures() {
    let mut fixture = Fixture::new();
    let after = changed(&fixture.program);
    standalone(&mut fixture.catalog, "Edit", &fixture.program, &after).unwrap();
    let mut newer = after.clone();
    newer.name = "Concurrent work".into();
    fixture
        .catalog
        .replace_material_program(&after, &newer)
        .unwrap();
    let error = rollback_program(
        &mut fixture.catalog,
        &after,
        &fixture.program,
        "compile failed".into(),
    );
    assert!(error.starts_with("compile failed; restoring the previous material also failed:"));
    assert_eq!(fixture.catalog.material_program(newer.id).unwrap(), newer);
}

#[test]
fn extraction_and_undo_publish_real_function_drafts_and_program_changes_as_one_unit() {
    let mut fixture = Fixture::new();
    let function = function();
    let after = changed(&fixture.program);
    extract_function(&mut fixture.catalog, &function, |catalog| {
        standalone(catalog, "Extract", &fixture.program, &after)
    })
    .unwrap();
    assert_eq!(
        fixture.catalog.material_functions().unwrap(),
        vec![function.clone()]
    );
    assert_eq!(fixture.catalog.material_program(after.id).unwrap(), after);
    undo_extraction(
        &mut fixture.catalog,
        "Extract",
        &fixture.program,
        &after,
        &function,
        standalone,
    )
    .unwrap();
    assert!(fixture.catalog.material_functions().unwrap().is_empty());
    assert!(fixture.catalog.material_drafts.programs.is_empty());
    assert!(fixture.catalog.material_drafts.functions.is_empty());
    assert_eq!(fixture.disk(), fixture.program);
}

#[test]
fn failed_extraction_cleans_up_new_function_or_reports_failed_cleanup_without_hiding_it() {
    let mut fixture = Fixture::new();
    let function = function();
    assert_eq!(
        extract_function(&mut fixture.catalog, &function, |_| Err(
            "program failed".into()
        )),
        Err("program failed".into())
    );
    assert!(fixture.catalog.material_functions().unwrap().is_empty());
    let mut newer = function.clone();
    newer.name = "Changed during publication".into();
    let error = extract_function(&mut fixture.catalog, &function, |catalog| {
        catalog.replace_material_function(&function, &newer)?;
        Err("program failed".into())
    })
    .unwrap_err();
    assert!(error.starts_with("program failed; removing the extracted function also failed:"));
    assert_eq!(fixture.catalog.material_functions().unwrap(), vec![newer]);
    assert!(
        extract_function(&mut fixture.catalog, &function, |_| panic!(
            "Existing identity must reject before publishing"
        ))
        .is_err()
    );
}

#[test]
fn failed_function_removal_restores_material_or_reports_restore_failure() {
    for fail_restore in [false, true] {
        let mut fixture = Fixture::new();
        let function = function();
        let after = changed(&fixture.program);
        extract_function(&mut fixture.catalog, &function, |catalog| {
            standalone(catalog, "Extract", &fixture.program, &after)
        })
        .unwrap();
        let mut newer = function.clone();
        newer.name = "Newer function draft".into();
        fixture
            .catalog
            .replace_material_function(&function, &newer)
            .unwrap();
        let mut calls = 0;
        let error = undo_extraction(
            &mut fixture.catalog,
            "Extract",
            &fixture.program,
            &after,
            &function,
            |catalog, label, before, after| {
                calls += 1;
                if fail_restore && calls == 2 {
                    return Err("restore failed".into());
                }
                standalone(catalog, label, before, after)
            },
        )
        .unwrap_err();
        assert_eq!(calls, 2);
        assert_eq!(fixture.catalog.material_functions().unwrap(), vec![newer]);
        assert_eq!(
            fixture.catalog.material_program(after.id).unwrap(),
            if fail_restore {
                fixture.program.clone()
            } else {
                after
            }
        );
        assert_eq!(error.contains("restoring the material after the failed function removal also failed: restore failed"), fail_restore);
        assert_eq!(fixture.disk(), fixture.program);
    }
}

#[test]
fn function_publication_checks_all_indexed_callers_and_rejects_stale_or_incompatible_changes() {
    let mut fixture = Fixture::new();
    let function = function();
    function
        .save_ron(
            fixture
                .root
                .path()
                .join("function.aestra.material-function.ron"),
        )
        .unwrap();
    let mut caller = fixture.program.clone();
    caller
        .expressions
        .iter_mut()
        .find(|e| e.id == caller.outputs.alpha)
        .unwrap()
        .kind = MaterialExpressionKind::FunctionCall {
        function: MaterialFunctionRef::Project(function.id),
        output: function.outputs[0].id,
        arguments: BTreeMap::new(),
    };
    caller
        .save_ron(fixture.root.path().join("program.aestra.material.ron"))
        .unwrap();
    fixture.catalog.refresh();
    let disk = fs::read(
        fixture
            .root
            .path()
            .join("function.aestra.material-function.ron"),
    )
    .unwrap();
    let mut after = function.clone();
    after.inputs[0].default = Some(MaterialValue::Float(0.75));
    let report = replace_function(&mut fixture.catalog, &function, &function, &after).unwrap();
    assert!(report.contains("1 direct call sites checked"));
    let baseline = fixture.catalog.material_drafts.clone();
    assert_eq!(
        replace_function(&mut fixture.catalog, &after, &function, &function),
        Err("Function changed since this edit was prepared".into())
    );
    let mut invalid = after.clone();
    invalid.inputs[0].default = None;
    assert!(replace_function(&mut fixture.catalog, &after, &after, &invalid).is_err());
    assert_eq!(fixture.catalog.material_drafts, baseline);
    assert_eq!(
        fs::read(
            fixture
                .root
                .path()
                .join("function.aestra.material-function.ron")
        )
        .unwrap(),
        disk
    );
    assert_eq!(fixture.catalog.material_functions().unwrap(), vec![after]);
}
