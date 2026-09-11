use super::*;
use aestra_core::material::MaterialProgram;

#[test]
fn function_recovery_preserves_identity_drafts_and_conflict_baselines() {
    use aestra_core::material::MaterialFunction;
    for scenario in ["normal", "moved", "missing", "duplicate", "external"] {
        let directory = tempfile::tempdir().unwrap();
        let (mut session, mut catalog, program) = fixture(&directory.path().join("project"));
        edit(&mut session, &mut catalog, &program);
        let function = MaterialFunction::from_ron(include_str!(
            "../../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
        ))
        .unwrap();
        let path = catalog.root().join("function.aestra.material-function.ron");
        function.save_ron(&path).unwrap();
        catalog.refresh();
        session
            .open_material_function(&catalog, function.id)
            .unwrap();
        let mut changed = function.clone();
        changed.name = "Recovered function".into();
        catalog
            .replace_material_function(&function, &changed)
            .unwrap();
        session.set_material_drafts(catalog.material_drafts.clone());
        let effect = session.effect.clone();
        let (mut persistence, candidate) = snapshot(&directory.path().join("recovery"), &session);
        let moved = catalog.root().join("moved.aestra.material-function.ron");
        match scenario {
            "moved" => fs::rename(&path, &moved).unwrap(),
            "missing" => fs::remove_file(&path).unwrap(),
            "duplicate" => function.save_ron(&moved).unwrap(),
            "external" => fs::write(
                &path,
                format!("// external\n{}", function.to_pretty_ron().unwrap()),
            )
            .unwrap(),
            _ => {}
        }
        let warnings =
            restore_candidate(&mut session, &mut persistence, &candidate, &mut catalog).unwrap();
        assert_eq!(session.standalone_function(), Some(function.id));
        assert_eq!(session.effect, effect);
        assert_eq!(session.material_drafts.programs.len(), 1);
        assert_eq!(
            session.material_drafts.functions[&function.id]
                .current
                .as_ref(),
            Some(&changed)
        );
        assert_eq!(warnings.is_empty(), matches!(scenario, "normal" | "moved"));
        if matches!(scenario, "missing" | "duplicate") {
            assert!(session.graph_function(&catalog).is_err());
        } else {
            assert_eq!(session.graph_function(&catalog).unwrap(), changed);
        }
        if scenario == "external" {
            let bytes = fs::read(&path).unwrap();
            assert!(session.material_drafts.save().is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        if scenario == "moved" {
            assert_eq!(session.material_drafts.functions[&function.id].path, moved);
            session.material_drafts.preflight().unwrap();
        }
    }
}

#[test]
fn autosave_retains_clean_function_target() {
    let directory = tempfile::tempdir().unwrap();
    let (mut session, mut catalog, _) = fixture(&directory.path().join("project"));
    let function = aestra_core::material::MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
    ))
    .unwrap();
    function
        .save_ron(catalog.root().join("function.aestra.material-function.ron"))
        .unwrap();
    catalog.refresh();
    session
        .save_as(catalog.root().join("effect.aestra.ron"))
        .unwrap();
    session
        .open_material_function(&catalog, function.id)
        .unwrap();
    assert!(!session.dirty);
    let mut state = AutosaveState::new(&session, true);
    let mut persistence = RecoveryPersistence::for_test(directory.path().join("recovery"), None);
    let settings = EditorSettings::default();
    let localizer = Localizer::new("en-US").unwrap();
    let now = Instant::now()
        + Duration::from_secs(u64::from(settings.general.autosave_interval_seconds) + 1);
    autosave_recovery_at(
        &mut session,
        &settings,
        &mut persistence,
        &mut state,
        now,
        &localizer,
    );
    let (_, candidate, _) = RecoveryPersistence::discover_in(directory.path().join("recovery"));
    assert_eq!(
        candidate.unwrap().material_target(),
        &session.material_target
    );
}

