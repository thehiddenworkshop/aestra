//! Shipping semantic-material shader boundary.
pub(super) fn compose(wgsl: &str, lighting: bool) -> String {
    super::material_lighting::compose(wgsl, lighting)
}
pub(super) fn shader(source: String, path: String) -> bevy::shader::Shader {
    bevy::shader::Shader::from_wgsl(source, path)
}
