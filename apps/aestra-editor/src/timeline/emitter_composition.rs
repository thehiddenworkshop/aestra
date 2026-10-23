//! Production emitter headers and their context, color and automation popovers.
use super::automation::{AutomationLaneProjection, EmitterAutomationVisibilityMenuAnchor};
use super::clicks::{
    collapse_emitter_multi_selection_on_primary_click, configure_timeline_track_action_control,
    open_timeline_track_context_menu,
};
use super::effect_clip_composition::spawn_track_reorder_icon;
use super::gestures::stop_timeline_control_press;
use super::interaction_state::*;
use super::navigation_cursor::EntityCursor;
use super::reorder::{
    begin_emitter_track_reorder, drop_effect_clip_track_reorder, drop_emitter_track_reorder,
    finish_emitter_track_reorder,
};
use super::timeline_pointer::TimelineButton as Button;
use super::track_style::{emitter_timing_label, layer_color};
use super::{ChoreographyAction, TimelineState};
use crate::feathers::{
    action_activation::{EditorNativeControl, FeathersActionButton},
    button::spawn_tool_button as mini_button,
    color_picker::{ColorPickerLabels, spawn_color_picker},
    context_menu::{
        spawn_pointer_context_menu, spawn_pointer_context_menu_custom_item,
        spawn_pointer_context_menu_item,
    },
    icon::{SvgColor, UiSvg, load_svg_icon},
    list_row::KeyboardNavigableListRow,
    scenes as ui_shell,
    tooltip::EditorTooltip,
};
use crate::{EditorSession, localization::Localizer, theme};
use aestra_core::{ChoreographyTrackId, EmitterId};
use bevy::{
    feathers::controls::ButtonVariant,
    prelude::*,
    ui::{RelativeCursorPosition, Selected},
    ui_widgets::{
        ListItem,
        popover::{Popover, PopoverAlign, PopoverPlacement, PopoverSide},
    },
    window::SystemCursorIcon,
};
use fluent_bundle::FluentArgs;

