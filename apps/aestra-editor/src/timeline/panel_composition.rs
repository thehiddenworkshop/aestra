//! Actual timeline panel composition, assembled from the shared production controls.
use super::{
    actions::ChoreographyAction,
    cue_composition::{spawn_choreography_event_lane, spawn_ruler},
    drop_execution::{
        drop_project_effect_on_timeline, drop_project_effect_on_track_headers,
        hide_invalid_timeline_drop_feedback, show_invalid_timeline_drop_feedback,
    },
    gestures::{seek_timeline_on_drag, seek_timeline_on_press},
    host_motion,
    interaction_state::*,
    presentation::timeline_vertical_content_height,
    scroll_composition::{spawn_horizontal_gutter, spawn_vertical_gutter},
    state::TimelineState,
    timeline_pointer::TimelineButton as Button,
    toolbar_composition::{
        describe_timeline_control, spawn_timeline_toolbar, timeline_effect_heading,
    },
    track_composition::{spawn_choreography_headers, spawn_choreography_rows},
};
use crate::feathers::{button::spawn_tool_button as mini_button, list_row::KeyboardNavigableList};
use crate::{
    CurvesState, EditorModuleRegistry, EditorNativeControl, Localizer, ProjectEffectCatalog,
    session::EditorSession, theme,
};
use bevy::{
    input_focus::tab_navigation::TabIndex,
    prelude::*,
    ui::RelativeCursorPosition,
    ui_widgets::{ListBox, ScrollArea},
};

