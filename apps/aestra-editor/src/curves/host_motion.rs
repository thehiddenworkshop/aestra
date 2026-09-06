//! Target adapter for effect transform curves; uses the shared curve model and raster widget.
use super::*;
use crate::timeline::{TimelineState, host_motion as poses};
use aestra_authoring::{EffectCommand, EffectTransaction};
use aestra_core::{Curve, CurveInterpolation, EffectId, HostTransformTrack, QuaternionKey};

const LABELS: [&str; 10] = [
    "Position X",
    "Position Y",
    "Position Z",
    "Scale X",
    "Scale Y",
    "Scale Z",
    "Rotation X (quaternion)",
    "Rotation Y (quaternion)",
    "Rotation Z (quaternion)",
    "Rotation W (quaternion)",
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Selection {
    effect: EffectId,
    channel: u8,
    key: usize,
}
pub(super) struct DragState {
    selection: Selection,
    original: HostTransformTrack,
    candidate: HostTransformTrack,
    time: f32,
}
#[derive(Component)]
struct Graph;
#[derive(Component)]
struct Key(usize);

pub(super) fn install(app: &mut App) {
    app.add_systems(
        Update,
        cancel_drag
            .before(handle_curves_actions)
            .in_set(CurvesSet::Actions),
    );
}

fn scalar(track: &HostTransformTrack, channel: u8) -> Option<&Curve> {
    match channel {
        0..=2 => track.curves.translation.get(channel as usize),
        3..=5 => track.curves.scale.get(channel as usize - 3),
        _ => None,
    }
}
fn scalar_mut(track: &mut HostTransformTrack, channel: u8) -> Option<&mut Curve> {
    match channel {
        0..=2 => track.curves.translation.get_mut(channel as usize),
        3..=5 => track.curves.scale.get_mut(channel as usize - 3),
        _ => None,
    }
}
fn channel_keys(track: &HostTransformTrack, channel: u8) -> Vec<CurveKey> {
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
fn interpolation(track: &HostTransformTrack, channel: u8) -> CurveInterpolation {
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

fn select_pose_time(session: &mut EditorSession, timeline: &mut TimelineState, time: f32) {
    if let Some(index) = session
        .effect
        .host_transform_track
        .as_ref()
        .and_then(|t| t.curves.key_times().iter().position(|t| *t == time))
    {
        poses::select_pose(session, timeline, index);
    }
}

fn commit(
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

fn projection(track: &HostTransformTrack, channel: u8) -> AutomationCurveData {
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
fn drag_projection(
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

fn edit_key(
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

fn cancel_drag(
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

pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &CurvesState,
) -> bool {
    let Some(selection) = state.host.filter(|s| s.effect == session.effect.id) else {
        return false;
    };
    let Some(track) = session.effect.host_transform_track.as_ref() else {
        return false;
    };
    parent.spawn(Node { width: Val::Percent(100.0), height: Val::Percent(100.0), min_height: Val::Px(0.0), ..default() }).with_children(|row| {
        row.spawn(Node { width: Val::Px(210.0), flex_shrink: 0.0, ..default() }).with_children(|list| {
            crate::feathers::scroll::spawn_vertical_scroll_area(list, crate::ScrollMemoryKey::Curves, Node { flex_grow:1.0, min_height:Val::Px(0.0), flex_direction:FlexDirection::Column, row_gap:Val::Px(4.0), ..default() }, |list| {
                for (i,label) in LABELS.iter().enumerate() { crate::spawn_feathers_action_button(list, label, CurvesAction::OpenHost(i as u8), selection.channel == i as u8); }
            });
        });
        row.spawn(Node { flex_grow:1.0, min_width:Val::Px(0.0), flex_direction:FlexDirection::Column, padding:UiRect::all(Val::Px(10.0)), row_gap:Val::Px(8.0), ..default() }).with_children(|panel| {
            panel.spawn((Text::new(format!("Effect transform · {} · 0–{:.3} seconds", LABELS[selection.channel as usize], track.end_time())), TextFont { font_size:FontSize::Px(12.0), ..default() }, TextColor(theme::TEXT)));
            panel.spawn(Node { flex_wrap:FlexWrap::Wrap, column_gap:Val::Px(5.0), flex_shrink:0.0, ..default() }).with_children(|tools| {
                for (label,mode) in [("Step",CurveInterpolation::Step),("Linear",CurveInterpolation::Linear),("Smooth",CurveInterpolation::Smooth)] { crate::spawn_feathers_action_button(tools,label,CurvesAction::SetInterpolation(mode),interpolation(track, selection.channel)==mode); }
                crate::spawn_feathers_action_button(tools,"Key at playhead",CurvesAction::HostAddKey,false);
                crate::spawn_feathers_action_button(tools,"Delete key",CurvesAction::HostDeleteKey,false);
            });
            panel.spawn((Text::new(if selection.channel < 6 { "Drag keys to change time and value. Escape cancels. Each channel has independent keys." } else { "Quaternion components are linked. Drag horizontally to retime; edit orientation with the pose inspector or gizmo." }), TextFont { font_size:FontSize::Px(10.0), ..default() }, TextColor(theme::TEXT_MUTED)));
            let data = projection(track, selection.channel);
            let end = track.end_time().max(0.001);
            panel.spawn((Graph, Node { width:Val::Percent(100.0), flex_grow:1.0, min_height:Val::Px(112.0), position_type:PositionType::Relative, overflow:Overflow::clip(), ..default() }, BackgroundColor(theme::TIMELINE_BG))).with_children(|graph| {
                automation_curve::spawn_automation_curve(graph, &data);
                for (index,key) in channel_keys(track, selection.channel).iter().enumerate() {
                    let selected = Selection { key:index, ..selection };
                    graph.spawn((Key(index), Button, EditorNativeControl, EntityCursor::System(SystemCursorIcon::Grab), Node { position_type:PositionType::Absolute, left:Val::Percent(key.time/end*100.0), top:Val::Percent(data.top_percent_for_value(key.value).unwrap_or(50.0)), width:Val::Px(12.0),height:Val::Px(12.0),margin:UiRect { left:Val::Px(-6.0),top:Val::Px(-6.0),..default() }, border:UiRect::all(Val::Px(2.0)),border_radius:BorderRadius::MAX,..default() }, BackgroundColor(theme::ACCENT), BorderColor::all(if index == selection.key { Color::WHITE } else { theme::ACCENT_DIM }), crate::feathers::tooltip::EditorTooltip::description(format!("{:.3} s · {:.4}",key.time,key.value))))
                        .observe(move |event:On<Pointer<Click>>, mut session:ResMut<EditorSession>, mut state:ResMut<CurvesState>, mut timeline:ResMut<TimelineState>| {
                            if event.button != PointerButton::Primary { return; }
                            if state.host_released.take() == Some(event.entity) { return; }
                            if session.effect.id != selected.effect { return; }
                            if let Some(time) = session.effect.host_transform_track.as_ref().and_then(|t| channel_keys(t, selected.channel).get(index).map(|k| k.time)) {
                                state.host=Some(selected); select_pose_time(&mut session,&mut timeline,time);
                            }
                        })
                        .observe(move |event:On<Pointer<DragStart>>, session:Res<EditorSession>, mut state:ResMut<CurvesState>| {
                            if event.button != PointerButton::Primary || session.effect.id != selected.effect { return; }
                            if session.pending_change.is_some() || session.locks.is_locked(aestra_authoring::SemanticTarget::Effect(session.effect.id)) { return; }
                            let Some(original)=session.effect.host_transform_track.clone() else { return; };
                            state.host=Some(selected); state.host_released=None;
                            state.host_drag=Some(DragState { selection:selected, candidate:original.clone(), original,time:key_time(&session,selected) });
                        })
                        .observe(move |event:On<Pointer<Drag>>, graph:Single<(&ComputedNode,&Children),With<Graph>>, mut session:ResMut<EditorSession>, mut state:ResMut<CurvesState>, mut nodes:Query<(&Key,&mut Node)>, mut rasters:Query<&mut automation_curve::AutomationCurveRaster>| {
                            if event.button != PointerButton::Primary { return; }
                            let Some(drag)=state.host_drag.as_mut().filter(|d| d.selection==selected) else { return; };
                            if session.effect.host_transform_track.as_ref()!=Some(&drag.original) { return; }
                            let size=graph.0.size()*graph.0.inverse_scale_factor;
                            let Some((candidate,time))=edit_key(&drag.original,selected,event.distance,size) else { return; };
                            if !session.preview_interaction(EffectTransaction::single("Preview transform curve",EffectCommand::SetHostTransformTrack { track:Some(candidate.clone()) })) { return; }
                            drag.candidate=candidate; drag.time=time;
                            let data=drag_projection(&drag.original, &drag.candidate, selected.channel);
                            let original_keys = channel_keys(&drag.original, selected.channel);
                            let candidate_keys = channel_keys(&drag.candidate, selected.channel);
                            for (key,mut node) in &mut nodes {
                                let Some(original_key) = original_keys.get(key.0) else { continue; };
                                let time = if key.0 == selected.key { time } else { original_key.time };
                                if let Some(value) = candidate_keys.iter().find(|k| k.time == time).map(|k| k.value) {
                                    node.left=Val::Percent(time/drag.original.end_time().max(0.001)*100.0);
                                    node.top=Val::Percent(data.top_percent_for_value(value).unwrap_or(50.0));
                                }
                            }
                            for child in graph.1.iter() {
                                if let Ok(mut raster) = rasters.get_mut(child) { raster.set_data(data.clone()); }
                            }
                        })
                        .observe(move |event:On<Pointer<DragEnd>>,mut session:ResMut<EditorSession>,mut state:ResMut<CurvesState>,mut timeline:ResMut<TimelineState>| {
                            if event.button != PointerButton::Primary { return; }
                            state.host_released=Some(event.entity);
                            if let Some(drag)=state.host_drag.take().filter(|d|d.selection==selected)
                                && session.effect.id==selected.effect && session.effect.host_transform_track.as_ref()==Some(&drag.original) { commit(&mut session,&mut state,&mut timeline,drag.candidate,drag.time); }
                        });
                }
            });
        });
    });
    true
}

fn key_time(session: &EditorSession, selection: Selection) -> f32 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_core::{EmitterTransform, HostTransformKey};
    use bevy_resvg::prelude::SvgFile;

    fn track() -> HostTransformTrack {
        HostTransformTrack::from_pose_keys(
            vec![
                HostTransformKey {
                    time: 0.0,
                    transform: EmitterTransform::default(),
                },
                HostTransformKey {
                    time: 2.0,
                    transform: EmitterTransform {
                        translation: [8.0, 4.0, 0.0],
                        ..default()
                    },
                },
            ],
            false,
        )
    }

    #[test]
    fn transform_view_spawns_shared_curve_raster_and_feather_controls() {
        use bevy::{
            asset::AssetPlugin, scene::ScenePlugin, text::TextPlugin, ui_widgets::Scrollbar,
        };
        let mut effect = test_support::effect_with_timing_slack();
        effect.host_transform_track = Some(track());
        let selection = Selection {
            effect: effect.id,
            channel: 0,
            key: 1,
        };
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<SvgFile>()
        .init_resource::<InputFocus>()
        .insert_resource(EditorSession::from_test_effect(effect))
        .insert_resource(CurvesState {
            host: Some(selection),
            ..default()
        })
        .add_systems(
            Startup,
            |mut commands: Commands, session: Res<EditorSession>, state: Res<CurvesState>| {
                commands.spawn(Node::default()).with_children(|parent| {
                    assert!(spawn(parent, &session, &state));
                });
            },
        );
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world
                .query::<&automation_curve::AutomationCurveRaster>()
                .iter(world)
                .count(),
            1
        );
        assert_eq!(world.query::<&Key>().iter(world).count(), 2);
        assert_eq!(world.query::<&Scrollbar>().iter(world).count(), 1);
        let controls: Vec<_> = world
            .query::<&CurvesAction>()
            .iter(world)
            .copied()
            .collect();
        assert_eq!(
            controls
                .iter()
                .filter(|a| matches!(a, CurvesAction::SetInterpolation(_)))
                .count(),
            3
        );
        assert_eq!(
            controls
                .iter()
                .filter(|a| matches!(a, CurvesAction::OpenHost(_)))
                .count(),
            10
        );
    }

    #[test]
    fn interpolation_and_independent_keys_are_undoable() {
        let mut session = test_support::session_with_timing_slack();
        let original = track();
        assert!(session.execute(
            "Set track",
            EffectCommand::SetHostTransformTrack {
                track: Some(original.clone())
            },
            true
        ));
        let mut state = CurvesState::default();
        let mut timeline = TimelineState::default();
        open(&mut session, &mut state, &mut timeline, 0);
        set_interpolation(
            &mut session,
            &mut state,
            &mut timeline,
            CurveInterpolation::Step,
        );
        let changed = session.effect.host_transform_track.as_ref().unwrap();
        assert_eq!(
            changed.curves.translation[0].interpolation,
            CurveInterpolation::Step
        );
        assert_eq!(
            changed.curves.translation[1],
            original.curves.translation[1]
        );
        session.undo();
        assert_eq!(
            session.effect.host_transform_track.as_ref(),
            Some(&original)
        );
        session.redo();
        assert_eq!(
            session
                .effect
                .host_transform_track
                .as_ref()
                .unwrap()
                .curves
                .translation[0]
                .interpolation,
            CurveInterpolation::Step
        );

        session.seek_time(1.0);
        add_key(&mut session, &mut state, &mut timeline);
        let changed = session.effect.host_transform_track.as_ref().unwrap();
        assert_eq!(changed.curves.translation[0].keys.len(), 3);
        assert_eq!(changed.curves.translation[1].keys.len(), 2);
        assert_eq!(changed.curves.rotation.keys.len(), 2);
        delete_key(&mut session, &mut state, &mut timeline);
        assert_eq!(
            session
                .effect
                .host_transform_track
                .as_ref()
                .unwrap()
                .curves
                .translation[0]
                .keys
                .len(),
            2
        );
        session.undo();
        assert_eq!(
            session
                .effect
                .host_transform_track
                .as_ref()
                .unwrap()
                .curves
                .translation[0]
                .keys
                .len(),
            3
        );
    }

    #[test]
    fn drag_keeps_axes_fixed_and_changes_only_the_target_channel() {
        let original = track();
        let selection = Selection {
            effect: EffectId::new(),
            channel: 0,
            key: 1,
        };
        let (candidate, time) = edit_key(
            &original,
            selection,
            Vec2::new(-25.0, -10.0),
            Vec2::splat(100.0),
        )
        .unwrap();
        assert_eq!(time, 1.5);
        assert_eq!(
            candidate.curves.translation[1..],
            original.curves.translation[1..]
        );
        assert_eq!(candidate.curves.scale, original.curves.scale);
        let base = projection(&original, 0);
        let preview = drag_projection(&original, &candidate, 0);
        for value in [0.0, 4.0, 8.0] {
            assert_eq!(
                base.top_percent_for_value(value),
                preview.top_percent_for_value(value)
            );
        }
        assert!(
            edit_key(
                &original,
                selection,
                Vec2::new(-100.0, 0.0),
                Vec2::splat(100.0)
            )
            .is_none(),
            "a key cannot collide with the time-zero anchor"
        );
    }

    #[test]
    fn quaternion_component_drag_retimes_without_breaking_rotation() {
        let original = track();
        let selection = Selection {
            effect: EffectId::new(),
            channel: 9,
            key: 1,
        };
        let (candidate, time) = edit_key(
            &original,
            selection,
            Vec2::new(-25.0, 90.0),
            Vec2::splat(100.0),
        )
        .unwrap();
        assert_eq!(time, 1.5);
        assert_eq!(
            candidate.curves.rotation.keys[1].value,
            original.curves.rotation.keys[1].value
        );
        assert_eq!(candidate.curves.translation, original.curves.translation);
        candidate.validate().unwrap();
    }

    #[test]
    fn escape_cancels_the_temporary_curve_without_touching_history() {
        let mut session = test_support::session_with_timing_slack();
        let original = track();
        session.execute(
            "Set track",
            EffectCommand::SetHostTransformTrack {
                track: Some(original.clone()),
            },
            true,
        );
        let selection = Selection {
            effect: session.effect.id,
            channel: 0,
            key: 1,
        };
        let (candidate, time) = edit_key(
            &original,
            selection,
            Vec2::new(-25.0, 0.0),
            Vec2::splat(100.0),
        )
        .unwrap();
        assert!(session.preview_interaction(EffectTransaction::single(
            "Preview",
            EffectCommand::SetHostTransformTrack {
                track: Some(candidate.clone())
            }
        )));
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(CurvesState {
                host: Some(selection),
                host_drag: Some(DragState {
                    selection,
                    original: original.clone(),
                    candidate,
                    time,
                }),
                ..default()
            })
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, cancel_drag);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(app.world().resource::<CurvesState>().host_drag.is_none());
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        assert_eq!(
            session.effect.host_transform_track.as_ref(),
            Some(&original)
        );
        session.undo();
        assert!(
            session.effect.host_transform_track.is_none(),
            "cancel must not introduce a history entry"
        );
    }
}
