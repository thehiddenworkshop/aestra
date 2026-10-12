//! Scene wrappers whose composition is shared by shipping and candidate Bevy.
use bevy::{
    feathers::controls::{
        ButtonVariant, ColorChannel, FeathersButton, FeathersCheckbox, FeathersColorPlane,
        FeathersColorSlider, FeathersMenu, FeathersMenuButton, FeathersMenuDivider,
        FeathersMenuItem, FeathersMenuPopup, FeathersScrollbar, FeathersSlider, FeathersTextInput,
        FeathersTextInputContainer, FeathersToolButton,
    },
    prelude::*,
    ui_widgets::{ControlOrientation, SliderValue},
};

pub(crate) fn feathers_button() -> impl Scene {
    bsn! {
        @FeathersButton
    }
}

pub(crate) fn feathers_primary_button() -> impl Scene {
    bsn! {
        @FeathersButton {
            @variant: ButtonVariant::Primary,
        }
    }
}

pub(crate) fn feathers_plain_button() -> impl Scene {
    bsn! {
        @FeathersButton {
            @variant: ButtonVariant::Plain,
        }
    }
}

pub(crate) fn feathers_tool_button() -> impl Scene {
    bsn! {
        @FeathersToolButton
    }
}

pub(crate) fn feathers_checkbox() -> impl Scene {
    bsn! {
        @FeathersCheckbox
    }
}

pub(crate) fn feathers_text_input_container() -> impl Scene {
    bsn! {
        @FeathersTextInputContainer
    }
}

pub(crate) fn feathers_text_input() -> impl Scene {
    bsn! {
        @FeathersTextInput
    }
}

pub(crate) fn feathers_slider(value: f32, min: f32, max: f32) -> impl Scene {
    bsn! {
        @FeathersSlider {
            @min: min,
            @max: max,
        }
        SliderValue(value)
    }
}

pub(crate) fn feathers_color_slider(value: f32, channel: ColorChannel) -> impl Scene {
    bsn! {
        @FeathersColorSlider {
            @value: value,
            @channel: channel,
        }
    }
}

pub(crate) fn feathers_hue_saturation_plane() -> impl Scene {
    bsn! { @FeathersColorPlane::HueSaturation }
}

pub(crate) fn feathers_vertical_scrollbar(target: Entity) -> impl Scene {
    bsn! {
        @FeathersScrollbar {
            @target: target,
            @orientation: ControlOrientation::Vertical,
        }
    }
}

pub(crate) fn feathers_horizontal_scrollbar(target: Entity) -> impl Scene {
    bsn! {
        @FeathersScrollbar {
            @target: target,
            @orientation: ControlOrientation::Horizontal,
        }
    }
}

pub(crate) fn feathers_menu() -> impl Scene {
    bsn! {
        @FeathersMenu
    }
}

pub(crate) fn feathers_menu_button() -> impl Scene {
    bsn! {
        @FeathersMenuButton {
            @arrow: false,
        }
    }
}

pub(crate) fn feathers_menu_popup() -> impl Scene {
    bsn! {
        @FeathersMenuPopup
    }
}

pub(crate) fn feathers_menu_item() -> impl Scene {
    bsn! {
        @FeathersMenuItem
    }
}

pub(crate) fn feathers_menu_divider() -> impl Scene {
    bsn! {
        @FeathersMenuDivider
    }
}
