//! Production outer effect-clip header, context menu and shared reorder icon construction.
use super::clicks::{
    configure_timeline_track_action_control, open_effect_clip_source_from_header,
    open_effect_clip_track_context_menu,
};
use super::gestures::stop_timeline_control_press;
use super::interaction_state::*;
use super::navigation_cursor::EntityCursor;
use super::reorder::{
    begin_effect_clip_track_reorder, drop_effect_clip_track_reorder, drop_emitter_track_reorder,
    finish_effect_clip_track_reorder,
};
use super::timeline_pointer::TimelineButton as Button;
use super::track_style::{effect_reference_color, emitter_timing_label};
use super::{ChoreographyAction, TimelineState};
use crate::feathers::{
    action_activation::EditorNativeControl,
    button::spawn_tool_button as mini_button,
    context_menu::{spawn_pointer_context_menu, spawn_pointer_context_menu_item},
    icon::{SvgColor, UiSvg, load_svg_icon},
    list_row::KeyboardNavigableListRow,
    tooltip::EditorTooltip,
};
use crate::{EditorSession, localization::Localizer, theme};
use aestra_authoring::SemanticTarget;
use aestra_core::{ChoreographyTrackId, EffectClip, EffectClipId};
use bevy::{
    feathers::controls::ButtonVariant,
    prelude::*,
    ui::{RelativeCursorPosition, Selected},
    ui_widgets::ListItem,
    window::SystemCursorIcon,
};
use fluent_bundle::FluentArgs;

