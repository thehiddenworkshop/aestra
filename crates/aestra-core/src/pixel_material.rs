//! Reference pixel operations shared by material authoring previews.

/// Returns the center of the containing grid cell in the supplied coordinate space.
pub fn snap(position: [f32; 2], cell_size: f32) -> [f32; 2] {
    let size = if cell_size.is_finite() && cell_size > 0.0 {
        cell_size
    } else {
        1.0
    };
    position.map(|axis| {
        let center = ((axis / size).floor() + 0.5) * size;
        if center.is_finite() { center } else { axis }
    })
}

/// Quantizes linear RGB across the HDR range and preserves alpha.
pub fn posterize(linear_rgba: [f32; 4], levels: f32) -> [f32; 4] {
    let intervals = if levels.is_finite() {
        (levels + 0.5).floor().clamp(2.0, 16_777_216.0) - 1.0
    } else {
        1.0
    };
    let [r, g, b, a] = linear_rgba;
    let [r, g, b] = [r, g, b].map(|channel| {
        let rounded = (channel * intervals + 0.5).floor() / intervals;
        if rounded.is_finite() {
            rounded
        } else {
            channel
        }
    });
    [r, g, b, a]
}

/// Snaps normalized UVs to the centers of a grid with the given columns and rows.
/// The grid continues outside the unit square so the sampler can apply its address mode.
pub fn pixelate_uv(uv: [f32; 2], cells: [f32; 2]) -> [f32; 2] {
    std::array::from_fn(|axis| {
        let count = if cells[axis].is_finite() {
            (cells[axis] + 0.5).floor().clamp(1.0, 8_388_608.0)
        } else {
            1.0
        };
        let center = ((uv[axis] * count).floor() + 0.5) / count;
        if center.is_finite() { center } else { uv[axis] }
    })
}

/// Rounds a scalar to the nearest multiple of a positive step.
/// Invalid steps and unrepresentable results leave the value unchanged.
pub fn quantize(value: f32, step: f32) -> f32 {
    if !step.is_finite() || step <= 0.0 {
        return value;
    }
    let rounded = (value / step + 0.5).floor() * step;
    if rounded.is_finite() { rounded } else { value }
}

/// Returns one at or above the cutoff and zero otherwise.
pub fn threshold(value: f32, cutoff: f32) -> f32 {
    f32::from(value >= cutoff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_uses_cell_centers_on_both_sides_of_zero() {
        assert_eq!(snap([-0.01, 0.0], 2.0), [-1.0, 1.0]);
        assert_eq!(snap([-2.0, 1.99], 2.0), [-1.0, 1.0]);
        assert_eq!(snap([-2.01, 2.0], 2.0), [-3.0, 3.0]);
        assert_eq!(snap([-0.01, 0.0], f32::NAN), [-0.5, 0.5]);
        assert_eq!(snap([f32::MAX, -f32::MAX], 0.5), [f32::MAX, -f32::MAX]);
    }

    #[test]
    fn posterize_preserves_alpha_and_quantizes_hdr_in_linear_light() {
        assert_eq!(
            posterize([0.22, 0.49, 1.76, 0.13], 5.0),
            [0.25, 0.5, 1.75, 0.13]
        );
        assert_eq!(
            posterize([0.49, 0.50, 1.51, 0.13], 0.0),
            [0.0, 1.0, 2.0, 0.13]
        );
        assert_eq!(
            posterize([0.49, 0.50, 1.51, 0.13], f32::INFINITY),
            [0.0, 1.0, 2.0, 0.13]
        );
        assert_eq!(
            posterize([f32::MAX, 0.0, 1.0, 0.4], 5.0),
            [f32::MAX, 0.0, 1.0, 0.4]
        );
    }

    #[test]
    fn pixelate_uv_uses_rectangular_cells_without_clamping_tiled_uvs() {
        assert_eq!(pixelate_uv([0.0, 0.99], [4.0, 2.0]), [0.125, 0.75]);
        assert_eq!(pixelate_uv([-0.01, 1.0], [4.0, 2.0]), [-0.125, 1.25]);
        assert_eq!(pixelate_uv([0.2, 0.7], [f32::NAN, 0.0]), [0.5, 0.5]);
        assert_eq!(pixelate_uv([f32::MAX, 0.2], [4.0, 2.49]), [f32::MAX, 0.25]);
    }

    #[test]
    fn scalar_quantize_and_threshold_handle_signed_values_and_inclusive_cutoff() {
        assert_eq!(quantize(-0.38, 0.25), -0.5);
        assert_eq!(quantize(-0.125, 0.25), 0.0);
        assert_eq!(quantize(1.78, 0.25), 1.75);
        assert_eq!(quantize(1.78, 0.0), 1.78);
        assert_eq!(quantize(1.78, f32::INFINITY), 1.78);
        assert_eq!(quantize(f32::MAX, 0.25), f32::MAX);
        assert_eq!(threshold(-0.5, -0.25), 0.0);
        assert_eq!(threshold(-0.25, -0.25), 1.0);
        assert_eq!(threshold(0.75, -0.25), 1.0);
        assert_eq!(threshold(f32::NAN, -0.25), 0.0);
    }
}
