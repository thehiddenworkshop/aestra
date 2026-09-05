use aestra_compiler::EffectCompiler;
use aestra_core::{EffectAsset, ModuleParameters, RendererProperties, material::MaterialProgram};
use aestra_gpu::{GpuEffectArtifact, ribbon_bounds::RibbonParticleBounds};
use aestra_runtime::EffectInstance;
use std::{collections::BTreeMap, sync::Arc};

fn bounds(effect: &EffectAsset) -> RibbonParticleBounds {
    let program = MaterialProgram::from_ron(include_str!(
        "../../../assets/materials/ribbon_lab.aestra.material.ron"
    ))
    .unwrap();
    let compiled = EffectCompiler::default()
        .compile_with_material_programs(effect, &BTreeMap::from([(program.id, program)]))
        .unwrap();
    let instance = EffectInstance::new(Arc::new(compiled));
    let dynamics = GpuEffectArtifact::dynamics_from_instance(&instance).unwrap();
    let ribbon = dynamics.ribbon_bounds[0];
    assert!(
        dynamics
            .bounds_half_extents
            .cmpge(ribbon.half_extents(glam::Mat3::IDENTITY).unwrap())
            .all()
    );
    ribbon
}

#[test]
fn authored_width_size_motion_and_emitter_transform_refresh_ribbon_bounds() {
    let mut effect = EffectAsset::from_ron(include_str!(
        "../../../assets/effects/ribbon_lab.aestra.ron"
    ))
    .unwrap();
    let initial = bounds(&effect);
    assert_eq!(initial.maximum_half_width, 6.0);
    effect.emitters[0].renderers[0].properties = RendererProperties::Ribbon { width: 3.0 };
    assert_eq!(bounds(&effect).maximum_half_width, 18.0);
    for module in &mut effect.emitters[0].modules {
        if let ModuleParameters::Appearance { size, .. } = &mut module.parameters {
            for key in &mut size.keys {
                key.value *= 2.0;
            }
        }
    }
    assert_eq!(bounds(&effect).maximum_half_width, 36.0);
    effect.emitters[0].transform.scale = [0.2, 4.0, 2.0];
    let scaled = bounds(&effect);
    assert_eq!(scaled.maximum_half_width, 144.0);
    effect.emitters[0].transform.translation = [100.0, -20.0, 30.0];
    let moved = bounds(&effect);
    assert!(
        (moved.position_half_extents
            - scaled.position_half_extents
            - glam::Vec3::new(100.0, 20.0, 30.0))
        .abs()
        .max_element()
            < 0.001
    );
    for module in &mut effect.emitters[0].modules {
        if let ModuleParameters::Motion { gravity, .. } = &mut module.parameters {
            *gravity = [1000.0, -1000.0, 1000.0];
        }
    }
    assert!(
        bounds(&effect)
            .position_half_extents
            .cmpgt(moved.position_half_extents)
            .all()
    );
}
