//! Production graph toolbar chrome, frame buttons and frame-action dispatch.
use super::{
    navigation::{FeathersGraphViewport, GraphFrameTarget},
    toolbar_input,
};
use crate::{
    feathers::{
        action_activation::{FeathersActionButton, PendingFeathersActivation, queue_activation},
        icon::{SvgColor, UiSvg, load_svg_icon},
        scenes,
        tooltip::EditorTooltip,
    },
    theme,
};
use bevy::{prelude::*, ui_widgets::Activate};
#[derive(Component, Debug, Clone)]
pub(crate) struct GraphFrameAction {
    pub(super) key: String,
    pub(super) target: GraphFrameTarget,
}

impl GraphFrameAction {
    pub(crate) fn new(key: impl Into<String>, target: GraphFrameTarget) -> Self {
        Self {
            key: key.into(),
            target,
        }
    }
}
#[cfg(test)]
impl GraphFrameAction {
    pub(crate) fn key_target(&self) -> (&str, GraphFrameTarget) {
        (&self.key, self.target)
    }
}

pub(crate) fn graph_toolbar_bundle() -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(38.0),
            min_height: Val::Px(38.0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(Val::Px(8.0)),
            column_gap: Val::Px(4.0),
            ..default()
        },
        BackgroundColor(theme::PANEL_LIGHT),
    )
}

pub(crate) fn spawn_graph_toolbar_summary(parent: &mut ChildSpawnerCommands, text: String) {
    parent.spawn(Node {
        flex_grow: 1.0,
        ..default()
    });
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(theme::TEXT_FAINT),
    ));
}

pub(crate) fn spawn_graph_tool_button<A: Component>(
    parent: &mut ChildSpawnerCommands,
    assets: &AssetServer,
    icon_path: &'static str,
    label: String,
    action: A,
) -> Entity {
    parent
        .spawn_empty()
        .apply_scene(scenes::feathers_tool_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.clone()),
            EditorTooltip::description(label),
            Node {
                width: Val::Px(28.0),
                height: Val::Px(24.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_child((
            Node {
                width: Val::Px(15.0),
                height: Val::Px(15.0),
                ..default()
            },
            UiSvg(load_svg_icon(assets, icon_path)),
            SvgColor(Color::WHITE),
            Pickable::IGNORE,
        ))
        .id()
}

pub(crate) fn spawn_graph_frame_button(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    icon_path: &'static str,
    label: String,
    action: GraphFrameAction,
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
                width: Val::Px(28.0),
                height: Val::Px(24.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ))
        .with_child((
            Node {
                width: Val::Px(15.0),
                height: Val::Px(15.0),
                ..default()
            },
            UiSvg(load_svg_icon(asset_server, icon_path)),
            SvgColor(Color::WHITE),
            Pickable::IGNORE,
        ));
    entity
}

pub(super) fn queue_graph_frame_activation(
    activate: On<Activate>,
    actions: Query<(), (With<FeathersActionButton>, With<GraphFrameAction>)>,
    commands: Commands,
) {
    queue_activation(activate, actions, commands);
}

pub(super) fn handle_graph_frame_buttons(
    mut commands: Commands,
    actions: toolbar_input::Controls,
    mut viewports: Query<&mut FeathersGraphViewport>,
) {
    for (entity, pressed, action, pending) in actions.iter() {
        if !pressed || !pending {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>();
        toolbar_input::clear_pressed(&mut commands, entity);
        for mut viewport in &mut viewports {
            if viewport.key == action.key {
                viewport.frame_request = Some(action.target);
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/toolbar.rs"]
mod tests;
