//! Candidate semantic-material shader boundary; selected at coherent engine switch.
pub(super) fn compose(wgsl: &str, lighting: bool) -> String {
    super::shader_composition::compose_material(
        wgsl,
        lighting,
        super::shader_composition::Dialect::Bevy020,
    )
}
pub(super) fn shader(source: String, path: String) -> bevy::shader::Shader {
    bevy::shader::Shader::from_wesl(source, path)
}
