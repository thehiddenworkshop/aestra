//! Native semantic-material shader boundary.
pub(super) fn compose(wgsl: &str, lighting: bool) -> String {
    super::shader_composition::compose_material(wgsl, lighting)
}
pub(super) fn shader(source: String, path: String) -> bevy::shader::Shader {
    bevy::shader::Shader::from_wesl(source, path)
}
