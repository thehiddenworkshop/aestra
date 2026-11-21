
fn aestra_dither_cell(position: vec2<f32>, cell_size: f32) -> vec2<i32> {
    let exponent = bitcast<u32>(cell_size) & 0x7f800000u;
    let size = select(1.0, cell_size, cell_size > 0.0 && exponent != 0x7f800000u);
    let bounded = clamp(floor(position / size), vec2<f32>(-16777216.0), vec2<f32>(16777216.0));
    return vec2<i32>(bounded);
}

fn aestra_binary_coverage(alpha: f32, rank: u32, tile_area: u32) -> f32 {
    return select(0.0, 1.0, alpha > (f32(rank) + 0.5) / f32(tile_area));
}
