//! Delayed, window-aware editor tooltips.

use crate::theme;
use bevy::{
    picking::events::{DragStart, Over, Pointer},
    prelude::*,
    ui::RelativeCursorPosition,
    ui_widgets::popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide},
};
use std::time::{Duration, Instant};

pub(crate) const DEFAULT_TOOLTIP_DELAY: Duration = Duration::from_millis(650);

/// Resolved editor-facing tooltip content.
///
/// Strings are localized by the call site before this component is inserted. Keeping localization
/// outside the widget lets the same tooltip render asset names, shortcuts, and generated text too.
#[derive(Component, Clone, Debug)]
#[require(RelativeCursorPosition)]
pub(crate) struct EditorTooltip {
    title: Option<String>,
    description: String,
    shortcut: Option<String>,
    footer: Option<String>,
    delay: Duration,
}

impl EditorTooltip {
    pub(crate) fn description(description: impl Into<String>) -> Self {
        Self {
            title: None,
            description: description.into(),
            shortcut: None,
            footer: None,
            delay: DEFAULT_TOOLTIP_DELAY,
        }
    }

    pub(crate) fn titled(title: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            title: Some(title.into()),
            ..Self::description(description)
        }
    }

    pub(crate) fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    #[allow(dead_code)] // Part of the shared tooltip contract; no current surface needs a footer.
    pub(crate) fn with_footer(mut self, footer: impl Into<String>) -> Self {
        self.footer = Some(footer.into());
        self
    }

    #[allow(dead_code)] // The default is currently consistent across every editor surface.
    pub(crate) fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub(crate) fn accessible_label(&self) -> String {
        [
            self.title.as_deref(),
            Some(self.description.as_str()),
            self.shortcut.as_deref(),
            self.footer.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(". ")
    }
}

#[derive(Component)]
pub(crate) struct TooltipPopup;

#[derive(Resource, Default)]
pub(crate) struct TooltipState {
    target: Option<Entity>,
    hovered_at: Option<Instant>,
    popup: Option<Entity>,
}

pub(crate) fn begin_tooltip(
    over: On<Pointer<Over>>,
    tooltips: Query<(), With<EditorTooltip>>,
    mut state: ResMut<TooltipState>,
    mut commands: Commands,
) {
    if !tooltips.contains(over.entity) || state.target == Some(over.entity) {
        return;
    }
    clear_popup(&mut commands, &mut state);
    state.target = Some(over.entity);
    state.hovered_at = Some(Instant::now());
}

/// Dragging starts an edit or layout operation, so stale hover help must not obscure it.
pub(crate) fn dismiss_tooltip_on_drag(
    _drag: On<Pointer<DragStart>>,
    mut state: ResMut<TooltipState>,
    mut commands: Commands,
) {
    clear_tooltip(&mut commands, &mut state);
}

pub(crate) fn update_tooltip(
    mut commands: Commands,
    mut state: ResMut<TooltipState>,
    tooltips: Query<(&EditorTooltip, &RelativeCursorPosition)>,
    popups: Query<(), With<TooltipPopup>>,
) {
    if state.popup.is_some_and(|popup| !popups.contains(popup)) {
        state.popup = None;
    }
    let Some(target) = state.target else {
        return;
    };
    let Ok((tooltip, cursor)) = tooltips.get(target) else {
        clear_tooltip(&mut commands, &mut state);
        return;
    };
    if !cursor.cursor_over() {
        clear_tooltip(&mut commands, &mut state);
        return;
    }
    if state.popup.is_some()
        || state
            .hovered_at
            .is_none_or(|started| started.elapsed() < tooltip.delay)
    {
        return;
    }

    let content = tooltip.clone();
    let accessible_label = content.accessible_label();
    let mut popup = None;
    commands.entity(target).with_children(|target| {
        popup = Some(
            target
                .spawn((
                    TooltipPopup,
                    Popover {
                        positions: vec![
                            PopoverPlacement {
                                side: PopoverSide::Left,
                                align: PopoverAlign::Center,
                                gap: 8.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Right,
                                align: PopoverAlign::Center,
                                gap: 8.0,
                            },
                            PopoverPlacement {
                                side: PopoverSide::Bottom,
                                align: PopoverAlign::Start,
                                gap: 6.0,
                            },
                        ],
                        window_margin: 10.0,
                    },
                    OverrideClip,
                    GlobalZIndex(300),
                    Pickable::IGNORE,
                    AccessibleLabel(accessible_label),
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(280.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(5.0),
                        padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                    BorderColor::all(theme::BORDER_BRIGHT),
                    BoxShadow::new(
                        Color::srgba(0.0, 0.0, 0.0, 0.65),
                        Val::Px(0.0),
                        Val::Px(2.0),
                        Val::Px(3.0),
                        Val::Px(5.0),
                    ),
                ))
                .with_children(|popup| {
                    if content.title.is_some() || content.shortcut.is_some() {
                        popup
                            .spawn(Node {
                                width: Val::Percent(100.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::SpaceBetween,
                                column_gap: Val::Px(10.0),
                                ..default()
                            })
                            .with_children(|header| {
                                if let Some(title) = content.title.as_ref() {
                                    header.spawn(tooltip_text(title, 11.0, theme::TEXT));
                                }
                                if let Some(shortcut) = content.shortcut.as_ref() {
                                    header.spawn(tooltip_text(shortcut, 9.0, theme::TEXT_MUTED));
                                }
                            });
                    }
                    popup.spawn(tooltip_text(&content.description, 10.0, theme::TEXT));
                    if let Some(footer) = content.footer.as_ref() {
                        popup
                            .spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    padding: UiRect::top(Val::Px(5.0)),
                                    border: UiRect::top(Val::Px(1.0)),
                                    ..default()
                                },
                                BorderColor::all(theme::BORDER),
                            ))
                            .with_child(tooltip_text(footer, 9.0, theme::TEXT_MUTED));
                    }
                })
                .id(),
        );
    });
    state.popup = popup;
}

fn tooltip_text(text: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        Pickable::IGNORE,
    )
}

fn clear_popup(commands: &mut Commands, state: &mut TooltipState) {
    if let Some(popup) = state.popup.take() {
        commands.entity(popup).despawn();
    }
}

fn clear_tooltip(commands: &mut Commands, state: &mut TooltipState) {
    clear_popup(commands, state);
    state.target = None;
    state.hovered_at = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_defaults_to_delayed_description() {
        let tooltip = EditorTooltip::description("Particle lifetime")
            .with_shortcut("F")
            .with_footer("Hold Shift for precision");

        assert_eq!(tooltip.title, None);
        assert_eq!(tooltip.description, "Particle lifetime");
        assert_eq!(tooltip.shortcut.as_deref(), Some("F"));
        assert_eq!(tooltip.footer.as_deref(), Some("Hold Shift for precision"));
        assert_eq!(tooltip.delay, DEFAULT_TOOLTIP_DELAY);
    }

    #[test]
    fn tooltip_automatically_tracks_relative_cursor_position() {
        let mut world = World::new();
        let entity = world
            .spawn(EditorTooltip::titled("Lifetime", "Particle lifetime"))
            .id();

        assert!(world.entity(entity).contains::<RelativeCursorPosition>());
    }

    #[test]
    fn tooltip_delay_can_be_overridden() {
        let delay = Duration::from_millis(1200);
        let tooltip = EditorTooltip::description("Slow help").with_delay(delay);

        assert_eq!(tooltip.delay, delay);
    }
}
