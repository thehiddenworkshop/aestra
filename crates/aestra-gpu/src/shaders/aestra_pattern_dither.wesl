
fn aestra_pattern_dither(
    pattern: texture_2d<f32>,
    alpha: f32,
    position: vec2<f32>,
    cell_size: f32,
) -> f32 {
    let size = vec2<i32>(textureDimensions(pattern, 0));
    let cell = aestra_dither_cell(position, cell_size) % size;
    let wrapped = select(cell, cell + size, cell < vec2<i32>(0));
    let threshold = textureLoad(pattern, wrapped, 0).a;
    // The renderer uses a negative alpha texel until the pattern is ready.
    if threshold < 0.0 {
        return alpha;
    }
    return select(0.0, 1.0, alpha >= 1.0 || (alpha > 0.0 && alpha > threshold));
}
