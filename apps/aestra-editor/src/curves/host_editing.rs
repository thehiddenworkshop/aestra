//! Shared production host-curve edits, projection math and cancellation.
use super::state::{CurvesState, Selection};
use crate::feathers::automation_curve::{AutomationCurveData, AutomationCurvePoint};
use crate::{
    EditorSession,
    timeline::{TimelineState, host_motion as poses},
};
use aestra_authoring::EffectCommand;
use aestra_core::{Curve, CurveInterpolation, CurveKey, HostTransformTrack, QuaternionKey};
use bevy::prelude::*;

pub(super) fn scalar(track: &HostTransformTrack, channel: u8) -> Option<&Curve> {
    match channel {
        0..=2 => track.curves.translation.get(channel as usize),
        3..=5 => track.curves.scale.get(channel as usize - 3),
        _ => None,
    }
}
pub(super) fn scalar_mut(track: &mut HostTransformTrack, channel: u8) -> Option<&mut Curve> {
    match channel {
        0..=2 => track.curves.translation.get_mut(channel as usize),
        3..=5 => track.curves.scale.get_mut(channel as usize - 3),
        _ => None,
    }
}
pub(super) fn channel_keys(track: &HostTransformTrack, channel: u8) -> Vec<CurveKey> {
    if let Some(curve) = scalar(track, channel) {
        curve
            .keys
            .iter()
            .map(|key| CurveKey::new(key.time, curve.output_value(key.value)))
            .collect()
    } else {
        track
            .curves
            .rotation
            .keys
            .iter()
            .map(|key| CurveKey::new(key.time, key.value[(channel - 6).min(3) as usize]))
            .collect()
    }
}
pub(super) fn interpolation(track: &HostTransformTrack, channel: u8) -> CurveInterpolation {
    scalar(track, channel).map_or(track.curves.rotation.interpolation, |curve| {
        curve.interpolation
    })
}

pub(super) fn open(
    session: &mut EditorSession,
    state: &mut CurvesState,
    timeline: &mut TimelineState,
    channel: u8,
) {
    if state.host_drag.take().is_some() {
        session.restore_interaction_preview();
    }
    let Some(track) = session.effect.host_transform_track.as_ref() else {
        return;
    };
    let channel = channel.min(9);
    let time = poses::selected_pose(session, timeline)
        .and_then(|(i, _)| track.keys().get(i).map(|key| key.time))
        .unwrap_or(session.time());
    let keys = channel_keys(track, channel);
    let index = keys
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (a.time - time).abs().total_cmp(&(b.time - time).abs()))
        .map_or(0, |(i, _)| i);
    state.host = Some(Selection {
        effect: session.effect.id,
        channel,
        key: index,
    });
    state.complex = None;
    select_pose_time(session, timeline, keys[index].time);
}

pub(super) fn select_pose_time(
    session: &mut EditorSession,
    timeline: &mut TimelineState,
    time: f32,
) {
    if let Some(index) = session
        .effect
        .host_transform_track
        .as_ref()
        .and_then(|t| t.curves.key_times().iter().position(|t| *t == time))
    {
        poses::select_pose(session, timeline, index);
    }
}

pub(super) fn commit(
    session: &mut EditorSession,
    state: &mut CurvesState,
    timeline: &mut TimelineState,
    candidate: HostTransformTrack,
    time: f32,
) {
    let Some(mut selection) = state.host.filter(|s| s.effect == session.effect.id) else {
        return;
    };
    if let Err(error) = candidate.validate() {
        session.restore_interaction_preview();
        session.status = error.to_string();
        session.ui_revision += 1;
        return;
    }
    let changed = session.effect.host_transform_track.as_ref() != Some(&candidate);
    if changed
        && !session.execute(
            "Edited transform curve",
            EffectCommand::SetHostTransformTrack {
                track: Some(candidate),
            },
            true,
        )
    {
        session.restore_interaction_preview();
        return;
    }
    // A drag may finish exactly where it began; discard its temporary preview
    // even when there is no semantic edit to commit.
    if !changed {
        session.restore_interaction_preview();
    }
    let track = session.effect.host_transform_track.as_ref().unwrap();
    let keys = channel_keys(track, selection.channel);
    selection.key = keys
        .iter()
        .position(|key| key.time == time)
        .unwrap_or(selection.key.min(keys.len() - 1));
    state.host = Some(selection);
    select_pose_time(session, timeline, keys[selection.key].time);
}

