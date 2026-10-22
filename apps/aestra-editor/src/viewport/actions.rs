//! Production viewport action queue, dispatch, execution and display state.
use super::{PreviewCameraController, PreviewDisplayMode, ViewportAction, control_input};
use crate::{
    EditorSession,
    docking::ToolPanel,
    feathers::action_activation::{
        EditorNativeControl, FeathersActionButton, PendingFeathersActivation,
    },
    localization::Localizer,
    menus::MenuState,
    persistence::persist_editor_settings,
    settings::{EditorSettings, SettingsPersistence},
};
use bevy::{gizmos::transform_gizmo::TransformGizmoSettings, prelude::*, ui_widgets::Activate};

pub(super) struct Control {
    pub(super) entity: Entity,
    pub(super) pressed: bool,
    pub(super) action: ViewportAction,
    pub(super) pending: bool,
}

#[derive(Resource, Default)]
pub(crate) struct PreviewDisplayState {
    pub(super) mode: PreviewDisplayMode,
}

impl PreviewDisplayState {
    pub(crate) fn set_mode(&mut self, mode: PreviewDisplayMode) {
        self.mode = mode;
    }
}

pub(super) fn queue_viewport_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<ViewportAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        control_input::queue_pressed(&mut commands, activate.entity);
    }
}

pub(super) fn handle_viewport_action_buttons(
    mut commands: Commands,
    mut buttons: control_input::Controls,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    for Control {
        entity,
        pressed,
        action,
        pending,
    } in buttons.iter()
    {
        if !pressed || !pending {
            continue;
        }
        commands
            .entity(entity)
            .remove::<PendingFeathersActivation>();
        control_input::clear_pressed(&mut commands, entity);
        menu.open = None;
        menu.panels_open = false;
        if menu.tab_context.take().is_some() {
            session.ui_revision += 1;
        }
        commands.trigger(action);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn execute_viewport_action(
    action: On<ViewportAction>,
    mut session: ResMut<EditorSession>,
    mut menu: ResMut<MenuState>,
    mut settings: ResMut<EditorSettings>,
    mut settings_persistence: ResMut<SettingsPersistence>,
    localizer: Res<Localizer>,
    mut preview_camera: ResMut<PreviewCameraController>,
    mut preview_display: ResMut<PreviewDisplayState>,
    mut transform_gizmo_settings: ResMut<TransformGizmoSettings>,
    mut maximized: ResMut<crate::docking::MaximizedPanel>,
) {
    match *action {
        ViewportAction::ToggleGrid => {
            menu.show_grid = !menu.show_grid;
            settings.preview.show_grid = menu.show_grid;
            session.ui_revision += 1;
            persist_editor_settings(
                &settings,
                &mut settings_persistence,
                &mut session,
                &localizer,
            );
        }
        ViewportAction::FramePreview => preview_camera.request_frame(),
        ViewportAction::SetTransformGizmoMode(mode) => {
            transform_gizmo_settings.mode = mode;
        }
        ViewportAction::SetPreviewDisplayMode(mode) => preview_display.set_mode(mode),
        ViewportAction::ToggleMaximize => {
            maximized.0 = if maximized.0 == Some(ToolPanel::Viewport) {
                None
            } else {
                Some(ToolPanel::Viewport)
            };
            session.ui_revision += 1;
        }
    }
}

type UnclassifiedViewportControl = (
    Added<ViewportAction>,
    With<control_input::AuditedButton>,
    Without<FeathersActionButton>,
    Without<EditorNativeControl>,
);

pub(super) fn audit_viewport_controls(controls: Query<Entity, UnclassifiedViewportControl>) {
    if cfg!(debug_assertions)
        && let Some(entity) = controls.iter().next()
    {
        panic!(
            "viewport control {entity:?} must use FeathersActionButton or be explicitly marked \
             EditorNativeControl"
        );
    }
}

#[cfg(test)]
#[path = "actions/tests.rs"]
mod tests;
