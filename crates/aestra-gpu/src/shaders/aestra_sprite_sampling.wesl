// (geometry expansion, alpha attenuation). Preserve continuous projected area × alpha.
// A zero-area particle must not acquire energy. Tiny footprints are intentionally culled
// by geometry rather than overflowing the expansion ratio. No radius/opacity simulation edits.
fn aestra_sprite_sampling(projected_pixels: f32, minimum_pixels: f32) -> vec2<f32> {
    if minimum_pixels <= 0.0 || projected_pixels >= minimum_pixels || projected_pixels <= 0.000001 {
        return vec2<f32>(1.0, 1.0);
    }
    let ratio = projected_pixels / minimum_pixels;
    return vec2<f32>(1.0 / ratio, ratio * ratio);
}

// Center Jacobian in physical main-pass pixels. Independent of viewport origin and jitter;
// exact for standard camera-facing perspective/orthographic quads at constant view depth.
fn aestra_sprite_projected_pixels(clip_from_world: mat4x4<f32>, center: vec4<f32>,
    axis_x: vec3<f32>, axis_y: vec3<f32>, viewport_size: vec2<f32>) -> f32 {
    let clip = clip_from_world * center;
    if clip.w <= 0.000001 { return 0.0; }
    let x = clip_from_world * vec4<f32>(axis_x, 0.0);
    let y = clip_from_world * vec4<f32>(axis_y, 0.0);
    let ndc = clip.xy / clip.w;
    let factor = max(viewport_size, vec2<f32>(1.0)) * 0.5 / clip.w;
    return min(length((x.xy - ndc * x.w) * factor), length((y.xy - ndc * y.w) * factor));
}
