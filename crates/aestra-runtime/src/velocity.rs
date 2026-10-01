use aestra_core::VelocityDistribution;

/// Samples the explicit F2 velocity modes from two uniforms in [0, 1).
/// Cone spread is the full opening angle in degrees. Sphere/hemisphere/cone
/// are uniform in solid angle, disk in area, ring in azimuth. All except disk
/// return unit vectors; disk returns a planar vector of length sqrt(u).
/// LegacyCone is sampled by the existing backend-specific legacy code.
pub fn sample_velocity_distribution(
    mode: VelocityDistribution,
    axis: [f32; 3],
    spread_degrees: f32,
    u: f32,
    v: f32,
) -> [f32; 3] {
    use VelocityDistribution::*;
    let authored_axis = axis;
    // Keep the reciprocal normal even on GPUs that lower division to a
    // reciprocal multiply and flush subnormals for very large authored axes.
    let authored_largest = axis[0].abs().max(axis[1].abs()).max(axis[2].abs());
    let axis = if authored_largest > 1e20 {
        axis.map(|value| value * 1e-20)
    } else {
        axis
    };
    let largest = axis[0].abs().max(axis[1].abs()).max(axis[2].abs());
    let forward = if largest > 0.0 {
        let scaled = axis.map(|value| value / largest);
        let length2 = scaled[0] * scaled[0] + scaled[1] * scaled[1] + scaled[2] * scaled[2];
        super::scale3(scaled, 1.0 / length2.sqrt())
    } else {
        [0.0, 1.0, 0.0]
    };
    if matches!(mode, Constant | LegacyCone) {
        return forward;
    }
    let phi = v * std::f32::consts::TAU;
    let (height, radius) = match mode {
        Cone => {
            let half_angle = (spread_degrees.abs() * 0.5)
                .to_radians()
                .min(std::f32::consts::PI);
            let height = 1.0 - u * (1.0 - half_angle.cos());
            (height, (1.0 - height * height).max(0.0).sqrt())
        }
        Sphere => {
            let height = 1.0 - 2.0 * u;
            (height, (1.0 - height * height).max(0.0).sqrt())
        }
        Hemisphere => (u, (1.0 - u * u).max(0.0).sqrt()),
        Disk => (0.0, u.sqrt()),
        Ring => (0.0, 1.0),
        Constant | LegacyCone => unreachable!(),
    };
    // Choose from authored components, not a rounded normalized threshold:
    // CPU/GPU cannot disagree on a near-pole basis due to sqrt precision.
    let helper = if largest > 0.0 && authored_axis[1].abs() <= authored_axis[0].abs() {
        [0.0, 1.0, 0.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let tangent = super::normalize3(super::cross3(helper, forward));
    let bitangent = super::cross3(forward, tangent);
    super::add3(
        super::scale3(forward, height),
        super::add3(
            super::scale3(tangent, radius * phi.cos()),
            super::scale3(bitangent, radius * phi.sin()),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distributions_have_their_geometric_and_density_contracts() {
        for axis in [
            [0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 2.0, 3.0],
            [0.0; 3],
            [2e-7, 0.0, 0.0],
            [f32::MAX, 0.0, 0.0],
        ] {
            let forward =
                sample_velocity_distribution(VelocityDistribution::Constant, axis, 0.0, 0.0, 0.0);
            for mode in VelocityDistribution::ALL {
                let mut mean_height = 0.0;
                let mut mean_radius_squared = 0.0;
                for index in 0..4096 {
                    let u = (index as f32 + 0.5) / 4096.0;
                    let v = ((index * 1597) % 4096) as f32 / 4096.0;
                    let d = sample_velocity_distribution(mode, axis, 60.0, u, v);
                    let length2: f32 = d.into_iter().map(|x| x * x).sum();
                    let height: f32 = d.into_iter().zip(forward).map(|(x, y)| x * y).sum();
                    if mode == VelocityDistribution::Disk {
                        assert!((length2 - u).abs() < 1e-5);
                    } else {
                        assert!((length2 - 1.0).abs() < 1e-5, "{mode:?}: {d:?}");
                    }
                    if matches!(
                        mode,
                        VelocityDistribution::Disk | VelocityDistribution::Ring
                    ) {
                        assert!(height.abs() < 1e-5);
                    }
                    if mode == VelocityDistribution::Cone {
                        assert!(height >= 30_f32.to_radians().cos() - 1e-5);
                    }
                    if mode == VelocityDistribution::Hemisphere {
                        assert!(height >= -1e-5);
                    }
                    mean_height += height / 4096.0;
                    mean_radius_squared += length2 / 4096.0;
                }
                if mode == VelocityDistribution::Sphere {
                    assert!(mean_height.abs() < 1e-5);
                }
                if mode == VelocityDistribution::Hemisphere {
                    assert!((mean_height - 0.5).abs() < 1e-5);
                }
                if mode == VelocityDistribution::Disk {
                    assert!((mean_radius_squared - 0.5).abs() < 1e-5);
                }
            }
        }
    }
}
