//! Production timeline toolbar composition and shared header-control helpers.
use super::interaction_state::*;
use super::{TimelineAction, TimelineSnapMode, TimelineState};
use crate::feathers::{
    action_activation::FeathersActionButton,
    button::spawn_tool_button as mini_button,
    combo_box::{ComboOption, spawn_combo_control},
    icon::{SvgColor, UiSvg, load_svg_icon},
    scenes as ui_shell,
    tooltip::EditorTooltip,
};
use crate::{EditorSession, localization::Localizer, theme, transport::TransportAction};
use bevy::{prelude::*, ui::RelativeCursorPosition};
use fluent_bundle::FluentArgs;

pub(super) fn spawn_timeline_toolbar(
    timeline: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &TimelineState,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    timeline
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                height: Val::Px(38.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(14.0)),
                column_gap: Val::Px(14.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme::PANEL_LIGHT),
        ))
        .with_children(|header| {
            header.spawn((
                Text::new("00:00.000  /  00:02.800"),
                TimeLabel,
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::ACCENT),
            ));
            let previous = mini_button(header, "<", TransportAction::StepFrame(-1));
            describe_timeline_control(header, previous, localizer.text("timeline-previous-frame"));
            let next = mini_button(header, ">", TransportAction::StepFrame(1));
            describe_timeline_control(header, next, localizer.text("timeline-next-frame"));
            timeline_icon_button(
                header,
                asset_server,
                "icons/center-focus.svg",
                localizer.text("timeline-frame-all"),
                TimelineAction::FrameAll,
            );
            timeline_icon_button(
                header,
                asset_server,
                "icons/marker.svg",
                localizer.text("timeline-add-marker"),
                TimelineAction::AddMarker,
            );
            timeline_icon_button(
                header,
                asset_server,
                "icons/event.svg",
                localizer.text("timeline-add-event"),
                TimelineAction::AddChoreographyEvent,
            );
            let cut_tool = timeline_icon_button(
                header,
                asset_server,
                "icons/split-h.svg",
                "Split the selected emitter region at the playhead".into(),
                TimelineAction::SplitEmitterRegion,
            );
            header
                .commands()
                .entity(cut_tool)
                .insert((TimelineRegionToolButton, RelativeCursorPosition::default()));
            let merge_tool = timeline_icon_button(
                header,
                asset_server,
                "icons/merge.svg",
                "Consolidate selected emitter regions".into(),
                TimelineAction::JoinEmitterRegion,
            );
            header
                .commands()
                .entity(merge_tool)
                .insert((TimelineRegionToolButton, RelativeCursorPosition::default()));
            let snap_options = TimelineSnapMode::ALL
                .into_iter()
                .map(|mode| ComboOption {
                    label: localizer.text(mode.message_id()),
                    selected: state.snap == mode,
                    action: TimelineAction::SetSnap(mode),
                })
                .collect::<Vec<_>>();
            spawn_combo_control(
                header,
                &localizer.text(state.snap.message_id()),
                &localizer.text("timeline-snapping-description"),
                &snap_options,
                112.0,
            );
            header.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            header.spawn((
                Text::new(format!(
                    "{} {}",
                    session.tick_rate(),
                    localizer.text("timeline-hertz")
                )),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
            header.spawn((
                Text::new(format!(
                    "{} {:.2}s",
                    localizer.text("timeline-duration"),
                    session.playback_duration()
                )),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
            ));
            let decrease_duration =
                mini_button(header, "-", TimelineAction::AdjustEffectDuration(-0.25));
            describe_timeline_control(
                header,
                decrease_duration,
                localizer.text("timeline-decrease-duration"),
            );
            let increase_duration =
                mini_button(header, "+", TimelineAction::AdjustEffectDuration(0.25));
            describe_timeline_control(
                header,
                increase_duration,
                localizer.text("timeline-increase-duration"),
            );
        });
}

pub(super) fn describe_timeline_control(
    parent: &mut ChildSpawnerCommands,
    entity: Entity,
    description: String,
) {
    parent.commands().entity(entity).insert((
        AccessibleLabel(description.clone()),
        EditorTooltip::description(description),
    ));
}

pub(super) fn timeline_icon_button(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    icon_path: &'static str,
    label: String,
    action: TimelineAction,
) -> Entity {
    let mut button = parent.spawn_empty();
    button.apply_scene(ui_shell::feathers_tool_button());
    let entity = button.id();
    button
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.clone()),
            EditorTooltip::description(label),
            Node {
                width: Val::Px(28.0),
                height: Val::Px(26.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_child((
            Node {
                width: Val::Px(16.0),
                height: Val::Px(16.0),
                ..default()
            },
            UiSvg(load_svg_icon(asset_server, icon_path)),
            SvgColor(theme::TEXT),
            Pickable::IGNORE,
        ));
    entity
}

pub(super) fn timeline_effect_heading(localizer: &Localizer, name: &str) -> String {
    let mut args = FluentArgs::new();
    args.set("name", name);
    localizer.text_with("timeline-effect-emitters", &args)
}

#[cfg(test)]
#[path = "tests/toolbar_composition.rs"]
mod tests;
