//! Pixel-grid presentation and offscreen images for host-owned cameras.

use bevy::{
    camera::Viewport,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{TextureFormat, TextureUsages},
};

/// Snap native GPU sprite and flipbook centers to the pixels of a rendered `Camera2d`.
///
/// The grid starts at the viewport's top-left pixel edge. Centers round to the
/// nearest edge, with ties toward the right and bottom. Snapping translates
/// the projected quad and leaves simulation, quad size, and depth intact.
/// Use a low-resolution render target with nearest sampling and `Msaa::Off`
/// for pixel art. Cameras with temporal jitter use smooth placement.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PixelGrid;

/// Snap native GPU Ribbon and Trail joints, Trail cap centers, and Beam endpoints in `Camera2d`.
///
/// Both segments at a joint use the same viewport-pixel offset to keep the strip joined.
/// Width, simulation, and material coordinates keep their smooth-path values. Use a
/// nearest-filtered low-resolution target for pixel art. Cameras with temporal jitter use
/// smooth placement.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PixelTrailGrid;

/// Creates a nearest-filtered sRGB image for a 2D or 3D camera's image target.
///
/// Use a transparent camera clear for isolated effects. Standard source-over rendering
/// into that target yields premultiplied RGB in linear light. Composite accordingly or
/// convert to straight alpha before writing an ordinary PNG. The image supports GPU readback.
/// Callers must keep the size within their render device's texture limits.
pub fn pixel_render_target(size: UVec2) -> Option<Image> {
    if size.min_element() == 0 {
        return None;
    }
    size.x.checked_mul(size.y)?.checked_mul(4)?;
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    image.sampler = ImageSampler::nearest();
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    Some(image)
}

/// Fit a virtual image into a physical output at the largest whole-number scale.
///
/// Odd letterbox remainders leave the extra pixel on the right or bottom.
/// Returns `None` if either virtual dimension is zero or the image cannot fit at 1×.
pub fn integer_pixel_viewport(virtual_size: UVec2, output_size: UVec2) -> Option<Viewport> {
    if virtual_size.min_element() == 0 {
        return None;
    }
    let scale = (output_size / virtual_size).min_element();
    if scale == 0 {
        return None;
    }
    let physical_size = virtual_size * scale;
    Some(Viewport {
        physical_position: (output_size - physical_size) / 2,
        physical_size,
        ..default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_render_target_preserves_dimensions_alpha_and_readback_usage() {
        let size = UVec2::new(96, 48);
        let image = pixel_render_target(size).unwrap();
        assert_eq!(image.size(), size);
        assert_eq!(
            image.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb
        );
        assert!(image.texture_descriptor.usage.contains(
            TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC
        ));
        assert_eq!(image.sampler, ImageSampler::nearest());
        assert!(pixel_render_target(UVec2::new(0, 48)).is_none());
        assert!(pixel_render_target(UVec2::splat(u32::MAX)).is_none());
    }

    #[test]
    fn virtual_viewport_resizes_at_integer_steps_with_pixel_aligned_letterboxing() {
        let virtual_size = UVec2::new(192, 128);
        for (output, scale, origin) in [
            (UVec2::new(192, 128), 1, UVec2::ZERO),
            (UVec2::new(768, 512), 4, UVec2::ZERO),
            (UVec2::new(769, 651), 4, UVec2::new(0, 69)),
            (UVec2::new(767, 512), 3, UVec2::new(95, 64)),
        ] {
            let viewport = integer_pixel_viewport(virtual_size, output).unwrap();
            assert_eq!(viewport.physical_size, virtual_size * scale);
            assert_eq!(viewport.physical_position, origin);
        }
        assert!(integer_pixel_viewport(virtual_size, UVec2::new(191, 200)).is_none());
        assert!(integer_pixel_viewport(virtual_size, UVec2::ZERO).is_none());
        assert!(integer_pixel_viewport(UVec2::new(0, 128), UVec2::splat(512)).is_none());
    }
}
