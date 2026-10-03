use super::*;

fn route_label(effect: &EffectAsset, id: EventRouteId) -> String {
    let Some(route) = effect.particle_outputs.iter().find(|route| route.id == id) else {
        return id.to_string();
    };
    let emitter = effect
        .emitters
        .iter()
        .find(|item| item.id == route.source)
        .map_or_else(|| route.source.to_string(), |item| item.name.clone());
    let output = effect
        .event_outputs
        .iter()
        .find(|item| item.id == route.output)
        .map_or_else(|| route.output.to_string(), |item| item.name.clone());
    format!("{emitter} / {output} / {:?}", route.trigger)
}

pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    parent.spawn((
        Text::new(localizer.text("scene-lights-heading")),
        TextColor(theme::ACCENT),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
    ));
    let options = compatible_routes(&session.effect, None)
        .into_iter()
        .map(|id| ComboOption {
            label: route_label(&session.effect, id),
            selected: false,
            action: LightAction::Add(id),
        })
        .collect::<Vec<_>>();
    if options.is_empty() {
        parent
            .spawn_empty()
            .apply_scene(label_dim(localizer.text("scene-lights-no-routes")));
    } else {
        spawn_combo_control(
            parent,
            &localizer.text("scene-lights-add"),
            &localizer.text("scene-lights-route"),
            &options,
            250.0,
        );
    }
    for (index, binding) in session.effect.point_lights.iter().enumerate() {
        if let SemanticTarget::PointLight(id) = session.selection.primary
            && binding.id != id
        {
            continue;
        }
        parent
            .spawn((
                LightInspector(binding.id),
                RelativeCursorPosition::default(),
                PropertiesSemanticTarget {
                    target: SemanticTarget::PointLight(binding.id),
                    base_border: theme::BORDER,
                },
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::all(Val::Px(7.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(5.0),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(theme::PANEL),
                BorderColor::all(theme::BORDER),
            ))
            .with_children(|card| {
                let options = compatible_routes(&session.effect, Some(binding.id))
                    .into_iter()
                    .map(|id| ComboOption {
                        label: route_label(&session.effect, id),
                        selected: id == binding.route,
                        action: LightAction::Route(binding.id, id),
                    })
                    .collect::<Vec<_>>();
                if !options.iter().any(|option| option.selected) {
                    card.spawn((
                        Text::new(localizer.text("scene-lights-invalid-route")),
                        TextColor(Color::srgb(1.0, 0.7, 0.25)),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                    ));
                }
                combo_row(
                    card,
                    &localizer.text("scene-lights-route"),
                    &route_label(&session.effect, binding.route),
                    &options,
                );
                let color_id = binding
                    .color_parameter
                    .as_ref()
                    .map(|color| color.parameter);
                let mut colors = vec![ComboOption {
                    label: localizer.text("scene-lights-constant"),
                    selected: color_id.is_none(),
                    action: LightAction::ColorSource(binding.id, None),
                }];
                colors.extend(
                    session
                        .effect
                        .parameters
                        .iter()
                        .filter(|parameter| matches!(parameter.default, Value::Gradient(_)))
                        .map(|parameter| ComboOption {
                            label: parameter.name.clone(),
                            selected: color_id == Some(parameter.id),
                            action: LightAction::ColorSource(binding.id, Some(parameter.id)),
                        }),
                );
                let color_label = colors.iter().find(|option| option.selected).map_or_else(
                    || localizer.text("scene-lights-invalid-color"),
                    |option| option.label.clone(),
                );
                combo_row(
                    card,
                    &localizer.text("scene-lights-color"),
                    &color_label,
                    &colors,
                );
                if let Some(color) = &binding.color_parameter {
                    number_row(
                        card,
                        &localizer.text("scene-lights-age"),
                        &[(LightNumber::Age(binding.id), color.normalized_age)],
                        0.0,
                        1.0,
                        0.01,
                    );
                    if let Some(parameter) = session
                        .effect
                        .parameters
                        .iter()
                        .find(|item| item.id == color.parameter)
                        && let Value::Gradient(gradient) = &parameter.default
                    {
                        spawn_gradient(card, parameter.id, gradient, localizer);
                    }
                } else {
                    number_row(
                        card,
                        &localizer.text("scene-lights-rgb"),
                        &binding
                            .pulse
                            .linear_color
                            .iter()
                            .enumerate()
                            .map(|(channel, value)| {
                                (LightNumber::Color(binding.id, channel), *value)
                            })
                            .collect::<Vec<_>>(),
                        0.0,
                        1.0,
                        0.01,
                    );
                }
                for (kind, key) in [
                    (LightCurve::Intensity, "scene-lights-intensity"),
                    (LightCurve::Range, "scene-lights-range"),
                ] {
                    spawn_curve(card, binding, kind, &localizer.text(key), localizer);
                }
                number_row(
                    card,
                    &localizer.text("scene-lights-radius"),
                    &[(LightNumber::Radius(binding.id), binding.pulse.radius)],
                    0.0,
                    f32::MAX,
                    0.01,
                );
                number_row(
                    card,
                    &localizer.text("scene-lights-duration"),
                    &[(
                        LightNumber::Duration(binding.id),
                        binding.pulse.duration_seconds,
                    )],
                    0.001,
                    f32::MAX,
                    0.01,
                );
                card.spawn(Node {
                    width: Val::Percent(100.0),
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(5.0),
                    row_gap: Val::Px(3.0),
                    ..default()
                })
                .with_children(|actions| {
                    for (key, action) in [
                        ("scene-lights-copy", LightAction::Copy(binding.id)),
                        ("scene-lights-paste", LightAction::Paste(binding.id)),
                        ("scene-lights-remove", LightAction::Remove(binding.id)),
                    ] {
                        spawn_feathers_action_button(actions, &localizer.text(key), action, false);
                    }
                });
                let prefix = format!("effect.point_lights[{index}]");
                for diagnostic in &session.diagnostics.diagnostics {
                    if diagnostic.path == prefix
                        || diagnostic.path.starts_with(&format!("{prefix}."))
                    {
                        card.spawn((
                            Text::new(&diagnostic.message),
                            TextColor(Color::srgb(1.0, 0.7, 0.25)),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                        ));
                    }
                }
            });
    }
}

fn combo_row(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    current: &str,
    options: &[ComboOption<LightAction>],
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            spawn_property_label(row, title);
            if options.is_empty() {
                row.spawn_empty().apply_scene(label_dim(current.to_owned()));
            } else {
                spawn_combo_control(row, current, title, options, 240.0);
            }
        });
}

