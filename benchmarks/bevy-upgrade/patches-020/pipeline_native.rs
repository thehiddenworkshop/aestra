//! Production specialization and explicit layouts, not inferred wgpu layouts.
use crate::{draw_instance::*, pipeline::*, shader_support};
#[path = "../../../bevy/aestra-bevy-render/src/gpu/shader_composition.rs"]
pub(super) mod composition;
use aestra_core::{
    MaterialExpressionId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialInput, MaterialProgram, MaterialValue,
    },
};
use aestra_gpu::{
    GpuBlend,
    material::{MaterialBackendCapabilities, MaterialShaderCompiler},
};
use bevy::{
    pbr::{MeshPipeline, MeshPipelineKey},
    prelude::*,
    render::{
        RenderApp,
        render_resource::*,
        renderer::{RenderAdapterInfo, RenderDevice},
    },
    shader::ShaderDefVal,
    sprite_render::{Mesh2dPipeline, Mesh2dPipelineKey},
};
use std::sync::Arc;

pub(super) fn binding(
    program: &MaterialProgram,
    assets: &mut Assets<Shader>,
) -> GpuSemanticMaterialBinding {
    let ir = aestra_compiler::MaterialCompiler.compile(program).unwrap();
    let program = Arc::new(
        MaterialShaderCompiler
            .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
            .unwrap(),
    );
    GpuSemanticMaterialBinding {
        render_state: program.render_state_policy.default,
        program,
        shader: assets.add(Shader::from_wgsl("", "material")),
        multisampled_shader: assets.add(Shader::from_wgsl("", "material-msaa")),
        uniforms: Arc::from([]),
        textures: vec![],
        fallback_texture: default(),
    }
}

pub(super) fn draw(material: GpuSemanticMaterialBinding) -> GpuDrawInstance {
    GpuDrawInstance {
        sort_range: UVec2::ZERO,
        renderer_kind: 0,
        owner: Entity::PLACEHOLDER,
        mesh: None,
        wireframe_geometry: None,
        renderers: default(),
        particles: default(),
        alive: default(),
        aux: default(),
        indirect: default(),
        render_globals: default(),
        render_params: default(),
        texture: default(),
        fallback_texture: default(),
        renderer_order: 0,
        emitter_index: 0,
        indirect_offset: 0,
        trail_instances: None,
        trail_owners: 0,
        blend: GpuBlend::Alpha,
        material: aestra_core::MaterialId::from_u128(1),
        semantic_material: Some(material),
        render_mode: GpuRenderMode::Rendered,
        mesh_center: Vec3::ZERO,
        sampled_sprite_cull: None,
    }
}

pub(super) fn depth_program() -> MaterialProgram {
    let id = |value| MaterialExpressionId::from_u128(value);
    let mut program = MaterialProgram::additive_sprite("Depth fade");
    program.expressions.extend([
        MaterialExpression {
            id: id(0xd001),
            kind: MaterialExpressionKind::Input(MaterialInput::SceneDepth),
        },
        MaterialExpression {
            id: id(0xd002),
            kind: MaterialExpressionKind::Input(MaterialInput::PixelDepth),
        },
        MaterialExpression {
            id: id(0xd003),
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: id(0xd004),
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: id(0xd005),
            kind: MaterialExpressionKind::DepthFade {
                scene_depth: id(0xd001),
                pixel_depth: id(0xd002),
                fade_distance: id(0xd003),
                invert: id(0xd004),
            },
        },
    ]);
    program.outputs.alpha = id(0xd005);
    program
}

#[test]
fn scene_depth_keys_choose_msaa_shader_and_wireframe_keeps_only_deformation() {
    let mut assets = Assets::default();
    let mut program = depth_program();
    program.domain = aestra_core::material::MaterialDomain::Mesh;
    let offset = MaterialExpressionId::from_u128(0xd006);
    program.expressions.push(MaterialExpression {
        id: offset,
        kind: MaterialExpressionKind::Constant(MaterialValue::Vec3([0.1, 0.0, 0.0])),
    });
    program.outputs.vertex_offset = Some(offset);
    let material = binding(&program, &mut assets);
    let single = semantic_pipeline_key(Some(&material), TextureFormat::Rgba16Float, 1, 0).unwrap();
    let msaa = semantic_pipeline_key(Some(&material), TextureFormat::Rgba16Float, 4, 0).unwrap();
    assert!(single.requires_scene_depth && msaa.requires_scene_depth);
    assert_eq!(single.shader, material.shader);
    assert_eq!(msaa.shader, material.multisampled_shader);
    let mut draw = draw(material);
    draw.mesh = Some(Handle::default());
    draw.render_mode = GpuRenderMode::Wireframe;
    assert!(material_for_draw(&draw).is_some());
    assert!(
        !draw_pipeline_key(&draw, TextureFormat::Rgba16Float, 4, 0)
            .unwrap()
            .requires_scene_depth
    );
}

