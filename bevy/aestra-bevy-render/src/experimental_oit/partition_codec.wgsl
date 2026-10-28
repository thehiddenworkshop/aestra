// Probe-only codec from Bevy 0.20 rgb9e5/draw (MIT OR Apache-2.0).
// The native interface is intentionally stubbed; production sort/resolve bodies
// are compiled unchanged except for region dispatch and fixed prepass selection.
fn pack_24bit_depth_8bit_alpha(depth: f32, alpha: f32) -> u32 {
    return (u32(saturate(depth) * f32(0xffffffu) + 0.5) << 8u) |
        u32(saturate(alpha) * 255.0 + 0.5);
}
fn unpack_24bit_depth_8bit_alpha(packed: u32) -> vec2<f32> {
    return vec2(f32(packed >> 8u) / f32(0xffffffu), f32(packed & 0xffu) / 255.0);
}
fn vec3_to_rgb9e5_(input: vec3<f32>) -> u32 {
    let rgb = clamp(input, vec3(0.0), vec3(65408.0));
    let maxrgb = max(rgb.r, max(rgb.g, rgb.b));
    let floor_log2 = i32((bitcast<u32>(maxrgb) & 0x7f800000u) >> 23u) - 127;
    var exponent = max(-16, floor_log2) + 16;
    var denom = exp2(f32(exponent - 24));
    if i32(floor(maxrgb / denom + 0.5)) == 512 {
        denom *= 2.0;
        exponent += 1;
    }
    let n = vec3<u32>(floor(rgb / denom + 0.5));
    return (u32(exponent) << 27u) | (n.b << 18u) | (n.g << 9u) | n.r;
}
fn rgb9e5_to_vec3_(v: u32) -> vec3<f32> {
    let scale = exp2(f32(i32(v >> 27u) - 24));
    return vec3(f32(v & 511u), f32((v >> 9u) & 511u), f32((v >> 18u) & 511u)) * scale;
}
