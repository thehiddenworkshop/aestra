//! Production Curves workspace, property graphs and attached pointer observers.
use super::control_input::NativeButton as Button;
use super::cursor::EntityCursor;
use super::keyboard::CurveGraph;
use super::pointer::{PointerClick, PointerDrag, PointerDragEnd};
use super::property_editing::*;
use super::state::{ComplexSelection, CurvesState};
use super::{CurvesAction, host_composition as host_motion};
use crate::feathers::{
    automation_curve::{self, AutomationCurveData},
    scroll::spawn_vertical_scroll_area,
};
use crate::{
    EditorModuleRegistry, EditorNativeControl, EditorSession, Localizer, ScrollMemoryKey,
    localized_properties_input, mini_button, stack_button, theme,
};
use aestra_compiler::InputMetadata;
use aestra_core::{ColorKey, CurveKey, ModuleId, ModuleInstance, Value};
use bevy::{
    picking::pointer::PointerButton, prelude::*, ui::RelativeCursorPosition,
    window::SystemCursorIcon,
};

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CurveGraphKey(pub(super) ComplexSelection);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CurveGraphValueLabel(pub(super) ComplexSelection);

pub(crate) fn spawn_curves_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
    workspace: &CurvesState,
    localizer: &Localizer,
) {
    if host_motion::spawn(parent, session, workspace) {
        return;
    }
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

pub(super) fn spawn_complex_input_list(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
    workspace: &CurvesState,
    localizer: &Localizer,
) {
    let Some(selected_layer) = session.selected_layer() else {
        return;
    };
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
                    for module in &selected_layer.modules {
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

pub(super) fn complex_input_is_visible(module: &ModuleInstance, input: &InputMetadata) -> bool {
    matches!(
        module.property_source(input.name),
        Some(aestra_core::PropertySource::Curve(_))
            | Some(aestra_core::PropertySource::Gradient(_))
    )
}

pub(super) fn parent_list_button<A: Component>(
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

pub(super) fn vector_channel_button(
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

pub(super) fn spawn_curve_ordinate(parent: &mut ChildSpawnerCommands, minimum: f32, maximum: f32) {
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

pub(super) fn curve_drag_label_margin(time: f32, top: f32) -> UiRect {
    UiRect {
        left: Val::Px(if time > 0.82 { -62.0 } else { 9.0 }),
        top: Val::Px(if top < 18.0 { 8.0 } else { -24.0 }),
        ..default()
    }
}

pub(super) fn spawn_curve_drag_value_label(
    graph: &mut ChildSpawnerCommands,
    selection: ComplexSelection,
    key: CurveKey,
    top: f32,
    curve: &aestra_core::Curve,
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

pub(super) fn spawn_curve_graph(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input_index: u8,
    input: &InputMetadata,
    curve: &aestra_core::Curve,
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
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(5.0),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|row| {
            for (label, mode) in [
                ("Step", aestra_core::CurveInterpolation::Step),
                ("Linear", aestra_core::CurveInterpolation::Linear),
                ("Smooth", aestra_core::CurveInterpolation::Smooth),
            ] {
                crate::spawn_feathers_action_button(
                    row,
                    label,
                    CurvesAction::SetInterpolation(mode),
                    curve.interpolation == mode,
                );
            }
        });
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
                        move |click: On<PointerClick>,
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
                        move |drag: On<PointerDrag>,
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
                        move |drag: On<PointerDragEnd>,
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

pub(super) fn spawn_gradient_graph(
    parent: &mut ChildSpawnerCommands,
    module: ModuleId,
    input_index: u8,
    input: &InputMetadata,
    gradient: &aestra_core::Gradient,
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
                        move |click: On<PointerClick>,
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
                        move |drag: On<PointerDrag>,
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
                            let Some(emitter) = session.selected_layer() else { return; };
                            let Some(Value::Gradient(gradient)) = emitter
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
                        move |drag: On<PointerDragEnd>,
                              graph: Single<&ComputedNode, With<CurveGraph>>,
                              mut session: ResMut<EditorSession>,
                              mut workspace: ResMut<CurvesState>| {
                            if drag.button != PointerButton::Primary {
                                return;
                            }
                            let width = graph.size().x * graph.inverse_scale_factor;
                            let Some(emitter) = session.selected_layer() else { return; };
                            let Some(Value::Gradient(gradient)) = emitter
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

pub(super) fn spawn_complex_controls(
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

#[cfg(test)]
#[path = "tests/composition.rs"]
mod tests;
