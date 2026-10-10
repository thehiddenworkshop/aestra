//! Opt-in clustered point irradiance for semantic materials. No new light pool/readback.

pub(super) fn compose(wgsl: &str, enabled: bool) -> String {
    super::shader_composition::compose_material(
        wgsl,
        enabled,
        super::shader_composition::Dialect::Bevy019,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_callback_composes_only_opt_in_and_retains_neutral_2d_branch() {
        let program = aestra_core::material::MaterialProgram::load_ron(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets/test/materials/fireworks_lit_smoke.aestra.material.ron"),
        )
        .unwrap();
        let ir = aestra_compiler::MaterialCompiler.compile(&program).unwrap();
        let compiled = aestra_gpu::material::MaterialShaderCompiler
            .compile(
                &ir,
                &aestra_gpu::material::MaterialBackendCapabilities::portable_minimum(),
            )
            .unwrap();
        let source = compose(&compiled.shader.wgsl, true);
        assert!(source.contains("#ifdef AESTRA_SCENE_POINT_LIGHTING"));
        assert!(source.contains("aestra_bevy_point_irradiance(world, pixel)"));
        assert!(source.contains("#else\n    return vec3<f32>(0.0);"));
        assert!(source.contains("#ifndef AESTRA_SCENE_POINT_LIGHTING\n@group(0) @binding(0)\nvar<uniform> view: View;\n#endif"));
        assert!(source.contains("#import bevy_pbr::mesh_view_bindings::view"));
        assert_eq!(compose(&compiled.shader.wgsl, false), compiled.shader.wgsl);
    }

    #[test]
    fn real_point_helper_validates_with_cluster_contract() {
        let helper = include_str!("material_lighting.wgsl")
            .replace("aestra_sprite_clusters::", "clusters_")
            .replace("aestra_sprite_scene::", "scene_");
        let source = format!(
            r#"
struct View {{ view_from_world: mat4x4<f32>, clip_from_view: mat4x4<f32> }}
struct Light {{ position_radius: vec4<f32>, color_inverse_square_range: vec4<f32>, range: f32 }}
struct Lights {{ data: array<Light, 64> }}
struct Indices {{ first_point_light_index_offset: u32, first_spot_light_index_offset: u32 }}
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> scene_clustered_lights: Lights;
fn clusters_view_fragment_cluster_index(pixel: vec2<f32>, z: f32, ortho: bool) -> u32 {{ return 0u; }}
fn clusters_unpack_clusterable_object_index_ranges(index: u32) -> Indices {{ return Indices(0u, 1u); }}
fn clusters_get_clusterable_object_id(index: u32) -> u32 {{ return index; }}
{helper}
@fragment fn main(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {{
    return vec4<f32>(aestra_bevy_point_irradiance(vec3<f32>(0.5), pixel.xy), 1.0);
}}
"#
        );
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap();
        assert!(helper.contains("min(32u,"));
        assert!(helper.contains("light.range <= 0.0"));
    }
}
