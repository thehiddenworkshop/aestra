use aestra_authoring::{
    CommandError, CommandExecutor, CommandHistory, EffectCommand, EffectTransaction, LockState,
    Selection, SemanticTarget,
};
use aestra_core::{
    EffectAsset, Emitter, EventId, EventLink, EventTrigger, MODULE_EMISSION, MODULE_INITIALIZE,
    ScalarRange, Value,
};

fn test_effect() -> EffectAsset {
    let mut effect = EffectAsset::new("Authoring Test", 2.0);
    effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
    effect
}

#[test]
fn semantic_parameter_command_executes_without_ui() {
    let mut effect = test_effect();
    let emitter = effect.emitters[0].id;
    let module = effect.emitters[0]
        .module_by_type(MODULE_EMISSION)
        .unwrap()
        .id;
    let transaction = EffectTransaction::single(
        "Set spawn rate",
        EffectCommand::SetModuleParameter {
            emitter,
            module,
            parameter: "spawn_rate".into(),
            value: Value::Scalar(72.0),
        },
    );

    let outcome = CommandExecutor::execute(&mut effect, &LockState::default(), &transaction)
        .expect("command must execute");

    assert_eq!(effect.emitters[0].spawn_rate(), 72.0);
    assert!(!outcome.diff.is_empty());
    assert_eq!(outcome.inverse.commands.len(), 1);
}

#[test]
fn failed_transaction_is_atomic() {
    let mut effect = test_effect();
    let before = effect.clone();
    let transaction = EffectTransaction::new(
        "Invalid edit",
        vec![
            EffectCommand::SetEffectName {
                name: "Should Roll Back".into(),
            },
            EffectCommand::SetEffectDuration { duration: 0.0 },
        ],
    );

    let error = CommandExecutor::execute(&mut effect, &LockState::default(), &transaction)
        .expect_err("invalid transaction must fail");

    assert!(matches!(error, CommandError::Validation(_)));
    assert_eq!(effect, before);
}

#[test]
fn transaction_validates_only_after_all_commands() {
    let mut effect = test_effect();
    let emitter = effect.emitters[0].id;
    let transaction = EffectTransaction::new(
        "Shorten effect",
        vec![
            EffectCommand::SetEffectDuration { duration: 1.0 },
            EffectCommand::SetEmitterTiming {
                id: emitter,
                start_time: 0.0,
                duration: 1.0,
            },
        ],
    );

    CommandExecutor::execute(&mut effect, &LockState::default(), &transaction).unwrap();

    assert_eq!(effect.duration, 1.0);
    assert_eq!(effect.emitters[0].duration, 1.0);
}

#[test]
fn command_history_preserves_ids_across_undo_and_redo() {
    let mut effect = test_effect();
    let source = effect.emitters[0].id;
    let command = EffectCommand::duplicate_emitter(&effect, source).unwrap();
    let duplicate = match &command {
        EffectCommand::AddEmitter { emitter, .. } => emitter.id,
        _ => unreachable!(),
    };
    let mut history = CommandHistory::default();

    history
        .execute(
            &mut effect,
            &LockState::default(),
            EffectTransaction::single("Duplicate emitter", command),
        )
        .unwrap();
    assert_eq!(effect.emitters[1].id, duplicate);

    history.undo(&mut effect).unwrap().unwrap();
    assert_eq!(effect.emitters.len(), 1);

    history.redo(&mut effect).unwrap().unwrap();
    assert_eq!(effect.emitters[1].id, duplicate);
}

#[test]
fn locked_module_rejects_parameter_changes() {
    let mut effect = test_effect();
    let emitter = effect.emitters[0].id;
    let module = effect.emitters[0]
        .module_by_type(MODULE_INITIALIZE)
        .unwrap()
        .id;
    let mut locks = LockState::default();
    locks.lock(SemanticTarget::Module(module));
    let transaction = EffectTransaction::single(
        "Change lifetime",
        EffectCommand::SetModuleParameter {
            emitter,
            module,
            parameter: "lifetime".into(),
            value: Value::Range(ScalarRange::new(0.1, 0.2)),
        },
    );

    let error = CommandExecutor::execute(&mut effect, &locks, &transaction).unwrap_err();

    assert!(matches!(
        error,
        CommandError::Locked {
            target: SemanticTarget::Module(id)
        } if id == module
    ));
}

#[test]
fn locked_curve_rejects_replacement_through_its_module() {
    let mut effect = test_effect();
    let emitter = effect.emitters[0].id;
    let module = effect.emitters[0]
        .module_by_type(aestra_core::MODULE_APPEARANCE)
        .unwrap()
        .id;
    let curve = effect.emitters[0].size_curve().clone();
    let mut replacement = curve.clone();
    replacement.keys[0].value = 99.0;
    let mut locks = LockState::default();
    locks.lock(SemanticTarget::Curve(curve.id));

    let error = CommandExecutor::execute(
        &mut effect,
        &locks,
        &EffectTransaction::single(
            "Replace size curve",
            EffectCommand::SetModuleParameter {
                emitter,
                module,
                parameter: "size".into(),
                value: Value::Curve(replacement),
            },
        ),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        CommandError::Locked {
            target: SemanticTarget::Curve(id)
        } if id == curve.id
    ));
}

#[test]
fn deleting_an_emitter_removes_and_restores_connected_events() {
    let mut effect = test_effect();
    let second = Emitter::basic_sprite("Second", 2.0);
    let source = effect.emitters[0].id;
    let target = second.id;
    effect.emitters.push(second);
    let event = EventLink {
        id: EventId::new(),
        source,
        trigger: EventTrigger::OnDeath,
        target,
    };
    effect.events.push(event.clone());
    let mut history = CommandHistory::default();

    history
        .execute(
            &mut effect,
            &LockState::default(),
            EffectTransaction::single(
                "Delete emitter",
                EffectCommand::RemoveEmitter { id: source },
            ),
        )
        .unwrap();
    assert!(effect.events.is_empty());

    history.undo(&mut effect).unwrap().unwrap();
    assert_eq!(effect.emitters[0].id, source);
    assert_eq!(effect.events, vec![event]);
}

#[test]
fn semantic_selection_repairs_after_deletion() {
    let mut effect = test_effect();
    effect.emitters.push(Emitter::basic_sprite("Second", 2.0));
    let removed = effect.emitters[0].id;
    let remaining = effect.emitters[1].id;
    let mut selection = Selection::for_effect(&effect);
    assert_eq!(selection.emitter(&effect), Some(removed));

    CommandExecutor::execute(
        &mut effect,
        &LockState::default(),
        &EffectTransaction::single("Delete", EffectCommand::RemoveEmitter { id: removed }),
    )
    .unwrap();
    selection.repair(&effect);

    assert_eq!(selection.emitter(&effect), Some(remaining));
}
