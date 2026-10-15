//! Production automation lane/header construction with real control and raster composition.
use super::automation::{AutomationLaneKeys, AutomationLaneProjection};
use super::automation_gestures::{
    add_automation_key_from_graph, apply_automation_graph_geometry, begin_automation_key_drag,
    begin_automation_lane_resize, finish_automation_key_drag, finish_automation_lane_resize,
    move_automation_key_drag, move_automation_lane_resize, select_automation_key,
};
use super::gestures::stop_timeline_control_press;
use super::interaction_state::*;
use super::navigation_cursor::EntityCursor;
use super::timeline_pointer::TimelineButton as Button;
use super::{ChoreographyAction, TimelineState};
use crate::feathers::{
    action_activation::{EditorNativeControl, FeathersActionButton},
    automation_curve,
    icon::{SvgColor, UiSvg, load_svg_icon},
    scenes,
    tooltip::EditorTooltip,
};
use crate::{CurvesState, EditorSession, localization::Localizer, theme};
use aestra_core::Emitter;
use bevy::{prelude::*, ui::RelativeCursorPosition, window::SystemCursorIcon};

pub(super) fn choreography_icon_button(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    icon_path: &'static str,
    label: String,
    action: ChoreographyAction,
) -> Entity {
    let mut button = parent.spawn_empty();
    button.apply_scene(scenes::feathers_tool_button());
    let entity = button.id();
    button
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.clone()),
            EditorTooltip::description(label),
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_child((
            Node {
                width: Val::Px(13.0),
                height: Val::Px(13.0),
                ..default()
            },
            UiSvg(load_svg_icon(asset_server, icon_path)),
            SvgColor(theme::TEXT),
            Pickable::IGNORE,
        ));
    entity
}

