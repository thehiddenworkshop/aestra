//! Production host-motion actions and numeric edit transactions; UI composition stays native.
use super::motion_state::{Action, NumberPreview, active};
use super::{EditorSession, TimelineState};
use aestra_authoring::{EffectCommand, EffectTransaction, SemanticTarget};
use aestra_core::{EffectId, EmitterTransform, HostTransformKey, HostTransformTrack};
use bevy::{prelude::*, ui_widgets::ValueChange};

pub(super) fn cancel_drag(
    keys: Res<ButtonInput<KeyCode>>,
    controls: Query<(), With<NumberControl>>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    let stale = state.host_motion.drag.as_ref().is_some_and(|drag| {
        drag.effect != session.effect.id
            || session.effect.host_transform_track.as_ref() != Some(&drag.original)
    });
    if (stale || keys.just_pressed(KeyCode::Escape)) && state.host_motion.drag.take().is_some() {
        session.restore_interaction_preview();
        session.ui_revision += 1;
    }
    let cancel_number = state
        .host_motion
        .number_preview
        .as_ref()
        .is_some_and(|preview| {
            keys.just_pressed(KeyCode::Escape)
                || !active(&session, &state)
                || preview.effect != session.effect.id
                || state.host_motion.selected != Some(preview.index)
                || session.effect.host_transform_track.as_ref() != Some(&preview.original)
                || !controls.contains(preview.entity)
        });
    if cancel_number {
        state.host_motion.cancelled_input =
            state.host_motion.number_preview.take().map(|p| p.entity);
        session.restore_interaction_preview();
        session.ui_revision += 1;
    }
    if state
        .host_motion
        .cancelled_input
        .is_some_and(|entity| !controls.contains(entity))
    {
        state.host_motion.cancelled_input = None;
    }
}

#[derive(Component, Clone, Copy)]
pub(super) struct NumberControl {
    pub(super) effect: EffectId,
    pub(super) index: usize,
    pub(super) field: Field,
    pub(super) initial: f32,
}

#[derive(Clone, Copy)]
pub(super) enum Field {
    Time,
    Position(usize),
    Rotation(usize),
    Scale(usize),
}

/// View bounds include motion endpoints without extending semantic effect playback.
pub(super) fn timeline_duration(session: &EditorSession) -> f32 {
    session
        .effect
        .host_transform_track
        .as_ref()
        .map_or(session.playback_duration(), |track| {
            session.playback_duration().max(track.end_time())
        })
}

pub(crate) fn selected_pose(
    session: &EditorSession,
    state: &TimelineState,
) -> Option<(usize, EmitterTransform)> {
    if !active(session, state) {
        return None;
    }
    let index = state.host_motion.selected?;
    let key = session
        .effect
        .host_transform_track
        .as_ref()?
        .keys()
        .get(index)?
        .clone();
    Some((index, key.transform))
}

pub(crate) fn busy(state: &TimelineState) -> bool {
    state.host_motion.drag.is_some() || state.host_motion.number_preview.is_some()
}

pub(crate) fn select_pose(session: &mut EditorSession, state: &mut TimelineState, index: usize) {
    if session
        .effect
        .host_transform_track
        .as_ref()
        .is_some_and(|track| index < track.keys().len())
    {
        select(session, state, Some(index));
    }
}

pub(crate) fn replace_pose(
    track: &HostTransformTrack,
    index: usize,
    transform: EmitterTransform,
) -> Result<HostTransformTrack, String> {
    let mut candidate = track.clone();
    if !transform.is_valid() {
        return Err("Invalid transform pose".into());
    }
    let time = candidate
        .keys()
        .get(index)
        .ok_or("The pose key no longer exists")?
        .time;
    candidate.set_pose(time, transform);
    candidate.validate().map_err(|e| e.to_string())?;
    Ok(candidate)
}

pub(super) fn select(session: &mut EditorSession, state: &mut TimelineState, index: Option<usize>) {
    state.host_motion.effect = Some(session.effect.id);
    state.host_motion.selected = index;
    state.host_motion.error = None;
    state.inspected_child = None;
    state.clear_emitter_selection();
    state.selected_automation_key = None;
    session.selected_emitter_region = None;
    session.selection.primary = SemanticTarget::Effect(session.effect.id);
    session.ui_revision += 1;
}

