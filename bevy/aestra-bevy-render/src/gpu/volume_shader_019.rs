//! Shipping volume shader language boundary.
pub(super) fn compose(program: &str, entry: &str) -> String {
    super::shader_composition::compose_volume(
        program,
        entry,
        super::shader_composition::Dialect::Bevy019,
    )
}

pub(super) fn shader(source: String, path: String) -> bevy::shader::Shader {
    bevy::shader::Shader::from_wgsl(source, path)
}
