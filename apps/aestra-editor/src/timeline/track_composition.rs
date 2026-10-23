//! Production choreography header/row assembly shared by both engine graphs.
use super::{
    actions::ChoreographyAction,
    automation::{automation_lane_is_visible, emitter_automation_lanes},
    clicks::{
        open_timeline_region_context_menu, select_timeline_clip, select_timeline_effect_clip,
    },
    clip_gestures::{
        begin_effect_clip_timeline_drag, finish_effect_clip_timeline_drag,
        move_effect_clip_timeline_drag,
    },
    cue_composition::spawn_effect_drop_spacer,
    effect_clip_composition::spawn_effect_clip_track_header,
    emitter_composition::spawn_emitter_track_header,
    gestures::stop_timeline_control_press,
    interaction_state::*,
    lane_composition::{spawn_automation_lane_header, spawn_automation_lane_row},
    navigation_cursor::EntityCursor,
    presentation::{
        apply_timeline_bar_geometry, choreography_grid_row, timeline_boundary_is_visible,
    },
    projections::{ReferencedTrackKind, effect_clip_source_name, referenced_track_projections},
    referenced_effect::{
        spawn_referenced_effect_clip_track_header, spawn_referenced_emitter_track_header,
        spawn_referenced_track_row,
    },
    region_gestures::{
        begin_timeline_clip_drag, finish_timeline_clip_drag, move_timeline_clip_drag,
    },
    state::TimelineState,
    timeline_pointer::TimelineButton as Button,
    track_style::{effect_reference_color, emitter_timing_label, layer_color},
};
use crate::feathers::tooltip::EditorTooltip;
use crate::{
    CurvesState, EditorModuleRegistry, EditorNativeControl, Localizer, ProjectEffectCatalog,
    session::EditorSession, theme,
};
use aestra_authoring::SemanticTarget;
use aestra_core::ChoreographyTrackId;
use bevy::{prelude::*, ui::RelativeCursorPosition, window::SystemCursorIcon};
use fluent_bundle::FluentArgs;

pub(super) fn spawn_choreography_headers(
    headers: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &TimelineState,
    catalog: &ProjectEffectCatalog,
    registry: &EditorModuleRegistry,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    for clip in &session.effect.effect_clips {
        let grid_row = choreography_grid_row(
            &session.effect,
            state,
            catalog,
            ChoreographyTrackId::EffectClip(clip.id),
        );
        let source_name = effect_clip_source_name(catalog, clip.source);
        let source = catalog.cached_effect(clip.source).ok();
        spawn_effect_clip_track_header(
            headers,
            session,
            state,
            localizer,
            clip,
            &source_name,
            source.as_ref().map_or(0, |effect| {
                effect.effect_clips.len() + effect.emitters.len()
            }),
            grid_row,
            asset_server,
        );
        for (index, projection) in referenced_track_projections(catalog, state, clip)
            .iter()
            .enumerate()
        {
            let child_grid_row = grid_row + index as i16 + 1;
            match &projection.kind {
                ReferencedTrackKind::EffectClip {
                    clip,
                    source_name,
                    child_count,
                } => spawn_referenced_effect_clip_track_header(
                    headers,
                    state,
                    localizer,
                    &projection.path,
                    projection.depth,
                    clip,
                    source_name,
                    *child_count,
                    child_grid_row,
                    asset_server,
                ),
                ReferencedTrackKind::Emitter { emitter, .. } => {
                    spawn_referenced_emitter_track_header(
                        headers,
                        state,
                        localizer,
                        &projection.path,
                        projection.depth,
                        emitter,
                        child_grid_row,
                    );
                }
            }
        }
    }
    for (index, emitter) in session.effect.emitters.iter().enumerate() {
        let grid_row = choreography_grid_row(
            &session.effect,
            state,
            catalog,
            ChoreographyTrackId::Emitter(emitter.id),
        );
        let automation_lanes =
            emitter_automation_lanes(&session.effect, emitter, registry, localizer);
        spawn_emitter_track_header(
            headers,
            session,
            state,
            localizer,
            index,
            emitter.id,
            &emitter.name,
            emitter.enabled,
            emitter.display_color,
            grid_row,
            &automation_lanes,
            asset_server,
        );
        if state.expanded_automation_emitters.contains(&emitter.id) {
            for (lane_index, lane) in automation_lanes
                .iter()
                .filter(|lane| automation_lane_is_visible(state, &lane.id))
                .enumerate()
            {
                spawn_automation_lane_header(
                    headers,
                    localizer,
                    state,
                    lane,
                    grid_row + lane_index as i16 + 1,
                    asset_server,
                );
            }
        }
    }
    spawn_effect_drop_spacer(headers);
}

