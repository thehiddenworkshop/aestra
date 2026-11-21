
fn aestra_pixel_finite(value: f32) -> bool {
    return (bitcast<u32>(value) & 0x7f800000u) != 0x7f800000u;
}

fn aestra_pixel_snap(position: vec2<f32>, cell_size: f32) -> vec2<f32> {
    let size = select(1.0, cell_size, aestra_pixel_finite(cell_size) && cell_size > 0.0);
    let center = (floor(position / size) + vec2<f32>(0.5)) * size;
    let finite = (bitcast<vec2<u32>>(center) & vec2<u32>(0x7f800000u)) != vec2<u32>(0x7f800000u);
    return select(position, center, finite);
}

fn aestra_posterize(color: vec4<f32>, levels: f32) -> vec4<f32> {
    let count = select(2.0, levels, aestra_pixel_finite(levels));
    let intervals = clamp(floor(count + 0.5), 2.0, 16777216.0) - 1.0;
    let snapped = floor(color.rgb * intervals + vec3<f32>(0.5)) / intervals;
    let finite = (bitcast<vec3<u32>>(snapped) & vec3<u32>(0x7f800000u)) != vec3<u32>(0x7f800000u);
    return vec4<f32>(select(color.rgb, snapped, finite), color.a);
}

fn aestra_pixelate_uv(uv: vec2<f32>, cells: vec2<f32>) -> vec2<f32> {
    let valid = vec2<bool>(aestra_pixel_finite(cells.x), aestra_pixel_finite(cells.y));
    let count = clamp(floor(select(vec2<f32>(1.0), cells, valid) + 0.5), vec2<f32>(1.0), vec2<f32>(8388608.0));
    let center = (floor(uv * count) + vec2<f32>(0.5)) / count;
    let finite = vec2<bool>(aestra_pixel_finite(center.x), aestra_pixel_finite(center.y));
    return select(uv, center, finite);
}

fn aestra_quantize(value: f32, step: f32) -> f32 {
    if !aestra_pixel_finite(step) || step <= 0.0 {
        return value;
    }
    let rounded = floor(value / step + 0.5) * step;
    return select(value, rounded, aestra_pixel_finite(rounded));
}

fn aestra_threshold(value: f32, cutoff: f32) -> f32 {
    return select(0.0, 1.0, value >= cutoff);
}
