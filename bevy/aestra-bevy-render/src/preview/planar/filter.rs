//! Per-source HDR mip filtering between mirror capture and receiver rendering.
use bevy::{
    asset::embedded_asset,
    core_pipeline::schedule::{Core3d, Core3dSystems},
    extract::extract_component::{ExtractComponent, ExtractComponentPlugin},
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        diagnostic::RecordDiagnostics,
        render_asset::RenderAssets,
        render_resource::{
            binding_types::{texture_2d, texture_storage_2d},
            *,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        texture::GpuImage,
    },
};
use std::collections::{HashMap, HashSet};

#[derive(Component, Clone, ExtractComponent)]
#[extract_component_sync_target(Self)]
#[extract_app(RenderApp)]
pub(super) struct Source {
    pub capture: Handle<Image>,
    pub filtered: Handle<Image>,
}

#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    id: CachedComputePipelineId,
}
struct Entry {
    capture: Texture,
    filtered: Texture,
    levels: Vec<(BindGroup, UVec2)>,
}
#[derive(Resource, Default)]
struct Chains(HashMap<AssetId<Image>, Entry>);

pub(super) fn install(app: &mut App) {
    app.add_plugins(ExtractComponentPlugin::<Source, RenderApp>::default());
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .init_resource::<Chains>()
            .add_systems(RenderStartup, init)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareBindGroups))
            .add_systems(
                Core3d,
                filter
                    .after(Core3dSystems::Prepass)
                    .before(Core3dSystems::MainPass),
            );
        embedded_asset!(app, "filter.wgsl");
    }
}

fn init(mut commands: Commands, assets: Res<AssetServer>, cache: Res<PipelineCache>) {
    let layout = BindGroupLayoutDescriptor::new(
        "Aestra planar HDR mips",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_storage_2d(TextureFormat::Rgba16Float, StorageTextureAccess::WriteOnly),
            ),
        ),
    );
    let id = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("Aestra planar radiance reduction".into()),
        layout: vec![layout.clone()],
        shader: assets.load("embedded://aestra_bevy_render/preview/planar/filter.wgsl"),
        entry_point: Some("downsample".into()),
        ..default()
    });
    commands.insert_resource(Pipeline { layout, id });
}

fn prepare(
    sources: Query<&Source>,
    images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    pipeline: Res<Pipeline>,
    mut chains: ResMut<Chains>,
) {
    let mut retained = HashSet::new();
    for source in &sources {
        let (Some(capture), Some(filtered)) =
            (images.get(&source.capture), images.get(&source.filtered))
        else {
            continue;
        };
        let key = source.filtered.id();
        retained.insert(key);
        if chains.0.get(&key).is_some_and(|e| {
            e.capture.id() == capture.texture.id() && e.filtered.id() == filtered.texture.id()
        }) {
            continue;
        }
        let mut levels = Vec::new();
        for level in 1..filtered.texture.mip_level_count() {
            let view = |mip| {
                filtered.texture.create_view(&TextureViewDescriptor {
                    base_mip_level: mip,
                    mip_level_count: Some(1),
                    ..default()
                })
            };
            let input = view(level - 1);
            let output = view(level);
            let group = device.create_bind_group(
                "Aestra planar mip",
                &cache.get_bind_group_layout(&pipeline.layout),
                &BindGroupEntries::sequential((&input, &output)),
            );
            let size = UVec2::new(
                (filtered.texture.width() >> level).max(1),
                (filtered.texture.height() >> level).max(1),
            );
            levels.push((group, size));
        }
        chains.0.insert(
            key,
            Entry {
                capture: capture.texture.clone(),
                filtered: filtered.texture.clone(),
                levels,
            },
        );
    }
    chains.0.retain(|key, _| retained.contains(key));
}

fn filter(
    view: ViewQuery<&Source>,
    chains: Res<Chains>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    mut context: RenderContext,
) {
    let source = view.into_inner();
    let (Some(entry), Some(pipeline)) = (
        chains.0.get(&source.filtered.id()),
        cache.get_compute_pipeline(pipeline.id),
    ) else {
        return;
    };
    let recorder = context.diagnostic_recorder();
    let recorder = recorder.as_deref();
    let span = recorder.time_span(context.command_encoder(), "aestra_planar_mips");
    context.command_encoder().copy_texture_to_texture(
        entry.capture.as_image_copy(),
        entry.filtered.as_image_copy(),
        entry.capture.size(),
    );
    let mut pass = context
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor {
            label: Some("Aestra planar mip chain"),
            ..default()
        });
    pass.set_pipeline(pipeline);
    for (group, size) in &entry.levels {
        pass.set_bind_group(0, group, &[]);
        pass.dispatch_workgroups(size.x.div_ceil(8), size.y.div_ceil(8), 1);
    }
    drop(pass);
    span.end(context.command_encoder());
}