pub(super) fn spawn_effect_clip_track_header(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &TimelineState,
    localizer: &Localizer,
    clip: &EffectClip,
    source_name: &str,
    child_count: usize,
    grid_row: i16,
    asset_server: &AssetServer,
) {
    let selected = session.selection.primary == SemanticTarget::EffectClip(clip.id);
    let path = EffectClipPath::root_path(clip.id);
    let expanded = state.expanded_effect_clips.contains(&path);
    let muted = state.muted_effect_clips.contains(&clip.id);
    let soloed = state.solo_effect_clip == Some(clip.id);
    let mut args = FluentArgs::new();
    args.set("name", source_name);
    let label = localizer.text_with("timeline-select-effect-clip", &args);
    let mut header = parent.spawn((
        Button,
        EditorNativeControl,
        ListItem,
        KeyboardNavigableListRow,
        EffectClipTrackHeader { clip: clip.id },
        TimelineTrackHeader {
            track: ChoreographyTrackId::EffectClip(clip.id),
        },
        TimelineChoreographyGridRow(grid_row),
        RelativeCursorPosition::default(),
        ChoreographyAction::SelectEffectClip(clip.id),
        AccessibleLabel(label.clone()),
        EditorTooltip::description(label),
        Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            height: Val::Px(31.0),
            flex_shrink: 0.0,
            padding: UiRect::horizontal(Val::Px(7.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(5.0),
            border: UiRect::bottom(Val::Px(1.0)),
            grid_row: GridPlacement::start(grid_row),
            ..default()
        },
        BackgroundColor(if selected {
            theme::SELECTION
        } else {
            theme::PANEL_DARK
        }),
        BorderColor::all(theme::BORDER.with_alpha(0.55)),
    ));
    if selected {
        header.insert(Selected);
    }
    header.observe(drop_effect_clip_track_reorder);
    header.observe(drop_emitter_track_reorder);
    header.observe(open_effect_clip_source_from_header);
    header.observe(open_effect_clip_track_context_menu);
    header.with_children(|row| {
        let reorder_label =
            emitter_timing_label(localizer, "timeline-reorder-effect-clip", source_name);
        spawn_effect_clip_reorder_handle(row, clip.id, reorder_label, asset_server);
        let disclosure = mini_button(row, "", ChoreographyAction::ToggleEffectClipExpanded(path));
        let disclosure_label = localizer.text(if expanded {
            "timeline-collapse-effect-clip"
        } else {
            "timeline-expand-effect-clip"
        });
        row.commands().entity(disclosure).insert((
            AccessibleLabel(disclosure_label.clone()),
            EditorTooltip::description(disclosure_label),
            Node {
                width: Val::Px(22.0),
                height: Val::Px(23.0),
                flex_shrink: 0.0,
                ..default()
            },
        ));
        configure_timeline_track_action_control(row.commands(), disclosure);
        row.commands()
            .entity(disclosure)
            .observe(open_effect_clip_track_context_menu);
        row.commands().entity(disclosure).with_children(|button| {
            button.spawn((
                Node {
                    width: Val::Px(18.0),
                    height: Val::Px(18.0),
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
                width: Val::Px(14.0),
                height: Val::Px(14.0),
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
        let mute = mini_button(row, "M", ChoreographyAction::ToggleEffectClipMuted(clip.id));
        let mute_label = localizer.text(if muted {
            "timeline-unmute-effect-clip"
        } else {
            "timeline-mute-effect-clip"
        });
        row.commands().entity(mute).insert((
            AccessibleLabel(mute_label.clone()),
            EditorTooltip::description(mute_label),
        ));
        configure_timeline_track_action_control(row.commands(), mute);
        row.commands()
            .entity(mute)
            .observe(open_effect_clip_track_context_menu);
        if muted {
            row.commands()
                .entity(mute)
                .insert((Selected, ButtonVariant::Primary));
        }
        let solo = mini_button(row, "S", ChoreographyAction::ToggleEffectClipSolo(clip.id));
        let solo_label = localizer.text(if soloed {
            "timeline-unsolo-effect-clip"
        } else {
            "timeline-solo-effect-clip"
        });
        row.commands().entity(solo).insert((
            AccessibleLabel(solo_label.clone()),
            EditorTooltip::description(solo_label),
        ));
        configure_timeline_track_action_control(row.commands(), solo);
        row.commands()
            .entity(solo)
            .observe(open_effect_clip_track_context_menu);
        if soloed {
            row.commands()
                .entity(solo)
                .insert((Selected, ButtonVariant::Primary));
        }
        row.spawn((
            Text::new(source_name),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(theme::TEXT),
            TextLayout::no_wrap(),
            Node {
                min_width: Val::Px(0.0),
                flex_shrink: 1.0,
                overflow: Overflow::clip(),
                ..default()
            },
            Pickable::IGNORE,
        ));
        row.spawn((
            Text::new(child_count.to_string()),
            TextFont {
                font_size: FontSize::Px(8.0),
                ..default()
            },
            TextColor(theme::TEXT_FAINT),
            Pickable::IGNORE,
        ));
        if state.context_effect_clip == Some(clip.id) {
            spawn_effect_clip_context_menu(
                row,
                localizer,
                clip.id,
                muted,
                soloed,
                state.context_menu_position,
            );
        }
    });
}

pub(super) fn spawn_effect_clip_context_menu(
    parent: &mut ChildSpawnerCommands,
    localizer: &Localizer,
    clip: EffectClipId,
    muted: bool,
    soloed: bool,
    position: Vec2,
) {
    spawn_pointer_context_menu(
        parent,
        position,
        TimelineTrackContextMenuAnchor,
        EffectClipTrackContextMenu,
        |menu| {
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text("timeline-menu-edit-source"),
                ChoreographyAction::EditEffectClipSource(clip),
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text("timeline-menu-explode-effect-clip"),
                crate::asset_actions::AssetAction::ExplodeEffectClip(clip),
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text(if muted {
                    "timeline-menu-unmute"
                } else {
                    "timeline-menu-mute"
                }),
                ChoreographyAction::ToggleEffectClipMuted(clip),
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text(if soloed {
                    "timeline-menu-unsolo"
                } else {
                    "timeline-menu-solo"
                }),
                ChoreographyAction::ToggleEffectClipSolo(clip),
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text("timeline-menu-delete"),
                ChoreographyAction::DeleteEffectClip(clip),
            );
        },
    );
}

pub(super) fn spawn_effect_clip_reorder_handle(
    row: &mut ChildSpawnerCommands,
    clip: EffectClipId,
    label: String,
    asset_server: &AssetServer,
) {
    let handle = row
        .spawn((
            Button,
            EditorNativeControl,
            EffectClipTrackReorderHandle { clip },
            AccessibleLabel(label.clone()),
            EditorTooltip::description(label),
            EntityCursor::System(SystemCursorIcon::Grab),
            Node {
                width: Val::Px(18.0),
                height: Val::Px(23.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .observe(stop_timeline_control_press)
        .observe(begin_effect_clip_track_reorder)
        .observe(finish_effect_clip_track_reorder)
        .id();
    spawn_track_reorder_icon(row, handle, asset_server);
}

pub(super) fn spawn_track_reorder_icon(
    row: &mut ChildSpawnerCommands,
    handle: Entity,
    asset_server: &AssetServer,
) {
    row.commands().entity(handle).with_children(|handle| {
        handle.spawn((
            Node {
                width: Val::Px(18.0),
                height: Val::Px(20.0),
                ..default()
            },
            UiSvg(load_svg_icon(asset_server, "icons/drag-vertical.svg")),
            SvgColor(theme::TEXT_MUTED),
            Pickable::IGNORE,
        ));
    });
}

#[cfg(test)]
#[path = "tests/effect_clip_composition.rs"]
mod tests;
