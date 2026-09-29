//! Rapier physics for Aestra effects (host bindings HB10, the `EnginePhysicsQuery` collision
//! provider).
//!
//! Add [`AestraRapierPlugin`] next to `bevy_rapier3d`'s `RapierPhysicsPlugin` and Aestra's
//! `AestraPlugin`, and give an effect entity an [`AestraPhysicsQuery`]: every frame the plugin asks
//! Rapier's query pipeline for the colliders within the query's radius of the effect and stores them,
//! as analytic proxies, in the effect's [`AestraPhysicsColliders`]. Particles of emitters carrying a
//! `Physics` collider then bounce off the game's rigid bodies — moving ones included — on the GPU.
//!
//! Shapes map to proxies as closely as the proxies allow: balls, cuboids, capsules, segments and
//! half-spaces exactly; round cuboids as their outer box; compounds part by part; everything else
//! (meshes, height fields, cylinders, cones, convex hulls) as its local bounding box.
//!
//! ```no_run
//! use aestra_bevy::{AestraPhysicsQuery, AestraPlugin};
//! use aestra_bevy_rapier::AestraRapierPlugin;
//! use bevy::prelude::*;
//! use bevy_rapier3d::prelude::{NoUserData, RapierPhysicsPlugin};
//!
//! App::new()
//!     .add_plugins((
//!         DefaultPlugins,
//!         RapierPhysicsPlugin::<NoUserData>::default(),
//!         AestraPlugin,
//!         AestraRapierPlugin,
//!     ))
//!     .run();
//! // Then spawn effects with `(EffectPlayer::new(..), AestraPhysicsQuery::within(30.0))`.
//! ```

use aestra_bevy::{
    AestraPhysicsColliders, AestraPhysicsQuery, AestraSet, PhysicsPose, PhysicsProxy,
};
use bevy::prelude::*;
use bevy_rapier3d::parry::shape::{Shape, TypedShape};
use bevy_rapier3d::prelude::{Collider, QueryFilter, ReadRapierContext};

/// Fills [`AestraPhysicsColliders`] from Rapier for every effect carrying an [`AestraPhysicsQuery`],
/// each frame, while host input is resolved ([`AestraSet::ResolveHostInputs`]).
pub struct AestraRapierPlugin;

impl Plugin for AestraRapierPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            query_rapier_colliders.in_set(AestraSet::ResolveHostInputs),
        );
    }
}

/// Queries Rapier around every effect with an [`AestraPhysicsQuery`].
pub fn query_rapier_colliders(
    mut commands: Commands,
    effects: Query<(Entity, &GlobalTransform, &AestraPhysicsQuery)>,
    context: ReadRapierContext,
    colliders: Query<(&Collider, &GlobalTransform)>,
) {
    let Ok(context) = context.single() else {
        return;
    };
    for (entity, transform, query) in &effects {
        let center = transform.translation();
        let probe = Collider::ball(query.radius);
        let mut proxies = Vec::new();
        context.intersect_shape(
            center,
            Quat::IDENTITY,
            &*probe.raw,
            QueryFilter::default(),
            |hit| {
                if let Ok((collider, pose)) = colliders.get(hit) {
                    proxies_of(&*collider.raw, PhysicsPose::of(pose), &mut proxies);
                }
                true
            },
        );
        commands
            .entity(entity)
            .insert(AestraPhysicsColliders::nearest(center, proxies, query.max));
    }
}

fn vector(v: bevy_rapier3d::parry::math::Vector) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// The proxies approximating a Rapier (parry) shape placed at `pose`.
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
