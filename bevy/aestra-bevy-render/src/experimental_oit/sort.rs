//! Sort only overflowing pixel lists before resolve, reusing native node links.
use super::{OitApproved, diagnostics};
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    camera::MainPassResolutionOverride,
    core_pipeline::{
        core_3d::main_transparent_pass_3d,
        oit::{
            OitBuffers, OrderIndependentTransparencySettings,
            OrderIndependentTransparencySettingsOffset,
            resolve::{OitResolvePipelineId, node::oit_resolve},
        },
        schedule::{Core3d, Core3dSystems},
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        diagnostic::RecordDiagnostics,
        render_resource::{
            binding_types::{storage_buffer_sized, uniform_buffer},
            *,
        },
        renderer::{RenderContext, RenderDevice, ViewQuery},
        view::{ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    id: CachedComputePipelineId,
    group: Option<BindGroup>,
}

pub(super) fn install(app: &mut App) {
    if app.get_sub_app(RenderApp).is_none() {
        return;
    }
    embedded_asset!(app, "sort.wesl");
    app.sub_app_mut(RenderApp)
        .add_systems(RenderStartup, init)
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareBindGroups))
        .add_systems(
            Core3d,
            sort.after(main_transparent_pass_3d)
                .after(diagnostics::Measure)
                .before(oit_resolve)
                .in_set(Core3dSystems::MainPass),
        );
}

fn init(
    mut commands: Commands,
    assets: Res<AssetServer>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
) {
    if device.limits().max_storage_buffers_per_shader_stage < 2 {
        return;
    }
    let layout = BindGroupLayoutDescriptor::new(
        "aestra OIT overflow links",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<OrderIndependentTransparencySettings>(true),
                storage_buffer_sized(false, None),
                storage_buffer_sized(false, None),
            ),
        ),
    );
    let id = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("aestra OIT overflow merge sort".into()),
        layout: vec![layout.clone()],
        shader: load_embedded_asset!(&*assets, "sort.wesl"),
        ..default()
    });
    commands.insert_resource(Pipeline {
        layout,
        id,
        group: None,
    });
}

fn prepare(
    pipeline: Option<ResMut<Pipeline>>,
    approved: Query<(), With<OitApproved>>,
    buffers: Option<Res<OitBuffers>>,
    views: Res<ViewUniforms>,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
) {
    let Some(mut pipeline) = pipeline else { return };
    pipeline.group = None;
    if approved.is_empty() {
        return;
    }
    let Some(buffers) = buffers else { return };
    if let (Some(view), Some(settings), Some(nodes), Some(heads)) = (
        views.uniforms.binding(),
        buffers.settings.binding(),
        buffers.nodes.binding(),
        buffers.heads.binding(),
    ) {
        pipeline.group = Some(device.create_bind_group(
            "aestra OIT overflow links",
            &cache.get_bind_group_layout(&pipeline.layout),
            &BindGroupEntries::sequential((view, settings, nodes, heads)),
        ));
    }
}

#[allow(clippy::type_complexity)]
fn sort(
    view: ViewQuery<(
        &OitApproved,
        &ViewUniformOffset,
        &OrderIndependentTransparencySettingsOffset,
        &ExtractedCamera,
        &OitResolvePipelineId,
        Has<MainPassResolutionOverride>,
    )>,
    pipeline: Option<Res<Pipeline>>,
    cache: Res<PipelineCache>,
    mut context: RenderContext,
) {
    let Some(pipeline) = pipeline else { return };
    let (_, view, settings, camera, resolve, resolution_override) = view.into_inner();
    // Sub-viewports/overrides remain outside this experiment's qualified path.
    if camera.viewport.is_some()
        || resolution_override
        || cache.get_render_pipeline(resolve.0).is_none()
    {
        return;
    }
    let (Some(group), Some(compute), Some(size)) = (
        &pipeline.group,
        cache.get_compute_pipeline(pipeline.id),
        camera.physical_target_size,
    ) else {
        return;
    };
    let diagnostics = context.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let mut pass = context
        .command_encoder()
        .begin_compute_pass(&ComputePassDescriptor {
            label: Some("aestra OIT overflow sort"),
            ..default()
        });
    let span = diagnostics.pass_span(&mut pass, "aestra_oit_overflow_sort");
    pass.set_pipeline(compute);
    pass.set_bind_group(0, group, &[view.offset, settings.offset]);
    pass.dispatch_workgroups(size.x.div_ceil(8), size.y.div_ceil(8), 1);
    span.end(&mut pass);
}
