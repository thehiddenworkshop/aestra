//! Whole render-graph interval in the instrumented lane, not a sum of pass percentiles.
use bevy::{
    ecs::system::SystemState,
    prelude::*,
    render::{
        RenderApp,
        diagnostic::{RecordDiagnostics, resolve_encoder},
        renderer::{RenderContext, RenderGraph, RenderGraphSystems},
    },
};

pub(super) const CPU_PATH: &str = "render/fireworks_render_graph/elapsed_cpu";

pub(super) fn install(app: &mut App) {
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(
            RenderGraph,
            (
                begin.in_set(RenderGraphSystems::Begin),
                end.after(RenderGraphSystems::Render)
                    .after(
                        aestra_bevy::gpu::particle_light_readback::ParticleLightReadbackSet::Copy,
                    )
                    .before(resolve_encoder)
                    .before(RenderGraphSystems::Submit),
            ),
        );
    }
}

// Diagnostic span stacks are thread-local. Exclusive systems keep both ends
// on the schedule thread while GPU timestamps bracket all intervening commands.
fn begin(world: &mut World) {
    let mut state = SystemState::<RenderContext>::new(world);
    let mut context = state.get_mut(world).expect("render context is initialized");
    let recorder = context.diagnostic_recorder();
    recorder
        .as_deref()
        .begin_time_span(context.command_encoder(), "fireworks_render_graph".into());
    state.apply(world);
}

fn end(world: &mut World) {
    let mut state = SystemState::<RenderContext>::new(world);
    let mut context = state.get_mut(world).expect("render context is initialized");
    let recorder = context.diagnostic_recorder();
    recorder.as_deref().end_time_span(context.command_encoder());
    state.apply(world);
}
