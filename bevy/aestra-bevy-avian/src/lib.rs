//! Avian physics for Aestra effects (host bindings HB10, the `EnginePhysicsQuery` collision
//! provider).
//!
//! Temporarily suspended during Aestra's Bevy upgrade, outside workspace CI.
//! This adapter targets Bevy 0.19 only; see `bevy/PHYSICS_ADAPTERS.md` for restoration.
//!
//! Add [`AestraAvianPlugin`] next to Avian's `PhysicsPlugins` and Aestra's `AestraPlugin`, and give an
//! effect entity an [`AestraPhysicsQuery`]: every frame the plugin asks Avian's spatial query for the
//! colliders within the query's radius of the effect and stores them, as analytic proxies, in the
//! effect's [`AestraPhysicsColliders`]. Particles of emitters carrying a `Physics` collider then
//! bounce off the game's rigid bodies — moving ones included — on the GPU.
//!
//! Shapes map to proxies as closely as the proxies allow: balls, cuboids, capsules, segments and
//! half-spaces exactly; round cuboids as their outer box; compounds part by part; everything else
//! (meshes, height fields, cylinders, cones, convex hulls) as its local bounding box.
//!
//! ```no_run
//! use aestra_bevy::{AestraPhysicsQuery, AestraPlugin};
//! use aestra_bevy_avian::AestraAvianPlugin;
//! use avian3d::prelude::PhysicsPlugins;
//! use bevy::prelude::*;
//!
//! App::new()
//!     .add_plugins((DefaultPlugins, PhysicsPlugins::default(), AestraPlugin, AestraAvianPlugin))
//!     .run();
//! // Then spawn effects with `(EffectPlayer::new(..), AestraPhysicsQuery::within(30.0))`.
//! ```

use aestra_bevy::{
    AestraPhysicsColliders, AestraPhysicsQuery, AestraSet, PhysicsPose, PhysicsProxy,
};
use avian3d::parry::shape::{Shape, TypedShape};
use avian3d::prelude::{Collider, SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;

/// Fills [`AestraPhysicsColliders`] from Avian for every effect carrying an [`AestraPhysicsQuery`],
/// each frame, while host input is resolved ([`AestraSet::ResolveHostInputs`]).
pub struct AestraAvianPlugin;

impl Plugin for AestraAvianPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            query_avian_colliders.in_set(AestraSet::ResolveHostInputs),
        );
    }
}

/// Queries Avian around every effect with an [`AestraPhysicsQuery`].
pub fn query_avian_colliders(
    mut commands: Commands,
    effects: Query<(Entity, &GlobalTransform, &AestraPhysicsQuery)>,
    spatial: SpatialQuery,
    colliders: Query<(&Collider, &GlobalTransform)>,
) {
    for (entity, transform, query) in &effects {
        let center = transform.translation();
        let hits = spatial.shape_intersections(
            &Collider::sphere(query.radius),
            center,
            Quat::IDENTITY,
            &SpatialQueryFilter::default(),
        );
        let mut proxies = Vec::new();
        for hit in hits {
            if let Ok((collider, pose)) = colliders.get(hit) {
                proxies_of(
                    &**collider.shape_scaled(),
                    PhysicsPose::of(pose),
                    &mut proxies,
                );
            }
        }
        commands
            .entity(entity)
            .insert(AestraPhysicsColliders::nearest(center, proxies, query.max));
    }
}

fn vector(v: avian3d::parry::math::Vector) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// The proxies approximating an Avian (parry) shape placed at `pose`.
pub fn proxies_of(shape: &dyn Shape, pose: PhysicsPose, out: &mut Vec<PhysicsProxy>) {
    match shape.as_typed_shape() {
        TypedShape::Ball(ball) => out.push(pose.sphere(Vec3::ZERO, ball.radius)),
        TypedShape::Cuboid(cuboid) => {
            out.push(pose.cuboid(Vec3::ZERO, vector(cuboid.half_extents)));
        }
        TypedShape::Capsule(capsule) => out.push(pose.capsule(
            vector(capsule.segment.a),
            vector(capsule.segment.b),
            capsule.radius,
        )),
        TypedShape::Segment(segment) => {
            out.push(pose.capsule(vector(segment.a), vector(segment.b), 0.0));
        }
        TypedShape::HalfSpace(half_space) => out.push(pose.half_space(vector(half_space.normal))),
        TypedShape::RoundCuboid(round) => out.push(pose.cuboid(
            Vec3::ZERO,
            vector(round.inner_shape.half_extents) + Vec3::splat(round.border_radius),
        )),
        TypedShape::Compound(compound) => {
            for (part_pose, part) in compound.shapes() {
                let rotation = part_pose.rotation;
                proxies_of(
                    &**part,
                    pose.then(
                        vector(part_pose.translation),
                        Quat::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w),
                    ),
                    out,
                );
            }
        }
        _ => {
            let bounds = shape.compute_local_aabb();
            out.push(pose.bounds(vector(bounds.mins), vector(bounds.maxs)));
        }
    }
}
