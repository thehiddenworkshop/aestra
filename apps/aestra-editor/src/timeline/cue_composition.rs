//! Production ruler, marker/event controls and effect insertion spacer composition.
use super::TimelineAction;
use super::clicks::{select_choreography_event, select_timeline_marker};
use super::gestures::{
    begin_choreography_event_drag, begin_timeline_marker_drag, finish_choreography_event_drag,
    finish_timeline_marker_drag, move_choreography_event_drag, move_timeline_marker_drag,
    stop_timeline_control_press,
};
use super::interaction_state::*;
use super::navigation_cursor::EntityCursor;
use super::timeline_pointer::TimelineButton as Button;
use crate::feathers::{action_activation::EditorNativeControl, tooltip::EditorTooltip};
use crate::{EditorSession, localization::Localizer, theme};
use aestra_authoring::SemanticTarget;
use bevy::{prelude::*, ui::RelativeCursorPosition, window::SystemCursorIcon};

pub(super) fn spawn_effect_drop_spacer(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        TimelineEffectDropSpacer,
        RelativeCursorPosition::default(),
        Pickable::IGNORE,
        Node {
            display: Display::None,
            width: Val::Percent(100.0),
            height: Val::Px(31.0),
            flex_shrink: 0.0,
            grid_row: GridPlacement::start(1),
            ..default()
        },
    ));
}

pub(super) fn spawn_ruler(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    for index in 0..32 {
        parent
            .spawn((
                TimelineRulerTick(index),
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    left: Val::Percent(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(1.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(theme::BORDER.with_alpha(0.55)),
                Pickable::IGNORE,
            ))
            .with_child((
                Text::new("0.0"),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(4.0),
                    top: Val::Px(4.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
    }
    for marker in &session.effect.markers {
        let selected = session.selection.primary == SemanticTarget::Marker(marker.id);
        parent
            .spawn((
                Button,
                EditorNativeControl,
                TimelineMarker { marker: marker.id },
                TimelineAction::SelectMarker(marker.id),
                AccessibleLabel(format!(
                    "{} {}",
                    localizer.text("timeline-marker"),
                    marker.name
                )),
                RelativeCursorPosition::default(),
                EntityCursor::System(SystemCursorIcon::Grab),
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    left: Val::Percent(0.0),
                    top: Val::Px(0.0),
                    height: Val::Px(24.0),
                    max_width: Val::Px(120.0),
                    padding: UiRect::axes(Val::Px(5.0), Val::Px(2.0)),
                    align_items: AlignItems::Center,
                    border: UiRect::left(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(if selected {
                    theme::ACCENT_DIM.with_alpha(0.8)
                } else {
                    theme::PANEL_LIGHT.with_alpha(0.9)
                }),
                BorderColor::all(theme::ACCENT),
            ))
            .observe(select_timeline_marker)
            .observe(stop_timeline_control_press)
            .observe(begin_timeline_marker_drag)
            .observe(move_timeline_marker_drag)
            .observe(finish_timeline_marker_drag)
            .with_child((
                Text::new(&marker.name),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(if selected {
                    theme::TEXT
                } else {
                    theme::TEXT_MUTED
                }),
                TextLayout::no_wrap(),
                Pickable::IGNORE,
            ));
    }
}

pub(super) fn spawn_choreography_event_lane(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(25.0),
                height: Val::Px(28.0),
                border: UiRect::vertical(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK.with_alpha(0.72)),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|lane| {
            for event in &session.effect.choreography_events {
                let selected =
                    session.selection.primary == SemanticTarget::ChoreographyEvent(event.id);
                lane.spawn((
                    Button,
                    EditorNativeControl,
                    TimelineChoreographyEvent { event: event.id },
                    TimelineAction::SelectChoreographyEvent(event.id),
                    AccessibleLabel(format!(
                        "{} {}",
                        localizer.text("timeline-event"),
                        event.name
                    )),
                    EditorTooltip::description(format!("{} · {:.3}s", event.name, event.time)),
                    EntityCursor::System(SystemCursorIcon::Grab),
                    Node {
                        position_type: PositionType::Absolute,
                        display: Display::None,
                        left: Val::Percent(0.0),
                        top: Val::Px(4.0),
                        width: Val::Px(18.0),
                        height: Val::Px(18.0),
                        margin: UiRect::left(Val::Px(-9.0)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(Val::Px(if selected { 2.0 } else { 1.0 })),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(if selected {
                        theme::ACCENT
                    } else {
                        theme::ACCENT_DIM
                    }),
                    BorderColor::all(if selected { theme::TEXT } else { theme::ACCENT }),
                    Text::new("◆"),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT),
                ))
                .observe(select_choreography_event)
                .observe(stop_timeline_control_press)
                .observe(begin_choreography_event_drag)
                .observe(move_choreography_event_drag)
                .observe(finish_choreography_event_drag);
            }
        });
}

#[cfg(test)]
#[path = "tests/cue_composition.rs"]
mod tests;
