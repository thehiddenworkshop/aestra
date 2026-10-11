//! Compile actual assets produced by the production MaterialPreparationParams/cache.
use crate::{draw_instance::GpuDrawInstance, material::*, preparation, shader_support};
use aestra_core::{EffectAsset, Emitter, material::MaterialProgram};
use bevy::{
    asset::{AssetApp, AssetPlugin},
    prelude::*,
    render::storage::ShaderBuffer,
    shader::{ShaderDefVal, Source},
};
use std::sync::Arc;

#[test]
fn prepared_material_assets_compile_through_actual_020_shader_cache() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_asset::<Shader>()
        .init_asset::<ShaderBuffer>()
        .init_resource::<crate::ProjectAssetCache>()
        .init_resource::<crate::particle_lights::AestraParticleLightSettings>()
        .insert_resource(crate::GpuCapabilities {
            max_particles: 262_144,
            max_storage_buffer_binding_size: 128 << 20,
            max_buffer_size: 256 << 20,
            max_compute_workgroups_per_dimension: 65_535,
            ..default()
        })
        .add_systems(Update, preparation::prepare_gpu_effects);
    preparation::install(&mut app);
    let mut source = EffectAsset::new("Prepared shaders", 3.);
    source.emitters.push(Emitter::basic_sprite("Sprite", 3.));
    let effect = Arc::new(
        aestra_compiler::EffectCompiler::default()
            .compile(&source)
            .unwrap(),
    );
    let lit = MaterialProgram::load_ron(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../assets/test/materials/fireworks_lit_smoke.aestra.material.ron"),
    )
    .unwrap();
    for program in [
        MaterialProgram::additive_sprite("Unlit"),
        crate::pipeline_native::depth_program(),
        lit,
    ] {
        let program = compile_material_program(&program).unwrap();
        let mut presented = crate::PresentedEffect::new(effect.clone());
        let material = presented.effect().emitters[0].renderers[0].material;
        presented.bind_material(
            material,
            MaterialRuntimeBinding::new(program.clone(), program.render_state_policy.default)
                .unwrap(),
        );
        let owner = app
            .world_mut()
            .spawn((
                presented,
                crate::EffectRuntimeStatus {
                    active: crate::ActiveBackend::Gpu,
                    reason: "shader-asset qualification".into(),
                    compatibility: crate::CompatibilityReport::compatible(
                        crate::CompatibilityTarget::NativeGpu,
                    ),
                },
            ))
            .id();
        app.update();
        let binding = app
            .world_mut()
            .query::<&GpuDrawInstance>()
            .iter(app.world())
            .find(|draw| draw.owner == owner)
            .unwrap()
            .semantic_material
            .clone()
            .unwrap();
        assert_eq!(
            binding.shader == binding.multisampled_shader,
            !program.requires_scene_depth()
        );
        for lighting in [false, true] {
            for (handle, multisampled) in [
                (&binding.shader, false),
                (&binding.multisampled_shader, true),
            ] {
                let shader = app
                    .world()
                    .resource::<Assets<Shader>>()
                    .get(handle)
                    .unwrap()
                    .clone();
                assert!(
                    matches!(shader.source, Source::Wesl(_)),
                    "generated 0.20 assets must carry WESL metadata"
                );
                let mut defs = vec![ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 1)];
                if lighting {
                    defs.push("AESTRA_SCENE_POINT_LIGHTING".into());
                }
                if program.requires_scene_depth() && multisampled {
                    defs.push("MULTISAMPLED".into());
                }
                let compiled = shader_support::compile_asset(shader, &defs);
                assert_eq!(compiled.module.entry_points.len(), 2);
            }
        }
        app.world_mut().entity_mut(owner).despawn();
    }
}
