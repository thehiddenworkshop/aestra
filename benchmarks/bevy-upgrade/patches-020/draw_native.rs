//! Real command tuples, allocator slices and prepass/view bind groups on hardware.
use crate::{
    draw_commands::*, draw_preparation, draw_resources::*, pipeline::*, pipeline_native,
    shader_support, view_phases::PhaseDraw,
};
use aestra_core::material::{
    MaterialDomain, MaterialExpression, MaterialExpressionKind, MaterialProgram, MaterialValue,
};
use aestra_gpu::{GpuBlend, GpuParticle, GpuRenderGlobals, GpuRenderParams, GpuRenderer};
use bevy::{
    asset::RenderAssetUsages,
    camera::{RenderTarget, ShadowLodOrigin},
    core_pipeline::{
        core_2d::{CORE_2D_DEPTH_FORMAT, Transparent2d},
        core_3d::Transparent3d,
        prepass::{DepthPrepass, ViewPrepassTextures},
    },
    ecs::system::RunSystemOnce,
    mesh::Indices,
    pbr::{MeshPipeline, ViewKeyCache},
    prelude::*,
    render::{
        RenderApp,
        render_asset::RenderAssets,
        render_phase::{AddRenderCommand, DrawFunctions, PhaseItem, TrackedRenderPass},
        render_resource::*,
        renderer::{RenderAdapterInfo, RenderDevice, RenderQueue},
        storage::GpuShaderBuffer,
        sync_world::MainEntity,
        view::ExtractedView,
    },
    shader::ShaderDefVal,
    sprite_render::{Mesh2dPipeline, Mesh2dPipelineKey},
};

fn upload<T: ShaderType + encase::internal::WriteInto>(
    world: &mut World,
    value: T,
) -> Handle<bevy::render::storage::ShaderBuffer> {
    let mut bytes = encase::StorageBuffer::new(Vec::new());
    bytes.write(&value).unwrap();
    let usage = BufferUsages::STORAGE
        | BufferUsages::INDIRECT
        | BufferUsages::COPY_DST
        | BufferUsages::COPY_SRC;
    let buffer = world
        .resource::<RenderDevice>()
        .create_buffer_with_data(&BufferInitDescriptor {
            label: Some("qualification input"),
            contents: &bytes.into_inner(),
            usage,
        });
    let handle = Handle::Uuid(bevy::asset::uuid::Uuid::new_v4(), default());
    world
        .resource_mut::<RenderAssets<GpuShaderBuffer>>()
        .insert(
            handle.id(),
            GpuShaderBuffer {
                buffer,
                label: "qualification input".into(),
                buffer_usage: usage,
                had_data: true,
            },
        );
    handle
}

pub(super) fn triangle(indexed: bool) -> Mesh {
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[-0.5, -0.5, 0.0], [0.5, -0.5, 0.0], [0.0, 0.5, 0.0]],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 3])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0; 2]; 3])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0; 2]; 3])
    .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1.0, 0.0, 0.0, 1.0]; 3]);
    if indexed {
        mesh.with_inserted_indices(Indices::U32(vec![0, 1, 2]))
    } else {
        mesh
    }
}

pub(super) fn mesh_program() -> MaterialProgram {
    let mut program = MaterialProgram::additive_sprite("Deformed mesh");
    program.domain = MaterialDomain::Mesh;
    let id = aestra_core::MaterialExpressionId::from_u128(0xe001);
    program.expressions.push(MaterialExpression {
        id,
        kind: MaterialExpressionKind::Constant(MaterialValue::Vec3([0.1, 0.0, 0.0])),
    });
    program.outputs.vertex_offset = Some(id);
    let color = program.outputs.color;
    program
        .expressions
        .iter_mut()
        .find(|expression| expression.id == color)
        .unwrap()
        .kind = MaterialExpressionKind::Input(aestra_core::material::MaterialInput::Bitangent);
    program
}

