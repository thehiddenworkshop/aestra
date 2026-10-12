use super::*;
use bevy::prelude::World;

fn function() -> MaterialFunction {
    let mut function = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    function.custom_wesl = None;
    function
}

fn record(
    editor: &mut FunctionEditor,
    key: &Key,
    before: &MaterialFunction,
    name: &str,
) -> MaterialFunction {
    let mut after = before.clone();
    after.name = name.into();
    assert_eq!(
        editor
            .edit_applied(
                key.clone(),
                before.clone(),
                after.clone(),
                |expected, next| {
                    assert_eq!(expected, before);
                    assert_eq!(next, &after);
                    Ok(name.into())
                }
            )
            .unwrap(),
        Some(name.into())
    );
    after
}

#[test]
fn empty_history_never_publishes_or_creates_state() {
    let mut editor = FunctionEditor::default();
    let key = ("project".into(), function().id);
    for undo in [true, false] {
        assert_eq!(
            editor.step_applied(&key, undo, |_, _| panic!("No entry")),
            Err("No function history in this direction".into())
        );
        assert!(!editor.available_for_function(&key.0, key.1, undo));
    }
    assert!(editor.histories.is_empty());
    assert!(editor.reports.is_empty());
}

#[test]
fn history_resource_round_trips_exact_payloads_in_lifo_order() {
    let mut world = World::new();
    world.init_resource::<FunctionEditor>();
    let mut editor = world.resource_mut::<FunctionEditor>();
    let first = function();
    let key = ("project".into(), first.id);
    let second = record(&mut editor, &key, &first, "second");
    let third = record(&mut editor, &key, &second, "third");
    for (undo, before, after) in [
        (true, &third, &second),
        (true, &second, &first),
        (false, &first, &second),
        (false, &second, &third),
    ] {
        editor
            .step_applied(&key, undo, |expected, next| {
                assert_eq!(expected, before);
                assert_eq!(next, after);
                Ok(next.name.clone())
            })
            .unwrap();
        assert_eq!(editor.reports[&key], after.name);
    }
    assert_eq!(editor.histories[&key].undo.len(), 2);
    assert!(editor.histories[&key].redo.is_empty());
}

#[test]
fn failed_edit_preserves_stacks_and_records_diagnostic_for_retry() {
    let mut editor = FunctionEditor::default();
    let before = function();
    let key = ("project".into(), before.id);
    let after = record(&mut editor, &key, &before, "after");
    editor
        .step_applied(&key, true, |_, _| Ok("undone".into()))
        .unwrap();
    assert_eq!(
        editor.edit_applied(key.clone(), before.clone(), after.clone(), |_, _| Err(
            "source conflict".into()
        )),
        Err("source conflict".into())
    );
    assert!(editor.histories[&key].undo.is_empty());
    assert_eq!(
        editor.histories[&key].redo,
        vec![(before.clone(), after.clone())]
    );
    assert_eq!(editor.reports[&key], "source conflict");
    record(&mut editor, &key, &before, "retry");
    assert!(editor.histories[&key].redo.is_empty());
}

#[test]
fn failed_undo_and_redo_preserve_exact_stacks_and_last_report() {
    let mut editor = FunctionEditor::default();
    let before = function();
    let key = ("project".into(), before.id);
    let after = record(&mut editor, &key, &before, "after");
    for undo in [true, false] {
        let original_undo = editor.histories[&key].undo.clone();
        let original_redo = editor.histories[&key].redo.clone();
        let report = editor.reports[&key].clone();
        assert_eq!(
            editor.step_applied(&key, undo, |_, _| Err("publication failed".into())),
            Err("publication failed".into())
        );
        assert_eq!(editor.histories[&key].undo, original_undo);
        assert_eq!(editor.histories[&key].redo, original_redo);
        assert_eq!(editor.reports[&key], report);
        editor
            .step_applied(&key, undo, |_, _| Ok("retry succeeded".into()))
            .unwrap();
    }
    assert_eq!(editor.histories[&key].undo, vec![(before, after)]);
}

#[test]
fn noop_and_readonly_edits_never_publish_or_fork_redo() {
    let mut editor = FunctionEditor::default();
    let before = function();
    let key = ("project".into(), before.id);
    record(&mut editor, &key, &before, "after");
    editor
        .step_applied(&key, true, |_, _| Ok("undone".into()))
        .unwrap();
    let redo = editor.histories[&key].redo.clone();
    assert_eq!(
        editor.edit_applied(key.clone(), before.clone(), before, |_, _| panic!("No-op")),
        Ok(None)
    );
    let mut readonly = function();
    readonly.custom_wesl = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap()
    .custom_wesl;
    let mut edited = readonly.clone();
    edited.name = "not allowed".into();
    assert_eq!(
        editor.edit_applied(
            key.clone(),
            readonly.clone(),
            readonly.clone(),
            |_, _| panic!("No-op readonly")
        ),
        Ok(None)
    );
    assert_eq!(
        editor.edit_applied(key.clone(), readonly, edited, |_, _| panic!("Readonly")),
        Err("Custom WESL signatures remain read-only".into())
    );
    assert_eq!(editor.histories[&key].redo, redo);
    assert_eq!(editor.reports[&key], "undone");
}

#[test]
fn root_identity_local_forks_and_clearing_are_independent() {
    let mut editor = FunctionEditor::default();
    let first = function();
    let mut second = first.clone();
    second.id = MaterialFunctionId::new();
    let keys = [
        ("first".into(), first.id),
        ("second".into(), first.id),
        ("first".into(), second.id),
    ];
    for key in &keys {
        let before = if key.1 == first.id { &first } else { &second };
        record(&mut editor, key, before, "after");
        editor
            .step_applied(key, true, |_, _| Ok("undone".into()))
            .unwrap();
    }
    record(&mut editor, &keys[0], &first, "fork");
    assert!(!editor.available_for_function(&keys[0].0, keys[0].1, false));
    assert!(editor.available_for_function(&keys[1].0, keys[1].1, false));
    assert!(editor.available_for_function(&keys[2].0, keys[2].1, false));
    editor.clear_function_redo(&keys[1].0, keys[1].1);
    assert!(!editor.available_for_function(&keys[1].0, keys[1].1, false));
    editor.clear_redo();
    assert!(!editor.available_for_function(&keys[2].0, keys[2].1, false));
    editor.clear_function(&keys[0].0, keys[0].1);
    assert!(!editor.histories.contains_key(&keys[0]));
    assert!(!editor.reports.contains_key(&keys[0]));
    assert!(editor.reports.contains_key(&keys[1]));
    editor.clear_function_redo(std::path::Path::new("absent"), first.id);
    editor.clear_function(std::path::Path::new("absent"), first.id);
    assert_eq!(editor.histories.len(), 2);
}

#[test]
fn history_remains_bounded_at_256_without_losing_latest_edits() {
    let mut editor = FunctionEditor::default();
    let mut before = function();
    let key = ("project".into(), before.id);
    for index in 0..260 {
        before = record(&mut editor, &key, &before, &index.to_string());
    }
    assert_eq!(editor.histories[&key].undo.len(), 256);
    for index in (4..260).rev() {
        editor
            .step_applied(&key, true, |expected, next| {
                assert_eq!(expected.name, index.to_string());
                assert_eq!(next.name, (index - 1).to_string());
                Ok("undone".into())
            })
            .unwrap();
    }
    assert!(!editor.available_for_function(&key.0, key.1, true));
    assert_eq!(editor.histories[&key].redo.len(), 256);
}
