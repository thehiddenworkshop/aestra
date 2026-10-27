//! Aestra-owned resolve for approved cameras only; no global Bevy shader override.
use super::OitApproved;
use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    core_pipeline::{
        FullscreenShader,
        oit::{
            OrderIndependentTransparencySettings,
            resolve::{OitResolvePipeline, OitResolvePipelineId, queue_oit_resolve_pipeline},
        },
        prepass::DepthPrepass,
    },
    prelude::*,
    render::{Render, RenderApp, RenderSystems, render_resource::*, view::ExtractedView},
    shader::ShaderDefVal,
};
use std::collections::HashMap;

pub(super) fn install(app: &mut App) {
    if app.get_sub_app(RenderApp).is_none() {
        return;
    }
    embedded_asset!(app, "resolve.wesl");
    app.sub_app_mut(RenderApp).add_systems(
        Render,
        queue
            .after(queue_oit_resolve_pipeline)
            .in_set(RenderSystems::Queue),
    );
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    format: TextureFormat,
    sorted_max: u32,
    prepass: bool,
}

fn queue(
    mut commands: Commands,
    cameras: Query<
        (
            Entity,
            &ExtractedView,
            &OrderIndependentTransparencySettings,
            Has<DepthPrepass>,
        ),
        With<OitApproved>,
    >,
    cache: Res<PipelineCache>,
    resolve: Option<Res<OitResolvePipeline>>,
    fullscreen: Option<Res<FullscreenShader>>,
    assets: Res<AssetServer>,
    mut pipelines: Local<HashMap<Key, CachedRenderPipelineId>>,
) {
    let (Some(resolve), Some(fullscreen)) = (resolve, fullscreen) else {
        return;
    };
    for (entity, view, settings, prepass) in &cameras {
        let key = Key {
            format: view.target_format,
            sorted_max: settings.sorted_fragment_max_count,
            prepass,
        };
        let id = *pipelines.entry(key).or_insert_with(|| {
            let mut layout = vec![resolve.view_bind_group_layout.clone()];
            let mut defs = vec![ShaderDefVal::UInt(
                "SORTED_FRAGMENT_MAX_COUNT".into(),
                key.sorted_max,
            )];
            if key.prepass {
                defs.push("DEPTH_PREPASS".into());
            } else {
                layout.push(resolve.oit_depth_bind_group_layout.clone());
            }
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("aestra_oit_resolve_premultiplied".into()),
                layout,
                vertex: fullscreen.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader: load_embedded_asset!(&*assets, "resolve.wesl"),
                    shader_defs: defs,
                    targets: vec![Some(ColorTargetState {
                        format: key.format,
                        blend: Some(BlendState {
                            color: BlendComponent::OVER,
                            alpha: BlendComponent::OVER,
                        }),
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                ..default()
            })
        });
        // Until this variant is ready, retain the native pipeline queued just
        // before us. Never make an already-working resolve skip list clearing
        // merely because the adapter's specialization is still compiling.
        if cache.get_render_pipeline(id).is_some() {
            commands.entity(entity).insert(OitResolvePipelineId(id));
        }
    }
}