pub(super) fn spawn_emitter_track_header(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &TimelineState,
    localizer: &Localizer,
    index: usize,
    emitter: EmitterId,
    name: &str,
    enabled: bool,
    display_color: Option<[f32; 4]>,
    grid_row: i16,
    automation_lanes: &[AutomationLaneProjection],
    asset_server: &AssetServer,
) {
    let primary_emitter = session.selection.emitter(&session.effect);
    let multi_selection_is_current =
        primary_emitter.is_some_and(|primary| state.selected_emitters.contains(&primary));
    let selected = if multi_selection_is_current {
        state.selected_emitters.contains(&emitter)
    } else {
        primary_emitter == Some(emitter)
    };
    let diagnostic = emitter_has_diagnostic(session, index);
    let mut args = FluentArgs::new();
    args.set("name", name);
    let soloed = session.solo_emitter == Some(emitter);
    let state_message = match (enabled, soloed, session.solo_emitter.is_some()) {
        (false, true, _) => "timeline-emitter-muted-soloed-status",
        (true, true, _) => "timeline-emitter-soloed-status",
        (true, false, true) => "timeline-emitter-suppressed-status",
        (true, false, false) => "timeline-emitter-enabled-status",
        (false, false, _) => "timeline-emitter-muted-status",
    };
    args.set("state", localizer.text(state_message));
    let accessible_label = localizer.text_with("timeline-select-emitter-status", &args);
    let mut header = parent.spawn((
        Button,
        EditorNativeControl,
        ListItem,
        KeyboardNavigableListRow,
        EmitterTrackHeader { emitter },
        TimelineTrackHeader {
            track: ChoreographyTrackId::Emitter(emitter),
        },
        TimelineChoreographyGridRow(grid_row),
        ChoreographyAction::SelectEmitter(emitter),
        AccessibleLabel(accessible_label),
        RelativeCursorPosition::default(),
        Node {
            width: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            height: Val::Px(31.0),
            flex_shrink: 0.0,
            padding: UiRect::horizontal(Val::Px(7.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            position_type: PositionType::Relative,
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
    if diagnostic {
        header.insert(EmitterTrackDiagnostic);
    }
    if selected {
        header.insert(Selected);
    }
    if !enabled {
        header.insert(EmitterTrackDisabled);
    }
    header
        .observe(collapse_emitter_multi_selection_on_primary_click)
        .observe(open_timeline_track_context_menu)
        .observe(drop_emitter_track_reorder)
        .observe(drop_effect_clip_track_reorder)
        .with_children(|row| {
            let reorder_label = emitter_timing_label(localizer, "timeline-reorder-emitter", name);
            let handle = row
                .spawn((
                    Button,
                    EditorNativeControl,
                    EmitterTrackReorderHandle { emitter },
                    AccessibleLabel(reorder_label.clone()),
                    EditorTooltip::description(reorder_label),
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
                .observe(begin_emitter_track_reorder)
                .observe(finish_emitter_track_reorder)
                .id();
            spawn_track_reorder_icon(row, handle, asset_server);
            let track_color = layer_color(emitter, display_color);
            let color_label =
                emitter_timing_label(localizer, "timeline-change-emitter-color", name);
            let mut color_button = row.spawn_empty();
            let color_control = color_button.id();
            color_button
                .apply_scene(ui_shell::feathers_plain_button())
                .insert((
                    EmitterTrackColorChip,
                    RelativeCursorPosition::default(),
                    FeathersActionButton,
                    ChoreographyAction::ToggleEmitterColorPicker(emitter),
                    AccessibleLabel(color_label.clone()),
                    EditorTooltip::description(color_label),
                    Node {
                        width: Val::Px(18.0),
                        height: Val::Px(21.0),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        padding: UiRect::all(Val::Px(3.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                ));
            if state.color_picker_emitter == Some(emitter) {
                color_button.insert((Selected, ButtonVariant::Primary));
            }
            color_button.with_children(|chip| {
                chip.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(track_color),
                    BorderColor::all(theme::BORDER_BRIGHT),
                    EmitterTrackColorSwatch { emitter },
                    Pickable::IGNORE,
                ));
                if state.color_picker_emitter == Some(emitter) {
                    spawn_emitter_color_picker(
                        chip,
                        localizer,
                        emitter,
                        display_color,
                        color_components(layer_color(emitter, None)),
                    );
                }
            });
            configure_timeline_track_action_control(row.commands(), color_control);
            row.commands()
                .entity(color_control)
                .observe(open_timeline_track_context_menu);
            let muted = !enabled;
            let mute = mini_button(
                row,
                "M",
                ChoreographyAction::SetEmitterEnabled {
                    emitter,
                    enabled: !enabled,
                },
            );
            let mute_label = localizer.text(if muted {
                "timeline-unmute-emitter"
            } else {
                "timeline-mute-emitter"
            });
            row.commands().entity(mute).insert((
                AccessibleLabel(mute_label.clone()),
                EditorTooltip::description(mute_label),
            ));
            configure_timeline_track_action_control(row.commands(), mute);
            row.commands()
                .entity(mute)
                .observe(open_timeline_track_context_menu);
            if muted {
                row.commands()
                    .entity(mute)
                    .insert((Selected, ButtonVariant::Primary));
            }
            let solo = mini_button(row, "S", ChoreographyAction::ToggleEmitterSolo(emitter));
            let solo_label = localizer.text(if soloed {
                "timeline-unsolo-emitter"
            } else {
                "timeline-solo-emitter"
            });
            row.commands().entity(solo).insert((
                AccessibleLabel(solo_label.clone()),
                EditorTooltip::description(solo_label),
            ));
            configure_timeline_track_action_control(row.commands(), solo);
            row.commands()
                .entity(solo)
                .observe(open_timeline_track_context_menu);
            if soloed {
                row.commands()
                    .entity(solo)
                    .insert((Selected, ButtonVariant::Primary));
            }
            if !automation_lanes.is_empty() {
                let visible = state.expanded_automation_emitters.contains(&emitter)
                    && automation_lanes
                        .iter()
                        .any(|lane| state.visible_automation_lanes.contains(&lane.id));
                let menu_open = state.automation_menu_emitter == Some(emitter);
                let label = localizer.text("timeline-automation-visibility");
                let disclosure = mini_button(
                    row,
                    "A",
                    ChoreographyAction::ToggleEmitterAutomation(emitter),
                );
                row.commands().entity(disclosure).insert((
                    EmitterAutomationMenuButton,
                    RelativeCursorPosition::default(),
                    AccessibleLabel(label.clone()),
                    EditorTooltip::description(label),
                    Node {
                        width: Val::Px(23.0),
                        height: Val::Px(23.0),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                ));
                if visible || menu_open {
                    row.commands()
                        .entity(disclosure)
                        .insert((Selected, ButtonVariant::Primary));
                }
                configure_timeline_track_action_control(row.commands(), disclosure);
                if menu_open {
                    row.commands().entity(disclosure).with_children(|button| {
                        spawn_emitter_automation_visibility_menu(
                            button,
                            state,
                            localizer,
                            emitter,
                            automation_lanes,
                            asset_server,
                        );
                    });
                }
            } else {
                row.spawn(Node {
                    width: Val::Px(23.0),
                    height: Val::Px(1.0),
                    flex_shrink: 0.0,
                    ..default()
                });
            }
            row.spawn((
                TimelineTrackNameLabel,
                Text::new(name),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                TextLayout::no_wrap(),
                Pickable::IGNORE,
                Node {
                    min_width: Val::Px(0.0),
                    height: Val::Px(23.0),
                    flex_grow: 1.0,
                    overflow: Overflow::clip(),
                    align_items: AlignItems::Center,
                    ..default()
                },
                EditorTooltip::description(name),
            ));
            if diagnostic {
                row.spawn((
                    Text::new("!"),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::ACCENT),
                    AccessibleLabel(localizer.text("timeline-emitter-diagnostic")),
                    EditorTooltip::description(localizer.text("timeline-emitter-diagnostic")),
                ));
            }
            if state.context_emitter == Some(emitter) {
                spawn_emitter_context_menu(
                    row,
                    localizer,
                    emitter,
                    enabled,
                    soloed,
                    state
                        .selected_emitter_regions
                        .iter()
                        .any(|(selected, _)| *selected == emitter),
                    state.context_menu_position,
                );
            }
        });
}

pub(super) fn spawn_emitter_automation_visibility_menu(
    parent: &mut ChildSpawnerCommands,
    state: &TimelineState,
    localizer: &Localizer,
    emitter: EmitterId,
    lanes: &[AutomationLaneProjection],
    asset_server: &AssetServer,
) {
    let lane_ids = lanes.iter().map(|lane| lane.id.clone()).collect::<Vec<_>>();
    spawn_pointer_context_menu(
        parent,
        Vec2::new(11.5, 23.0),
        EmitterAutomationVisibilityMenuAnchor,
        EmitterAutomationVisibilityMenu,
        |menu| {
            spawn_automation_visibility_menu_item(
                menu,
                asset_server,
                &localizer.text("timeline-show-all-automation"),
                &localizer.text("timeline-show-all-automation"),
                true,
                ChoreographyAction::SetEmitterAutomationVisibility {
                    emitter,
                    lanes: lane_ids.clone(),
                    visible: true,
                },
            );
            spawn_automation_visibility_menu_item(
                menu,
                asset_server,
                &localizer.text("timeline-hide-all-automation"),
                &localizer.text("timeline-hide-all-automation"),
                false,
                ChoreographyAction::SetEmitterAutomationVisibility {
                    emitter,
                    lanes: lane_ids,
                    visible: false,
                },
            );
            for lane in lanes {
                let is_visible = state.visible_automation_lanes.contains(&lane.id);
                let mut args = FluentArgs::new();
                args.set("name", lane.label.as_str());
                let accessible_label = localizer.text_with(
                    if is_visible {
                        "timeline-hide-automation-lane"
                    } else {
                        "timeline-show-automation-lane"
                    },
                    &args,
                );
                spawn_automation_visibility_menu_item(
                    menu,
                    asset_server,
                    &accessible_label,
                    &lane.label,
                    is_visible,
                    ChoreographyAction::SetAutomationLaneVisibility {
                        lane: lane.id.clone(),
                        visible: !is_visible,
                    },
                );
            }
        },
    );
}

pub(super) fn spawn_automation_visibility_menu_item(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    accessible_label: &str,
    display_label: &str,
    visible: bool,
    action: ChoreographyAction,
) {
    spawn_pointer_context_menu_custom_item(parent, accessible_label, action, |item| {
        item.spawn((
            Node {
                width: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|content| {
            content.spawn((
                Node {
                    width: Val::Px(14.0),
                    height: Val::Px(14.0),
                    flex_shrink: 0.0,
                    ..default()
                },
                UiSvg(load_svg_icon(
                    asset_server,
                    if visible {
                        "icons/show.svg"
                    } else {
                        "icons/hide.svg"
                    },
                )),
                SvgColor(theme::TEXT),
                Pickable::IGNORE,
            ));
            content.spawn((
                Text::new(display_label),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                TextLayout::no_wrap(),
                Pickable::IGNORE,
            ));
        });
    });
}

pub(super) fn spawn_emitter_context_menu(
    parent: &mut ChildSpawnerCommands,
    localizer: &Localizer,
    emitter: EmitterId,
    enabled: bool,
    soloed: bool,
    has_selected_regions: bool,
    position: Vec2,
) {
    spawn_pointer_context_menu(
        parent,
        position,
        TimelineTrackContextMenuAnchor,
        EmitterTrackContextMenu,
        |menu| {
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text(if enabled {
                    "timeline-menu-mute"
                } else {
                    "timeline-menu-unmute"
                }),
                ChoreographyAction::SetEmitterEnabled {
                    emitter,
                    enabled: !enabled,
                },
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text(if soloed {
                    "timeline-menu-unsolo"
                } else {
                    "timeline-menu-solo"
                }),
                ChoreographyAction::ToggleEmitterSolo(emitter),
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text("timeline-menu-create-reusable-effect"),
                crate::asset_actions::AssetAction::CreateReusableEffectFromSelection,
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text(if has_selected_regions {
                    "timeline-menu-duplicate-regions"
                } else {
                    "timeline-menu-duplicate"
                }),
                if has_selected_regions {
                    ChoreographyAction::DuplicateSelectedEmitterRegions
                } else {
                    ChoreographyAction::DuplicateEmitter(Some(emitter))
                },
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text(if has_selected_regions {
                    "timeline-menu-delete-regions"
                } else {
                    "timeline-menu-delete"
                }),
                if has_selected_regions {
                    ChoreographyAction::DeleteSelectedEmitterRegions
                } else {
                    ChoreographyAction::DeleteEmitter(Some(emitter))
                },
            );
        },
    );
}

pub(super) fn spawn_emitter_color_picker(
    parent: &mut ChildSpawnerCommands,
    localizer: &Localizer,
    emitter: EmitterId,
    display_color: Option<[f32; 4]>,
    automatic_color: [f32; 4],
) {
    parent
        .spawn((
            Popover {
                positions: color_picker_popover_positions(),
                window_margin: 9.0,
            },
            EmitterTrackColorPickerPopover,
            RelativeCursorPosition::default(),
            OverrideClip,
            GlobalZIndex(260),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(258.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(7.0),
                padding: UiRect::all(Val::Px(9.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER_BRIGHT),
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.0, 0.62),
                Val::Px(0.0),
                Val::Px(2.0),
                Val::Px(3.0),
                Val::Px(5.0),
            ),
        ))
        .with_children(|popup| {
            popup.spawn((
                Text::new(localizer.text("timeline-color-picker-title")),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Pickable::IGNORE,
            ));
            spawn_color_picker(
                popup,
                display_color,
                automatic_color,
                ColorPickerLabels {
                    accessible: localizer.text("timeline-color-picker-accessible"),
                    hue_saturation: localizer.text("timeline-color-picker-hue-saturation"),
                    lightness: localizer.text("timeline-color-picker-lightness"),
                    alpha: localizer.text("timeline-color-picker-alpha"),
                    automatic: localizer.text("timeline-color-picker-auto"),
                    rgb: localizer.text("timeline-color-picker-rgb"),
                    hsl: localizer.text("timeline-color-picker-hsl"),
                    red: localizer.text("timeline-color-picker-red"),
                    green: localizer.text("timeline-color-picker-green"),
                    blue: localizer.text("timeline-color-picker-blue"),
                    hue: localizer.text("timeline-color-picker-hue"),
                    saturation: localizer.text("timeline-color-picker-saturation"),
                    hex: localizer.text("timeline-color-picker-hex"),
                },
                EmitterTrackColorPicker { emitter },
            );
        });
}

pub(super) fn color_picker_popover_positions() -> Vec<PopoverPlacement> {
    [
        (PopoverSide::Right, PopoverAlign::End),
        (PopoverSide::Left, PopoverAlign::End),
        (PopoverSide::Top, PopoverAlign::Start),
        (PopoverSide::Right, PopoverAlign::Start),
        (PopoverSide::Left, PopoverAlign::Start),
        (PopoverSide::Bottom, PopoverAlign::Start),
    ]
    .into_iter()
    .map(|(side, align)| PopoverPlacement {
        side,
        align,
        gap: 7.0,
    })
    .collect()
}

pub(super) fn emitter_has_diagnostic(session: &EditorSession, index: usize) -> bool {
    let prefix = format!("effect.emitters[{index}]");
    session
        .diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.path.starts_with(&prefix))
}

pub(super) fn color_components(color: Color) -> [f32; 4] {
    let color = color.to_srgba();
    [color.red, color.green, color.blue, color.alpha]
}

#[cfg(test)]
#[path = "tests/emitter_composition.rs"]
mod tests;
