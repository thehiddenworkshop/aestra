// Bevy 0.19 clustered point lights, sampled at the march point's depth, not the box's back face.
var<private> aestra_volume_world_from_grid: mat4x4<f32>;

fn aestra_volume_scene_lighting(uvw: vec3<f32>, pixel: vec2<f32>, max_lights: u32) -> vec3<f32> {
    if (max_lights == 0u) { return vec3<f32>(0.0); }
    let world = aestra_volume_world_from_grid * vec4<f32>(uvw - vec3<f32>(0.5), 1.0);
    let view_z = (view.view_from_world * world).z;
    if (view_z >= 0.0) { return vec3<f32>(0.0); }
    let cluster = volume_clusters::view_fragment_cluster_index(pixel, view_z, view.clip_from_view[3].w == 1.0);
    let indices = volume_clusters::unpack_clusterable_object_index_ranges(cluster);
    let count = min(min(max_lights, 32u), indices.first_spot_light_index_offset - indices.first_point_light_index_offset);
    var illumination = vec3<f32>(0.0);
    for (var offset = 0u; offset < count; offset += 1u) {
        let id = volume_clusters::get_clusterable_object_id(indices.first_point_light_index_offset + offset);
        let light = volume_scene::clustered_lights.data[id];
        // Retired/zero-range native GPU slots must stay neutral and cannot consume a singular
        // inverse-range calculation. They still count towards the fixed visited-entry budget.
        if (light.range <= 0.0 || all(light.color_inverse_square_range.xyz <= vec3<f32>(0.0))) { continue; }
        let delta = light.position_radius.xyz - world.xyz;
        let distance_squared = dot(delta, delta);
        // Same smooth range falloff as Bevy's PBR point lights. A finite source-radius floor
        // avoids a singularity inside a light; no normals/BRDF for a participating medium.
        let factor = distance_squared * light.color_inverse_square_range.w;
        let range_falloff = max(1.0 - factor * factor, 0.0);
        let attenuation = range_falloff * range_falloff / max(distance_squared, max(light.position_radius.w * light.position_radius.w, 0.0001));
        illumination += max(light.color_inverse_square_range.xyz, vec3<f32>(0.0)) * attenuation;
    }
    // Isotropic phase 1/(4*pi). Dynamic lights have no volume self/opaque shadows in this slice.
    return illumination * 0.0795774715;
}
