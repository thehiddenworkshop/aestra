use crate::theme;
use bevy::prelude::*;

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

pub(crate) fn main_row() -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            flex_grow: 1.0,
            min_height: px(260),
            position_type: PositionType::Relative,
        }
        BackgroundColor(theme::APP_BG)
    }
}

pub(crate) fn side_pane(width: f32, border: UiRect) -> impl Scene {
    bsn! {
        Node {
            width: px(width),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            border: {border},
            flex_shrink: 0.0,
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
            min_width: px(320),
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

pub(crate) fn bottom_workspace(height: f32) -> impl Scene {
    bsn! {
        Node {
            width: percent(100),
            height: px(height),
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            border: {UiRect::top(px(1))},
        }
        BackgroundColor(theme::PANEL)
        BorderColor::all(theme::BORDER_BRIGHT)
    }
}
