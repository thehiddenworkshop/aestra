// gradient.wesl — sample WESL module for testing the Aestra editor.

// A simple two-colour vertical gradient.
fn vertical_gradient(uv: vec2<f32>, top: vec3<f32>, bottom: vec3<f32>) -> vec3<f32> {
    return mix(bottom, top, clamp(uv.y, 0.0, 1.0));
}

// Radial falloff from the centre of the uv space.
fn radial_falloff(uv: vec2<f32>, softness: f32) -> f32 {
    let centered = uv - vec2<f32>(0.5, 0.5);
    let dist = length(centered) * 2.0;
    return 1.0 - smoothstep(1.0 - softness, 1.0, dist);
}
