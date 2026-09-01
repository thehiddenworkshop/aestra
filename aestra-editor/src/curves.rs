//! Curves workspace, curve/gradient selection, and semantic key editing.

use crate::feathers::automation_curve::{
    self, AutomationCurveData, AutomationCurvePoint, AutomationGradientPoint,
};
use crate::*;
use aestra_bevy::{ColorKey, CurveKey, ModuleId, ModuleInstance, Value};
use aestra_compiler::{InputControl, InputMetadata, ModuleRegistry};
use bevy::{
    feathers::cursor::EntityCursor, input_focus::InputFocus, text::EditableText,
    ui::RelativeCursorPosition, ui_widgets::Activate, window::SystemCursorIcon,
};

pub(crate) struct EditorCurvesPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CurvesSet {
    Actions,
}

impl Plugin for EditorCurvesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurvesState>()
            .add_observer(queue_curves_action_activation)
            .add_systems(
                Update,
                (curves_keyboard_input, handle_curves_actions)
                    .chain()
                    .in_set(CurvesSet::Actions),
            );
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurvesAction {
    OpenInput(ModuleId, u8),
    SelectVectorChannel(u8),
    AddKey,
    DeleteKey,
    AdjustTime(i8),
    AdjustCurveValue(i8),
    AdjustGradientChannel(u8, i8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ComplexSelection {
    pub(crate) module: ModuleId,
    pub(crate) input: u8,
    pub(crate) key: usize,
}

#[derive(Resource, Default)]
pub(crate) struct CurvesState {
    complex: Option<ComplexSelection>,
    vector_channel: u8,
}

impl CurvesState {
    pub(crate) fn clear(&mut self) {
        self.complex = None;
        self.vector_channel = 0;
    }

    pub(crate) fn select_key_channel(
        &mut self,
        module: ModuleId,
        input: u8,
        key: usize,
        vector_channel: Option<u8>,
    ) {
        self.complex = Some(ComplexSelection { module, input, key });
        self.vector_channel = vector_channel.unwrap_or(0).min(2);
    }

    pub(crate) fn selected_key(&self) -> Option<ComplexSelection> {
        self.complex
    }

    pub(crate) fn selected_vector_channel(&self) -> u8 {
        self.vector_channel
    }

    #[cfg(test)]
    pub(crate) fn has_selection(&self) -> bool {
        self.complex.is_some()
    }

    #[cfg(test)]
    pub(crate) fn select_for_test(&mut self, module: ModuleId, input: u8, key: usize) {
        self.complex = Some(ComplexSelection { module, input, key });
    }
}

#[derive(Component)]
struct CurveGraph;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct CurveGraphKey(ComplexSelection);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct CurveGraphValueLabel(ComplexSelection);

fn curves_keyboard_input(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    graphs: Query<&RelativeCursorPosition, With<CurveGraph>>,
    focus: Option<Res<InputFocus>>,
    editable_text: Query<(), With<EditableText>>,
    mut session: ResMut<EditorSession>,
    registry: Res<EditorModuleRegistry>,
    mut state: ResMut<CurvesState>,
) {
    let Some(keys) = keys else {
        return;
    };
    let editing_text = focus
        .as_ref()
        .and_then(|focus| focus.get())
        .is_some_and(|entity| editable_text.contains(entity));
    if editing_text {
        return;
    }
    let cursor = graphs
        .iter()
        .find_map(|cursor| cursor.normalized.filter(|_| cursor.cursor_over()));
    let Some(cursor) = cursor else {
        return;
    };
    if keys.just_pressed(KeyCode::Insert) {
        add_complex_key_at_pointer(&mut session, &registry.0, &mut state, cursor);
    } else if keys.just_pressed(KeyCode::Delete) {
        edit_complex_key(
            &mut session,
            &registry.0,
            &mut state,
            ComplexKeyEdit::Delete,
        );
    }
}

fn queue_curves_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<CurvesAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

#[allow(clippy::type_complexity)]
fn handle_curves_actions(
    mut commands: Commands,
    mut actions: Query<
        (
            Entity,
            &Interaction,
            &CurvesAction,
            Option<&FeathersActionButton>,
            Option<&PendingFeathersActivation>,
            &mut BackgroundColor,
        ),
        (
            Changed<Interaction>,
            Or<(With<Button>, With<FeathersActionButton>)>,
        ),
    >,
    mut session: ResMut<EditorSession>,
    registry: Res<EditorModuleRegistry>,
    mut state: ResMut<CurvesState>,
    mut layout: ResMut<WorkspaceLayout>,
) {
    for (entity, interaction, action, feathers, pending, mut background) in &mut actions {
        match *interaction {
            Interaction::Hovered if feathers.is_none() => background.0 = theme::BUTTON_HOVER,
            Interaction::None if feathers.is_none() => background.0 = theme::BUTTON,
            Interaction::Pressed => {
                if feathers.is_some() {
                    if pending.is_none() {
                        continue;
                    }
                    commands
                        .entity(entity)
                        .remove::<PendingFeathersActivation>()
                        .insert(Interaction::None);
                } else {
                    background.0 = theme::ACCENT_DIM;
                }
                match *action {
                    CurvesAction::OpenInput(module, input) => {
                        reveal_dock_panel(&mut layout, &mut session, DockPanel::Curves);
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

pub(crate) fn spawn_curves_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
    workspace: &CurvesState,
    localizer: &Localizer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|workspace_panel| {
            workspace_panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(38.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(14.0)),
                        column_gap: Val::Px(8.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                ))
                .with_children(|header| {
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        Text::new(localizer.text("curves-header-hint")),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                    ));
                });
            workspace_panel
                .spawn(Node {
                    flex_grow: 1.0,
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|body| {
                    spawn_complex_input_list(body, session, registry, workspace, localizer);
                    let Some(selection) = workspace.complex else {
                        body.spawn((
                            Text::new(localizer.text("curves-choose-property")),
                            TextFont {
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_MUTED),
                            Node {
                                margin: UiRect::all(Val::Px(28.0)),
                                ..default()
                            },
                        ));
                        return;
                    };
                    let Some((module, input, value)) =
                        resolve_complex_input(session, registry, selection)
                    else {
                        body.spawn((
                            Text::new(localizer.text("curves-property-missing")),
                            TextFont {
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(Color::srgb(1.0, 0.38, 0.32)),
                            Node {
                                margin: UiRect::all(Val::Px(28.0)),
                                ..default()
                            },
                        ));
                        return;
                    };
                    if !complex_input_is_visible(module, input) {
                        body.spawn((
                            Text::new(localizer.text("curves-choose-property")),
                            TextFont {
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_MUTED),
                            Node {
                                margin: UiRect::all(Val::Px(28.0)),
                                ..default()
                            },
                        ));
                        return;
                    }
                    body.spawn(Node {
                        flex_grow: 1.0,
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(8.0)),
                        row_gap: Val::Px(6.0),
                        ..default()
                    })
                    .with_children(|editor| match value {
                        Value::Curve(curve) => spawn_curve_graph(
                            editor,
                            module.id,
                            selection.input,
                            input,
                            &curve,
                            selection.key,
                            None,
                            localizer,
                        ),
                        Value::Vec3Curve(curves) => {
                            editor
                                .spawn(Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(30.0),
                                    align_items: AlignItems::Center,
                                    column_gap: Val::Px(4.0),
                                    ..default()
                                })
                                .with_children(|channels| {
                                    for (channel, label) in ["X", "Y", "Z"].into_iter().enumerate()
                                    {
                                        vector_channel_button(
                                            channels,
                                            label,
                                            channel as u8,
                                            workspace.vector_channel == channel as u8,
                                        );
                                    }
                                });
                            let channel = workspace.vector_channel.min(2);
                            spawn_curve_graph(
                                editor,
                                module.id,
                                selection.input,
                                input,
                                &curves.curves[channel as usize],
                                selection.key,
                                Some(channel),
                                localizer,
                            );
                        }
                        Value::Gradient(gradient) => spawn_gradient_graph(
                            editor,
                            module.id,
                            selection.input,
                            input,
                            &gradient,
                            selection.key,
                            localizer,
                        ),
                        _ => {}
                    });
                });
        });
}

fn spawn_complex_input_list(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
    workspace: &CurvesState,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Node {
                width: Val::Px(224.0),
                height: Val::Percent(100.0),
                border: UiRect::right(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|column| {
            spawn_vertical_scroll_area(
                column,
                ScrollMemoryKey::Curves,
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(7.0)),
                    row_gap: Val::Px(4.0),
                    ..default()
                },
                |list| {
                    for module in &session.selected_layer().modules {
                        let Some(metadata) = registry.0.get(&module.module_type) else {
                            continue;
                        };
                        for (input_index, input) in metadata.inputs.iter().enumerate() {
                            if !complex_input_is_visible(module, input) {
                                continue;
                            }
                            let selected = workspace.complex.is_some_and(|selection| {
                                selection.module == module.id
                                    && selection.input == input_index as u8
                            });
                            let display_name = localized_properties_input(
                                localizer,
                                input.name,
                                input.display_name,
                                false,
                            );
                            parent_list_button(
                                list,
                                &format!("{} / {display_name}", metadata.display_name),
                                CurvesAction::OpenInput(module.id, input_index as u8),
                                selected,
                            );
                        }
                    }
                },
            );
        });
}

fn complex_input_is_visible(module: &ModuleInstance, input: &InputMetadata) -> bool {
    matches!(
        module.property_source(input.name),
        Some(aestra_bevy::PropertySource::Curve(_))
            | Some(aestra_bevy::PropertySource::Gradient(_))
    )
}

fn parent_list_button<A: Component>(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: A,
    selected: bool,
) {
    parent
        .spawn((
            Button,
            EditorNativeControl,
            action,
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(28.0),
                padding: UiRect::horizontal(Val::Px(7.0)),
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(if selected {
                theme::SELECTION
            } else {
                theme::BUTTON
            }),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(if selected {
                    theme::ACCENT
                } else {
                    theme::TEXT_MUTED
                }),
            ));
        });
}

