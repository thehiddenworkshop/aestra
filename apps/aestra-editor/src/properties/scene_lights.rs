//! F7B2: representative (event-driven) scene lights, not per-particle host events.
use super::*;
use crate::feathers::{
    automation_curve::{self, AutomationCurveData, AutomationCurvePoint, AutomationGradientPoint},
    number_input::ScrubbableNumber,
};
use aestra_core::{
    EventAggregation, EventRouteId, LightColorParameter, PointLightBinding, PointLightPulse,
};

#[cfg(test)]
mod tests;
mod ui;
pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    ui::spawn(parent, session, localizer);
}

#[derive(Resource, Default)]
struct LightClipboard(Option<PointLightBinding>);

#[derive(Component)]
struct LightInspector(EventRouteId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LightCurve {
    Intensity,
    Range,
}

impl LightCurve {
    fn get(self, binding: &PointLightBinding) -> &Curve {
        match self {
            Self::Intensity => &binding.pulse.intensity_lumens,
            Self::Range => &binding.pulse.range,
        }
    }
    fn get_mut(self, binding: &mut PointLightBinding) -> &mut Curve {
        match self {
            Self::Intensity => &mut binding.pulse.intensity_lumens,
            Self::Range => &mut binding.pulse.range,
        }
    }
}

#[derive(Component, Debug, Clone, Copy)]
enum LightAction {
    Add(EventRouteId),
    Remove(EventRouteId),
    Route(EventRouteId, EventRouteId),
    ColorSource(EventRouteId, Option<ParameterId>),
    Copy(EventRouteId),
    Paste(EventRouteId),
    Interpolation(EventRouteId, LightCurve, aestra_core::CurveInterpolation),
    AddKey(EventRouteId, LightCurve),
    RemoveKey(EventRouteId, LightCurve, usize),
    AddColorKey(ParameterId),
    RemoveColorKey(ParameterId, usize),
}

#[derive(Component, Debug, Clone, Copy)]
enum LightNumber {
    Radius(EventRouteId),
    Duration(EventRouteId),
    Color(EventRouteId, usize),
    Age(EventRouteId),
    CurveTime(EventRouteId, LightCurve, usize),
    CurveValue(EventRouteId, LightCurve, usize),
    GradientTime(ParameterId, usize),
    GradientChannel(ParameterId, usize, usize),
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<LightClipboard>()
        .add_observer(activate)
        .add_observer(change_number)
        .add_systems(Update, clipboard_shortcuts.in_set(PropertiesSet::Input))
        .add_systems(Update, sync_numbers.in_set(PropertiesSet::Sync));
}

fn clipboard_shortcuts(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    focus: Option<Res<bevy::input_focus::InputFocus>>,
    editable: Query<(), With<bevy::text::EditableText>>,
    inspectors: Query<(&LightInspector, &RelativeCursorPosition)>,
    mut session: ResMut<EditorSession>,
    mut clipboard: ResMut<LightClipboard>,
    localizer: Res<Localizer>,
) {
    let Some(keys) = keys else {
        return;
    };
    if !(keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight))
        || focus
            .as_ref()
            .and_then(|focus| focus.get())
            .is_some_and(|entity| editable.contains(entity))
    {
        return;
    }
    let Some(id) = inspectors
        .iter()
        .find_map(|(inspector, cursor)| cursor.cursor_over().then_some(inspector.0))
    else {
        return;
    };
    if keys.just_pressed(KeyCode::KeyC) {
        apply(
            LightAction::Copy(id),
            &mut session,
            &mut clipboard,
            &localizer,
        );
    } else if keys.just_pressed(KeyCode::KeyV) {
        apply(
            LightAction::Paste(id),
            &mut session,
            &mut clipboard,
            &localizer,
        );
    }
}

