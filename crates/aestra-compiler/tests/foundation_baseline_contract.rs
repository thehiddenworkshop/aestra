//! S0 regression baseline (see docs/new/aestra_foundation_tasks_S0_S1.md, task S0-A2).
//!
//! A characterization net over the representative showcase effects: it pins the *structural*
//! shape of each fixture's compiled output — emitter lifecycle instruction counts, renderer plan
//! kinds, seek mode, and portable requirements. The shared-foundation refactor (S1) must not change
//! any of these; a diff here means the foundation work silently altered existing effects, which S0
//! exists to catch.
//!
//! This is deliberately coarse (counts + kinds, not full instruction vectors) so it flags real
//! structural drift without churning on every incidental lowering tweak.

use aestra_compiler::EffectCompiler;
use aestra_core::{EffectAsset, material::MaterialProgram};
use aestra_runtime::CompiledEffect;
use std::collections::BTreeMap;

fn compile_standalone(effect_ron: &str) -> CompiledEffect {
    let asset = EffectAsset::from_ron(effect_ron).expect("effect fixture parses");
    assert!(
        asset.validation_report().diagnostics.is_empty(),
        "fixture authored clean (no validation diagnostics)"
    );
    EffectCompiler::default()
        .compile(&asset)
        .expect("standalone effect compiles")
}

fn compile_with_material(effect_ron: &str, material_ron: &str) -> CompiledEffect {
    let asset = EffectAsset::from_ron(effect_ron).expect("effect fixture parses");
    assert!(
        asset.validation_report().diagnostics.is_empty(),
        "fixture authored clean (no validation diagnostics)"
    );
    let program = MaterialProgram::from_ron(material_ron).expect("material fixture parses");
    EffectCompiler::default()
        .compile_with_material_programs(&asset, &BTreeMap::from([(program.id, program)]))
        .expect("material-referencing effect compiles")
}

/// A compact, deterministic structural fingerprint of a compiled effect.
fn fingerprint(compiled: &CompiledEffect) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let requirements = &compiled.requirements;
    let _ = writeln!(
        out,
        "seek={:?} max_particles={} emitters={} source_map={}",
        compiled.seek_mode,
        compiled.max_particles,
        compiled.emitters.len(),
        compiled.source_map.len(),
    );
    let _ = writeln!(
        out,
        "req: gpu_sim={} native_present={} renderers={:?}",
        requirements.gpu_simulation, requirements.native_gpu_presentation, requirements.renderers,
    );
    for emitter in &compiled.emitters {
        let kinds: Vec<&str> = emitter
            .renderers
            .iter()
            .map(|renderer| renderer_kind(renderer))
            .collect();
        let _ = writeln!(
            out,
            "  [{}] enabled={} eu={} ps={} pu={} renderers={:?}",
            emitter.name,
            emitter.enabled,
            emitter.execution.emitter_update.len(),
            emitter.execution.particle_spawn.len(),
            emitter.execution.particle_update.len(),
            kinds,
        );
    }
    out
}

/// Stable variant label for a renderer plan (avoids pinning float parameters).
fn renderer_kind(renderer: &aestra_runtime::RendererPlan) -> &'static str {
    use aestra_runtime::RendererPlanKind::*;
    match renderer.kind {
        Sprite => "Sprite",
        Flipbook { .. } => "Flipbook",
        Trail { .. } => "Trail",
        Ribbon { .. } => "Ribbon",
        Mesh { .. } => "Mesh",
    }
}

/// The representative showcase fixtures shared by every S0 baseline test in this file.
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
fn showcase_effects_compile_to_a_stable_structural_baseline() {
    let cases = showcase();
    let report = cases
        .iter()
        .map(|(name, compiled)| format!("== {name} ==\n{}", fingerprint(compiled).trim_end()))
        .collect::<Vec<_>>()
        .join("\n");
    // Print the actual baseline so a blessed value can be captured (`--nocapture`).
    eprintln!("\n{report}");

    // Coarse invariants that must hold for every showcase effect regardless of the exact numbers.
    for (name, compiled) in &cases {
        assert!(
            !compiled.emitters.is_empty(),
            "{name} lowered at least one emitter"
        );
        assert!(
            compiled.max_particles > 0,
            "{name} has a bounded particle budget"
        );
    }

    // Blessed structural baseline from `main` (foundation_baseline.txt). Regenerate intentionally
    // (never to paper over an unexpected diff) by running with `--nocapture` and updating the file.
    let expected = include_str!("foundation_baseline.txt");
    assert_eq!(report.trim_end(), expected.trim_end());
}

/// Canonical sample times (seconds) and seed for the CPU-reference baseline.
const FRAMES: [f32; 4] = [0.1, 0.5, 1.0, 1.5];
const SEED: u64 = 42;

#[test]
fn showcase_effects_have_deterministic_cpu_evaluation() {
    let cases = showcase();
    let mut report = Vec::new();
    let (mut first, mut second) = (Vec::new(), Vec::new());
    for (name, compiled) in &cases {
        for &time in &FRAMES {
            aestra_runtime::evaluate(compiled, time, SEED, &mut first);
            aestra_runtime::evaluate(compiled, time, SEED, &mut second);
            // Re-evaluating the same (effect, time, seed) is bit-identical — the core determinism
            // guarantee the whole hybrid seek model depends on.
            assert_eq!(
                first, second,
                "{name} @ {time}s is not deterministic across identical evaluations"
            );
            // Alive-particle count is a platform-stable integer; pin it so S1 cannot change how many
            // particles an existing effect presents at a canonical time.
            report.push(format!("{name} t={time} n={}", first.len()));
        }
    }
    let report = report.join("\n");
    eprintln!("\n{report}");

    let expected = include_str!("foundation_cpu_baseline.txt");
    assert_eq!(report.trim_end(), expected.trim_end());
}