#[test]
#[ignore = "creates an isolated native-recovery smoke fixture under target; run with --nocapture"]
fn native_material_recovery_fixture() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!(
            "material-recovery-smoke-{}",
            aestra_core::EffectId::new()
        ));
    let (mut session, mut catalog, program) = fixture(&directory.join("project"));
    edit(&mut session, &mut catalog, &program);
    snapshot(&directory.join("config/recovery"), &session);
    println!(
        "NATIVE_RECOVERY_CONFIG={}",
        directory.join("config").canonicalize().unwrap().display()
    );
}

fn fixture(root: &Path) -> (EditorSession, ProjectEffectCatalog, MaterialProgram) {
    fs::create_dir_all(root).unwrap();
    let program = MaterialProgram::additive_sprite("Shared").normalized();
    program
        .save_ron(root.join("source.aestra.material.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(root.canonicalize().unwrap());
    let mut session = crate::test_support::session_with_timing_slack();
    session.source_path = None;
    session.open_material_program(&catalog, program.id).unwrap();
    (session, catalog, program)
}
fn snapshot(directory: &Path, session: &EditorSession) -> (RecoveryPersistence, RecoveryCandidate) {
    let mut persistence = RecoveryPersistence::for_test(directory.to_owned(), None);
    persistence
        .persist_document(
            &session.effect,
            session.source_path.as_deref(),
            &session.material_drafts,
            &session.material_target,
        )
        .unwrap();
    let (persistence, candidate, diagnostic) =
        RecoveryPersistence::discover_in(directory.to_owned());
    assert!(diagnostic.is_none());
    (persistence, candidate.unwrap())
}
fn edit(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    program: &MaterialProgram,
) -> MaterialProgram {
    let mut changed = program.clone();
    changed.name = "Recovered edit".into();
    catalog.replace_material_program(program, &changed).unwrap();
    session.set_material_drafts(catalog.material_drafts.clone());
    changed
}

#[test]
fn restores_standalone_target_and_draft_without_requiring_effect_usage() {
    let directory = tempfile::tempdir().unwrap();
    let (mut session, mut catalog, program) = fixture(&directory.path().join("project"));
    let changed = edit(&mut session, &mut catalog, &program);
    let effect = session.effect.clone();
    let (mut persistence, candidate) = snapshot(&directory.path().join("recovery"), &session);
    let mut restarted = crate::test_support::session_with_timing_slack();
    let (_, mut other_catalog, _) = fixture(&directory.path().join("other"));
    assert!(
        restore_candidate(
            &mut restarted,
            &mut persistence,
            &candidate,
            &mut other_catalog
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(restarted.effect, effect);
    assert_eq!(restarted.standalone_material(), Some(program.id));
    assert!(restarted.material_history_active);
    assert_eq!(other_catalog.root(), catalog.root());
    assert_eq!(other_catalog.material_program(program.id).unwrap(), changed);
    assert!(
        restarted
            .graph_authoring_document(&other_catalog)
            .unwrap()
            .effect
            .is_none()
    );
    restarted.material_drafts.preflight().unwrap();
    assert!(persistence.has_active());
    assert_eq!(
        MaterialProgram::load_ron(catalog.root().join("source.aestra.material.ron")).unwrap(),
        program
    );
}

#[test]
fn external_changes_keep_recovered_draft_and_original_conflict_baseline() {
    let directory = tempfile::tempdir().unwrap();
    let (mut session, mut catalog, program) = fixture(&directory.path().join("project"));
    let changed = edit(&mut session, &mut catalog, &program);
    let (mut persistence, candidate) = snapshot(&directory.path().join("recovery"), &session);
    let path = catalog.root().join("source.aestra.material.ron");
    let external = format!(
        "// changed externally\n{}",
        program.to_pretty_ron().unwrap()
    );
    fs::write(&path, &external).unwrap();
    let warnings =
        restore_candidate(&mut session, &mut persistence, &candidate, &mut catalog).unwrap();
    assert!(!warnings.is_empty());
    assert_eq!(catalog.material_program(program.id).unwrap(), changed);
    assert!(session.material_drafts.save().is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), external);
}

#[test]
fn moved_source_rebinds_unique_path_without_rebasing_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let (mut session, mut catalog, program) = fixture(&directory.path().join("project"));
    let changed = edit(&mut session, &mut catalog, &program);
    let (mut persistence, candidate) = snapshot(&directory.path().join("recovery"), &session);
    let new_path = catalog.root().join("renamed.aestra.material.ron");
    fs::rename(catalog.root().join("source.aestra.material.ron"), &new_path).unwrap();
    assert!(
        restore_candidate(&mut session, &mut persistence, &candidate, &mut catalog)
            .unwrap()
            .is_empty()
    );
    assert_eq!(session.material_drafts.programs[&program.id].path, new_path);
    session.material_drafts.preflight().unwrap();
    session.material_drafts.save().unwrap();
    assert_eq!(MaterialProgram::load_ron(&new_path).unwrap(), changed);
    // A move plus external edits must still conflict with the saved baseline.
    program.save_ron(&new_path).unwrap();
    fs::write(
        &new_path,
        format!("// external\n{}", program.to_pretty_ron().unwrap()),
    )
    .unwrap();
    assert!(
        !restore_candidate(&mut session, &mut persistence, &candidate, &mut catalog)
            .unwrap()
            .is_empty()
    );
    assert!(session.material_drafts.preflight().is_err());
}

#[test]
fn missing_and_duplicate_sources_keep_target_and_drafts_without_arbitrary_fallback() {
    let directory = tempfile::tempdir().unwrap();
    let (mut session, mut catalog, program) = fixture(&directory.path().join("project"));
    let changed = edit(&mut session, &mut catalog, &program);
    let (mut persistence, candidate) = snapshot(&directory.path().join("recovery"), &session);
    let path = catalog.root().join("source.aestra.material.ron");
    fs::remove_file(&path).unwrap();
    assert!(
        !restore_candidate(&mut session, &mut persistence, &candidate, &mut catalog)
            .unwrap()
            .is_empty()
    );
    assert_eq!(session.standalone_material(), Some(program.id));
    assert_eq!(
        session.material_drafts.programs[&program.id]
            .current
            .as_ref(),
        Some(&changed)
    );
    assert!(session.graph_material_programs(&catalog).is_err());
    program.save_ron(&path).unwrap();
    program
        .save_ron(catalog.root().join("duplicate.aestra.material.ron"))
        .unwrap();
    assert!(
        !restore_candidate(&mut session, &mut persistence, &candidate, &mut catalog)
            .unwrap()
            .is_empty()
    );
    assert!(session.graph_material_programs(&catalog).is_err());
    assert_eq!(session.material_drafts.count(), 1);
}

#[test]
fn unsafe_paths_and_mismatched_roots_reject_restore_without_replacing_session() {
    let directory = tempfile::tempdir().unwrap();
    let (mut session, mut catalog, program) = fixture(&directory.path().join("project"));
    edit(&mut session, &mut catalog, &program);
    let original = session.effect.clone();
    session
        .material_drafts
        .programs
        .get_mut(&program.id)
        .unwrap()
        .path = directory.path().join("outside.ron");
    let (mut persistence, candidate) = snapshot(&directory.path().join("unsafe"), &session);
    assert!(restore_candidate(&mut session, &mut persistence, &candidate, &mut catalog).is_err());
    assert_eq!(session.effect, original);
    assert!(!persistence.has_active());
    session.set_material_drafts(catalog.material_drafts.clone());
    let other = directory.path().join("other");
    fs::create_dir_all(&other).unwrap();
    session.material_target = MaterialEditingTarget::Program {
        root: other,
        id: program.id,
    };
    let (mut persistence, candidate) = snapshot(&directory.path().join("mismatch"), &session);
    assert!(restore_candidate(&mut session, &mut persistence, &candidate, &mut catalog).is_err());
    assert_eq!(session.effect, original);
    assert!(!persistence.has_active());
}

#[test]
fn clean_material_target_is_recoverable_even_if_effect_file_is_newer() {
    let directory = tempfile::tempdir().unwrap();
    let (mut session, mut catalog, program) = fixture(&directory.path().join("project"));
    let effect_path = catalog.root().join("effect.aestra.ron");
    session.save_as(&effect_path).unwrap();
    assert!(!session.dirty);
    let (mut persistence, _) = snapshot(&directory.path().join("recovery"), &session);
    session.effect.save_ron(&effect_path).unwrap();
    let (_, candidate, _) = RecoveryPersistence::discover_in(directory.path().join("recovery"));
    let candidate =
        candidate.expect("active material target must not be aged out by the effect file");
    session.return_to_effect_material();
    restore_candidate(&mut session, &mut persistence, &candidate, &mut catalog).unwrap();
    assert_eq!(session.standalone_material(), Some(program.id));
    assert!(!session.dirty);
}

#[test]
fn autosave_tracks_material_edits_and_target_changes_without_effect_revision_changes() {
    let directory = tempfile::tempdir().unwrap();
    let (mut session, mut catalog, program) = fixture(&directory.path().join("project"));
    session.return_to_effect_material();
    let mut state = AutosaveState::new(&session, true);
    let mut persistence = RecoveryPersistence::for_test(directory.path().join("recovery"), None);
    let settings = EditorSettings::default();
    let localizer = Localizer::new("en-US").unwrap();
    let interval = Duration::from_secs(u64::from(settings.general.autosave_interval_seconds));
    let mut now = Instant::now();
    let revision = session.document_revision();
    session.open_material_program(&catalog, program.id).unwrap();
    for name in ["First draft", "Second draft"] {
        let before = catalog.material_program(program.id).unwrap();
        let mut changed = before.clone();
        changed.name = name.into();
        catalog.replace_material_program(&before, &changed).unwrap();
        session.set_material_drafts(catalog.material_drafts.clone());
        autosave_recovery_at(
            &mut session,
            &settings,
            &mut persistence,
            &mut state,
            now,
            &localizer,
        );
        now += interval;
        autosave_recovery_at(
            &mut session,
            &settings,
            &mut persistence,
            &mut state,
            now,
            &localizer,
        );
        let (_, candidate, _) = RecoveryPersistence::discover_in(directory.path().join("recovery"));
        let candidate = candidate.unwrap();
        assert_eq!(candidate.material_target(), &session.material_target);
        assert_eq!(
            candidate.material_drafts().programs[&program.id]
                .current
                .as_ref(),
            Some(&changed)
        );
        assert_eq!(session.document_revision(), revision);
        now += interval;
    }
    session.return_to_effect_material();
    autosave_recovery_at(
        &mut session,
        &settings,
        &mut persistence,
        &mut state,
        now,
        &localizer,
    );
    now += interval;
    autosave_recovery_at(
        &mut session,
        &settings,
        &mut persistence,
        &mut state,
        now,
        &localizer,
    );
    let (_, candidate, _) = RecoveryPersistence::discover_in(directory.path().join("recovery"));
    assert_eq!(
        candidate.unwrap().material_target(),
        &MaterialEditingTarget::EffectInstance
    );
}

#[test]
fn autosave_keeps_clean_standalone_target_but_clears_it_after_return_to_clean_effect() {
    let directory = tempfile::tempdir().unwrap();
    let (mut session, catalog, program) = fixture(&directory.path().join("project"));
    session
        .save_as(catalog.root().join("effect.aestra.ron"))
        .unwrap();
    session.return_to_effect_material();
    let mut state = AutosaveState::new(&session, true);
    let mut persistence = RecoveryPersistence::for_test(directory.path().join("recovery"), None);
    let settings = EditorSettings::default();
    let localizer = Localizer::new("en-US").unwrap();
    let now = Instant::now();
    session.open_material_program(&catalog, program.id).unwrap();
    autosave_recovery_at(
        &mut session,
        &settings,
        &mut persistence,
        &mut state,
        now,
        &localizer,
    );
    autosave_recovery_at(
        &mut session,
        &settings,
        &mut persistence,
        &mut state,
        now + Duration::from_secs(u64::from(settings.general.autosave_interval_seconds)),
        &localizer,
    );
    assert!(persistence.has_active());
    session.return_to_effect_material();
    autosave_recovery_at(
        &mut session,
        &settings,
        &mut persistence,
        &mut state,
        now + Duration::from_secs(u64::from(settings.general.autosave_interval_seconds) + 1),
        &localizer,
    );
    assert!(!persistence.has_active());
}
