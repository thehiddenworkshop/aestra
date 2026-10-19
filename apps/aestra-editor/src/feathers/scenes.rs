use crate::theme;
mod common;
pub(crate) use super::number_input::{
    feathers_integer_input, feathers_labeled_scalar_input, feathers_scalar_input,
};
use bevy::prelude::*;
pub(crate) use common::*;

mod root;
pub(crate) use root::editor_root;
mod viewport;
pub(crate) use viewport::viewport_pane;

pub(crate) fn editor_content_host() -> impl Scene {
    bsn! {
        Node {
            grid_row: GridPlacement::start(3),
            min_height: px(0),
            min_width: px(0),
            position_type: PositionType::Relative,
            flex_direction: FlexDirection::Column,
        }
        BackgroundColor(Color::NONE)
    }
}

pub(crate) fn editor_content() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
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
