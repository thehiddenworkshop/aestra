//! Actual host-motion transient state and keyboard ownership, separated from UI execution.
use super::{EditorSession, TimelineState};
use aestra_authoring::SemanticTarget;
use aestra_core::{EffectId, HostTransformTrack};
use bevy::prelude::*;

#[derive(Debug, Default)]
pub(crate) struct HostMotionState {
    pub(super) effect: Option<EffectId>,
    pub(super) selected: Option<usize>,
    pub(super) drag: Option<KeyDrag>,
    pub(super) number_preview: Option<NumberPreview>,
    pub(super) cancelled_input: Option<Entity>,
    pub(super) released_key: Option<Entity>,
    pub(super) error: Option<String>,
}

#[derive(Debug)]
pub(super) struct NumberPreview {
    pub(super) entity: Entity,
    pub(super) effect: EffectId,
    pub(super) index: usize,
    pub(super) original: HostTransformTrack,
}

#[derive(Debug)]
pub(super) struct KeyDrag {
    pub(super) effect: EffectId,
    pub(super) index: usize,
    pub(super) original: HostTransformTrack,
    pub(super) candidate: HostTransformTrack,
    pub(super) selected: usize,
    pub(super) start: f32,
}

#[derive(Component, Event, Clone, Copy)]
pub(super) enum Action {
    Inspect,
    Add,
    Delete,
    Clear,
    Repeat(bool),
    CloseLoop,
    Select(usize),
}

pub(crate) fn active(session: &EditorSession, state: &TimelineState) -> bool {
    state.host_motion.effect == Some(session.effect.id)
        && session.selection.primary == SemanticTarget::Effect(session.effect.id)
        && state.inspected_child.is_none()
}

pub(super) fn keyboard_input(
    session: &EditorSession,
    state: &TimelineState,
    keys: &ButtonInput<KeyCode>,
    commands: &mut Commands,
) -> bool {
    if !active(session, state) {
        return false;
    }
    if keys.just_pressed(KeyCode::Delete) {
        commands.trigger(Action::Delete);
    } else if keys.just_pressed(KeyCode::Insert) {
        commands.trigger(Action::Add);
    }
    // Never fall through to emitter deletion/duplication while a pose is selected.
    true
}
