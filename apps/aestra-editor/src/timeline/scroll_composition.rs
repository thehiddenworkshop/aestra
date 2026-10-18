//! Actual timeline scrollbar gutters and retained Feathers scroll controls.
use super::{
    gestures::{stop_timeline_control_drag, stop_timeline_control_press},
    interaction_state::*,
    state::TimelineState,
};
use crate::feathers::scroll::{spawn_horizontal_scrollbar, spawn_vertical_scrollbar};
use crate::{EditorSession, theme};
use bevy::prelude::*;

pub(super) fn spawn_horizontal_gutter(
    tracks: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &TimelineState,
) {
    tracks
        .spawn((
            TimelineHorizontalScrollbarGutter,
            Node {
                display: Display::None,
                width: Val::Percent(100.0),
                height: Val::Px(15.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Stretch,
                padding: UiRect {
                    top: Val::Px(4.0),
                    left: Val::Px(6.0),
                    right: Val::Px(6.0),
                    ..default()
                },
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            BorderColor::all(theme::BORDER),
        ))
        .observe(stop_timeline_control_press)
        .observe(stop_timeline_control_drag)
        .with_children(|gutter| {
            let scroll_target = gutter
                .spawn((
                    TimelineHorizontalScrollViewport,
                    ScrollPosition::default(),
                    Pickable::IGNORE,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        right: Val::Px(0.0),
                        top: Val::Px(0.0),
                        bottom: Val::Px(0.0),
                        overflow: Overflow::scroll_x(),
                        scrollbar_width: 0.0,
                        ..default()
                    },
                ))
                .with_child((
                    TimelineHorizontalScrollContent,
                    Pickable::IGNORE,
                    Node {
                        width: Val::Percent(
                            (session.playback_duration().max(0.05) / state.view.span().max(0.001)
                                * 100.0)
                                .clamp(100.0, 100_000.0),
                        ),
                        height: Val::Px(1.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ))
                .id();
            let scrollbar = spawn_horizontal_scrollbar(gutter, scroll_target);
            gutter
                .commands()
                .entity(scrollbar)
                .observe(stop_timeline_control_press)
                .observe(stop_timeline_control_drag);
        });
}

pub(super) fn spawn_vertical_gutter(body: &mut ChildSpawnerCommands, target: Entity) {
    body.spawn((
        TimelineVerticalScrollbarGutter,
        Node {
            display: Display::None,
            width: Val::Px(15.0),
            height: Val::Auto,
            flex_shrink: 0.0,
            align_self: AlignSelf::Stretch,
            align_items: AlignItems::Stretch,
            margin: UiRect {
                top: Val::Px(25.0),
                bottom: Val::Px(15.0),
                ..default()
            },
            padding: UiRect::left(Val::Px(4.0)),
            border: UiRect::left(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(theme::PANEL_DARK),
        BorderColor::all(theme::BORDER),
    ))
    .with_children(|gutter| {
        let scrollbar = spawn_vertical_scrollbar(gutter, target);
        gutter.commands().entity(scrollbar).insert(Node {
            width: Val::Px(10.0),
            height: Val::Percent(100.0),
            display: Display::None,
            padding: UiRect::horizontal(Val::Px(3.0)),
            ..default()
        });
    });
}

#[cfg(test)]
#[path = "scroll_composition/tests.rs"]
mod tests;
