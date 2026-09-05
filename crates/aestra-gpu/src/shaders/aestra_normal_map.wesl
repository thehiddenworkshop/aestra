// Linear RGB normal-map decoding. Alpha is ignored by the caller.
fn aestra_normal_map_unit(value: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    let length_squared = dot(value, value);
    if length_squared <= 0.000000000001 { return fallback; }
    return value * inverseSqrt(length_squared);
}

fn aestra_normal_map(sample: vec3<f32>, strength: f32, flip_y: bool,
    normal: vec3<f32>, tangent: vec3<f32>, bitangent: vec3<f32>) -> vec3<f32> {
    let n = aestra_normal_map_unit(normal, vec3<f32>(0.0, 0.0, 1.0));
    if strength <= 0.0 { return n; }
    // Repair interpolation drift without discarding the imported/mirrored basis orientation.
    let axis = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.y) < 0.99);
    let t = aestra_normal_map_unit(tangent - n * dot(n, tangent), normalize(cross(axis, n)));
    let cross_nt = cross(n, t);
    let b = cross_nt * select(-1.0, 1.0, dot(cross_nt, bitangent) >= 0.0);
    let decoded = sample * 2.0 - vec3<f32>(1.0);
    let local = vec3<f32>(decoded.x * strength, decoded.y * strength * select(1.0, -1.0, flip_y), decoded.z);
    return aestra_normal_map_unit(t * local.x + b * local.y + n * local.z, n);
}
