use crate::{
    ActiveBackend, EffectRuntimeStatus, PresentedEffect, TextureAssetCache,
    gpu::GpuFallbackTextures,
};
use aestra_runtime::{
    FlipbookFrameContext, MaterialColorPlan, RendererPlanKind, flipbook_frame_index,
};
use bevy::{
    camera::visibility::RenderLayers,
    ecs::system::SystemParam,
    math::Rect,
    prelude::{
        AssetServer, Assets, Children, Color, Commands, Component, Entity, Image, Quat, Query, Res,
        ResMut, Sprite, Transform, Vec2, Vec3, Visibility, Without,
    },
};
use std::time::Instant;

#[derive(Component)]
pub(crate) struct PresentedParticle {
    sample_index: usize,
    renderer_index: usize,
}

#[derive(Component)]
pub(crate) struct CpuPresentationPrepared;

pub(crate) fn prepare_cpu_effects(
    mut commands: Commands,
    effects: Query<
        (Entity, &PresentedEffect, &EffectRuntimeStatus),
        Without<CpuPresentationPrepared>,
    >,
) {
    for (entity, effect, runtime) in &effects {
        if !matches!(
            runtime.active,
            ActiveBackend::CpuReference | ActiveBackend::GpuReadback
        ) {
            continue;
        }
        let capacity = effect.effect().max_particles.min(4096);
        let renderer_capacity = effect
            .effect()
            .emitters
            .iter()
            .filter(|emitter| emitter.enabled)
            .map(|emitter| emitter.renderers.len())
            .max()
            .unwrap_or(0);
        commands
            .entity(entity)
            .insert(CpuPresentationPrepared)
            .with_children(|parent| {
                for sample_index in 0..capacity {
                    for renderer_index in 0..renderer_capacity {
                        parent.spawn((
                            PresentedParticle {
                                sample_index,
                                renderer_index,
                            },
                            Sprite::from_color(Color::WHITE, Vec2::ONE),
                            Transform::default(),
                            Visibility::Hidden,
                        ));
                    }
                }
            });
    }
}

#[derive(SystemParam)]
pub(crate) struct TexturePresentationAssets<'w> {
    asset_server: Res<'w, AssetServer>,
    images: Res<'w, Assets<Image>>,
    texture_cache: ResMut<'w, TextureAssetCache>,
    fallback_textures: Res<'w, GpuFallbackTextures>,
}

pub(crate) fn present_cpu_effects(
    mut textures: TexturePresentationAssets,
    mut effects: Query<(
        &mut PresentedEffect,
        Option<&Children>,
        &EffectRuntimeStatus,
    )>,
    mut particles: Query<(
        &PresentedParticle,
        &mut Sprite,
        &mut Transform,
        &mut Visibility,
    )>,
) {
    for (mut effect, children, runtime) in &mut effects {
        if runtime.active == ActiveBackend::Gpu {
            effect.cpu_evaluation_time = None;
            continue;
        }
        let uses_gpu_readback =
            runtime.active == ActiveBackend::GpuReadback && !effect.gpu_samples.is_empty();
        let samples = if uses_gpu_readback {
            effect.cpu_evaluation_time = None;
            std::mem::take(&mut effect.gpu_samples)
        } else {
            let mut samples = std::mem::take(&mut effect.cpu_samples);
            let started = Instant::now();
            effect.instance.evaluate(&mut samples);
            effect.cpu_evaluation_time = Some(started.elapsed());
            samples
        };

        let Some(children) = children else {
            restore_samples(&mut effect, uses_gpu_readback, samples);
            continue;
        };
        for child in children.iter() {
            let Ok((slot, mut sprite, mut transform, mut visibility)) = particles.get_mut(*child)
            else {
                continue;
            };
            let Some(sample) = samples.get(slot.sample_index) else {
                *visibility = Visibility::Hidden;
                continue;
            };
            let Some(emitter) = effect.effect().emitters.get(sample.emitter_index) else {
                *visibility = Visibility::Hidden;
                continue;
            };
            let Some(renderer) = emitter.renderers.get(slot.renderer_index) else {
                *visibility = Visibility::Hidden;
                continue;
            };
            let Some(material) = effect.effect().material(renderer.material) else {
                *visibility = Visibility::Hidden;
                continue;
            };
            let (texture, uv) = match &renderer.kind {
                RendererPlanKind::Mesh { .. }
                | RendererPlanKind::Ribbon { .. }
                | RendererPlanKind::Trail { .. } => {
                    *visibility = Visibility::Hidden;
                    continue;
                }
                RendererPlanKind::Sprite => (material.texture, material.uv),
                RendererPlanKind::Flipbook {
                    flipbook,
                    time_source,
                    playback,
                    random_start,
                } => {
                    let Some(flipbook) = effect.effect().flipbook(*flipbook) else {
                        *visibility = Visibility::Hidden;
                        continue;
                    };
                    let frame = flipbook_frame_index(
                        flipbook,
                        FlipbookFrameContext {
                            time_source: *time_source,
                            playback: *playback,
                            random_start: *random_start,
                            effect_time: playhead_time(&effect),
                            normalized_age: sample.normalized_age,
                            particle_index: sample.particle_index,
                            seed: effect.instance.seed(),
                        },
                    );
                    (Some(flipbook.texture), flipbook.frames[frame])
                }
            };
            sprite.rect = None;
            sprite.image = textures.fallback_textures.white.clone();
            if let Some(texture) = texture
                && let Some(asset) = effect
                    .effect()
                    .assets
                    .iter()
                    .find(|asset| asset.source == texture)
            {
                let handle = textures
                    .texture_cache
                    .load(&textures.asset_server, &asset.path);
                if let Some(image) = textures.images.get(&handle) {
                    let image_size = image.size_f32();
                    sprite.rect = Some(Rect::from_corners(
                        Vec2::from_array(uv.min) * image_size,
                        Vec2::from_array(uv.max) * image_size,
                    ));
                    sprite.image = handle;
                } else {
                    sprite.image = textures.fallback_textures.missing.clone();
                }
            }
            let color = match &material.color {
                MaterialColorPlan::ParticleColor => sample.color,
                MaterialColorPlan::Value(value) => {
                    *value.resolve(effect.instance.parameter_values())
                }
            };
            sprite.color = Color::srgba(color[0], color[1], color[2], color[3]);
            sprite.custom_size = Some(Vec2::splat(sample.size.max(0.01)));
            transform.translation = Vec3::from_array(sample.position);
            transform.rotation = Quat::from_rotation_z(sample.rotation);
            transform.scale = Vec3::ONE;
            *visibility = Visibility::Visible;
        }
        restore_samples(&mut effect, uses_gpu_readback, samples);
    }
}

