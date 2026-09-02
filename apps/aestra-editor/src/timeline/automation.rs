use super::*;

#[derive(Component, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AutomationLaneId {
    pub(super) emitter: EmitterId,
    pub(super) module: ModuleId,
    pub(super) input: u8,
    pub(super) parameter: String,
    pub(super) channel: Option<u8>,
}

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct TimelineAutomationKeySelection {
    pub(super) lane: AutomationLaneId,
    pub(super) key: usize,
}

#[derive(Clone, Debug)]
pub(super) struct TimelineAutomationKeyDrag {
    pub(super) selection: TimelineAutomationKeySelection,
    pub(super) source_start: f32,
    pub(super) source_duration: f32,
    pub(super) original_time: f32,
    pub(super) current_time: f32,
    pub(super) original_value: Option<f32>,
    pub(super) current_value: Option<f32>,
}

#[derive(Component)]
pub(super) struct TimelineAutomationLaneResizeHandle;

#[derive(Component, Clone)]
pub(super) struct TimelineAutomationLaneGraph(pub(super) AutomationLaneId);

#[derive(Component)]
pub(super) struct TimelineAutomationLane;

#[derive(Component)]
pub(super) struct TimelineAutomationKey;

#[derive(Component)]
pub(super) struct EmitterAutomationMenuButton;

#[derive(Component)]
pub(super) struct EmitterAutomationVisibilityMenu;

#[derive(Component)]
pub(super) struct EmitterAutomationVisibilityMenuAnchor;

#[derive(Clone)]
pub(super) enum AutomationLaneKeys {
    Curve(Vec<CurveKey>),
    Gradient(Vec<ColorKey>),
}

impl AutomationLaneKeys {
    pub(super) fn times(&self) -> impl Iterator<Item = f32> + '_ {
        match self {
            Self::Curve(keys) => EitherAutomationTimes::Curve(keys.iter()),
            Self::Gradient(keys) => EitherAutomationTimes::Gradient(keys.iter()),
        }
    }

    pub(super) fn len(&self) -> usize {
        match self {
            Self::Curve(keys) => keys.len(),
            Self::Gradient(keys) => keys.len(),
        }
    }

    fn curve_value(&self, key: usize) -> Option<f32> {
        match self {
            Self::Curve(keys) => keys.get(key).map(|key| key.value),
            Self::Gradient(_) => None,
        }
    }

    pub(super) fn graph_data(&self) -> AutomationCurveData {
        match self {
            Self::Curve(keys) => AutomationCurveData::Curve {
                points: keys
                    .iter()
                    .map(|key| AutomationCurvePoint {
                        time: key.time,
                        value: key.value,
                    })
                    .collect(),
                value_bounds: None,
            },
            Self::Gradient(keys) => AutomationCurveData::Gradient(
                keys.iter()
                    .map(|key| AutomationGradientPoint {
                        time: key.time,
                        color: key.color,
                    })
                    .collect(),
            ),
        }
    }
}

enum EitherAutomationTimes<'a> {
    Curve(std::slice::Iter<'a, CurveKey>),
    Gradient(std::slice::Iter<'a, ColorKey>),
}

impl Iterator for EitherAutomationTimes<'_> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Curve(keys) => keys.next().map(|key| key.time),
            Self::Gradient(keys) => keys.next().map(|key| key.time),
        }
    }
}

#[derive(Clone)]
pub(super) struct AutomationLaneProjection {
    pub(super) id: AutomationLaneId,
    pub(super) label: String,
    pub(super) keys: AutomationLaneKeys,
}

pub(super) fn emitter_automation_lanes(
    effect: &EffectAsset,
    emitter: &Emitter,
    registry: &EditorModuleRegistry,
    localizer: &Localizer,
) -> Vec<AutomationLaneProjection> {
    let mut lanes = Vec::new();
    for module in &emitter.modules {
        let Some(metadata) = registry.0.get(&module.module_type) else {
            continue;
        };
        for (input, input_metadata) in metadata.inputs.iter().enumerate() {
            let source = module.property_source(input_metadata.name);
            let value = bound_automation_parameter(effect, module, input_metadata.name)
                .map(|parameter| parameter.default.clone())
                .or_else(|| module_parameter(module, input_metadata.name));
            let display_name = localized_properties_input(
                localizer,
                input_metadata.name,
                input_metadata.display_name,
                false,
            );
            let lane_id = |channel| AutomationLaneId {
                emitter: emitter.id,
                module: module.id,
                input: input as u8,
                parameter: input_metadata.name.into(),
                channel,
            };
            match (source, value) {
                (Some(aestra_core::PropertySource::Curve(_)), Some(Value::Curve(curve))) => {
                    lanes.push(AutomationLaneProjection {
                        id: lane_id(None),
                        label: display_name,
                        keys: AutomationLaneKeys::Curve(curve.keys),
                    });
                }
                (Some(aestra_core::PropertySource::Curve(_)), Some(Value::Vec3Curve(curves))) => {
                    for (channel, curve) in curves.curves.into_iter().enumerate() {
                        lanes.push(AutomationLaneProjection {
                            id: lane_id(Some(channel as u8)),
                            label: format!("{display_name} {}", ["X", "Y", "Z"][channel]),
                            keys: AutomationLaneKeys::Curve(curve.keys),
                        });
                    }
                }
                (
                    Some(aestra_core::PropertySource::Gradient(_)),
                    Some(Value::Gradient(gradient)),
                ) => lanes.push(AutomationLaneProjection {
                    id: lane_id(None),
                    label: display_name,
                    keys: AutomationLaneKeys::Gradient(gradient.keys),
                }),
                _ => {}
            }
        }
    }
    lanes
}