fn textured(mut program: MaterialProgram) -> MaterialProgram {
    use aestra_core::material::{
        MaterialEvaluationDomain, MaterialParameter, MaterialTextureDescriptor, MaterialValueType,
    };
    let parameter = aestra_core::MaterialParameterId::from_u128(0xe010);
    let texture = aestra_core::MaterialExpressionId::from_u128(0xe011);
    let uv = aestra_core::MaterialExpressionId::from_u128(0xe012);
    let sample = aestra_core::MaterialExpressionId::from_u128(0xe013);
    let color = aestra_core::MaterialExpressionId::from_u128(0xe014);
    let mask = aestra_core::MaterialExpressionId::from_u128(0xe015);
    program.parameters.push(MaterialParameter {
        id: parameter,
        name: "Qualification texture".into(),
        value_type: MaterialValueType::Texture2D(MaterialTextureDescriptor {
            color_space: aestra_core::material::MaterialTextureColorSpace::SrgbColor,
            sampler: default(),
        }),
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: None,
    });
    program.expressions.extend([
        MaterialExpression {
            id: texture,
            kind: MaterialExpressionKind::Parameter(parameter),
        },
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(aestra_core::material::MaterialInput::Uv0),
        },
        MaterialExpression {
            id: sample,
            kind: MaterialExpressionKind::SampleTexture { texture, uv },
        },
        MaterialExpression {
            id: mask,
            kind: MaterialExpressionKind::ExtractComponent {
                value: sample,
                component: aestra_core::material::MaterialVectorComponent::X,
            },
        },
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Multiply(program.outputs.color, mask),
        },
    ]);
    program.outputs.color = color;
    program
}

fn issue<P: PhaseItem>(world: &World, view: Entity, item: &P, samples: u32) {
    let device = world.resource::<RenderDevice>();
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let texture = |format| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("qualification attachment"),
                size: Extent3d {
                    width: 64,
                    height: 64,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: TextureDimension::D2,
                format,
                usage: TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
    };
    let color = texture(TextureFormat::Bgra8UnormSrgb);
    let depth = texture(CORE_2D_DEPTH_FORMAT);
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let colors = [Some(wgpu::RenderPassColorAttachment {
            view: &color,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })];
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("actual Aestra draw commands"),
            color_attachments: &colors,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        let mut pass = TrackedRenderPass::new(device, pass);
        let mut functions = world.resource::<DrawFunctions<P>>().write();
        functions.prepare(world);
        functions
            .get_mut(item.draw_function())
            .unwrap()
            .draw(world, &mut pass, view, item)
            .unwrap();
    }
    world.resource::<RenderQueue>().submit([encoder.finish()]);
    device
        .wgpu_device()
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(30)),
        })
        .unwrap();
    let error = pollster::block_on(scope.pop());
    assert!(
        error.is_none(),
        "actual command validation failed: {error:?}"
    );
}