pub(super) fn fail(
    session: &mut EditorSession,
    state: &mut TimelineState,
    error: impl Into<String>,
) {
    let error = error.into();
    session.status = error.clone();
    state.host_motion.error = Some(error);
    session.ui_revision += 1;
}

pub(super) fn commit(
    session: &mut EditorSession,
    state: &mut TimelineState,
    track: Option<HostTransformTrack>,
    index: Option<usize>,
) {
    if let Some(track) = &track
        && let Err(error) = track.validate()
    {
        fail(session, state, error.to_string());
        session.restore_interaction_preview();
        return;
    }
    if track == session.effect.host_transform_track {
        session.restore_interaction_preview();
        // Feather can report the unchanged value on focus loss. Do not rebuild the
        // focused control in response: that would generate another focus loss.
        if !active(session, state) || state.host_motion.selected != index {
            select(session, state, index);
        }
    } else if session.execute(
        "Edit host motion",
        EffectCommand::SetHostTransformTrack { track },
        true,
    ) {
        select(session, state, index);
    } else {
        session.restore_interaction_preview();
    }
}

/// Adding at an existing time selects that key instead of creating a duplicate.
pub(super) fn add_key(
    track: Option<&HostTransformTrack>,
    time: f32,
) -> (HostTransformTrack, usize) {
    let transform = track
        .and_then(|track| aestra_runtime::CompiledHostTransformTrack::new(track.clone()).ok())
        .map_or_else(EmitterTransform::default, |track| track.sample(time));
    let mut track = track.cloned().unwrap_or(HostTransformTrack::from_pose_keys(
        vec![HostTransformKey {
            time: 0.0,
            transform,
        }],
        false,
    ));
    // Continuous playback addresses a repeating trajectory by its own period.
    let time = if track.repeat {
        let end = track.keys().last().unwrap().time;
        if time > end {
            time.rem_euclid(end)
        } else {
            time
        }
    } else {
        time.max(0.0)
    };
    if let Some(index) = track
        .keys()
        .iter()
        .position(|key| (key.time - time).abs() < 1e-5)
    {
        return (track, index);
    }
    track.curves.set_pose(time, transform, true);
    let index = track
        .keys()
        .iter()
        .position(|key| key.time == time)
        .unwrap();
    (track, index)
}

pub(super) fn move_key(
    track: &HostTransformTrack,
    index: usize,
    time: f32,
) -> Result<(HostTransformTrack, usize), String> {
    if index >= track.keys().len() || !time.is_finite() || time < 0.0 {
        return Err("Invalid key time".into());
    }
    if index == 0 && time != 0.0 {
        return Err("The first pose key must stay at zero".into());
    }
    if track
        .keys()
        .iter()
        .enumerate()
        .any(|(i, key)| i != index && (key.time - time).abs() < 1e-5)
    {
        return Err("A pose key already exists at that time".into());
    }
    if track.repeat && index != track.keys().len() - 1 && time >= track.keys().last().unwrap().time
    {
        return Err("An interior key must stay before the loop endpoint".into());
    }
    if track.repeat && index == track.keys().len() - 1 && time <= track.keys()[index - 1].time {
        return Err("The loop endpoint must stay after all interior keys".into());
    }
    let mut candidate = track.clone();
    candidate.curves.retime(track.keys()[index].time, time);
    candidate.validate().map_err(|e| e.to_string())?;
    let selected = candidate
        .keys()
        .iter()
        .position(|key| key.time == time)
        .unwrap();
    Ok((candidate, selected))
}

pub(super) fn edit_pose(
    track: &HostTransformTrack,
    index: usize,
    field: Field,
    value: f32,
) -> Result<HostTransformTrack, String> {
    let mut key = track
        .keys()
        .get(index)
        .ok_or("The pose key no longer exists")?
        .clone();
    match field {
        Field::Position(axis) => key.transform.translation[axis] = value,
        Field::Scale(axis) => key.transform.scale[axis] = value,
        Field::Rotation(axis) => {
            let (x, y, z) = Quat::from_array(key.transform.rotation).to_euler(EulerRot::XYZ);
            let mut angles = [x, y, z];
            angles[axis] = value.to_radians();
            key.transform.rotation =
                Quat::from_euler(EulerRot::XYZ, angles[0], angles[1], angles[2]).to_array();
        }
        Field::Time => return Err("Use the key-time editor to retime a pose".into()),
    }
    replace_pose(track, index, key.transform)
}