#[cfg(test)]
pub(super) fn automation_lane_count(emitter: &Emitter) -> usize {
    emitter
        .modules
        .iter()
        .map(|module| {
            let parameters: Vec<&str> = match &module.parameters {
                ModuleParameters::Emission { .. } => vec!["spawn_rate", "burst_count"],
                ModuleParameters::Shape { .. } => vec!["shape"],
                ModuleParameters::Initialize { .. } => vec![
                    "lifetime",
                    "speed",
                    "direction",
                    "spread_degrees",
                    "angular_velocity",
                ],
                ModuleParameters::Motion { .. } => vec!["gravity", "drag", "turbulence"],
                ModuleParameters::Appearance { .. } => vec!["size", "opacity", "color"],
                ModuleParameters::Custom(values) => values.keys().map(String::as_str).collect(),
            };
            parameters
                .into_iter()
                .map(|parameter| {
                    if !module
                        .property_source(parameter)
                        .is_some_and(source_is_automation)
                    {
                        return 0;
                    }
                    if matches!(
                        module.active_parameter_value(parameter),
                        Some(Value::Vec3Curve(_))
                    ) {
                        3
                    } else {
                        1
                    }
                })
                .sum::<usize>()
        })
        .sum()
}

pub(super) fn visible_automation_lane_count(state: &TimelineState, emitter: &Emitter) -> usize {
    state
        .visible_automation_lanes
        .iter()
        .filter(|lane| emitter_has_automation_lane(emitter, lane))
        .count()
}

fn emitter_has_automation_lane(emitter: &Emitter, lane: &AutomationLaneId) -> bool {
    if lane.emitter != emitter.id {
        return false;
    }
    let Some(module) = emitter
        .modules
        .iter()
        .find(|module| module.id == lane.module)
    else {
        return false;
    };
    matches!(
        (
            module.property_source(&lane.parameter),
            module.active_parameter_value(&lane.parameter),
            lane.channel,
        ),
        (
            Some(aestra_core::PropertySource::Curve(_)),
            Some(Value::Vec3Curve(_)),
            Some(0..=2)
        ) | (
            Some(aestra_core::PropertySource::Curve(_)),
            Some(Value::Curve(_)),
            None
        ) | (
            Some(aestra_core::PropertySource::Gradient(_)),
            Some(Value::Gradient(_)),
            None
        )
    )
}

#[cfg(test)]
pub(super) fn source_is_automation(source: aestra_core::PropertySource) -> bool {
    matches!(
        source,
        aestra_core::PropertySource::Curve(_) | aestra_core::PropertySource::Gradient(_)
    )
}

pub(super) fn automation_lane_is_visible(state: &TimelineState, lane: &AutomationLaneId) -> bool {
    state.expanded_automation_emitters.contains(&lane.emitter)
        && state.visible_automation_lanes.contains(lane)
}

pub(super) fn automation_lanes_height(state: &TimelineState, emitter: &Emitter) -> f32 {
    let default_height = automation_curve::DEFAULT_HEIGHT;
    visible_automation_lane_count(state, emitter) as f32 * default_height
        + state
            .automation_lane_heights
            .iter()
            .filter(|(lane, _)| {
                emitter_has_automation_lane(emitter, lane)
                    && state.visible_automation_lanes.contains(*lane)
            })
            .map(|(_, height)| *height - default_height)
            .sum::<f32>()
}

pub(super) fn automation_lane_value(
    effect: &EffectAsset,
    lane: &AutomationLaneId,
) -> Option<Value> {
    let emitter = effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == lane.emitter)?;
    let module = emitter
        .modules
        .iter()
        .find(|module| module.id == lane.module)?;
    if let Some(parameter) = bound_automation_parameter(effect, module, &lane.parameter) {
        return Some(parameter.default.clone());
    }
    module_parameter(module, &lane.parameter)
}

fn bound_automation_parameter<'a>(
    effect: &'a EffectAsset,
    module: &aestra_core::ModuleInstance,
    input: &str,
) -> Option<&'a EffectParameter> {
    let parameter_id = module.bindings.get(input)?;
    effect
        .parameters
        .iter()
        .find(|parameter| parameter.id == *parameter_id)
}

