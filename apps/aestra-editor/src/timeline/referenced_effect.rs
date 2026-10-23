#[cfg(test)]
use super::clicks::referenced_emitter_click_action;
pub(super) use super::clicks::{
    open_referenced_emitter_source_from_header, select_timeline_referenced_emitter,
};
use super::gestures::stop_timeline_control_press;
use super::interaction_state::*;
use super::presentation::apply_timeline_bar_geometry;
use super::timeline_pointer::TimelineButton as Button;
use super::track_style::{effect_reference_color, emitter_timing_label, layer_color};
use super::{ChoreographyAction, TimelineState};
use crate::feathers::{
    action_activation::EditorNativeControl,
    button::spawn_tool_button as mini_button,
    icon::{SvgColor, UiSvg, load_svg_icon},
    list_row::KeyboardNavigableListRow,
    tooltip::EditorTooltip,
};
use crate::localization::Localizer;
use crate::theme;
#[cfg(test)]
use aestra_core::{EffectAssetRef, EffectClipId, EmitterId};
use aestra_core::{EffectClip, Emitter};
use bevy::{prelude::*, ui::Selected, ui_widgets::ListItem};
use fluent_bundle::FluentArgs;
#[cfg(test)]
use std::time::Duration;

pub(super) use super::projections::*;

