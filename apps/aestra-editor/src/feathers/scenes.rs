use crate::theme;
mod common;
pub(crate) use super::number_input::{
    feathers_integer_input, feathers_labeled_scalar_input, feathers_scalar_input,
};
use bevy::{input_focus::tab_navigation::TabGroup, prelude::*};
pub(crate) use common::*;

pub(crate) fn editor_root() -> impl Scene {
    bsn! {
        TabGroup::new(0)
        Node {
            width: percent(100),
            height: percent(100),
            min_width: px(crate::dock_ui::EDITOR_MIN_SIZE.x),
            min_height: px(crate::dock_ui::EDITOR_MIN_SIZE.y),
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
