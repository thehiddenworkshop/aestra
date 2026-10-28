use super::*;
use crate::{EffectAsset, EffectCompiler, EffectPlayer, PlaybackHistoryPolicy};
use aestra_core::material::*;

#[test]
fn semantic_programs_are_shared_but_runtime_bindings_are_independent() {
    let mut program = MaterialProgram::additive_sprite("Shared shader");
    let parameter = aestra_core::MaterialParameterId::new();
    program.parameters.push(MaterialParameter {
        id: parameter,
        name: "Softness".into(),
        value_type: MaterialValueType::Float,
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Float(1.0)),
    });
    let material = aestra_core::MaterialId::new();
    let mut asset = EffectAsset::new("Semantic", 3.0);
    let mut emitter = crate::Emitter::basic_sprite("Stars", 3.0);
    emitter.renderers[0].material = material;
    asset.emitters.push(emitter);
    asset.material_instances.push(MaterialInstance {
        id: material,
        program: MaterialProgramRef::Project(program.id),
        values: BTreeMap::from([(
            parameter,
            MaterialParameterValue::Constant(MaterialValue::Float(0.2)),
        )]),
        render_state: MaterialRenderState::additive_sprite(),
    });
    let programs = BTreeMap::from([(program.id, program)]);
    let effect = Arc::new(
        EffectCompiler::default()
            .compile_with_material_programs(&asset, &programs)
            .unwrap(),
    );
    let mut cache = EffectPresentationCache::default();
    let mut a = cache.instantiate(effect.clone());
    let b = cache.instantiate(effect.clone());
    let shared = b.material_binding(material).unwrap().program().clone();
    assert!(Arc::ptr_eq(
        a.material_binding(material).unwrap().program(),
        &shared
    ));
    let mut binding = a.material_binding(material).unwrap().clone();
    binding
        .set_value(parameter, MaterialValue::Float(0.7))
        .unwrap();
    a.bind_material(material, binding);
    assert_eq!(
        a.material_binding(material).unwrap().values()[&parameter],
        MaterialValue::Float(0.7)
    );
    assert_eq!(
        b.material_binding(material).unwrap().values()[&parameter],
        MaterialValue::Float(0.2)
    );
    let c = cache.instantiate(effect);
    assert_eq!(
        c.material_binding(material).unwrap().values()[&parameter],
        MaterialValue::Float(0.2)
    );
    asset.name = "Replacement with stable IDs".into();
    let replacement = Arc::new(
        EffectCompiler::default()
            .compile_with_material_programs(&asset, &programs)
            .unwrap(),
    );
    let replaced = cache.instantiate(replacement);
    assert!(!Arc::ptr_eq(
        replaced.material_binding(material).unwrap().program(),
        &shared
    ));
}

#[test]
fn independent_players_share_one_template_and_retire_with_the_artifact() {
    let effect = Arc::new(
        EffectCompiler::default()
            .compile(&EffectAsset::new("Shared", 3.0))
            .unwrap(),
    );
    let weak = Arc::downgrade(&effect);
    let mut cache = EffectPresentationCache::default();
    let mut a = cache.instantiate(effect.clone());
    let b = cache.instantiate(effect.clone());
    a.instance.set_seed(42);
    a.instance.set_playback_time(1.0);
    a.set_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
    assert_eq!(b.simulation_time(), 0.0);
    assert_ne!(a.instance.seed(), b.instance.seed());
    assert_eq!(cache.templates.len(), 1);
    drop(effect);
    cache.prune_unused();
    assert_eq!(cache.templates.len(), 1);
    drop((a, b));
    cache.prune_unused();
    assert!(cache.templates.is_empty());
    assert!(weak.upgrade().is_none());
}

#[test]
fn same_id_replacement_gets_a_distinct_template() {
    let mut asset = EffectAsset::new("First", 3.0);
    let first = Arc::new(EffectCompiler::default().compile(&asset).unwrap());
    asset.name = "Replacement".into();
    let second = Arc::new(EffectCompiler::default().compile(&asset).unwrap());
    assert_eq!(first.source, second.source);
    let mut cache = EffectPresentationCache::default();
    let a = cache.instantiate(first);
    let b = cache.instantiate(second);
    assert_eq!(a.effect().name, "First");
    assert_eq!(b.effect().name, "Replacement");
    assert_eq!(cache.templates.len(), 2);
}

#[test]
fn plugin_preparation_copies_player_state_without_changing_the_template() {
    let effect = Arc::new(
        EffectCompiler::default()
            .compile(&EffectAsset::new("Player", 3.0))
            .unwrap(),
    );
    let mut app = App::new();
    app.init_resource::<EffectPresentationCache>()
        .add_systems(Update, crate::playback::prepare_player_presentations);
    let mut first = EffectPlayer::from_compiled(effect.clone());
    first.instance_mut().set_seed(73);
    first.seek(1.0);
    first.set_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
    let a = app.world_mut().spawn(first).id();
    let b = app
        .world_mut()
        .spawn(EffectPlayer::from_compiled(effect))
        .id();
    app.update();
    let first = app.world().get::<PresentedEffect>(a).unwrap();
    let second = app.world().get::<PresentedEffect>(b).unwrap();
    assert_eq!(first.instance.seed(), 73);
    assert_eq!(first.simulation_time(), 1.0);
    assert_eq!(second.simulation_time(), 0.0);
    assert_ne!(second.instance.seed(), 73);
    assert_eq!(
        app.world()
            .resource::<EffectPresentationCache>()
            .templates
            .len(),
        1
    );
    app.world_mut().despawn(a);
    app.world_mut().despawn(b);
    app.update();
    assert!(
        app.world()
            .resource::<EffectPresentationCache>()
            .templates
            .is_empty()
    );
}
