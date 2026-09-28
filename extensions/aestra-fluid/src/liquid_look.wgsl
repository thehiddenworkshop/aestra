// Aestra Fluid's liquid look (fluid F8): the surface of a liquid's particles, found where the ray
// first crosses a level of the liquid fraction they splat on the grid, and shaded as water — lit
// body colour absorbed through the liquid's thickness, a Fresnel reflection and a specular highlight.
// Composed after the backend's volume interface; field slot 0 is the liquid fraction (1 where the
// liquid is full).
//
// Constant words (packed by `pack_liquid_look` in liquid.rs):
//   0 march steps            1 surface level          2 absorption per unit of thickness
//   3 ambient                4..6 body colour          7 light intensity
//   8..10 light direction (effect space, towards the light, unit length)
//   11 shininess             12..14 light colour

fn liquid_fraction(uvw: vec3<f32>) -> f32 {
    return aestra_volume_field(0u, uvw).x;
}

fn liquid_look_vec3(index: u32) -> vec3<f32> {
    return vec3<f32>(
        aestra_volume_constant_f32(index),
        aestra_volume_constant_f32(index + 1u),
        aestra_volume_constant_f32(index + 2u),
    );
}

// The surface normal at `uvw`, in effect space: against the fraction's gradient, one cell across.
fn liquid_normal(uvw: vec3<f32>, size: vec3<f32>) -> vec3<f32> {
    let e = vec3<f32>(1.0) / vec3<f32>(aestra_volume.dims.xyz);
    let gradient = vec3<f32>(
        liquid_fraction(uvw + vec3<f32>(e.x, 0.0, 0.0)) - liquid_fraction(uvw - vec3<f32>(e.x, 0.0, 0.0)),
        liquid_fraction(uvw + vec3<f32>(0.0, e.y, 0.0)) - liquid_fraction(uvw - vec3<f32>(0.0, e.y, 0.0)),
        liquid_fraction(uvw + vec3<f32>(0.0, 0.0, e.z)) - liquid_fraction(uvw - vec3<f32>(0.0, 0.0, e.z)),
    ) / (2.0 * e * size);
    return -gradient / max(length(gradient), 1e-6);
}

fn liquid_jitter(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
}

fn liquid_look(ray: AestraVolumeRay) -> vec4<f32> {
    let steps = max(aestra_volume_constant(0u), 1u);
    let level = aestra_volume_constant_f32(1u);
    let span = ray.t_far - ray.t_near;
    if (span <= 0.0) {
        return vec4<f32>(0.0);
    }
    let dt = span / f32(steps);

    // The first crossing of the level, then bisected down to a small fraction of a step.
    var t = ray.t_near;
    var hit = liquid_fraction(ray.origin + ray.direction * t) >= level;
    if (!hit) {
        t += dt * liquid_jitter(ray.pixel);
        var before = t;
        for (var i = 0u; i < steps; i += 1u) {
            if (t > ray.t_far) {
                break;
            }
            if (liquid_fraction(ray.origin + ray.direction * t) >= level) {
                hit = true;
                var low = before;
                var high = t;
                for (var k = 0u; k < 5u; k += 1u) {
                    let middle = 0.5 * (low + high);
                    if (liquid_fraction(ray.origin + ray.direction * middle) >= level) {
                        high = middle;
                    } else {
                        low = middle;
                    }
                }
                t = high;
                break;
            }
            before = t;
            t += dt;
        }
    }
    if (!hit) {
        return vec4<f32>(0.0);
    }
    let surface = ray.origin + ray.direction * t;

    // How much liquid the ray crosses behind the surface, for the absorption.
    var thickness = 0.0;
    var inside = t;
    for (var i = 0u; i < steps; i += 1u) {
        inside += dt;
        if (inside > ray.t_far || liquid_fraction(ray.origin + ray.direction * inside) < level) {
            break;
        }
        thickness += dt;
    }

    // Where the liquid meets the box, the ray enters it through a box face: that face is the surface.
    var normal = liquid_normal(surface, ray.size);
    if (t <= ray.t_near) {
        let edge = min(surface, vec3<f32>(1.0) - surface);
        let low = surface < vec3<f32>(0.5);
        if (edge.x <= edge.y && edge.x <= edge.z) {
            normal = vec3<f32>(select(1.0, -1.0, low.x), 0.0, 0.0);
        } else if (edge.y <= edge.z) {
            normal = vec3<f32>(0.0, select(1.0, -1.0, low.y), 0.0);
        } else {
            normal = vec3<f32>(0.0, 0.0, select(1.0, -1.0, low.z));
        }
    }
    let view = -normalize(ray.direction * ray.size);
    let towards_light = normalize(liquid_look_vec3(8u));
    let light = liquid_look_vec3(12u) * aestra_volume_constant_f32(7u);
    let ambient = aestra_volume_constant_f32(3u);
    let half_vector = normalize(towards_light + view);
    let diffuse = max(dot(normal, towards_light), 0.0);
    let specular = pow(max(dot(normal, half_vector), 0.0), aestra_volume_constant_f32(11u));
    let facing = max(dot(normal, view), 0.0);
    let fresnel = 0.02 + 0.98 * pow(1.0 - facing, 5.0);

    let absorbed = 1.0 - exp(-aestra_volume_constant_f32(2u) * max(thickness, dt));
    let body = liquid_look_vec3(4u) * (ambient + 0.6 * diffuse * light);
    let reflected = light * (0.35 * fresnel + specular);
    let alpha = clamp(absorbed + fresnel * (1.0 - absorbed), 0.0, 1.0);
    return vec4<f32>(body * absorbed * (1.0 - fresnel) + reflected, alpha);
}
