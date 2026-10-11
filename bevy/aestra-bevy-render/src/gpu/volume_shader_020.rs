//! Candidate volume shader language boundary; selected at the coherent engine switch.
pub(super) fn compose(program: &str, entry: &str) -> String {
    super::shader_composition::compose_volume(
        program,
        entry,
        super::shader_composition::Dialect::Bevy020,
    )
}

pub(super) fn shader(source: String, path: String) -> bevy::shader::Shader {
    bevy::shader::Shader::from_wesl(source, path)
}
