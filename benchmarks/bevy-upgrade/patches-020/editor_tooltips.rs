//! Production tooltip composition, layout and pointer lifecycle on Bevy 0.20.
#[path = "../../../apps/aestra-editor/src/theme/feathers.rs"]
pub mod boundary;
#[path = "../../../apps/aestra-editor/src/theme/palette.rs"]
pub mod palette;
mod theme {
    pub(crate) use super::palette::*;
}
#[path = "../../../apps/aestra-editor/src/feathers/pointer.rs"]
#[allow(
    dead_code,
    unused_imports,
    reason = "Menu/text pointer exports are tested by editor_text"
)]
mod pointer;
#[path = "../../../apps/aestra-editor/src/feathers/tooltip.rs"]
#[allow(
    dead_code,
    clippy::too_many_arguments,
    reason = "Panel call sites are outside this fixture; system-parameter lint policy matches the editor"
)]
mod tooltip;
