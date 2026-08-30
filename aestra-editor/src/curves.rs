//! Curves workspace, curve/gradient selection, and semantic key editing.

use crate::*;
use aestra_bevy::{ColorKey, CurveKey, ModuleId, ModuleInstance, Value};
use aestra_compiler::{InputControl, InputMetadata, ModuleRegistry};
use bevy::ui_widgets::Activate;

pub(crate) struct EditorCurvesPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CurvesSet {
    Actions,
}

impl Plugin for EditorCurvesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurvesState>()
            .add_observer(queue_curves_action_activation)
            .add_systems(Update, handle_curves_actions.in_set(CurvesSet::Actions));
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CurvesAction {
    OpenInput(ModuleId, u8),
    AddKey,
    DeleteKey,
    AdjustTime(i8),
    AdjustCurveValue(i8),
    AdjustGradientChannel(u8, i8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ComplexSelection {
    module: ModuleId,
    input: u8,
    key: usize,
}

#[derive(Resource, Default)]
pub(crate) struct CurvesState {
    complex: Option<ComplexSelection>,
}

impl CurvesState {
    pub(crate) fn clear(&mut self) {
        self.complex = None;
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
                            localizer,
                        ),
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
                            if !matches!(
                                input.control,
                                InputControl::Curve { .. } | InputControl::Gradient
                            ) {
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
    let value = module_parameter(module, input.name)?;
    Some((module, input, value))
}

fn spawn_curve_graph(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input_index: u8,
    input: &InputMetadata,
    curve: &aestra_bevy::Curve,
    selected_key: usize,
    localizer: &Localizer,
) {
    let InputControl::Curve { step, min, max } = input.control else {
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
    parent
        .spawn((
            CurveGraph,
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
        .with_children(|graph| {
            for index in 0..64 {
                let time = index as f32 / 63.0;
                let normalized = ((curve.sample(time) - min) / (max - min)).clamp(0.0, 1.0);
                graph.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(time * 100.0),
                        top: Val::Percent((1.0 - normalized) * 100.0),
                        width: Val::Px(2.0),
                        height: Val::Px(2.0),
                        ..default()
                    },
                    BackgroundColor(theme::ACCENT_DIM),
                ));
            }
            for (key_index, key) in curve.keys.iter().enumerate() {
                let normalized = ((key.value - min) / (max - min)).clamp(0.0, 1.0);
                let parameter = input.name;
                graph
                    .spawn((
                        Button,
                        EditorNativeControl,
                        UiTransform::default(),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(key.time * 100.0),
                            top: Val::Percent((1.0 - normalized) * 100.0),
                            width: Val::Px(11.0),
                            height: Val::Px(11.0),
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
                        |drag: On<Pointer<Drag>>, mut transforms: Query<&mut UiTransform>| {
                            if drag.button == PointerButton::Primary
                                && let Ok(mut transform) = transforms.get_mut(drag.entity)
                            {
                                transform.translation = Val2::px(drag.distance.x, drag.distance.y);
                            }
                        },
                    )
                    .observe(
                        move |drag: On<Pointer<DragEnd>>,
                              graph: Single<&ComputedNode, With<CurveGraph>>,
                              mut transforms: Query<&mut UiTransform>,
                              mut session: ResMut<EditorSession>,
                              mut workspace: ResMut<CurvesState>| {
                            if drag.button != PointerButton::Primary {
                                return;
                            }
                            if let Ok(mut transform) = transforms.get_mut(drag.entity) {
                                transform.translation = Val2::ZERO;
                            }
                            let graph_size = graph.size() * graph.inverse_scale_factor;
                            let Some(Value::Curve(curve)) = session
                                .selected_layer()
                                .modules
                                .iter()
                                .find(|item| item.id == module)
                                .and_then(|item| module_parameter(item, parameter))
                            else {
                                return;
                            };
                            let Some(mut key) = curve.keys.get(key_index).copied() else {
                                return;
                            };
                            let previous = key_index
                                .checked_sub(1)
                                .and_then(|index| curve.keys.get(index))
                                .map_or(0.0, |key| key.time + 0.001);
                            let next = curve
                                .keys
                                .get(key_index + 1)
                                .map_or(1.0, |key| key.time - 0.001);
                            key.time =
                                (key.time + drag.distance.x / graph_size.x).clamp(previous, next);
                            key.value = (key.value - drag.distance.y / graph_size.y * (max - min))
                                .clamp(min, max);
                            session.set_curve_key(module, parameter, key_index, key);
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
    parent
        .spawn((
            CurveGraph,
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
            for index in 0..64 {
                let time = index as f32 / 63.0;
                let color = gradient.sample(time);
                graph.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(time * 100.0),
                        top: Val::Px(0.0),
                        width: Val::Percent(100.0 / 64.0 + 0.1),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(color[0], color[1], color[2], color[3])),
                ));
            }
            for (key_index, key) in gradient.keys.iter().enumerate() {
                let parameter = input.name;
                graph
                    .spawn((
                        Button,
                        EditorNativeControl,
                        UiTransform::default(),
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Percent(key.time * 100.0),
                            bottom: Val::Px(4.0),
                            width: Val::Px(13.0),
                            height: Val::Px(20.0),
                            border: UiRect::all(Val::Px(if key_index == selected_key {
                                2.0
                            } else {
                                1.0
                            })),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(
                            key.color[0],
                            key.color[1],
                            key.color[2],
                            key.color[3],
                        )),
                        BorderColor::all(if key_index == selected_key {
                            Color::WHITE
                        } else {
                            theme::BORDER_BRIGHT
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
                        |drag: On<Pointer<Drag>>, mut transforms: Query<&mut UiTransform>| {
                            if drag.button == PointerButton::Primary
                                && let Ok(mut transform) = transforms.get_mut(drag.entity)
                            {
                                transform.translation = Val2::px(drag.distance.x, 0.0);
                            }
                        },
                    )
                    .observe(
                        move |drag: On<Pointer<DragEnd>>,
                              graph: Single<&ComputedNode, With<CurveGraph>>,
                              mut transforms: Query<&mut UiTransform>,
                              mut session: ResMut<EditorSession>| {
                            if drag.button != PointerButton::Primary {
                                return;
                            }
                            if let Ok(mut transform) = transforms.get_mut(drag.entity) {
                                transform.translation = Val2::ZERO;
                            }
                            let width = graph.size().x * graph.inverse_scale_factor;
                            let Some(Value::Gradient(gradient)) = session
                                .selected_layer()
                                .modules
                                .iter()
                                .find(|item| item.id == module)
                                .and_then(|item| module_parameter(item, parameter))
                            else {
                                return;
                            };
                            let Some(mut key) = gradient.keys.get(key_index).copied() else {
                                return;
                            };
                            let previous = key_index
                                .checked_sub(1)
                                .and_then(|index| gradient.keys.get(index))
                                .map_or(0.0, |key| key.time + 0.001);
                            let next = gradient
                                .keys
                                .get(key_index + 1)
                                .map_or(1.0, |key| key.time - 0.001);
                            key.time = (key.time + drag.distance.x / width).clamp(previous, next);
                            session.set_gradient_key(module, parameter, key_index, key);
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
    let Some(value) = module_parameter(module, parameter) else {
        session.status = "The selected authored value no longer exists".into();
        return;
    };

    match (value, edit) {
        (Value::Curve(curve), ComplexKeyEdit::Add) => {
            let (index, time) = insertion_time(
                &curve.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
            );
            let value = curve.sample(time);
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
            let InputControl::Curve { step, min, max } = control else {
                return;
            };
            key.value = (key.value + direction as f32 * step).clamp(min, max);
            session.set_curve_key(selection.module, parameter, selection.key, key);
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
                ) {
                    return ComplexSelection {
                        module: module.id,
                        input: input as u8,
                        key: 0,
                    };
                }
            }
        }
        panic!("embedded effect should expose a curve input");
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
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
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
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
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
    fn key_time_helpers_preserve_ordering() {
        assert_eq!(insertion_time(&[0.0, 0.4, 1.0], 1), (2, 0.7));
        assert_eq!(insertion_time(&[0.0, 1.0], 1), (1, 0.5));
        assert_eq!(bounded_key_time(&[0.0, 0.4, 1.0], 1, 2.0), 0.999);
        assert_eq!(bounded_key_time(&[0.0, 0.4, 1.0], 1, -1.0), 0.001);
    }
}
