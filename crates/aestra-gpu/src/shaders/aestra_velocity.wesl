// Explicit F2 modes; mirrors aestra_runtime::sample_velocity_distribution.
// The caller supplies independent direction uniforms (channels 30 and 31).
fn aestra_velocity_direction(mode: u32, axis: vec3<f32>, spread_degrees: f32, u: f32, v: f32) -> vec3<f32> {
    var forward = vec3<f32>(0.0, 1.0, 0.0);
    // Avoid a subnormal reciprocal for extreme finite axes on GPU hardware.
    var safe_axis = axis;
    if max(abs(axis.x), max(abs(axis.y), abs(axis.z))) > 1e20 {
        safe_axis = axis * 1e-20;
    }
    let largest = max(abs(safe_axis.x), max(abs(safe_axis.y), abs(safe_axis.z)));
    if largest > 0.0 {
        let scaled = safe_axis / largest;
        forward = scaled / sqrt(dot(scaled, scaled));
    }
    if mode <= 1u { return forward; }
    var height = 0.0;
    var radius = 1.0;
    if mode == 2u {
        let half_angle = min(abs(spread_degrees) * 0.008726646259971648, 3.141592653589793);
        height = 1.0 - u * (1.0 - cos(half_angle));
        radius = sqrt(max(1.0 - height * height, 0.0));
    } else if mode == 3u {
        height = 1.0 - 2.0 * u;
        radius = sqrt(max(1.0 - height * height, 0.0));
    } else if mode == 4u {
        height = u;
        radius = sqrt(max(1.0 - u * u, 0.0));
    } else if mode == 5u {
        radius = sqrt(u);
    }
    var helper = vec3<f32>(0.0, 1.0, 0.0);
    if largest == 0.0 || abs(axis.y) > abs(axis.x) { helper = vec3<f32>(1.0, 0.0, 0.0); }
    let tangent = normalize(cross(helper, forward));
    let bitangent = cross(forward, tangent);
    let phi = v * 6.283185307179586;
    return forward * height + tangent * (radius * cos(phi)) + bitangent * (radius * sin(phi));
}
