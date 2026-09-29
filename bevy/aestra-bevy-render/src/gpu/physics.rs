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

#[cfg(test)]
mod tests {
    use super::*;

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