pub(super) fn set_automation_lane_value_command(
    effect: &EffectAsset,
    lane: &AutomationLaneId,
    value: Value,
) -> Option<EffectCommand> {
    let emitter = effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == lane.emitter)?;
    let module = emitter
        .modules
        .iter()
        .find(|module| module.id == lane.module)?;
    if let Some(mut parameter) =
        bound_automation_parameter(effect, module, &lane.parameter).cloned()
    {
        parameter.default = value;
        return Some(EffectCommand::SetParameter {
            id: parameter.id,
            parameter,
        });
    }
    if let Some(source) = module.property_source(&lane.parameter)
        && source != aestra_core::PropertySource::Constant
        && module
            .property_source_values
            .get(&lane.parameter)
            .is_some_and(|values| values.iter().any(|candidate| candidate.source == source))
    {
        return Some(EffectCommand::SetModulePropertySourceValue {
            emitter: lane.emitter,
            module: lane.module,
            parameter: lane.parameter.clone(),
            source,
            value,
        });
    }
    Some(EffectCommand::SetModuleParameter {
        emitter: lane.emitter,
        module: lane.module,
        parameter: lane.parameter.clone(),
        value,
    })
}

pub(super) fn automation_lane_keys(
    effect: &EffectAsset,
    lane: &AutomationLaneId,
) -> Option<AutomationLaneKeys> {
    match automation_lane_value(effect, lane)? {
        Value::Curve(curve) => Some(AutomationLaneKeys::Curve(curve.keys)),
        Value::Vec3Curve(curves) => lane
            .channel
            .and_then(|channel| curves.curves.get(channel as usize))
            .map(|curve| AutomationLaneKeys::Curve(curve.keys.clone())),
        Value::Gradient(gradient) => Some(AutomationLaneKeys::Gradient(gradient.keys)),
        _ => None,
    }
}

pub(super) fn curve_stored_sample(curve: &aestra_core::Curve, time: f32) -> f32 {
    let sampled = curve.sample(time);
    let Some(range) = curve.output_range else {
        return sampled;
    };
    let span = range.max - range.min;
    if span.abs() <= f32::EPSILON {
        return curve.keys.first().map_or(0.0, |key| key.value);
    }
    ((sampled - range.min) / span).clamp(0.0, 1.0)
}

impl TimelineState {
    pub(super) fn automation_lane_height(&self, lane: &AutomationLaneId) -> f32 {
        self.automation_lane_heights
            .get(lane)
            .copied()
            .unwrap_or(automation_curve::DEFAULT_HEIGHT)
    }
}

pub(super) fn update_automation_lane_graph_visuals(
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    mut graphs: Query<(&TimelineAutomationLaneGraph, &mut Node)>,
) {
    for (graph, mut node) in &mut graphs {
        let Some((start, duration)) = emitter_preview_timing(&session, &state, graph.0.emitter)
        else {
            node.display = Display::None;
            continue;
        };
        apply_automation_graph_geometry(&mut node, start, duration, state.view);
    }
}

fn emitter_preview_timing(
    session: &EditorSession,
    state: &TimelineState,
    emitter: EmitterId,
) -> Option<(f32, f32)> {
    let emitter = session
        .effect
        .emitters
        .iter()
        .find(|candidate| candidate.id == emitter)?;
    let region = state
        .selected_emitter_region
        .filter(|(selected_emitter, _)| *selected_emitter == emitter.id)
        .and_then(|(_, region)| emitter.timeline_region(region))
        .or_else(|| emitter.timeline_regions().into_iter().next())?;
    Some(
        state
            .drag
            .filter(|drag| drag.emitter == emitter.id && drag.region == region.id)
            .map_or(
                (region.start_time - region.source_offset, emitter.duration),
                |drag| {
                    (
                        drag.current_start - drag.current_source_offset,
                        drag.source_duration,
                    )
                },
            ),
    )
}

pub(super) fn apply_automation_graph_geometry(
    node: &mut Node,
    start_time: f32,
    duration: f32,
    view: TimelineView,
) {
    node.display = if start_time + duration > view.start && start_time < view.end {
        Display::Flex
    } else {
        Display::None
    };
    node.left = Val::Percent(view.normalized_time(start_time) * 100.0);
    node.width = Val::Percent(duration / view.span().max(0.001) * 100.0);
}

pub(super) fn update_automation_key_visuals(
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    mut keys: Query<(&TimelineAutomationKeySelection, &mut Node), With<TimelineAutomationKey>>,
) {
    let drag_preview = state
        .automation_key_drag
        .as_ref()
        .and_then(|drag| automation_key_drag_preview_data(&session.effect, drag));
    for (selection, mut node) in &mut keys {
        let Some((emitter_start, emitter_duration)) =
            emitter_preview_timing(&session, &state, selection.lane.emitter)
        else {
            node.display = Display::None;
            continue;
        };
        let time = state
            .automation_key_drag
            .as_ref()
            .filter(|drag| drag.selection == *selection)
            .map(|drag| drag.current_time)
            .or_else(|| {
                automation_lane_keys(&session.effect, &selection.lane)
                    .and_then(|keys| keys.times().nth(selection.key))
                    .map(|normalized| emitter_start + normalized * emitter_duration)
            });
        let Some(time) = time else {
            node.display = Display::None;
            continue;
        };
        let position = state.view.normalized_time(time);
        node.display = if (0.0..=1.0).contains(&position) {
            Display::Flex
        } else {
            Display::None
        };
        node.left = Val::Percent(position.clamp(0.0, 1.0) * 100.0);
        if state
            .automation_key_drag
            .as_ref()
            .is_some_and(|drag| drag.selection.lane == selection.lane)
            && let Some(AutomationCurveData::Curve { .. }) = drag_preview.as_ref()
        {
            node.top = Val::Percent(
                drag_preview
                    .as_ref()
                    .map_or(50.0, |preview| preview.key_top_percent(selection.key)),
            );
        }
    }
}

