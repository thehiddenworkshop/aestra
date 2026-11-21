//! Native semantic-material shader boundary.
pub(super) fn compose(wgsl: &str, lighting: bool) -> String {
    use super::shader_composition::{compose_fragment_list, compose_material, compose_pixel_grid};

    compose_pixel_grid(&compose_fragment_list(
        &compose_material(wgsl, lighting),
        true,
    ))
}
pub(super) fn shader(source: String, path: String) -> bevy::shader::Shader {
    bevy::shader::Shader::from_wesl(source, path)
}
