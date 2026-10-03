use aestra_authoring::{EffectDiff, SemanticTarget};
use aestra_core::{EffectAsset, Emitter, ParticlePointLightProperties, SceneOutputInstance};

#[test]
fn emitter_scene_output_only_edits_are_not_lost_from_semantic_diffs() {
    let mut before = EffectAsset::new("Embers", 2.0);
    before.emitters.push(Emitter::basic_sprite("embers", 2.0));
    let mut after = before.clone();
    after.emitters[0]
        .scene_outputs
        .push(SceneOutputInstance::particle_point_light(
            ParticlePointLightProperties::new(1000.0, 10.0),
        ));
    let diff = EffectDiff::between(&before, &after);
    assert_eq!(diff.changes.len(), 1);
    assert_eq!(diff.changes[0].path, "emitter.scene_outputs");
    assert_eq!(
        diff.changes[0].target,
        SemanticTarget::Emitter(after.emitters[0].id)
    );
    assert!(!EffectDiff::between(&after, &before).is_empty());
}