fn automation_key_drag_preview_data(
    effect: &EffectAsset,
    drag: &TimelineAutomationKeyDrag,
) -> Option<AutomationCurveData> {
    let mut preview = automation_lane_keys(effect, &drag.selection.lane)?.graph_data();
    let normalized_time =
        ((drag.current_time - drag.source_start) / drag.source_duration.max(0.001)).clamp(0.0, 1.0);
    match &mut preview {
        AutomationCurveData::Curve { points, .. } => {
            let point = points.get_mut(drag.selection.key)?;
            point.time = normalized_time;
            if let Some(value) = drag.current_value {
                point.value = value;
            }
        }
        AutomationCurveData::Gradient(points) => {
            points.get_mut(drag.selection.key)?.time = normalized_time;
        }
    }
    Some(preview)
}

pub(super) fn update_automation_curve_drag_preview(
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    graphs: Query<(&TimelineAutomationLaneGraph, &Children)>,
    mut rasters: Query<&mut automation_curve::AutomationCurveRaster>,
) {
    let Some(drag) = state.automation_key_drag.as_ref() else {
        return;
    };
    let Some(preview) = automation_key_drag_preview_data(&session.effect, drag) else {
        return;
    };
    for (graph, children) in &graphs {
        if graph.0 != drag.selection.lane {
            continue;
        }
        for child in children.iter() {
            let Ok(mut raster) = rasters.get_mut(child) else {
                continue;
            };
            if raster.data() != &preview {
                raster.set_data(preview.clone());
            }
        }
    }
}

fn snap_automation_key_time(
    candidate: f32,
    session: &EditorSession,
    mode: TimelineSnapMode,
    view: TimelineView,
    canvas_width: f32,
) -> (f32, Option<f32>) {
    match mode {
        TimelineSnapMode::None => (candidate, None),
        TimelineSnapMode::Frames => {
            let frame = 1.0 / session.clock.tick_rate().max(1) as f32;
            let snapped = (candidate / frame).round() * frame;
            (snapped, Some(snapped))
        }
        TimelineSnapMode::Seconds => {
            let interval = nice_timeline_step(view.span(), canvas_width) / 5.0;
            let snapped = (candidate / interval).round() * interval;
            (snapped, Some(snapped))
        }
        TimelineSnapMode::Smart => {
            let threshold = view.span() / canvas_width.max(1.0) * 9.0;
            let frame = 1.0 / session.clock.tick_rate().max(1) as f32;
            let mut targets = vec![
                0.0,
                session.playback_duration(),
                session.time(),
                (candidate / frame).round() * frame,
            ];
            targets.extend(session.effect.markers.iter().map(|marker| marker.time));
            targets.extend(
                session
                    .effect
                    .choreography_events
                    .iter()
                    .map(|event| event.time),
            );
            for emitter in &session.effect.emitters {
                for region in emitter.timeline_regions() {
                    targets.push(region.start_time);
                    targets.push(region.end_time());
                }
            }
            for clip in &session.effect.effect_clips {
                targets.push(clip.start_time);
                targets.push(clip.start_time + clip.duration);
            }
            let nearest = targets.into_iter().min_by(|left, right| {
                (candidate - *left)
                    .abs()
                    .total_cmp(&(candidate - *right).abs())
            });
            nearest
                .filter(|target| (candidate - *target).abs() <= threshold)
                .map_or((candidate, None), |target| (target, Some(target)))
        }
    }
}

pub(super) fn add_automation_key_from_graph(
    mut click: On<Pointer<Click>>,
    graphs: Query<(&TimelineAutomationLaneGraph, &RelativeCursorPosition)>,
    session: Res<EditorSession>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary || click.count < 2 {
        return;
    }
    let Ok((graph, cursor)) = graphs.get(click.event_target()) else {
        return;
    };
    let Some(position) = cursor.normalized else {
        return;
    };
    let Some(action) = add_automation_key_at_pointer_action(&session.effect, &graph.0, position)
    else {
        return;
    };
    commands.trigger(action);
    click.propagate(false);
}

pub(super) fn add_automation_key_at_pointer_action(
    effect: &EffectAsset,
    lane: &AutomationLaneId,
    position: Vec2,
) -> Option<ChoreographyAction> {
    let normalized_time = (position.x + 0.5).clamp(0.0, 1.0);
    let value_bits = automation_lane_keys(effect, lane)?
        .graph_data()
        .value_for_top_percent((position.y + 0.5) * 100.0)
        .map(f32::to_bits);
    Some(ChoreographyAction::AddAutomationKeyAt {
        lane: lane.clone(),
        normalized_time_bits: normalized_time.to_bits(),
        value_bits,
    })
}

