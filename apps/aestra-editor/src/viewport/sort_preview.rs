//! Sort placement for active preview instances in the editor's native 2D scene.

use super::{
    PreviewCameraController, PreviewDisplayState, ViewportAction, pixel_preview::PixelSceneCamera,
    presentation::PreviewPresentedEffect, scene::PreviewXyCamera,
};
use crate::{
    Localizer, ScrollMemoryKey, feathers::button::spawn_action_button,
    feathers::scroll::spawn_vertical_scroll_area, theme,
};
use aestra_bevy_render::{AestraSort2d, PresentedEffect};
use bevy::{
    camera::visibility::RenderLayers,
    feathers::controls::ButtonVariant,
    math::FloatOrd,
    prelude::*,
    ui::{RelativeCursorPosition, Selected},
};
use fluent_bundle::FluentArgs;
use std::cmp::Reverse;

#[derive(Component)]
pub(super) struct SortOverlay;

#[derive(Component)]
pub(super) struct SortPanel;

#[derive(Component)]
pub(super) struct SortRows;

#[derive(Component)]
pub(super) struct SavedSortPolicy(Option<AestraSort2d>);

pub(super) fn spawn_controls(parent: &mut ChildSpawnerCommands, localizer: &Localizer) {
    parent
        .spawn((
            SortOverlay,
            RelativeCursorPosition::default(),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                bottom: Val::Px(8.0),
                right: Val::Px(8.0),
                max_width: Val::Percent(88.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: Val::Px(5.0),
                ..default()
            },
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    SortPanel,
                    Node {
                        display: Display::None,
                        width: Val::Px(320.0),
                        max_width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(6.0),
                        padding: UiRect::all(Val::Px(8.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                    BorderColor::all(theme::BORDER_BRIGHT),
                ))
                .with_children(|panel| {
                    panel
                        .spawn(Node {
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: Val::Px(8.0),
                            row_gap: Val::Px(4.0),
                            ..default()
                        })
                        .with_children(|header| {
                            header.spawn((
                                Text::new(localizer.text("viewport-sort-front-to-back")),
                                TextFont::from_font_size(12.0),
                                TextColor(theme::TEXT),
                            ));
                            spawn_action_button(
                                header,
                                &localizer.text("viewport-sort-by-y"),
                                ViewportAction::ToggleYSortPreview,
                                false,
                            );
                        });
                    panel.spawn(Node::default()).with_children(|body| {
                        spawn_vertical_scroll_area(
                            body,
                            ScrollMemoryKey::PreviewSortOrder,
                            Node {
                                max_height: Val::Px(88.0),
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                ..default()
                            },
                            |rows| {
                                rows.spawn((
                                    SortRows,
                                    Text::default(),
                                    TextFont::from_font_size(11.0),
                                    TextColor(theme::TEXT_MUTED),
                                    Pickable::IGNORE,
                                ));
                            },
                        );
                    });
                });
            spawn_action_button(
                overlay,
                &localizer.text("viewport-sort-order"),
                ViewportAction::ToggleSortOrder,
                false,
            );
        });
}

pub(super) fn sync_preview_sort_policy(
    mut commands: Commands,
    controller: Res<PreviewCameraController>,
    display: Res<PreviewDisplayState>,
    effects: Query<
        (Entity, Option<&AestraSort2d>, Option<&SavedSortPolicy>),
        With<PreviewPresentedEffect>,
    >,
) {
    let preview_y = panel_open(&controller, &display) && display.preview_y_sort;
    for (entity, policy, saved) in &effects {
        match (preview_y, saved) {
            (true, None) => {
                commands.entity(entity).insert((
                    SavedSortPolicy(policy.copied()),
                    AestraSort2d::WorldY { units_per_y: 1.0 },
                ));
            }
            (false, Some(saved)) => {
                let mut entity = commands.entity(entity);
                entity.remove::<SavedSortPolicy>();
                if let Some(policy) = saved.0 {
                    entity.insert(policy);
                } else {
                    entity.remove::<AestraSort2d>();
                }
            }
            _ => {}
        }
    }
}

pub(super) fn sync_controls(
    mut commands: Commands,
    controller: Res<PreviewCameraController>,
    display: Res<PreviewDisplayState>,
    mut nodes: Query<(&mut Node, Has<SortPanel>), Or<(With<SortOverlay>, With<SortPanel>)>>,
    buttons: Query<(Entity, &ViewportAction, Has<Selected>)>,
) {
    let available = display.sort_order_available(controller.xy);
    let open = panel_open(&controller, &display);
    for (node, panel) in &mut nodes {
        let visible = if panel { open } else { available };
        node.map_unchanged(|node| &mut node.display)
            .set_if_neq(if visible {
                Display::Flex
            } else {
                Display::None
            });
    }
    for (entity, action, selected) in &buttons {
        let active = match *action {
            ViewportAction::ToggleSortOrder => open,
            ViewportAction::ToggleYSortPreview => open && display.preview_y_sort,
            _ => continue,
        };
        if active && !selected {
            commands
                .entity(entity)
                .insert((Selected, ButtonVariant::Primary));
        } else if !active && selected {
            commands
                .entity(entity)
                .remove::<Selected>()
                .insert(ButtonVariant::Normal);
        }
    }
}

pub(super) fn update_rows(
    controller: Res<PreviewCameraController>,
    display: Res<PreviewDisplayState>,
    localizer: Res<Localizer>,
    cameras: Query<(&Camera, &RenderLayers), Or<(With<PreviewXyCamera>, With<PixelSceneCamera>)>>,
    effects: Query<
        (
            Entity,
            &PresentedEffect,
            &GlobalTransform,
            &InheritedVisibility,
            Option<&RenderLayers>,
            Option<&AestraSort2d>,
        ),
        With<PreviewPresentedEffect>,
    >,
    mut rows: Query<&mut Text, With<SortRows>>,
) {
    if !panel_open(&controller, &display) || rows.is_empty() {
        return;
    }
    let mut sorted = Vec::new();
    if let Some((_, camera_layers)) = cameras.iter().find(|(camera, _)| camera.is_active) {
        for (entity, effect, transform, visible, layers, policy) in &effects {
            let layers = layers.unwrap_or_default();
            if !visible.get() || !layers.intersects(camera_layers) {
                continue;
            }
            let policy = policy.copied().unwrap_or_default();
            let position = effect.world_from_effect(transform).w_axis.truncate();
            sorted.push((
                FloatOrd(policy.sort_key(position)),
                entity,
                effect,
                policy,
                layers,
            ));
        }
    }
    // Match the native phase's ascending key and owner order, then display the front first.
    sorted.sort_unstable_by_key(|&(key, entity, ..)| Reverse((key, entity)));
    let mut message = String::new();
    for (key, _, effect, policy, layers) in sorted {
        let mode = match policy {
            AestraSort2d::WorldZ => localizer.text("viewport-sort-world-z"),
            AestraSort2d::WorldY { .. } => localizer.text("viewport-sort-by-y"),
            AestraSort2d::Custom(_) => localizer.text("viewport-sort-custom"),
        };
        let layers = layers
            .iter()
            .map(|layer| layer.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let mut args = FluentArgs::new();
        args.set("key", format!("{:.2}", key.0));
        args.set("mode", mode);
        args.set("layers", layers);
        args.set("name", effect.effect().name.as_str());
        if !message.is_empty() {
            message.push('\n');
        }
        message.push_str(&localizer.text_with("viewport-sort-order-row", &args));
    }
    if message.is_empty() {
        message = localizer.text("viewport-sort-order-empty");
    }
    for mut text in &mut rows {
        if text.0 != message {
            text.0.clone_from(&message);
        }
    }
}

fn panel_open(controller: &PreviewCameraController, display: &PreviewDisplayState) -> bool {
    display.sort_order_available(controller.xy) && display.show_sort_order
}

#[cfg(test)]
#[path = "tests/sort_preview.rs"]
mod tests;
