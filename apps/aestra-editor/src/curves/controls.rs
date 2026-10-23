//! Production Curves action consumer with a version-specific control-state boundary.
use super::property_editing::{ComplexKeyEdit, edit_complex_key, resolve_complex_input};
use super::state::{ComplexSelection, CurvesState};
use super::{CurvesAction, control_input, host_editing as host_motion};
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
use crate::{
    EditorModuleRegistry, EditorSession, ToolPanel, WorkspaceLayout, reveal_dock_panel, theme,
};
use aestra_core::Value;
use bevy::{prelude::*, ui_widgets::Activate};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    None,
    Hovered,
    Pressed,
}
pub(super) struct Control<'a> {
    pub(super) entity: Entity,
    pub(super) phase: Phase,
    pub(super) action: CurvesAction,
    pub(super) feathers: bool,
    pub(super) pending: bool,
    pub(super) background: Mut<'a, BackgroundColor>,
}

pub(super) fn queue_curves_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<CurvesAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        control_input::queue_pressed(&mut commands, activate.entity);
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn handle_curves_actions(
    mut commands: Commands,
    mut actions: control_input::Controls,
    mut session: ResMut<EditorSession>,
    registry: Res<EditorModuleRegistry>,
    mut state: ResMut<CurvesState>,
    mut layout: ResMut<WorkspaceLayout>,
    mut timeline: ResMut<crate::timeline::TimelineState>,
) {
    for Control {
        entity,
        phase,
        action,
        feathers,
        pending,
        mut background,
    } in actions.iter()
    {
        match phase {
            Phase::Hovered if !feathers => background.0 = theme::BUTTON_HOVER,
            Phase::None if !feathers => background.0 = theme::BUTTON,
            Phase::Pressed => {
                if feathers {
                    if !pending {
                        continue;
                    }
                    commands
                        .entity(entity)
                        .remove::<PendingFeathersActivation>();
                    control_input::clear_pressed(&mut commands, entity);
                } else {
                    background.0 = theme::ACCENT_DIM;
                }
                match action {
                    CurvesAction::OpenHost(channel) => {
                        host_motion::open(&mut session, &mut state, &mut timeline, channel);
                        reveal_dock_panel(&mut layout, &mut session, ToolPanel::Curves);
                    }
                    CurvesAction::HostAddKey => {
                        host_motion::add_key(&mut session, &mut state, &mut timeline)
                    }
                    CurvesAction::HostDeleteKey => {
                        host_motion::delete_key(&mut session, &mut state, &mut timeline)
                    }
                    CurvesAction::OpenInput(module, input) => {
                        state.host = None;
                        reveal_dock_panel(&mut layout, &mut session, ToolPanel::Curves);
                        state.complex = Some(ComplexSelection {
                            module,
                            input,
                            key: 0,
                        });
                        state.vector_channel = 0;
                        session.ui_revision += 1;
                    }
                    CurvesAction::SelectVectorChannel(channel) => {
                        state.vector_channel = channel.min(2);
                        if let Some(selection) = state.complex.as_mut() {
                            selection.key = 0;
                        }
                        session.ui_revision += 1;
                    }
                    CurvesAction::AddKey => {
                        edit_complex_key(&mut session, &registry.0, &mut state, ComplexKeyEdit::Add)
                    }
                    CurvesAction::SetInterpolation(interpolation) => {
                        if state.host.is_some() {
                            host_motion::set_interpolation(
                                &mut session,
                                &mut state,
                                &mut timeline,
                                interpolation,
                            );
                            continue;
                        }
                        if let Some(selection) = state.complex
                            && let Some((module, input, mut value)) =
                                resolve_complex_input(&session, &registry, selection)
                        {
                            let (module, parameter) = (module.id, input.name);
                            let curve = match &mut value {
                                Value::Curve(curve) => Some(curve),
                                Value::Vec3Curve(curves) => {
                                    curves.curves.get_mut(state.vector_channel as usize)
                                }
                                _ => None,
                            };
                            if let Some(curve) = curve
                                && curve.interpolation != interpolation
                            {
                                curve.interpolation = interpolation;
                                session.set_active_module_property_value(
                                    module,
                                    parameter,
                                    value,
                                    "Changed curve interpolation",
                                );
                            }
                        }
                    }
                    CurvesAction::DeleteKey => edit_complex_key(
                        &mut session,
                        &registry.0,
                        &mut state,
                        ComplexKeyEdit::Delete,
                    ),
                    CurvesAction::AdjustTime(direction) => edit_complex_key(
                        &mut session,
                        &registry.0,
                        &mut state,
                        ComplexKeyEdit::Time(direction),
                    ),
                    CurvesAction::AdjustCurveValue(direction) => edit_complex_key(
                        &mut session,
                        &registry.0,
                        &mut state,
                        ComplexKeyEdit::CurveValue(direction),
                    ),
                    CurvesAction::AdjustGradientChannel(channel, direction) => edit_complex_key(
                        &mut session,
                        &registry.0,
                        &mut state,
                        ComplexKeyEdit::GradientChannel(channel, direction),
                    ),
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "tests/controls.rs"]
mod tests;
