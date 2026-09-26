// Aestra Fluid's volume look (fluid F3): smoke ray-marched through the density grid, lit by one
// directional light with self-shadowing, over an ambient term. Composed after the backend's volume
// interface (`aestra_gpu::volume::volume_interface_wgsl`); field slot 0 is density.
//
// Constant words (packed by `pack_volume` in lib.rs):
//   0 march steps            1 shadow steps           2 opacity (extinction per density per unit)
//   3 ambient                4..6 smoke colour        7 light intensity
//   8..10 light direction (effect space, towards the light, unit length)
//   12..14 light colour

fn fluid_density(uvw: vec3<f32>) -> f32 {
    return max(aestra_volume_field(0u, uvw).x, 0.0);
}

fn fluid_vec3(index: u32) -> vec3<f32> {
    return vec3<f32>(
        aestra_volume_constant_f32(index),
        aestra_volume_constant_f32(index + 1u),
        aestra_volume_constant_f32(index + 2u),
    );
}

// Interleaved gradient noise: offsets each pixel's first sample so banding becomes fine noise.
fn fluid_jitter(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
}

// Light reaching grid point `uvw`: transmittance along `towards_light` (grid coordinates per unit
// length) out of the box, over `steps` steps of `step` units.
fn fluid_shadow(uvw: vec3<f32>, towards_light: vec3<f32>, step: f32, steps: u32, opacity: f32) -> f32 {
    var optical_depth = 0.0;
    for (var i = 1u; i <= steps; i += 1u) {
        let p = uvw + towards_light * (f32(i) * step);
        if (any(p < vec3<f32>(0.0)) || any(p > vec3<f32>(1.0))) {
            break;
        }
        optical_depth += fluid_density(p) * opacity * step;
    }
    return exp(-optical_depth);
}

fn fluid_volume(ray: AestraVolumeRay) -> vec4<f32> {
    let steps = max(aestra_volume_constant(0u), 1u);
    let shadow_steps = aestra_volume_constant(1u);
    let opacity = aestra_volume_constant_f32(2u);
    let ambient = aestra_volume_constant_f32(3u);
    let albedo = fluid_vec3(4u);
    let light = fluid_vec3(12u) * aestra_volume_constant_f32(7u);
    let towards_light = fluid_vec3(8u) / ray.size;
    // Shadow rays cover half the box's largest side.
    let shadow_step = max(ray.size.x, max(ray.size.y, ray.size.z)) * 0.5 / f32(max(shadow_steps, 1u));

    let span = ray.t_far - ray.t_near;
    if (span <= 0.0) {
        return vec4<f32>(0.0);
    }
    let dt = span / f32(steps);
    var t = ray.t_near + dt * fluid_jitter(ray.pixel);
    var transmittance = 1.0;
    var color = vec3<f32>(0.0);
    for (var i = 0u; i < steps; i += 1u) {
        if (t > ray.t_far) {
            break;
        }
        let p = ray.origin + ray.direction * t;
        let density = fluid_density(p);
        if (density > 1e-4) {
            let absorbed = 1.0 - exp(-density * opacity * dt);
            var lit = vec3<f32>(ambient);
            if (shadow_steps > 0u) {
                lit += light * fluid_shadow(p, towards_light, shadow_step, shadow_steps, opacity);
            } else {
                lit += light;
            }
            color += transmittance * absorbed * albedo * lit;
            transmittance *= 1.0 - absorbed;
            if (transmittance < 0.005) {
                break;
            }
        }
        t += dt;
    }
    return vec4<f32>(color, 1.0 - transmittance);
}