pub(super) fn spawn_referenced_emitter_track_header(
    parent: &mut ChildSpawnerCommands,
    state: &TimelineState,
    localizer: &Localizer,
    path: &EffectClipPath,
    depth: usize,
    emitter: &Emitter,
    grid_row: i16,
) {
    let selected = state.inspected_child.as_ref()
        == Some(&EffectClipChildSelection::Emitter {
            path: path.clone(),
            emitter: emitter.id,
        });
    let mut args = FluentArgs::new();
    args.set("name", emitter.name.as_str());
    let label = localizer.text_with("timeline-inspect-referenced-emitter", &args);
    let color = layer_color(emitter.id, emitter.display_color);
    let mut header = parent.spawn((
        Button,
        EditorNativeControl,
        ListItem,
        KeyboardNavigableListRow,
        ReferencedEmitterTrackHeader,
        TimelineChoreographyGridRow(grid_row),
        ChoreographyAction::SelectEffectClipEmitter {
            path: path.clone(),
            emitter: emitter.id,
        },
        AccessibleLabel(label.clone()),
        EditorTooltip::description(label),
        Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            height: Val::Px(27.0),
            flex_shrink: 0.0,
            padding: UiRect::new(
                Val::Px(7.0 + depth as f32 * 22.0),
                Val::Px(7.0),
                Val::Px(0.0),
                Val::Px(0.0),
            ),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            border: UiRect::bottom(Val::Px(1.0)),
            grid_row: GridPlacement::start(grid_row),
            ..default()
        },
        BackgroundColor(if selected {
            theme::SELECTION.with_alpha(0.75)
        } else {
            theme::PANEL_DARK
        }),
        BorderColor::all(theme::BORDER.with_alpha(0.4)),
    ));
    if selected {
        header.insert(Selected);
    }
    header.observe(open_referenced_emitter_source_from_header);
    header.with_children(|row| {
        row.spawn((
            Text::new("└"),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
            Pickable::IGNORE,
        ));
        row.spawn((
            Node {
                width: Val::Px(9.0),
                height: Val::Px(9.0),
                flex_shrink: 0.0,
                border_radius: BorderRadius::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(color),
            Pickable::IGNORE,
        ));
        row.spawn((
            Text::new(&emitter.name),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(if emitter.enabled {
                theme::TEXT_MUTED
            } else {
                theme::TEXT_FAINT
            }),
            TextLayout::no_wrap(),
            Node {
                min_width: Val::Px(0.0),
                flex_grow: 1.0,
                overflow: Overflow::clip(),
                ..default()
            },
            Pickable::IGNORE,
        ));
        row.spawn((
            Text::new(localizer.text("timeline-read-only-short")),
            TextFont {
                font_size: FontSize::Px(7.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
            Pickable::IGNORE,
        ));
    });
}

pub(super) fn spawn_referenced_effect_clip_track_header(
    parent: &mut ChildSpawnerCommands,
    state: &TimelineState,
    localizer: &Localizer,
    path: &EffectClipPath,
    depth: usize,
    clip: &EffectClip,
    source_name: &str,
    child_count: usize,
    grid_row: i16,
    asset_server: &AssetServer,
) {
    let selected = state.inspected_child.as_ref()
        == Some(&EffectClipChildSelection::EffectClip { path: path.clone() });
    let expanded = state.expanded_effect_clips.contains(path);
    let label = emitter_timing_label(localizer, "timeline-select-effect-clip", source_name);
    let mut header = parent.spawn((
        Button,
        EditorNativeControl,
        ListItem,
        KeyboardNavigableListRow,
        ReferencedEmitterTrackHeader,
        TimelineChoreographyGridRow(grid_row),
        ChoreographyAction::SelectReferencedEffectClip(path.clone()),
        AccessibleLabel(label.clone()),
        EditorTooltip::description(label),
        Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            height: Val::Px(27.0),
            flex_shrink: 0.0,
            padding: UiRect::new(
                Val::Px(7.0 + depth as f32 * 22.0),
                Val::Px(7.0),
                Val::Px(0.0),
                Val::Px(0.0),
            ),
            align_items: AlignItems::Center,
            column_gap: Val::Px(5.0),
            border: UiRect::bottom(Val::Px(1.0)),
            grid_row: GridPlacement::start(grid_row),
            ..default()
        },
        BackgroundColor(if selected {
            theme::SELECTION.with_alpha(0.75)
        } else {
            theme::PANEL_DARK
        }),
        BorderColor::all(theme::BORDER.with_alpha(0.4)),
    ));
    if selected {
        header.insert(Selected);
    }
    header.with_children(|row| {
        let disclosure = mini_button(
            row,
            "",
            ChoreographyAction::ToggleEffectClipExpanded(path.clone()),
        );
        row.commands().entity(disclosure).insert(Node {
            display: if child_count > 0 {
                Display::Flex
            } else {
                Display::None
            },
            width: Val::Px(20.0),
            height: Val::Px(21.0),
            flex_shrink: 0.0,
            ..default()
        });
        row.commands().entity(disclosure).with_children(|button| {
            button.spawn((
                Node {
                    width: Val::Px(16.0),
                    height: Val::Px(16.0),
                    ..default()
                },
                UiSvg(load_svg_icon(
                    asset_server,
                    if expanded {
                        "icons/chevron-down.svg"
                    } else {
                        "icons/chevron-right.svg"
                    },
                )),
                SvgColor(theme::TEXT),
                Pickable::IGNORE,
            ));
        });
        row.spawn((
            Node {
                width: Val::Px(13.0),
                height: Val::Px(13.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(effect_reference_color(clip.source)),
            Pickable::IGNORE,
        ))
        .with_child((
            Text::new("FX"),
            TextFont {
                font_size: FontSize::Px(7.0),
                ..default()
            },
            TextColor(theme::PANEL_DARK),
            Pickable::IGNORE,
        ));
        row.spawn((
            Text::new(source_name),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(theme::TEXT_MUTED),
            TextLayout::no_wrap(),
            Node {
                min_width: Val::Px(0.0),
                flex_grow: 1.0,
                overflow: Overflow::clip(),
                ..default()
            },
            Pickable::IGNORE,
        ));
        row.spawn((
            Text::new(localizer.text("timeline-read-only-short")),
            TextFont {
                font_size: FontSize::Px(7.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
            Pickable::IGNORE,
        ));
    });
}

pub(super) fn spawn_referenced_track_row(
    parent: &mut ChildSpawnerCommands,
    state: &TimelineState,
    localizer: &Localizer,
    root_clip: &EffectClip,
    projection: &ReferencedTrackProjection,
    muted: bool,
    suppressed: bool,
    grid_row: i16,
) {
    let (selected, color, name, action) = match &projection.kind {
        ReferencedTrackKind::EffectClip {
            clip, source_name, ..
        } => (
            state.inspected_child.as_ref()
                == Some(&EffectClipChildSelection::EffectClip {
                    path: projection.path.clone(),
                }),
            effect_reference_color(clip.source),
            source_name.as_str(),
            ChoreographyAction::SelectReferencedEffectClip(projection.path.clone()),
        ),
        ReferencedTrackKind::Emitter { emitter, .. } => (
            state.inspected_child.as_ref()
                == Some(&EffectClipChildSelection::Emitter {
                    path: projection.path.clone(),
                    emitter: emitter.id,
                }),
            layer_color(emitter.id, emitter.display_color),
            emitter.name.as_str(),
            ChoreographyAction::SelectEffectClipEmitter {
                path: projection.path.clone(),
                emitter: emitter.id,
            },
        ),
    };
    parent
        .spawn((
            TimelineChoreographyGridRow(grid_row),
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(27.0),
                flex_shrink: 0.0,
                position_type: PositionType::Relative,
                border: UiRect::bottom(Val::Px(1.0)),
                grid_row: GridPlacement::start(grid_row),
                ..default()
            },
        ))
        .with_children(|track| {
            let Some(timing) = projection.timing else {
                return;
            };
            let mut child_node = Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(0.0),
                top: Val::Px(4.0),
                width: Val::Percent(1.0),
                height: Val::Px(19.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(7.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                border: UiRect::all(Val::Px(if selected { 2.0 } else { 1.0 })),
                overflow: Overflow::clip(),
                ..default()
            };
            apply_timeline_bar_geometry(
                &mut child_node,
                root_clip.start_time + timing.root_source_start - root_clip.source_offset,
                timing.duration,
                state.view,
            );
            track
                .spawn((
                    TimelineReferencedEmitter {
                        clip: root_clip.id,
                        source_start: timing.root_source_start,
                        source_duration: timing.duration,
                    },
                    child_node,
                    BackgroundColor(color.with_alpha(if muted || suppressed {
                        0.08
                    } else {
                        0.20
                    })),
                    BorderColor::all(if selected {
                        theme::TEXT
                    } else {
                        color.with_alpha(0.75)
                    }),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        Button,
                        EditorNativeControl,
                        TimelineReferencedEmitterControl,
                        action,
                        AccessibleLabel(localizer.text("timeline-inspect-referenced-emitter-bar")),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            right: Val::Px(0.0),
                            top: Val::Px(0.0),
                            bottom: Val::Px(0.0),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .observe(select_timeline_referenced_emitter)
                    .observe(stop_timeline_control_press);
                    bar.spawn((
                        Text::new(name),
                        TextFont {
                            font_size: FontSize::Px(8.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        TextLayout::no_wrap(),
                        Node {
                            min_width: Val::Px(0.0),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                });
        });
}

#[cfg(test)]
pub(super) fn mapped_referenced_emitter_timing(
    clip: &EffectClip,
    emitter: &Emitter,
) -> Option<(f32, f32)> {
    map_referenced_interval(
        ReferencedTimingContext {
            root_source_start: clip.source_offset,
            local_source_offset: clip.source_offset,
            duration: clip.duration,
        },
        emitter.start_time,
        emitter.duration,
    )
    .map(|(timing, _)| {
        (
            clip.start_time + timing.root_source_start - clip.source_offset,
            timing.duration,
        )
    })
}

#[cfg(test)]
#[path = "tests/referenced_effect_composition.rs"]
mod composition_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn referenced_emitter_timing_is_clipped_and_mapped_into_parent_time() {
        let source = EffectAssetRef::new(aestra_core::EffectId::from_u128(0xfeed));
        let mut clip = EffectClip::new(source, 1.0, 2.0);
        clip.source_offset = 0.5;
        let mut emitter = Emitter::basic_sprite("Child", 2.0);
        emitter.start_time = 0.25;
        emitter.duration = 1.5;

        let (start, duration) = mapped_referenced_emitter_timing(&clip, &emitter).unwrap();
        assert!((start - 1.0).abs() < 0.000_1);
        assert!((duration - 1.25).abs() < 0.000_1);

        emitter.start_time = 4.0;
        assert!(mapped_referenced_emitter_timing(&clip, &emitter).is_none());
    }

    #[test]
    fn nested_reference_timing_is_clipped_through_every_ancestor() {
        let root = ReferencedTimingContext {
            root_source_start: 0.5,
            local_source_offset: 0.5,
            duration: 2.0,
        };
        let (nested_clip, clipped_offset) = map_referenced_interval(root, 0.25, 1.5).unwrap();
        assert!((nested_clip.root_source_start - 0.5).abs() < 0.000_1);
        assert!((nested_clip.duration - 1.25).abs() < 0.000_1);
        assert!((clipped_offset - 0.25).abs() < 0.000_1);

        let nested_source = ReferencedTimingContext {
            root_source_start: nested_clip.root_source_start,
            local_source_offset: 0.2 + clipped_offset,
            duration: nested_clip.duration,
        };
        let (nested_emitter, _) = map_referenced_interval(nested_source, 0.4, 1.0).unwrap();
        assert!((nested_emitter.root_source_start - 0.5).abs() < 0.000_1);
        assert!((nested_emitter.duration - 0.95).abs() < 0.000_1);
        assert!(map_referenced_interval(nested_source, 2.0, 0.25).is_none());
    }

    #[test]
    fn effect_clip_paths_distinguish_reused_nested_sources() {
        let root = EffectClipPath::root_path(EffectClipId::new());
        let first_branch = root.child(EffectClipId::new());
        let second_branch = root.child(EffectClipId::new());
        let shared_source_clip = EffectClipId::new();
        let first_leaf = first_branch.child(shared_source_clip);
        let second_leaf = second_branch.child(shared_source_clip);

        assert_ne!(first_leaf, second_leaf);
        assert!(first_leaf.starts_with(&root));
        assert!(first_leaf.starts_with(&first_branch));
        assert!(!first_leaf.starts_with(&second_branch));
    }

    #[test]
    fn double_click_opens_referenced_emitter_source_with_the_emitter_target() {
        let path = EffectClipPath::root_path(EffectClipId::new());
        let emitter = EmitterId::new();
        let mut state = TimelineState::framed(1.0);
        let selection = ChoreographyAction::SelectEffectClipEmitter {
            path: path.clone(),
            emitter,
        };

        assert_eq!(
            referenced_emitter_click_action(&mut state, Duration::ZERO, 1, &selection),
            selection
        );
        assert_eq!(
            referenced_emitter_click_action(&mut state, Duration::from_millis(200), 1, &selection,),
            ChoreographyAction::EditEffectClipEmitterSource { path, emitter }
        );
    }
}
