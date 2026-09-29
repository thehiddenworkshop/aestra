//! The host's physics scene for stateful particles (host bindings HB10, the `EnginePhysicsQuery`
//! collision provider).
//!
//! An effect whose emitters carry a `Physics` collider collides with the [`AestraPhysicsColliders`]
//! on its entity: the physics engine's colliders around it, as analytic proxies in world space,
//! refreshed every frame. Aestra depends on no physics crate. A physics adapter fills the component —
//! `aestra-bevy-rapier` (`AestraRapierPlugin`) and `aestra-bevy-avian` (`AestraAvianPlugin`) query
//! their engine around every effect carrying an [`AestraPhysicsQuery`] — or a game fills it from its
//! own physics, in [`crate::AestraRenderSet::Prepare`]'s schedule before rendering (any `Update`
//! system does).

use aestra_runtime::{MAX_PHYSICS_PROXIES, PhysicsProxy, PhysicsScene};
use bevy::prelude::*;

/// Asks a physics adapter for the host's colliders around this effect (host bindings HB10): those
/// overlapping a sphere of `radius` around the effect's origin, the `max` nearest kept.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct AestraPhysicsQuery {
    pub radius: f32,
    /// At most [`MAX_PHYSICS_PROXIES`].
    pub max: usize,
}

impl AestraPhysicsQuery {
    /// Colliders within `radius` of the effect, as many as fit.
    pub fn within(radius: f32) -> Self {
        Self {
            radius,
            max: MAX_PHYSICS_PROXIES,
        }
    }
}

impl Default for AestraPhysicsQuery {
    fn default() -> Self {
        Self::within(50.0)
    }
}

/// The host's physics colliders around an effect, in world space (host bindings HB10): what its
/// `Physics` colliders collide with this frame. Filled by a physics adapter or the game.
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct AestraPhysicsColliders(pub PhysicsScene);

impl AestraPhysicsColliders {
    /// The `max` proxies nearest `center` (by signed distance), in a stable order: an adapter's
    /// last step before storing what its query found.
    pub fn nearest(center: Vec3, proxies: Vec<PhysicsProxy>, max: usize) -> Self {
        let max = max.min(MAX_PHYSICS_PROXIES);
        let mut ranked: Vec<(f32, usize, PhysicsProxy)> = proxies
            .into_iter()
            .enumerate()
            .map(|(index, proxy)| (proxy.distance_normal(center.to_array()).0, index, proxy))
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        Self(PhysicsScene {
            proxies: ranked
                .into_iter()
                .take(max)
                .map(|(_, _, proxy)| proxy)
                .collect(),
        })
    }
}

/// A collider's pose in the world, placing the primitives an adapter reads off its shape (in the
/// collider's local frame) as [`PhysicsProxy`]s. Scale is the shape's own: adapters read already
/// scaled shapes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsPose {
    pub translation: Vec3,
    pub rotation: Quat,
}

impl PhysicsPose {
    /// The pose of an entity from its global transform (its scale left to the shape).
    pub fn of(transform: &GlobalTransform) -> Self {
        let (_, rotation, translation) = transform.to_scale_rotation_translation();
        Self {
            translation,
            rotation: rotation.normalize(),
        }
    }

    /// A sub-shape's pose, at `translation` and `rotation` in this frame (a compound's part).
    pub fn then(self, translation: Vec3, rotation: Quat) -> Self {
        Self {
            translation: self.translation + self.rotation * translation,
            rotation: (self.rotation * rotation).normalize(),
        }
    }

    fn point(self, local: Vec3) -> [f32; 3] {
        (self.translation + self.rotation * local).to_array()
    }

    pub fn sphere(self, center: Vec3, radius: f32) -> PhysicsProxy {
        PhysicsProxy::Sphere {
            center: self.point(center),
            radius,
        }
    }

    pub fn capsule(self, a: Vec3, b: Vec3, radius: f32) -> PhysicsProxy {
        PhysicsProxy::Capsule {
            a: self.point(a),
            b: self.point(b),
            radius,
        }
    }

    /// A box of `half_extents` about `center`, aligned with this frame.
    pub fn cuboid(self, center: Vec3, half_extents: Vec3) -> PhysicsProxy {
        PhysicsProxy::Box {
            center: self.point(center),
            rotation: self.rotation.to_array(),
            half_extents: half_extents.abs().to_array(),
        }
    }

    /// The half-space behind the plane through this frame's origin with outward `normal`.
    pub fn half_space(self, normal: Vec3) -> PhysicsProxy {
        let normal = (self.rotation * normal).normalize_or(Vec3::Y);
        PhysicsProxy::HalfSpace {
            normal: normal.to_array(),
            distance: normal.dot(self.translation),
        }
    }

    /// The box spanning `min`–`max` in this frame: how an adapter approximates a shape it has no
    /// primitive for (a mesh, a height field, a cone) — by its local bounds.
    pub fn bounds(self, min: Vec3, max: Vec3) -> PhysicsProxy {
        self.cuboid((min + max) * 0.5, (max - min) * 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poses_place_primitives_in_the_world() {
        let pose = PhysicsPose {
            translation: Vec3::new(0.0, 5.0, 0.0),
            rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
        };
        // The local +x axis points along world +y.
        let PhysicsProxy::Sphere { center, radius } = pose.sphere(Vec3::X, 1.0) else {
            unreachable!()
        };
        assert!(Vec3::from_array(center).distance(Vec3::new(0.0, 6.0, 0.0)) < 1e-5);
        assert_eq!(radius, 1.0);
        let PhysicsProxy::HalfSpace { normal, distance } = pose.half_space(Vec3::X) else {
            unreachable!()
        };
        assert!(Vec3::from_array(normal).distance(Vec3::Y) < 1e-5);
        assert!((distance - 5.0).abs() < 1e-5);
        let part = pose.then(Vec3::new(2.0, 0.0, 0.0), Quat::IDENTITY);
        assert!(part.translation.distance(Vec3::new(0.0, 7.0, 0.0)) < 1e-5);
    }

    #[test]
    fn the_nearest_proxies_are_kept_in_a_stable_order() {
        let sphere = |x: f32| PhysicsProxy::Sphere {
            center: [x, 0.0, 0.0],
            radius: 1.0,
        };
        let kept = AestraPhysicsColliders::nearest(
            Vec3::ZERO,
            vec![sphere(10.0), sphere(-3.0), sphere(3.0), sphere(20.0)],
            3,
        );
        assert_eq!(kept.0.proxies, [sphere(-3.0), sphere(3.0), sphere(10.0)]);
    }
}