pub(super) fn spawn_automation_lane_header(
    parent: &mut ChildSpawnerCommands,
    localizer: &Localizer,
    state: &TimelineState,
    lane: &AutomationLaneProjection,
    grid_row: i16,
    asset_server: &AssetServer,
) {
    parent
        .spawn((
            TimelineAutomationLane,
            lane.id.clone(),
            TimelineChoreographyGridRow(grid_row),
            Node {
                width: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                height: Val::Px(state.automation_lane_height(&lane.id)),
                flex_shrink: 0.0,
                padding: UiRect {
                    left: Val::Px(41.0),
                    right: Val::Px(7.0),
                    ..default()
                },
                align_items: AlignItems::FlexStart,
                column_gap: Val::Px(6.0),
                border: UiRect::bottom(Val::Px(1.0)),
                grid_row: GridPlacement::start(grid_row),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            BorderColor::all(theme::BORDER.with_alpha(0.35)),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(&lane.label),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
                TextLayout::no_wrap(),
                Node {
                    min_width: Val::Px(0.0),
                    flex_grow: 1.0,
                    margin: UiRect::top(Val::Px(10.0)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                EditorTooltip::description(&lane.label),
                Pickable::IGNORE,
            ));
            let add_label = localizer.text("timeline-add-automation-key");
            let add = choreography_icon_button(
                row,
                asset_server,
                "icons/plus.svg",
                add_label.clone(),
                ChoreographyAction::AddAutomationKey(lane.id.clone()),
            );
            row.commands().entity(add).insert((
                EditorTooltip::description(add_label).with_shortcut("Insert"),
                Node {
                    width: Val::Px(22.0),
                    height: Val::Px(22.0),
                    margin: UiRect::top(Val::Px(5.0)),
                    ..default()
                },
            ));
            if let Some(selection) = state
                .selected_automation_key
                .as_ref()
                .filter(|selection| selection.lane == lane.id)
            {
                let remove_label = localizer.text("timeline-delete-automation-key");
                let remove = choreography_icon_button(
                    row,
                    asset_server,
                    "icons/minus.svg",
                    remove_label.clone(),
                    ChoreographyAction::DeleteAutomationKey(selection.clone()),
                );
                row.commands().entity(remove).insert((
                    EditorTooltip::description(remove_label).with_shortcut("Delete"),
                    Node {
                        width: Val::Px(22.0),
                        height: Val::Px(22.0),
                        margin: UiRect::top(Val::Px(5.0)),
                        ..default()
                    },
                ));
            }
        });
}

pub(super) fn spawn_automation_lane_row(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &TimelineState,
    curves: &CurvesState,
    localizer: &Localizer,
    emitter: &Emitter,
    lane: &AutomationLaneProjection,
    grid_row: i16,
) {
    parent
        .spawn((
            TimelineAutomationLane,
            lane.id.clone(),
            TimelineChoreographyGridRow(grid_row),
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(state.automation_lane_height(&lane.id)),
                flex_shrink: 0.0,
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                border: UiRect::bottom(Val::Px(1.0)),
                grid_row: GridPlacement::start(grid_row),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK.with_alpha(0.24)),
            BorderColor::all(theme::BORDER.with_alpha(0.30)),
        ))
        .with_children(|row| {
            let region = state
                .selected_emitter_region
                .filter(|(selected_emitter, _)| *selected_emitter == emitter.id)
                .and_then(|(_, region)| emitter.timeline_region(region))
                .or_else(|| emitter.timeline_regions().into_iter().next())
                .expect("an emitter always projects at least one timeline region");
            let graph_data = lane.keys.graph_data();
            let source_start = region.start_time - region.source_offset;
            let source_duration = emitter.duration;
            let mut graph_node = Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                height: Val::Percent(100.0),
                overflow: Overflow::clip(),
                ..default()
            };
            apply_automation_graph_geometry(
                &mut graph_node,
                source_start,
                source_duration,
                state.view,
            );
            row.spawn((
                TimelineAutomationLaneGraph(lane.id.clone()),
                RelativeCursorPosition::default(),
                graph_node,
                BackgroundColor(theme::ACCENT.with_alpha(0.035)),
            ))
            .observe(add_automation_key_from_graph)
            .with_children(|graph| {
                automation_curve::spawn_automation_curve(graph, &graph_data);
            });
            let curve_selection = curves.selected_key();
            let emitter_selected = session.selection.emitter(&session.effect) == Some(emitter.id);
            for (key, normalized_time) in lane.keys.times().enumerate() {
                let selection = TimelineAutomationKeySelection {
                    lane: lane.id.clone(),
                    key,
                };
                let selected = emitter_selected
                    && curve_selection.is_some_and(|selected| {
                        selected.module == lane.id.module
                            && selected.input == lane.id.input
                            && selected.key == key
                            && lane
                                .id
                                .channel
                                .is_none_or(|channel| curves.selected_vector_channel() == channel)
                    });
                let absolute_time = source_start + normalized_time * source_duration;
                let position = state.view.normalized_time(absolute_time);
                let top = graph_data.key_top_percent(key);
                let visible = (0.0..=1.0).contains(&position);
                let gradient_handle = matches!(&lane.keys, AutomationLaneKeys::Gradient(_));
                let mut control = row.spawn((
                    Button,
                    EditorNativeControl,
                    TimelineAutomationKey,
                    selection.clone(),
                    ChoreographyAction::SelectAutomationKey(selection),
                    AccessibleLabel(format!("{} {:.3}", lane.label, absolute_time)),
                    EntityCursor::System(if gradient_handle {
                        SystemCursorIcon::EwResize
                    } else {
                        SystemCursorIcon::Grab
                    }),
                    ZIndex(2),
                ));
                if gradient_handle {
                    control
                        .insert((
                            Node {
                                display: if visible {
                                    Display::Flex
                                } else {
                                    Display::None
                                },
                                position_type: PositionType::Absolute,
                                left: Val::Percent(position.clamp(0.0, 1.0) * 100.0),
                                top: Val::Px(0.0),
                                width: Val::Px(11.0),
                                height: Val::Percent(100.0),
                                margin: UiRect::left(Val::Px(-5.5)),
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_child((
                            Node {
                                width: Val::Px(if selected { 3.0 } else { 2.0 }),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(if selected {
                                theme::TEXT
                            } else {
                                theme::TEXT_MUTED
                            }),
                            Pickable::IGNORE,
                        ));
                } else {
                    control.insert((
                        Node {
                            display: if visible {
                                Display::Flex
                            } else {
                                Display::None
                            },
                            position_type: PositionType::Absolute,
                            left: Val::Percent(position.clamp(0.0, 1.0) * 100.0),
                            top: Val::Percent(top),
                            width: Val::Px(11.0),
                            height: Val::Px(11.0),
                            margin: UiRect {
                                left: Val::Px(-5.5),
                                top: Val::Px(-5.5),
                                ..default()
                            },
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(if selected {
                            theme::ACCENT
                        } else {
                            theme::TEXT_MUTED
                        }),
                        BorderColor::all(if selected {
                            theme::TEXT
                        } else {
                            theme::BORDER_BRIGHT
                        }),
                    ));
                }
                control
                    .observe(select_automation_key)
                    .observe(begin_automation_key_drag)
                    .observe(move_automation_key_drag)
                    .observe(finish_automation_key_drag)
                    .observe(stop_timeline_control_press);
            }
            row.spawn((
                Button,
                EditorNativeControl,
                TimelineAutomationLaneResizeHandle,
                lane.id.clone(),
                AccessibleLabel(localizer.text("timeline-resize-automation-lane")),
                EntityCursor::System(SystemCursorIcon::NsResize),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    height: Val::Px(7.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(Color::NONE),
                ZIndex(4),
            ))
            .observe(begin_automation_lane_resize)
            .observe(move_automation_lane_resize)
            .observe(finish_automation_lane_resize)
            .observe(stop_timeline_control_press)
            .with_child((
                Node {
                    width: Val::Px(34.0),
                    height: Val::Px(2.0),
                    border_radius: BorderRadius::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(theme::BORDER_BRIGHT.with_alpha(0.7)),
                Pickable::IGNORE,
            ));
        });
}

#[cfg(test)]
#[path = "lane_composition/tests.rs"]
mod tests;
