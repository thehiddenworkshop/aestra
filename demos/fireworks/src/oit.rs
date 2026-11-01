//! Explicit host admission. No camera/quality fallback is silently called OIT.
use super::{AudienceCamera, project::Options};
use aestra_bevy::{
    ActiveBackend, AestraRuntimeStatus, CompiledEffectProject, ExperimentalOitSettings,
    ExperimentalOitStatus, PresentedEffect,
};
use bevy::prelude::*;

#[derive(Resource, Default, Debug)]
pub struct Admission {
    pub ready: bool,
    pub renderers: usize,
    pub sprites: usize,
    pub trails: usize,
    pub target_preparation_updates: u64,
}

pub fn settings(options: &Options) -> ExperimentalOitSettings {
    ExperimentalOitSettings {
        fragments_per_pixel_average: options.oit_fragments_per_pixel,
        sorted_fragment_max_count: 32,
        max_storage_bytes: 128 * 1024 * 1024,
    }
}

pub fn audit(project: &CompiledEffectProject) -> Result<Admission, String> {
    let mut result = Admission::default();
    for effect in std::iter::once(&project.root).chain(project.dependencies.values()) {
        if !effect.extension_stages.is_empty() {
            return Err(format!(
                "OIT host does not qualify extension rendering: {}",
                effect.name
            ));
        }
        let presented = PresentedEffect::new(effect.clone());
        for emitter in &effect.emitters {
            for renderer in &emitter.renderers {
                let binding = presented
                    .material_binding_for_emitter(renderer.material, emitter.source)
                    .ok_or_else(|| {
                        format!(
                            "OIT host requires a resolved semantic material: {}/{}",
                            effect.name, emitter.name
                        )
                    })?;
                if !ExperimentalOitSettings::supports_renderer(
                    &renderer.kind,
                    aestra_bevy::BlendMode::Alpha,
                    Some(binding.render_state()),
                ) {
                    return Err(format!(
                        "OIT unsupported renderer/material: {}/{}",
                        effect.name, emitter.name
                    ));
                }
                result.renderers += 1;
                match renderer.kind {
                    aestra_bevy::RendererPlanKind::Sprite
                    | aestra_bevy::RendererPlanKind::Flipbook { .. } => result.sprites += 1,
                    aestra_bevy::RendererPlanKind::Trail { .. } => result.trails += 1,
                    _ => {}
                }
            }
        }
    }
    if result.renderers == 0 {
        return Err("OIT show has no eligible renderers".into());
    }
    Ok(result)
}

/// The normal demo stops on observed loss. Qualification reports retain a full
/// failed run for diagnosis; completion is deliberately not a loss-free verdict.
#[allow(clippy::too_many_arguments)]
pub fn guard(
    options: Res<Options>,
    mut admission: ResMut<Admission>,
    cameras: Query<Option<&ExperimentalOitStatus>, With<AudienceCamera>>,
    runtime: Res<AestraRuntimeStatus>,
    effects: Query<(&aestra_bevy::EffectRuntimeStatus, &PresentedEffect)>,
    mut exit: MessageWriter<AppExit>,
    mut players: Query<&mut aestra_bevy::EffectPlayer>,
    diagnostics: Option<Res<aestra_bevy::ExperimentalOitDiagnostics>>,
    reporting: Option<Res<super::report::Report>>,
    partitions: Option<Res<aestra_bevy::ExperimentalOitPartitionStatus>>,
) {
    if !options.experimental_oit {
        admission.ready = true;
        return;
    }
    admission.ready = false;
    // The render-thread partition mailbox can lag main-world camera changes.
    // Wait for the current target before interpreting old partition admission;
    // only the renderer's explicit preparation state is retryable.
    let camera_status = cameras.single().ok().flatten();
    let target_preparing = camera_status.is_none_or(ExperimentalOitStatus::is_preparing);
    if camera_status.is_some_and(ExperimentalOitStatus::is_preparing) {
        admission.target_preparation_updates += 1;
    }
    if let Some(status) = camera_status
        && !status.is_preparing()
        && let Some(reason) = status.rejection
    {
        for mut player in &mut players {
            player.playing = false;
        }
        error!(
            "Experimental fireworks OIT rejected: {reason} (estimated {} bytes); no silent fallback",
            status.estimated_storage_bytes
        );
        exit.write(AppExit::error());
        return;
    }
    if options.oit_partitions.is_some() {
        let Some(status) = partitions.and_then(|p| p.snapshot()) else {
            return;
        };
        if (!target_preparing && status.rejection.is_some())
            || (reporting.is_none() && status.observed_loss)
        {
            for mut player in &mut players {
                player.playing = false;
            }
            error!("Experimental partitioned transparency rejected or lost fragments: {status:?}");
            exit.write(AppExit::error());
            return;
        }
        if !status.ready && !target_preparing {
            return;
        }
    }
    if reporting.is_none()
        && let Some(sample) = diagnostics.and_then(|d| d.snapshot())
        && let Some(reason) = capacity_failure(&sample)
    {
        error!(
            "Experimental fireworks OIT failed observed capacity/integrity: {reason}; stopping, no silent fallback"
        );
        for mut player in &mut players {
            player.playing = false;
        }
        exit.write(AppExit::error());
        return;
    }
    if effects.iter().any(|(status, effect)| {
        effect
            .effect()
            .emitters
            .iter()
            .any(|e| !e.renderers.is_empty())
            && !matches!(status.active, ActiveBackend::Gpu | ActiveBackend::Pending)
    }) {
        error!("Experimental fireworks OIT rejected an effect's non-native presentation backend");
        for mut player in &mut players {
            player.playing = false;
        }
        exit.write(AppExit::error());
        return;
    }
    if let Some(status) = camera_status {
        if status.is_preparing() {
            return;
        }
        if runtime.active == ActiveBackend::Gpu {
            admission.ready = true;
        } else if runtime.active != ActiveBackend::Pending {
            for mut player in &mut players {
                player.playing = false;
            }
            error!(
                "Experimental fireworks OIT requires native GPU presentation, got {:?}",
                runtime.active
            );
            exit.write(AppExit::error());
        }
    }
}

pub(super) fn capacity_failure(
    sample: &aestra_bevy::ExperimentalOitDiagnosticSample,
) -> Option<String> {
    (sample.dropped_fragments > 0 || sample.invalid_links > 0).then(|| {
        format!(
            "{} fragments dropped, {} invalid links",
            sample.dropped_fragments, sample.invalid_links
        )
    })
}

#[cfg(test)]
#[path = "oit_tests.rs"]
mod tests;