pub(super) fn spawn_choreography_rows(
    rows: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &TimelineState,
    catalog: &ProjectEffectCatalog,
    registry: &EditorModuleRegistry,
    curves: &CurvesState,
    localizer: &Localizer,
) {
    for clip in &session.effect.effect_clips {
        let grid_row = choreography_grid_row(
            &session.effect,
            state,
            catalog,
            ChoreographyTrackId::EffectClip(clip.id),
        );
        let source_name = effect_clip_source_name(catalog, clip.source);
        let selected = session.selection.primary == SemanticTarget::EffectClip(clip.id);
        let color = effect_reference_color(clip.source);
        let muted = state.muted_effect_clips.contains(&clip.id);
        let suppressed = session.solo_emitter.is_some()
            || state.solo_effect_clip.is_some_and(|solo| solo != clip.id);
        let mut args = FluentArgs::new();
        args.set("name", source_name.as_str());
        let move_label = localizer.text_with("timeline-move-effect-clip", &args);
        rows.spawn((
            TimelineTrackDropRow {
                track: ChoreographyTrackId::EffectClip(clip.id),
            },
            TimelineChoreographyGridRow(grid_row),
            RelativeCursorPosition::default(),
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(31.0),
                flex_shrink: 0.0,
                position_type: PositionType::Relative,
                border: UiRect::bottom(Val::Px(1.0)),
                grid_row: GridPlacement::start(grid_row),
                ..default()
            },
            BorderColor::all(theme::BORDER.with_alpha(0.45)),
        ))
        .with_children(|track| {
            let mut clip_node = Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(0.0),
                top: Val::Px(4.0),
                width: Val::Percent(1.0),
                height: Val::Px(23.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(8.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                border: UiRect::all(Val::Px(if selected { 2.0 } else { 1.0 })),
                overflow: Overflow::clip(),
                ..default()
            };
            apply_timeline_bar_geometry(&mut clip_node, clip.start_time, clip.duration, state.view);
            track
                .spawn((
                    TimelineEffectClip { clip: clip.id },
                    clip_node,
                    BackgroundColor(color.with_alpha(if muted || suppressed {
                        0.10
                    } else if selected {
                        0.42
                    } else {
                        0.25
                    })),
                    BorderColor::all(if selected { theme::TEXT } else { color }),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        Button,
                        EditorNativeControl,
                        TimelineEffectClipControl,
                        TimelineEffectClipInteraction {
                            clip: clip.id,
                            kind: TimelineDragKind::Move,
                        },
                        ChoreographyAction::SelectEffectClip(clip.id),
                        AccessibleLabel(move_label.clone()),
                        EditorTooltip::description(move_label),
                        EntityCursor::System(SystemCursorIcon::Grab),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(8.0),
                            right: Val::Px(8.0),
                            top: Val::Px(0.0),
                            bottom: Val::Px(0.0),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .observe(begin_effect_clip_timeline_drag)
                    .observe(move_effect_clip_timeline_drag)
                    .observe(finish_effect_clip_timeline_drag)
                    .observe(select_timeline_effect_clip)
                    .observe(stop_timeline_control_press);
                    bar.spawn((
                        Text::new(source_name.clone()),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        TextLayout::no_wrap(),
                        Node {
                            min_width: Val::Px(0.0),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                    for (kind, left, right, message_id) in [
                        (
                            TimelineDragKind::TrimStart,
                            Val::Px(0.0),
                            Val::Auto,
                            "timeline-trim-effect-clip-start",
                        ),
                        (
                            TimelineDragKind::TrimEnd,
                            Val::Auto,
                            Val::Px(0.0),
                            "timeline-trim-effect-clip-end",
                        ),
                    ] {
                        let trim_label = localizer.text_with(message_id, &args);
                        let boundary = match kind {
                            TimelineDragKind::TrimStart => clip.start_time,
                            TimelineDragKind::TrimEnd => clip.start_time + clip.duration,
                            TimelineDragKind::Move => {
                                unreachable!()
                            }
                        };
                        bar.spawn((
                            Button,
                            EditorNativeControl,
                            TimelineEffectClipControl,
                            TimelineEffectClipInteraction {
                                clip: clip.id,
                                kind,
                            },
                            ChoreographyAction::SelectEffectClip(clip.id),
                            AccessibleLabel(trim_label.clone()),
                            EditorTooltip::description(trim_label),
                            EntityCursor::System(SystemCursorIcon::EwResize),
                            Node {
                                display: if timeline_boundary_is_visible(boundary, state.view) {
                                    Display::Flex
                                } else {
                                    Display::None
                                },
                                position_type: PositionType::Absolute,
                                left,
                                right,
                                top: Val::Px(0.0),
                                width: Val::Px(8.0),
                                height: Val::Percent(100.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .observe(begin_effect_clip_timeline_drag)
                        .observe(move_effect_clip_timeline_drag)
                        .observe(finish_effect_clip_timeline_drag)
                        .observe(select_timeline_effect_clip)
                        .observe(stop_timeline_control_press)
                        .with_child((
                            Node {
                                width: Val::Px(2.0),
                                height: Val::Px(13.0),
                                border_radius: BorderRadius::all(Val::Px(1.0)),
                                ..default()
                            },
                            BackgroundColor(theme::TEXT),
                            Pickable::IGNORE,
                        ));
                    }
                });
        });
        for (index, projection) in referenced_track_projections(catalog, state, clip)
            .iter()
            .enumerate()
        {
            spawn_referenced_track_row(
                rows,
                state,
                localizer,
                clip,
                projection,
                muted,
                suppressed,
                grid_row + index as i16 + 1,
            );
        }
    }
    for emitter in &session.effect.emitters {
        let grid_row = choreography_grid_row(
            &session.effect,
            state,
            catalog,
            ChoreographyTrackId::Emitter(emitter.id),
        );
        rows.spawn((
            TimelineTrackDropRow {
                track: ChoreographyTrackId::Emitter(emitter.id),
            },
            TimelineChoreographyGridRow(grid_row),
            RelativeCursorPosition::default(),
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(31.0),
                flex_shrink: 0.0,
                position_type: PositionType::Relative,
                border: UiRect::bottom(Val::Px(1.0)),
                grid_row: GridPlacement::start(grid_row),
                ..default()
            },
            BorderColor::all(theme::BORDER.with_alpha(0.45)),
        ))
        .with_children(|track| {
            let audible_in_preview =
                emitter.enabled && session.solo_emitter.is_none_or(|solo| solo == emitter.id);
            for region in emitter.timeline_regions() {
                let region_selection = (emitter.id, region.id);
                let region_is_selected = state.selected_emitter_regions.contains(&region_selection);
                let move_label =
                    emitter_timing_label(localizer, "timeline-move-emitter-clip", &emitter.name);
                let mut clip_node = Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(0.0),
                    top: Val::Px(5.0),
                    width: Val::Percent(1.0),
                    height: Val::Px(21.0),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    border: UiRect::all(Val::Px(if region_is_selected { 2.0 } else { 1.0 })),
                    overflow: Overflow::clip(),
                    ..default()
                };
                apply_timeline_bar_geometry(
                    &mut clip_node,
                    region.start_time,
                    region.duration,
                    state.view,
                );
                track
                    .spawn((
                        TimelineClip {
                            emitter: emitter.id,
                            region: region.id,
                        },
                        RelativeCursorPosition::default(),
                        clip_node,
                        BackgroundColor(layer_color(emitter.id, emitter.display_color).with_alpha(
                            if audible_in_preview {
                                if region_is_selected { 0.36 } else { 0.28 }
                            } else {
                                0.10
                            },
                        )),
                        BorderColor::all(if region_is_selected {
                            theme::ACCENT
                        } else {
                            layer_color(emitter.id, emitter.display_color)
                                .with_alpha(if audible_in_preview { 1.0 } else { 0.45 })
                        }),
                    ))
                    .with_children(|clip| {
                        clip.spawn((
                            Button,
                            EditorNativeControl,
                            TimelineClipInteraction {
                                emitter: emitter.id,
                                region: region.id,
                                kind: TimelineDragKind::Move,
                            },
                            ChoreographyAction::SelectEmitterRegion {
                                emitter: emitter.id,
                                region: region.id,
                            },
                            AccessibleLabel(move_label.clone()),
                            EditorTooltip::description(move_label),
                            EntityCursor::System(SystemCursorIcon::Grab),
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(8.0),
                                right: Val::Px(8.0),
                                top: Val::Px(0.0),
                                bottom: Val::Px(0.0),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .observe(begin_timeline_clip_drag)
                        .observe(move_timeline_clip_drag)
                        .observe(finish_timeline_clip_drag)
                        .observe(select_timeline_clip)
                        .observe(open_timeline_region_context_menu)
                        .observe(stop_timeline_control_press);
                        for (kind, left, right) in [
                            (TimelineDragKind::TrimStart, Val::Px(0.0), Val::Auto),
                            (TimelineDragKind::TrimEnd, Val::Auto, Val::Px(0.0)),
                        ] {
                            let boundary_label = emitter_timing_label(
                                localizer,
                                match kind {
                                    TimelineDragKind::TrimStart => "timeline-trim-emitter-start",
                                    TimelineDragKind::TrimEnd => "timeline-trim-emitter-end",
                                    TimelineDragKind::Move => {
                                        unreachable!()
                                    }
                                },
                                &emitter.name,
                            );
                            let boundary = match kind {
                                TimelineDragKind::TrimStart => region.start_time,
                                TimelineDragKind::TrimEnd => region.start_time + region.duration,
                                TimelineDragKind::Move => {
                                    unreachable!()
                                }
                            };
                            clip.spawn((
                                Button,
                                EditorNativeControl,
                                TimelineClipInteraction {
                                    emitter: emitter.id,
                                    region: region.id,
                                    kind,
                                },
                                ChoreographyAction::SelectEmitterRegion {
                                    emitter: emitter.id,
                                    region: region.id,
                                },
                                AccessibleLabel(boundary_label.clone()),
                                EditorTooltip::description(boundary_label),
                                EntityCursor::System(SystemCursorIcon::EwResize),
                                Node {
                                    display: if timeline_boundary_is_visible(boundary, state.view) {
                                        Display::Flex
                                    } else {
                                        Display::None
                                    },
                                    position_type: PositionType::Absolute,
                                    left,
                                    right,
                                    top: Val::Px(0.0),
                                    width: Val::Px(8.0),
                                    height: Val::Percent(100.0),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    ..default()
                                },
                                BackgroundColor(Color::NONE),
                            ))
                            .observe(begin_timeline_clip_drag)
                            .observe(move_timeline_clip_drag)
                            .observe(finish_timeline_clip_drag)
                            .observe(select_timeline_clip)
                            .observe(open_timeline_region_context_menu)
                            .observe(stop_timeline_control_press)
                            .with_child((
                                Node {
                                    width: Val::Px(2.0),
                                    height: Val::Px(13.0),
                                    ..default()
                                },
                                BackgroundColor(layer_color(emitter.id, emitter.display_color)),
                                Pickable::IGNORE,
                            ));
                        }
                    });
            }
        });
        if state.expanded_automation_emitters.contains(&emitter.id) {
            for (lane_index, lane) in
                emitter_automation_lanes(&session.effect, emitter, registry, localizer)
                    .iter()
                    .filter(|lane| automation_lane_is_visible(state, &lane.id))
                    .enumerate()
            {
                spawn_automation_lane_row(
                    rows,
                    session,
                    state,
                    curves,
                    localizer,
                    emitter,
                    lane,
                    grid_row + lane_index as i16 + 1,
                );
            }
        }
    }
    spawn_effect_drop_spacer(rows);
}

#[cfg(test)]
#[path = "tests/track_composition.rs"]
mod tests;
