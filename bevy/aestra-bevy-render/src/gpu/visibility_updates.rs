//! Main-world draw-child transforms, bounds, wireframe geometry and culling policy.
//! Shared schedule installer for shipping and candidate engine adapters.
use super::{
    bounds,
    draw_instance::{GpuDrawInstance, HostMotionDraw},
    effect_inputs::GpuEffectBuffers,
    ribbon_bounds,
    transform_sync::{self, sync_gpu_render_transforms},
    wireframe,
};
use crate::PresentedEffect;
use aestra_gpu::GpuBlend;
use bevy::{camera::visibility, prelude::*};

pub(super) fn install(app: &mut App) {
    transform_sync::install(app);
    app.add_systems(
        PostUpdate,
        sync_host_motion_draw_globals
            .after(bevy::transform::TransformSystems::Propagate)
            .before(bounds::sync_mesh_bounds)
            .before(ribbon_bounds::sync_ribbon_bounds)
            .before(wireframe::prepare_wireframe_geometry)
            .before(sync_gpu_render_transforms),
    );
    app.add_systems(
        PostUpdate,
        sync_host_motion_draw_transforms.before(bevy::transform::TransformSystems::Propagate),
    );
    app.add_systems(
        PostUpdate,
        (
            bounds::sync_mesh_bounds,
            ribbon_bounds::sync_ribbon_bounds,
            wireframe::prepare_wireframe_geometry,
        )
            .after(bevy::transform::TransformSystems::Propagate)
            .before(visibility::VisibilitySystems::CheckVisibility),
    );
    app.add_systems(
        PostUpdate,
        sync_host_motion_replay_culling
            .after(bounds::sync_mesh_bounds)
            .after(ribbon_bounds::sync_ribbon_bounds)
            .before(visibility::VisibilitySystems::CheckVisibility),
    );
}

#[derive(Component)]
struct HostMotionReplayCulling;

fn sync_host_motion_draw_globals(
    players: Query<(&PresentedEffect, &GlobalTransform), Without<HostMotionDraw>>,
    mut draws: Query<(&ChildOf, &mut GlobalTransform), With<HostMotionDraw>>,
) {
    for (parent, mut global) in &mut draws {
        if let Ok((player, placement)) = players.get(parent.parent()) {
            let matrix = Mat4::from(placement.affine())
                * Mat4::from_cols_array(
                    &player
                        .instance
                        .host_transform_context()
                        .matrix_at(player.simulation_time()),
                );
            global.set_if_neq(GlobalTransform::from(matrix));
        }
    }
}

// During a bounded multi-frame seek the processed pose can lag the requested pose.
// Do not cull mixed sprite/mesh/ribbon draws at the future pose. Trails have their
// own per-camera world-history culling; ordinary bounds resume when motion is removed.
#[allow(clippy::type_complexity)]
fn sync_host_motion_replay_culling(
    mut commands: Commands,
    sampling: Option<Res<crate::sampling::SpriteSampling>>,
    players: Query<(&PresentedEffect, &GpuEffectBuffers)>,
    mut draws: Query<(
        Entity,
        &ChildOf,
        &mut GpuDrawInstance,
        Has<visibility::NoFrustumCulling>,
        Has<HostMotionReplayCulling>,
        Has<ribbon_bounds::RibbonBoundsSource>,
    )>,
) {
    let minimum_pixels = sampling
        .as_deref()
        .copied()
        .unwrap_or_default()
        .normalized()
        .minimum_pixels;
    for (entity, parent, mut draw, uncullable, was_forced, ribbon) in &mut draws {
        // A view-dependent pixel floor can expand a sprite beyond world-space AABBs.
        // Bypass unpadded main-world frustum culling; safe analytic draws receive a
        // padded per-view queue check instead. Unknown bounds still use GPU clip rejection.
        let sampled_sprite =
            minimum_pixels > 0.0 && draw.renderer_kind == 0 && draw.blend == GpuBlend::Additive;
        let replaying_host = players.get(parent.parent()).is_ok_and(|(player, gpu)| {
            gpu.has_trails && !player.instance.host_transform_context().is_identity()
        });
        if replaying_host {
            draw.sampled_sprite_cull = None;
        }
        let forced = sampled_sprite || replaying_host;
        if forced {
            if !uncullable || !was_forced {
                commands
                    .entity(entity)
                    .insert((HostMotionReplayCulling, visibility::NoFrustumCulling));
            }
        } else if was_forced {
            commands.entity(entity).remove::<HostMotionReplayCulling>();
            if draw.mesh.is_none() && !ribbon && draw.trail_instances.is_none() {
                commands
                    .entity(entity)
                    .remove::<visibility::NoFrustumCulling>();
            }
        }
    }
}

// Keep Bevy visibility/sorting transforms aligned with the matrix used by the shader.
// The host's own placement is never overwritten by animation.
fn sync_host_motion_draw_transforms(
    players: Query<&PresentedEffect>,
    mut draws: Query<(&ChildOf, &mut Transform), With<HostMotionDraw>>,
) {
    for (parent, mut transform) in &mut draws {
        if let Ok(player) = players.get(parent.parent()) {
            let desired = Transform::from_matrix(Mat4::from_cols_array(
                &player
                    .instance
                    .host_transform_context()
                    .matrix_at(player.simulation_time()),
            ));
            if *transform != desired {
                *transform = desired;
            }
        }
    }
}

#[cfg(test)]
#[path = "visibility_tests.rs"]
mod tests;