fn vector_channel_button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    channel: u8,
    selected: bool,
) {
    parent
        .spawn((
            Button,
            EditorNativeControl,
            CurvesAction::SelectVectorChannel(channel),
            Node {
                width: Val::Px(30.0),
                height: Val::Px(24.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(if selected {
                theme::SELECTION
            } else {
                theme::BUTTON
            }),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(if selected { theme::ACCENT } else { theme::TEXT }),
            Pickable::IGNORE,
        ));
}

fn resolve_complex_input<'a>(
    session: &'a EditorSession,
    registry: &'a EditorModuleRegistry,
    selection: ComplexSelection,
) -> Option<(&'a ModuleInstance, &'a InputMetadata, Value)> {
    let module = session
        .selected_layer()
        .modules
        .iter()
        .find(|module| module.id == selection.module)?;
    let input = registry
        .0
        .get(&module.module_type)?
        .inputs
        .get(selection.input as usize)?;
    let value = curve_module_parameter(session, module, input.name)?;
    Some((module, input, value))
}

fn curve_module_parameter(
    session: &EditorSession,
    module: &ModuleInstance,
    parameter: &str,
) -> Option<Value> {
    if let Some(parameter_id) = module.bindings.get(parameter) {
        return session
            .effect
            .parameters
            .iter()
            .find(|candidate| candidate.id == *parameter_id)
            .map(|parameter| parameter.default.clone());
    }
    module_parameter(module, parameter)
}

fn editable_curve(
    session: &EditorSession,
    module: ModuleId,
    parameter: &str,
    vector_channel: Option<u8>,
) -> Option<aestra_bevy::Curve> {
    let module = session
        .selected_layer()
        .modules
        .iter()
        .find(|candidate| candidate.id == module)?;
    match (
        curve_module_parameter(session, module, parameter)?,
        vector_channel,
    ) {
        (Value::Curve(curve), None) => Some(curve),
        (Value::Vec3Curve(curves), Some(channel)) => {
            curves.curves.get(channel.min(2) as usize).cloned()
        }
        _ => None,
    }
}

fn update_vector_curve(
    session: &mut EditorSession,
    module: ModuleId,
    parameter: &str,
    channel: u8,
    label: &str,
    edit: impl FnOnce(&mut aestra_bevy::Curve) -> bool,
) -> bool {
    let Some(module_instance) = session
        .selected_layer()
        .modules
        .iter()
        .find(|candidate| candidate.id == module)
    else {
        return false;
    };
    let Some(Value::Vec3Curve(mut curves)) =
        curve_module_parameter(session, module_instance, parameter)
    else {
        return false;
    };
    let Some(curve) = curves.curves.get_mut(channel.min(2) as usize) else {
        return false;
    };
    if !edit(curve) {
        return false;
    }
    session.set_active_module_property_value(module, parameter, Value::Vec3Curve(curves), label)
}

fn set_editable_curve_key(
    session: &mut EditorSession,
    module: ModuleId,
    parameter: &str,
    vector_channel: Option<u8>,
    index: usize,
    key: CurveKey,
) {
    if let Some(channel) = vector_channel {
        update_vector_curve(
            session,
            module,
            parameter,
            channel,
            &format!("Changed {parameter} {channel} curve key"),
            |curve| {
                let Some(previous) = curve.keys.get_mut(index) else {
                    return false;
                };
                *previous = key;
                true
            },
        );
    } else {
        session.set_curve_key(module, parameter, index, key);
    }
}

fn insert_vector_curve_key(
    session: &mut EditorSession,
    module: ModuleId,
    parameter: &str,
    channel: u8,
    index: usize,
    key: CurveKey,
) {
    update_vector_curve(
        session,
        module,
        parameter,
        channel,
        &format!("Added {parameter} {channel} curve key"),
        |curve| {
            if index > curve.keys.len() {
                return false;
            }
            curve.keys.insert(index, key);
            true
        },
    );
}

fn remove_vector_curve_key(
    session: &mut EditorSession,
    module: ModuleId,
    parameter: &str,
    channel: u8,
    index: usize,
) {
    update_vector_curve(
        session,
        module,
        parameter,
        channel,
        &format!("Removed {parameter} {channel} curve key"),
        |curve| {
            if index >= curve.keys.len() {
                return false;
            }
            curve.keys.remove(index);
            true
        },
    );
}

fn curve_graph_data(curve: &aestra_bevy::Curve) -> AutomationCurveData {
    let output_range = curve.output_range();
    let value_bounds = if curve.output_range.is_some() {
        Some((0.0, 1.0))
    } else {
        Some((output_range.min, output_range.max))
    };
    AutomationCurveData::Curve {
        points: curve
            .keys
            .iter()
            .map(|key| AutomationCurvePoint {
                time: key.time,
                value: key.value,
            })
            .collect(),
        value_bounds,
    }
}

fn formatted_curve_output_value(value: f32) -> String {
    crate::feathers::number_input::formatted(value, 3)
}

fn curve_key_output_value(curve: &aestra_bevy::Curve, key: CurveKey) -> f32 {
    curve.output_value(key.value)
}

fn gradient_graph_data(gradient: &aestra_bevy::Gradient) -> AutomationCurveData {
    AutomationCurveData::Gradient(
        gradient
            .keys
            .iter()
            .map(|key| AutomationGradientPoint {
                time: key.time,
                color: key.color,
            })
            .collect(),
    )
}

fn curve_drag_preview(
    curve: &aestra_bevy::Curve,
    key_index: usize,
    distance: Vec2,
    graph_size: Vec2,
    min: f32,
    max: f32,
) -> Option<(CurveKey, AutomationCurveData)> {
    let graph_data = curve_graph_data(curve);
    let mut key = curve.keys.get(key_index).copied()?;
    let previous = key_index
        .checked_sub(1)
        .and_then(|index| curve.keys.get(index))
        .map_or(0.0, |key| key.time + 0.001);
    let next = curve
        .keys
        .get(key_index + 1)
        .map_or(1.0, |key| key.time - 0.001);
    key.time = (key.time + distance.x / graph_size.x.max(1.0)).clamp(previous, next);
    let original_top = graph_data.key_top_percent(key_index);
    let target_top = original_top + distance.y / graph_size.y.max(1.0) * 100.0;
    key.value = graph_data
        .value_for_top_percent(target_top)
        .unwrap_or(key.value)
        .clamp(min, max);
    let mut preview = graph_data;
    let AutomationCurveData::Curve { points, .. } = &mut preview else {
        return None;
    };
    let point = points.get_mut(key_index)?;
    point.time = key.time;
    point.value = key.value;
    Some((key, preview))
}

fn gradient_drag_preview(
    gradient: &aestra_bevy::Gradient,
    key_index: usize,
    distance_x: f32,
    graph_width: f32,
) -> Option<(ColorKey, AutomationCurveData)> {
    let mut key = gradient.keys.get(key_index).copied()?;
    let previous = key_index
        .checked_sub(1)
        .and_then(|index| gradient.keys.get(index))
        .map_or(0.0, |key| key.time + 0.001);
    let next = gradient
        .keys
        .get(key_index + 1)
        .map_or(1.0, |key| key.time - 0.001);
    key.time = (key.time + distance_x / graph_width.max(1.0)).clamp(previous, next);
    let mut preview = gradient_graph_data(gradient);
    let AutomationCurveData::Gradient(points) = &mut preview else {
        return None;
    };
    points.get_mut(key_index)?.time = key.time;
    Some((key, preview))
}

fn spawn_curve_ordinate(parent: &mut ChildSpawnerCommands, minimum: f32, maximum: f32) {
    parent
        .spawn(Node {
            width: Val::Px(48.0),
            height: Val::Percent(100.0),
            flex_shrink: 0.0,
            position_type: PositionType::Relative,
            border: UiRect::right(Val::Px(1.0)),
            ..default()
        })
        .insert(BorderColor::all(theme::BORDER))
        .with_children(|axis| {
            for (value, top, bottom) in [
                (maximum, Val::Percent(3.0), Val::Auto),
                (minimum, Val::Auto, Val::Percent(3.0)),
            ] {
                axis.spawn((
                    Text::new(formatted_curve_output_value(value)),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                    TextLayout::justify(Justify::Right),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(3.0),
                        right: Val::Px(6.0),
                        top,
                        bottom,
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
            }
        });
}

fn curve_drag_label_margin(time: f32, top: f32) -> UiRect {
    UiRect {
        left: Val::Px(if time > 0.82 { -62.0 } else { 9.0 }),
        top: Val::Px(if top < 18.0 { 8.0 } else { -24.0 }),
        ..default()
    }
}

fn spawn_curve_drag_value_label(
    graph: &mut ChildSpawnerCommands,
    selection: ComplexSelection,
    key: CurveKey,
    top: f32,
    curve: &aestra_bevy::Curve,
) {
    graph.spawn((
        CurveGraphValueLabel(selection),
        Text::new(formatted_curve_output_value(curve_key_output_value(
            curve, key,
        ))),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(theme::TEXT),
        TextLayout::no_wrap(),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(key.time * 100.0),
            top: Val::Percent(top),
            min_width: Val::Px(50.0),
            height: Val::Px(18.0),
            padding: UiRect::horizontal(Val::Px(5.0)),
            margin: curve_drag_label_margin(key.time, top),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(theme::PANEL),
        BorderColor::all(theme::BORDER_BRIGHT),
        Visibility::Hidden,
        ZIndex(3),
        Pickable::IGNORE,
    ));
}

fn spawn_curve_graph(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input_index: u8,
    input: &InputMetadata,
    curve: &aestra_bevy::Curve,
    selected_key: usize,
    vector_channel: Option<u8>,
    localizer: &Localizer,
) {
    let Some((step, min, max)) = curve_value_bounds(&input.control, curve) else {
        return;
    };
    let display_name = localized_properties_input(localizer, input.name, input.display_name, false);
    let description = localized_properties_input(localizer, input.name, input.description, true);
    parent.spawn((
        Text::new(format!("{display_name}  ·  {description}")),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(theme::TEXT_MUTED),
    ));
    let graph_data = curve_graph_data(curve);
    let output_range = curve.output_range();
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(112.0),
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::TIMELINE_BG),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|container| {
            spawn_curve_ordinate(container, output_range.min, output_range.max);
            container
                .spawn((
                    CurveGraph,
                    RelativeCursorPosition::default(),
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        height: Val::Percent(100.0),
                        position_type: PositionType::Relative,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                ))
                .with_children(|graph| {
                    automation_curve::spawn_automation_curve(graph, &graph_data);
                    for (key_index, key) in curve.keys.iter().enumerate() {
                        let top = graph_data.key_top_percent(key_index);
                        let parameter = input.name;
                        graph
                    .spawn((
                        Button,
                        EditorNativeControl,
                        CurveGraphKey(ComplexSelection {
                            module,
                            input: input_index,
                            key: key_index,
                        }),
                        EntityCursor::System(SystemCursorIcon::Grab),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(key.time * 100.0),
                            top: Val::Percent(top),
                            width: Val::Px(11.0),
                            height: Val::Px(11.0),
                            margin: UiRect {
                                left: Val::Px(-5.5),
                                top: Val::Px(-5.5),
                                ..default()
                            },
                            border: UiRect::all(Val::Px(if key_index == selected_key {
                                2.0
                            } else {
                                1.0
                            })),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme::ACCENT),
                        BorderColor::all(if key_index == selected_key {
                            Color::WHITE
                        } else {
                            theme::ACCENT_DIM
                        }),
                    ))
                    .observe(
                        move |click: On<Pointer<Click>>,
                              mut session: ResMut<EditorSession>,
                              mut workspace: ResMut<CurvesState>| {
                            if click.button == PointerButton::Primary {
                                workspace.complex = Some(ComplexSelection {
                                    module,
                                    input: input_index,
                                    key: key_index,
                                });
                                session.ui_revision += 1;
                            }
                        },
                    )
                    .observe(
                        move |drag: On<Pointer<Drag>>,
                              graph: Single<(&ComputedNode, &Children), With<CurveGraph>>,
                              session: Res<EditorSession>,
                              mut key_nodes: Query<
                                  (&CurveGraphKey, &mut Node),
                                  Without<CurveGraphValueLabel>,
                              >,
                              mut value_labels: Query<
                                  (
                                      &CurveGraphValueLabel,
                                      &mut Node,
                                      &mut Text,
                                      &mut Visibility,
                                  ),
                                  Without<CurveGraphKey>,
                              >,
                              mut rasters: Query<
                            &mut automation_curve::AutomationCurveRaster,
                        >| {
                            if drag.button != PointerButton::Primary {
                                return;
                            }
                            let (computed, children) = *graph;
                            let graph_size = computed.size() * computed.inverse_scale_factor;
                            let Some(curve) = editable_curve(
                                &session,
                                module,
                                parameter,
                                vector_channel,
                            )
                            else {
                                return;
                            };
                            let Some((key, preview)) = curve_drag_preview(
                                &curve,
                                key_index,
                                drag.distance,
                                graph_size,
                                min,
                                max,
                            ) else {
                                return;
                            };
                            let AutomationCurveData::Curve { points, .. } = &preview else {
                                return;
                            };
                            for (selection, mut node) in &mut key_nodes {
                                if selection.0.module != module
                                    || selection.0.input != input_index
                                {
                                    continue;
                                }
                                let Some(point) = points.get(selection.0.key) else {
                                    continue;
                                };
                                node.left = Val::Percent(point.time * 100.0);
                                node.top = Val::Percent(
                                    preview.key_top_percent(selection.0.key),
                                );
                            }
                            let top = preview.key_top_percent(key_index);
                            for (selection, mut node, mut text, mut visibility) in
                                &mut value_labels
                            {
                                if selection.0
                                    != (ComplexSelection {
                                        module,
                                        input: input_index,
                                        key: key_index,
                                    })
                                {
                                    continue;
                                }
                                node.left = Val::Percent(key.time * 100.0);
                                node.top = Val::Percent(top);
                                node.margin = curve_drag_label_margin(key.time, top);
                                text.0 = formatted_curve_output_value(curve_key_output_value(
                                    &curve, key,
                                ));
                                *visibility = Visibility::Visible;
                            }
                            for child in children.iter() {
                                if let Ok(mut raster) = rasters.get_mut(child)
                                    && raster.data() != &preview
                                {
                                    raster.set_data(preview.clone());
                                }
                            }
                        },
                    )
                    .observe(
                        move |drag: On<Pointer<DragEnd>>,
                              graph: Single<&ComputedNode, With<CurveGraph>>,
                              mut session: ResMut<EditorSession>,
                              mut workspace: ResMut<CurvesState>,
                              mut value_labels: Query<
                                  (&CurveGraphValueLabel, &mut Visibility),
                                  Without<CurveGraphKey>,
                              >| {
                            if drag.button != PointerButton::Primary {
                                return;
                            }
                            for (selection, mut visibility) in &mut value_labels {
                                if selection.0
                                    == (ComplexSelection {
                                        module,
                                        input: input_index,
                                        key: key_index,
                                    })
                                {
                                    *visibility = Visibility::Hidden;
                                }
                            }
                            let graph_size = graph.size() * graph.inverse_scale_factor;
                            let Some(curve) = editable_curve(
                                &session,
                                module,
                                parameter,
                                vector_channel,
                            )
                            else {
                                return;
                            };
                            let Some((key, _)) = curve_drag_preview(
                                &curve,
                                key_index,
                                drag.distance,
                                graph_size,
                                min,
                                max,
                            ) else {
                                return;
                            };
                            set_editable_curve_key(
                                &mut session,
                                module,
                                parameter,
                                vector_channel,
                                key_index,
                                key,
                            );
                            workspace.complex = Some(ComplexSelection {
                                module,
                                input: input_index,
                                key: key_index,
                            });
                        },
                    );
                        spawn_curve_drag_value_label(
                            graph,
                            ComplexSelection {
                                module,
                                input: input_index,
                                key: key_index,
                            },
                            *key,
                            top,
                            curve,
                        );
                    }
                });
        });
    spawn_complex_controls(
        parent,
        curve.keys.get(selected_key).copied(),
        None,
        step,
        localizer,
    );
}

