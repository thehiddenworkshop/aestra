//! Actual extraction -> assets -> install/queues -> render graph, without manual draws.
//! Inputs are fixture-owned GPU records, not the production simulation producers.
use crate::{draw_commands::PreparedMeshDraw, draw_resources::*, pipeline_native};
use aestra_core::material::MaterialProgram;
use aestra_gpu::{GpuParticle, GpuRenderGlobals, GpuRenderParams, GpuRenderer};
use bevy::{
    asset::{RenderAssetUsages, io::embedded::EmbeddedAssetRegistry},
    camera::{RenderTarget, ShadowLodOrigin, primitives::Aabb, visibility::RenderLayers},
    core_pipeline::{core_2d::Transparent2d, core_3d::Transparent3d, prepass::DepthPrepass},
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin, RenderSystems,
        render_phase::{PhaseItem, ViewSortedRenderPhases},
        render_resource::*,
        renderer::{RenderAdapterInfo, RenderDevice, RenderQueue},
        storage::ShaderBuffer,
        sync_world::MainEntity,
        view::ExtractedView,
    },
};
use std::collections::BTreeMap;

#[path = "../../../bevy/aestra-bevy-render/src/gpu/storage_buffers_020.rs"]
mod storage_buffers;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/storage_encoding.rs"]
mod storage_encoding;

#[derive(Resource, Default, Debug)]
struct Capture {
    phases: BTreeMap<Entity, Vec<Entity>>,
    owners: Vec<Entity>,
    preparation: u32,
}

fn capture(
    views: Query<(&MainEntity, &ExtractedView)>,
    two: Res<ViewSortedRenderPhases<Transparent2d>>,
    three: Res<ViewSortedRenderPhases<Transparent3d>>,
    submissions: Res<Submissions>,
    mut capture: ResMut<Capture>,
) {
    capture.phases.clear();
    for (main, view) in &views {
        let items = if let Some(phase) = two.get(&view.retained_view_entity) {
            phase
                .items
                .values()
                .map(|item| item.main_entity().id())
                .collect()
        } else if let Some(phase) = three.get(&view.retained_view_entity) {
            phase
                .items
                .values()
                .map(|item| item.main_entity().id())
                .collect()
        } else {
            continue;
        };
        capture.phases.insert(main.id(), items);
    }
    let mut frame = submissions.0.lock().unwrap();
    assert!(!frame.overflow);
    capture.owners = frame.draws.drain(..).map(|draw| draw.owner).collect();
}

fn mark_sort(mut capture: ResMut<Capture>) {
    capture.preparation = 1;
}
fn mark_compaction(mut capture: ResMut<Capture>) {
    assert_eq!(capture.preparation, 1);
    capture.preparation = 2;
}
fn audit_bindings(mut capture: ResMut<Capture>) {
    assert_eq!(capture.preparation, 2);
    capture.preparation = 3;
}

// Test-only stand-in for completed compute output. The production preparation
// resets mesh instance counts; write one only after it, in the actual schedule.
fn fixture_mesh_counts(draws: Query<&PreparedMeshDraw>, queue: Res<RenderQueue>) {
    for draw in &draws {
        queue.write_buffer(&draw.indirect, 4, &1u32.to_le_bytes());
    }
}

fn asset<T: ShaderType + encase::internal::WriteInto>(
    world: &mut World,
    value: T,
) -> Handle<ShaderBuffer> {
    world
        .resource_mut::<Assets<ShaderBuffer>>()
        .add(storage_buffers::new(value))
}