pub(super) fn begin_automation_lane_resize(
    mut drag: On<Pointer<DragStart>>,
    handles: Query<&AutomationLaneId, With<TimelineAutomationLaneResizeHandle>>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(lane) = handles.get(drag.event_target()) else {
        return;
    };
    state.automation_lane_resize = Some((lane.clone(), state.automation_lane_height(lane)));
    override_cursor.0 = Some(EntityCursor::System(SystemCursorIcon::NsResize));
    **cursor = CursorIcon::System(SystemCursorIcon::NsResize);
    drag.propagate(false);
}

pub(super) fn move_automation_lane_resize(
    mut drag: On<Pointer<Drag>>,
    handles: Query<&AutomationLaneId, With<TimelineAutomationLaneResizeHandle>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut state: ResMut<TimelineState>,
    mut lanes: Query<(&AutomationLaneId, &mut Node), With<TimelineAutomationLane>>,
) {
    let Ok(lane) = handles.get(drag.event_target()) else {
        return;
    };
    let Some((active, start)) = state.automation_lane_resize.clone() else {
        return;
    };
    if active != *lane {
        return;
    }
    let distance = screen_distance_to_logical(drag.distance.y, window.scale_factor());
    let height =
        (start + distance).clamp(automation_curve::MIN_HEIGHT, automation_curve::MAX_HEIGHT);
    state.automation_lane_heights.insert(active.clone(), height);
    for (candidate, mut node) in &mut lanes {
        if *candidate == active {
            node.height = Val::Px(height);
        }
    }
    drag.propagate(false);
}

pub(super) fn finish_automation_lane_resize(
    mut drag: On<Pointer<DragEnd>>,
    handles: Query<&AutomationLaneId, With<TimelineAutomationLaneResizeHandle>>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(lane) = handles.get(drag.event_target()) else {
        return;
    };
    if state
        .automation_lane_resize
        .take()
        .is_some_and(|(active, _)| active == *lane)
    {
        session.ui_revision += 1;
    }
    override_cursor.0 = None;
    **cursor = CursorIcon::System(SystemCursorIcon::NsResize);
    drag.propagate(false);
}

pub(super) fn begin_automation_key_drag(
    mut drag: On<Pointer<DragStart>>,
    controls: Query<&TimelineAutomationKeySelection, With<TimelineAutomationKey>>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(selection) = controls.get(drag.event_target()) else {
        return;
    };
    let Some(emitter) = session
        .effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == selection.lane.emitter)
    else {
        return;
    };
    let Some(keys) = automation_lane_keys(&session.effect, &selection.lane) else {
        return;
    };
    let Some(normalized) = keys.times().nth(selection.key) else {
        return;
    };
    let region = state
        .selected_emitter_region
        .filter(|(selected_emitter, _)| *selected_emitter == emitter.id)
        .and_then(|(_, region)| emitter.timeline_region(region))
        .or_else(|| emitter.timeline_regions().into_iter().next())
        .expect("an emitter always has a timeline region");
    let source_start = region.start_time - region.source_offset;
    let source_duration = emitter.duration;
    let time = source_start + normalized * source_duration;
    let value = keys.curve_value(selection.key);
    let cursor_icon = if matches!(keys, AutomationLaneKeys::Gradient(_)) {
        SystemCursorIcon::EwResize
    } else {
        SystemCursorIcon::Grabbing
    };
    state.automation_key_drag = Some(TimelineAutomationKeyDrag {
        selection: selection.clone(),
        source_start,
        source_duration,
        original_time: time,
        current_time: time,
        original_value: value,
        current_value: value,
    });
    override_cursor.0 = Some(EntityCursor::System(cursor_icon));
    **cursor = CursorIcon::System(cursor_icon);
    drag.propagate(false);
}

pub(super) fn select_automation_key(
    mut click: On<Pointer<Click>>,
    controls: Query<&TimelineAutomationKeySelection, With<TimelineAutomationKey>>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(selection) = controls.get(click.event_target()) else {
        return;
    };
    commands.trigger(ChoreographyAction::SelectAutomationKey(selection.clone()));
    click.propagate(false);
}

pub(super) fn move_automation_key_drag(
    mut drag_event: On<Pointer<Drag>>,
    controls: Query<&TimelineAutomationKeySelection, With<TimelineAutomationKey>>,
    canvases: Query<&ComputedNode, With<TimelineCanvas>>,
    window: Single<&Window, With<PrimaryWindow>>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    let Ok(selection) = controls.get(drag_event.event_target()) else {
        return;
    };
    let Some(mut drag) = state.automation_key_drag.clone() else {
        return;
    };
    if drag.selection != *selection {
        return;
    }
    drag_event.propagate(false);
    let width = canvases
        .iter()
        .map(|canvas| canvas.size().x)
        .fold(0.0, f32::max)
        .max(1.0);
    let logical_distance_x =
        screen_distance_to_logical(drag_event.distance.x, window.scale_factor());
    let candidate = drag.original_time + logical_distance_x / width * state.view.span();
    let (candidate, guide) =
        snap_automation_key_time(candidate, &session, state.snap, state.view, width);
    let Some(keys) = automation_lane_keys(&session.effect, &selection.lane) else {
        return;
    };
    let epsilon = drag.source_duration.max(0.001) * 0.0005;
    let lower = selection
        .key
        .checked_sub(1)
        .and_then(|index| keys.times().nth(index))
        .map_or(drag.source_start, |time| {
            drag.source_start + time * drag.source_duration + epsilon
        });
    let upper = keys
        .times()
        .nth(selection.key + 1)
        .map_or(drag.source_start + drag.source_duration, |time| {
            drag.source_start + time * drag.source_duration - epsilon
        });
    drag.current_time = candidate.clamp(lower.min(upper), upper.max(lower));
    if let Some(original_value) = drag.original_value {
        let graph = keys.graph_data();
        let original_top = graph.top_percent_for_value(original_value).unwrap_or(50.0);
        let logical_distance_y =
            screen_distance_to_logical(drag_event.distance.y, window.scale_factor());
        let candidate_top = original_top
            + logical_distance_y / state.automation_lane_height(&selection.lane).max(1.0) * 100.0;
        drag.current_value = graph.value_for_top_percent(candidate_top);
    }
    state.automation_key_drag = Some(drag);
    state.snap_guide = guide.filter(|time| (*time - candidate).abs() <= 0.0001);
}

