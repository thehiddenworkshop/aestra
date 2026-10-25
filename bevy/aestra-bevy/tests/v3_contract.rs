use aestra_bevy::{
    AssetDefinition, AssetError, AssetId, CurveId, EffectAsset, EffectCompiler, EffectId,
    EffectInstance, EffectPlaybackMode, EffectPlayer, EffectProfile, Emitter, EmitterId,
    GradientId, MaterialProperties, ModuleId, ModuleParameters, ParticleSample, ProfileValue,
    RendererId, ScalarRange,
};
use std::sync::Arc;

#[test]
fn immutable_v3_contract_preserves_its_public_shape() {
    let effect = contract_effect();

    assert_eq!(effect.format_version, 4);
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
fn strict_loading_rejects_a_non_current_format() {
    // Pre-release carries no migrations, so an older format is an explicit UnsupportedFormat error.
    let source = contract_effect().to_pretty_ron().unwrap().replacen(
        "format_version: 4",
        "format_version: 2",
        1,
    );
    let error = EffectAsset::from_ron(&source).expect_err("format v2 must be rejected");
    assert!(matches!(
        error,
        AssetError::UnsupportedFormat {
            found: 2,
            current: 4
        }
    ));
}

#[test]
fn immutable_v3_contract_has_stable_pretty_serialization() {
    let effect = contract_effect();
    let serialized = effect.to_pretty_ron().expect("v3 contract must serialize");

    // RON's default pretty newline is OS-dependent. The existing byte golden was
    // recorded with CRLF; preserve it exactly, normalizing only newline encoding.
    // Exercise both forms on every runner instead of testing only its native form.
    for source in [
        serialized.replace("\r\n", "\n"),
        canonical_contract_serialization(&serialized),
    ] {
        let canonical = canonical_contract_serialization(&source);
        assert_eq!(
            canonical.len(),
            6_923,
            "update only for an intentional format change"
        );
        assert_eq!(
            fnv1a64(canonical.as_bytes()),
            0x41d3_b334_ee6a_e057,
            "update only for an intentional format change"
        );
        assert_eq!(
            EffectAsset::from_ron(&source).expect("serialized v3 contract must reload"),
            effect
        );
    }
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
    compare_moment_snapshots(&actual, &expected)
        .unwrap_or_else(|error| panic!("{error}\nactual:\n{actual}\nexpected:\n{expected}"));
}

#[test]
fn golden_moment_comparison_accepts_reported_linux_roundoff() {
    let expected = include_str!("fixtures/v3_contract.moments");
    let linux = expected
        .replace("-40.254278", "-40.254277")
        .replace("252.819920", "252.819921")
        .replace("97.281041", "97.281040");
    assert_ne!(linux, expected);
    assert_eq!(compare_moment_snapshots(&linux, expected), Ok(()));
    assert_eq!(
        compare_moment_snapshots(&linux.replace("\r\n", "\n"), expected),
        Ok(())
    );
}

#[test]
fn golden_moment_comparison_rejects_contract_drift_and_nonfinite_values() {
    let expected = include_str!("fixtures/v3_contract.moments");
    for changed in [
        expected.replacen("count=12", "count=13", 1),
        expected.replacen("emitters=[12]", "emitters=[13]", 1),
        expected.replacen("t=0.125", "t=0.126", 1),
        expected.replacen("-40.254278", "-40.254268", 1),
        expected.replacen("size=48.000000", "size=48.000010", 1),
        expected.replacen("alpha=0.000000", "alpha=0.000010", 1),
        expected.replacen("-40.254278", "NaN", 1),
        expected.replacen("size=48.000000", "size=inf", 1),
        expected.replacen("alpha=0.000000", "alpha=-inf", 1),
        expected.replacen(" pos=", " position=", 1),
        expected.lines().take(5).collect::<Vec<_>>().join("\n"),
        format!("{expected}\n{}", expected.lines().next().unwrap()),
    ] {
        assert_ne!(changed, expected);
        assert!(
            compare_moment_snapshots(&changed, expected).is_err(),
            "contract drift was accepted: {changed}"
        );
    }
}

fn canonical_contract_serialization(source: &str) -> String {
    source.replace("\r\n", "\n").replace('\n', "\r\n")
}

fn compare_moment_snapshots(actual: &str, expected: &str) -> Result<(), String> {
    // Snapshots print six decimals. Allow at most two last-place decimal units
    // for cross-platform f32 math; never scale tolerance with aggregate magnitude.
    const MAX_ABSOLUTE_ERROR: f64 = 0.000_002;
    let actual: Vec<_> = actual.lines().collect();
    let expected: Vec<_> = expected.lines().collect();
    if actual.len() != expected.len() {
        return Err(format!(
            "moment count changed: {} != {}",
            actual.len(),
            expected.len()
        ));
    }
    for (index, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
        let (actual_prefix, actual) =
            moment_values(actual).ok_or_else(|| format!("malformed actual moment {index}"))?;
        let (expected_prefix, expected) =
            moment_values(expected).ok_or_else(|| format!("malformed golden moment {index}"))?;
        if actual_prefix != expected_prefix {
            return Err(format!(
                "moment {index} time/count/emitter counts changed: {actual_prefix} != {expected_prefix}"
            ));
        }
        for ((field, actual), expected) in
            ["position.x", "position.y", "position.z", "size", "alpha"]
                .into_iter()
                .zip(actual)
                .zip(expected)
        {
            if !actual.is_finite()
                || !expected.is_finite()
                || (actual - expected).abs() > MAX_ABSOLUTE_ERROR
            {
                return Err(format!(
                    "moment {index} {field}: {actual} != {expected} (absolute tolerance {MAX_ABSOLUTE_ERROR})"
                ));
            }
        }
    }
    Ok(())
}

fn moment_values(line: &str) -> Option<(&str, [f64; 5])> {
    let (prefix, values) = line.split_once(" pos=")?;
    let (position, values) = values.split_once(" size=")?;
    let (size, alpha) = values.split_once(" alpha=")?;
    let mut position = position.strip_prefix('(')?.strip_suffix(')')?.split(',');
    let values = [
        position.next()?.parse().ok()?,
        position.next()?.parse().ok()?,
        position.next()?.parse().ok()?,
        size.parse().ok()?,
        alpha.parse().ok()?,
    ];
    if position.next().is_some() {
        return None;
    }
    Some((prefix, values))
}

#[test]
fn continuous_loop_keeps_particles_from_the_previous_cycle_alive() {
    let mut effect = EffectAsset::new("Continuous", 1.0);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    let mut emitter = Emitter::basic_sprite("Emitter", 1.0);
    *emitter.spawn_rate_mut() = 0.0;
    *emitter.burst_count_mut() = 1;
    *emitter.lifetime_mut() = ScalarRange::new(2.0, 2.0);
    effect.emitters.push(emitter);

    let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
    let mut continuous = EffectInstance::new(compiled.clone());
    continuous.seek(1.1);
    let mut samples = Vec::new();
    continuous.evaluate(&mut samples);
    assert_eq!(samples.len(), 2);

    let mut player = EffectPlayer::from_compiled(compiled);
    player.seek_simulation_time(1.1);
    assert_eq!(player.instance().history_revision(), 0);
    assert_ne!(player.instance().history_epoch(), 0);
    assert!((player.elapsed() - 0.1).abs() < 0.0001);
    assert!((player.simulation_time() - 1.1).abs() < 0.0001);
    player.instance().evaluate(&mut samples);
    assert_eq!(samples.len(), 2);

    let mut restart_effect = effect;
    restart_effect.playback_mode = EffectPlaybackMode::LoopRestart;
    let mut restart = EffectInstance::new(Arc::new(
        EffectCompiler::default().compile(&restart_effect).unwrap(),
    ));
    restart.seek(1.1);
    restart.evaluate(&mut samples);
    assert_eq!(samples.len(), 1);
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
