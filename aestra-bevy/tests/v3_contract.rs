use aestra_bevy::{
    AssetError, EffectAsset, EffectCompiler, EffectInstance, EffectProfile, ParticleSample,
    ProfileValue,
};
use std::sync::Arc;

const V3_REFERENCE: &str = include_str!("../../assets/effects/prism_bloom.aestra.ron");
const TEXTURED_REFERENCE: &str = include_str!("../../assets/effects/ember_sigil.aestra.ron");

#[test]
fn bundled_v3_asset_preserves_its_public_shape() {
    let effect = EffectAsset::from_ron(V3_REFERENCE).expect("v3 reference asset must load");

    assert_eq!(effect.format_version, 3);
    assert_eq!(
        effect.id.to_string(),
        "8f245a4d-4c55-4d8b-a404-09a20cf67a01"
    );
    assert_eq!(effect.emitters.len(), 4);
    assert_eq!(effect.events.len(), 2);
    assert_eq!(
        effect
            .emitters
            .iter()
            .map(|emitter| emitter.name.as_str())
            .collect::<Vec<_>>(),
        [
            "Prism Core",
            "Spectrum Shards",
            "Floating Dust",
            "Bloom Ring",
        ]
    );
    assert!(
        effect
            .emitters
            .iter()
            .all(|emitter| { emitter.modules.len() == 5 && !emitter.renderers.is_empty() })
    );
}

#[test]
fn bundled_textured_effect_compiles_with_multiple_renderer_paths() {
    let effect = EffectAsset::from_ron(TEXTURED_REFERENCE).unwrap();
    let compiled = EffectCompiler::default().compile(&effect).unwrap();

    assert_eq!(compiled.assets.len(), 1);
    assert_eq!(compiled.emitters[0].renderers.len(), 1);
    assert!(
        compiled
            .material(compiled.emitters[0].renderers[0].material)
            .unwrap()
            .texture
            .is_some()
    );
    let profile = EffectProfile::from_compiled(&compiled);
    assert_eq!(profile.texture_sample_count, ProfileValue::Estimated(1));
    assert_eq!(profile.texture_memory_bytes, ProfileValue::Unavailable);
}

#[test]
fn format_v2_is_rejected_without_a_legacy_path() {
    let source = V3_REFERENCE.replacen("format_version: 3", "format_version: 2", 1);
    let error = EffectAsset::from_ron(&source).expect_err("format v2 must be rejected");
    assert!(matches!(error, AssetError::Validation(_)));
}

#[test]
fn bundled_v3_asset_has_stable_pretty_serialization() {
    let effect = EffectAsset::from_ron(V3_REFERENCE).expect("v3 reference asset must load");
    let serialized = effect
        .to_pretty_ron()
        .expect("v3 reference asset must serialize");

    assert_eq!(
        serialized.len(),
        23_310,
        "update only for an intentional format change"
    );
    assert_eq!(
        fnv1a64(serialized.as_bytes()),
        0x6fad_5d0e_7a4d_d7b2,
        "update only for an intentional format change"
    );
    assert_eq!(
        EffectAsset::from_ron(&serialized).expect("serialized v3 reference must reload"),
        effect
    );
}

#[test]
fn bundled_v3_evaluation_matches_golden_moments() {
    let effect = EffectAsset::from_ron(V3_REFERENCE).expect("v3 reference asset must load");
    let compiled = EffectCompiler::default()
        .compile(&effect)
        .expect("v3 reference asset must compile");
    let mut instance = EffectInstance::new(Arc::new(compiled));
    let mut samples = Vec::new();
    let mut actual = String::new();

    for time in [0.0_f32, 0.125, 0.5, 1.0, 2.0, 2.75] {
        instance.seek(time);
        instance.evaluate(&mut samples);
        actual.push_str(&snapshot_line(time, effect.emitters.len(), &samples));
    }

    assert_eq!(actual, include_str!("fixtures/v3_prism_bloom.moments"));
}

fn snapshot_line(time: f32, emitter_count: usize, samples: &[ParticleSample]) -> String {
    let mut per_emitter = vec![0_usize; emitter_count];
    let mut position = [0.0_f64; 3];
    let mut size = 0.0_f64;
    let mut alpha = 0.0_f64;

    for sample in samples {
        per_emitter[sample.emitter_index] += 1;
        for (total, component) in position.iter_mut().zip(sample.position) {
            *total += f64::from(component);
        }
        size += f64::from(sample.size);
        alpha += f64::from(sample.color[3]);
    }

    format!(
        "t={time:.3} count={} emitters={per_emitter:?} pos=({:.6},{:.6},{:.6}) size={size:.6} alpha={alpha:.6}\n",
        samples.len(),
        position[0],
        position[1],
        position[2],
    )
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
