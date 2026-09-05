//! Rebuild the numerical normal-map fixture: cargo run -p aestra-viewer --example generate_normal_map
//! Run from the repository root. This is linear vector data, not an sRGB illustration.
fn main() -> Result<(), image::ImageError> {
    let size = 64;
    let image = image::RgbImage::from_fn(size, size, |x, y| {
        let u = (x as f32 + 0.5) / size as f32 * std::f32::consts::TAU * 2.0;
        let v = (y as f32 + 0.5) / size as f32 * std::f32::consts::TAU * 2.0;
        // Periodic crossed waves: analytic slopes keep the texture tileable.
        let nx = -0.8 * u.cos() * v.sin();
        let ny = -0.8 * u.sin() * v.cos();
        let length = (nx * nx + ny * ny + 1.0).sqrt();
        image::Rgb([nx, ny, 1.0].map(|value| ((value / length * 0.5 + 0.5) * 255.0).round() as u8))
    });
    image.save("assets/textures/lab_normal.png")
}