pub(super) fn set_interpolation(
    session: &mut EditorSession,
    state: &mut CurvesState,
    timeline: &mut TimelineState,
    mode: CurveInterpolation,
) {
    let Some(selection) = state.host.filter(|s| s.effect == session.effect.id) else {
        return;
    };
    let Some(mut track) = session.effect.host_transform_track.clone() else {
        return;
    };
    let keys = channel_keys(&track, selection.channel);
    let time = keys.get(selection.key).map_or(0.0, |key| key.time);
    if let Some(curve) = scalar_mut(&mut track, selection.channel) {
        curve.interpolation = mode;
    } else {
        track.curves.rotation.interpolation = mode;
    }
    commit(session, state, timeline, track, time);
}

pub(super) fn add_key(
    session: &mut EditorSession,
    state: &mut CurvesState,
    timeline: &mut TimelineState,
) {
    let Some(selection) = state.host.filter(|s| s.effect == session.effect.id) else {
        return;
    };
    let Some(mut track) = session.effect.host_transform_track.clone() else {
        return;
    };
    let time = if track.repeat {
        track.sample_time(session.time())
    } else {
        session.time()
    };
    if let Some(curve) = scalar_mut(&mut track, selection.channel) {
        if !curve.keys.iter().any(|key| (key.time - time).abs() < 1e-5) {
            // Keep normalized storage if present; the sampled output is converted back.
            let output = curve.sample_at(time);
            let value = curve.output_range.map_or(output, |r| {
                if r.max == r.min {
                    0.0
                } else {
                    (output - r.min) / (r.max - r.min)
                }
            });
            curve.keys.push(CurveKey::new(time, value));
            curve.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
        }
    } else if !track
        .curves
        .rotation
        .keys
        .iter()
        .any(|key| (key.time - time).abs() < 1e-5)
    {
        let value = track.curves.rotation.sample_at(time);
        track
            .curves
            .rotation
            .keys
            .push(QuaternionKey { time, value });
        track
            .curves
            .rotation
            .keys
            .sort_by(|a, b| a.time.total_cmp(&b.time));
    }
    commit(session, state, timeline, track, time);
}

pub(super) fn delete_key(
    session: &mut EditorSession,
    state: &mut CurvesState,
    timeline: &mut TimelineState,
) {
    let Some(selection) = state.host.filter(|s| s.effect == session.effect.id) else {
        return;
    };
    let Some(mut track) = session.effect.host_transform_track.clone() else {
        return;
    };
    if selection.key == 0 {
        session.status = "Each transform channel needs its time-zero anchor".into();
        return;
    }
    if let Some(curve) = scalar_mut(&mut track, selection.channel) {
        if selection.key >= curve.keys.len() {
            return;
        }
        curve.keys.remove(selection.key);
    } else {
        if selection.key >= track.curves.rotation.keys.len() {
            return;
        }
        track.curves.rotation.keys.remove(selection.key);
    }
    commit(session, state, timeline, track, 0.0);
}

pub(super) fn projection(track: &HostTransformTrack, channel: u8) -> AutomationCurveData {
    let end = track.end_time().max(0.001);
    let points = if channel < 6 {
        channel_keys(track, channel)
            .into_iter()
            .map(|key| AutomationCurvePoint {
                time: key.time / end,
                value: key.value,
            })
            .collect()
    } else {
        // Component plots must show actual slerp, not independent scalar interpolation.
        (0..384)
            .map(|i| {
                let time = i as f32 / 383.0;
                AutomationCurvePoint {
                    time,
                    value: track.curves.rotation.sample_at(time * end)[(channel - 6) as usize],
                }
            })
            .collect()
    };
    AutomationCurveData::Curve {
        points,
        value_bounds: if channel >= 6 {
            Some((-1.0, 1.0))
        } else {
            None
        },
        interpolation: if channel >= 6 {
            CurveInterpolation::Linear
        } else {
            interpolation(track, channel)
        },
    }
}

