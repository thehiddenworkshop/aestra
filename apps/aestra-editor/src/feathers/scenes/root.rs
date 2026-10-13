//! Actual editor root, including its structural tab group and layout floor.
use bevy::{input_focus::tab_navigation::TabGroup, prelude::*};

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
