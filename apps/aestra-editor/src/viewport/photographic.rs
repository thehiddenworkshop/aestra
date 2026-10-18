//! Production photographic camera response and transparent overlay composition.
use super::navigation::PreviewRenderCamera;
use crate::settings::EditorSettings;
use bevy::{camera::visibility::RenderLayers, prelude::*};

pub(super) fn sync_preview_photographic_response(
    mut commands: Commands,
    settings: Res<EditorSettings>,
    cameras: Query<(Entity, Option<&AppliedPhotographicResponse>), With<PreviewRenderCamera>>,
) {
    let desired = settings
        .preview
        .photographic_enabled
        .then_some(settings.preview.photographic.normalized());
    for (camera, applied) in &cameras {
        if applied.is_some_and(|applied| applied.0 == desired) {
            continue;
        }
        // A new legacy camera needs no mutation: preserve Bevy's baseline required components.
        if let Some(profile) = desired {
            profile.apply(&mut commands.entity(camera));
        } else if applied.is_some() {
            aestra_bevy_render::preview::restore_legacy_3d_response(&mut commands.entity(camera));
        }
        commands
            .entity(camera)
            .insert(AppliedPhotographicResponse(desired));
    }
}

#[derive(Component)]
pub(super) struct AppliedPhotographicResponse(
    Option<aestra_bevy_render::preview::PhotographicPreview>,
);

#[allow(clippy::type_complexity)]
pub(super) fn sync_photographic_overlay_composition(
    mut commands: Commands,
    settings: Res<EditorSettings>,
    mut cameras: Query<
        (
            Entity,
            &mut Camera,
            Option<&RenderLayers>,
            Has<IsDefaultUiCamera>,
            Option<&AppliedOverlayComposition>,
        ),
        Without<PreviewRenderCamera>,
    >,
) {
    let enabled = settings.preview.photographic_enabled;
    for (entity, mut camera, layers, ui, applied) in &mut cameras {
        if !ui && !layers.is_some_and(|layers| layers == &RenderLayers::layer(15)) {
            continue;
        }
        if applied.is_some_and(|applied| applied.0 == enabled) {
            continue;
        }
        if enabled {
            // HDR and LDR cameras have separate intermediate textures. Clear the overlay to
            // transparent, not an opaque/stale LDR image that would hide the tonemapped viewport.
            camera.clear_color = ClearColorConfig::Custom(Color::NONE);
            camera.output_mode = bevy::camera::CameraOutputMode::Write {
                blend_state: Some(bevy::render::render_resource::BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            };
        } else if applied.is_some() {
            camera.clear_color = ClearColorConfig::None;
            camera.output_mode = Default::default();
        }
        commands
            .entity(entity)
            .insert(AppliedOverlayComposition(enabled));
    }
}

#[derive(Component)]
pub(super) struct AppliedOverlayComposition(bool);

#[cfg(test)]
#[path = "photographic/tests.rs"]
mod tests;
