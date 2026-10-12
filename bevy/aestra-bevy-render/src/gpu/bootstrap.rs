//! Production GPU startup, shared by shipping and the candidate engine adapter.
use super::{
    alpha_sort::{self, GpuAlphaSortStatistics},
    draw_resources, extension_stages, extraction, geometry_statistics, particle_lights,
    preparation::{self, GpuReadbackOwner},
    preparation_timing, render,
    simulation_graph::run_simulation,
    simulation_pipeline::{init_pipeline, prepare_bind_groups},
    simulation_timing,
    stateful_simulation::{StatefulStates, init_stateful_pipeline, prepare_stateful_states},
    trail_compaction, trail_culling, visibility_updates, volume,
};
use crate::{AestraRenderSettings, GpuCapabilities, PresentedEffect, capabilities::select_backend};
use aestra_gpu::{
    GpuParticle,
    shader::{SIMULATION_WESL, SPRITE_RENDER_WESL},
};
use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        gpu_readback::ReadbackComplete,
        renderer::{RenderGraph, RenderGraphSystems},
    },
};
use std::path::{Path, PathBuf};

pub(crate) fn install(app: &mut App) {
    let alpha_statistics = GpuAlphaSortStatistics::default();
    app.insert_resource(alpha_statistics.clone());
    install_shader_assets(app);
    particle_lights::install(app);
    extension_stages::install(app);
    volume::install(app);
    let timing_mailbox = simulation_timing::TimingMailbox::default();
    let preparation_mailboxes = preparation_timing::PreparationMailboxes::default();
    app.insert_resource(preparation_mailboxes.clone())
        .add_systems(PreUpdate, preparation_timing::receive);
    let geometry_mailbox = geometry_statistics::GeometryMailbox::default();
    app.insert_resource(geometry_mailbox.clone())
        .add_systems(PreUpdate, geometry_statistics::receive);
    app.insert_resource(timing_mailbox.clone())
        .add_systems(PreUpdate, simulation_timing::receive_timings);
    preparation::install(app);
    extraction::install(app);
    extraction::install_device_publication(app);
    visibility_updates::install(app);
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        let capabilities = GpuCapabilities::unavailable("Bevy has no render sub-application");
        let requested = app.world().resource::<AestraRenderSettings>().presentation;
        let status = select_backend(requested, &capabilities);
        app.insert_resource(capabilities).insert_resource(status);
        return;
    };
    render_app
        .insert_resource(alpha_statistics)
        .insert_resource(preparation_mailboxes)
        .insert_resource(geometry_mailbox)
        .init_resource::<geometry_statistics::Submissions>()
        .add_systems(
            RenderGraph,
            (
                geometry_statistics::begin
                    .after(RenderGraphSystems::Begin)
                    .before(RenderGraphSystems::Render),
                geometry_statistics::finish
                    .after(RenderGraphSystems::Render)
                    .before(RenderGraphSystems::Submit),
            ),
        )
        .insert_resource(timing_mailbox)
        .add_systems(RenderStartup, (init_pipeline, init_stateful_pipeline))
        .init_resource::<StatefulStates>()
        .add_systems(
            Render,
            (
                prepare_bind_groups,
                // Persistent stateful buffers are allocated alongside the analytic bind groups; the
                // dispatch that consumes them lands in the next increment.
                prepare_stateful_states,
            )
                .in_set(RenderSystems::PrepareBindGroups),
        )
        // Run inside the render-graph diagnostics window (after `Begin`) and before
        // the graph draws (`Render`), so the `aestra::gpu::simulate` GPU timestamp
        // span is captured and particles are simulated before they are rendered.
        .add_systems(
            RenderGraph,
            run_simulation
                .in_set(draw_resources::SimulateEffects)
                .after(RenderGraphSystems::Begin)
                .before(RenderGraphSystems::Render),
        );
    render::install(render_app);
    trail_culling::install(render_app);
    trail_compaction::install(render_app);
    alpha_sort::install(render_app);
}

fn install_shader_assets(app: &App) {
    let registry = app.world().resource::<EmbeddedAssetRegistry>();
    registry.insert_asset(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/gpu/alpha_sort.wgsl"),
        Path::new("aestra_bevy_render/shaders/alpha_sort.wgsl"),
        include_str!("alpha_sort.wgsl").as_bytes(),
    );
    let shader_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../aestra-gpu/src/shaders");
    registry.insert_asset(
        shader_root.join("aestra_trail_compact.wesl"),
        Path::new("aestra_bevy_render/shaders/aestra_trail_compact.wesl"),
        aestra_gpu::shader::trail_compact_wesl().into_bytes(),
    );
    registry.insert_asset(
        shader_root.join("aestra_trail_cull.wesl"),
        Path::new("aestra_bevy_render/shaders/aestra_trail_cull.wesl"),
        aestra_gpu::shader::trail_cull_wesl().into_bytes(),
    );
    registry.insert_asset(
        shader_root.join("aestra_mesh_wireframe.wesl"),
        Path::new("aestra_bevy_render/shaders/aestra_mesh_wireframe.wesl"),
        aestra_gpu::shader::mesh_wireframe_wesl().into_bytes(),
    );
    registry.insert_asset(
        shader_root.join("aestra_simulation.wesl"),
        Path::new("aestra_bevy_render/shaders/aestra_simulation.wesl"),
        SIMULATION_WESL.as_bytes(),
    );
    registry.insert_asset(
        shader_root.join("aestra_sprite_render.wesl"),
        Path::new("aestra_bevy_render/shaders/aestra_sprite_render.wesl"),
        SPRITE_RENDER_WESL.as_bytes(),
    );
    // The unified stateful simulation module (hybrid roadmap M6), composed from the proven aestra-gpu
    // primitives. Plain WGSL (no WESL composition), so it is embedded directly as .wgsl.
    registry.insert_asset(
        shader_root.join("aestra_stateful_simulation.wgsl"),
        Path::new("aestra_bevy_render/shaders/aestra_stateful_simulation.wgsl"),
        aestra_gpu::stateful_simulation_wgsl().into_bytes(),
    );
}

pub(crate) fn receive_readback(
    event: On<ReadbackComplete>,
    owners: Query<&GpuReadbackOwner>,
    mut players: Query<&mut PresentedEffect>,
) {
    let Ok(owner) = owners.get(event.event_target()) else {
        return;
    };
    let Ok(mut player) = players.get_mut(owner.0) else {
        return;
    };
    let particles: Vec<GpuParticle> = event.to_shader_type();
    player.gpu_samples.clear();
    player.gpu_samples.extend(
        particles
            .into_iter()
            .filter(|particle| particle.packed_emitter_alive & 0xffff != 0)
            .map(|particle| aestra_runtime::ParticleSample {
                emitter_index: (particle.packed_emitter_alive >> 16) as usize,
                particle_index: particle.particle_index,
                position: particle.position.to_array(),
                size: particle.size,
                rotation: particle.rotation,
                color: particle.color.to_array(),
                normalized_age: particle.normalized_age,
            }),
    );
}