fn spawn_gradient_graph(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input_index: u8,
    input: &InputMetadata,
    gradient: &aestra_bevy::Gradient,
    selected_key: usize,
    localizer: &Localizer,
) {
    let display_name = localized_properties_input(localizer, input.name, input.display_name, false);
    let description = localized_properties_input(localizer, input.name, input.description, true);
    parent.spawn((
        Text::new(format!("{display_name}  ·  {description}")),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(theme::TEXT_MUTED),
    ));
    let graph_data = gradient_graph_data(gradient);
    parent
        .spawn((
            CurveGraph,
            RelativeCursorPosition::default(),
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(82.0),
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::TIMELINE_BG),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|graph| {
            automation_curve::spawn_automation_curve(graph, &graph_data);
            for (key_index, key) in gradient.keys.iter().enumerate() {
                let parameter = input.name;
                graph
                    .spawn((
                        Button,
                        EditorNativeControl,
                        EntityCursor::System(SystemCursorIcon::EwResize),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(key.time * 100.0),
                            top: Val::Px(0.0),
                            width: Val::Px(11.0),
                            height: Val::Percent(100.0),
                            margin: UiRect::left(Val::Px(-5.5)),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ))
                    .with_child((
                        Node {
                            width: Val::Px(if key_index == selected_key { 3.0 } else { 2.0 }),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(if key_index == selected_key {
                            theme::TEXT
                        } else {
                            theme::TEXT_MUTED
                        }),
                        Pickable::IGNORE,
                    ))
                    .observe(
                        move |click: On<Pointer<Click>>,
                              mut session: ResMut<EditorSession>,
                              mut workspace: ResMut<CurvesState>| {
                            if click.button == PointerButton::Primary {
                                workspace.complex = Some(ComplexSelection {
                                    module,
                                    input: input_index,
                                    key: key_index,
                                });
                                session.ui_revision += 1;
                            }
                        },
                    )
                    .observe(
                        move |drag: On<Pointer<Drag>>,
                              graph: Single<(&ComputedNode, &Children), With<CurveGraph>>,
                              session: Res<EditorSession>,
                              mut nodes: Query<&mut Node>,
                              mut rasters: Query<
                            &mut automation_curve::AutomationCurveRaster,
                        >| {
                            if drag.button != PointerButton::Primary {
                                return;
                            }
                            let (computed, children) = *graph;
                            let width = computed.size().x * computed.inverse_scale_factor;
                            let Some(Value::Gradient(gradient)) = session
                                .selected_layer()
                                .modules
                                .iter()
                                .find(|item| item.id == module)
                                .and_then(|item| curve_module_parameter(&session, item, parameter))
                            else {
                                return;
                            };
                            let Some((key, preview)) = gradient_drag_preview(
                                &gradient,
                                key_index,
                                drag.distance.x,
                                width,
                            ) else {
                                return;
                            };
                            if let Ok(mut node) = nodes.get_mut(drag.entity) {
                                node.left = Val::Percent(key.time * 100.0);
                            }
                            for child in children.iter() {
                                if let Ok(mut raster) = rasters.get_mut(child)
                                    && raster.data() != &preview
                                {
                                    raster.set_data(preview.clone());
                                }
                            }
                        },
                    )
                    .observe(
                        move |drag: On<Pointer<DragEnd>>,
                              graph: Single<&ComputedNode, With<CurveGraph>>,
                              mut session: ResMut<EditorSession>,
                              mut workspace: ResMut<CurvesState>| {
                            if drag.button != PointerButton::Primary {
                                return;
                            }
                            let width = graph.size().x * graph.inverse_scale_factor;
                            let Some(Value::Gradient(gradient)) = session
                                .selected_layer()
                                .modules
                                .iter()
                                .find(|item| item.id == module)
                                .and_then(|item| curve_module_parameter(&session, item, parameter))
                            else {
                                return;
                            };
                            let Some((key, _)) = gradient_drag_preview(
                                &gradient,
                                key_index,
                                drag.distance.x,
                                width,
                            ) else {
                                return;
                            };
                            session.set_gradient_key(module, parameter, key_index, key);
                            workspace.complex = Some(ComplexSelection {
                                module,
                                input: input_index,
                                key: key_index,
                            });
                        },
                    );
            }
        });
    spawn_complex_controls(
        parent,
        None,
        gradient.keys.get(selected_key).copied(),
        0.05,
        localizer,
    );
}

fn spawn_complex_controls(
    parent: &mut ChildSpawnerCommands,
    curve_key: Option<CurveKey>,
    color_key: Option<ColorKey>,
    value_step: f32,
    localizer: &Localizer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Px(34.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(5.0),
            ..default()
        })
        .with_children(|controls| {
            stack_button(
                controls,
                &localizer.text("curves-add-key"),
                CurvesAction::AddKey,
                56.0,
            );
            stack_button(
                controls,
                &localizer.text("curves-delete-key"),
                CurvesAction::DeleteKey,
                56.0,
            );
            let time = curve_key
                .map(|key| key.time)
                .or_else(|| color_key.map(|key| key.time));
            if let Some(time) = time {
                controls.spawn((
                    Text::new(format!("{} {time:.3}", localizer.text("curves-time"))),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                ));
                mini_button(controls, "−", CurvesAction::AdjustTime(-1));
                mini_button(controls, "+", CurvesAction::AdjustTime(1));
            }
            if let Some(key) = curve_key {
                controls.spawn((
                    Text::new(format!(
                        "{} {:.3}  ·  {} {value_step:.2}",
                        localizer.text("curves-value"),
                        key.value,
                        localizer.text("curves-step")
                    )),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                ));
                mini_button(controls, "−", CurvesAction::AdjustCurveValue(-1));
                mini_button(controls, "+", CurvesAction::AdjustCurveValue(1));
            }
            if let Some(key) = color_key {
                for (channel, label) in ["R", "G", "B", "A"].into_iter().enumerate() {
                    controls.spawn((
                        Text::new(format!("{label}{:.2}", key.color[channel])),
                        TextFont {
                            font_size: FontSize::Px(8.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                    ));
                    mini_button(
                        controls,
                        "−",
                        CurvesAction::AdjustGradientChannel(channel as u8, -1),
                    );
                    mini_button(
                        controls,
                        "+",
                        CurvesAction::AdjustGradientChannel(channel as u8, 1),
                    );
                }
            }
        });
}

fn add_complex_key_at_pointer(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    workspace: &mut CurvesState,
    cursor: Vec2,
) {
    let Some(selection) = workspace.complex else {
        session.status = "Select a curve or gradient first".into();
        return;
    };
    let Some(module) = session
        .selected_layer()
        .modules
        .iter()
        .find(|module| module.id == selection.module)
    else {
        session.status = "The selected module no longer exists".into();
        return;
    };
    let Some(input) = registry
        .get(&module.module_type)
        .and_then(|metadata| metadata.inputs.get(selection.input as usize))
    else {
        session.status = "The selected input metadata no longer exists".into();
        return;
    };
    let parameter = input.name;
    let control = input.control;
    let Some(value) = curve_module_parameter(session, module, parameter) else {
        session.status = "The selected authored value no longer exists".into();
        return;
    };
    let time = (cursor.x + 0.5).clamp(0.0, 1.0);
    let top = (cursor.y + 0.5).clamp(0.0, 1.0) * 100.0;

    match value {
        Value::Curve(curve) => {
            if let Some(index) = curve
                .keys
                .iter()
                .position(|key| (key.time - time).abs() <= 0.0005)
            {
                workspace.complex = Some(ComplexSelection {
                    key: index,
                    ..selection
                });
                session.ui_revision += 1;
                return;
            }
            let index = curve
                .keys
                .iter()
                .position(|key| key.time > time)
                .unwrap_or(curve.keys.len());
            let Some((_, min, max)) = curve_value_bounds(&control, &curve) else {
                return;
            };
            let value = curve_graph_data(&curve)
                .value_for_top_percent(top)
                .unwrap_or_else(|| curve.sample(time))
                .clamp(min, max);
            session.add_curve_key(
                selection.module,
                parameter,
                index,
                CurveKey::new(time, value),
            );
            workspace.complex = Some(ComplexSelection {
                key: index,
                ..selection
            });
        }
        Value::Vec3Curve(curves) => {
            let channel = workspace.vector_channel.min(2);
            let curve = &curves.curves[channel as usize];
            if let Some(index) = curve
                .keys
                .iter()
                .position(|key| (key.time - time).abs() <= 0.0005)
            {
                workspace.complex = Some(ComplexSelection {
                    key: index,
                    ..selection
                });
                session.ui_revision += 1;
                return;
            }
            let index = curve
                .keys
                .iter()
                .position(|key| key.time > time)
                .unwrap_or(curve.keys.len());
            let Some((_, min, max)) = curve_value_bounds(&control, curve) else {
                return;
            };
            let value = curve_graph_data(curve)
                .value_for_top_percent(top)
                .unwrap_or_else(|| curve.sample(time))
                .clamp(min, max);
            insert_vector_curve_key(
                session,
                selection.module,
                parameter,
                channel,
                index,
                CurveKey::new(time, value),
            );
            workspace.complex = Some(ComplexSelection {
                key: index,
                ..selection
            });
        }
        Value::Gradient(gradient) => {
            if let Some(index) = gradient
                .keys
                .iter()
                .position(|key| (key.time - time).abs() <= 0.0005)
            {
                workspace.complex = Some(ComplexSelection {
                    key: index,
                    ..selection
                });
                session.ui_revision += 1;
                return;
            }
            let index = gradient
                .keys
                .iter()
                .position(|key| key.time > time)
                .unwrap_or(gradient.keys.len());
            session.add_gradient_key(
                selection.module,
                parameter,
                index,
                ColorKey::new(time, gradient.sample(time)),
            );
            workspace.complex = Some(ComplexSelection {
                key: index,
                ..selection
            });
        }
        _ => session.status = "This property does not contain editable keys".into(),
    }
}

#[derive(Clone, Copy)]
enum ComplexKeyEdit {
    Add,
    Delete,
    Time(i8),
    CurveValue(i8),
    GradientChannel(u8, i8),
}

fn edit_complex_key(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    workspace: &mut CurvesState,
    edit: ComplexKeyEdit,
) {
    let Some(selection) = workspace.complex else {
        session.status = "Select a curve or gradient first".into();
        return;
    };
    let Some(module) = session
        .selected_layer()
        .modules
        .iter()
        .find(|module| module.id == selection.module)
    else {
        session.status = "The selected module no longer exists".into();
        return;
    };
    let Some(input) = registry
        .get(&module.module_type)
        .and_then(|metadata| metadata.inputs.get(selection.input as usize))
    else {
        session.status = "The selected input metadata no longer exists".into();
        return;
    };
    let parameter = input.name;
    let control = input.control;
    let Some(value) = curve_module_parameter(session, module, parameter) else {
        session.status = "The selected authored value no longer exists".into();
        return;
    };
    let vector_channel = workspace.vector_channel.min(2);

    match (value, edit) {
        (Value::Curve(curve), ComplexKeyEdit::Add) => {
            let (index, time) = insertion_time(
                &curve.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
            );
            let value = curve_stored_sample(&curve, time);
            session.add_curve_key(
                selection.module,
                parameter,
                index,
                CurveKey::new(time, value),
            );
            workspace.complex = Some(ComplexSelection {
                key: index,
                ..selection
            });
        }
        (Value::Vec3Curve(curves), ComplexKeyEdit::Add) => {
            let curve = &curves.curves[vector_channel as usize];
            let (index, time) = insertion_time(
                &curve.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
            );
            insert_vector_curve_key(
                session,
                selection.module,
                parameter,
                vector_channel,
                index,
                CurveKey::new(time, curve_stored_sample(curve, time)),
            );
            workspace.complex = Some(ComplexSelection {
                key: index,
                ..selection
            });
        }
        (Value::Gradient(gradient), ComplexKeyEdit::Add) => {
            let (index, time) = insertion_time(
                &gradient.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
            );
            let color = gradient.sample(time);
            session.add_gradient_key(
                selection.module,
                parameter,
                index,
                ColorKey::new(time, color),
            );
            workspace.complex = Some(ComplexSelection {
                key: index,
                ..selection
            });
        }
        (Value::Curve(curve), ComplexKeyEdit::Delete) => {
            if curve.keys.len() <= 2 {
                session.status = "A curve must keep at least two keys".into();
                return;
            }
            let index = selection.key.min(curve.keys.len() - 1);
            session.remove_curve_key(selection.module, parameter, index);
            workspace.complex = Some(ComplexSelection {
                key: index.min(curve.keys.len() - 2),
                ..selection
            });
        }
        (Value::Vec3Curve(curves), ComplexKeyEdit::Delete) => {
            let curve = &curves.curves[vector_channel as usize];
            if curve.keys.len() <= 2 {
                session.status = "A curve must keep at least two keys".into();
                return;
            }
            let index = selection.key.min(curve.keys.len() - 1);
            remove_vector_curve_key(session, selection.module, parameter, vector_channel, index);
            workspace.complex = Some(ComplexSelection {
                key: index.min(curve.keys.len() - 2),
                ..selection
            });
        }
        (Value::Gradient(gradient), ComplexKeyEdit::Delete) => {
            if gradient.keys.len() <= 2 {
                session.status = "A gradient must keep at least two keys".into();
                return;
            }
            let index = selection.key.min(gradient.keys.len() - 1);
            session.remove_gradient_key(selection.module, parameter, index);
            workspace.complex = Some(ComplexSelection {
                key: index.min(gradient.keys.len() - 2),
                ..selection
            });
        }
        (Value::Curve(curve), ComplexKeyEdit::Time(direction)) => {
            let Some(mut key) = curve.keys.get(selection.key).copied() else {
                return;
            };
            key.time = bounded_key_time(
                &curve.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
                key.time + direction as f32 * 0.01,
            );
            session.set_curve_key(selection.module, parameter, selection.key, key);
        }
        (Value::Vec3Curve(curves), ComplexKeyEdit::Time(direction)) => {
            let curve = &curves.curves[vector_channel as usize];
            let Some(mut key) = curve.keys.get(selection.key).copied() else {
                return;
            };
            key.time = bounded_key_time(
                &curve.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
                key.time + direction as f32 * 0.01,
            );
            set_editable_curve_key(
                session,
                selection.module,
                parameter,
                Some(vector_channel),
                selection.key,
                key,
            );
        }
        (Value::Gradient(gradient), ComplexKeyEdit::Time(direction)) => {
            let Some(mut key) = gradient.keys.get(selection.key).copied() else {
                return;
            };
            key.time = bounded_key_time(
                &gradient.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
                key.time + direction as f32 * 0.01,
            );
            session.set_gradient_key(selection.module, parameter, selection.key, key);
        }
        (Value::Curve(curve), ComplexKeyEdit::CurveValue(direction)) => {
            let Some(mut key) = curve.keys.get(selection.key).copied() else {
                return;
            };
            let Some((step, min, max)) = curve_value_bounds(&control, &curve) else {
                return;
            };
            key.value = (key.value + direction as f32 * step).clamp(min, max);
            session.set_curve_key(selection.module, parameter, selection.key, key);
        }
        (Value::Vec3Curve(curves), ComplexKeyEdit::CurveValue(direction)) => {
            let curve = &curves.curves[vector_channel as usize];
            let Some(mut key) = curve.keys.get(selection.key).copied() else {
                return;
            };
            let Some((step, min, max)) = curve_value_bounds(&control, curve) else {
                return;
            };
            key.value = (key.value + direction as f32 * step).clamp(min, max);
            set_editable_curve_key(
                session,
                selection.module,
                parameter,
                Some(vector_channel),
                selection.key,
                key,
            );
        }
        (Value::Gradient(gradient), ComplexKeyEdit::GradientChannel(channel, direction)) => {
            let Some(mut key) = gradient.keys.get(selection.key).copied() else {
                return;
            };
            let Some(value) = key.color.get_mut(channel as usize) else {
                return;
            };
            *value = (*value + direction as f32 * 0.05).clamp(0.0, 1.0);
            session.set_gradient_key(selection.module, parameter, selection.key, key);
        }
        _ => session.status = "This edit does not apply to the selected property".into(),
    }
}

fn curve_value_bounds(
    control: &InputControl,
    curve: &aestra_bevy::Curve,
) -> Option<(f32, f32, f32)> {
    if curve.output_range.is_some() {
        return Some((0.01, 0.0, 1.0));
    }
    match control {
        InputControl::Curve { step, min, max } => Some((*step, *min, *max)),
        InputControl::Number { step, min, max } | InputControl::Vector { step, min, max } => {
            let authored_min = curve
                .keys
                .iter()
                .map(|key| key.value)
                .fold(f32::INFINITY, f32::min);
            let authored_max = curve
                .keys
                .iter()
                .map(|key| key.value)
                .fold(f32::NEG_INFINITY, f32::max);
            let minimum = min.unwrap_or(if authored_min.is_finite() {
                authored_min
            } else {
                0.0
            });
            let maximum = max.unwrap_or_else(|| {
                if authored_max.is_finite() {
                    authored_max.max(minimum + *step)
                } else {
                    minimum + *step
                }
            });
            Some((*step, minimum, maximum.max(minimum + f32::EPSILON)))
        }
        _ => None,
    }
}

fn curve_stored_sample(curve: &aestra_bevy::Curve, time: f32) -> f32 {
    let sampled = curve.sample(time);
    let Some(range) = curve.output_range else {
        return sampled;
    };
    let span = range.max - range.min;
    if span.abs() <= f32::EPSILON {
        curve.keys.first().map_or(0.0, |key| key.value)
    } else {
        ((sampled - range.min) / span).clamp(0.0, 1.0)
    }
}

fn insertion_time(times: &[f32], selected: usize) -> (usize, f32) {
    if times.is_empty() {
        return (0, 0.5);
    }
    let selected = selected.min(times.len() - 1);
    if let Some(next) = times.get(selected + 1) {
        (selected + 1, (times[selected] + next) * 0.5)
    } else if selected > 0 {
        (selected, (times[selected - 1] + times[selected]) * 0.5)
    } else {
        (1, (times[0] + 1.0) * 0.5)
    }
}

fn bounded_key_time(times: &[f32], index: usize, value: f32) -> f32 {
    let previous = index
        .checked_sub(1)
        .and_then(|index| times.get(index))
        .map_or(0.0, |time| time + 0.001);
    let next = times.get(index + 1).map_or(1.0, |time| time - 0.001);
    value.clamp(previous, next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    fn first_curve_selection(
        session: &EditorSession,
        registry: &EditorModuleRegistry,
    ) -> ComplexSelection {
        for module in &session.selected_layer().modules {
            let Some(metadata) = registry.0.get(&module.module_type) else {
                continue;
            };
            for (input, metadata) in metadata.inputs.iter().enumerate() {
                if matches!(
                    module_parameter(module, metadata.name),
                    Some(Value::Curve(_))
                ) && complex_input_is_visible(module, metadata)
                {
                    return ComplexSelection {
                        module: module.id,
                        input: input as u8,
                        key: 0,
                    };
                }
            }
        }
        panic!("test fixture should expose a curve input");
    }

    fn curve_key_count(
        session: &EditorSession,
        registry: &EditorModuleRegistry,
        selection: ComplexSelection,
    ) -> usize {
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module.id == selection.module)
            .unwrap();
        let input = &registry.0.get(&module.module_type).unwrap().inputs[selection.input as usize];
        let Some(Value::Curve(curve)) = module_parameter(module, input.name) else {
            panic!("selection should resolve to a curve");
        };
        curve.keys.len()
    }

    #[test]
    fn curves_plugin_owns_feathers_activation_and_selection() {
        let session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let selection = first_curve_selection(&session, &registry);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(registry)
            .init_resource::<WorkspaceLayout>()
            .add_plugins(EditorCurvesPlugin);
        let control = app
            .world_mut()
            .spawn((
                Button,
                FeathersActionButton,
                Interaction::None,
                CurvesAction::OpenInput(selection.module, selection.input),
                BackgroundColor::default(),
            ))
            .id();

        app.world_mut().trigger(Activate { entity: control });
        app.update();

        assert!(app.world().resource::<CurvesState>().has_selection());
        assert!(
            !app.world()
                .entity(control)
                .contains::<PendingFeathersActivation>()
        );
    }

    #[test]
    fn adding_a_curve_key_is_one_undoable_semantic_edit() {
        let session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let selection = first_curve_selection(&session, &registry);
        let initial = curve_key_count(&session, &registry, selection);
        let mut state = CurvesState::default();
        state.select_for_test(selection.module, selection.input, selection.key);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(registry)
            .insert_resource(state)
            .init_resource::<WorkspaceLayout>()
            .add_plugins(EditorCurvesPlugin);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            CurvesAction::AddKey,
            BackgroundColor::default(),
        ));

        app.update();

        let registry = app.world().resource::<EditorModuleRegistry>();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(curve_key_count(session, registry, selection), initial + 1);
        app.world_mut().resource_mut::<EditorSession>().undo();
        let registry = app.world().resource::<EditorModuleRegistry>();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(curve_key_count(session, registry, selection), initial);
    }

    #[test]
    fn insert_and_delete_shortcuts_edit_the_curve_under_the_pointer() {
        let session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let selection = first_curve_selection(&session, &registry);
        let initial = curve_key_count(&session, &registry, selection);
        let mut state = CurvesState::default();
        state.select_for_test(selection.module, selection.input, selection.key);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(registry)
            .insert_resource(state)
            .init_resource::<WorkspaceLayout>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(EditorCurvesPlugin);
        app.world_mut().spawn((
            CurveGraph,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::new(0.17, -0.1)),
            },
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Insert);

        app.update();

        let inserted_selection = app
            .world()
            .resource::<CurvesState>()
            .selected_key()
            .unwrap();
        let registry = app.world().resource::<EditorModuleRegistry>();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(curve_key_count(session, registry, selection), initial + 1);
        let (_, _, Value::Curve(curve)) =
            resolve_complex_input(session, registry, inserted_selection).unwrap()
        else {
            panic!("expected curve input");
        };
        assert!((curve.keys[inserted_selection.key].time - 0.67).abs() < 0.0001);

        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.release(KeyCode::Insert);
            keys.press(KeyCode::Delete);
        }
        app.update();

        let registry = app.world().resource::<EditorModuleRegistry>();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(curve_key_count(session, registry, selection), initial);
        app.world_mut().resource_mut::<EditorSession>().undo();
        let registry = app.world().resource::<EditorModuleRegistry>();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(curve_key_count(session, registry, selection), initial + 1);
    }

    #[test]
    fn curves_workspace_projects_keys_into_shared_feather_data() {
        let session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let selection = first_curve_selection(&session, &registry);
        let (_, _, Value::Curve(curve)) =
            resolve_complex_input(&session, &registry, selection).unwrap()
        else {
            panic!("expected curve input");
        };

        let AutomationCurveData::Curve { points, .. } = curve_graph_data(&curve) else {
            panic!("expected Feather curve data");
        };
        assert_eq!(points.len(), curve.keys.len());
        assert_eq!(points[selection.key].time, curve.keys[selection.key].time);
        assert_eq!(points[selection.key].value, curve.keys[selection.key].value);
    }

    #[test]
    fn vector_curve_edits_only_the_selected_gravity_channel_and_is_undoable() {
        let mut session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module.parameter_value("gravity").is_some())
            .unwrap();
        let module_id = module.id;
        let input = registry
            .0
            .get(&module.module_type)
            .unwrap()
            .inputs
            .iter()
            .position(|input| input.name == "gravity")
            .unwrap() as u8;
        let source =
            aestra_bevy::PropertySource::Curve(aestra_bevy::PropertyEvaluationDomain::ParticleLife);
        let module = session
            .effect
            .emitters
            .iter_mut()
            .flat_map(|emitter| emitter.modules.iter_mut())
            .find(|module| module.id == module_id)
            .unwrap();
        module.property_sources.insert("gravity".into(), source);
        module.property_source_values.insert(
            "gravity".into(),
            vec![aestra_bevy::PropertySourceValue::new(
                source,
                Value::Vec3Curve(aestra_bevy::Vec3Curve::constant([1.0, 2.0, 3.0])),
            )],
        );
        let selection = ComplexSelection {
            module: module_id,
            input,
            key: 0,
        };
        let mut state = CurvesState {
            complex: Some(selection),
            vector_channel: 1,
        };

        edit_complex_key(&mut session, &registry.0, &mut state, ComplexKeyEdit::Add);

        let (_, _, Value::Vec3Curve(curves)) =
            resolve_complex_input(&session, &registry, selection).unwrap()
        else {
            panic!("gravity should remain an XYZ curve source");
        };
        assert_eq!(curves.curves[0].keys.len(), 2);
        assert_eq!(curves.curves[1].keys.len(), 3, "{}", session.status);
        assert_eq!(curves.curves[2].keys.len(), 2);
        session.undo();
        let (_, _, Value::Vec3Curve(curves)) =
            resolve_complex_input(&session, &registry, selection).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(curves.curves[1].keys.len(), 2);
    }

    #[test]
    fn normalized_curve_graph_uses_fixed_shape_bounds_and_real_output_labels() {
        let curve = aestra_bevy::Curve::normalized(
            vec![CurveKey::new(0.0, 0.25), CurveKey::new(1.0, 0.75)],
            aestra_bevy::ScalarRange::new(5.8, 32.0),
        );
        let AutomationCurveData::Curve { value_bounds, .. } = curve_graph_data(&curve) else {
            panic!("expected Feather curve data");
        };

        assert_eq!(value_bounds, Some((0.0, 1.0)));
        assert_eq!(
            formatted_curve_output_value(curve.output_range().min),
            "5.8"
        );
        assert_eq!(formatted_curve_output_value(curve.output_range().max), "32");
        assert_eq!(
            formatted_curve_output_value(curve_key_output_value(&curve, curve.keys[1])),
            "25.45"
        );
    }

    #[test]
    fn curves_workspace_visibility_uses_the_explicit_source_not_key_count() {
        let mut session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let selection = first_curve_selection(&session, &registry);
        let (_, input, Value::Curve(mut curve)) =
            resolve_complex_input(&session, &registry, selection).unwrap()
        else {
            panic!("expected curve input");
        };
        let parameter = input.name;
        curve.keys.truncate(1);
        session.set_module_parameter(selection.module, parameter, Value::Curve(curve));
        let module = session
            .effect
            .emitters
            .iter_mut()
            .flat_map(|emitter| emitter.modules.iter_mut())
            .find(|module| module.id == selection.module)
            .unwrap();
        module.property_sources.insert(
            parameter.into(),
            aestra_bevy::PropertySource::Curve(aestra_bevy::PropertyEvaluationDomain::ParticleLife),
        );
        let input = &registry.0.get(&module.module_type).unwrap().inputs[selection.input as usize];
        assert!(complex_input_is_visible(module, input));

        module
            .property_sources
            .insert(parameter.into(), aestra_bevy::PropertySource::Constant);

        let module = session
            .selected_layer()
            .modules
            .iter()
            .find(|module| module.id == selection.module)
            .unwrap();
        let input = &registry.0.get(&module.module_type).unwrap().inputs[selection.input as usize];

        assert!(!complex_input_is_visible(module, input));
    }

    #[test]
    fn curve_drag_preview_updates_the_shared_raster_data() {
        let session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let selection = first_curve_selection(&session, &registry);
        let (_, input, Value::Curve(curve)) =
            resolve_complex_input(&session, &registry, selection).unwrap()
        else {
            panic!("expected curve input");
        };
        let InputControl::Curve { min, max, .. } = input.control else {
            panic!("expected curve metadata");
        };

        let (key, preview) = curve_drag_preview(
            &curve,
            selection.key,
            Vec2::new(12.0, 8.0),
            Vec2::new(120.0, 100.0),
            min,
            max,
        )
        .unwrap();
        let AutomationCurveData::Curve { points, .. } = preview else {
            panic!("expected curve preview");
        };
        assert_eq!(points[selection.key].time, key.time);
        assert_eq!(points[selection.key].value, key.value);
        assert_ne!(key.time, curve.keys[selection.key].time);
    }

    #[test]
    fn key_time_helpers_preserve_ordering() {
        assert_eq!(insertion_time(&[0.0, 0.4, 1.0], 1), (2, 0.7));
        assert_eq!(insertion_time(&[0.0, 1.0], 1), (1, 0.5));
        assert_eq!(bounded_key_time(&[0.0, 0.4, 1.0], 1, 2.0), 0.999);
        assert_eq!(bounded_key_time(&[0.0, 0.4, 1.0], 1, -1.0), 0.001);
    }
}
