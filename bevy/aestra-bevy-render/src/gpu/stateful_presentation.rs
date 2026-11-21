//! Retain the last submitted pose while independent stateful emitters advance.
use super::{
    catchup_pacing::stateful_catchup_budget,
    coupled_simulation::joint_checkpoint_tick,
    effect_inputs::StatefulDispatch,
    stateful_simulation::{
        STATEFUL_TICK_DT, StatefulPersistentState, StatefulRenderBuffers, advance_stateful_emitter,
        is_live_advance, present_stateful_emitter, stateful_tick_time,
    },
};
use aestra_runtime::{DEFAULT_PLAYBACK_TICK_RATE, PresentationCadence, SeekQuality, trace_tick};
use bevy::render::{
    render_resource::{BindGroupLayout, Buffer, BufferId, CommandEncoder, ComputePipeline},
    renderer::RenderDevice,
};
use std::num::NonZeroU32;

const TICK_RATE: NonZeroU32 = NonZeroU32::new(DEFAULT_PLAYBACK_TICK_RATE).unwrap();
// The shader's storage layout starts with a mat4 followed by effect time.
const RENDER_TIME_OFFSET: u64 = 64;

pub(super) struct SteppedPresentation<'a> {
    pub(super) cadence: PresentationCadence,
    pub(super) globals: &'a Buffer,
}

#[derive(Clone, Copy)]
pub(super) struct HeldFrame {
    output: BufferId,
    cadence: PresentationCadence,
    tick: u32,
}

/// Advances the existing live state and writes the latest due sample into the
/// regular draw buffers before advancing any remaining ticks. No extra particle
/// buffers are needed. Out-of-step emitters use regular presentation until
/// their ordinary catch-up has aligned them.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_stepped_stateful(
    device: &RenderDevice,
    encoder: &mut CommandEncoder,
    (death, spawn, present, order): (
        &ComputePipeline,
        &ComputePipeline,
        &ComputePipeline,
        Option<&ComputePipeline>,
    ),
    layout: &BindGroupLayout,
    states: &mut [StatefulPersistentState],
    dispatches: &[StatefulDispatch],
    render: &StatefulRenderBuffers<'_>,
    request: SteppedPresentation<'_>,
    time: f32,
    quality: SeekQuality,
) -> Option<f32> {
    let uploads = render.uploads?;
    let first = states.first()?;
    let last = first.last_tick;
    if states.iter().any(|state| state.last_tick != last) {
        return None;
    }
    let target = trace_tick(time).min(u64::from(u32::MAX)) as u32;
    let rewind = states
        .iter_mut()
        .filter_map(|state| state.rewind.take())
        .min();
    let live = rewind.is_none() && is_live_advance(last, target);
    let output = render.particles.id();
    let cadence = request.cadence;
    let mut held = states[0]
        .held_frame
        .take()
        .filter(|held| held.output == output && held.cadence == cadence);
    let back_to = rewind.map_or(target, |rewind| target.min(rewind));
    if back_to < last {
        held = None;
        // Reconstruct the requested held pose as part of the existing seek. A
        // checkpoint inside the held interval would skip the pose's source tick.
        let source = cadence.sample_tick(target, TICK_RATE).min(back_to);
        let shared = joint_checkpoint_tick(states, &[], source, None);
        for state in states.iter_mut() {
            if let Some(tick) = shared {
                state.restore_at(encoder, tick);
            } else {
                state.reset_to_zero(device);
            }
        }
    }
    let start = states[0].last_tick;
    let reached = target.min(start.saturating_add(stateful_catchup_budget(quality)));
    let sample = cadence.sample_tick(reached, TICK_RATE);
    let publish = held.is_none_or(|held| sample > held.tick);
    // Enabling mid-playback seeds from the live tick until the next boundary.
    let capture_tick = sample.max(start);
    if publish {
        encoder.clear_buffer(render.counters, 0, Some(4));
    }
    for (state, dispatch) in states.iter_mut().zip(dispatches) {
        if publish {
            advance_stateful_emitter(
                device,
                encoder,
                (death, spawn),
                layout,
                state,
                dispatch,
                render,
                capture_tick,
                live,
            );
            present_stateful_emitter(
                device,
                encoder,
                present,
                order,
                layout,
                state,
                dispatch,
                render,
                capture_tick as f32 * STATEFUL_TICK_DT,
            );
        }
        advance_stateful_emitter(
            device,
            encoder,
            (death, spawn),
            layout,
            state,
            dispatch,
            render,
            reached,
            live,
        );
    }
    if publish {
        held = Some(HeldFrame {
            output,
            cadence,
            tick: capture_tick,
        });
    }
    let frame = held.expect("the first stepped pose is always written");
    states[0].held_frame = Some(frame);
    let shown_time = stateful_tick_time(frame.tick);
    uploads.borrow_mut().copy_at(
        device,
        encoder,
        request.globals,
        RENDER_TIME_OFFSET,
        &shown_time.to_le_bytes(),
    );
    // A one-shot may end between fixed ticks, and the host will reject future telemetry.
    Some(shown_time.min(time))
}
