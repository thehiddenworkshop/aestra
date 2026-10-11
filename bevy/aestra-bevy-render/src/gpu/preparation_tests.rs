//! The same preparation regressions run on both engine graphs.
use super::*;
use aestra_core::{EffectAsset, Emitter, MaterialExpressionId, MaterialParameterId, material::*};
use bevy::asset::{AssetApp, AssetPlugin};

pub(crate) fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_asset::<Shader>()
        .init_asset::<ShaderBuffer>()
        .init_resource::<ProjectAssetCache>()
        .init_resource::<particle_lights::AestraParticleLightSettings>()
        .init_resource::<crate::sampling::SpriteSampling>()
        .insert_resource(GpuCapabilities {
            max_particles: 262_144,
            max_storage_buffer_binding_size: 128 << 20,
            max_buffer_size: 256 << 20,
            max_compute_workgroups_per_dimension: 65_535,
            ..Default::default()
        })
        .add_systems(Update, prepare_gpu_effects);
    install(&mut app);
    app
}

pub(crate) fn source() -> EffectAsset {
    let mut source = EffectAsset::new("Preparation", 3.0);
    let mut emitter = Emitter::basic_sprite("Particles", 3.0);
    emitter.max_particles = 32;
    source.emitters.push(emitter);
    source
}

fn presented(source: &EffectAsset) -> PresentedEffect {
    PresentedEffect::new(Arc::new(
        aestra_compiler::EffectCompiler::default()
            .compile(source)
            .unwrap(),
    ))
}

fn status(active: ActiveBackend) -> EffectRuntimeStatus {
    EffectRuntimeStatus {
        active,
        reason: "preparation unit test (no device)".into(),
        compatibility: CompatibilityReport::compatible(crate::CompatibilityTarget::NativeGpu),
    }
}

fn draws(app: &mut App, owner: Entity) -> Vec<GpuDrawInstance> {
    app.world_mut()
        .query::<&GpuDrawInstance>()
        .iter(app.world())
        .filter(|draw| draw.owner == owner)
        .cloned()
        .collect()
}

#[test]
fn production_preparation_allocates_once_and_live_inputs_keep_all_handles() {
    let mut app = app();
    let owner = app
        .world_mut()
        .spawn((
            presented(&source()),
            status(ActiveBackend::Gpu),
            RenderLayers::layer(2),
        ))
        .id();
    app.update();
    let gpu = app.world().get::<GpuEffectBuffers>(owner).unwrap().clone();
    assert!(app.world().get::<GpuPresentationPrepared>(owner).is_some());
    assert_eq!(gpu.total_slots, 32);
    assert_eq!(
        app.world().get::<particle_lights::Pools>(owner).unwrap().0,
        vec![(0, 32)]
    );
    assert_eq!(draws(&mut app, owner).len(), 1);
    let children = app.world().get::<Children>(owner).unwrap().len();
    let initial = draws(&mut app, owner).pop().unwrap();
    let assets = app.world().resource::<Assets<ShaderBuffer>>().len();
    let before = storage_buffers::bytes(
        app.world()
            .resource::<Assets<ShaderBuffer>>()
            .get(&gpu.renderers)
            .unwrap(),
    )
    .unwrap()
    .to_vec();
    app.world_mut()
        .get_mut::<PresentedEffect>(owner)
        .unwrap()
        .set_render_mode(EffectRenderMode::Wireframe);
    *app.world_mut().get_mut::<RenderLayers>(owner).unwrap() = RenderLayers::layer(5);
    app.world_mut()
        .resource_mut::<crate::sampling::SpriteSampling>()
        .minimum_pixels = 2.0;
    for _ in 0..3 {
        app.update();
    }
    let current = app.world().get::<GpuEffectBuffers>(owner).unwrap();
    assert_eq!(
        (
            &gpu.particles,
            &gpu.globals,
            &gpu.render_globals,
            &gpu.emitters,
            &gpu.renderers
        ),
        (
            &current.particles,
            &current.globals,
            &current.render_globals,
            &current.emitters,
            &current.renderers
        )
    );
    assert_eq!(app.world().resource::<Assets<ShaderBuffer>>().len(), assets);
    assert_eq!(app.world().get::<Children>(owner).unwrap().len(), children);
    let after = draws(&mut app, owner).pop().unwrap();
    assert_eq!(after.render_params, initial.render_params);
    assert_eq!(after.render_mode, GpuRenderMode::Wireframe);
    let mut query = app.world_mut().query::<(&GpuDrawInstance, &RenderLayers)>();
    assert!(
        query
            .iter(app.world())
            .filter(|(draw, _)| draw.owner == owner)
            .all(|(_, layers)| *layers == RenderLayers::layer(5))
    );
    let packed = storage_buffers::bytes(
        app.world()
            .resource::<Assets<ShaderBuffer>>()
            .get(&gpu.renderers)
            .unwrap(),
    )
    .unwrap();
    assert_ne!(packed, before);
    // Initial mesh placement preserves relative nonuniform/negative scale.
    assert!(
        mesh_from_emitter(aestra_core::EmitterTransform {
            scale: [-2., 4., 1.],
            ..Default::default()
        })
        .abs_diff_eq(Mat4::from_scale(Vec3::new(-0.5, 1., 0.25)), 1e-6)
    );
}

