use aestra_compiler::{EffectCompiler, MaterialCompiler, MaterialGraphCreateKind};
use aestra_core::{
    EffectAsset, RendererProperties,
    material::{MaterialDomain, MaterialInput, MaterialProgram},
};
use aestra_runtime::{
    BackendCapabilities, CompatibilityTarget, RendererCapability, RendererPlanKind,
};
use std::collections::BTreeMap;

fn fixture() -> (EffectAsset, MaterialProgram) {
    (
        EffectAsset::from_ron(include_str!(
            "../../../assets/effects/ribbon_lab.aestra.ron"
        ))
        .unwrap(),
        MaterialProgram::from_ron(include_str!(
            "../../../assets/materials/ribbon_lab.aestra.material.ron"
        ))
        .unwrap(),
    )
}

#[test]
fn ribbon_lab_compiles_and_requires_native_presentation() {
    let (effect, program) = fixture();
    let compiled = EffectCompiler::default()
        .compile_with_material_programs(&effect, &BTreeMap::from([(program.id, program)]))
        .unwrap();
    assert!(matches!(
        compiled.emitters[0].renderers[0].kind,
        RendererPlanKind::Ribbon { width: 1.0 }
    ));
    assert_eq!(
        aestra_runtime::EffectProfile::from_compiled(&compiled).dispatch_count,
        aestra_runtime::ProfileValue::Estimated(3)
    );
    assert!(
        compiled
            .requirements
            .renderers
            .contains(&RendererCapability::RibbonParticles)
    );
    for target in [
        CompatibilityTarget::CpuReference,
        CompatibilityTarget::GpuReadback,
    ] {
        assert!(
            !compiled
                .requirements
                .compatibility_report(&BackendCapabilities::default(), target)
                .is_compatible()
        );
    }
}

#[test]
fn ribbon_inputs_are_domain_specific_and_width_must_be_positive() {
    let (effect, program) = fixture();
    for input in [MaterialInput::RibbonUv, MaterialInput::RibbonDirection] {
        assert!(
            MaterialCompiler
                .plan_graph_node_creation(&program, MaterialGraphCreateKind::Input(input), None)
                .is_ok()
        );
        for domain in [MaterialDomain::Sprite, MaterialDomain::Mesh] {
            let mut invalid = MaterialProgram::additive_sprite("Other domain");
            invalid.domain = domain;
            assert!(
                MaterialCompiler
                    .plan_graph_node_creation(&invalid, MaterialGraphCreateKind::Input(input), None)
                    .is_err()
            );
        }
    }
    for width in [0.0, -1.0, f32::INFINITY, f32::NAN] {
        let mut invalid = effect.clone();
        invalid.emitters[0].renderers[0].properties = RendererProperties::Ribbon { width };
        assert!(
            EffectCompiler::default()
                .compile_with_material_programs(
                    &invalid,
                    &BTreeMap::from([(program.id, program.clone())])
                )
                .is_err()
        );
    }
    let mut mismatch = MaterialProgram::additive_sprite("Wrong domain");
    mismatch.id = program.id;
    mismatch.render_state_policy = program.render_state_policy;
    mismatch.domain = MaterialDomain::Mesh;
    MaterialCompiler.compile(&mismatch).unwrap();
    assert!(
        EffectCompiler::default()
            .compile_with_material_programs(&effect, &BTreeMap::from([(program.id, mismatch)]))
            .is_err()
    );
}