pub(super) fn execute(
    event: On<Action>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    if !matches!(*event, Action::Inspect | Action::Add | Action::Select(_))
        && !active(&session, &state)
    {
        return;
    }
    let current = session.effect.host_transform_track.clone();
    match *event {
        Action::Inspect => {
            let index = current.as_ref().and_then(|t| {
                t.keys()
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        (a.time - session.time())
                            .abs()
                            .total_cmp(&(b.time - session.time()).abs())
                    })
                    .map(|(i, _)| i)
            });
            select(&mut session, &mut state, index);
        }
        Action::Select(index) => {
            if current.as_ref().is_some_and(|t| index < t.keys().len()) {
                select(&mut session, &mut state, Some(index));
            }
        }
        Action::Add => {
            let (track, index) = add_key(current.as_ref(), session.time());
            commit(&mut session, &mut state, Some(track), Some(index));
        }
        Action::Clear => commit(&mut session, &mut state, None, None),
        Action::Delete => {
            let Some(mut track) = current else {
                return;
            };
            let Some(index) = state
                .host_motion
                .selected
                .filter(|i| *i < track.keys().len())
            else {
                return;
            };
            if track.keys().len() == 1 {
                commit(&mut session, &mut state, None, None);
            } else if index == 0 {
                fail(
                    &mut session,
                    &mut state,
                    "The first key anchors time zero. Use Clear motion to remove the track.",
                );
            } else {
                track.curves.remove_time(track.keys()[index].time);
                let index = index.min(track.keys().len() - 1);
                commit(&mut session, &mut state, Some(track), Some(index));
            }
        }
        Action::Repeat(repeat) => {
            let Some(mut track) = current else {
                return;
            };
            track.repeat = repeat;
            let index = state.host_motion.selected;
            if repeat && track.validate().is_err() {
                fail(
                    &mut session,
                    &mut state,
                    "Repeat needs matching endpoint poses and a positive period. Use Close loop first.",
                );
            } else {
                commit(&mut session, &mut state, Some(track), index);
            }
        }
        Action::CloseLoop => {
            let Some(mut track) = current else {
                return;
            };
            let first = track.keys()[0].transform;
            if track.keys().len() == 1 {
                track.curves.set_pose(session.effect.duration, first, true);
            } else {
                track.curves.set_pose(track.end_time(), first, false);
            }
            track.repeat = true;
            let index = state.host_motion.selected;
            commit(&mut session, &mut state, Some(track), index);
        }
    }
}

pub(super) fn number_changed(
    event: On<ValueChange<f32>>,
    controls: Query<&NumberControl>,
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    let Ok(control) = controls.get(event.source) else {
        return;
    };
    if control.effect != session.effect.id
        || !active(&session, &state)
        || state.host_motion.selected != Some(control.index)
        || state.host_motion.cancelled_input == Some(event.source)
    {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        state.host_motion.cancelled_input = Some(event.source);
        state.host_motion.number_preview = None;
        session.restore_interaction_preview();
        session.ui_revision += 1;
        return;
    }
    // Programmatic Feather text initialization also emits ValueChange. Ignore the
    // authored value, including rotation's Euler/quaternion round-trip noise.
    if event.value == control.initial {
        if state
            .host_motion
            .number_preview
            .as_ref()
            .is_some_and(|p| p.entity == event.source)
        {
            state.host_motion.number_preview = None;
            session.restore_interaction_preview();
        }
        return;
    }
    let Some(track) = session.effect.host_transform_track.as_ref() else {
        return;
    };
    let result = match control.field {
        Field::Time => move_key(track, control.index, event.value),
        field => edit_pose(track, control.index, field, event.value).map(|t| (t, control.index)),
    };
    let original = track.clone();
    match result {
        Ok((track, index)) if event.is_final => {
            state.host_motion.number_preview = None;
            commit(&mut session, &mut state, Some(track), Some(index))
        }
        Ok((track, _)) => {
            if session.preview_interaction(EffectTransaction::single(
                "Preview host pose",
                EffectCommand::SetHostTransformTrack { track: Some(track) },
            )) {
                state.host_motion.number_preview = Some(NumberPreview {
                    entity: event.source,
                    effect: control.effect,
                    index: control.index,
                    original,
                });
            }
        }
        Err(error) if event.is_final => {
            state.host_motion.number_preview = None;
            fail(&mut session, &mut state, error);
            session.restore_interaction_preview();
        }
        Err(_) => {}
    }
}
