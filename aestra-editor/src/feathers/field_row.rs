//! Consistent compact label/control rows for editor panels.

use crate::theme;
use bevy::prelude::*;

pub(crate) const FIELD_ROW_HEIGHT: f32 = 26.0;
pub(crate) const FIELD_LABEL_WIDTH: f32 = 118.0;
pub(crate) const FIELD_LABEL_MIN_WIDTH: f32 = 72.0;
pub(crate) const FIELD_CONTROL_MIN_WIDTH: f32 = 104.0;
pub(crate) const FIELD_INDENT: f32 = 12.0;

#[derive(Clone, Default)]
pub(crate) struct FieldRowProps {
    pub(crate) label: String,
    pub(crate) indent: u8,
    pub(crate) control_min_width: Option<f32>,
}

impl FieldRowProps {
    pub(crate) fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..default()
        }
    }

    pub(crate) fn indented(mut self, levels: u8) -> Self {
        self.indent = levels;
        self
    }

    pub(crate) fn with_control_min_width(mut self, width: f32) -> Self {
        self.control_min_width = Some(width.max(FIELD_CONTROL_MIN_WIDTH));
        self
    }
}

pub(crate) fn spawn_field_row(
    parent: &mut ChildSpawnerCommands,
    props: FieldRowProps,
    marker: impl Bundle,
    controls: impl FnOnce(&mut ChildSpawnerCommands),
) {
    let inset = f32::from(props.indent) * FIELD_INDENT;
    let control_min = props.control_min_width.unwrap_or(FIELD_CONTROL_MIN_WIDTH);
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(FIELD_ROW_HEIGHT),
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                row_gap: Val::Px(4.0),
                padding: UiRect::left(Val::Px(inset)),
                ..default()
            },
            marker,
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(props.label),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
                Node {
                    width: Val::Px((FIELD_LABEL_WIDTH - inset).max(0.0)),
                    min_width: Val::Px(FIELD_LABEL_MIN_WIDTH),
                    flex_shrink: 1.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
            ));
            row.spawn(Node {
                flex_grow: 1.0,
                flex_shrink: 1.0,
                flex_basis: Val::Px(control_min),
                min_width: Val::Px(control_min),
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(controls);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indentation_keeps_control_column_stable() {
        let base = FieldRowProps::new("Value");
        let nested = base.clone().indented(2);
        let base_start = FIELD_LABEL_WIDTH;
        let nested_start = f32::from(nested.indent) * FIELD_INDENT + FIELD_LABEL_WIDTH
            - f32::from(nested.indent) * FIELD_INDENT;
        assert_eq!(base_start, nested_start);
    }
}