#[test]
fn backend_gates_empty_effects_and_limit_rejections_do_not_allocate_particles() {
    let mut app = app();
    let pending = app
        .world_mut()
        .spawn((presented(&source()), status(ActiveBackend::Pending)))
        .id();
    let cpu = app
        .world_mut()
        .spawn((presented(&source()), status(ActiveBackend::CpuReference)))
        .id();
    let empty = app
        .world_mut()
        .spawn((
            presented(&EffectAsset::new("Empty", 1.)),
            status(ActiveBackend::Gpu),
        ))
        .id();
    app.update();
    for owner in [pending, cpu, empty] {
        assert!(app.world().get::<GpuEffectBuffers>(owner).is_none());
    }
    assert!(app.world().get::<GpuPresentationPrepared>(empty).is_some());
    assert!(
        app.world()
            .get::<GpuPresentationPrepared>(pending)
            .is_none()
    );
    assert_eq!(app.world().resource::<Assets<ShaderBuffer>>().len(), 0);
    app.world_mut()
        .resource_mut::<GpuCapabilities>()
        .max_particles = 1;
    let rejected = app
        .world_mut()
        .spawn((presented(&source()), status(ActiveBackend::Gpu)))
        .id();
    app.update();
    let rejected_status = app.world().get::<EffectRuntimeStatus>(rejected).unwrap();
    assert_eq!(rejected_status.active, ActiveBackend::CpuReference);
    assert!(rejected_status.reason.contains("unsupported"));
    assert!(app.world().get::<GpuEffectBuffers>(rejected).is_none());
    assert_eq!(app.world().resource::<Assets<ShaderBuffer>>().len(), 0);
}

#[test]
fn readback_backend_owns_a_single_particle_readback_without_native_draws() {
    let mut app = app();
    let owner = app
        .world_mut()
        .spawn((presented(&source()), status(ActiveBackend::GpuReadback)))
        .id();
    app.update();
    assert!(app.world().get::<GpuEffectBuffers>(owner).is_some());
    assert!(draws(&mut app, owner).is_empty());
    let mut query = app
        .world_mut()
        .query::<(&GpuReadbackOwner, &ChildOf, &Readback)>();
    let owners: Vec<_> = query
        .iter(app.world())
        .map(|(target, parent, _)| (target.0, parent.parent()))
        .collect();
    assert_eq!(owners, [(owner, owner)]);
    // This public observation remains unknown until a context-valid GPU sample arrives.
    assert!(
        GpuTrailStatistics::default()
            .usage(&presented(&source()).instance)
            .is_none()
    );
    assert_eq!(
        StatefulAppearance::plain().size.keys[0].to_array(),
        [0., 1.]
    );
}

