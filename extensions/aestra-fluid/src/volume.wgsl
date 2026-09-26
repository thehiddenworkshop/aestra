// Aestra Fluid's volume look (fluid F3): smoke ray-marched through the density grid, lit by one
// directional light with self-shadowing, over an ambient term. Composed after the backend's volume
// interface (`aestra_gpu::volume::volume_interface_wgsl`); field slot 0 is density.
//
// Constant words (packed by `pack_volume` in lib.rs):
//   0 march steps            1 shadow steps           2 opacity (extinction per density per unit)
//   3 ambient                4..6 smoke colour        7 light intensity
//   8..10 light direction (effect space, towards the light, unit length)
//   12..14 light colour
//   16 temperature field slot (0xffffffff: no fire)   17 fire intensity   18 kelvin per unit

const FLUID_NO_SLOT: u32 = 0xffffffffu;

fn fluid_density(uvw: vec3<f32>) -> f32 {
    return max(aestra_volume_field(0u, uvw).x, 0.0);
}

// The colour of a blackbody at `kelvin` (Tanner Helland's fit of the Planckian locus), in linear RGB
// with its largest channel at 1.
fn fluid_blackbody(kelvin: f32) -> vec3<f32> {
    let t = clamp(kelvin, 1000.0, 40000.0) / 100.0;
    var rgb: vec3<f32>;
    if (t <= 66.0) {
        rgb.x = 255.0;
        rgb.y = 99.4708025861 * log(t) - 161.1195681661;
    } else {
        rgb.x = 329.698727446 * pow(t - 60.0, -0.1332047592);
        rgb.y = 288.1221695283 * pow(t - 60.0, -0.0755148492);
    }
    if (t >= 66.0) {
        rgb.z = 255.0;
    } else if (t <= 19.0) {
        rgb.z = 0.0;
    } else {
        rgb.z = 138.5177312231 * log(t - 10.0) - 305.0447927307;
    }
    return pow(clamp(rgb / 255.0, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2));
}

// Light the burning gas at `uvw` emits per unit length: blackbody-coloured, rising steeply with
// temperature above the dull-red glow at ~600 K.
fn fluid_fire(uvw: vec3<f32>, slot: u32) -> vec3<f32> {
    let kelvin = max(aestra_volume_field(slot, uvw).x, 0.0) * aestra_volume_constant_f32(18u);
    let glow = max(kelvin - 600.0, 0.0) / 1400.0;
    return fluid_blackbody(kelvin) * (glow * glow * glow) * aestra_volume_constant_f32(17u);
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

    let fire_slot = aestra_volume_constant(16u);

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
        // Burning gas glows whether or not it carries smoke; smoke in front of it hides it.
        if (fire_slot != FLUID_NO_SLOT) {
            color += transmittance * fluid_fire(p, fire_slot) * dt;
        }
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