pub(super) fn finish_automation_key_drag(
    mut drag_event: On<Pointer<DragEnd>>,
    controls: Query<&TimelineAutomationKeySelection, With<TimelineAutomationKey>>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut curves: ResMut<CurvesState>,
    localizer: Res<Localizer>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(selection) = controls.get(drag_event.event_target()) else {
        return;
    };
    let Some(drag) = state.automation_key_drag.take() else {
        return;
    };
    if drag.selection != *selection {
        state.automation_key_drag = Some(drag);
        return;
    }
    drag_event.propagate(false);
    state.snap_guide = None;
    override_cursor.0 = None;
    **cursor = CursorIcon::System(if drag.original_value.is_some() {
        SystemCursorIcon::Grab
    } else {
        SystemCursorIcon::EwResize
    });
    let time_changed = (drag.current_time - drag.original_time).abs() > 0.0001;
    let value_changed = drag
        .original_value
        .zip(drag.current_value)
        .is_some_and(|(original, current)| (current - original).abs() > 0.0001);
    if !time_changed && !value_changed {
        return;
    }
    let normalized =
        ((drag.current_time - drag.source_start) / drag.source_duration.max(0.001)).clamp(0.0, 1.0);
    let command = match automation_lane_value(&session.effect, &selection.lane) {
        Some(Value::Curve(mut curve)) => {
            let Some(mut key) = curve.keys.get(selection.key).copied() else {
                return;
            };
            key.time = normalized;
            if let Some(value) = drag.current_value {
                key.value = value;
            }
            curve.keys[selection.key] = key;
            let Some(command) = set_automation_lane_value_command(
                &session.effect,
                &selection.lane,
                Value::Curve(curve),
            ) else {
                return;
            };
            command
        }
        Some(Value::Vec3Curve(mut curves_value)) => {
            let Some(curve) = selection
                .lane
                .channel
                .and_then(|channel| curves_value.curves.get_mut(channel as usize))
            else {
                return;
            };
            let Some(mut key) = curve.keys.get(selection.key).copied() else {
                return;
            };
            key.time = normalized;
            if let Some(value) = drag.current_value {
                key.value = value;
            }
            curve.keys[selection.key] = key;
            let Some(command) = set_automation_lane_value_command(
                &session.effect,
                &selection.lane,
                Value::Vec3Curve(curves_value),
            ) else {
                return;
            };
            command
        }
        Some(Value::Gradient(mut gradient)) => {
            let Some(mut key) = gradient.keys.get(selection.key).copied() else {
                return;
            };
            key.time = normalized;
            gradient.keys[selection.key] = key;
            let Some(command) = set_automation_lane_value_command(
                &session.effect,
                &selection.lane,
                Value::Gradient(gradient),
            ) else {
                return;
            };
            command
        }
        _ => return,
    };
    if session.execute(
        localizer.text("timeline-move-automation-key-command"),
        command,
        true,
    ) {
        curves.select_key_channel(
            selection.lane.module,
            selection.lane.input,
            selection.key,
            selection.lane.channel,
        );
        state.selected_automation_key = Some(selection.clone());
    }
}

