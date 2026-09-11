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
            "../../../assets/test/effects/ribbon_lab.aestra.ron"
        ))
        .unwrap(),
        MaterialProgram::from_ron(include_str!(
            "../../../assets/test/materials/ribbon_lab.aestra.material.ron"
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
        RendererPlanKind::Ribbon {
            width: 0.35,
            strand_count: 3
        }
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
fn ribbon_strand_count_defaults_validates_and_rejects_conflicting_shared_links() {
    let legacy = include_str!("../../../assets/test/effects/ribbon_lab.aestra.ron")
        .replace(", strand_count: 3", "");
    let legacy = EffectAsset::from_ron(&legacy).unwrap();
    assert!(matches!(
        legacy.emitters[0].renderers[0].properties,
        RendererProperties::Ribbon {
            strand_count: 1,
            ..
        }
    ));
    let (effect, program) = fixture();
    let programs = BTreeMap::from([(program.id, program)]);
    for strand_count in [0, 1, 3, 256, 257, u32::MAX] {
        let mut effect = effect.clone();
        effect.emitters[0].renderers[0].properties = RendererProperties::Ribbon {
            width: 1.0,
            strand_count,
        };
        if (1..=256).contains(&strand_count) {
            let serialized = effect.to_pretty_ron().unwrap();
            assert_eq!(EffectAsset::from_ron(&serialized).unwrap(), effect);
        }
        assert_eq!(
            EffectCompiler::default()
                .compile_with_material_programs(&effect, &programs)
                .is_ok(),
            (1..=256).contains(&strand_count)
        );
    }
    let mut effect = effect;
    let mut other = effect.emitters[0].renderers[0].clone();
    other.id = aestra_core::RendererId::new();
    other.properties = RendererProperties::Ribbon {
        width: 2.0,
        strand_count: 2,
    };
    effect.emitters[0].renderers.push(other);
    let report = EffectCompiler::default()
        .compile_with_material_programs(&effect, &programs)
        .unwrap_err();
    assert!(format!("{report:?}").contains("same strand count"));
    effect.emitters[0].renderers[1].properties = RendererProperties::Ribbon {
        width: 2.0,
        strand_count: 3,
    };
    assert!(
        EffectCompiler::default()
            .compile_with_material_programs(&effect, &programs)
            .is_ok()
    );
    effect.emitters[0].renderers[1].properties = RendererProperties::Ribbon {
        width: 2.0,
        strand_count: 2,
    };
    effect.emitters[0].renderers[1].enabled = false;
    assert!(
        EffectCompiler::default()
            .compile_with_material_programs(&effect, &programs)
            .is_ok()
    );
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
        invalid.emitters[0].renderers[0].properties = RendererProperties::Ribbon {
            width,
            strand_count: 1,
        };
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
