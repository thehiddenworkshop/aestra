//! Shared regressions for the real session preview installer, not a simulated installer.
use super::*;
use aestra_core::material::{MaterialInstance, MaterialProgramRef, MaterialRenderState};

fn fixture() -> (EditorSession, MaterialProgram) {
    let mut session = crate::test_support::session_with_timing_slack();
    let program = MaterialProgram::additive_sprite("Installed material").normalized();
    session.effect.material_instances.push(MaterialInstance {
        id: MaterialId::new(),
        program: MaterialProgramRef::Project(program.id),
        values: Default::default(),
        render_state: MaterialRenderState::additive_sprite(),
    });
    session.preview_seed = 41;
    let compiled = compile(&session, &program);
    session.install_compiled_project_root(compiled).unwrap();
    (session, program)
}

fn compile(session: &EditorSession, program: &MaterialProgram) -> Arc<CompiledEffect> {
    Arc::new(
        EffectCompiler::default()
            .compile_with_material_programs(
                &session.effect,
                &BTreeMap::from([(program.id, program.clone())]),
            )
            .unwrap(),
    )
}

fn rename_effect(name: &str) -> EffectTransaction {
    EffectTransaction::new(
        "Preview name",
        vec![EffectCommand::SetEffectName { name: name.into() }],
    )
}

#[test]
fn installation_replaces_instance_but_preserves_document_history_clock_and_seed() {
    let (mut session, mut program) = fixture();
    session.set_effect_name("Authored edit");
    session.seek_time(1.0);
    session.playing = true;
    let mut samples = Vec::new();
    session.evaluate_preview(&mut samples);
    assert!(!samples.is_empty());
    session.samples = samples;
    let effect = session.effect.clone();
    let clock = session.driver.clock;
    let source = session.source_path.clone();
    let history = session.effect_undo_len();
    let selection = session.selection;
    let dirty = session.dirty;
    let revision = session.document_revision();
    program.name = "Fresh compilation".into();
    let compiled = compile(&session, &program);
    session
        .install_compiled_project_root(compiled.clone())
        .unwrap();
    assert!(Arc::ptr_eq(session.preview().unwrap().effect(), &compiled));
    assert_eq!(session.preview().unwrap().seed(), 41);
    assert_eq!(session.effect, effect);
    assert_eq!(session.driver.clock, clock);
    assert_eq!(session.source_path, source);
    assert_eq!(session.effect_undo_len(), history);
    assert_eq!(session.selection, selection);
    assert_eq!(session.dirty, dirty);
    assert!(session.playing);
    assert_eq!(session.document_revision(), revision.wrapping_add(1));
    assert!(session.samples.is_empty());
    assert!(session.driver.checkpoints().unwrap().is_empty());
    session.undo();
    assert_ne!(session.effect.name, effect.name);
    assert_eq!(
        session
            .preview()
            .unwrap()
            .effect()
            .material_program(program.id),
        Some(&program)
    );
}

#[test]
fn solo_installation_recompiles_only_selected_emitter_without_mutating_authored_flags() {
    let (mut session, program) = fixture();
    let effect = session.effect.clone();
    let solo = effect.emitters[1].id;
    assert!(session.toggle_preview_solo(solo));
    session
        .install_compiled_project_root(compile(&session, &program))
        .unwrap();
    let enabled: Vec<_> = session
        .preview()
        .unwrap()
        .effect()
        .emitters
        .iter()
        .filter(|emitter| emitter.enabled)
        .map(|emitter| emitter.source)
        .collect();
    assert_eq!(enabled, [solo]);
    assert_eq!(session.effect, effect);
    assert_eq!(
        session
            .preview()
            .unwrap()
            .effect()
            .material_program(program.id),
        Some(&program)
    );
}

#[test]
fn interaction_candidate_takes_precedence_over_pending_candidate_and_neither_is_committed() {
    let (mut session, mut program) = fixture();
    let original = session.effect.clone();
    let history = session.effect_undo_len();
    assert!(session.preview_transaction(rename_effect("Pending")));
    assert!(session.preview_interaction(rename_effect("Interaction")));
    program.name = "New source".into();
    session
        .install_compiled_project_root(compile(&session, &program))
        .unwrap();
    assert_eq!(session.preview().unwrap().effect().name, "Interaction");
    assert_eq!(session.effect, original);
    assert_eq!(session.effect_undo_len(), history);
    assert_eq!(
        session
            .pending_change
            .as_ref()
            .unwrap()
            .preview
            .candidate()
            .name,
        "Pending"
    );
    session.restore_interaction_preview();
    session
        .install_compiled_project_root(compile(&session, &program))
        .unwrap();
    assert_eq!(session.preview().unwrap().effect().name, "Pending");
    assert_eq!(session.effect, original);
}

#[test]
fn failed_solo_recompile_keeps_previous_preview_samples_checkpoints_and_revision() {
    let (mut session, program) = fixture();
    session.toggle_preview_solo(session.effect.emitters[0].id);
    session.seek_time(1.0);
    let mut samples = Vec::new();
    session.evaluate_preview(&mut samples);
    session.samples = samples;
    let previous = session.preview().unwrap().effect().clone();
    let revision = session.document_revision();
    let checkpoints = session.driver.checkpoints().unwrap().len();
    let clock = session.driver.clock;
    let samples = session.samples.clone();
    let mut incomplete = (*compile(&session, &program)).clone();
    incomplete.material_programs.clear();
    assert!(
        session
            .install_compiled_project_root(Arc::new(incomplete))
            .is_err()
    );
    assert!(Arc::ptr_eq(session.preview().unwrap().effect(), &previous));
    assert_eq!(session.document_revision(), revision);
    assert_eq!(session.driver.clock, clock);
    assert_eq!(session.driver.checkpoints().unwrap().len(), checkpoints);
    assert_eq!(session.samples, samples);
}

#[test]
fn successful_installation_recovers_a_stale_preview_gate() {
    let (mut session, program) = fixture();
    session.set_preview_opt(None);
    assert!(session.preview().is_none());
    assert!(session.preview_mut().is_none());
    session
        .install_compiled_project_root(compile(&session, &program))
        .unwrap();
    assert!(session.preview().is_some());
    assert!(session.preview_mut().is_some());
}
