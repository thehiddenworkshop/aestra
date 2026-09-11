// noise.wesl — sample WESL module for testing the Aestra editor.
// Open it from the Assets browser, toggle Edit/Preview, and edit to see live diagnostics.

fn hash2(p: vec2<f32>) -> f32 {
    let k = vec2<f32>(127.1, 311.7);
    return fract(sin(dot(p, k)) * 43758.5453);
}

// Smooth value noise in the 0..1 range.
fn value_noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let frac = fract(p);
    let a = hash2(cell);
    let b = hash2(cell + vec2<f32>(1.0, 0.0));
    let c = hash2(cell + vec2<f32>(0.0, 1.0));
    let d = hash2(cell + vec2<f32>(1.0, 1.0));
    let u = frac * frac * (3.0 - 2.0 * frac);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}
