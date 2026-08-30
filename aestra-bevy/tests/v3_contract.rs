use aestra_bevy::{
    AssetDefinition, AssetError, AssetId, CurveId, EffectAsset, EffectCompiler, EffectId,
    EffectInstance, EffectProfile, Emitter, EmitterId, GradientId, MaterialProperties, ModuleId,
    ModuleParameters, ParticleSample, ProfileValue, RendererId,
};
use std::sync::Arc;

#[test]
fn immutable_v3_contract_preserves_its_public_shape() {
    let effect = contract_effect();

    assert_eq!(effect.format_version, 3);
    assert_eq!(effect.id, EffectId::from_u128(1));
    assert_eq!(effect.emitters.len(), 1);
    assert_eq!(effect.emitters[0].name, "Contract Emitter");
    assert_eq!(effect.emitters[0].modules.len(), 5);
    assert_eq!(effect.emitters[0].renderers.len(), 1);
}

#[test]
fn immutable_textured_contract_compiles_with_a_texture_path() {
    let effect = textured_contract_effect();
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
fn strict_loading_reports_that_format_v2_requires_a_migration_path() {
    let source = contract_effect().to_pretty_ron().unwrap().replacen(
        "format_version: 3",
        "format_version: 2",
        1,
    );
    let error = EffectAsset::from_ron(&source).expect_err("format v2 must be rejected");
    assert!(matches!(
        error,
        AssetError::UnsupportedFormat {
            found: 2,
            current: 3
        }
    ));
}

#[test]
fn immutable_v3_contract_has_stable_pretty_serialization() {
    let effect = contract_effect();
    let serialized = effect.to_pretty_ron().expect("v3 contract must serialize");

    assert_eq!(
        serialized.len(),
        6_001,
        "update only for an intentional format change"
    );
    assert_eq!(
        fnv1a64(serialized.as_bytes()),
        0x0ac7_aa40_6902_089c,
        "update only for an intentional format change"
    );
    assert_eq!(
        EffectAsset::from_ron(&serialized).expect("serialized v3 contract must reload"),
        effect
    );
}

#[test]
fn immutable_v3_contract_evaluation_matches_golden_moments() {
    let effect = contract_effect();
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

    // Git may materialize text fixtures with CRLF on Windows runners. The snapshot contract is
    // about evaluated particle data, not the checkout's line-ending policy.
    let expected = include_str!("fixtures/v3_contract.moments").replace("\r\n", "\n");
    assert_eq!(actual, expected);
}

fn contract_effect() -> EffectAsset {
    let mut effect = EffectAsset::new("V3 Contract", 3.0);
    effect.id = EffectId::from_u128(1);
    let mut emitter = Emitter::basic_sprite("Contract Emitter", effect.duration);
    emitter.id = EmitterId::from_u128(2);
    for (index, module) in emitter.modules.iter_mut().enumerate() {
        module.id = ModuleId::from_u128(10 + index as u128);
        if let ModuleParameters::Appearance {
            size,
            opacity,
            color,
        } = &mut module.parameters
        {
            size.id = CurveId::from_u128(20);
            opacity.id = CurveId::from_u128(21);
            color.id = GradientId::from_u128(22);
        }
    }
    emitter.renderers[0].id = RendererId::from_u128(30);
    effect.emitters.push(emitter);
    effect
}

fn textured_contract_effect() -> EffectAsset {
    let mut effect = contract_effect();
    effect.name = "Textured V3 Contract".into();
    let mut texture = AssetDefinition::texture("Contract Texture", "textures/contract.png");
    texture.id = AssetId::from_u128(40);
    let MaterialProperties::Sprite {
        texture: material_texture,
        ..
    } = &mut effect.materials[0].properties;
    *material_texture = Some(texture.id);
    effect.assets.push(texture);
    effect
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