#[test]
fn actual_draw_keys_separate_variants_and_preserve_diagnostic_policy() {
    let mut assets = Assets::default();
    let mut draw = draw(binding(
        &MaterialProgram::additive_sprite("Identity"),
        &mut assets,
    ));
    let base = draw_pipeline_key(&draw, TextureFormat::Bgra8UnormSrgb, 1, 0).unwrap();
    for (format, samples, bits) in [
        (TextureFormat::Rgba16Float, 1, 0),
        (TextureFormat::Bgra8UnormSrgb, 4, 0),
        (TextureFormat::Bgra8UnormSrgb, 1, 1),
    ] {
        assert_ne!(
            Some(base.clone()),
            draw_pipeline_key(&draw, format, samples, bits)
        );
    }
    draw.semantic_material.as_mut().unwrap().uniforms = Arc::from([255; 16]);
    assert_eq!(
        Some(base),
        draw_pipeline_key(&draw, TextureFormat::Bgra8UnormSrgb, 1, 0)
    );
    draw.render_mode = GpuRenderMode::Wireframe;
    assert!(material_for_draw(&draw).is_none());
    assert!(draw_pipeline_key(&draw, TextureFormat::Bgra8UnormSrgb, 1, 0).is_none());
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_production_pipeline_descriptors_validate_with_explicit_layouts() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(bevy::window::WindowPlugin {
        primary_window: None,
        exit_condition: bevy::window::ExitCondition::DontExit,
        ..default()
    }));
    app.finish();
    app.cleanup();
    app.update();
    let world = app.sub_app(RenderApp).world();
    let info = world.resource::<RenderAdapterInfo>();
    assert_ne!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "hardware GPU required"
    );
    println!(
        "Native explicit pipeline qualification: {} / {:?} / {}",
        info.name, info.backend, info.driver_info
    );
    let device = world.resource::<RenderDevice>().clone();
    let mut assets = Assets::<Shader>::default();
    let sprite = assets.add(Shader::from_wgsl("", "sprite"));
    let wire = assets.add(Shader::from_wgsl("", "wire"));
    let pipeline = GpuSpritePipeline::new(
        world.resource::<Mesh2dPipeline>().clone(),
        world.resource::<MeshPipeline>().clone(),
        sprite.clone(),
        wire.clone(),
    );
    assert_eq!(pipeline.effect_layout.entries.len(), 8);
    assert_eq!(pipeline.scene_depth_layout.entries.len(), 2);
    assert_eq!(pipeline.multisampled_scene_depth_layout.entries.len(), 2);
    let smoke = MaterialProgram::load_ron(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../assets/test/materials/fireworks_lit_smoke.aestra.material.ron"),
    )
    .unwrap();
    let materials = [
        None,
        Some(binding(
            &MaterialProgram::additive_sprite("Sprite"),
            &mut assets,
        )),
        Some(binding(&smoke, &mut assets)),
        Some(binding(&depth_program(), &mut assets)),
    ];
    let mut count = 0;
    for two_d in [true, false] {
        for format in [TextureFormat::Bgra8UnormSrgb, TextureFormat::Rgba16Float] {
            for samples in [1, 4] {
                for material in &materials {
                    for blend in [GpuBlend::Alpha, GpuBlend::Additive, GpuBlend::Multiply] {
                        for mode in [GpuRenderMode::Rendered, GpuRenderMode::Wireframe] {
                            let semantic = material.clone().and_then(|material| {
                                let mut draw = draw(material);
                                draw.render_mode = mode;
                                draw_pipeline_key(&draw, format, samples, 0)
                            });
                            let key = GpuSpritePipelineKey {
                                view: if two_d {
                                    GpuSpriteViewKey::TwoD(
                                        Mesh2dPipelineKey::from_msaa_samples(samples)
                                            | Mesh2dPipelineKey::from_target_format(format),
                                    )
                                } else {
                                    GpuSpriteViewKey::ThreeD(
                                        MeshPipelineKey::from_msaa_samples(samples)
                                            | MeshPipelineKey::from_target_format(format),
                                    )
                                },
                                mesh_wireframe: material.is_none()
                                    && mode == GpuRenderMode::Wireframe,
                                mesh_layout: None,
                                blend,
                                render_mode: mode,
                                material: semantic,
                            };
                            let descriptor = pipeline.specialize(key.clone());
                            if key
                                .material
                                .as_ref()
                                .is_some_and(|material| material.requires_scene_depth)
                            {
                                assert_eq!(descriptor.layout.len(), 4);
                                assert_eq!(
                                    descriptor.layout[3],
                                    if samples > 1 {
                                        pipeline.multisampled_scene_depth_layout.clone()
                                    } else {
                                        pipeline.scene_depth_layout.clone()
                                    }
                                );
                            }
                            assert_eq!(descriptor.multisample.count, samples);
                            assert_eq!(
                                descriptor.fragment.as_ref().unwrap().targets[0]
                                    .as_ref()
                                    .unwrap()
                                    .format,
                                format
                            );
                            assert_eq!(
                                descriptor.primitive.topology,
                                if key.mesh_wireframe {
                                    PrimitiveTopology::LineList
                                } else {
                                    PrimitiveTopology::TriangleStrip
                                }
                            );
                            assert_eq!(
                                descriptor.depth_stencil.as_ref().unwrap().depth_compare,
                                Some(CompareFunction::GreaterEqual)
                            );
                            let shader = descriptor.vertex.shader.clone();
                            if key.mesh_wireframe {
                                assert_eq!(descriptor.vertex.buffers[0].array_stride, 56);
                                assert_eq!(descriptor.vertex.buffers[0].attributes.len(), 5);
                            }
                            let source = if key.mesh_wireframe {
                                aestra_gpu::shader::mesh_wireframe_wesl()
                            } else if let Some(material) = material
                                .as_ref()
                                .filter(|_| mode == GpuRenderMode::Rendered)
                            {
                                composition::compose_material(
                                    if material.program.requires_scene_depth() && samples > 1 {
                                        &material.program.multisampled_shader.wgsl
                                    } else {
                                        &material.program.shader.wgsl
                                    },
                                    material.program.requires_scene_lighting(),
                                    composition::Dialect::Bevy020,
                                )
                            } else {
                                aestra_gpu::shader::SPRITE_RENDER_WESL.to_owned()
                            };
                            let mut defs = descriptor.vertex.shader_defs.clone();
                            // PipelineCache installs these same device-derived global defs.
                            let storage = device.limits().max_storage_buffers_per_shader_stage;
                            defs.push(ShaderDefVal::UInt(
                                "AVAILABLE_STORAGE_BUFFER_BINDINGS".into(),
                                storage,
                            ));
                            defs.push(ShaderDefVal::Bool(
                                "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3".into(),
                                storage >= 3,
                            ));
                            defs.push(ShaderDefVal::Bool(
                                "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_6".into(),
                                storage >= 6,
                            ));
                            let compiled = shader_support::compile(source, &defs);
                            assert!(compiled.module.entry_points.iter().any(|entry| entry.name
                                == descriptor.vertex.entry_point.as_deref().unwrap()));
                            let mut cache = PipelineCache::new(device.clone(), true);
                            cache.set_shader(
                                shader.id(),
                                Shader::from_wgsl(compiled.wgsl.clone(), "qualified-flat.wgsl"),
                            );
                            let mut specialized =
                                SpecializedRenderPipelines::<GpuSpritePipeline>::default();
                            let id = specialized.specialize(&cache, &pipeline, key.clone());
                            assert_eq!(id, specialized.specialize(&cache, &pipeline, key));
                            cache.process_queue();
                            assert!(
                                matches!(
                                    cache.get_render_pipeline_state(id),
                                    CachedPipelineState::Ok(_)
                                ),
                                "pipeline {count}: {:?}",
                                cache.get_render_pipeline_state(id)
                            );
                            count += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(count, 192);
    println!(
        "Validated {count} production descriptors with explicit layouts (2D/3D, SDR/HDR, 1x/4x MSAA, blend, diagnostic and semantic lighting variants)"
    );
}
