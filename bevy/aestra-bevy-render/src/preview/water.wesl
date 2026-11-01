import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::{view, clustered_lights},
    clustered_forward as clusters,
};

struct Water {
    parameters: vec4<f32>,
    sky_radiance: vec4<f32>,
    reflection_clip: mat4x4<f32>,
    reflection_texel: vec4<f32>,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> water: Water;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var reflected_image: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var reflected_sampler: sampler;

// Reflection mode replaces analytical specular glints with captured radiance.
// Retain unshadowed point-light Lambert illumination without counting a luminous
// star both as a point specular source and as a reflected particle image.
fn receiver_diffuse(world: vec3<f32>, normal: vec3<f32>, pixel: vec2<f32>) -> vec3<f32> {
    let z = (view.view_from_world * vec4<f32>(world, 1.0)).z;
    let cluster = clusters::view_fragment_cluster_index(pixel, z, false);
    let range = clusters::unpack_clusterable_object_index_ranges(cluster);
    var result = vec3<f32>(0.0);
    for (var offset = range.first_point_light_index_offset; offset < range.first_spot_light_index_offset; offset += 1u) {
        let id = clusters::get_clusterable_object_id(offset);
        let light = clustered_lights.data[id];
        if light.range <= 0.0 { continue; }
        let delta = light.position_radius.xyz - world;
        let d2 = max(dot(delta, delta), 0.0001);
        let factor = d2 * light.color_inverse_square_range.w;
        let fade = max(1.0 - factor * factor, 0.0);
        let attenuation = fade * fade / max(d2, max(light.position_radius.w * light.position_radius.w, 0.0001));
        result += max(light.color_inverse_square_range.xyz, vec3<f32>(0.0)) * attenuation
            * max(dot(normal, delta * inverseSqrt(d2)), 0.0) * 0.318309886;
    }
    return result * view.exposure;
}

// Isotropic GGX visible-normal sampling. The capture supplies directional HDR
// radiance; wave normals determine reflection directions rather than a UV offset.
fn visible_normal(v: vec3<f32>, alpha: f32, u: vec2<f32>) -> vec3<f32> {
    let vh = normalize(vec3<f32>(alpha * v.xy, v.z));
    let lensq = dot(vh.xy, vh.xy);
    var t1 = vec3<f32>(1.0, 0.0, 0.0);
    if lensq > 0.000001 { t1 = vec3<f32>(-vh.y, vh.x, 0.0) * inverseSqrt(lensq); }
    let t2 = cross(vh, t1);
    let radius = sqrt(u.x);
    let phi = 6.2831853 * u.y;
    let x = radius * cos(phi);
    let original_y = radius * sin(phi);
    let blend = 0.5 * (1.0 + vh.z);
    let y = mix(sqrt(max(0.0, 1.0 - x*x)), original_y, blend);
    let nh = x*t1 + y*t2 + sqrt(max(0.0, 1.0 - x*x - y*y))*vh;
    return normalize(vec3<f32>(alpha*nh.xy, max(0.0,nh.z)));
}

fn smith_lambda(cosine: f32, alpha: f32) -> f32 {
    let c2 = max(cosine*cosine, 0.000001);
    return 0.5 * (sqrt(1.0 + alpha*alpha*(1.0-c2)/c2) - 1.0);
}

fn water_fresnel(cosine: f32) -> f32 {
    let c = clamp(cosine,0.0,1.0);
    let eta = 1.333;
    let ct = sqrt(max(0.0,1.0-(1.0-c*c)/(eta*eta)));
    let rs = (c-eta*ct)/(c+eta*ct);
    let rp = (eta*c-ct)/(eta*c+ct);
    return 0.5*(rs*rs+rp*rp);
}

fn reflection(normal: vec3<f32>, view_direction: vec3<f32>, roughness: f32) -> vec3<f32> {
    let nv = dot(normal,view_direction);
    if nv <= 0.0001 { return vec3<f32>(0.0); }
    let tangent = normalize(cross(vec3<f32>(0.0,0.0,1.0), normal));
    let bitangent = cross(normal,tangent);
    let v = vec3<f32>(dot(view_direction,tangent),dot(view_direction,bitangent),nv);
    let alpha = max(0.001,roughness*roughness);
    let lambda_v = smith_lambda(nv,alpha);
    let maximum_lod = f32(textureNumLevels(reflected_image)-1u);
    var result = vec3<f32>(0.0);
    for (var i=0u; i<12u; i+=1u) {
        let u = vec2<f32>((f32(i)+0.5)/12.0,fract(f32(reverseBits(i))*2.3283064365386963e-10+0.618033989));
        let h_local = visible_normal(v,alpha,u);
        let h = tangent*h_local.x+bitangent*h_local.y+normal*h_local.z;
        let light = reflect(-view_direction,h);
        let nl = dot(normal,light);
        if nl<=0.0001 || light.y<=0.0 { continue; }
        // Direction-only projection is a planar/far-field approximation, not a
        // depth-aware ray intersection. A flat normal recovers planar alignment.
        let clip = water.reflection_clip*vec4<f32>(light,0.0);
        if clip.w<=0.00001 { continue; }
        let uv = vec2<f32>(0.5+clip.x/clip.w*0.5,0.5-clip.y/clip.w*0.5);
        if any(uv<vec2<f32>(0.0)) || any(uv>vec2<f32>(1.0)) { continue; }
        let nh = max(dot(normal,h),0.0);
        let denominator = nh*nh*(alpha*alpha-1.0)+1.0;
        let distribution = alpha*alpha/(3.14159265*denominator*denominator);
        let pdf = max(distribution/(4.0*nv*(1.0+lambda_v)),0.000001);
        let focal = water.reflection_texel.zw;
        let plane_position = (uv*2.0-vec2<f32>(1.0))/focal;
        let texel_solid_angle = 4.0*water.reflection_texel.x*water.reflection_texel.y/
            (focal.x*focal.y*pow(1.0+dot(plane_position,plane_position),1.5));
        let lod = clamp(0.5*log2(1.0/(12.0*pdf*texel_solid_angle)),0.0,maximum_lod);
        let geometry = (1.0+lambda_v)/(1.0+lambda_v+smith_lambda(nl,alpha));
        let fresnel = water_fresnel(dot(view_direction,h));
        // Samples leaving the capture contribute zero; never renormalize them
        // or boost dark/rare glints to reconstruct a readable mirrored shell.
        result += textureSampleLevel(reflected_image,reflected_sampler,uv,lod).rgb*geometry*fresnel/12.0;
    }
    return result;
}

// Direction X/Z, wavelength in metres, slope amplitude. Each band follows the
// deep-water gravity-wave dispersion relation instead of one scrolling tile.
fn wave_field(position: vec2<f32>, seconds: f32, strength: f32) -> vec3<f32> {
    let bands = array<vec4<f32>, 8>(
        vec4<f32>(0.94, 0.34, 18.0, 0.035),
        vec4<f32>(0.72, -0.69, 11.0, 0.026),
        vec4<f32>(0.25, 0.97, 5.3, 0.021),
        vec4<f32>(0.86, 0.51, 2.4, 0.016),
        vec4<f32>(-0.42, 0.91, 1.1, 0.014),
        vec4<f32>(0.99, -0.14, 0.48, 0.011),
        vec4<f32>(0.55, 0.84, 0.21, 0.009),
        vec4<f32>(-0.81, 0.59, 0.09, 0.006),
    );
    let footprint = max(length(dpdx(position)), length(dpdy(position)));
    var gradient = vec2<f32>(0.0);
    var unresolved_variance = 0.0;
    for (var i = 0u; i < 8u; i += 1u) {
        let band = bands[i];
        let direction = normalize(band.xy);
        let k = 6.2831853 / band.z;
        let phase = dot(position, direction) * k - seconds * sqrt(9.81 * k) + f32(i) * 2.399963;
        // Remove subpixel oscillation and carry its slope variance into the BRDF
        // rather than turning distant water into glittering aliasing.
        let visible = 1.0 - smoothstep(0.25, 0.75, footprint / band.z);
        let slope = band.w * strength;
        gradient += direction * cos(phase) * slope * visible;
        unresolved_variance += 0.5 * slope * slope * (1.0 - visible * visible);
    }
    return vec3<f32>(gradient, unresolved_variance);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr = pbr_input_from_standard_material(in, is_front);
    let waves = wave_field(in.world_position.xz, water.parameters.x, water.parameters.y);
    pbr.N = normalize(vec3<f32>(-waves.x, 1.0, -waves.y));
    let base = pbr.material.perceptual_roughness;
    pbr.material.perceptual_roughness = clamp(pow(base * base * base * base + waves.z, 0.25), 0.035, 0.5);
    // PBR supplies ordinary direct lighting; captured radiance uses its own
    // microfacet Fresnel/visibility weight. Display processing happens once.
    var out: FragmentOutput;
    let fresnel = 0.02037 + (1.0 - 0.02037) * pow(1.0 - clamp(dot(pbr.N, pbr.V), 0.0, 1.0), 5.0);
    if water.parameters.z > 0.5 {
        let diffuse = receiver_diffuse(in.world_position.xyz, pbr.N, in.position.xy) * pbr.material.base_color.rgb;
        let specular = reflection(pbr.N, pbr.V, pbr.material.perceptual_roughness);
        out.color = vec4<f32>(diffuse + specular + water.sky_radiance.rgb * fresnel, 1.0);
    } else {
        out.color = apply_pbr_lighting(pbr);
        out.color = vec4<f32>(out.color.rgb + water.sky_radiance.rgb * fresnel, out.color.a);
    }
    out.color = main_pass_post_lighting_processing(pbr, out.color);
    return out;
}