fn material(
    world: &mut World,
    program: &MaterialProgram,
    name: &str,
) -> crate::draw_instance::GpuSemanticMaterialBinding {
    let mut assets = world.resource_mut::<Assets<Shader>>();
    let binding = pipeline_native::binding(program, &mut assets);
    for (index, (handle, source)) in [
        (&binding.shader, &binding.program.shader.wgsl),
        (
            &binding.multisampled_shader,
            &binding.program.multisampled_shader.wgsl,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let source = pipeline_native::composition::compose_material(
            source,
            binding.program.requires_scene_lighting(),
            pipeline_native::composition::Dialect::Bevy020,
        );
        assets
            .insert(
                handle.id(),
                Shader::from_wesl(source, format!("qualification/{name}_{index}.wesl")),
            )
            .unwrap();
    }
    binding
}

fn settle(app: &mut App, expected: &[(Entity, &[Entity])], draws: usize) {
    let mut expected_owners = expected
        .iter()
        .flat_map(|(_, entities)| entities.iter())
        .map(|entity| {
            app.world()
                .get::<crate::GpuDrawInstance>(*entity)
                .unwrap()
                .owner
        })
        .collect::<Vec<_>>();
    expected_owners.sort();
    assert_eq!(expected_owners.len(), draws);
    for _ in 0..160 {
        app.update();
        let snapshot = app.sub_app(RenderApp).world().resource::<Capture>();
        let matches = expected.iter().all(|(view, entities)| {
            snapshot.phases.get(view).is_some_and(|actual| {
                let mut actual = actual.clone();
                actual.sort();
                let mut expected = entities.to_vec();
                expected.sort();
                actual == expected
            })
        });
        let mut owners = snapshot.owners.clone();
        owners.sort();
        if matches && owners == expected_owners {
            assert_eq!(snapshot.preparation, 3);
            return;
        }
        std::thread::yield_now();
    }
    panic!(
        "scheduled queue did not converge for {expected:?} / {expected_owners:?}: {:?}",
        app.sub_app(RenderApp).world().resource::<Capture>()
    );
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_installed_queues_render_and_retire_through_real_schedule() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            }),
    );
    crate::extraction::install(&mut app);
    app.add_systems(Update, crate::wireframe::prepare_wireframe_geometry);
    let render = app.sub_app_mut(RenderApp);
    crate::render::install(render);
    render
        .init_resource::<AlphaSort>()
        .init_resource::<TrailCompaction>()
        .init_resource::<TrailCulling>()
        .init_resource::<Submissions>()
        .init_resource::<Capture>()
        .add_systems(
            Render,
            (
                mark_sort
                    .in_set(PrepareAlphaSort)
                    .in_set(RenderSystems::PrepareBindGroups),
                mark_compaction
                    .in_set(TrailCompactionSystems::Prepare)
                    .in_set(RenderSystems::PrepareBindGroups)
                    .after(PrepareAlphaSort),
                audit_bindings
                    .after(crate::draw_preparation::prepare_render_bind_groups)
                    .in_set(RenderSystems::PrepareBindGroups),
                fixture_mesh_counts
                    .in_set(TrailCompactionSystems::Compact)
                    .after(RenderSystems::PrepareBindGroups)
                    .before(RenderSystems::Render),
                capture.in_set(RenderSystems::Cleanup),
            ),
        );
    for (path, source) in [
        (
            "aestra_bevy_render/shaders/aestra_sprite_render.wesl",
            aestra_gpu::shader::SPRITE_RENDER_WESL.to_owned(),
        ),
        (
            "aestra_bevy_render/shaders/aestra_mesh_wireframe.wesl",
            aestra_gpu::shader::mesh_wireframe_wesl(),
        ),
    ] {
        app.world()
            .resource::<EmbeddedAssetRegistry>()
            .insert_asset(
                std::path::PathBuf::from(path),
                std::path::Path::new(path),
                source.into_bytes(),
            );
    }
    app.finish();
    app.cleanup();
    let info = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderAdapterInfo>();
    assert_ne!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "hardware GPU required"
    );
    assert_eq!(info.backend, wgpu::Backend::Vulkan);
    println!(
        "Native scheduled queues: {} / {:?} / {}",
        info.name, info.backend, info.driver_info
    );
    let device = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderDevice>()
        .clone();
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let mut cameras = Vec::new();
    for (order, two_d, layer) in [(0, true, 0), (1, false, 0), (2, false, 1)] {
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
            if layer == 1 { Msaa::Sample4 } else { Msaa::Off },
            Transform::from_xyz(0., 0., 5.).looking_at(Vec3::ZERO, Vec3::Y),
            RenderLayers::layer(layer),
        ));
        cameras.push(camera.id());
    }
    let world = app.world_mut();
    let texture = world.resource_mut::<Assets<Image>>().add(Image::new_fill(
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
    let mesh = world
        .resource_mut::<Assets<Mesh>>()
        .add(crate::draw_native::triangle(true));
    let renderers = asset(
        world,
        vec![GpuRenderer {
            emitter_index: 0,
            blend_mode: 0,
            softness: 1.,
            textured: 0,
            uv_min: [0.; 2].into(),
            uv_max: [1.; 2].into(),
            tint: [1.; 4].into(),
            particle_color: 1,
            renderer_kind: 0,
            frame_count: 1,
            playback_mode: 0,
            flipbook_flags: 0,
            frame_rate: 0.,
            attribute_flags: [0u32; 3].into(),
            frames: [[0.; 4].into(); aestra_gpu::MAX_FLIPBOOK_FRAMES],
        }],
    );
    let particles = asset(
        world,
        vec![GpuParticle {
            color: [1.; 4].into(),
            size: 1.,
            packed_emitter_alive: 1,
            ..default()
        }],
    );
    let alive = asset(world, vec![0u32; 16]);
    let aux = asset(world, vec![0u32; 64]);
    let identity = aestra_gpu::GpuGlobals::default().world_from_effect;
    let globals = asset(
        world,
        GpuRenderGlobals {
            world_from_effect: identity,
            ..default()
        },
    );
    let params = asset(
        world,
        GpuRenderParams {
            mesh_from_local: identity,
            ..default()
        },
    );
    let mut indirect_asset = storage_buffers::indirect(vec![4u32, 1, 0, 0]);
    storage_buffers::update(&mut indirect_asset, vec![4u32, 1, 0, 0]);
    assert_eq!(storage_buffers::bytes(&indirect_asset).unwrap().len(), 16);
    let indirect = world
        .resource_mut::<Assets<ShaderBuffer>>()
        .add(indirect_asset);
    let plain = material(
        world,
        &MaterialProgram::additive_sprite("Scheduled sprite"),
        "sprite",
    );
    let depth = material(world, &pipeline_native::depth_program(), "depth");
    let mesh_material = material(world, &crate::draw_native::mesh_program(), "mesh");
    let mut entities = Vec::new();
    for case in 0..6 {
        let mut draw = pipeline_native::draw(if case == 2 {
            depth.clone()
        } else if (3..=4).contains(&case) {
            mesh_material.clone()
        } else {
            plain.clone()
        });
        if case == 0 {
            draw.semantic_material = None;
        }
        if (3..=4).contains(&case) {
            draw.mesh = Some(mesh.clone());
        }
        if case == 4 {
            draw.render_mode = crate::GpuRenderMode::Wireframe;
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
        draw.renderer_order = case;
        let owner = world.spawn_empty().id();
        draw.owner = owner;
        entities.push(
            world
                .spawn((
                    draw,
                    Aabb::from_min_max(Vec3::splat(-1.), Vec3::splat(1.)),
                    RenderLayers::layer(usize::from(case == 5)),
                ))
                .id(),
        );
    }
    let [legacy, semantic, depth_draw, mesh_draw, wire, isolated] = entities.try_into().unwrap();
    let [two, three, msaa] = cameras.try_into().unwrap();
    settle(
        &mut app,
        &[
            (two, &[legacy, semantic, mesh_draw, wire]),
            (three, &[legacy, semantic, depth_draw, mesh_draw, wire]),
            (msaa, &[isolated]),
        ],
        10,
    );
    // The isolated MSAA view must pick the multisampled scene-depth command.
    app.world_mut()
        .get_mut::<crate::GpuDrawInstance>(isolated)
        .unwrap()
        .semantic_material = Some(depth.clone());
    settle(
        &mut app,
        &[
            (two, &[legacy, semantic, mesh_draw, wire]),
            (three, &[legacy, semantic, depth_draw, mesh_draw, wire]),
            (msaa, &[isolated]),
        ],
        10,
    );
    // Newly unsupported materials must retire their old retained 2D entry.
    app.world_mut()
        .get_mut::<crate::GpuDrawInstance>(semantic)
        .unwrap()
        .semantic_material = Some(depth);
    settle(
        &mut app,
        &[
            (two, &[legacy, mesh_draw, wire]),
            (three, &[legacy, semantic, depth_draw, mesh_draw, wire]),
            (msaa, &[isolated]),
        ],
        9,
    );
    app.world_mut()
        .get_mut::<crate::GpuDrawInstance>(mesh_draw)
        .unwrap()
        .mesh = Some(Handle::default());
    settle(
        &mut app,
        &[
            (two, &[legacy, wire]),
            (three, &[legacy, semantic, depth_draw, wire]),
            (msaa, &[isolated]),
        ],
        7,
    );
    app.world_mut()
        .get_mut::<crate::GpuDrawInstance>(semantic)
        .unwrap()
        .semantic_material = Some(plain.clone());
    app.world_mut()
        .get_mut::<crate::GpuDrawInstance>(mesh_draw)
        .unwrap()
        .mesh = Some(mesh.clone());
    app.world_mut()
        .entity_mut(legacy)
        .insert(Visibility::Hidden);
    settle(
        &mut app,
        &[
            (two, &[semantic, mesh_draw, wire]),
            (three, &[semantic, depth_draw, mesh_draw, wire]),
            (msaa, &[isolated]),
        ],
        8,
    );
    app.world_mut()
        .entity_mut(legacy)
        .insert(Visibility::Visible);
    settle(
        &mut app,
        &[
            (two, &[legacy, semantic, mesh_draw, wire]),
            (three, &[legacy, semantic, depth_draw, mesh_draw, wire]),
            (msaa, &[isolated]),
        ],
        10,
    );
    app.world_mut()
        .entity_mut(legacy)
        .insert(RenderLayers::layer(1));
    settle(
        &mut app,
        &[
            (two, &[semantic, mesh_draw, wire]),
            (three, &[semantic, depth_draw, mesh_draw, wire]),
            (msaa, &[isolated, legacy]),
        ],
        9,
    );
    app.world_mut()
        .entity_mut(legacy)
        .insert(RenderLayers::layer(0));
    settle(
        &mut app,
        &[
            (two, &[legacy, semantic, mesh_draw, wire]),
            (three, &[legacy, semantic, depth_draw, mesh_draw, wire]),
            (msaa, &[isolated]),
        ],
        10,
    );
    {
        let mut draw = app
            .world_mut()
            .get_mut::<crate::GpuDrawInstance>(mesh_draw)
            .unwrap();
        draw.mesh = None;
        draw.semantic_material = Some(plain);
    }
    settle(
        &mut app,
        &[
            (two, &[legacy, semantic, mesh_draw, wire]),
            (three, &[legacy, semantic, depth_draw, mesh_draw, wire]),
            (msaa, &[isolated]),
        ],
        10,
    );
    {
        let world = app.sub_app_mut(RenderApp).world_mut();
        let render_entity = world
            .query::<(Entity, &MainEntity)>()
            .iter(world)
            .find(|(_, main)| main.id() == mesh_draw)
            .unwrap()
            .0;
        assert!(
            world.get::<PreparedMeshDraw>(render_entity).is_none(),
            "mesh-to-sprite change must retire geometry and its pipeline layout"
        );
    }
    {
        let mut draw = app
            .world_mut()
            .get_mut::<crate::GpuDrawInstance>(mesh_draw)
            .unwrap();
        draw.mesh = Some(mesh);
        draw.semantic_material = Some(mesh_material);
    }
    settle(
        &mut app,
        &[
            (two, &[legacy, semantic, mesh_draw, wire]),
            (three, &[legacy, semantic, depth_draw, mesh_draw, wire]),
            (msaa, &[isolated]),
        ],
        10,
    );
    app.world_mut().despawn(wire);
    app.world_mut().despawn(msaa);
    settle(
        &mut app,
        &[
            (two, &[legacy, semantic, mesh_draw]),
            (three, &[legacy, semantic, depth_draw, mesh_draw]),
        ],
        7,
    );
    assert!(
        !app.sub_app(RenderApp)
            .world()
            .resource::<Capture>()
            .phases
            .contains_key(&msaa)
    );
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
        "scheduled render validation failed: {error:?}"
    );
}
