use super::*;
use aestra_authoring::{
    MaterialAuthoringDocument, MaterialCommand, MaterialCommandExecutor, MaterialTransaction,
};
use bevy::prelude::*;

fn entry(index: usize) -> MaterialProgramHistoryEntry {
    let before = MaterialProgram::additive_sprite(format!("Before {index}"));
    let mut after = before.clone();
    after.name = format!("After {index}");
    MaterialProgramHistoryEntry {
        label: index.to_string(),
        before,
        after,
        created_function: None,
    }
}

#[test]
fn empty_stacks_do_not_invoke_the_host_publication_callback() {
    let mut history = MaterialProgramEditHistory::default();
    for undo in [true, false] {
        assert_eq!(
            history
                .step_applied(undo, |_| panic!("No entry to publish"))
                .unwrap(),
            None
        );
    }
}

#[test]
fn successful_undo_redo_are_lifo_and_preserve_exact_payloads() {
    let mut history = MaterialProgramEditHistory::default();
    for index in 0..3 {
        history.record_applied(entry(index));
    }
    for index in (0..3).rev() {
        assert_eq!(
            history
                .step_applied(true, |entry| {
                    assert_eq!(entry.label, index.to_string());
                    assert_eq!(entry.before.name, format!("Before {index}"));
                    assert_eq!(entry.after.name, format!("After {index}"));
                    Ok(())
                })
                .unwrap(),
            Some(index.to_string())
        );
    }
    for index in 0..3 {
        assert_eq!(
            history.step_applied(false, |_| Ok(())).unwrap(),
            Some(index.to_string())
        );
    }
    assert_eq!(history.undo.len(), 3);
    assert!(history.redo.is_empty());
}

#[test]
fn failed_undo_and_redo_keep_the_original_stack_entry_for_retry() {
    let mut history = MaterialProgramEditHistory::default();
    history.record_applied(entry(1));
    for undo in [true, false] {
        assert_eq!(
            history.step_applied(undo, |_| Err("Publication failed".into())),
            Err("Publication failed".into())
        );
        assert_eq!(
            (history.undo.len(), history.redo.len()),
            if undo { (1, 0) } else { (0, 1) }
        );
        assert_eq!(
            history.step_applied(undo, |_| Ok(())).unwrap(),
            Some("1".into())
        );
    }
}

#[test]
fn new_edits_clear_redo_and_keep_the_existing_256_entry_limit() {
    let mut history = MaterialProgramEditHistory::default();
    for index in 0..MATERIAL_HISTORY_LIMIT + 5 {
        history.record_applied(entry(index));
    }
    assert_eq!(history.undo.len(), MATERIAL_HISTORY_LIMIT);
    assert_eq!(history.undo.front().unwrap().label, "5");
    history.step_applied(true, |_| Ok(())).unwrap();
    assert_eq!(history.redo.len(), 1);
    history.record_applied(entry(900));
    assert!(history.redo.is_empty());
    assert_eq!(history.undo.back().unwrap().label, "900");
}

#[test]
fn program_root_scopes_and_clear_do_not_touch_other_documents_or_effect_history() {
    let mut history = MaterialProgramEditHistory::default();
    let first = MaterialProgramId::new();
    let second = MaterialProgramId::new();
    history.record_applied(entry(0));
    for (root, id) in [("A", first), ("B", first), ("A", second)] {
        history
            .standalone
            .entry((PathBuf::from(root), id))
            .or_default()
            .record_applied(entry(1));
    }
    history.clear_program(Path::new("A"), first);
    assert!(!history.available_for_program(Path::new("A"), first, true));
    assert!(history.available_for_program(Path::new("B"), first, true));
    assert!(history.available_for_program(Path::new("A"), second, true));
    assert_eq!(history.undo.len(), 1);
    history.clear();
    assert!(history.standalone.is_empty());
    assert!(history.undo.is_empty());
    assert!(history.redo.is_empty());
}

#[test]
fn real_history_resource_round_trips_a_validated_program_replacement() {
    let entry = entry(2);
    let original = entry.before.clone();
    let edited = entry.after.clone();
    let mut document = MaterialAuthoringDocument::standalone(vec![edited.clone()]);
    let mut app = App::new();
    app.init_resource::<MaterialProgramEditHistory>();
    app.world_mut()
        .resource_mut::<MaterialProgramEditHistory>()
        .record_applied(entry);
    for undo in [true, false] {
        app.world_mut()
            .resource_mut::<MaterialProgramEditHistory>()
            .step_applied(undo, |entry| {
                MaterialCommandExecutor::execute(
                    &mut document,
                    &MaterialTransaction::single(
                        &entry.label,
                        MaterialCommand::ReplaceMaterialProgram {
                            id: entry.before.id,
                            program: if undo {
                                entry.before.clone()
                            } else {
                                entry.after.clone()
                            },
                        },
                    ),
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
            })
            .unwrap();
        assert_eq!(
            document.programs[0],
            if undo {
                original.clone()
            } else {
                edited.clone()
            }
        );
    }
}