// Seed completed producer records, then exercise the actual consumer commands.
// Compute dispatch correctness is deliberately not claimed by this fixture.
fn routing(world: &mut World, view: Entity, entity: Entity, pipeline: CachedRenderPipelineId) {
    let draw = world.get::<crate::GpuDrawInstance>(entity).unwrap().clone();
    let buffer = |world: &World, handle: &Handle<bevy::render::storage::ShaderBuffer>| {
        world
            .resource::<RenderAssets<GpuShaderBuffer>>()
            .get(handle)
            .unwrap()
            .buffer
            .clone()
    };
    let indices = buffer(world, &draw.alive);
    let params = buffer(world, &draw.render_params);
    let indirect = buffer(world, &draw.indirect);
    let group = world.get::<GpuRenderBindGroup>(entity).unwrap().0.clone();
    let sort = AlphaSortEntry {
        count: 1,
        runs: [indices.clone(), indices.clone()],
        indices: indices.clone(),
        render_params: params.clone(),
        uniforms: vec![params.clone()],
        bindings: vec![group.clone()],
    };
    assert_eq!(sort.count, 1);
    assert!(sort.runs.iter().all(|run| run.size() >= 4));
    assert_eq!(sort.uniforms[0].id(), params.id());
    assert_eq!(sort.bindings[0].id(), group.id());
    world
        .resource_mut::<AlphaSort>()
        .entries
        .insert((view, entity), sort);
    let id = world
        .resource::<DrawFunctions<Transparent3d>>()
        .read()
        .id::<DrawGpuSprites3d>();
    let item = PhaseDraw {
        entity: (entity, MainEntity::from(Entity::PLACEHOLDER)),
        pipeline,
        draw_function: id,
        indexed: false,
    }
    .three_d(Vec3::ZERO, 0);
    let count = |world: &World| {
        world
            .resource::<Submissions>()
            .0
            .lock()
            .unwrap()
            .draws
            .len()
    };
    issue(world, view, &item, 1);
    assert_eq!(count(world), 0, "prepared but undispatched sort must Skip");
    world.resource_mut::<AlphaSort>().dispatched = true;
    issue(world, view, &item, 1);
    assert_eq!(count(world), 0, "missing sorted view binding must Skip");
    world
        .run_system_once(draw_preparation::prepare_render_bind_groups)
        .unwrap();
    assert!(
        world
            .get::<GpuRenderBindGroup>(entity)
            .unwrap()
            .2
            .contains_key(&view)
    );
    issue(world, view, &item, 1);
    assert_eq!(count(world), 1);
    world
        .resource::<Submissions>()
        .0
        .lock()
        .unwrap()
        .draws
        .clear();
    world.resource_mut::<AlphaSort>().entries.clear();
    world.resource_mut::<AlphaSort>().dispatched = false;

    // These are distinct, valid indirect buffers so the recorded BufferId proves
    // precedence (view culling > compaction > direct fallback), not only a count.
    let compact_handle = upload(
        world,
        vec![4u32, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    );
    let compact = buffer(world, &compact_handle);
    let culled_handle = upload(world, vec![4u32, 1, 0, 0]);
    let culled = buffer(world, &culled_handle);
    let entry = TrailCompactEntry {
        owner: draw.owner,
        output: compact.clone(),
        fallback: indirect.clone(),
        render_params: params.clone(),
        scratch: indices.clone(),
        params: params.clone(),
        bindings: group.clone(),
        count: 1,
        owners: 1,
        renderer: 0,
    };
    assert_eq!(entry.owner, draw.owner);
    assert_eq!(entry.fallback.id(), indirect.id());
    assert!(entry.scratch.size() >= 4 && entry.params.size() >= 4);
    assert_eq!(entry.bindings.id(), group.id());
    assert_eq!((entry.count, entry.owners, entry.renderer), (1, 1, 0));
    world
        .resource_mut::<TrailCompaction>()
        .entries
        .insert(entity, entry);
    let entry = TrailCullEntry {
        owner: draw.owner,
        params,
        indirect: culled.clone(),
        bindings: group.clone(),
        compact_bindings: group.clone(),
    };
    assert_eq!(entry.owner, draw.owner);
    assert!(entry.params.size() >= 4);
    assert_eq!(entry.bindings.id(), group.id());
    assert_eq!(entry.compact_bindings.id(), group.id());
    world
        .resource_mut::<TrailCulling>()
        .entries
        .insert((view, entity), entry);
    world
        .get_mut::<crate::GpuDrawInstance>(entity)
        .unwrap()
        .trail_instances = Some(1);
    world
        .run_system_once(draw_preparation::prepare_render_bind_groups)
        .unwrap();
    assert!(world.get::<GpuRenderBindGroup>(entity).unwrap().1.is_some());
    for (compact_ready, cull_ready, expected) in [
        (false, false, None),
        (true, false, Some(compact.id())),
        (true, true, Some(culled.id())),
        (false, true, Some(culled.id())),
    ] {
        world.resource_mut::<TrailCompaction>().dispatched = compact_ready;
        world.resource_mut::<TrailCulling>().dispatched = cull_ready;
        issue(world, view, &item, 1);
        let mut frame = world.resource::<Submissions>().0.lock().unwrap();
        assert_eq!(frame.draws.len(), 1);
        let submission = &frame.draws[0];
        assert_eq!(
            submission.command.as_ref().map(|(buffer, _)| buffer.id()),
            expected
        );
        assert_eq!(
            submission.command.as_ref().map(|(_, offset)| *offset),
            expected.map(|_| 0)
        );
        assert_eq!(
            submission.direct,
            if expected.is_none() { [4, 1] } else { [0, 0] }
        );
        frame.draws.clear();
    }
    world.resource_mut::<TrailCompaction>().entries.clear();
    world.resource_mut::<TrailCulling>().entries.clear();
    world.resource_mut::<TrailCompaction>().dispatched = false;
    world.resource_mut::<TrailCulling>().dispatched = false;
    world
        .get_mut::<crate::GpuDrawInstance>(entity)
        .unwrap()
        .trail_instances = None;
    // Missing effect bindings and missing allocator preparation must never draw.
    world.entity_mut(entity).remove::<GpuRenderBindGroup>();
    issue(world, view, &item, 1);
    assert_eq!(count(world), 0);
    world
        .run_system_once(draw_preparation::prepare_render_bind_groups)
        .unwrap();
    world
        .get_mut::<crate::GpuDrawInstance>(entity)
        .unwrap()
        .mesh = Some(Handle::default());
    issue(world, view, &item, 1);
    assert_eq!(count(world), 0);
}

#[test]
fn submission_frame_remains_bounded_without_particle_data() {
    let submissions = Submissions::default();
    for _ in 0..MAX_DRAWS + 1 {
        submissions.record(Entity::PLACEHOLDER, None, [4, 1], Topology::Strip);
    }
    let frame = submissions.0.lock().unwrap();
    assert!(frame.overflow);
    assert_eq!(frame.draws.len(), MAX_DRAWS);
    assert!(
        frame
            .draws
            .iter()
            .all(|draw| draw.owner == Entity::PLACEHOLDER
                && draw.command.is_none()
                && draw.direct == [4, 1]
                && matches!(draw.topology, Topology::Strip))
    );
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_commands_submit_with_real_mesh_view_and_prepass_bindings() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(bevy::window::WindowPlugin {
        primary_window: None,
        exit_condition: bevy::window::ExitCondition::DontExit,
        ..default()
    }));
    app.finish();
    app.cleanup();
    let mut cameras = Vec::new();
    for (order, two_d, samples) in [(0, true, 1), (1, false, 1), (2, false, 4)] {
        let target =
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new_target_texture(
                    64,
                    64,
                    TextureFormat::Bgra8UnormSrgb,
                    None,
                ));
        let mut camera = if two_d {
            app.world_mut().spawn(Camera2d)
        } else {
            app.world_mut()
                .spawn((Camera3d::default(), DepthPrepass, ShadowLodOrigin))
        };
        camera.insert((
            Camera { order, ..default() },
            RenderTarget::Image(target.into()),
            if samples == 1 {
                Msaa::Off
            } else {
                Msaa::Sample4
            },
            Transform::from_xyz(0.0, 0.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
        cameras.push((camera.id(), two_d, samples));
    }
    let texture = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_fill(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[255; 4],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        ));
    let meshes = [true, false].map(|indexed| {
        app.world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(triangle(indexed))
    });
    let mut shader_assets = Assets::<Shader>::default();
    let mesh_material = pipeline_native::binding(&mesh_program(), &mut shader_assets);
    let mut wire_draw = pipeline_native::draw(mesh_material.clone());
    wire_draw.mesh = Some(meshes[0].clone());
    wire_draw.render_mode = crate::GpuRenderMode::Wireframe;
    let wire_entity = app.world_mut().spawn(wire_draw).id();
    app.world_mut()
        .run_system_once(crate::wireframe::prepare_wireframe_geometry)
        .unwrap();
    let wire_geometry = app
        .world()
        .get::<crate::GpuDrawInstance>(wire_entity)
        .unwrap()
        .wireframe_geometry
        .clone()
        .unwrap();
    // Bounded warm-up of the real camera/prepass/allocator systems, not a timing gate.
    for _ in 0..24 {
        app.update();
    }
    let render = app.sub_app_mut(RenderApp);
    render
        .init_resource::<AlphaSort>()
        .init_resource::<TrailCulling>()
        .init_resource::<TrailCompaction>()
        .init_resource::<Submissions>()
        .add_render_command::<Transparent2d, DrawGpuSprites>()
        .add_render_command::<Transparent2d, DrawSemanticGpuSprites>()
        .add_render_command::<Transparent3d, DrawGpuSprites3d>()
        .add_render_command::<Transparent3d, DrawSemanticGpuSprites3d>()
        .add_render_command::<Transparent3d, DrawSemanticDepthGpuSprites3d>();
    let world = render.world_mut();
    let info = world.resource::<RenderAdapterInfo>();
    assert_ne!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "hardware GPU required"
    );
    println!(
        "Native draw command qualification: {} / {:?} / {}",
        info.name, info.backend, info.driver_info
    );
    let device = world.resource::<RenderDevice>().clone();
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let pipeline = GpuSpritePipeline::new(
        world.resource::<Mesh2dPipeline>().clone(),
        world.resource::<MeshPipeline>().clone(),
        shader_assets.add(Shader::from_wgsl("", "legacy")),
        shader_assets.add(Shader::from_wgsl("", "wire")),
    );
    world.insert_resource(pipeline);
    world.insert_resource(PipelineCache::new(device.clone(), true));
    world
        .run_system_once(draw_preparation::prepare_scene_depth_bind_groups)
        .unwrap();
    let views = cameras
        .iter()
        .map(|(main, two_d, samples)| {
            let entity = world
                .query::<(Entity, &MainEntity, &ExtractedView)>()
                .iter(world)
                .find(|(_, entity, _)| entity.id() == *main)
                .unwrap()
                .0;
            if !two_d {
                assert!(world.get::<GpuSceneDepthBindGroup>(entity).is_some());
            }
            (entity, *two_d, *samples)
        })
        .collect::<Vec<_>>();
    let renderer = GpuRenderer {
        emitter_index: 0,
        blend_mode: 0,
        softness: 1.0,
        textured: 0,
        uv_min: [0.0; 2].into(),
        uv_max: [1.0; 2].into(),
        tint: [1.0; 4].into(),
        particle_color: 1,
        renderer_kind: 0,
        frame_count: 1,
        playback_mode: 0,
        flipbook_flags: 0,
        frame_rate: 0.0,
        attribute_flags: [0u32; 3].into(),
        frames: [[0.0; 4].into(); aestra_gpu::MAX_FLIPBOOK_FRAMES],
    };
    let renderers = upload(world, vec![renderer]);
    let particles = upload(
        world,
        vec![GpuParticle {
            color: [1.0; 4].into(),
            size: 1.0,
            packed_emitter_alive: 1,
            ..default()
        }],
    );
    let alive = upload(world, vec![0u32; 16]);
    let aux = upload(world, vec![0u32; 64]);
    let identity = aestra_gpu::GpuGlobals::default().world_from_effect;
    let globals = upload(
        world,
        GpuRenderGlobals {
            world_from_effect: identity,
            ..default()
        },
    );
    let params = upload(
        world,
        GpuRenderParams {
            mesh_from_local: identity,
            ..default()
        },
    );
    let indirect = upload(world, vec![4u32, 1, 0, 0]);
    let mut successes = 0;
    for &(view, two_d, samples) in &views {
        // Legacy, semantic, depth fade, indexed/nonindexed mesh, deformed wireframe,
        // then authored point-lit smoke with actual texture/sampler/uniform slots.
        for case in 0..8 {
            if two_d && case >= 2 {
                continue;
            }
            let program = if case == 2 {
                pipeline_native::depth_program()
            } else if (3..=5).contains(&case) {
                mesh_program()
            } else if case >= 6 {
                let smoke = MaterialProgram::load_ron(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
                        "../../../assets/test/materials/fireworks_lit_smoke.aestra.material.ron",
                    ),
                )
                .unwrap();
                if case == 7 { textured(smoke) } else { smoke }
            } else {
                MaterialProgram::additive_sprite("Sprite")
            };
            let mut material = pipeline_native::binding(&program, &mut shader_assets);
            if case >= 6 {
                assert!(material.program.requires_scene_lighting());
                let layout = &material.program.resource_layout;
                assert!(layout.uniforms.size > 0);
                if case == 7 {
                    assert!(!layout.textures.is_empty() && !layout.samplers.is_empty());
                }
                material.uniforms = vec![0u8; layout.uniforms.size as usize].into();
                material.textures = vec![texture.clone(); layout.textures.len()];
                material.fallback_texture = texture.clone();
            }
            let mut draw = pipeline_native::draw(material.clone());
            if case == 0 {
                draw.semantic_material = None;
            }
            if (3..=5).contains(&case) {
                draw.mesh = Some(meshes[usize::from(case == 4)].clone());
            }
            if case == 5 {
                draw.render_mode = crate::GpuRenderMode::Wireframe;
                draw.wireframe_geometry = Some(wire_geometry.clone());
            }
            draw.renderers = renderers.clone();
            draw.particles = particles.clone();
            draw.alive = alive.clone();
            draw.aux = aux.clone();
            draw.render_globals = globals.clone();
            draw.render_params = params.clone();
            draw.indirect = indirect.clone();
            draw.texture = texture.clone();
            draw.fallback_texture = texture.clone();
            let entity = world.spawn(draw.clone()).id();
            world
                .run_system_once(draw_preparation::prepare_mesh_draws)
                .unwrap();
            world
                .run_system_once(draw_preparation::prepare_render_bind_groups)
                .unwrap();
            if (3..=5).contains(&case) {
                let previous = world
                    .get::<PreparedMeshDraw>(entity)
                    .expect("real mesh allocation must prepare");
                let identity = (previous.indirect.id(), previous.vertex.id());
                world
                    .run_system_once(draw_preparation::prepare_mesh_draws)
                    .unwrap();
                let prepared = world.get::<PreparedMeshDraw>(entity).unwrap();
                assert_eq!(
                    (prepared.indirect.id(), prepared.vertex.id()),
                    identity,
                    "unchanged mesh preparation must reuse buffers"
                );
            }
            let mesh = world.get::<PreparedMeshDraw>(entity);
            if (3..=5).contains(&case) {
                let mesh = mesh.expect("real mesh allocation must prepare");
                world.resource::<RenderQueue>().write_buffer(
                    &mesh.indirect,
                    4,
                    &1u32.to_le_bytes(),
                );
                assert_eq!(mesh.index.is_some(), case != 4);
            }
            assert!(world.get::<GpuRenderBindGroup>(entity).is_some());
            if case != 0 {
                assert!(world.get::<GpuMaterialBindGroup>(entity).is_some());
            }
            let key = GpuSpritePipelineKey {
                mesh_wireframe: case == 5,
                mesh_layout: mesh.and_then(|mesh| mesh.layout.clone()),
                view: if two_d {
                    GpuSpriteViewKey::TwoD(
                        Mesh2dPipelineKey::from_msaa_samples(samples)
                            | Mesh2dPipelineKey::from_target_format(TextureFormat::Bgra8UnormSrgb),
                    )
                } else {
                    GpuSpriteViewKey::ThreeD(
                        *world
                            .resource::<ViewKeyCache>()
                            .get(
                                &world
                                    .get::<ExtractedView>(view)
                                    .unwrap()
                                    .retained_view_entity,
                            )
                            .unwrap(),
                    )
                },
                blend: GpuBlend::Alpha,
                render_mode: draw.render_mode,
                material: draw_pipeline_key(
                    &draw,
                    TextureFormat::Bgra8UnormSrgb,
                    samples,
                    u64::from(!two_d),
                ),
            };
            let descriptor = world.resource::<GpuSpritePipeline>().specialize(key);
            let source = if case == 0 {
                aestra_gpu::shader::SPRITE_RENDER_WESL.to_owned()
            } else {
                pipeline_native::composition::compose_material(
                    if case == 2 && samples > 1 {
                        &material.program.multisampled_shader.wgsl
                    } else {
                        &material.program.shader.wgsl
                    },
                    material.program.requires_scene_lighting(),
                    pipeline_native::composition::Dialect::Bevy020,
                )
            };
            let mut defs = descriptor.vertex.shader_defs.clone();
            let storage = device.limits().max_storage_buffers_per_shader_stage;
            defs.extend([
                ShaderDefVal::UInt("AVAILABLE_STORAGE_BUFFER_BINDINGS".into(), storage),
                ShaderDefVal::Bool(
                    "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3".into(),
                    storage >= 3,
                ),
                ShaderDefVal::Bool(
                    "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_6".into(),
                    storage >= 6,
                ),
            ]);
            let compiled = shader_support::compile(source, &defs);
            let mut cache = world.resource_mut::<PipelineCache>();
            cache.set_shader(
                descriptor.vertex.shader.id(),
                Shader::from_wgsl(compiled.wgsl.clone(), "native-command.wgsl"),
            );
            let pipeline = cache.queue_render_pipeline(descriptor);
            cache.process_queue();
            assert!(
                matches!(
                    cache.get_render_pipeline_state(pipeline),
                    CachedPipelineState::Ok(_)
                ),
                "{:?}",
                cache.get_render_pipeline_state(pipeline)
            );
            let indexed = world
                .get::<PreparedMeshDraw>(entity)
                .is_some_and(|mesh| mesh.index.is_some());
            let main = MainEntity::from(Entity::PLACEHOLDER);
            if two_d {
                let functions = world.resource::<DrawFunctions<Transparent2d>>().read();
                let id = if case == 0 {
                    functions.id::<DrawGpuSprites>()
                } else {
                    functions.id::<DrawSemanticGpuSprites>()
                };
                drop(functions);
                issue(
                    world,
                    view,
                    &PhaseDraw {
                        entity: (entity, main),
                        pipeline,
                        draw_function: id,
                        indexed,
                    }
                    .two_d(0),
                    samples,
                );
            } else {
                let functions = world.resource::<DrawFunctions<Transparent3d>>().read();
                let id = if case == 0 {
                    functions.id::<DrawGpuSprites3d>()
                } else if case == 2 {
                    functions.id::<DrawSemanticDepthGpuSprites3d>()
                } else {
                    functions.id::<DrawSemanticGpuSprites3d>()
                };
                drop(functions);
                issue(
                    world,
                    view,
                    &PhaseDraw {
                        entity: (entity, main),
                        pipeline,
                        draw_function: id,
                        indexed,
                    }
                    .three_d(Vec3::ZERO, 0),
                    samples,
                );
            }
            let submissions = world.resource::<Submissions>();
            let mut frame = submissions.0.lock().unwrap();
            assert_eq!(
                frame.draws.len(),
                1,
                "command must submit, not silently Skip (case {case})"
            );
            assert_eq!(frame.draws[0].owner, draw.owner);
            assert!(matches!(
                (case, frame.draws[0].topology),
                (5, Topology::Lines)
                    | (3 | 4, Topology::Triangles)
                    | (0..=2 | 6 | 7, Topology::Strip)
            ));
            frame.draws.clear();
            drop(frame);
            if !two_d && samples == 1 && case == 0 {
                routing(world, view, entity, pipeline);
            }
            if (3..=5).contains(&case) {
                let mut stale = world.get_mut::<crate::GpuDrawInstance>(entity).unwrap();
                stale.mesh = Some(Handle::default());
                stale.wireframe_geometry = None;
                world
                    .run_system_once(draw_preparation::prepare_mesh_draws)
                    .unwrap();
                assert!(
                    world.get::<PreparedMeshDraw>(entity).is_none(),
                    "missing geometry must remove stale preparation"
                );
            }
            world.despawn(entity);
            successes += 1;
        }
    }
    assert_eq!(successes, 18);
    // Actual prepass absence must remove the stale Aestra depth group.
    let depth_view = views[1].0;
    world
        .get_mut::<ViewPrepassTextures>(depth_view)
        .unwrap()
        .depth = None;
    world
        .run_system_once(draw_preparation::prepare_scene_depth_bind_groups)
        .unwrap();
    assert!(world.get::<GpuSceneDepthBindGroup>(depth_view).is_none());
    assert!(world.get::<GpuSceneDepthBindGroup>(views[2].0).is_some());
    let error = pollster::block_on(scope.pop());
    assert!(error.is_none(), "native draw validation failed: {error:?}");
    println!(
        "Submitted {successes} actual command tuples with real Bevy view/prepass groups and allocator-backed mesh draws"
    );
}
