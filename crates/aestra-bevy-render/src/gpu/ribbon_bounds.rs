//! Ribbon bounds resolve after transform propagation, before Bevy's visibility checks.
use aestra_gpu::ribbon_bounds::RibbonParticleBounds;
use bevy::{
    camera::{primitives::Aabb, visibility::NoFrustumCulling},
    prelude::*,
};

#[derive(Component, Default)]
pub(super) struct RibbonBoundsSource(pub Option<RibbonParticleBounds>);

pub(super) fn sync_ribbon_bounds(
    mut commands: Commands,
    mut draws: Query<(
        Entity,
        &RibbonBoundsSource,
        &GlobalTransform,
        &mut Aabb,
        Has<NoFrustumCulling>,
    )>,
) {
    for (entity, source, transform, mut aabb, uncullable) in &mut draws {
        let bounds = source.0.and_then(|model| {
            let affine = transform.affine();
            if !affine.translation.is_finite() {
                return None;
            }
            model.half_extents(Mat3::from(affine.matrix3))
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        camera::primitives::Frustum,
        math::{
            Affine3A,
            primitives::{HalfSpace, ViewFrustum},
        },
        transform::TransformSystems,
    };

    fn model(width: f32) -> RibbonParticleBounds {
        RibbonParticleBounds {
            position_half_extents: Vec3::new(0.0, 2.0, 0.0),
            maximum_half_width: width,
        }
    }

    fn frustum() -> Frustum {
        Frustum(ViewFrustum {
            half_spaces: [
                Vec3::X,
                Vec3::NEG_X,
                Vec3::Y,
                Vec3::NEG_Y,
                Vec3::Z,
                Vec3::NEG_Z,
            ]
            .map(|normal| HalfSpace::new(normal.extend(10.0))),
        })
    }

    #[test]
    fn wide_ribbon_crosses_viewport_edge_even_when_compressed_pivot_is_outside() {
        for scale in [Vec3::new(0.01, 4.0, 0.5), Vec3::new(-0.01, 4.0, -0.5)] {
            let transform = Affine3A::from_scale_rotation_translation(
                scale,
                Quat::IDENTITY,
                Vec3::new(13.0, 0.0, 0.0),
            );
            let source = model(1.0);
            let bounds = Aabb {
                center: Vec3::ZERO.into(),
                half_extents: source
                    .half_extents(Mat3::from(transform.matrix3))
                    .unwrap()
                    .into(),
            };
            assert!(frustum().intersects_obb(&bounds, &transform, true, true));
            let naive = Aabb {
                center: Vec3::ZERO.into(),
                half_extents: (source.position_half_extents + Vec3::ONE).into(),
            };
            assert!(!frustum().intersects_obb(&naive, &transform, true, true));
            let outside = Affine3A::from_translation(Vec3::new(20.0, 0.0, 0.0)) * transform;
            assert!(
                !frustum().intersects_obb(&bounds, &outside, true, true),
                "fully offscreen strips can be culled"
            );
        }
    }

    #[test]
    fn edits_and_parent_transforms_refresh_bounds_in_the_same_frame_and_recover_from_invalid_data()
    {
        let mut app = App::new();
        app.add_plugins(bevy::transform::TransformPlugin)
            .add_systems(
                PostUpdate,
                sync_ribbon_bounds.after(TransformSystems::Propagate),
            );
        let parent = app.world_mut().spawn(Transform::IDENTITY).id();
        let draw = app
            .world_mut()
            .spawn((
                ChildOf(parent),
                Transform::IDENTITY,
                Aabb::default(),
                RibbonBoundsSource(Some(model(1.0))),
                NoFrustumCulling,
            ))
            .id();
        app.update();
        assert!(!app.world().entity(draw).contains::<NoFrustumCulling>());
        let initial = app.world().get::<Aabb>(draw).unwrap().half_extents;
        app.world_mut()
            .get_mut::<RibbonBoundsSource>(draw)
            .unwrap()
            .0 = Some(model(3.0));
        app.update();
        assert!(
            app.world()
                .get::<Aabb>(draw)
                .unwrap()
                .half_extents
                .cmpgt(initial)
                .all()
        );
        app.world_mut().get_mut::<Transform>(parent).unwrap().scale = Vec3::new(-0.01, 5.0, 1.0);
        app.update();
        let actual = app.world().get::<Aabb>(draw).unwrap().half_extents;
        let expected = model(3.0)
            .half_extents(Mat3::from_diagonal(Vec3::new(-0.01, 5.0, 1.0)))
            .unwrap();
        assert!((Vec3::from(actual) - expected).abs().max_element() < 0.001);
        // Collapse one parent axis: no finite local box can enclose world-facing width.
        app.world_mut()
            .get_mut::<Transform>(parent)
            .unwrap()
            .scale
            .x = 0.0;
        app.update();
        assert!(app.world().entity(draw).contains::<NoFrustumCulling>());
        *app.world_mut().get_mut::<Transform>(parent).unwrap() = Transform::IDENTITY;
        app.update();
        assert!(!app.world().entity(draw).contains::<NoFrustumCulling>());
        app.world_mut()
            .get_mut::<RibbonBoundsSource>(draw)
            .unwrap()
            .0 = None;
        app.update();
        assert!(app.world().entity(draw).contains::<NoFrustumCulling>());
        app.world_mut()
            .get_mut::<RibbonBoundsSource>(draw)
            .unwrap()
            .0 = Some(model(f32::NAN));
        app.update();
        assert!(app.world().entity(draw).contains::<NoFrustumCulling>());
        app.world_mut()
            .get_mut::<RibbonBoundsSource>(draw)
            .unwrap()
            .0 = Some(model(1.0));
        app.update();
        assert!(!app.world().entity(draw).contains::<NoFrustumCulling>());
        assert_eq!(app.world().get::<Aabb>(draw).unwrap().half_extents, initial);
    }
}
