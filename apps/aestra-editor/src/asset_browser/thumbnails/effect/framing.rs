//! Fit the final camera to visible pixels, not the deliberately conservative history bounds.
use super::*;

pub(super) fn fit(bytes: &[u8]) -> Option<(Vec2, f32)> {
    if bytes.len() != (EDGE * EDGE * 4) as usize {
        return None;
    }
    // The isolated camera has a uniform background. Median corners tolerate one faint edge pixel.
    let corners = [0, EDGE - 1, EDGE * (EDGE - 1), EDGE * EDGE - 1];
    let background: [u8; 3] = std::array::from_fn(|channel| {
        let mut values = corners.map(|i| bytes[i as usize * 4 + channel]);
        values.sort_unstable();
        values[1]
    });
    let mut lo = UVec2::splat(EDGE);
    let mut hi = UVec2::ZERO;
    let mut count = 0;
    for (index, pixel) in bytes.as_chunks::<4>().0.iter().enumerate() {
        if (0..3).any(|channel| pixel[channel].abs_diff(background[channel]) > 3) {
            let p = UVec2::new(index as u32 % EDGE, index as u32 / EDGE);
            lo = lo.min(p);
            hi = hi.max(p);
            count += 1;
        }
    }
    if count < 4 {
        return None;
    }
    // Keep a visible margin for soft edges. Limit each zoom to 8x and never crop a filled image.
    let side = ((hi - lo + UVec2::ONE).max_element() as f32 + 2.0) / 0.84;
    let scale = (side / EDGE as f32).max(0.125);
    if scale >= 0.98 {
        return None;
    }
    let center = (lo.as_vec2() + hi.as_vec2() + Vec2::ONE) * 0.5;
    let offset = center - Vec2::splat(EDGE as f32 * 0.5);
    Some((Vec2::new(offset.x, -offset.y) / EDGE as f32, scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_pixels_center_and_enlarge_with_padding() {
        let mut pixels = [6, 7, 9, 255].repeat((EDGE * EDGE) as usize);
        assert!(fit(&pixels).is_none());
        for y in 10..26 {
            for x in 72..80 {
                let index = ((y * EDGE + x) * 4) as usize;
                pixels[index..index + 4].copy_from_slice(&[120, 150, 200, 255]);
            }
        }
        let (offset, scale) = fit(&pixels).unwrap();
        assert!(offset.x > 0.0 && offset.y > 0.0);
        assert!(scale > 0.125 && scale < 0.2);
        assert!(16.0 / scale < EDGE as f32 * 0.84);
    }

    #[test]
    fn nearly_full_frame_is_not_zoomed_and_bad_capture_is_rejected() {
        let mut pixels = [6, 7, 9, 255].repeat((EDGE * EDGE) as usize);
        for y in 4..EDGE - 4 {
            for x in 4..EDGE - 4 {
                pixels[((y * EDGE + x) * 4) as usize] = 255;
            }
        }
        assert!(fit(&pixels).is_none());
        assert!(fit(&[]).is_none());
    }
}
