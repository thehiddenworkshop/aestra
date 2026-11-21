
fn aestra_blue_noise(alpha: f32, position: vec2<f32>, cell_size: f32) -> f32 {
    let tile_mask = i32(AESTRA_BLUE_NOISE_WIDTH - 1u);
    let cell = vec2<u32>(aestra_dither_cell(position, cell_size) & vec2<i32>(tile_mask));
    let index = cell.y * AESTRA_BLUE_NOISE_WIDTH + cell.x;
    let word = AESTRA_BLUE_NOISE_RANKS[index >> 1u];
    let rank = (word >> ((index & 1u) * 16u)) & 0xffffu;
    return aestra_binary_coverage(alpha, rank, AESTRA_BLUE_NOISE_WIDTH * AESTRA_BLUE_NOISE_WIDTH);
}
