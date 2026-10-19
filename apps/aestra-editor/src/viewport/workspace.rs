//! Actual preview panel/toolbar construction and status-label synchronization.
use super::{
    PreviewDisplayMode, ViewportAction,
    gizmo_state::{ShapeGizmoValueLabel, TransformGizmoValueLabel},
    presentation::{
        GizmoModeLabel, PreviewDisplayModeIcon, PreviewPresentedEffect, TransformGizmoModeIcon,
    },
    runtime_preview::EditorPreviewProject,
    shortcuts::PreviewCanvas,
};
use crate::{
    feathers::{
        action_activation::FeathersActionButton,
        icon::{SvgColor, UiSvg, load_svg_icon},
        scenes as ui_shell,
        tooltip::EditorTooltip,
    },
    localization::Localizer,
    session::EditorSession,
    theme,
};
use aestra_bevy_render::{ActiveBackend, EffectRuntimeStatus};
use bevy::{gizmos::transform_gizmo::TransformGizmoMode, prelude::*, ui::RelativeCursorPosition};

const VIEWPORT_TOOL_BUTTON_SIZE: f32 = 28.0;
const VIEWPORT_TOOL_ICON_SIZE: f32 = 20.0;

#[derive(Component)]
pub(super) struct ParticleCountLabel;