/// Exactly the runtime's representative-route contract, excluding routes already in use.
fn compatible_routes(effect: &EffectAsset, owner: Option<EventRouteId>) -> Vec<EventRouteId> {
    effect
        .particle_outputs
        .iter()
        .filter(|route| {
            route.aggregation == EventAggregation::FirstPerTick
                && effect
                    .emitters
                    .iter()
                    .any(|emitter| emitter.id == route.source)
                && effect
                    .event_outputs
                    .iter()
                    .any(|output| output.id == route.output)
                && effect
                    .particle_outputs
                    .iter()
                    .filter(|other| other.source == route.source && other.output == route.output)
                    .count()
                    == 1
                && !effect
                    .point_lights
                    .iter()
                    .any(|binding| binding.route == route.id && Some(binding.id) != owner)
        })
        .map(|route| route.id)
        .collect()
}

fn edit_binding(
    session: &mut EditorSession,
    id: EventRouteId,
    label: String,
    edit: impl FnOnce(&mut PointLightBinding),
) -> bool {
    let Some(original) = session
        .effect
        .point_lights
        .iter()
        .find(|binding| binding.id == id)
    else {
        return false;
    };
    let mut binding = original.clone();
    edit(&mut binding);
    if binding == *original {
        return false;
    }
    session.execute(label, EffectCommand::SetPointLight { id, binding }, true)
}

/// Paste settings only: keep the destination's route, binding and curve identities. A missing
/// gradient parameter is rejected by validation, never rebound by name or to another parameter.
fn paste_settings(destination: &mut PointLightBinding, source: &PointLightBinding) {
    let intensity_id = destination.pulse.intensity_lumens.id;
    let range_id = destination.pulse.range.id;
    destination.pulse = source.pulse.clone();
    destination.pulse.intensity_lumens.id = intensity_id;
    destination.pulse.range.id = range_id;
    destination.color_parameter = source.color_parameter.clone();
}

fn apply(
    action: LightAction,
    session: &mut EditorSession,
    clipboard: &mut LightClipboard,
    localizer: &Localizer,
) -> bool {
    let label = localizer.text("scene-lights-edit");
    match action {
        LightAction::Copy(id) => {
            clipboard.0 = session
                .effect
                .point_lights
                .iter()
                .find(|item| item.id == id)
                .cloned();
            if clipboard.0.is_none() {
                return false;
            }
            session.status = localizer.text("scene-lights-copied");
            session.ui_revision += 1;
            false
        }
        LightAction::Add(route) => {
            if !compatible_routes(&session.effect, None).contains(&route) {
                return false;
            }
            let binding = PointLightBinding::new(
                route,
                PointLightPulse::flash([1.0; 3], 500_000.0, 80.0, 0.6),
            );
            let id = binding.id;
            let changed = session.execute(
                localizer.text("scene-lights-add-command"),
                EffectCommand::AddPointLight {
                    binding,
                    index: session.effect.point_lights.len(),
                },
                true,
            );
            if changed {
                session.selection.primary = SemanticTarget::PointLight(id);
            }
            changed
        }
        LightAction::Remove(id) => session.execute(
            localizer.text("scene-lights-remove-command"),
            EffectCommand::RemovePointLight { id },
            true,
        ),
        LightAction::Route(id, route) => {
            if !compatible_routes(&session.effect, Some(id)).contains(&route) {
                return false;
            }
            edit_binding(session, id, label, |binding| binding.route = route)
        }
        LightAction::ColorSource(id, parameter) => edit_binding(session, id, label, |binding| {
            binding.color_parameter = parameter.map(|parameter| LightColorParameter {
                parameter,
                normalized_age: 0.5,
            });
        }),
        LightAction::Paste(id) => {
            let Some(source) = clipboard.0.as_ref() else {
                session.status = localizer.text("scene-lights-clipboard-empty");
                session.ui_revision += 1;
                return false;
            };
            edit_binding(
                session,
                id,
                localizer.text("scene-lights-paste-command"),
                |binding| paste_settings(binding, source),
            )
        }
        LightAction::Interpolation(id, kind, interpolation) => {
            edit_binding(session, id, label, |binding| {
                kind.get_mut(binding).interpolation = interpolation
            })
        }
        LightAction::AddKey(id, kind) => edit_binding(session, id, label, |binding| {
            let curve = kind.get_mut(binding);
            if curve.keys.len() >= aestra_core::MAX_LIGHT_CURVE_KEYS {
                return;
            }
            if let Some(time) = new_key_time(curve.keys.iter().map(|key| key.time)) {
                let value = curve
                    .keys
                    .first()
                    .map_or(0.0, |_| stored_sample(curve, time));
                curve.keys.push(CurveKey::new(time, value));
                curve.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
            }
        }),
        LightAction::RemoveKey(id, kind, index) => edit_binding(session, id, label, |binding| {
            let curve = kind.get_mut(binding);
            if curve.keys.len() > 1 && index < curve.keys.len() {
                curve.keys.remove(index);
            }
        }),
        LightAction::AddColorKey(parameter) => {
            edit_gradient(session, parameter, label, |gradient| {
                if let Some(time) = new_key_time(gradient.keys.iter().map(|key| key.time)) {
                    let color = gradient.sample(time);
                    gradient.keys.push(ColorKey::new(time, color));
                    gradient.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
                }
            })
        }
        LightAction::RemoveColorKey(parameter, index) => {
            edit_gradient(session, parameter, label, |gradient| {
                if gradient.keys.len() > 1 && index < gradient.keys.len() {
                    gradient.keys.remove(index);
                }
            })
        }
    }
}

