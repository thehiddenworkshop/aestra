// Unshadowed isotropic point scattering, evaluated at the billboard fragment (not its center).
// At most 32 visited cluster entries, including retired slots. Not strongest-light selection.
fn aestra_bevy_point_irradiance(world: vec3<f32>, pixel: vec2<f32>) -> vec3<f32> {
    let view_z = (view.view_from_world * vec4<f32>(world, 1.0)).z;
    if (view_z >= 0.0) { return vec3<f32>(0.0); }
    let cluster = aestra_sprite_clusters::view_fragment_cluster_index(pixel, view_z, view.clip_from_view[3].w == 1.0);
    let indices = aestra_sprite_clusters::unpack_clusterable_object_index_ranges(cluster);
    let count = min(32u, indices.first_spot_light_index_offset - indices.first_point_light_index_offset);
    var illumination = vec3<f32>(0.0);
    for (var offset = 0u; offset < count; offset += 1u) {
        let id = aestra_sprite_clusters::get_clusterable_object_id(indices.first_point_light_index_offset + offset);
        let light = aestra_sprite_scene::clustered_lights.data[id];
        if (light.range <= 0.0 || all(light.color_inverse_square_range.xyz <= vec3<f32>(0.0))) { continue; }
        let delta = light.position_radius.xyz - world;
        let square = dot(delta, delta);
        let factor = square * light.color_inverse_square_range.w;
        let falloff = max(1.0 - factor * factor, 0.0);
        let attenuation = falloff * falloff / max(square, max(light.position_radius.w * light.position_radius.w, 0.0001));
        illumination += max(light.color_inverse_square_range.xyz, vec3<f32>(0.0)) * attenuation;
    }
    return illumination * 0.0795774715;
}