// Freeze the axes for an entire gesture so moving a key never shifts the plot
// underneath the pointer. Refit only after committing and rebuilding the view.
pub(super) fn drag_projection(
    original: &HostTransformTrack,
    candidate: &HostTransformTrack,
    channel: u8,
) -> AutomationCurveData {
    let base = projection(original, channel);
    let mut data = projection(candidate, channel);
    if let AutomationCurveData::Curve {
        points,
        value_bounds,
        ..
    } = &mut data
    {
        *value_bounds = Some((
            base.value_for_top_percent(92.0).unwrap(),
            base.value_for_top_percent(8.0).unwrap(),
        ));
        for point in points {
            point.time *= candidate.end_time().max(0.001) / original.end_time().max(0.001);
        }
    }
    data
}

pub(super) fn edit_key(
    original: &HostTransformTrack,
    selection: Selection,
    distance: Vec2,
    size: Vec2,
) -> Option<(HostTransformTrack, f32)> {
    if size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    let keys = channel_keys(original, selection.channel);
    let key = *keys.get(selection.key)?;
    let data = projection(original, selection.channel);
    let time = if selection.key == 0 {
        0.0
    } else {
        (key.time + distance.x / size.x * original.end_time().max(0.001))
            .clamp(0.0, original.end_time().max(0.001))
    };
    if keys
        .iter()
        .enumerate()
        .any(|(i, k)| i != selection.key && (k.time - time).abs() < 1e-5)
    {
        return None;
    }
    let value = data.value_for_top_percent(
        data.top_percent_for_value(key.value)? + distance.y / size.y * 100.0,
    )?;
    let mut candidate = original.clone();
    let repeat = candidate.repeat;
    let end = candidate.end_time();
    if let Some(curve) = scalar_mut(&mut candidate, selection.channel) {
        // Canonicalize only this channel's output units before changing a value.
        for (stored, output) in curve.keys.iter_mut().zip(keys) {
            stored.value = output.value;
        }
        curve.output_range = None;
        let value = if selection.channel >= 3 {
            value.max(0.001)
        } else {
            value
        };
        curve.keys[selection.key] = CurveKey::new(time, value);
        if repeat && (key.time == 0.0 || key.time == end) {
            if time != key.time {
                return None;
            }
            let other = if key.time == 0.0 { end } else { 0.0 };
            if let Some(k) = curve.keys.iter_mut().find(|k| k.time == other) {
                k.value = value;
            } else {
                curve.keys.push(CurveKey::new(other, value));
            }
        }
        curve.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
    } else {
        candidate.curves.rotation.keys[selection.key].time = time;
        candidate
            .curves
            .rotation
            .keys
            .sort_by(|a, b| a.time.total_cmp(&b.time));
    }
    candidate.validate().ok()?;
    Some((candidate, time))
}

pub(super) fn cancel_drag(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<CurvesState>,
) {
    if let Some(drag) = &state.host_drag
        && (keys.is_some_and(|keys| keys.just_pressed(KeyCode::Escape))
            || state.host != Some(drag.selection)
            || session.effect.id != drag.selection.effect
            || session.effect.host_transform_track.as_ref() != Some(&drag.original))
    {
        state.host_drag = None;
        session.restore_interaction_preview();
        session.ui_revision += 1;
    }
}

pub(super) fn key_time(session: &EditorSession, selection: Selection) -> f32 {
    session
        .effect
        .host_transform_track
        .as_ref()
        .and_then(|track| {
            channel_keys(track, selection.channel)
                .get(selection.key)
                .map(|key| key.time)
        })
        .unwrap_or(0.0)
}
