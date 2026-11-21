
fn aestra_bayer_rank2(x: i32, y: i32) -> i32 {
    return 2 * (x ^ y) + y;
}

fn aestra_bayer(alpha: f32, position: vec2<f32>, cell_size: f32, levels: u32) -> f32 {
    let cell = aestra_dither_cell(position, cell_size);
    var rank = aestra_bayer_rank2(cell.x & 1, cell.y & 1);
    if levels >= 2u {
        rank = 4 * rank + aestra_bayer_rank2((cell.x >> 1u) & 1, (cell.y >> 1u) & 1);
    }
    if levels >= 3u {
        rank = 4 * rank + aestra_bayer_rank2((cell.x >> 2u) & 1, (cell.y >> 2u) & 1);
    }
    let tile_area = 1u << (2u * levels);
    return aestra_binary_coverage(alpha, u32(rank), tile_area);
}