fn number_row(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    values: &[(LightNumber, f32)],
    min: f32,
    max: f32,
    step: f32,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|row| {
            spawn_property_label(row, title);
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                column_gap: Val::Px(3.0),
                ..default()
            })
            .with_children(|controls| {
                for (control, value) in values {
                    controls
                        .spawn(Node {
                            flex_grow: 1.0,
                            flex_basis: Val::Px(0.0),
                            min_width: Val::Px(44.0),
                            ..default()
                        })
                        .with_children(|wrapper| {
                            wrapper
                                .spawn_empty()
                                .apply_scene(ui_shell::feathers_scalar_input())
                                .insert((
                                    *control,
                                    ScrubbableNumber::new(*value, min, max, step),
                                    AccessibleLabel(title.to_owned()),
                                ));
                        });
                }
            });
        });
}

fn graph(parent: &mut ChildSpawnerCommands, data: AutomationCurveData) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Px(automation_curve::DEFAULT_HEIGHT),
            ..default()
        })
        .with_children(|graph| {
            automation_curve::spawn_automation_curve(graph, &data);
        });
}

fn spawn_curve(
    parent: &mut ChildSpawnerCommands,
    binding: &PointLightBinding,
    kind: LightCurve,
    title: &str,
    localizer: &Localizer,
) {
    let curve = kind.get(binding);
    parent
        .spawn_empty()
        .apply_scene(label_dim(title.to_owned()));
    graph(
        parent,
        AutomationCurveData::Curve {
            points: curve
                .keys
                .iter()
                .map(|key| AutomationCurvePoint {
                    time: key.time,
                    value: curve.output_value(key.value),
                })
                .collect(),
            value_bounds: None,
            interpolation: curve.interpolation,
        },
    );
    let interpolation = [
        (aestra_core::CurveInterpolation::Step, "scene-lights-step"),
        (
            aestra_core::CurveInterpolation::Linear,
            "scene-lights-linear",
        ),
        (
            aestra_core::CurveInterpolation::Smooth,
            "scene-lights-smooth",
        ),
    ]
    .map(|(mode, key)| ComboOption {
        label: localizer.text(key),
        selected: curve.interpolation == mode,
        action: LightAction::Interpolation(binding.id, kind, mode),
    });
    let current = interpolation.iter().find(|item| item.selected).unwrap();
    combo_row(
        parent,
        &localizer.text("scene-lights-interpolation"),
        &current.label,
        &interpolation,
    );
    for (index, key) in curve.keys.iter().enumerate() {
        parent
            .spawn(Node {
                width: Val::Percent(100.0),
                column_gap: Val::Px(3.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                row.spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    ..default()
                })
                .with_children(|fields| {
                    number_row(
                        fields,
                        &localizer.text("scene-lights-key-age"),
                        &[(LightNumber::CurveTime(binding.id, kind, index), key.time)],
                        0.0,
                        1.0,
                        0.01,
                    );
                    let min = if kind == LightCurve::Range {
                        0.001
                    } else {
                        0.0
                    };
                    let (min, max) = curve
                        .output_range
                        .map_or((min, f32::MAX), |range| (range.min, range.max));
                    number_row(
                        fields,
                        title,
                        &[(
                            LightNumber::CurveValue(binding.id, kind, index),
                            curve.output_value(key.value),
                        )],
                        min,
                        max,
                        if kind == LightCurve::Intensity {
                            100.0
                        } else {
                            0.1
                        },
                    );
                });
                if curve.keys.len() > 1 {
                    spawn_feathers_action_button(
                        row,
                        "×",
                        LightAction::RemoveKey(binding.id, kind, index),
                        false,
                    );
                }
            });
    }
    if curve.keys.len() < aestra_core::MAX_LIGHT_CURVE_KEYS {
        spawn_feathers_action_button(
            parent,
            &localizer.text("scene-lights-add-key"),
            LightAction::AddKey(binding.id, kind),
            false,
        );
    }
}