pub(crate) fn spawn_preview(
    parent: &mut ChildSpawnerCommands,
    maximized: bool,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    parent
        .spawn(())
        .apply_scene(ui_shell::viewport_pane())
        .with_children(|column| {
            column
                .spawn((
                    PreviewCanvas,
                    Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        min_height: Val::Px(180.0),
                        position_type: PositionType::Relative,
                        overflow: Overflow::clip(),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    BorderColor::all(theme::BORDER_BRIGHT),
                    RelativeCursorPosition::default(),
                ))
                .with_children(|canvas| {
                    canvas.spawn((
                        Text::new("MOVE · WORLD"),
                        GizmoModeLabel,
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(12.0),
                            top: Val::Px(10.0),
                            ..default()
                        },
                    ));
                    canvas
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(8.0),
                                top: Val::Px(28.0),
                                padding: UiRect::all(Val::Px(2.0)),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(2.0),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(4.0)),
                                ..default()
                            },
                            BackgroundColor(theme::PANEL.with_alpha(0.92)),
                            BorderColor::all(theme::BORDER),
                        ))
                        .with_children(|tools| {
                            spawn_transform_gizmo_tool_button(
                                tools,
                                TransformGizmoMode::Translate,
                                "viewport-gizmo-move",
                                "viewport-gizmo-move-description",
                                localizer,
                                asset_server,
                            );
                            spawn_transform_gizmo_tool_button(
                                tools,
                                TransformGizmoMode::Rotate,
                                "viewport-gizmo-rotate",
                                "viewport-gizmo-rotate-description",
                                localizer,
                                asset_server,
                            );
                            spawn_transform_gizmo_tool_button(
                                tools,
                                TransformGizmoMode::Scale,
                                "viewport-gizmo-scale",
                                "viewport-gizmo-scale-description",
                                localizer,
                                asset_server,
                            );
                        });
                    canvas.spawn((
                        ShapeGizmoValueLabel,
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(12.0),
                            top: Val::Px(36.0),
                            padding: UiRect::axes(Val::Px(7.0), Val::Px(4.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(theme::PANEL.with_alpha(0.94)),
                        BorderColor::all(theme::ACCENT_DIM),
                        Visibility::Hidden,
                        Pickable::IGNORE,
                    ));
                    canvas.spawn((
                        TransformGizmoValueLabel,
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(12.0),
                            top: Val::Px(36.0),
                            padding: UiRect::axes(Val::Px(7.0), Val::Px(4.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(theme::PANEL.with_alpha(0.94)),
                        BorderColor::all(theme::ACCENT_DIM),
                        Visibility::Hidden,
                        Pickable::IGNORE,
                    ));
                    canvas
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                right: Val::Px(8.0),
                                top: Val::Px(5.0),
                                height: Val::Px(28.0),
                                padding: UiRect::all(Val::Px(2.0)),
                                column_gap: Val::Px(2.0),
                                align_items: AlignItems::Center,
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(4.0)),
                                ..default()
                            },
                            BackgroundColor(theme::PANEL.with_alpha(0.92)),
                            BorderColor::all(theme::BORDER),
                        ))
                        .with_children(|tools| {
                            spawn_viewport_tool_button(
                                tools,
                                ViewportAction::FramePreview,
                                "viewport-frame-effect",
                                "viewport-frame-effect-description",
                                ViewportToolIcon::Frame,
                                localizer,
                                asset_server,
                            );
                            tools.spawn((
                                Node {
                                    width: Val::Px(1.0),
                                    height: Val::Px(14.0),
                                    margin: UiRect::horizontal(Val::Px(2.0)),
                                    ..default()
                                },
                                BackgroundColor(theme::BORDER_BRIGHT),
                                Pickable::IGNORE,
                            ));
                            spawn_viewport_tool_button(
                                tools,
                                ViewportAction::SetPreviewDisplayMode(
                                    PreviewDisplayMode::Wireframe,
                                ),
                                "viewport-wireframe",
                                "viewport-wireframe-description",
                                ViewportToolIcon::Wireframe,
                                localizer,
                                asset_server,
                            );
                            spawn_viewport_tool_button(
                                tools,
                                ViewportAction::SetPreviewDisplayMode(PreviewDisplayMode::Rendered),
                                "viewport-rendered",
                                "viewport-rendered-description",
                                ViewportToolIcon::Rendered,
                                localizer,
                                asset_server,
                            );
                            tools.spawn((
                                Node {
                                    width: Val::Px(1.0),
                                    height: Val::Px(14.0),
                                    margin: UiRect::horizontal(Val::Px(2.0)),
                                    ..default()
                                },
                                BackgroundColor(theme::BORDER_BRIGHT),
                                Pickable::IGNORE,
                            ));
                            spawn_viewport_tool_button(
                                tools,
                                ViewportAction::ToggleMaximize,
                                if maximized {
                                    "viewport-reduce"
                                } else {
                                    "viewport-enlarge"
                                },
                                if maximized {
                                    "viewport-reduce-description"
                                } else {
                                    "viewport-enlarge-description"
                                },
                                ViewportToolIcon::Fullscreen { maximized },
                                localizer,
                                asset_server,
                            );
                        });
                });
            column.spawn((
                Text::new("0 LIVE PARTICLES  |  60 FPS"),
                ParticleCountLabel,
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
}

pub(super) fn update_viewport_status_label(
    session: Res<EditorSession>,
    preview: Res<EditorPreviewProject>,
    preview_runtime: Query<Ref<EffectRuntimeStatus>, With<PreviewPresentedEffect>>,
    mut labels: Query<&mut Text, With<ParticleCountLabel>>,
) {
    let runtime_changed = preview_runtime
        .iter()
        .any(|runtime| runtime.is_added() || runtime.is_changed());
    if !session.is_changed() && !preview.is_changed() && !runtime_changed {
        return;
    }
    if let Some(error) = &preview.error {
        for mut text in &mut labels {
            text.0 = format!("PREVIEW ERROR  |  {error}");
        }
        return;
    }
    let backend = preview_runtime
        .iter()
        .next()
        .map_or("DETECTING GPU", |runtime| match runtime.active {
            ActiveBackend::Pending => "DETECTING GPU",
            ActiveBackend::Gpu => "NATIVE GPU",
            ActiveBackend::GpuReadback => "GPU READBACK",
            ActiveBackend::CpuReference => "CPU FALLBACK",
        });
    for mut text in &mut labels {
        text.0 = format!(
            "{} LIVE PARTICLES  |  {backend}",
            preview.live_particle_count
        );
    }
}

#[derive(Clone, Copy)]
enum ViewportToolIcon {
    Frame,
    Wireframe,
    Rendered,
    Fullscreen { maximized: bool },
}

fn spawn_transform_gizmo_tool_button(
    parent: &mut ChildSpawnerCommands,
    mode: TransformGizmoMode,
    label_id: &'static str,
    description_id: &'static str,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    let label = localizer.text(label_id);
    let shortcut = match mode {
        TransformGizmoMode::Translate => "1",
        TransformGizmoMode::Rotate => "2",
        TransformGizmoMode::Scale => "3",
    };
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_tool_button())
        .insert((
            ViewportAction::SetTransformGizmoMode(mode),
            FeathersActionButton,
            AccessibleLabel(label.clone()),
            EditorTooltip::titled(label, localizer.text(description_id)).with_shortcut(shortcut),
            Node {
                width: Val::Px(VIEWPORT_TOOL_BUTTON_SIZE),
                min_width: Val::Px(VIEWPORT_TOOL_BUTTON_SIZE),
                height: Val::Px(VIEWPORT_TOOL_BUTTON_SIZE),
                padding: UiRect::ZERO,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|button| {
            let path = match mode {
                TransformGizmoMode::Translate => "icons/move.svg",
                TransformGizmoMode::Rotate => "icons/rotate.svg",
                TransformGizmoMode::Scale => "icons/scale.svg",
            };
            spawn_viewport_svg_icon(button, asset_server, path)
                .insert(TransformGizmoModeIcon(mode));
        });
}

fn spawn_viewport_tool_button(
    parent: &mut ChildSpawnerCommands,
    action: ViewportAction,
    label_id: &'static str,
    description_id: &'static str,
    icon: ViewportToolIcon,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_tool_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(localizer.text(label_id)),
            EditorTooltip::description(localizer.text(description_id)),
            Node {
                width: Val::Px(VIEWPORT_TOOL_BUTTON_SIZE),
                min_width: Val::Px(VIEWPORT_TOOL_BUTTON_SIZE),
                height: Val::Px(VIEWPORT_TOOL_BUTTON_SIZE),
                padding: UiRect::ZERO,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|button| match icon {
            ViewportToolIcon::Frame => {
                spawn_viewport_svg_icon(button, asset_server, "icons/center-focus.svg");
            }
            ViewportToolIcon::Wireframe | ViewportToolIcon::Rendered => {
                let mode = if matches!(icon, ViewportToolIcon::Wireframe) {
                    PreviewDisplayMode::Wireframe
                } else {
                    PreviewDisplayMode::Rendered
                };
                let path = if mode == PreviewDisplayMode::Wireframe {
                    "icons/wireframe.svg"
                } else {
                    "icons/solid.svg"
                };
                spawn_viewport_svg_icon(button, asset_server, path)
                    .insert(PreviewDisplayModeIcon(mode));
            }
            ViewportToolIcon::Fullscreen { maximized } => {
                let path = if maximized {
                    "icons/fullscreen-exit-alt.svg"
                } else {
                    "icons/fullscreen-alt.svg"
                };
                spawn_viewport_svg_icon(button, asset_server, path);
            }
        });
}

fn spawn_viewport_svg_icon<'a>(
    parent: &'a mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    path: &'static str,
) -> EntityCommands<'a> {
    let icon = load_svg_icon(asset_server, path);
    parent.spawn((
        Node {
            width: Val::Px(VIEWPORT_TOOL_ICON_SIZE),
            height: Val::Px(VIEWPORT_TOOL_ICON_SIZE),
            ..default()
        },
        UiSvg(icon),
        SvgColor(theme::TEXT),
        Pickable::IGNORE,
    ))
}

#[cfg(test)]
#[path = "workspace/tests.rs"]
mod tests;
