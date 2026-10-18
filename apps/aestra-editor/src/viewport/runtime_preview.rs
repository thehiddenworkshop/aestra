//! Production preview project resolution, player construction and instance reconciliation.
use super::presentation::PreviewPresentedEffect;
use crate::{ProjectEffectCatalog, session::EditorSession, timeline::TimelineState};
use aestra_bevy_render::PresentedEffect;
use aestra_core::EffectClipId;
use aestra_runtime::{
    CompiledEffect, CompiledEffectProject, CompiledParameterOverride, DEFAULT_PLAYBACK_TICK_RATE,
    SeekQuality,
};
use bevy::{camera::visibility::RenderLayers, prelude::*};
use std::sync::Arc;

#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct PreviewEffectInstancePath(pub(super) Vec<EffectClipId>);

#[derive(Resource, Default)]
pub(super) struct EditorPreviewProject {
    pub(super) source_root: Option<Arc<CompiledEffect>>,
    pub(super) project: Option<Arc<CompiledEffectProject>>,
    pub(super) error: Option<String>,
    pub(super) failed_source_revision: Option<u64>,
    pub(super) live_particle_count: usize,
}

#[derive(Clone)]
pub(super) struct DesiredPreviewInstance {
    pub(super) path: Vec<EffectClipId>,
    pub(super) effect: Arc<CompiledEffect>,
    pub(super) time: f32,
    pub(super) seed: u64,
    pub(super) transform: Transform,
    pub(super) inherited: Arc<aestra_runtime::InheritedHostTransform>,
    pub(super) parameter_overrides: Vec<CompiledParameterOverride>,
}

pub(super) fn configured_preview_player(session: &EditorSession) -> Option<PresentedEffect> {
    let preview = session.preview()?;
    Some(configured_preview_instance(
        preview.effect().clone(),
        session.simulation_time(),
        session.preview_seed,
        &[],
    ))
}

pub(super) fn configured_preview_instance(
    effect: Arc<CompiledEffect>,
    time: f32,
    seed: u64,
    parameter_overrides: &[CompiledParameterOverride],
) -> PresentedEffect {
    let mut player = PresentedEffect::new(effect);
    player
        .instance
        .apply_compiled_parameter_overrides(parameter_overrides);
    player.instance.set_seed(seed);
    player.instance.seek(time);
    player
}

pub(super) fn spawn_preview_effect_player(
    commands: &mut Commands,
    session: &EditorSession,
    transform: Transform,
) {
    if let Some(player) = configured_preview_player(session) {
        commands.spawn((
            PreviewPresentedEffect,
            PreviewEffectInstancePath::default(),
            player,
            transform,
            RenderLayers::layer(0),
        ));
    }
}

pub(super) fn sync_project_preview(
    mut session: ResMut<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    mut preview: ResMut<EditorPreviewProject>,
) {
    let source_root = session.preview().map(|preview| preview.effect().clone());
    if source_root.is_none()
        && preview.failed_source_revision == Some(session.document_revision())
        && !catalog.is_changed()
    {
        return;
    }
    if preview
        .source_root
        .as_ref()
        .zip(source_root.as_ref())
        .is_some_and(|(current, source)| Arc::ptr_eq(current, source))
        && !catalog.is_changed()
    {
        return;
    }

    preview.source_root = source_root.clone();
    match catalog.compile_project(&session.effect) {
        Ok(mut project) => {
            preview.failed_source_revision = None;
            if (catalog.is_changed() || source_root.is_none())
                && let Err(error) = session.install_compiled_project_root(project.root.clone())
            {
                preview.error = Some(error.to_string());
                return;
            }
            // The editor may be showing a transient root preview (for example a gizmo drag or
            // root-emitter solo). Dependencies still come from the resolved project, while the
            // root must remain exactly the artifact owned by the session.
            project.root = session
                .preview()
                .expect("preview was installed")
                .effect()
                .clone();
            preview.source_root = Some(project.root.clone());
            preview.project = Some(Arc::new(project));
            preview.error = None;
        }
        Err(error) => {
            preview.failed_source_revision = Some(session.document_revision());
            preview.project = source_root.map(|root| {
                Arc::new(CompiledEffectProject {
                    root,
                    dependencies: Default::default(),
                })
            });
            preview.error = Some(error);
            preview.live_particle_count = 0;
        }
    }
}

pub(super) fn desired_preview_instances(
    preview: &EditorPreviewProject,
    timeline: &TimelineState,
    time: f32,
    seed: u64,
) -> Vec<DesiredPreviewInstance> {
    let Some(project) = preview.project.as_deref() else {
        return Vec::new();
    };
    project
        .instances_with(
            time,
            seed,
            aestra_runtime::HostTransformContext {
                motion: project.root.host_transform_track.clone(),
                inherited: Arc::default(),
            },
            |path, clip| {
                let mut clip = clip.clone();
                if path.is_empty() {
                    if !timeline.effect_clip_is_audible(clip.source_clip) {
                        return None;
                    }
                    if let Some((start, offset, duration)) =
                        timeline.effect_clip_preview_timing(clip.source_clip)
                    {
                        clip.start_time = start;
                        clip.source_offset = offset;
                        clip.duration = duration;
                    }
                }
                Some(clip)
            },
        )
        .into_iter()
        .filter(|instance| !instance.path.is_empty() || !timeline.effect_clip_solo_active())
        .map(|instance| DesiredPreviewInstance {
            path: instance.path,
            effect: instance.effect,
            time: instance.time,
            seed: instance.seed,
            transform: Transform::IDENTITY,
            inherited: instance.inherited,
            parameter_overrides: instance.parameter_overrides,
        })
        .collect()
}