fn spawn_gradient(
    parent: &mut ChildSpawnerCommands,
    id: ParameterId,
    gradient: &Gradient,
    localizer: &Localizer,
) {
    parent
        .spawn_empty()
        .apply_scene(label_dim(localizer.text("scene-lights-shared-gradient")));
    graph(
        parent,
        AutomationCurveData::Gradient(
            gradient
                .keys
                .iter()
                .map(|key| AutomationGradientPoint {
                    time: key.time,
                    color: key.color,
                })
                .collect(),
        ),
    );
    for (index, key) in gradient.keys.iter().enumerate() {
        number_row(
            parent,
            &localizer.text("scene-lights-key-age"),
            &[(LightNumber::GradientTime(id, index), key.time)],
            0.0,
            1.0,
            0.01,
        );
        number_row(
            parent,
            &localizer.text("scene-lights-rgba"),
            &key.color
                .iter()
                .enumerate()
                .map(|(channel, value)| (LightNumber::GradientChannel(id, index, channel), *value))
                .collect::<Vec<_>>(),
            0.0,
            1.0,
            0.01,
        );
        if gradient.keys.len() > 1 {
            spawn_feathers_action_button(
                parent,
                "×",
                LightAction::RemoveColorKey(id, index),
                false,
            );
        }
    }
    spawn_feathers_action_button(
        parent,
        &localizer.text("scene-lights-add-key"),
        LightAction::AddColorKey(id),
        false,
    );
}
