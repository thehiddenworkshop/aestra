use aestra_compiler::EffectCompiler as Compiler;
use aestra_core::{
    DiagnosticCode, EffectAsset,
    material::{MaterialDomain, MaterialProgram},
};
use aestra_runtime::{
    BackendCapabilities, CompatibilityTarget, RendererCapability, RendererPlanKind,
};
use std::collections::BTreeMap;

fn fixture() -> (EffectAsset, MaterialProgram) {
    (
        EffectAsset::from_ron(include_str!(
            "../../../assets/effects/mesh_material_lab.aestra.ron"
        ))
        .unwrap(),
        MaterialProgram::from_ron(include_str!(
            "../../../assets/materials/mesh_material_lab.aestra.material.ron"
        ))
        .unwrap(),
    )
}

#[test]
fn mesh_lab_lowers_to_portable_mesh_plans() {
    let (asset, program) = fixture();
    let compiled = Compiler::default()
        .compile_with_material_programs(&asset, &BTreeMap::from([(program.id, program)]))
        .unwrap();
    assert!(matches!(
        compiled.emitters[0].renderers[0].kind,
        RendererPlanKind::Mesh { .. }
    ));
    assert!(
        compiled
            .requirements
            .renderers
            .contains(&RendererCapability::MeshParticles)
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
fn meshes_reject_sprite_materials_and_missing_geometry() {
    let (mut asset, mut program) = fixture();
    program.domain = MaterialDomain::Sprite;
    // Keep render-state validation valid so this test isolates the renderer/domain mismatch.
    program.render_state_policy.default.cull_mode = aestra_core::material::MaterialCullMode::None;
    for state in &mut program.render_state_policy.allowed {
        state.cull_mode = aestra_core::material::MaterialCullMode::None;
    }
    asset.material_instances[0].render_state.cull_mode =
        aestra_core::material::MaterialCullMode::None;
    let error = Compiler::default()
        .compile_with_material_programs(&asset, &BTreeMap::from([(program.id, program)]))
        .unwrap_err();
    assert!(
        format!("{error:?}").contains(&format!("{:?}", DiagnosticCode::UnsupportedMaterialDomain))
    );
    let (mut asset, program) = fixture();
    asset.assets.clear();
    let error = Compiler::default()
        .compile_with_material_programs(&asset, &BTreeMap::from([(program.id, program)]))
        .unwrap_err();
    assert!(format!("{error:?}").contains("registered Mesh asset"));
}