fn new_key_time(times: impl Iterator<Item = f32>) -> Option<f32> {
    let mut times = times.chain([0.0, 1.0]).collect::<Vec<_>>();
    times.sort_by(f32::total_cmp);
    let pair = times
        .windows(2)
        .max_by(|a, b| (a[1] - a[0]).total_cmp(&(b[1] - b[0])))?;
    let time = pair[0] + (pair[1] - pair[0]) * 0.5;
    (time > pair[0] && time < pair[1]).then_some(time)
}

fn stored_sample(curve: &Curve, time: f32) -> f32 {
    let output = curve.sample(time);
    curve.output_range.map_or(output, |range| {
        if range.max == range.min {
            0.0
        } else {
            (output - range.min) / (range.max - range.min)
        }
    })
}

fn edit_gradient(
    session: &mut EditorSession,
    id: ParameterId,
    label: String,
    edit: impl FnOnce(&mut Gradient),
) -> bool {
    let Some(original) = session.effect.parameters.iter().find(|item| item.id == id) else {
        return false;
    };
    let mut parameter = original.clone();
    let Value::Gradient(gradient) = &mut parameter.default else {
        return false;
    };
    edit(gradient);
    if parameter == *original {
        return false;
    }
    session.execute(label, EffectCommand::SetParameter { id, parameter }, true)
}

fn activate(
    event: On<Activate>,
    actions: Query<&LightAction>,
    mut session: ResMut<EditorSession>,
    mut clipboard: ResMut<LightClipboard>,
    localizer: Res<Localizer>,
) {
    if let Ok(action) = actions.get(event.entity) {
        apply(*action, &mut session, &mut clipboard, &localizer);
    }
}

fn number_value(effect: &EffectAsset, control: LightNumber) -> Option<f32> {
    let binding = |id| effect.point_lights.iter().find(|binding| binding.id == id);
    match control {
        LightNumber::Radius(id) => Some(binding(id)?.pulse.radius),
        LightNumber::Duration(id) => Some(binding(id)?.pulse.duration_seconds),
        LightNumber::Color(id, channel) => binding(id)?.pulse.linear_color.get(channel).copied(),
        LightNumber::Age(id) => Some(binding(id)?.color_parameter.as_ref()?.normalized_age),
        LightNumber::CurveTime(id, kind, index) => {
            Some(kind.get(binding(id)?).keys.get(index)?.time)
        }
        LightNumber::CurveValue(id, kind, index) => {
            let curve = kind.get(binding(id)?);
            Some(curve.output_value(curve.keys.get(index)?.value))
        }
        LightNumber::GradientTime(id, index) | LightNumber::GradientChannel(id, index, _) => {
            let Value::Gradient(gradient) =
                &effect.parameters.iter().find(|item| item.id == id)?.default
            else {
                return None;
            };
            let key = gradient.keys.get(index)?;
            match control {
                LightNumber::GradientChannel(_, _, channel) => key.color.get(channel).copied(),
                _ => Some(key.time),
            }
        }
    }
}

