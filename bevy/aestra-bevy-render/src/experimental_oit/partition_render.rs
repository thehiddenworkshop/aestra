//! Real Bevy phase, bindings, depth, material shaders and indirect draw commands.
use super::*;
use bevy::{
    core_pipeline::oit::{
        OitBuffers, OrderIndependentTransparencySettingsOffset,
        resolve::{OitResolveBindGroup, OitResolvePipeline},
    },
    core_pipeline::prepass::DepthPrepass,
    render::{
        diagnostic::RecordDiagnostics,
        render_resource::*,
        renderer::{RenderContext, ViewQuery},
        view::{ViewDepthStencilTexture, ViewTarget, ViewUniformOffset, ViewUniforms},
    },
};

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn render(
    world: &World,
    view: ViewQuery<(
        &ExperimentalOitPartitions,
        &ExtractedView,
        &ViewTarget,
        &ViewDepthStencilTexture,
        &ViewUniformOffset,
        &OrderIndependentTransparencySettingsOffset,
        &OrderIndependentTransparencySettings,
        &AdapterResolveId,
        Has<DepthPrepass>,
    )>,
    held: Res<Transport>,
    buffers: Option<Res<OitBuffers>>,
    sort: Option<Res<super::super::sort::Pipeline>>,
    resolve: Option<Res<OitResolvePipeline>>,
    bind_group: Option<Res<OitResolveBindGroup>>,
    cache: Res<PipelineCache>,
    status: Res<ExperimentalOitPartitionStatus>,
    readbacks: Res<readback::Readbacks>,
    views: Res<ViewUniforms>,
    mut context: RenderContext,
) {
    let (Some(buffers), Some(resolve)) = (buffers, resolve) else {
        return;
    };
    let entity = view.entity();
    let (_, extracted, target, depth, uniform, settings_offset, settings, resolve_id, prepass) =
        view.into_inner();
    let Some(held) = held
        .phases
        .get(&extracted.retained_view_entity)
        .filter(|h| h.ready)
    else {
        return;
    };
    let (Some(heads), Some(counter)) = (buffers.heads.buffer(), buffers.atomic_counter.buffer())
    else {
        return;
    };
    let size = UVec2::new(extracted.viewport.z, extracted.viewport.w);
    if held.phase.items.is_empty() {
        // Queue ownership, not stale particle statistics, proves no capture can
        // write this frame. Keep one *GPU-observed* reset audit, rather than
        // manufacturing clean CPU counters or leaving the last active sample.
        let recorder = context.diagnostic_recorder();
        let recorder = recorder.as_deref();
        let aggregate = recorder.time_span(
            context.command_encoder(),
            "aestra_oit_partitions_with_health",
        );
        context.command_encoder().clear_buffer(heads, 0, None);
        context.command_encoder().clear_buffer(counter, 0, None);
        let audit = Region {
            x: 0,
            y: 0,
            w: size.x,
            h: size.y,
        };
        let bounds = readbacks.regions(&context, &[audit]);
        if let Some(health) = readbacks.begin(
            &mut context,
            &buffers,
            size,
            settings.sorted_fragment_max_count,
            &[audit],
            &bounds,
        ) {
            health.measure(&mut context, audit, 0);
            health.finish(&mut context, status.clone(), held.generation, true);
        }
        aggregate.end(context.command_encoder());
        return;
    }
    let (Some(sort), Some(bind_group), Some(depth_view), Some(pipeline)) = (
        sort,
        bind_group,
        depth.attachment.depth_stencil_views().depth_only_view(),
        cache.get_render_pipeline(resolve_id.0),
    ) else {
        return;
    };
    let Some(sort_pipeline) = cache.get_compute_pipeline(sort.region_id) else {
        return;
    };
    // Full target addressing and original projection, without a camera viewport.
    let depth_group = (!prepass).then(|| {
        context.render_device().create_bind_group(
            "Aestra partition resolve depth",
            &cache.get_bind_group_layout(&resolve.oit_depth_bind_group_layout),
            &BindGroupEntries::single(depth_view),
        )
    });
    let bounds = readbacks.regions(&context, &held.regions);
    let (Some(view_binding), Some(settings_binding), Some(node_binding), Some(head_binding)) = (
        views.uniforms.binding(),
        buffers.settings.binding(),
        buffers.nodes.binding(),
        buffers.heads.binding(),
    ) else {
        return;
    };
    let sort_groups: Vec<_> = (0..held.regions.len())
        .map(|i| {
            context.render_device().create_bind_group(
                "Aestra regional OIT sort bindings",
                &cache.get_bind_group_layout(&sort.region_layout),
                &BindGroupEntries::sequential((
                    view_binding.clone(),
                    settings_binding.clone(),
                    node_binding.clone(),
                    head_binding.clone(),
                    bounds.binding(i),
                )),
            )
        })
        .collect();
    let health = readbacks.begin(
        &mut context,
        &buffers,
        size,
        settings.sorted_fragment_max_count,
        &held.regions,
        &bounds,
    );
    let aggregate_recorder = context.diagnostic_recorder();
    let aggregate_recorder = aggregate_recorder.as_deref();
    let aggregate = aggregate_recorder.time_span(
        context.command_encoder(),
        "aestra_oit_partitions_with_health",
    );
    context.command_encoder().clear_buffer(heads, 0, None);
    context.command_encoder().clear_buffer(counter, 0, None);
    for (index, r) in held.regions.iter().enumerate() {
        {
            let recorder = context.diagnostic_recorder();
            let recorder = recorder.as_deref();
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("Aestra partition capture"),
                color_attachments: &[Some(target.get_color_attachment())],
                depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
                ..default()
            });
            let span = recorder.pass_span(&mut pass, format!("capture_{index}"));
            pass.set_scissor_rect(r.x, r.y, r.w, r.h);
            if let Err(error) = held.phase.render(&mut pass, world, entity) {
                // Real draw failures must not be reported as a healthy empty frame.
                panic!("partitioned OIT draw failed: {error:?}");
            }
            span.end(&mut pass);
        }
        if let Some(health) = &health {
            health.measure(&mut context, *r, index);
        }
        {
            let recorder = context.diagnostic_recorder();
            let recorder = recorder.as_deref();
            let mut pass = context
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("Aestra partition sort"),
                    ..default()
                });
            let span = recorder.pass_span(&mut pass, format!("sort_{index}"));
            pass.set_pipeline(sort_pipeline);
            pass.set_bind_group(
                0,
                &sort_groups[index],
                &[uniform.offset, settings_offset.offset],
            );
            pass.dispatch_workgroups(r.w.div_ceil(8), r.h.div_ceil(8), 1);
            span.end(&mut pass);
        }
        {
            let recorder = context.diagnostic_recorder();
            let recorder = recorder.as_deref();
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("Aestra partition resolve"),
                color_attachments: &[Some(target.get_color_attachment())],
                ..default()
            });
            let span = recorder.pass_span(&mut pass, format!("resolve_{index}"));
            pass.set_scissor_rect(r.x, r.y, r.w, r.h);
            pass.set_render_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[uniform.offset]);
            if let Some(group) = &depth_group {
                pass.set_bind_group(1, group, &[]);
            }
            pass.draw(0..3, 0..1);
            span.end(&mut pass);
        }
        // Explicit counter reset even when the entire region discarded. Resolve
        // clears occupied heads; all regions are disjoint and cover every pixel.
        context.command_encoder().clear_buffer(counter, 0, None);
    }
    // Clear defensively for native/ordinary mode transitions and view removal.
    context.command_encoder().clear_buffer(heads, 0, None);
    if let Some(health) = health {
        health.finish(&mut context, status.clone(), held.generation, false);
    }
    aggregate.end(context.command_encoder());
}
