use aestra_authoring::{
    CommandExecutor, CommandHistory, EffectCommand, EffectTransaction, LockState, Selection,
    SemanticTarget,
};
use aestra_core::{
    EffectAsset, Emitter, EventAggregation, EventDefinition, EventTrigger, ParticleOutputRoute,
    PointLightBinding, PointLightPulse,
};

fn fixture() -> (EffectAsset, PointLightBinding) {
    let mut effect = EffectAsset::new("Lights", 4.0);
    effect.emitters.push(Emitter::basic_sprite("Shell", 4.0));
    let output = EventDefinition::new("break");
    let route = ParticleOutputRoute::new(effect.emitters[0].id, EventTrigger::OnDeath, output.id);
    let light = PointLightBinding::new(
        route.id,
        PointLightPulse::flash([1.0; 3], 500_000.0, 80.0, 0.6),
    );
    effect.event_outputs.push(output);
    effect.particle_outputs.push(route);
    (effect, light)
}

#[test]
fn point_light_only_edits_have_semantic_diffs_history_and_source_roundtrip() {
    let (mut effect, light) = fixture();
    let original = effect.clone();
    let mut history = CommandHistory::default();
    let locks = LockState::default();
    let diff = history
        .execute(
            &mut effect,
            &locks,
            EffectTransaction::single(
                "Add light",
                EffectCommand::AddPointLight {
                    binding: light.clone(),
                    index: 0,
                },
            ),
        )
        .unwrap();
    assert_eq!(diff.changes.len(), 1);
    assert_eq!(diff.changes[0].target, SemanticTarget::PointLight(light.id));
    assert_eq!(diff.changes[0].path, "effect.point_lights[0]");
    let mut changed = light.clone();
    changed.pulse.duration_seconds = 1.0;
    let diff = history
        .execute(
            &mut effect,
            &locks,
            EffectTransaction::single(
                "Edit light",
                EffectCommand::SetPointLight {
                    id: light.id,
                    binding: changed.clone(),
                },
            ),
        )
        .unwrap();
    assert!(!diff.is_empty());
    assert_eq!(
        EffectAsset::from_ron(&effect.to_pretty_ron().unwrap()).unwrap(),
        effect
    );
    history.undo(&mut effect).unwrap();
    assert_eq!(effect.point_lights, vec![light.clone()]);
    history.redo(&mut effect).unwrap();
    assert_eq!(effect.point_lights, vec![changed.clone()]);
    history
        .execute(
            &mut effect,
            &locks,
            EffectTransaction::single(
                "Remove light",
                EffectCommand::RemovePointLight { id: light.id },
            ),
        )
        .unwrap();
    assert!(effect.point_lights.is_empty());
    history.undo(&mut effect).unwrap();
    assert_eq!(effect.point_lights, vec![changed]);
    history.undo(&mut effect).unwrap();
    history.undo(&mut effect).unwrap();
    assert_eq!(effect, original);
}

#[test]
fn incompatible_routes_and_deleting_dependencies_are_atomic_not_silently_repaired() {
    let (mut effect, light) = fixture();
    effect.point_lights.push(light.clone());
    let original = effect.clone();
    let mut route = effect.particle_outputs[0].clone();
    route.aggregation = EventAggregation::EachEvent { limit: 4 };
    for command in [
        EffectCommand::SetParticleOutput {
            id: route.id,
            route,
        },
        EffectCommand::RemoveParticleOutput { id: light.route },
        EffectCommand::RemoveEmitter {
            id: effect.emitters[0].id,
        },
    ] {
        assert!(
            CommandExecutor::execute(
                &mut effect,
                &LockState::default(),
                &EffectTransaction::single("Invalid", command)
            )
            .is_err()
        );
        assert_eq!(effect, original);
    }
    // Explicitly remove the binding and its route as one edit, then undo in dependency order.
    let transaction = EffectTransaction::new(
        "Remove route and light",
        vec![
            EffectCommand::RemovePointLight { id: light.id },
            EffectCommand::RemoveParticleOutput { id: light.route },
        ],
    );
    let outcome =
        CommandExecutor::execute(&mut effect, &LockState::default(), &transaction).unwrap();
    CommandExecutor::execute(&mut effect, &LockState::default(), &outcome.inverse).unwrap();
    assert_eq!(effect, original);
}

#[test]
fn binding_identity_locks_and_selection_are_stable() {
    let (mut effect, light) = fixture();
    effect.point_lights.push(light.clone());
    let mut replacement = light.clone();
    replacement.id = aestra_core::EventRouteId::new();
    replacement.pulse.radius = 0.2;
    let command = EffectCommand::SetPointLight {
        id: light.id,
        binding: replacement,
    };
    let mut locks = LockState::default();
    locks.lock(SemanticTarget::PointLight(light.id));
    assert!(
        CommandExecutor::execute(
            &mut effect,
            &locks,
            &EffectTransaction::single("Edit", command.clone())
        )
        .is_err()
    );
    CommandExecutor::execute(
        &mut effect,
        &LockState::default(),
        &EffectTransaction::single("Edit", command),
    )
    .unwrap();
    assert_eq!(effect.point_lights[0].id, light.id);
    let mut selection = Selection {
        primary: SemanticTarget::PointLight(light.id),
    };
    selection.repair(&effect);
    assert_eq!(selection.primary, SemanticTarget::PointLight(light.id));
    effect.point_lights.clear();
    selection.repair(&effect);
    assert_ne!(selection.primary, SemanticTarget::PointLight(light.id));
}
