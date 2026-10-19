//! Actual viewport-pane scene shared by shipping and candidate Bevy.
use bevy::prelude::*;

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
