struct AestraGpuLight2d {
    center: vec2<f32>,
    radius: f32,
    bands: u32,
    irradiance: vec3<f32>,
}

struct AestraGpuLights2d {
    count: u32,
    lights: array<AestraGpuLight2d>,
}

@group(3) @binding(0)
var<storage, read> aestra_lights_2d: AestraGpuLights2d;

fn aestra_point_irradiance_2d(world: vec2<f32>) -> vec3<f32> {
    return aestra_accumulate_lights_2d(world, vec3<f32>(0.0), false);
}

fn aestra_accumulate_lights_2d(world: vec2<f32>, normal: vec3<f32>, directional: bool) -> vec3<f32> {
    var irradiance = vec3<f32>(0.0);
    for (var i = 0u; i < aestra_lights_2d.count; i += 1u) {
        let light = aestra_lights_2d.lights[i];
        let offset = light.center - world;
        var attenuation = max(1.0 - length(offset) / light.radius, 0.0);
        if directional && attenuation > 0.0 {
            // Virtual elevation gives a forward-facing surface light at the center.
            let direction = normalize(vec3<f32>(offset, max(light.radius * 0.5, 0.00001)));
            attenuation *= clamp(dot(normal, direction), 0.0, 1.0);
        }
        if (light.bands != 0u) {
            let bands = f32(light.bands);
            attenuation = ceil(attenuation * bands) / bands;
        }
        irradiance += light.irradiance * attenuation;
    }
    return irradiance;
}

fn aestra_normal_point_irradiance_2d(world: vec2<f32>, quad: vec2<f32>, normal: vec3<f32>) -> vec3<f32> {
    // Recover the rendered quad basis from its geometric coordinates, which stay
    // local across atlas frame changes. Particle rotation and camera roll are
    // already part of the world position of the quad.
    let quad_dx = dpdx(quad);
    let quad_dy = dpdy(quad);
    let world_dx = dpdx(world);
    let world_dy = dpdy(world);
    let determinant = quad_dx.x * quad_dy.y - quad_dx.y * quad_dy.x;
    let tangent = (quad_dy.y * world_dx - quad_dx.y * world_dy) * sign(determinant);
    let bitangent = (quad_dx.x * world_dy - quad_dy.x * world_dx) * sign(determinant);
    var xy = vec2<f32>(0.0);
    if dot(tangent, tangent) > 0.0 && dot(bitangent, bitangent) > 0.0 {
        xy = normal.x * normalize(tangent) + normal.y * normalize(bitangent);
    }
    let world_normal = vec3<f32>(xy, normal.z);
    let squared_length = dot(world_normal, world_normal);
    if squared_length > 0.000000000001 {
        return aestra_accumulate_lights_2d(world, world_normal * inverseSqrt(squared_length), true);
    }
    return aestra_accumulate_lights_2d(world, vec3<f32>(0.0, 0.0, 1.0), true);
}
