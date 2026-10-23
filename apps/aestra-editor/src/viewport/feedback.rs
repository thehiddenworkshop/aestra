//! Production editor preview mocks, recorded inputs, output history and event-drop feedback.
use super::{presentation::PreviewPresentedEffect, runtime_preview::PreviewEffectInstancePath};
use crate::session::EditorSession;
use aestra_bevy_render::PresentedEffect;
use aestra_runtime::CompiledEffect;
use bevy::prelude::*;

/// Drives the edited effect's bindings from its preview stand-ins (host bindings HB11c), and its
/// inputs from those sent in the Interface section (event system E2): the root players and the
/// session's own instance read the same recorded input, so seeking and scrubbing replay it exactly.
/// Nested effects keep reading what their parents forward.
pub(super) fn apply_preview_mocks(
    mut session: ResMut<EditorSession>,
    mut traces: Local<crate::preview_mocks::PreviewMockTraces>,
    mut players: Query<
        (&PreviewEffectInstancePath, &mut PresentedEffect),
        With<PreviewPresentedEffect>,
    >,
) {
    let mocks = &session.preview_mocks;
    for (path, mut player) in &mut players {
        if !path.0.is_empty() {
            continue;
        }
        let trace = traces.trace(player.effect(), mocks);
        if crate::preview_mocks::needs_trace(&player.instance, trace.as_ref()) {
            player.instance.set_binding_trace(trace);
        }
        // The inputs sent from the Interface section (event system E2), replayed by every seek.
        if player.instance.received_events() != session.preview_inputs.as_slice() {
            player
                .instance
                .set_received_events(session.preview_inputs.clone());
        }
    }
    let Some(effect) = session.preview().map(|preview| preview.effect().clone()) else {
        return;
    };
    let trace = traces.trace(&effect, &session.preview_mocks);
    if session
        .preview()
        .is_some_and(|preview| crate::preview_mocks::needs_trace(preview, trace.as_ref()))
        && let Some(preview) = session.bypass_change_detection().preview_mut()
    {
        preview.set_binding_trace(trace);
    }
    let inputs = session.preview_inputs.clone();
    if session
        .preview()
        .is_some_and(|preview| preview.received_events() != inputs.as_slice())
        && let Some(preview) = session.bypass_change_detection().preview_mut()
    {
        preview.set_received_events(inputs);
    }
}

/// Children each authored event link lost, from the GPU totals of the compiled links (event system
/// E0). Compiled links are the authored ones between enabled emitters, in order, so each maps back by
/// its two emitters and trigger.
pub(super) fn event_link_drops(
    effect: &aestra_core::EffectAsset,
    compiled: &CompiledEffect,
    statistics: &aestra_bevy_render::gpu::GpuEventLinkStatistics,
) -> std::collections::BTreeMap<aestra_core::EventId, u64> {
    let mut drops = std::collections::BTreeMap::new();
    for (link, &dropped) in compiled.event_links.iter().zip(&statistics.dropped) {
        if dropped == 0 {
            continue;
        }
        let emitter = |index: usize| compiled.emitters.get(index).map(|emitter| emitter.source);
        let (Some(source), Some(target)) = (emitter(link.source), emitter(link.target)) else {
            continue;
        };
        if let Some(event) = effect.events.iter().find(|event| {
            event.source == source && event.target == target && event.trigger == link.trigger
        }) {
            drops.insert(event.id, dropped);
        }
    }
    drops
}

/// How many recent preview outputs the Interface section lists.
const PREVIEW_OUTPUTS_KEPT: usize = 8;

/// Keeps the latest outputs the edited effect's preview raised (event system E2b) for the Interface
/// section. Nested clips are left out: they are not the effect being edited.
pub(super) fn record_preview_outputs(
    mut session: ResMut<EditorSession>,
    mut outputs: MessageReader<aestra_bevy_render::gpu::AestraOutputEvent>,
    players: Query<&PreviewEffectInstancePath, With<PreviewPresentedEffect>>,
) {
    for output in outputs.read() {
        let root = players
            .get(output.effect)
            .is_ok_and(|path| path.0.is_empty() && output.clip_path.is_empty());
        if !root {
            continue;
        }
        session.preview_outputs.push_back(output.event.clone());
        while session.preview_outputs.len() > PREVIEW_OUTPUTS_KEPT {
            session.preview_outputs.pop_front();
        }
        session.preview_outputs_heard += 1;
    }
}

/// Keeps the session's per-link drop totals current for the Properties and Diagnostics panels,
/// rebuilding them only when a link starts or stops dropping.
pub(super) fn track_event_link_drops(
    mut session: ResMut<EditorSession>,
    players: Query<
        (
            &PresentedEffect,
            &PreviewEffectInstancePath,
            &aestra_bevy_render::gpu::GpuEventLinkStatistics,
        ),
        With<PreviewPresentedEffect>,
    >,
) {
    let drops = players
        .iter()
        .find(|(_, path, _)| path.0.is_empty())
        .map(|(player, _, statistics)| {
            event_link_drops(&session.effect, player.effect(), statistics)
        })
        .unwrap_or_default();
    if session.event_link_drops == drops {
        return;
    }
    let rebuild = !session.event_link_drops.keys().eq(drops.keys());
    session.event_link_drops = drops;
    if rebuild {
        session.ui_revision += 1;
    }
}

#[cfg(test)]
#[path = "tests/feedback.rs"]
mod tests;