fn set_number(
    session: &mut EditorSession,
    control: LightNumber,
    value: f32,
    label: String,
) -> bool {
    if !value.is_finite() {
        return false;
    }
    match control {
        LightNumber::GradientTime(id, index) | LightNumber::GradientChannel(id, index, _) => {
            edit_gradient(session, id, label, |gradient| {
                if let Some(key) = gradient.keys.get_mut(index) {
                    match control {
                        LightNumber::GradientChannel(_, _, channel) => {
                            if let Some(component) = key.color.get_mut(channel) {
                                *component = value;
                            }
                        }
                        _ => key.time = value,
                    }
                }
            })
        }
        _ => {
            let id = match control {
                LightNumber::Radius(id)
                | LightNumber::Duration(id)
                | LightNumber::Color(id, _)
                | LightNumber::Age(id)
                | LightNumber::CurveTime(id, _, _)
                | LightNumber::CurveValue(id, _, _) => id,
                _ => unreachable!(),
            };
            edit_binding(session, id, label, |binding| match control {
                LightNumber::Radius(_) => binding.pulse.radius = value,
                LightNumber::Duration(_) => binding.pulse.duration_seconds = value,
                LightNumber::Color(_, channel) => {
                    if let Some(component) = binding.pulse.linear_color.get_mut(channel) {
                        *component = value;
                    }
                }
                LightNumber::Age(_) => {
                    if let Some(color) = &mut binding.color_parameter {
                        color.normalized_age = value;
                    }
                }
                LightNumber::CurveTime(_, kind, index) => {
                    if let Some(key) = kind.get_mut(binding).keys.get_mut(index) {
                        key.time = value;
                    }
                }
                LightNumber::CurveValue(_, kind, index) => {
                    // Numeric controls display output units, including normalized authored curves.
                    let curve = kind.get_mut(binding);
                    let stored = curve.output_range.map_or(value, |range| {
                        if range.min == range.max {
                            0.0
                        } else {
                            (value - range.min) / (range.max - range.min)
                        }
                    });
                    if let Some(key) = curve.keys.get_mut(index) {
                        key.value = stored;
                    }
                }
                _ => unreachable!(),
            })
        }
    }
}

fn change_number(
    change: On<ValueChange<f32>>,
    controls: Query<&LightNumber>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
    mut commands: Commands,
) {
    // Commit once on release: scrubbing does not rebuild the inspector or runtime every frame.
    if !change.is_final {
        return;
    }
    if let Ok(control) = controls.get(change.source) {
        set_number(
            &mut session,
            *control,
            change.value,
            localizer.text("scene-lights-edit"),
        );
        if let Some(value) = number_value(&session.effect, *control) {
            commands.trigger(UpdateNumberInput {
                entity: change.source,
                value: NumberInputValue::F32(value),
            });
            commands
                .entity(change.source)
                .entry::<ScrubbableNumber>()
                .and_modify(move |mut scrub| scrub.value = value);
        }
    }
}

fn sync_numbers(
    mut commands: Commands,
    effect: Res<EditorSession>,
    controls: Query<(Entity, &LightNumber), Added<LightNumber>>,
) {
    for (entity, control) in &controls {
        if let Some(value) = number_value(&effect.effect, *control) {
            commands.trigger(UpdateNumberInput {
                entity,
                value: NumberInputValue::F32(value),
            });
        }
    }
}
