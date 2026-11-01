//! Opt-in additive planar presentation. This owns cameras/targets, never players.
use super::water::WaterMaterial;
use bevy::{
    camera::{
        CameraProjection, CameraUpdateSystems, Hdr, RenderTarget,
        visibility::{RenderLayers, VisibilitySystems},
    },
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    extract::extract_component::{ExtractComponent, ExtractComponentPlugin},
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{TextureFormat, TextureUsages},
        view::RenderVisibleEntities,
    },
    transform::TransformSystems,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
mod filter;
mod math;

/// Attach to a perspective source camera. The receiver must be a horizontal
/// WaterMaterial mesh. Alpha smoke is deliberately excluded from this first pass.
#[derive(Component, Clone, Debug)]
pub struct PlanarReflection {
    pub receiver: Entity,
    pub resolution_scale: f32,
    pub target_budget_bytes: u64,
}
impl PlanarReflection {
    pub fn new(receiver: Entity) -> Self {
        Self {
            receiver,
            resolution_scale: 0.5,
            target_budget_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReflectionSnapshot {
    pub target_size: UVec2,
    pub estimated_target_bytes: u64,
    pub queued_additive_draws: usize,
    pub omitted_transparent_draws: usize,
    pub rejection: Option<&'static str>,
}
/// CPU configuration/queued work, not a GPU-completion or live-particle counter.
#[derive(Component, Clone, Default)]
pub struct PlanarReflectionStatus(Arc<Mutex<ReflectionSnapshot>>);
impl PlanarReflectionStatus {
    pub fn snapshot(&self) -> ReflectionSnapshot {
        self.0.lock().unwrap().clone()
    }
}

#[derive(Component, Clone, ExtractComponent)]
#[extract_component_sync_target(Self)]
#[extract_app(RenderApp)]
pub(crate) struct ReflectionView {
    pub(crate) status: PlanarReflectionStatus,
}
impl ReflectionView {
    pub(crate) fn publish_draws(&self, queued: usize, omitted: usize) {
        let mut status = self.status.0.lock().unwrap();
        status.queued_additive_draws = queued;
        status.omitted_transparent_draws = omitted;
    }
}

#[derive(Component, Clone, Copy, ExtractComponent)]
#[extract_component_sync_target(Self)]
#[extract_app(RenderApp)]
struct ReflectionExcluded;

struct Capture {
    receiver: Entity,
    material: Handle<WaterMaterial>,
    camera: Entity,
    image: Handle<Image>,
    filtered: Handle<Image>,
    size: UVec2,
    status: PlanarReflectionStatus,
}
#[derive(Resource, Default)]
struct Captures(HashMap<Entity, Capture>);

pub struct PlanarReflectionPlugin;
impl Plugin for PlanarReflectionPlugin {
    fn build(&self, app: &mut App) {
        filter::install(app);
        app.init_resource::<Captures>()
            .add_plugins((
                ExtractComponentPlugin::<ReflectionView, RenderApp>::default(),
                ExtractComponentPlugin::<ReflectionExcluded, RenderApp>::default(),
            ))
            .add_systems(
                PostUpdate,
                sync.after(TransformSystems::Propagate)
                    .before(CameraUpdateSystems)
                    .before(VisibilitySystems::UpdateFrusta),
            );
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.add_systems(
                Render,
                exclude_surfaces
                    .after(RenderSystems::PrepareMeshes)
                    .after(bevy::render::view::collect_visible_cpu_culled_entities)
                    .before(RenderSystems::QueueMeshes),
            );
        }
    }
}

fn clear(materials: &mut Assets<WaterMaterial>, capture: &Capture) {
    if let Some(mut material) = materials.get_mut(&capture.material) {
        material.extension.clear_reflection();
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync(
    mut commands: Commands,
    sources: Query<
        (
            Entity,
            &Camera,
            &Projection,
            &GlobalTransform,
            &PlanarReflection,
            Option<&RenderLayers>,
            Option<&PlanarReflectionStatus>,
            Option<Ref<crate::gpu::EffectPipelinePreparation>>,
        ),
        Without<ReflectionView>,
    >,
    surfaces: Query<
        (
            Entity,
            &MeshMaterial3d<WaterMaterial>,
            &GlobalTransform,
            Option<&Visibility>,
            Option<&InheritedVisibility>,
            Has<ReflectionExcluded>,
        ),
        Without<ReflectionView>,
    >,
    mut views: Query<
        (
            &mut Camera,
            &mut Transform,
            &mut GlobalTransform,
            &mut Projection,
        ),
        With<ReflectionView>,
    >,
    mut captures: ResMut<Captures>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    runtime: Option<Res<crate::AestraRuntimeStatus>>,
    orphaned: Query<Entity, (With<PlanarReflectionStatus>, Without<PlanarReflection>)>,
) {
    for entity in &orphaned {
        commands.entity(entity).remove::<PlanarReflectionStatus>();
    }
    let retired: Vec<_> = captures
        .0
        .keys()
        .filter(|e| sources.get(**e).is_err())
        .copied()
        .collect();
    for entity in retired {
        let entry = captures.0.remove(&entity).unwrap();
        clear(&mut materials, &entry);
        commands.entity(entry.camera).despawn();
        if let Ok(mut source) = commands.get_entity(entity) {
            source.remove::<PlanarReflectionStatus>();
        }
        images.remove(entry.image.id());
        images.remove(entry.filtered.id());
        if let Ok(mut source) = commands.get_entity(entity) {
            source.remove::<filter::Source>();
        }
    }
    if !sources.is_empty() {
        for (entity, .., excluded) in &surfaces {
            if !excluded {
                commands.entity(entity).insert(ReflectionExcluded);
            }
        }
    }
    for (entity, source, projection, world, request, layers, previous_status, preparation) in
        &sources
    {
        let status = previous_status.cloned().unwrap_or_default();
        if previous_status.is_none() {
            commands.entity(entity).insert(status.clone());
        }
        let configuration = (|| {
            if runtime
                .as_ref()
                .is_some_and(|r| r.active != crate::ActiveBackend::Gpu)
            {
                return Err("reflection requires native GPU presentation");
            }
            let (_, surface, surface_world, visibility, inherited, _) = surfaces
                .get(request.receiver)
                .map_err(|_| "missing water receiver")?;
            if !materials.contains(&surface.0) {
                return Err("missing water material");
            }
            if sources.iter().any(|(other, _, _, _, r, ..)| {
                other != entity
                    && surfaces
                        .get(r.receiver)
                        .is_ok_and(|(_, s, ..)| s.0 == surface.0)
            }) {
                return Err("each source view needs its own water material");
            }
            let size = source
                .physical_viewport_size()
                .ok_or("source target unavailable")?;
            let Projection::Perspective(base) = projection else {
                return Err("perspective source required");
            };
            if !base.fov.is_finite()
                || !(0.001..std::f32::consts::PI - 0.001).contains(&base.fov)
                || !base.near.is_finite()
                || base.near <= 0.0
            {
                return Err("invalid perspective source");
            }
            let height = surface_world.translation().y;
            if !surface_world.to_matrix().is_finite()
                || surface_world.up().dot(Vec3::Y) < 0.9999
                || !world.to_matrix().is_finite()
            {
                return Err("finite horizontal water and camera transforms required");
            }
            if !source.is_active
                || visibility == Some(&Visibility::Hidden)
                || inherited.is_some_and(|v| !v.get())
                || world.translation().y <= height + 0.02
            {
                return Err("source inactive, receiver hidden or camera below water");
            }
            if !math::sees_plane(world, base) {
                return Err("water plane outside source view");
            }
            let (target_size, bytes) =
                math::extent(size, request.resolution_scale, request.target_budget_bytes)
                    .ok_or("invalid reflection resolution or target budget")?;
            Ok((surface, height, base, target_size, bytes))
        })();
        let (surface, height, base, target_size, bytes) = match configuration {
            Ok(configuration) => configuration,
            Err(reason) => {
                if let Some(entry) = captures.0.remove(&entity) {
                    clear(&mut materials, &entry);
                    commands.entity(entry.camera).despawn();
                    images.remove(entry.image.id());
                    images.remove(entry.filtered.id());
                }
                commands.entity(entity).remove::<filter::Source>();
                *status.0.lock().unwrap() = ReflectionSnapshot {
                    rejection: Some(reason),
                    ..default()
                };
                continue;
            }
        };
        if captures
            .0
            .get(&entity)
            .is_some_and(|c| c.receiver != request.receiver || c.material != surface.0)
        {
            let old = captures.0.remove(&entity).unwrap();
            clear(&mut materials, &old);
            commands.entity(old.camera).despawn();
            images.remove(old.image.id());
            images.remove(old.filtered.id());
        }
        let pose = math::reflected_pose(world, height);
        let mut base = base.clone();
        base.update(target_size.x as f32, target_size.y as f32);
        let plane = pose.to_matrix().transpose() * Vec4::new(0.0, 1.0, 0.0, -height - 0.02);
        let projection = math::MirrorProjection { base, plane };
        let clip = projection.get_clip_from_view() * pose.to_matrix().inverse();
        if let Some(entry) = captures.0.get_mut(&entity) {
            if entry.size != target_size {
                if let Some(mut image) = images.get_mut(&entry.image) {
                    *image = capture_image(target_size);
                }
                entry.size = target_size;
                if let Some(mut image) = images.get_mut(&entry.filtered) {
                    *image = filtered_image(target_size);
                }
            }
            if let Ok((mut camera, mut transform, mut global, mut current)) =
                views.get_mut(entry.camera)
            {
                camera.is_active = true;
                camera.order = source.order.saturating_sub(1);
                *transform = pose;
                *global = GlobalTransform::from(pose);
                *current = Projection::custom(projection);
            }
            commands
                .entity(entry.camera)
                .insert(layers.cloned().unwrap_or_default());
            if let Some(preparation) = preparation.filter(|p| p.is_changed()) {
                commands
                    .entity(entry.camera)
                    .insert(preparation.secondary_view());
            }
        } else {
            let image = images.add(capture_image(target_size));
            let filtered = images.add(filtered_image(target_size));
            commands.entity(entity).insert(filter::Source {
                capture: image.clone(),
                filtered: filtered.clone(),
            });
            let camera = commands
                .spawn((
                    Camera3d::default(),
                    Camera {
                        order: source.order.saturating_sub(1),
                        clear_color: ClearColorConfig::Custom(Color::BLACK),
                        ..default()
                    },
                    RenderTarget::Image(image.clone().into()),
                    Projection::custom(projection),
                    pose,
                    GlobalTransform::from(pose),
                    Hdr,
                    Msaa::Off,
                    Tonemapping::None,
                    DebandDither::Disabled,
                    ReflectionView {
                        status: status.clone(),
                    },
                    layers.cloned().unwrap_or_default(),
                ))
                .id();
            if let Some(preparation) = preparation {
                commands.entity(camera).insert(preparation.secondary_view());
            }
            captures.0.insert(
                entity,
                Capture {
                    receiver: request.receiver,
                    material: surface.0.clone(),
                    camera,
                    image,
                    filtered,
                    size: target_size,
                    status,
                },
            );
        }
        let entry = &captures.0[&entity];
        {
            let mut status = entry.status.0.lock().unwrap();
            status.target_size = target_size;
            status.estimated_target_bytes = bytes;
            status.rejection = None;
        }
        if materials.get(&surface.0).is_some_and(|m| {
            !m.extension
                .reflection_matches(&entry.filtered, clip, target_size)
        }) && let Some(mut material) = materials.get_mut(&surface.0)
        {
            material
                .extension
                .set_reflection(entry.filtered.clone(), clip, target_size);
        }
    }
}

fn filtered_image(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba16Float, None);
    image.texture_descriptor.mip_level_count = size.max_element().ilog2() + 1;
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::STORAGE_BINDING | TextureUsages::COPY_DST;
    image.sampler = bevy::image::ImageSampler::linear();
    image.data = None;
    image.copy_on_resize = false;
    image
}

fn capture_image(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba16Float, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    image.data = None;
    image.copy_on_resize = false;
    image
}

fn exclude_surfaces(
    excluded: Query<(), With<ReflectionExcluded>>,
    mut views: Query<&mut RenderVisibleEntities, With<ReflectionView>>,
) {
    for mut view in &mut views {
        for class in view.classes.values_mut() {
            class
                .entities_cpu_culling
                .retain(|(e, _)| !excluded.contains(*e));
            class
                .entities_gpu_culling
                .retain(|_, e| !excluded.contains(*e));
            class.added_entities.retain(|(e, _)| !excluded.contains(*e));
        }
    }
}

#[cfg(test)]
#[path = "tests/planar.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/radiance_filter.rs"]
mod filter_tests;