#[test]
fn persistent_trails_allocate_history_scratch_and_context_owned_readbacks() {
    use aestra_core::{
        ModuleInstance, RENDERER_TRAIL, RendererProperties, TrailEndCap, TrailSamplingMode,
        TrailUvMode,
    };
    let mut app = app();
    let mut source = source();
    let emitter = &mut source.emitters[0];
    emitter.modules.push(ModuleInstance::persistent());
    emitter.renderers[0].renderer_type = aestra_core::RendererTypeId::new(RENDERER_TRAIL);
    emitter.renderers[0].properties = RendererProperties::Trail {
        width: 0.1,
        sample_interval: 0.125,
        lifetime: 0.75,
        max_points: 5,
        max_trails: 32,
        sampling: TrailSamplingMode::Time,
        sample_distance: 0.1,
        curve_tolerance: 0.01,
        uv_mode: TrailUvMode::Stretch,
        tile_length: 1.,
        end_cap: TrailEndCap::Flat,
    };
    let presented = presented(&source);
    let artifact = GpuEffectArtifact::from_instance(&presented.instance).unwrap();
    let bytes = super::super::storage_encoding::encode(&artifact.particles).len();
    let owner = app
        .world_mut()
        .spawn((presented, status(ActiveBackend::Gpu)))
        .id();
    app.update();
    let gpu = app.world().get::<GpuEffectBuffers>(owner).unwrap();
    assert!(gpu.stateful_only && gpu.has_trails);
    assert_eq!(gpu.stateful_dispatch.len(), 1);
    assert_eq!(gpu.stateful_dispatch[0].capacity, 32);
    assert_eq!(gpu.trail_roots.len(), 1);
    let buffers = app.world().resource::<Assets<ShaderBuffer>>();
    assert_eq!(
        storage_buffers::bytes(buffers.get(&gpu.particles).unwrap())
            .unwrap()
            .len(),
        bytes
    );
    assert_eq!(
        storage_buffers::bytes(buffers.get(&gpu.aux).unwrap())
            .unwrap()
            .len(),
        gpu.trail_plan.aux_words as usize * 4
    );
    assert_eq!(
        storage_buffers::bytes(buffers.get(&gpu.counters).unwrap())
            .unwrap()
            .len(),
        8 * 4
    );
    let counters = gpu.counters.clone();
    let mut query = app
        .world_mut()
        .query::<(&GpuTrailReadbackOwner, &ChildOf, &Readback)>();
    assert_eq!(
        query
            .iter(app.world())
            .filter(|(readback, parent, _)| readback.0 == owner && parent.parent() == owner)
            .count(),
        1
    );
    let draw = draws(&mut app, owner).pop().unwrap();
    assert!(draw.trail_instances.is_some());
    assert!(app.world().get::<GpuTrailStatistics>(owner).is_some());
    app.update();
    assert_eq!(
        app.world().get::<GpuEffectBuffers>(owner).unwrap().counters,
        counters
    );
}

pub(crate) fn material_program() -> (MaterialProgram, MaterialParameterId) {
    let mut program = MaterialProgram::additive_sprite("Live preparation material");
    let value = MaterialParameterId::from_u128(0x501);
    let expression = MaterialExpressionId::from_u128(0x502);
    program.parameters.push(MaterialParameter {
        id: value,
        name: "Opacity".into(),
        value_type: MaterialValueType::Float,
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Float(0.75)),
    });
    program.expressions.push(MaterialExpression {
        id: expression,
        kind: MaterialExpressionKind::Parameter(value),
    });
    program.outputs.alpha = expression;
    (program, value)
}

#[test]
fn live_material_values_reuse_shader_assets_and_removing_binding_clears_draw() {
    let mut app = app();
    let mut owner = presented(&source());
    let material = owner.effect().emitters[0].renderers[0].material;
    let (program, parameter) = material_program();
    let program = crate::material::compile_material_program(&program).unwrap();
    let mut binding =
        MaterialRuntimeBinding::new(program, MaterialRenderState::additive_sprite()).unwrap();
    owner.bind_material(material, binding.clone());
    let owner = app
        .world_mut()
        .spawn((owner, status(ActiveBackend::Gpu)))
        .id();
    app.update();
    let before = draws(&mut app, owner)
        .pop()
        .unwrap()
        .semantic_material
        .unwrap();
    let shaders = app.world().resource::<Assets<Shader>>().len();
    let buffers = app.world().resource::<Assets<ShaderBuffer>>().len();
    binding
        .set_value(parameter, MaterialValue::Float(0.25))
        .unwrap();
    app.world_mut()
        .get_mut::<PresentedEffect>(owner)
        .unwrap()
        .bind_material(material, binding);
    app.update();
    let after = draws(&mut app, owner)
        .pop()
        .unwrap()
        .semantic_material
        .unwrap();
    assert_eq!(before.shader, after.shader);
    assert_eq!(before.multisampled_shader, after.multisampled_shader);
    assert!(Arc::ptr_eq(&before.program, &after.program));
    assert_ne!(before.uniforms, after.uniforms);
    assert_eq!(app.world().resource::<Assets<Shader>>().len(), shaders);
    assert_eq!(
        app.world().resource::<Assets<ShaderBuffer>>().len(),
        buffers
    );
    app.world_mut()
        .get_mut::<PresentedEffect>(owner)
        .unwrap()
        .unbind_material(material);
    app.update();
    assert!(
        draws(&mut app, owner)
            .pop()
            .unwrap()
            .semantic_material
            .is_none()
    );
}
