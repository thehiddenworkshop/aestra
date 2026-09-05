//! Conservative mesh presentation bounds, independent of any engine or asset loader.

use glam::{Mat3, Vec3};

/// Bounds in effect space. Geometry rotates about the mesh origin, not its AABB center.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshParticleBounds {
    pub position_half_extents: Vec3,
    pub linear_from_local: Mat3,
    pub maximum_size: f32,
}

impl MeshParticleBounds {
    /// Covers every particle angle, the entire mesh (including an off-center pivot), and
    /// the emitter's rotation/nonuniform scale. The effect transform is applied by the host.
    /// Returns None rather than providing unsafe culling bounds for invalid/overflowing data.
    pub fn half_extents(&self, minimum: Vec3, maximum: Vec3) -> Option<Vec3> {
        if !minimum.is_finite()
            || !maximum.is_finite()
            || minimum.cmpgt(maximum).any()
            || !self.position_half_extents.is_finite()
            || self.position_half_extents.min_element() < 0.0
            || !self.linear_from_local.is_finite()
            || !self.maximum_size.is_finite()
            || self.maximum_size < 0.0
        {
            return None;
        }
        let extent = minimum.abs().max(maximum.abs());
        let radius = extent.x.hypot(extent.y);
        let swept = Vec3::new(radius, radius, extent.z) * self.maximum_size;
        let linear = self.linear_from_local;
        let geometry = linear.x_axis.abs() * swept.x
            + linear.y_axis.abs() * swept.y
            + linear.z_axis.abs() * swept.z;
        let bounds = self.position_half_extents + geometry;
        // Small outward margin for floating-point matrix and shader arithmetic.
        let bounds = bounds * 1.0001 + Vec3::splat(0.001);
        bounds.is_finite().then_some(bounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    #[test]
    fn off_center_geometry_is_enclosed_at_every_angle_and_signed_scale() {
        let min = Vec3::new(10.0, -4.0, -8.0);
        let max = Vec3::new(40.0, 7.0, 3.0);
        for scale in [
            Vec3::ONE,
            Vec3::new(0.1, 8.0, 2.0),
            Vec3::new(-3.0, 0.25, -5.0),
        ] {
            let model = MeshParticleBounds {
                position_half_extents: Vec3::new(50.0, 20.0, 10.0),
                linear_from_local: Mat3::from_quat(Quat::from_euler(
                    glam::EulerRot::XYZ,
                    0.7,
                    1.2,
                    -0.4,
                )) * Mat3::from_diagonal(scale),
                maximum_size: 12.0,
            };
            let bounds = model.half_extents(min, max).unwrap();
            for angle in 0..360 {
                let rotation = Quat::from_rotation_z((angle as f32).to_radians());
                for x in [min.x, max.x] {
                    for y in [min.y, max.y] {
                        for z in [min.z, max.z] {
                            let vertex = model.linear_from_local
                                * (rotation * Vec3::new(x, y, z))
                                * model.maximum_size;
                            assert!(
                                (vertex.abs() + model.position_half_extents)
                                    .cmple(bounds)
                                    .all()
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_or_overflowing_geometry_cannot_enable_culling() {
        let model = MeshParticleBounds {
            position_half_extents: Vec3::ZERO,
            linear_from_local: Mat3::IDENTITY,
            maximum_size: 10.0,
        };
        assert!(
            model
                .half_extents(Vec3::splat(f32::NAN), Vec3::ONE)
                .is_none()
        );
        assert!(model.half_extents(Vec3::ONE, Vec3::ZERO).is_none());
        assert!(
            model
                .half_extents(Vec3::ZERO, Vec3::splat(f32::MAX))
                .is_none()
        );
    }
}
