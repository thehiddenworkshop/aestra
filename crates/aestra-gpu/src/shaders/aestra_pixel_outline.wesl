fn aestra_outline_solid(texture: texture_2d<f32>, texel: vec2<i32>, cutoff: f32) -> bool {
    let alpha = textureLoad(texture, texel, 0).a;
    return alpha > 0.0 && alpha >= cutoff;
}

fn aestra_pixel_outline(
    texture: texture_2d<f32>,
    uv: vec2<f32>,
    bounds: vec4<f32>,
    width: f32,
    threshold: f32,
    allow_outside: bool,
) -> f32 {
    if !(width >= 0.5) || !all(bounds.xy < bounds.zw) {
        return 0.0;
    }
    if !allow_outside && (!all(uv >= bounds.xy) || !all(uv < bounds.zw)) {
        return 0.0;
    }
    let size = vec2<f32>(textureDimensions(texture, 0));
    // Only texels with centers in the half-open frame can contribute.
    let limits = ceil(clamp(bounds, vec4<f32>(0.0), vec4<f32>(1.0)) * vec4<f32>(size, size) - 0.5);
    let low = vec2<i32>(limits.xy);
    let high = vec2<i32>(limits.zw);
    if any(low >= high) {
        return 0.0;
    }
    let radius = i32(floor(min(width, 4.0) + 0.5));
    let margin = vec2<i32>(select(0, radius, allow_outside));
    let texel_position = uv * size;
    if !all(texel_position >= vec2<f32>(low - margin)) ||
       !all(texel_position < vec2<f32>(high + margin)) {
        return 0.0;
    }
    let center = vec2<i32>(floor(texel_position));
    let cutoff = clamp(threshold, 0.0, 1.0);
    if all(center >= low) && all(center < high) && aestra_outline_solid(texture, center, cutoff) {
        return 0.0;
    }
    // Manhattan distance makes one-texel corners follow the sprite's pixel staircase.
    for (var y = max(low.y, center.y - radius); y < min(high.y, center.y + radius + 1); y++) {
        let horizontal = radius - abs(y - center.y);
        for (var x = max(low.x, center.x - horizontal); x < min(high.x, center.x + horizontal + 1); x++) {
            let texel = vec2<i32>(x, y);
            if any(texel != center) && aestra_outline_solid(texture, texel, cutoff) {
                return 1.0;
            }
        }
    }
    return 0.0;
}
