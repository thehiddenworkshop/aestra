//! S0 artifact round-trip baseline (see docs/new/aestra_foundation_tasks_S0_S1.md, task S0-A3).
//!
//! Locks the v2 compiled-artifact contract across the same representative showcase effects the
//! compiler baseline pins. For each fixture it asserts:
//!   - the encoded artifact carries the current format version and magic;
//!   - `decode(encode(compiled)) == compiled` (lossless round-trip);
//!   - re-encoding the decoded effect is byte-identical (deterministic serialization).
//!
//! A diff here means the compiled-artifact format changed. That is allowed only as a deliberate,
//! coordinated bump (see docs/new §44.5) — this test exists so an *accidental* change is caught
//! before the format is bumped for real.

use aestra_artifact::{ARTIFACT_MAGIC, CURRENT_ARTIFACT_VERSION, decode_effect, encode_effect};
use aestra_compiler::EffectCompiler;
use aestra_core::{EffectAsset, material::MaterialProgram};
use aestra_runtime::CompiledEffect;
use std::collections::BTreeMap;

fn compile_standalone(effect_ron: &str) -> CompiledEffect {
    let asset = EffectAsset::from_ron(effect_ron).expect("effect fixture parses");
    EffectCompiler::default()
        .compile(&asset)
        .expect("standalone effect compiles")
}

fn compile_with_material(effect_ron: &str, material_ron: &str) -> CompiledEffect {
    let asset = EffectAsset::from_ron(effect_ron).expect("effect fixture parses");
    let program = MaterialProgram::from_ron(material_ron).expect("material fixture parses");
    EffectCompiler::default()
        .compile_with_material_programs(&asset, &BTreeMap::from([(program.id, program)]))
        .expect("material-referencing effect compiles")
}

fn showcase() -> Vec<(&'static str, CompiledEffect)> {
    vec![
        (
            "prism_bloom",
            compile_standalone(include_str!(
                "../../../assets/test/effects/prism_bloom.aestra.ron"
            )),
        ),
        (
            "ember_sigil",
            compile_standalone(include_str!(
                "../../../assets/test/effects/ember_sigil.aestra.ron"
            )),
        ),
        (
            "plasma_burst",
            compile_standalone(include_str!(
                "../../../assets/test/effects/plasma_burst.aestra.ron"
            )),
        ),
        (
            "ribbon_lab",
            compile_with_material(
                include_str!("../../../assets/test/effects/ribbon_lab.aestra.ron"),
                include_str!("../../../assets/test/materials/ribbon_lab.aestra.material.ron"),
            ),
        ),
        (
            "trail_lab",
            compile_with_material(
                include_str!("../../../assets/test/effects/trail_lab.aestra.ron"),
                include_str!("../../../assets/test/materials/trail_lab.aestra.material.ron"),
            ),
        ),
        (
            "mesh_material_lab",
            compile_with_material(
                include_str!("../../../assets/test/effects/mesh_material_lab.aestra.ron"),
                include_str!(
                    "../../../assets/test/materials/mesh_material_lab.aestra.material.ron"
                ),
            ),
        ),
        (
            "material_graph_lab",
            compile_with_material(
                include_str!("../../../assets/test/effects/material_graph_lab.aestra.ron"),
                include_str!(
                    "../../../assets/test/materials/material_graph_lab.aestra.material.ron"
                ),
            ),
        ),
    ]
}

#[test]
fn showcase_effects_round_trip_through_the_versioned_artifact() {
    for (name, compiled) in showcase() {
        let bytes = encode_effect(&compiled).unwrap_or_else(|e| panic!("encode {name}: {e}"));
        let text = std::str::from_utf8(&bytes).expect("artifact is utf-8");
        assert!(
            text.contains(ARTIFACT_MAGIC),
            "{name} artifact carries the magic header"
        );
        assert!(
            text.contains(&format!("format_version:{CURRENT_ARTIFACT_VERSION}")),
            "{name} artifact is stamped with the current format version"
        );

        let restored = decode_effect(&bytes).unwrap_or_else(|e| panic!("decode {name}: {e}"));
        assert_eq!(restored, compiled, "{name} round-trips losslessly");

        let reencoded = encode_effect(&restored).unwrap();
        assert_eq!(
            reencoded, bytes,
            "{name} re-encodes byte-identically (deterministic serialization)"
        );
    }
}

#[test]
fn decoded_artifacts_evaluate_identically_to_their_source() {
    // Round-trip preserves behaviour, not just structure: the decoded artifact must produce the
    // exact same CPU-reference particles as the freshly compiled effect (ties S0-A3 to S0-A4).
    let (mut from_source, mut from_artifact) = (Vec::new(), Vec::new());
    for (name, compiled) in showcase() {
        let restored = decode_effect(&encode_effect(&compiled).unwrap()).unwrap();
        for &time in &[0.1_f32, 0.5, 1.0, 1.5] {
            aestra_runtime::evaluate(&compiled, time, 42, &mut from_source);
            aestra_runtime::evaluate(&restored, time, 42, &mut from_artifact);
            assert_eq!(
                from_source, from_artifact,
                "{name} @ {time}s evaluates differently after an artifact round-trip"
            );
        }
    }
}
