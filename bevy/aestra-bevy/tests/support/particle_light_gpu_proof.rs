//! F7E3 test-only bridge, NOT a shipping adapter/API. One source, one reserved
//! shadowless Bevy light slot, layer 0, GPU clustering only. All other setups
//! fail closed. No readback, replay or changes to StandardMaterial shaders.
use aestra_bevy::gpu::particle_lights::{
    AestraParticleLightSettings, GpuSelectedParticleLights, ParticleLightSelectionSet,
};
use bevy::{
    camera::visibility::RenderLayers,
    light::cluster::GlobalClusterSettings,
    pbr::{GlobalClusterableObjectMeta, GpuClusteredLight},
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_resource::*,
        renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue},
        sync_world::MainEntity,
        view::ExtractedView,
    },
};
use std::sync::{Arc, Mutex};

#[derive(Component, Clone, ExtractComponent)]
pub struct ProofSlot;

#[derive(Resource, Clone, Default, ExtractResource)]
pub struct ProofSettings {
    pub enabled: bool,
    pub owner: Option<Entity>,
}

#[derive(Debug, Clone, Default)]
pub struct Observation {
    pub dispatches: u64,
    pub sequence: u64,
    pub rejection: Option<String>,
}

#[derive(Resource, Clone, Default, ExtractResource)]
pub struct ProofStatistics(pub Arc<Mutex<Observation>>);

#[derive(Resource, Clone, ExtractResource)]
struct ProofShader(Handle<Shader>);
#[derive(Resource)]
struct ProofPipeline {
    layout: BindGroupLayoutDescriptor,
    id: CachedComputePipelineId,
}
#[derive(ShaderType)]
struct Params {
    destination: UVec4,
    limits: Vec4,
}

pub struct ProofPlugin;
impl Plugin for ProofPlugin {
    fn build(&self, app: &mut App) {
        let shader = app
            .world_mut()
            .resource_mut::<Assets<Shader>>()
            .add(Shader::from_wgsl(
                include_str!("particle_light_gpu_proof.wgsl"),
                file!(),
            ));
        app.insert_resource(ProofShader(shader))
            .init_resource::<ProofSettings>()
            .init_resource::<ProofStatistics>()
            .add_plugins((
                ExtractComponentPlugin::<ProofSlot>::default(),
                ExtractResourcePlugin::<ProofSettings>::default(),
                ExtractResourcePlugin::<ProofStatistics>::default(),
                ExtractResourcePlugin::<ProofShader>::default(),
            ));
        app.sub_app_mut(RenderApp)
            .add_systems(Render, init.in_set(RenderSystems::PrepareResources))
            .add_systems(
                RenderGraph,
                inject
                    .after(ParticleLightSelectionSet)
                    .before(RenderGraphSystems::Render),
            );
    }
}

fn init(
    mut commands: Commands,
    shader: Res<ProofShader>,
    settings: Res<ProofSettings>,
    existing: Option<Res<ProofPipeline>>,
    cache: Res<PipelineCache>,
) {
    if !settings.enabled || existing.is_some() {
        return;
    }
    let layout = BindGroupLayoutDescriptor::new(
        "F7E3 one-slot clustered GPU proof",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(48),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(16),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: Some(GpuClusteredLight::min_size()),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(Params::min_size()),
                },
                count: None,
            },
        ],
    );
    let id = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("F7E3 direct clustered GPU light".into()),
        layout: vec![layout.clone()],
        shader: shader.0.clone(),
        entry_point: Some("inject".into()),
        ..default()
    });
    commands.insert_resource(ProofPipeline { layout, id });
}

#[allow(clippy::too_many_arguments)]
fn inject(
    mut context: RenderContext,
    settings: Res<ProofSettings>,
    selected: Res<GpuSelectedParticleLights>,
    selection: Res<AestraParticleLightSettings>,
    clusters: Res<GlobalClusterSettings>,
    meta: Res<GlobalClusterableObjectMeta>,
    slots: Query<(Entity, &MainEntity), With<ProofSlot>>,
    views: Query<
        Option<&RenderLayers>,
        (
            With<ExtractedView>,
            With<bevy::render::camera::ExtractedCamera>,
        ),
    >,
    sources: Query<(&MainEntity, Option<&RenderLayers>)>,
    pipeline: Option<Res<ProofPipeline>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    stats: Res<ProofStatistics>,
) {
    let mut observation = stats.0.lock().unwrap();
    observation.rejection = None;
    if !settings.enabled {
        return;
    }
    let reject = |o: &mut Observation, why: &str| {
        o.rejection = Some(why.into());
    };
    if !clusters.supports_storage_buffers || clusters.gpu_clustering.is_none() {
        reject(
            &mut observation,
            "proof requires Bevy GPU clustering/storage",
        );
        return;
    }
    if views.iter().count() != 1
        || views
            .iter()
            .any(|layers| layers.is_some_and(|l| *l != RenderLayers::layer(0)))
    {
        reject(&mut observation, "proof supports one view on layer 0");
        return;
    }
    let Some(frame) = selected.frame() else {
        return;
    };
    if selection.max_lights != 1
        || frame.selected_capacity != 1
        || frame.manifest.len() != 1
        || Some(frame.manifest[0].owner) != settings.owner
        || frame.manifest[0].root != frame.manifest[0].owner
        || !frame.manifest[0].clip_path.is_empty()
    {
        reject(
            &mut observation,
            "proof supports exactly one root/output and selected slot",
        );
        return;
    }
    if !sources.iter().any(|(owner, layers)| {
        owner.id() == frame.manifest[0].owner && layers.is_none_or(|l| *l == RenderLayers::layer(0))
    }) {
        reject(&mut observation, "proof supports source layer 0 only");
        return;
    }
    let Ok((entity, _)) = slots.single() else {
        reject(&mut observation, "proof needs one reserved slot");
        return;
    };
    let Some(&index) = meta.entity_to_index.get(&entity) else {
        reject(&mut observation, "reserved light absent from Bevy frame");
        return;
    };
    let Some(BindingResource::Buffer(target)) = meta.gpu_clustered_lights.binding() else {
        reject(&mut observation, "missing Bevy light storage");
        return;
    };
    if !target.buffer.usage().contains(BufferUsages::STORAGE) {
        reject(&mut observation, "uniform light buffers unsupported");
        return;
    }
    let Some(pipeline) = pipeline else {
        return;
    };
    let Some(compiled) = cache.get_compute_pipeline(pipeline.id) else {
        if let CachedPipelineState::Err(error) = cache.get_compute_pipeline_state(pipeline.id) {
            observation.rejection = Some(format!("GPU bridge shader: {error}"));
        }
        return;
    };
    let mut uniform = UniformBuffer::from(Params {
        destination: UVec4::new(index as u32, 1, 0, 0),
        limits: Vec4::new(1_000_000.0, 200.0, 0.0, 0.0),
    });
    uniform.write_buffer(&device, &queue);
    let group = device.create_bind_group(
        "F7E3 direct clustered GPU light",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            frame.records.as_entire_buffer_binding(),
            frame.counters.as_entire_buffer_binding(),
            BindingResource::Buffer(target),
            uniform.binding().unwrap(),
        )),
    );
    {
        let mut pass = context
            .command_encoder()
            .begin_compute_pass(&ComputePassDescriptor {
                label: Some("F7E3 direct selected light before Bevy GPU clustering"),
                timestamp_writes: None,
            });
        pass.set_pipeline(compiled);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    observation.dispatches += 1;
    observation.sequence = frame.sequence;
}
