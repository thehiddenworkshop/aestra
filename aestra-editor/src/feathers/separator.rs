//! Horizontal and vertical separators using Aestra theme tokens.

use crate::theme;
use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct EditorSeparator;

#[derive(Clone, Copy, Default)]
pub(crate) enum SeparatorDirection {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy)]
pub(crate) struct SeparatorProps {
    pub(crate) direction: SeparatorDirection,
    pub(crate) alpha: f32,
}

impl SeparatorProps {
    pub(crate) fn vertical() -> Self {
        Self {
            direction: SeparatorDirection::Vertical,
            alpha: 0.18,
        }
    }

    pub(crate) fn horizontal() -> Self {
        Self {
            direction: SeparatorDirection::Horizontal,
            alpha: 0.18,
        }
    }

    pub(crate) fn with_alpha(mut self, alpha: f32) -> Self {
        self.alpha = alpha.clamp(0.0, 1.0);
        self
    }
}

pub(crate) fn separator(props: SeparatorProps) -> impl Bundle {
    let (width, height, align_self) = match props.direction {
        SeparatorDirection::Vertical => (Val::Px(1.0), Val::Auto, AlignSelf::Stretch),
        SeparatorDirection::Horizontal => (Val::Percent(100.0), Val::Px(1.0), AlignSelf::default()),
    };
    (
        EditorSeparator,
        Node {
            width,
            height,
            align_self,
            ..default()
        },
        BackgroundColor(theme::TEXT_MUTED.with_alpha(props.alpha)),
    )
}