#[allow(
    clippy::too_many_arguments,
    reason = "Production panel construction retains its explicit resource contract"
)]
pub(crate) fn spawn_timeline(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &TimelineState,
    catalog: &ProjectEffectCatalog,
    registry: &EditorModuleRegistry,
    curves: &CurvesState,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    let vertical_content_height = timeline_vertical_content_height(&session.effect, state, catalog);
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|timeline| {
            spawn_timeline_toolbar(timeline, session, state, localizer, asset_server);
            timeline
                .spawn((
                    TimelineBody,
                    Node {
                        flex_grow: 1.0,
                        width: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Row,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                ))
                .with_children(|body| {
                    body.spawn((
                        TimelineHeaderColumn,
                        Node {
                            width: Val::Px(244.0),
                            min_width: Val::Px(0.0),
                            height: Val::Percent(100.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            border: UiRect::right(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(theme::PANEL_DARK),
                        BorderColor::all(theme::BORDER),
                    ))
                    .with_children(|labels| {
                        labels
                            .spawn(Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(25.0),
                                flex_shrink: 0.0,
                                padding: UiRect::horizontal(Val::Px(8.0)),
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(6.0),
                                ..default()
                            })
                            .with_children(|toolbar| {
                                toolbar.spawn((
                                    Text::new(timeline_effect_heading(
                                        localizer,
                                        &session.effect.name,
                                    )),
                                    TimelineEffectHeading,
                                    TextFont {
                                        font_size: FontSize::Px(9.0),
                                        ..default()
                                    },
                                    TextColor(theme::TEXT_FAINT),
                                    TextLayout::no_wrap(),
                                    Node {
                                        min_width: Val::Px(0.0),
                                        flex_shrink: 1.0,
                                        overflow: Overflow::clip(),
                                        ..default()
                                    },
                                    Pickable::IGNORE,
                                ));
                                toolbar.spawn(Node {
                                    flex_grow: 1.0,
                                    ..default()
                                });
                                let add = mini_button(toolbar, "+", ChoreographyAction::AddEmitter);
                                describe_timeline_control(
                                    toolbar,
                                    add,
                                    localizer.text("edit-add-emitter"),
                                );
                            });
                        labels
                            .spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(28.0),
                                    flex_shrink: 0.0,
                                    padding: UiRect::horizontal(Val::Px(8.0)),
                                    align_items: AlignItems::Center,
                                    border: UiRect::top(Val::Px(1.0)),
                                    ..default()
                                },
                                BorderColor::all(theme::BORDER),
                            ))
                            .with_child((
                                Text::new(localizer.text("timeline-events-lane")),
                                TextFont {
                                    font_size: FontSize::Px(9.0),
                                    ..default()
                                },
                                TextColor(theme::TEXT_MUTED),
                                Pickable::IGNORE,
                            ));
                        host_motion::spawn_header(labels);
                        labels
                            .spawn((
                                TimelineVerticalPane::Headers,
                                TimelineAssetDropTarget,
                                RelativeCursorPosition::default(),
                                ScrollPosition(Vec2::new(0.0, state.vertical_scroll)),
                                ScrollArea,
                                ListBox,
                                KeyboardNavigableList,
                                TabIndex(0),
                                AccessibleLabel(localizer.text("timeline-emitters")),
                                Node {
                                    flex_grow: 1.0,
                                    min_height: Val::Px(0.0),
                                    width: Val::Percent(100.0),
                                    flex_direction: FlexDirection::Column,
                                    align_content: AlignContent::Start,
                                    overflow: Overflow::scroll_y(),
                                    scrollbar_width: 0.0,
                                    ..default()
                                },
                            ))
                            .observe(show_invalid_timeline_drop_feedback)
                            .observe(hide_invalid_timeline_drop_feedback)
                            .observe(drop_project_effect_on_track_headers)
                            .with_children(|viewport| {
                                viewport
                                    .spawn((
                                        TimelineVerticalContent,
                                        Node {
                                            width: Val::Percent(100.0),
                                            min_height: Val::Px(vertical_content_height),
                                            flex_shrink: 0.0,
                                            display: Display::Grid,
                                            grid_template_columns: vec![GridTrack::flex(1.0)],
                                            grid_auto_rows: vec![GridTrack::auto()],
                                            align_content: AlignContent::Start,
                                            ..default()
                                        },
                                    ))
                                    .with_children(|headers| {
                                        spawn_choreography_headers(
                                            headers,
                                            session,
                                            state,
                                            catalog,
                                            registry,
                                            localizer,
                                            asset_server,
                                        );
                                    });
                            });
                        labels.spawn((
                            TimelineHorizontalGutter,
                            Node {
                                display: if state.view.span()
                                    < session.playback_duration().max(0.05) * 0.999
                                {
                                    Display::Flex
                                } else {
                                    Display::None
                                },
                                width: Val::Percent(100.0),
                                height: Val::Px(15.0),
                                flex_shrink: 0.0,
                                border: UiRect::top(Val::Px(1.0)),
                                ..default()
                            },
                            BackgroundColor(theme::PANEL_DARK),
                            BorderColor::all(theme::BORDER),
                        ));
                    });
                    let mut vertical_scroll_target = None;
                    body.spawn((
                        Button,
                        EditorNativeControl,
                        TimelineCanvas,
                        AccessibleLabel(localizer.text("timeline-canvas-accessible")),
                        RelativeCursorPosition::default(),
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            height: Val::Percent(100.0),
                            min_height: Val::Px(0.0),
                            position_type: PositionType::Relative,
                            padding: UiRect::top(Val::Px(81.0)),
                            flex_direction: FlexDirection::Column,
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(theme::TIMELINE_BG),
                    ))
                    .observe(seek_timeline_on_press)
                    .observe(seek_timeline_on_drag)
                    .observe(show_invalid_timeline_drop_feedback)
                    .observe(hide_invalid_timeline_drop_feedback)
                    .observe(drop_project_effect_on_timeline)
                    .with_children(|tracks| {
                        spawn_ruler(tracks, session, localizer);
                        spawn_choreography_event_lane(tracks, session, localizer);
                        host_motion::spawn_lane(tracks, session);
                        vertical_scroll_target = Some(
                            tracks
                                .spawn((
                                    TimelineVerticalPane::Tracks,
                                    ScrollPosition(Vec2::new(0.0, state.vertical_scroll)),
                                    Node {
                                        width: Val::Percent(100.0),
                                        flex_grow: 1.0,
                                        min_height: Val::Px(0.0),
                                        flex_direction: FlexDirection::Column,
                                        align_content: AlignContent::Start,
                                        overflow: Overflow::scroll_y(),
                                        scrollbar_width: 0.0,
                                        ..default()
                                    },
                                ))
                                .with_children(|viewport| {
                                    viewport
                                        .spawn((
                                            TimelineVerticalContent,
                                            Node {
                                                width: Val::Percent(100.0),
                                                min_height: Val::Px(vertical_content_height),
                                                flex_shrink: 0.0,
                                                display: Display::Grid,
                                                grid_template_columns: vec![GridTrack::flex(1.0)],
                                                grid_auto_rows: vec![GridTrack::auto()],
                                                align_content: AlignContent::Start,
                                                ..default()
                                            },
                                        ))
                                        .with_children(|rows| {
                                            spawn_choreography_rows(
                                                rows, session, state, catalog, registry, curves,
                                                localizer,
                                            );
                                        });
                                })
                                .id(),
                        );
                        tracks.spawn((
                            TimelineSnapGuide,
                            Node {
                                display: Display::None,
                                position_type: PositionType::Absolute,
                                left: Val::Percent(0.0),
                                top: Val::Px(0.0),
                                width: Val::Px(1.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(theme::ACCENT),
                            Pickable::IGNORE,
                            ZIndex(3),
                        ));
                        tracks.spawn((
                            Playhead,
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Percent(0.0),
                                top: Val::Px(0.0),
                                width: Val::Px(1.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(theme::PLAYHEAD),
                            Pickable::IGNORE,
                            ZIndex(2),
                        ));
                        tracks
                            .spawn((
                                TimelineEffectDropPreview,
                                Node {
                                    display: Display::None,
                                    position_type: PositionType::Absolute,
                                    left: Val::Percent(0.0),
                                    top: Val::Px(30.0),
                                    width: Val::Percent(1.0),
                                    height: Val::Px(23.0),
                                    padding: UiRect::horizontal(Val::Px(8.0)),
                                    align_items: AlignItems::Center,
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(4.0)),
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                                BackgroundColor(theme::ACCENT.with_alpha(0.32)),
                                BorderColor::all(theme::ACCENT),
                                ZIndex(4),
                                Pickable::IGNORE,
                            ))
                            .with_child((
                                TimelineEffectDropPreviewLabel,
                                Text::new(""),
                                TextFont {
                                    font_size: FontSize::Px(9.0),
                                    ..default()
                                },
                                TextColor(theme::TEXT),
                                TextLayout::no_wrap(),
                                Pickable::IGNORE,
                            ));
                        tracks
                            .spawn((
                                TimelineInvalidDropFeedback::default(),
                                AccessibleLabel(localizer.text("timeline-drop-effect-ready-title")),
                                Node {
                                    display: Display::None,
                                    position_type: PositionType::Absolute,
                                    left: Val::Px(10.0),
                                    right: Val::Px(10.0),
                                    top: Val::Px(10.0),
                                    bottom: Val::Px(10.0),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(5.0),
                                    padding: UiRect::all(Val::Px(16.0)),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(4.0)),
                                    ..default()
                                },
                                BackgroundColor(theme::PANEL_DARK.with_alpha(0.94)),
                                BorderColor::all(theme::ACCENT),
                                Pickable::IGNORE,
                                ZIndex(4),
                            ))
                            .with_children(|feedback| {
                                feedback.spawn((
                                    Text::new(localizer.text("timeline-drop-effect-ready-title")),
                                    TimelineDropFeedbackTitle,
                                    TextFont {
                                        font_size: FontSize::Px(12.0),
                                        ..default()
                                    },
                                    TextColor(theme::ACCENT),
                                    TextLayout::justify(Justify::Center),
                                    Pickable::IGNORE,
                                ));
                                feedback.spawn((
                                    Text::new(localizer.text("timeline-drop-effect-ready-message")),
                                    TimelineDropFeedbackMessage,
                                    TextFont {
                                        font_size: FontSize::Px(10.0),
                                        ..default()
                                    },
                                    TextColor(theme::TEXT_MUTED),
                                    TextLayout::justify(Justify::Center),
                                    Pickable::IGNORE,
                                ));
                            });
                        spawn_horizontal_gutter(tracks, session, state);
                    });
                    if let Some(target) = vertical_scroll_target {
                        spawn_vertical_gutter(body, target);
                    }
                });
        });
}

#[cfg(test)]
#[path = "panel_composition/tests.rs"]
mod tests;
