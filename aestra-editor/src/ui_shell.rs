use crate::theme;
use bevy::{
    feathers::controls::{
        ButtonVariant, FeathersButton, FeathersCheckbox, FeathersMenu, FeathersMenuButton,
        FeathersMenuDivider, FeathersMenuItem, FeathersMenuPopup, FeathersNumberInput,
        FeathersScrollbar, FeathersToolButton, NumberFormat,
    },
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
        BackgroundColor(theme::APP_BG)
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
        BackgroundColor(theme::APP_BG)
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
        BackgroundColor(theme::VIEWPORT_FRAME)
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

pub(crate) fn feathers_vertical_scrollbar(target: Entity) -> impl Scene {
    bsn! {
        @FeathersScrollbar {
            @target: target,
            @orientation: ControlOrientation::Vertical,
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
