fn aestra_palette_width(palette: texture_2d<f32>) -> u32 {
    let size = textureDimensions(palette, 0);
    return select(0u, size.x, size.y == 1u && size.x <= AESTRA_MAX_PALETTE_COLORS);
}

fn aestra_palette_ramp(palette: texture_2d<f32>, factor: f32) -> vec4<f32> {
    let width = aestra_palette_width(palette);
    // A runtime expression can produce NaN. Use the first band in that case.
    let position = clamp(select(0.0, factor, factor == factor), 0.0, 1.0);
    if width == 0u {
        return vec4<f32>(vec3<f32>(position), 1.0);
    }
    let band = min(u32(position * f32(width)), width - 1u);
    return vec4<f32>(textureLoad(palette, vec2<i32>(i32(band), 0), 0).rgb, 1.0);
}

struct AestraPaletteMatch {
    index: u32,
    rgb: vec3<f32>,
}

fn aestra_match_palette(palette: texture_2d<f32>, width: u32, source: vec3<f32>) -> AestraPaletteMatch {
    var best = AestraPaletteMatch(0u, textureLoad(palette, vec2<i32>(0, 0), 0).rgb);
    let first_delta = source - best.rgb;
    var best_distance = dot(first_delta, first_delta);
    for (var index = 1u; index < width && best_distance > 0.0; index += 1u) {
        let swatch = textureLoad(palette, vec2<i32>(i32(index), 0), 0).rgb;
        let delta = source - swatch;
        let distance = dot(delta, delta);
        if distance < best_distance {
            best = AestraPaletteMatch(index, swatch);
            best_distance = distance;
        }
    }
    return best;
}

fn aestra_nearest_palette_color(palette: texture_2d<f32>, source: vec4<f32>) -> vec4<f32> {
    let width = aestra_palette_width(palette);
    if width == 0u {
        return source;
    }
    return vec4<f32>(aestra_match_palette(palette, width, source.rgb).rgb, source.a);
}

fn aestra_palette_shift(palette: texture_2d<f32>, source: vec4<f32>, steps: f32) -> vec4<f32> {
    let width = aestra_palette_width(palette);
    if width == 0u {
        return source;
    }
    let nearest = aestra_match_palette(palette, width, source.rgb);
    // Reduce before converting to an integer so large offsets cannot saturate the index.
    let exponent = bitcast<u32>(steps) & 0x7f800000u;
    let finite_steps = select(0.0, steps, exponent != 0x7f800000u);
    let shift = i32(trunc(finite_steps) % f32(width));
    if shift == 0 {
        return vec4<f32>(nearest.rgb, source.a);
    }
    let count = i32(width);
    let index = (i32(nearest.index) + shift + count) % count;
    return vec4<f32>(textureLoad(palette, vec2<i32>(index, 0), 0).rgb, source.a);
}

fn aestra_palette_swap(
    source_palette: texture_2d<f32>,
    target_palette: texture_2d<f32>,
    color: vec4<f32>,
) -> vec4<f32> {
    let width = aestra_palette_width(source_palette);
    if width == 0u || width != aestra_palette_width(target_palette) || color.a <= 0.0 {
        return color;
    }
    for (var index = 0u; index < width; index += 1u) {
        let texel = vec2<i32>(i32(index), 0);
        let swatch = textureLoad(source_palette, texel, 0).rgb;
        // Allow rounding differences between filtered sampling and a texel load.
        // The tolerance is smaller than the closest pair of 8-bit sRGB codes in linear space.
        if all(abs(color.rgb - swatch) <= vec3<f32>(0.0001)) {
            return vec4<f32>(textureLoad(target_palette, texel, 0).rgb, color.a);
        }
    }
    return color;
}
