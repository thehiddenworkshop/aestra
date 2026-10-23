//! Native volume shader language boundary.
pub(super) fn compose(program: &str, entry: &str) -> String {
    super::shader_composition::compose_volume(program, entry)
}

pub(super) fn shader(source: String, path: String) -> bevy::shader::Shader {
    bevy::shader::Shader::from_wesl(source, path)
}
