//! Camera-independent bounds for the world-space width used by ribbon geometry.
use glam::{Mat3, Vec3};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RibbonParticleBounds {
    /// Particle centers in effect space, including emitter motion and transform.
    pub position_half_extents: Vec3,
    /// Half width including Appearance size, renderer width and emitter scale.
    pub maximum_half_width: f32,
}

impl RibbonParticleBounds {
    /// Bevy applies the effect transform to this box. Pull a camera-independent
    /// world-space width sphere back through that transform so even a compressed
    /// local axis encloses the billboard. Segment interiors are convex combinations
    /// of endpoints, so enclosing all endpoint offsets also encloses every triangle.
    pub fn half_extents(&self, world_from_effect: Mat3) -> Option<Vec3> {
        if !self.position_half_extents.is_finite()
            || self.position_half_extents.min_element() < 0.0
            || !self.maximum_half_width.is_finite()
            || self.maximum_half_width < 0.0
            || !world_from_effect.is_finite()
        {
            return None;
        }
        // A singular transformed AABB cannot enclose width along its collapsed axis.
        let determinant = world_from_effect.determinant();
        if !determinant.is_finite() || determinant == 0.0 {
            return None;
        }
        let inverse = world_from_effect.inverse();
        if !inverse.is_finite() {
            return None;
        }
        // Match aestra_ribbon_vertex's maximum column length, including sheared parents.
        let scale = world_from_effect
            .x_axis
            .length()
            .max(world_from_effect.y_axis.length())
            .max(world_from_effect.z_axis.length());
        let radius = self.maximum_half_width * scale;
        let rows = inverse.transpose();
        let padding = Vec3::new(
            rows.x_axis.length(),
            rows.y_axis.length(),
            rows.z_axis.length(),
        ) * radius;
        let bounds = (self.position_half_extents + padding) * 1.0001 + Vec3::splat(0.001);
        bounds.is_finite().then_some(bounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{EulerRot, Quat};

    #[test]
    fn all_camera_facing_offsets_fit_under_rotation_mirrors_nonuniform_scale_and_shear() {
        let model = RibbonParticleBounds {
            position_half_extents: Vec3::new(7.0, 13.0, 3.0),
            maximum_half_width: 9.0,
        };
        let rotation = Mat3::from_quat(Quat::from_euler(EulerRot::XYZ, 0.4, -0.8, 1.2));
        for linear in [
            Mat3::IDENTITY,
            rotation * Mat3::from_diagonal(Vec3::new(0.01, 12.0, 0.4)),
            rotation * Mat3::from_diagonal(Vec3::new(-4.0, 0.2, -2.0)),
            Mat3::from_diagonal(Vec3::new(0.1, 5.0, 3.0)) * rotation,
        ] {
            let bounds = model.half_extents(linear).unwrap();
            let radius = model.maximum_half_width
                * linear
                    .x_axis
                    .length()
                    .max(linear.y_axis.length())
                    .max(linear.z_axis.length());
            for longitude in 0..36 {
                for latitude in 0..19 {
                    let a = longitude as f32 * std::f32::consts::TAU / 36.0;
                    let b = latitude as f32 * std::f32::consts::PI / 18.0;
                    let side = Vec3::new(a.cos() * b.sin(), a.sin() * b.sin(), b.cos());
                    let offset = linear.inverse() * (side * radius);
                    assert!(
                        (model.position_half_extents + offset.abs())
                            .cmple(bounds)
                            .all()
                    );
                }
            }
        }
    }

    #[test]
    fn unsafe_bounds_disable_culling_and_width_edits_expand_all_axes() {
        let mut model = RibbonParticleBounds {
            position_half_extents: Vec3::ONE,
            maximum_half_width: 1.0,
        };
        let before = model.half_extents(Mat3::IDENTITY).unwrap();
        model.maximum_half_width = 4.0;
        assert!(
            model
                .half_extents(Mat3::IDENTITY)
                .unwrap()
                .cmpgt(before)
                .all()
        );
        for linear in [
            Mat3::ZERO,
            Mat3::from_diagonal(Vec3::new(1.0, 0.0, 2.0)),
            Mat3::from_diagonal(Vec3::splat(f32::NAN)),
        ] {
            assert!(model.half_extents(linear).is_none());
        }
        for width in [f32::NAN, f32::INFINITY, -1.0, f32::MAX] {
            model.maximum_half_width = width;
            assert!(model.half_extents(Mat3::IDENTITY).is_none());
        }
    }
}
