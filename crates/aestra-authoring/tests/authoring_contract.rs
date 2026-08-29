use aestra_authoring::{
    ChangeKind, CommandError, CommandExecutor, CommandHistory, EffectCommand, EffectTransaction,
    LockState, Selection, SemanticTarget,
};
use aestra_core::{
    BlendMode, ColorKey, CurveKey, EffectAsset, EffectAssetRef, EffectClip, EffectClipSeed,
    EffectId, EffectParameter, Emitter, EmitterTransform, EventId, EventLink, EventTrigger,
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
fn emitter_transform_is_a_reversible_semantic_command() {
    let mut effect = test_effect();
    let emitter = effect.emitters[0].id;
    let transform = EmitterTransform {
        translation: [4.0, 5.0, 6.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [2.0, 2.0, 2.0],
    };
    let mut history = CommandHistory::default();
    history
        .execute(
            &mut effect,
            &LockState::default(),
            EffectTransaction::single(
                "Transform emitter",
                EffectCommand::SetEmitterTransform {
                    id: emitter,
                    transform,
                },
            ),
        )
        .unwrap();

    assert_eq!(effect.emitters[0].transform, transform);
    history.undo(&mut effect).unwrap();
    assert_eq!(effect.emitters[0].transform, EmitterTransform::default());
    history.redo(&mut effect).unwrap();
    assert_eq!(effect.emitters[0].transform, transform);
}

#[test]
fn material_edits_use_transactions_and_undo() {
    let mut effect = test_effect();
    let material_id = effect.materials[0].id;
    let mut replacement = effect.materials[0].clone();
    replacement.blend = BlendMode::Multiply;
    let transaction = EffectTransaction::single(
        "Change material blend",
        EffectCommand::SetMaterial {
            id: material_id,
            material: replacement,
        },
    );

    let outcome = CommandExecutor::execute(&mut effect, &LockState::default(), &transaction)
        .expect("material command must execute");
    assert_eq!(effect.materials[0].blend, BlendMode::Multiply);
    assert!(
        outcome
            .diff
            .changes
            .iter()
            .any(|change| change.path == "effect.materials")
    );

    CommandExecutor::execute(&mut effect, &LockState::default(), &outcome.inverse).unwrap();
    assert_eq!(effect.materials[0].blend, BlendMode::Additive);
}

#[test]
fn referenced_materials_cannot_be_removed() {
    let mut effect = test_effect();
    let before = effect.clone();
    let material = effect.materials[0].id;
    let error = CommandExecutor::execute(
        &mut effect,
        &LockState::default(),
        &EffectTransaction::single(
            "Remove material",
            EffectCommand::RemoveMaterial { id: material },
        ),
    )
    .unwrap_err();

    assert!(matches!(error, CommandError::Validation(_)));
    assert_eq!(effect, before);
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

#[test]
fn preview_is_non_mutating_and_commits_as_one_history_entry() {
    let mut effect = test_effect();
    let emitter = effect.emitters[0].id;
    let original = effect.clone();
    let transaction = EffectTransaction::new(
        "Retiming",
        vec![
            EffectCommand::SetEffectDuration { duration: 1.5 },
            EffectCommand::SetEmitterTiming {
                id: emitter,
                start_time: 0.0,
                duration: 1.5,
            },
        ],
    );

    let preview = CommandExecutor::preview(&effect, &LockState::default(), transaction).unwrap();
    assert_eq!(effect, original);
    assert_eq!(preview.candidate().duration, 1.5);
    assert_eq!(preview.diff().changes.len(), 2);

    let mut history = CommandHistory::default();
    history
        .commit_preview(&mut effect, &LockState::default(), preview)
        .unwrap();
    assert_eq!(effect.duration, 1.5);

    history.undo(&mut effect).unwrap().unwrap();
    assert_eq!(effect, original);
}

#[test]
fn stale_or_locked_previews_never_mutate_the_document() {
    let mut effect = test_effect();
    let emitter = effect.emitters[0].id;
    let preview = CommandExecutor::preview(
        &effect,
        &LockState::default(),
        EffectTransaction::single(
            "Rename emitter",
            EffectCommand::SetEmitterName {
                id: emitter,
                name: "Renamed".into(),
            },
        ),
    )
    .unwrap();
    effect.name = "Changed elsewhere".into();
    let before_commit = effect.clone();
    let mut history = CommandHistory::default();
    assert!(matches!(
        history.commit_preview(&mut effect, &LockState::default(), preview),
        Err(CommandError::StalePreview)
    ));
    assert_eq!(effect, before_commit);

    let module = effect.emitters[0]
        .module_by_type(MODULE_INITIALIZE)
        .unwrap()
        .id;
    let mut locks = LockState::default();
    locks.lock(SemanticTarget::Module(module));
    let locked_before = effect.clone();
    assert!(matches!(
        CommandExecutor::preview(
            &effect,
            &locks,
            EffectTransaction::single(
                "Change lifetime",
                EffectCommand::SetModuleParameter {
                    emitter,
                    module,
                    parameter: "lifetime".into(),
                    value: Value::Range(ScalarRange::new(0.2, 0.4)),
                },
            ),
        ),
        Err(CommandError::Locked { .. })
    ));
    assert_eq!(effect, locked_before);
}

#[test]
fn emitter_display_color_is_semantic_and_reversible() {
    let mut effect = test_effect();
    let emitter = effect.emitters[0].id;
    let original = effect.clone();
    let mut history = CommandHistory::default();

    history
        .execute(
            &mut effect,
            &LockState::default(),
            EffectTransaction::single(
                "Change timeline color",
                EffectCommand::SetEmitterDisplayColor {
                    id: emitter,
                    color: Some([0.25, 0.5, 0.75, 1.0]),
                },
            ),
        )
        .unwrap();
    assert_eq!(
        effect.emitters[0].display_color,
        Some([0.25, 0.5, 0.75, 1.0])
    );

    history.undo(&mut effect).unwrap().unwrap();
    assert_eq!(effect, original);
}

#[test]
fn effect_clip_commands_are_serializable_and_reversible() {
    let mut effect = test_effect();
    let clip = EffectClip::new(EffectId::from_u128(0xC11D), 0.25, 1.0);
    let clip_id = clip.id;
    let replacement = EffectAssetRef::new(EffectId::from_u128(0xC11E));
    let mut history = CommandHistory::default();

    let added = history
        .execute(
            &mut effect,
            &LockState::default(),
            EffectTransaction::single(
                "Add reusable effect",
                EffectCommand::AddEffectClip { clip, index: 0 },
            ),
        )
        .unwrap();
    assert!(added.changes.iter().any(|change| {
        change.kind == ChangeKind::Added && change.target == SemanticTarget::EffectClip(clip_id)
    }));

    history
        .execute(
            &mut effect,
            &LockState::default(),
            EffectTransaction::single(
                "Move reusable effect",
                EffectCommand::SetEffectClipTiming {
                    id: clip_id,
                    start_time: 0.5,
                    source_offset: 0.25,
                    duration: 1.25,
                },
            ),
        )
        .unwrap();
    history
        .execute(
            &mut effect,
            &LockState::default(),
            EffectTransaction::single(
                "Repair reusable effect source",
                EffectCommand::SetEffectClipSource {
                    id: clip_id,
                    source: replacement,
                },
            ),
        )
        .unwrap();
    history
        .execute(
            &mut effect,
            &LockState::default(),
            EffectTransaction::single(
                "Set reusable effect seed",
                EffectCommand::SetEffectClipSeed {
                    id: clip_id,
                    seed: EffectClipSeed::Fixed(77),
                },
            ),
        )
        .unwrap();

    let encoded = effect.to_pretty_ron().unwrap();
    let decoded = EffectAsset::from_ron(&encoded).unwrap();
    assert_eq!(decoded.effect_clips, effect.effect_clips);
    assert_eq!(effect.effect_clips[0].start_time, 0.5);
    assert_eq!(effect.effect_clips[0].source_offset, 0.25);
    assert_eq!(effect.effect_clips[0].duration, 1.25);
    assert_eq!(effect.effect_clips[0].seed, EffectClipSeed::Fixed(77));
    assert_eq!(effect.effect_clips[0].source, replacement);

    history.undo(&mut effect).unwrap().unwrap();
    assert_eq!(effect.effect_clips[0].seed, EffectClipSeed::Inherit);
    history.undo(&mut effect).unwrap().unwrap();
    assert_eq!(
        effect.effect_clips[0].source,
        EffectAssetRef::new(EffectId::from_u128(0xC11D))
    );
    history.undo(&mut effect).unwrap().unwrap();
    assert_eq!(effect.effect_clips[0].start_time, 0.25);
    history.undo(&mut effect).unwrap().unwrap();
    assert!(effect.effect_clips.is_empty());

    history.redo(&mut effect).unwrap().unwrap();
    history.redo(&mut effect).unwrap().unwrap();
    history.redo(&mut effect).unwrap().unwrap();
    history.redo(&mut effect).unwrap().unwrap();
    assert_eq!(effect.effect_clips[0].id, clip_id);
    assert_eq!(effect.effect_clips[0].seed, EffectClipSeed::Fixed(77));
    assert_eq!(effect.effect_clips[0].source, replacement);
}

#[test]
fn effect_clip_transform_and_order_are_undoable_stable_id_edits() {
    let mut effect = test_effect();
    let first = EffectClip::new(EffectId::from_u128(0xC11D), 0.0, 1.0);
    let first_id = first.id;
    let second = EffectClip::new(EffectId::from_u128(0xC11E), 0.5, 1.0);
    let second_id = second.id;
    effect.effect_clips = vec![first, second];
    let original = effect.clone();
    let mut history = CommandHistory::default();
    let transform = EmitterTransform {
        translation: [10.0, 2.0, -3.0],
        scale: [1.5, 1.5, 1.5],
        ..Default::default()
    };

    history
        .execute(
            &mut effect,
            &LockState::default(),
            EffectTransaction::single(
                "Transform clip",
                EffectCommand::SetEffectClipTransform {
                    id: first_id,
                    transform,
                },
            ),
        )
        .unwrap();
    history
        .execute(
            &mut effect,
            &LockState::default(),
            EffectTransaction::single(
                "Reorder clips",
                EffectCommand::MoveEffectClip {
                    id: first_id,
                    index: 1,
                },
            ),
        )
        .unwrap();

    assert_eq!(effect.effect_clips[0].id, second_id);
    assert_eq!(effect.effect_clips[1].id, first_id);
    assert_eq!(effect.effect_clips[1].transform, transform);
    history.undo(&mut effect).unwrap().unwrap();
    history.undo(&mut effect).unwrap().unwrap();
    assert_eq!(effect, original);
}

#[test]
fn invalid_effect_clip_edits_are_atomic_and_locked_clips_reject_edits() {
    let mut effect = test_effect();
    let clip = EffectClip::new(EffectId::from_u128(0xC11D), 0.25, 1.0);
    let clip_id = clip.id;
    effect.effect_clips.push(clip);
    let before = effect.clone();

    let invalid = EffectTransaction::single(
        "Move outside effect",
        EffectCommand::SetEffectClipTiming {
            id: clip_id,
            start_time: 1.5,
            source_offset: 0.0,
            duration: 1.0,
        },
    );
    assert!(matches!(
        CommandExecutor::execute(&mut effect, &LockState::default(), &invalid),
        Err(CommandError::Validation(_))
    ));
    assert_eq!(effect, before);

    let mut locks = LockState::default();
    locks.lock(SemanticTarget::EffectClip(clip_id));
    let locked = EffectTransaction::single(
        "Change seed",
        EffectCommand::SetEffectClipSeed {
            id: clip_id,
            seed: EffectClipSeed::Fixed(1),
        },
    );
    assert!(matches!(
        CommandExecutor::execute(&mut effect, &locks, &locked),
        Err(CommandError::Locked {
            target: SemanticTarget::EffectClip(id)
        }) if id == clip_id
    ));
    assert_eq!(effect, before);
}

#[test]
fn effect_clip_selection_repairs_after_deletion() {
    let mut effect = test_effect();
    let clip = EffectClip::new(EffectId::from_u128(0xC11D), 0.0, 1.0);
    let clip_id = clip.id;
    effect.effect_clips.push(clip);
    let mut selection = Selection::for_effect(&effect);
    selection.select_effect_clip(clip_id);
    assert_eq!(selection.effect_clip(), Some(clip_id));

    CommandExecutor::execute(
        &mut effect,
        &LockState::default(),
        &EffectTransaction::single(
            "Delete reusable effect",
            EffectCommand::RemoveEffectClip { id: clip_id },
        ),
    )
    .unwrap();
    selection.repair(&effect);

    assert_eq!(selection.effect_clip(), None);
    assert_eq!(selection.emitter(&effect), Some(effect.emitters[0].id));
}
