use bevy::prelude::{Component, Vec3};

/// Overrides the native 2D transparency sort key for a presented effect.
///
/// Insert this on the entity with an `EffectPlayer` or `PresentedEffect`. Keys
/// use the same ordering as ordinary Bevy sprites: larger values draw later.
/// All renderers of the effect share this policy and keep their authored order.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
pub enum AestraSort2d {
    /// Sort using the effect's propagated world Z. This is also the default
    /// when the component is absent.
    #[default]
    WorldZ,
    /// Sort by `world_z - world_y * units_per_y`. Positive scales draw lower
    /// effects in front. World Z can offset distinct groups of Y-sorted effects.
    WorldY { units_per_y: f32 },
    /// Sort with a host-provided key, in the same space as sprite world Z.
    Custom(f32),
}

impl AestraSort2d {
    /// Resolves the key for the given world-space anchor. Hosts can reuse this
    /// when calculating a comparable sort key for their own scene entities.
    pub fn sort_key(self, world_position: Vec3) -> f32 {
        let key = match self {
            Self::WorldZ => world_position.z,
            Self::WorldY { units_per_y } => world_position.z - world_position.y * units_per_y,
            Self::Custom(key) => key,
        };
        if key.is_finite() {
            key
        } else if world_position.z.is_finite() {
            world_position.z
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_world_axes_host_keys_and_invalid_values() {
        let upper = Vec3::new(20.0, 3.0, 8.0);
        let lower = Vec3::new(-20.0, -3.0, 8.0);
        assert_eq!(AestraSort2d::default().sort_key(upper), 8.0);
        let y_sort = AestraSort2d::WorldY { units_per_y: 2.0 };
        assert_eq!(y_sort.sort_key(upper), 2.0);
        assert_eq!(y_sort.sort_key(lower), 14.0);
        assert_eq!(AestraSort2d::Custom(-2.5).sort_key(lower), -2.5);
        assert_eq!(AestraSort2d::Custom(f32::NAN).sort_key(lower), 8.0);
        assert_eq!(y_sort.sort_key(Vec3::new(0.0, f32::MAX, 3.0)), 3.0);
        assert_eq!(y_sort.sort_key(Vec3::splat(f32::INFINITY)), 0.0);
    }
}