pub(super) fn handle_automation_action(
    action: &ChoreographyAction,
    commands: &mut Commands,
    session: &mut EditorSession,
    curves: &mut CurvesState,
    state: &mut TimelineState,
    localizer: &Localizer,
) -> bool {
    match action.clone() {
        ChoreographyAction::ToggleEmitterAutomation(emitter) => {
            state.context_emitter = None;
            state.color_picker_emitter = None;
            state.context_effect_clip = None;
            state.expanded_automation_emitters.insert(emitter);
            state.automation_menu_emitter =
                (state.automation_menu_emitter != Some(emitter)).then_some(emitter);
            session.ui_revision += 1;
        }
        ChoreographyAction::SetEmitterAutomationVisibility {
            emitter,
            lanes,
            visible,
        } => {
            state.expanded_automation_emitters.insert(emitter);
            if visible {
                state.visible_automation_lanes.extend(lanes);
            } else {
                for lane in lanes {
                    state.visible_automation_lanes.remove(&lane);
                }
                state.selected_automation_key = state
                    .selected_automation_key
                    .take()
                    .filter(|selection| selection.lane.emitter != emitter);
            }
            state.automation_menu_emitter = None;
            session.ui_revision += 1;
        }
        ChoreographyAction::SetAutomationLaneVisibility { lane, visible } => {
            state.expanded_automation_emitters.insert(lane.emitter);
            if visible {
                state.visible_automation_lanes.insert(lane);
            } else {
                state.visible_automation_lanes.remove(&lane);
                state.selected_automation_key = state
                    .selected_automation_key
                    .take()
                    .filter(|selection| selection.lane != lane);
            }
            session.ui_revision += 1;
        }
        ChoreographyAction::SelectAutomationKey(selection) => {
            if automation_lane_keys(&session.effect, &selection.lane)
                .is_some_and(|keys| selection.key < keys.len())
            {
                let selected_region = state
                    .selected_emitter_region
                    .filter(|(emitter, _)| *emitter == selection.lane.emitter);
                state.select_only_emitter(selection.lane.emitter);
                if let Some((emitter, region)) = selected_region {
                    state.select_only_emitter_region(emitter, region);
                    session.select_emitter_region(emitter, region);
                } else {
                    session.select_emitter(selection.lane.emitter);
                }
                curves.select_key_channel(
                    selection.lane.module,
                    selection.lane.input,
                    selection.key,
                    selection.lane.channel,
                );
                state.selected_automation_key = Some(selection);
                state.inspected_child = None;
                session.status = localizer.text("timeline-selected-automation-key");
                session.ui_revision += 1;
            }
        }
        ChoreographyAction::AddAutomationKey(lane) => {
            let Some(emitter) = session
                .effect
                .emitters
                .iter()
                .find(|emitter| emitter.id == lane.emitter)
            else {
                return true;
            };
            let Some(region) = state
                .selected_emitter_region
                .filter(|(selected_emitter, _)| *selected_emitter == emitter.id)
                .and_then(|(_, region)| emitter.timeline_region(region))
                .or_else(|| emitter.timeline_regions().into_iter().next())
            else {
                return true;
            };
            let source_start = region.start_time - region.source_offset;
            let duration = emitter.duration.max(0.001);
            let normalized_time = ((session.time() - source_start) / duration).clamp(0.0, 1.0);
            commands.trigger(ChoreographyAction::AddAutomationKeyAt {
                lane,
                normalized_time_bits: normalized_time.to_bits(),
                value_bits: None,
            });
        }
        ChoreographyAction::AddAutomationKeyAt {
            lane,
            normalized_time_bits,
            value_bits,
        } => {
            let normalized_time = f32::from_bits(normalized_time_bits).clamp(0.0, 1.0);
            let Some(keys) = automation_lane_keys(&session.effect, &lane) else {
                return true;
            };
            if let Some(index) = keys
                .times()
                .position(|time| (time - normalized_time).abs() <= 0.0005)
            {
                commands.trigger(ChoreographyAction::SelectAutomationKey(
                    TimelineAutomationKeySelection { lane, key: index },
                ));
                return true;
            }
            let index = keys
                .times()
                .position(|time| time > normalized_time)
                .unwrap_or_else(|| keys.len());
            let command = match automation_lane_value(&session.effect, &lane) {
                Some(Value::Curve(mut curve)) => {
                    let value = value_bits.map_or_else(
                        || curve_stored_sample(&curve, normalized_time),
                        f32::from_bits,
                    );
                    curve
                        .keys
                        .insert(index, CurveKey::new(normalized_time, value));
                    let Some(command) = set_automation_lane_value_command(
                        &session.effect,
                        &lane,
                        Value::Curve(curve),
                    ) else {
                        return true;
                    };
                    command
                }
                Some(Value::Vec3Curve(mut curves)) => {
                    let Some(curve) = lane
                        .channel
                        .and_then(|channel| curves.curves.get_mut(channel as usize))
                    else {
                        return true;
                    };
                    let value = value_bits.map_or_else(
                        || curve_stored_sample(curve, normalized_time),
                        f32::from_bits,
                    );
                    curve
                        .keys
                        .insert(index, CurveKey::new(normalized_time, value));
                    let Some(command) = set_automation_lane_value_command(
                        &session.effect,
                        &lane,
                        Value::Vec3Curve(curves),
                    ) else {
                        return true;
                    };
                    command
                }
                Some(Value::Gradient(mut gradient)) => {
                    let color = gradient.sample(normalized_time);
                    gradient
                        .keys
                        .insert(index, ColorKey::new(normalized_time, color));
                    let Some(command) = set_automation_lane_value_command(
                        &session.effect,
                        &lane,
                        Value::Gradient(gradient),
                    ) else {
                        return true;
                    };
                    command
                }
                _ => return true,
            };
            if session.execute(
                localizer.text("timeline-add-automation-key-command"),
                command,
                true,
            ) {
                state.select_only_emitter(lane.emitter);
                session.select_emitter(lane.emitter);
                curves.select_key_channel(lane.module, lane.input, index, lane.channel);
                state.selected_automation_key =
                    Some(TimelineAutomationKeySelection { lane, key: index });
            }
        }
        ChoreographyAction::DeleteAutomationKey(selection) => {
            let Some(keys) = automation_lane_keys(&session.effect, &selection.lane) else {
                return true;
            };
            if keys.len() <= 2 || selection.key >= keys.len() {
                session.status = localizer.text("timeline-automation-keep-two-keys");
                return true;
            }
            let command = match automation_lane_value(&session.effect, &selection.lane) {
                Some(Value::Curve(mut curve)) => {
                    curve.keys.remove(selection.key);
                    let Some(command) = set_automation_lane_value_command(
                        &session.effect,
                        &selection.lane,
                        Value::Curve(curve),
                    ) else {
                        return true;
                    };
                    command
                }
                Some(Value::Vec3Curve(mut curves)) => {
                    let Some(curve) = selection
                        .lane
                        .channel
                        .and_then(|channel| curves.curves.get_mut(channel as usize))
                    else {
                        return true;
                    };
                    curve.keys.remove(selection.key);
                    let Some(command) = set_automation_lane_value_command(
                        &session.effect,
                        &selection.lane,
                        Value::Vec3Curve(curves),
                    ) else {
                        return true;
                    };
                    command
                }
                Some(Value::Gradient(mut gradient)) => {
                    gradient.keys.remove(selection.key);
                    let Some(command) = set_automation_lane_value_command(
                        &session.effect,
                        &selection.lane,
                        Value::Gradient(gradient),
                    ) else {
                        return true;
                    };
                    command
                }
                _ => return true,
            };
            if session.execute(
                localizer.text("timeline-delete-automation-key-command"),
                command,
                true,
            ) {
                let next = selection.key.saturating_sub(1).min(keys.len() - 2);
                curves.select_key_channel(
                    selection.lane.module,
                    selection.lane.input,
                    next,
                    selection.lane.channel,
                );
                state.selected_automation_key = Some(TimelineAutomationKeySelection {
                    lane: selection.lane,
                    key: next,
                });
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn lanes_project_existing_curve_and_gradient_parameters() {
        let session = test_support::session_with_timing_slack();
        let emitter = &session.effect.emitters[0];
        let registry = EditorModuleRegistry::default();
        let localizer = Localizer::new("en-US").unwrap();

        let lanes = emitter_automation_lanes(&session.effect, emitter, &registry, &localizer);

        assert_eq!(lanes.len(), automation_lane_count(emitter));
        assert!(lanes.iter().any(|lane| lane.id.parameter == "size"));
        assert!(lanes.iter().any(|lane| lane.id.parameter == "opacity"));
        assert!(lanes.iter().any(|lane| lane.id.parameter == "color"));
        assert!(lanes.iter().all(|lane| lane.keys.len() >= 2));

        let mut constant_size = emitter.clone();
        let appearance = constant_size
            .modules
            .iter_mut()
            .find(|module| matches!(&module.parameters, ModuleParameters::Appearance { .. }))
            .unwrap();
        appearance.property_sources.insert(
            "size".into(),
            aestra_core::PropertySource::Curve(aestra_core::PropertyEvaluationDomain::ParticleLife),
        );
        let ModuleParameters::Appearance { size, .. } = &mut appearance.parameters else {
            panic!("fixture emitter should have appearance automation");
        };
        size.keys.truncate(1);
        let one_key_lanes =
            emitter_automation_lanes(&session.effect, &constant_size, &registry, &localizer);
        assert!(one_key_lanes.iter().any(|lane| lane.id.parameter == "size"));
        constant_size
            .modules
            .iter_mut()
            .find(|module| matches!(&module.parameters, ModuleParameters::Appearance { .. }))
            .unwrap()
            .property_sources
            .insert("size".into(), aestra_core::PropertySource::Constant);
        let constant_lanes =
            emitter_automation_lanes(&session.effect, &constant_size, &registry, &localizer);
        assert!(
            !constant_lanes
                .iter()
                .any(|lane| lane.id.parameter == "size")
        );
        assert_eq!(constant_lanes.len(), automation_lane_count(&constant_size));
    }

    #[test]
    fn emitter_time_curve_projects_as_an_automation_lane() {
        let mut session = test_support::session_with_timing_slack();
        let emitter = &mut session.effect.emitters[0];
        let emission = emitter
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == aestra_core::MODULE_EMISSION)
            .unwrap();
        let source =
            aestra_core::PropertySource::Curve(aestra_core::PropertyEvaluationDomain::EmitterTime);
        emission
            .property_sources
            .insert("spawn_rate".into(), source);
        emission.property_source_values.insert(
            "spawn_rate".into(),
            vec![aestra_core::PropertySourceValue::new(
                source,
                Value::Curve(aestra_core::Curve::new(vec![
                    CurveKey::new(0.0, 4.0),
                    CurveKey::new(1.0, 24.0),
                ])),
            )],
        );
        let registry = EditorModuleRegistry::default();
        let localizer = Localizer::new("en-US").unwrap();

        let lanes = emitter_automation_lanes(
            &session.effect,
            &session.effect.emitters[0],
            &registry,
            &localizer,
        );

        assert!(lanes.iter().any(|lane| lane.id.parameter == "spawn_rate"));
        assert_eq!(
            lanes.len(),
            automation_lane_count(&session.effect.emitters[0])
        );
    }
}
