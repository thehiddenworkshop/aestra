//! Production host-transform Curves workspace and attached key observers.
use super::CurvesAction;
use super::control_input::NativeButton as Button;
use super::cursor::EntityCursor;
use super::host_editing::*;
use super::pointer::{PointerClick, PointerDrag, PointerDragEnd, PointerDragStart};
use super::state::{CurvesState, DragState, Selection};
use crate::feathers::automation_curve;
use crate::{EditorNativeControl, EditorSession, theme, timeline::TimelineState};
use aestra_authoring::{EffectCommand, EffectTransaction};
use aestra_core::CurveInterpolation;
use bevy::{picking::pointer::PointerButton, prelude::*, window::SystemCursorIcon};

pub(super) const LABELS: [&str; 10] = [
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

#[derive(Component)]
pub(super) struct Graph;
#[derive(Component)]
pub(super) struct Key(pub(super) usize);

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
                        .observe(move |event:On<PointerClick>, mut session:ResMut<EditorSession>, mut state:ResMut<CurvesState>, mut timeline:ResMut<TimelineState>| {
                            if event.button != PointerButton::Primary { return; }
                            if state.host_released.take() == Some(event.entity) { return; }
                            if session.effect.id != selected.effect { return; }
                            if let Some(time) = session.effect.host_transform_track.as_ref().and_then(|t| channel_keys(t, selected.channel).get(index).map(|k| k.time)) {
                                state.host=Some(selected); select_pose_time(&mut session,&mut timeline,time);
                            }
                        })
                        .observe(move |event:On<PointerDragStart>, session:Res<EditorSession>, mut state:ResMut<CurvesState>| {
                            if event.button != PointerButton::Primary || session.effect.id != selected.effect { return; }
                            if session.pending_change.is_some() || session.locks.is_locked(aestra_authoring::SemanticTarget::Effect(session.effect.id)) { return; }
                            let Some(original)=session.effect.host_transform_track.clone() else { return; };
                            state.host=Some(selected); state.host_released=None;
                            state.host_drag=Some(DragState { selection:selected, candidate:original.clone(), original,time:key_time(&session,selected) });
                        })
                        .observe(move |event:On<PointerDrag>, graph:Single<(&ComputedNode,&Children),With<Graph>>, mut session:ResMut<EditorSession>, mut state:ResMut<CurvesState>, mut nodes:Query<(&Key,&mut Node)>, mut rasters:Query<&mut automation_curve::AutomationCurveRaster>| {
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
                        .observe(move |event:On<PointerDragEnd>,mut session:ResMut<EditorSession>,mut state:ResMut<CurvesState>,mut timeline:ResMut<TimelineState>| {
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