/// Compose after propagation so nonuniform parent scale retains the full affine
/// transform rather than a lossy TRS decomposition on each pooled particle.
#[allow(clippy::type_complexity)]
pub(crate) fn sync_particle_globals(
    mut commands: Commands,
    effects: Query<
        (
            &PresentedEffect,
            &bevy::prelude::GlobalTransform,
            Option<&RenderLayers>,
        ),
        Without<PresentedParticle>,
    >,
    mut particles: Query<
        (
            Entity,
            &bevy::prelude::ChildOf,
            &Transform,
            &mut bevy::prelude::GlobalTransform,
            Option<&RenderLayers>,
        ),
        bevy::prelude::With<PresentedParticle>,
    >,
) {
    for (entity, parent, local, mut global, layers) in &mut particles {
        if let Ok((effect, placement, desired_layers)) = effects.get(parent.parent()) {
            // RenderLayers do not inherit through Bevy's entity hierarchy.
            if layers != desired_layers {
                if let Some(layers) = desired_layers {
                    commands.entity(entity).insert(layers.clone());
                } else {
                    commands.entity(entity).remove::<RenderLayers>();
                }
            }
            let matrix = bevy::prelude::Mat4::from(placement.affine())
                * bevy::prelude::Mat4::from_cols_array(
                    &effect
                        .instance
                        .host_transform_context()
                        .matrix_at(effect.simulation_time()),
                )
                * local.to_matrix();
            *global = bevy::prelude::GlobalTransform::from(matrix);
        }
    }
}

fn restore_samples(
    effect: &mut PresentedEffect,
    gpu: bool,
    samples: Vec<aestra_runtime::ParticleSample>,
) {
    if gpu {
        effect.gpu_samples = samples;
    } else {
        effect.cpu_samples = samples;
    }
}

fn playhead_time(effect: &PresentedEffect) -> f32 {
    let compiled = effect.effect();
    if compiled.playback_mode.is_looping() && compiled.duration > 0.0 {
        effect.simulation_time().rem_euclid(compiled.duration)
    } else {
        effect
            .simulation_time()
            .clamp(0.0, compiled.duration.max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;
    use std::sync::Arc;

    #[test]
    fn cpu_particles_keep_composed_affine_transform_after_propagation() {
        let effect = aestra_core::EffectAsset::new("CPU nested placement", 2.0);
        let compiled = Arc::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&effect)
                .unwrap(),
        );
        let mut player = PresentedEffect::new(compiled);
        let a = aestra_core::EmitterTransform {
            scale: [2.0, 0.5, 1.0],
            ..default()
        };
        let b = aestra_core::EmitterTransform {
            rotation: Quat::from_rotation_z(0.7).to_array(),
            ..default()
        };
        player.instance.set_inherited_host_transform(Arc::new(
            aestra_runtime::InheritedHostTransform::default()
                .for_child(None, a, 0.0)
                .for_child(None, b, 0.0),
        ));
        let mut app = App::new();
        app.add_plugins(bevy::transform::TransformPlugin)
            .add_systems(
                PostUpdate,
                sync_particle_globals.after(bevy::transform::TransformSystems::Propagate),
            );
        let placement = Transform::from_xyz(10.0, 20.0, 30.0);
        let host = app
            .world_mut()
            .spawn((player, placement, RenderLayers::layer(7)))
            .id();
        let local = Transform::from_xyz(1.0, 2.0, 3.0);
        let particle = app
            .world_mut()
            .spawn((
                ChildOf(host),
                local,
                PresentedParticle {
                    sample_index: 0,
                    renderer_index: 0,
                },
            ))
            .id();
        for _ in 0..3 {
            app.update();
            let expected = placement.to_matrix()
                * crate::host_transform::matrix(a)
                * crate::host_transform::matrix(b)
                * local.to_matrix();
            let actual = Mat4::from(
                app.world()
                    .get::<GlobalTransform>(particle)
                    .unwrap()
                    .affine(),
            );
            assert!(actual.abs_diff_eq(expected, 1e-5));
            assert_eq!(
                app.world().get::<RenderLayers>(particle),
                Some(&RenderLayers::layer(7))
            );
        }
        app.world_mut().entity_mut(host).remove::<RenderLayers>();
        app.update();
        assert!(app.world().get::<RenderLayers>(particle).is_none());
    }
}
