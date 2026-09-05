//! Resolve geometry in the main world before visibility checks, never using sprite bounds
//! as a substitute for a mesh whose asset is still loading.

use aestra_gpu::mesh_bounds::MeshParticleBounds;
use bevy::{
    camera::{primitives::Aabb, visibility::NoFrustumCulling},
    mesh::VertexAttributeValues,
    prelude::*,
};
use std::collections::HashMap;

#[derive(Component)]
pub(super) struct MeshBoundsSource {
    mesh: Handle<Mesh>,
    pub motion: Option<MeshParticleBounds>,
}

impl MeshBoundsSource {
    pub fn new(mesh: Handle<Mesh>) -> Self {
        Self { mesh, motion: None }
    }
}

type GeometryCache = HashMap<AssetId<Mesh>, Option<(Vec3, Vec3)>>;

pub(super) fn sync_mesh_bounds(
    mut commands: Commands,
    meshes: Res<Assets<Mesh>>,
    mut cache: Local<GeometryCache>,
    mut draws: Query<(Entity, &MeshBoundsSource, &mut Aabb, Has<NoFrustumCulling>)>,
) {
    // Assets change on load, reload, mutation and removal. Cache per shared mesh, not per
    // emitter, and avoid scanning all vertex buffers on every playback frame.
    if meshes.is_changed() {
        cache.clear();
    }
    for (entity, source, mut aabb, uncullable) in &mut draws {
        let geometry = cache
            .entry(source.mesh.id())
            .or_insert_with(|| meshes.get(&source.mesh).and_then(geometry_bounds));
        let bounds = source
            .motion
            .and_then(|motion| geometry.and_then(|(min, max)| motion.half_extents(min, max)));
        if let Some(half_extents) = bounds {
            let bounds = Aabb {
                center: Vec3::ZERO.into(),
                half_extents: half_extents.into(),
            };
            if aabb.center != bounds.center || aabb.half_extents != bounds.half_extents {
                *aabb = bounds;
            }
            if uncullable {
                commands.entity(entity).remove::<NoFrustumCulling>();
            }
        } else if !uncullable {
            commands.entity(entity).insert(NoFrustumCulling);
        }
    }
}

fn geometry_bounds(mesh: &Mesh) -> Option<(Vec3, Vec3)> {
    let VertexAttributeValues::Float32x3(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)?
    else {
        return None;
    };
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    for position in positions {
        let position = Vec3::from_array(*position);
        if !position.is_finite() {
            return None;
        }
        minimum = minimum.min(position);
        maximum = maximum.max(position);
    }
    (!positions.is_empty()).then_some((minimum, maximum))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology};

    fn mesh(extent: f32) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3], [extent; 3]])
    }

    fn motion(size: f32) -> MeshParticleBounds {
        MeshParticleBounds {
            position_half_extents: Vec3::splat(5.0),
            linear_from_local: Mat3::IDENTITY,
            maximum_size: size,
        }
    }

    #[test]
    fn geometry_lifecycle_and_particle_edits_refresh_before_culling() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .add_systems(Update, sync_mesh_bounds);
        let handle = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .reserve_handle();
        let entity = app
            .world_mut()
            .spawn((
                MeshBoundsSource {
                    mesh: handle.clone(),
                    motion: Some(motion(2.0)),
                },
                Aabb::default(),
                NoFrustumCulling,
            ))
            .id();
        app.update();
        assert!(app.world().entity(entity).contains::<NoFrustumCulling>());
        app.world_mut()
            .resource_mut::<Assets<Mesh>>()
            .insert(handle.id(), mesh(10.0))
            .unwrap();
        app.update();
        assert!(!app.world().entity(entity).contains::<NoFrustumCulling>());
        let before = app.world().get::<Aabb>(entity).unwrap().half_extents;
        app.world_mut()
            .resource_mut::<Assets<Mesh>>()
            .get_mut(&handle)
            .unwrap()
            .insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3], [100.0; 3]]);
        app.update();
        let larger = app.world().get::<Aabb>(entity).unwrap().half_extents;
        assert!(larger.cmpgt(before).all());
        app.world_mut()
            .get_mut::<MeshBoundsSource>(entity)
            .unwrap()
            .motion = Some(motion(4.0));
        app.update();
        assert!(
            app.world()
                .get::<Aabb>(entity)
                .unwrap()
                .half_extents
                .cmpgt(larger)
                .all()
        );
        app.world_mut()
            .resource_mut::<Assets<Mesh>>()
            .insert(handle.id(), mesh(f32::NAN))
            .unwrap();
        app.update();
        assert!(app.world().entity(entity).contains::<NoFrustumCulling>());
        app.world_mut()
            .resource_mut::<Assets<Mesh>>()
            .insert(handle.id(), mesh(1.0))
            .unwrap();
        app.update();
        assert!(!app.world().entity(entity).contains::<NoFrustumCulling>());
        app.world_mut()
            .resource_mut::<Assets<Mesh>>()
            .remove(handle.id());
        app.update();
        assert!(app.world().entity(entity).contains::<NoFrustumCulling>());
    }

    #[test]
    fn empty_or_missing_positions_are_not_cullable() {
        assert!(
            geometry_bounds(&Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default()
            ))
            .is_none()
        );
        assert!(
            geometry_bounds(
                &mesh(1.0)
                    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, Vec::<[f32; 3]>::new())
            )
            .is_none()
        );
    }

    #[test]
    fn off_center_mesh_remains_visible_at_frustum_edge_with_scaled_effect() {
        use bevy::{
            camera::primitives::Frustum,
            math::{
                Affine3A,
                primitives::{HalfSpace, ViewFrustum},
            },
        };
        let frustum = Frustum(ViewFrustum {
            half_spaces: [
                Vec3::X,
                Vec3::NEG_X,
                Vec3::Y,
                Vec3::NEG_Y,
                Vec3::Z,
                Vec3::NEG_Z,
            ]
            .map(|normal| HalfSpace::new(normal.extend(100.0))),
        });
        let model = motion(10.0);
        let aabb = Aabb {
            center: Vec3::ZERO.into(),
            half_extents: model
                .half_extents(Vec3::new(40.0, -1.0, -1.0), Vec3::new(60.0, 1.0, 1.0))
                .unwrap()
                .into(),
        };
        let transform = Affine3A::from_scale_rotation_translation(
            Vec3::new(2.0, 0.5, 3.0),
            Quat::IDENTITY,
            Vec3::new(-1000.0, 0.0, 0.0),
        );
        // The pivot is far outside, but actual geometry crosses the view.
        assert_eq!(
            transform.transform_point3(Vec3::new(50.0, 0.0, 0.0) * model.maximum_size),
            Vec3::ZERO
        );
        assert!(frustum.intersects_obb(&aabb, &transform, true, true));
        let billboard = Aabb {
            center: Vec3::ZERO.into(),
            half_extents: Vec3::splat(10.0).into(),
        };
        assert!(!frustum.intersects_obb(&billboard, &transform, true, true));
        let outside = Affine3A::from_translation(Vec3::new(-4000.0, 0.0, 0.0)) * transform;
        assert!(!frustum.intersects_obb(&aabb, &outside, true, true));
    }
}
