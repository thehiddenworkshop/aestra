use aestra_authoring::{
    CommandError, CommandExecutor, CommandHistory, EffectCommand, EffectTransaction, LockState,
    Selection, SemanticTarget,
};
use aestra_core::{
    ColorKey, CurveKey, EffectAsset, EffectParameter, Emitter, EventId, EventLink, EventTrigger,
    MODULE_APPEARANCE, MODULE_EMISSION, MODULE_INITIALIZE, ParameterId, ScalarRange, Value,
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
fn module_stack_duplicate_delete_and_undo_are_semantic_commands() {
    let mut effect = test_effect();
    let emitter = effect.emitters[0].id;
    let original = effect.emitters[0]
        .module_by_type(MODULE_EMISSION)
        .unwrap()
        .id;
    let duplicate = EffectCommand::duplicate_module(&effect, emitter, original).unwrap();
    let duplicate_id = match &duplicate {
        EffectCommand::AddModule { module, .. } => module.id,
        _ => unreachable!(),
    };
    let outcome = CommandExecutor::execute(
        &mut effect,
        &LockState::default(),
        &EffectTransaction::single("Duplicate module", duplicate),
    )
    .unwrap();
    assert_eq!(effect.emitters[0].modules.len(), 6);
    assert_ne!(original, duplicate_id);

    CommandExecutor::execute(&mut effect, &LockState::default(), &outcome.inverse).unwrap();
    assert_eq!(effect.emitters[0].modules.len(), 5);

    CommandExecutor::execute(
        &mut effect,
        &LockState::default(),
        &EffectTransaction::single(
            "Delete required module",
            EffectCommand::RemoveModule {
                emitter,
                module: original,
            },
        ),
    )
    .expect("authoring permits compiler-invalid intermediate stacks");
    assert!(
        effect
            .validation_report()
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == aestra_core::DiagnosticCode::MissingModule)
    );
}

#[test]
fn curve_and_gradient_key_commands_are_atomic_and_reversible() {
    let mut effect = test_effect();
    let emitter = effect.emitters[0].id;
    let module = effect.emitters[0]
        .module_by_type(MODULE_APPEARANCE)
        .unwrap()
        .id;
    let curve_before = effect.emitters[0].size_curve().clone();
    let gradient_before = effect.emitters[0].color_gradient().clone();
    let transaction = EffectTransaction::new(
        "Edit appearance keys",
        vec![
            EffectCommand::SetCurveKey {
                emitter,
                module,
                parameter: "size".into(),
                index: 1,
                key: CurveKey::new(0.4, 14.0),
            },
            EffectCommand::SetGradientKey {
                emitter,
                module,
                parameter: "color".into(),
                index: 1,
                key: ColorKey::new(0.6, [1.0, 0.2, 0.5, 1.0]),
            },
        ],
    );
    let outcome = CommandExecutor::execute(&mut effect, &LockState::default(), &transaction)
        .expect("valid key edits must execute");
    assert_eq!(effect.emitters[0].size_curve().keys[1].value, 14.0);
    assert_eq!(effect.emitters[0].color_gradient().keys[1].time, 0.6);

    CommandExecutor::execute(&mut effect, &LockState::default(), &outcome.inverse).unwrap();
    assert_eq!(effect.emitters[0].size_curve(), &curve_before);
    assert_eq!(effect.emitters[0].color_gradient(), &gradient_before);

    let invalid = EffectTransaction::single(
        "Break key ordering",
        EffectCommand::SetCurveKey {
            emitter,
            module,
            parameter: "size".into(),
            index: 1,
            key: CurveKey::new(1.1, 4.0),
        },
    );
    let before = effect.clone();
    assert!(matches!(
        CommandExecutor::execute(&mut effect, &LockState::default(), &invalid),
        Err(CommandError::Validation(_))
    ));
    assert_eq!(effect, before);
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

#[test]
fn parameter_bindings_are_transactional_and_reversible() {
    let mut effect = test_effect();
    let emitter = effect.emitters[0].id;
    let module = effect.emitters[0]
        .module_by_type(MODULE_EMISSION)
        .unwrap()
        .id;
    let parameter = EffectParameter {
        id: ParameterId::new(),
        name: "Spawn Rate".into(),
        default: Value::Scalar(30.0),
        exposed: true,
    };
    let parameter_id = parameter.id;
    let mut history = CommandHistory::default();
    let transaction = EffectTransaction::new(
        "Expose spawn rate",
        vec![
            EffectCommand::AddParameter {
                parameter,
                index: 0,
            },
            EffectCommand::BindModuleParameter {
                emitter,
                module,
                parameter: "spawn_rate".into(),
                source: parameter_id,
            },
        ],
    );

    history
        .execute(&mut effect, &LockState::default(), transaction)
        .unwrap();
    assert_eq!(
        effect.emitters[0].module_by_id(module).unwrap().bindings["spawn_rate"],
        parameter_id
    );

    history.undo(&mut effect).unwrap().unwrap();
    assert!(effect.parameters.is_empty());
    assert!(
        effect.emitters[0]
            .module_by_id(module)
            .unwrap()
            .bindings
            .is_empty()
    );

    history.redo(&mut effect).unwrap().unwrap();
    assert_eq!(effect.parameters[0].id, parameter_id);
    assert_eq!(
        effect.emitters[0].module_by_id(module).unwrap().bindings["spawn_rate"],
        parameter_id
    );
}

#[test]
fn invalid_binding_type_leaves_the_document_unchanged() {
    let mut effect = test_effect();
    let parameter = EffectParameter {
        id: ParameterId::new(),
        name: "Gravity".into(),
        default: Value::Vec2([0.0, -9.8]),
        exposed: true,
    };
    let parameter_id = parameter.id;
    effect.parameters.push(parameter);
    let emitter = effect.emitters[0].id;
    let module = effect.emitters[0]
        .module_by_type(MODULE_EMISSION)
        .unwrap()
        .id;
    let before = effect.clone();

    let error = CommandExecutor::execute(
        &mut effect,
        &LockState::default(),
        &EffectTransaction::single(
            "Bind wrong type",
            EffectCommand::BindModuleParameter {
                emitter,
                module,
                parameter: "spawn_rate".into(),
                source: parameter_id,
            },
        ),
    )
    .unwrap_err();

    assert!(matches!(error, CommandError::Validation(_)));
    assert_eq!(effect, before);
}
