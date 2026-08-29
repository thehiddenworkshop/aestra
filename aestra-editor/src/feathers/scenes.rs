use crate::theme;
use bevy::{
    feathers::controls::{
        ButtonVariant, ColorChannel, FeathersButton, FeathersCheckbox, FeathersColorPlane,
        FeathersColorSlider, FeathersMenu, FeathersMenuButton, FeathersMenuDivider,
        FeathersMenuItem, FeathersMenuPopup, FeathersNumberInput, FeathersScrollbar,
        FeathersSlider, FeathersTextInput, FeathersTextInputContainer, FeathersToolButton,
        NumberFormat,
    },
    feathers::theme::ThemeToken,
    prelude::*,
    ui_widgets::ControlOrientation,
};

pub(crate) fn editor_root() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: percent(100),
            display: Display::Grid,
            grid_template_columns: {vec![GridTrack::flex(1.0)]},
            grid_template_rows: {vec![
                GridTrack::px(30.0),
                GridTrack::px(54.0),
                GridTrack::flex(1.0),
                GridTrack::px(24.0),
            ]},
        }
        // The window clear supplies the application background. Keeping this root transparent
        // lets the 3D preview camera show through the viewport dock's transparent cutout.
        BackgroundColor(Color::NONE)
    }
}

pub(crate) fn editor_content() -> impl Scene {
    bsn! {
        Node {
            grid_row: GridPlacement::start(3),
            min_height: px(0),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
        }
        BackgroundColor(Color::NONE)
    }
}

pub(crate) fn dock_pane() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: percent(100),
            min_width: px(0),
            min_height: px(0),
            flex_direction: FlexDirection::Column,
            position_type: PositionType::Relative,
        }
        BackgroundColor(theme::PANEL_DARK)
        BorderColor::all(theme::BORDER)
    }
}

pub(crate) fn viewport_pane() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            height: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            justify_content: JustifyContent::Center,
            row_gap: px(8),
            padding: px(12),
            min_width: px(0),
        }
        BackgroundColor(Color::NONE)
    }
}

pub(crate) fn splitter(vertical: bool) -> impl Scene {
    let width = if vertical { percent(100) } else { px(9) };
    let height = if vertical { px(9) } else { percent(100) };
    bsn! {
        Node {
            width,
            height,
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
        }
        BackgroundColor(theme::SPLITTER_GUTTER)
    }
}

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

pub(crate) fn feathers_integer_input() -> impl Scene {
    bsn! {
        @FeathersNumberInput {
            @number_format: NumberFormat::I32,
        }
    }
}

pub(crate) fn feathers_scalar_input() -> impl Scene {
    bsn! {
        @FeathersNumberInput
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
            @value: value,
            @min: min,
            @max: max,
        }
    }
}

pub(crate) fn feathers_hue_saturation_plane() -> impl Scene {
    bsn! {
        @FeathersColorPlane::HueSaturation
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

pub(crate) fn feathers_labeled_scalar_input(
    label_text: &'static str,
    sigil_color: ThemeToken,
) -> impl Scene {
    bsn! {
        @FeathersNumberInput {
            @label_text: label_text,
            @sigil_color: sigil_color,
        }
    }
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
