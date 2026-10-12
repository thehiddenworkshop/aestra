// Production widget dependencies shared by the numeric and color fixtures.
#[path = "../../../apps/aestra-editor/src/theme/bevy_020.rs"]
pub mod boundary;
#[path = "../../../apps/aestra-editor/src/theme/palette.rs"]
pub mod palette;
mod theme {
    pub(crate) use super::boundary::feathers_theme;
    pub(crate) use super::palette::*;
}
#[path = "../../../apps/aestra-editor/src/feathers/number_input/arrows.rs"]
mod arrows;
#[path = "../../../apps/aestra-editor/src/feathers/number_input/pointer_020.rs"]
mod pointer;
#[path = "../../../apps/aestra-editor/src/feathers/number_input/scrub.rs"]
mod scrub;
#[path = "../../../apps/aestra-editor/src/feathers/number_input/widget.rs"]
mod widget;
use scrub::{ScrubbableNumber, decimal_places, formatted, replace_number_text, scrub_multiplier};
#[path = "../../../apps/aestra-editor/src/feathers/icon.rs"]
#[allow(
    dead_code,
    unused_imports,
    reason = "Other icon consumers belong to editor panels outside this fixture"
)]
mod icon;
#[path = "../../../apps/aestra-editor/src/localization.rs"]
#[allow(
    dead_code,
    reason = "Locale selection panels are outside this numeric fixture"
)]
mod localization;
#[path = "../../../apps/aestra-editor/src/feathers/scenes/common.rs"]
mod scenes;
#[path = "../../../apps/aestra-editor/src/feathers/text_input.rs"]
mod text_input;
pub(crate) use localization::Localizer;
mod feathers {
    pub(crate) use super::{icon, text_input};
    #[allow(unused_imports, reason = "Only the color fixture uses this numeric facade")]
    pub(crate) mod number_input {
        pub(crate) use crate::scrub::{ScrubbableNumber, rounded};
        pub(crate) use crate::widget::{NumberInputValue, UpdateNumberInput, feathers_scalar_input};
    }
}
// Only the surrounding About panel is out of scope; the real localization
// catalog, resource and systems are used by production numeric decoration.
mod menus {
    #[derive(bevy::prelude::Component)]
    pub(crate) struct AboutDescription;
}