pub(super) fn spawn_preview_instance(commands: &mut Commands, desired: DesiredPreviewInstance) {
    let mut player = configured_preview_instance(
        desired.effect,
        desired.time,
        desired.seed,
        &desired.parameter_overrides,
    );
    player
        .instance
        .set_inherited_host_transform(desired.inherited);
    commands.spawn((
        PreviewPresentedEffect,
        PreviewEffectInstancePath(desired.path),
        player,
        desired.transform,
        RenderLayers::layer(0),
    ));
}

pub(super) fn sync_rendered_preview(
    mut commands: Commands,
    session: Res<EditorSession>,
    preview: Res<EditorPreviewProject>,
    timeline: Res<TimelineState>,
    mut previous_history_epoch: Local<Option<u32>>,
    mut players: Query<
        (
            Entity,
            &PreviewEffectInstancePath,
            &mut PresentedEffect,
            &mut Transform,
        ),
        With<PreviewPresentedEffect>,
    >,
) {
    let epoch = session.preview().map(|p| p.history_epoch());
    let discontinuity = *previous_history_epoch != epoch;
    *previous_history_epoch = epoch;
    let mut desired = desired_preview_instances(
        &preview,
        &timeline,
        session.simulation_time(),
        session.preview_seed,
    );
    for (entity, path, mut player, mut transform) in &mut players {
        let Some(index) = desired.iter().position(|item| item.path == path.0) else {
            commands.entity(entity).despawn();
            continue;
        };
        let instance = desired.swap_remove(index);
        if *transform != instance.transform {
            *transform = instance.transform;
        }
        if !Arc::ptr_eq(player.effect(), &instance.effect) {
            if compiled_effects_differ_only_in_live_edits(player.effect(), &instance.effect) {
                *player = configured_preview_instance(
                    instance.effect,
                    instance.time,
                    instance.seed,
                    &instance.parameter_overrides,
                );
            } else {
                commands.entity(entity).despawn();
                spawn_preview_instance(&mut commands, instance);
                continue;
            }
        }

        player
            .instance
            .set_inherited_host_transform(instance.inherited);
        if player.instance.seed() != instance.seed {
            player.instance.set_seed(instance.seed);
        }
        // Preview vs exact stateful seeking (hybrid roadmap M12): a history discontinuity is the
        // editor's "scrubbing this frame" signal, so request a bounded preview seek to keep dragging
        // responsive; a settled cursor (normal playback advance) refines to the authoritative exact
        // state. Only affects stateful effects; analytic ones seek directly regardless.
        player.set_seek_quality(if discontinuity {
            SeekQuality::Preview
        } else {
            SeekQuality::Exact
        });
        if discontinuity {
            player.instance.seek(instance.time);
        } else if (player.simulation_time() - instance.time).abs()
            > 0.5 / DEFAULT_PLAYBACK_TICK_RATE as f32
        {
            player.instance.set_playback_time(instance.time);
        }
    }
    for instance in desired {
        spawn_preview_instance(&mut commands, instance);
    }
}

pub(super) fn compiled_effects_differ_only_in_live_edits(
    current: &CompiledEffect,
    desired: &CompiledEffect,
) -> bool {
    if current.emitters.len() != desired.emitters.len() {
        return false;
    }
    let mut normalized = current.clone();
    for (emitter, desired_emitter) in normalized.emitters.iter_mut().zip(&desired.emitters) {
        emitter.transform = desired_emitter.transform;
    }
    for (clip, desired_clip) in normalized
        .effect_clips
        .iter_mut()
        .zip(&desired.effect_clips)
    {
        clip.transform = desired_clip.transform;
    }
    // Extension stages (fluid F3): the render world takes new constants and looks in place, so a
    // domain input edit or a dragged source keeps the simulation running. Anything that changes a
    // stage's shape still differs here and respawns the player.
    let live_stage = |stage: &mut aestra_runtime::CompiledExtensionStage,
                      desired: &aestra_runtime::CompiledExtensionStage| {
        if stage.block.constants.len() == desired.block.constants.len() {
            stage.block.constants.clone_from(&desired.block.constants);
        }
        if stage.modules.len() == desired.modules.len() {
            for (module, desired) in stage.modules.iter_mut().zip(&desired.modules) {
                module.parameters.clone_from(&desired.parameters);
            }
        }
        stage.presentations.clone_from(&desired.presentations);
    };
    for (stage, desired_stage) in normalized
        .extension_stages
        .iter_mut()
        .zip(&desired.extension_stages)
    {
        live_stage(stage, desired_stage);
    }
    for (emitter, desired_emitter) in normalized.emitters.iter_mut().zip(&desired.emitters) {
        for (stage, desired_stage) in emitter
            .extension_stages
            .iter_mut()
            .zip(&desired_emitter.extension_stages)
        {
            live_stage(stage, desired_stage);
        }
    }
    &normalized == desired
}

#[cfg(test)]
#[path = "runtime_preview/tests.rs"]
mod tests;
