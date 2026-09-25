//! The Module Stack panel (extensible-stages M9, §28.1–28.4): the navigable, stage-grouped list of an
//! emitter's modules and renderers, its filter, section headers and per-row diagnostics badges.

use super::*;

/// The Module Stack panel (extensible-stages M9, §28.1): a navigable, selectable list — the Effect and
/// Emitter items, then the stage-grouped compact module/renderer rows with per-stage add buttons and
/// diagnostics. Selecting a row drives the Properties panel (via the global `select_properties_header`
/// observer). Editing lives entirely in the Properties panel; this panel is navigation only.
pub(crate) fn spawn_module_stack_panel(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
    palette: &ModulePaletteState,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    let Some(layer) = session.selected_layer() else {
        parent
            .spawn(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|panel| {
                panel.spawn((
                    Text::new(localizer.text("properties-no-emitter")),
                    TextFont {
                        font_size: FontSize::Px(13.0),
                        ..default()
                    },
                    TextColor(theme::TEXT),
                    Node {
                        margin: UiRect::all(Val::Px(14.0)),
                        ..default()
                    },
                ));
            });
        return;
    };
    let Some(emitter_index) = session.selected_layer_index() else {
        return;
    };
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            // Filters rows in place as you type (see `apply_module_stack_filter`).
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(7.0), Val::Px(5.0)),
                    flex_shrink: 0.0,
                    ..default()
                })
                .with_children(|bar| {
                    crate::feathers::search_field::spawn_search_field(
                        bar,
                        "",
                        &localizer.text("module-stack-filter"),
                        &localizer.text("module-stack-filter-clear"),
                        ModuleStackSearch,
                    );
                });
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    ..default()
                })
                .with_children(|body| {
                    spawn_vertical_scroll_area(
                        body,
                        ScrollMemoryKey::Properties,
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::bottom(Val::Px(10.0)),
                            ..default()
                        },
                        |stack| {
                            // Effect + Emitter as selectable items (Niagara-style): their settings edit
                            // in the Properties panel, keeping the stack a clean navigable list (§28.1).
                            spawn_stack_nav_item(
                                stack,
                                &localizer.text("properties-effect"),
                                &session.effect.name,
                                SemanticTarget::Effect(session.effect.id),
                                session,
                            );
                            // Stages without a CPU reference say so (extensible plan §13.3).
                            let stage_types = aestra_compiler::ExtensionRegistry::linked().stages;
                            let gpu_only = |stage_type: &aestra_core::StageTypeId| {
                                stage_types
                                    .get(stage_type)
                                    .is_some_and(|stage| !stage.backend.has_cpu_reference())
                                    .then(|| localizer.text("properties-stage-gpu-only"))
                            };
                            // The effect's own simulation stages (fluid F2): domains its emitters
                            // share, listed with the effect rather than inside an emitter.
                            for (stage_index, stage) in
                                session.effect.simulation_stages.iter().enumerate()
                            {
                                spawn_simulation_stage_header(
                                    stack,
                                    &stage.name,
                                    gpu_only(&stage.stage_type).as_deref(),
                                );
                                for (module_index, module) in stage.modules.iter().enumerate() {
                                    spawn_module_stack_row(
                                        stack,
                                        module,
                                        registry.0.get(&module.module_type),
                                        &format!(
                                            "effect.simulation_stages[{stage_index}].modules[{module_index}]"
                                        ),
                                        session,
                                        asset_server,
                                    );
                                }
                            }
                            spawn_stack_nav_item(
                                stack,
                                &localizer.text("properties-emitter"),
                                &layer.name,
                                SemanticTarget::Emitter(layer.id),
                                session,
                            );
                            // Fixed lifecycle sections, including empty ones (§31.2).
                            let modules_path =
                                format!("effect.emitters[{emitter_index}].modules");
                            for stage in StackStage::LIFECYCLE {
                                spawn_stage_header(stack, stage);
                                let semantic =
                                    stage.semantic().expect("lifecycle stage has semantics");
                                for (module_index, module) in layer.modules.iter().enumerate() {
                                    if module.stage != semantic {
                                        continue;
                                    }
                                    spawn_module_stack_row(
                                        stack,
                                        module,
                                        registry.0.get(&module.module_type),
                                        &format!(
                                            "effect.emitters[{emitter_index}].modules[{module_index}]"
                                        ),
                                        session,
                                        asset_server,
                                    );
                                }
                                spawn_stage_diagnostics(
                                    stack,
                                    stage,
                                    &modules_path,
                                    session,
                                    registry,
                                );
                            }
                            // Authored simulation stages, in first-appearance order (§28.3).
                            let mut seen: Vec<&str> = Vec::new();
                            for module in &layer.modules {
                                let StageKind::Simulation(name) = &module.stage else {
                                    continue;
                                };
                                if seen.contains(&name.as_str()) {
                                    continue;
                                }
                                seen.push(name);
                                spawn_simulation_stage_header(
                                    stack,
                                    name,
                                    gpu_only(&layer.simulation_stage_type(name)).as_deref(),
                                );
                                for (module_index, sim_module) in
                                    layer.modules.iter().enumerate()
                                {
                                    if sim_module.stage != module.stage {
                                        continue;
                                    }
                                    spawn_module_stack_row(
                                        stack,
                                        sim_module,
                                        registry.0.get(&sim_module.module_type),
                                        &format!(
                                            "effect.emitters[{emitter_index}].modules[{module_index}]"
                                        ),
                                        session,
                                        asset_server,
                                    );
                                }
                            }
                            // Render section.
                            spawn_stage_header(stack, StackStage::Render);
                            for (renderer_index, renderer) in layer.renderers.iter().enumerate() {
                                spawn_renderer_stack_row(
                                    stack,
                                    renderer,
                                    &format!(
                                        "effect.emitters[{emitter_index}].renderers[{renderer_index}]"
                                    ),
                                    session,
                                );
                            }
                            spawn_stage_diagnostics(
                                stack,
                                StackStage::Render,
                                &format!("effect.emitters[{emitter_index}].renderers"),
                                session,
                                registry,
                            );
                        },
                    );
                });
            if palette.open {
                spawn_module_palette(panel, registry, palette);
            }
        });
}

/// A selectable Effect / Emitter navigation item at the top of the Module Stack panel (extensible-stages
/// M9, §28.1): a small kind caption and the item's name. Carries the selection/highlight components so
/// clicking it selects the target (via the global observer) and drives the Properties panel.
pub(super) fn spawn_stack_nav_item(
    parent: &mut ChildSpawnerCommands,
    kind: &str,
    name: &str,
    target: SemanticTarget,
    session: &EditorSession,
) {
    let selected = session.selection.primary == target;
    let base_border = if selected {
        theme::ACCENT_DIM
    } else {
        theme::BORDER
    };
    parent
        .spawn((
            PropertiesSemanticTarget {
                target,
                base_border,
            },
            PropertiesSelectionTarget(target),
            Node {
                width: Val::Auto,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                margin: UiRect::axes(Val::Px(7.0), Val::Px(1.0)),
                min_height: Val::Px(30.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(if selected {
                theme::PANEL_LIGHT
            } else {
                theme::PANEL
            }),
            BorderColor::all(base_border),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(kind.to_uppercase()),
                bevy::feathers::theme::ThemedText,
                TextColor(theme::TEXT_FAINT),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
            row.spawn((
                Text::new(name),
                bevy::feathers::theme::ThemedText,
                TextColor(theme::TEXT),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextLayout {
                    linebreak: LineBreak::NoWrap,
                    ..default()
                },
                Pickable::IGNORE,
            ));
        });
}

/// The Module Stack's filter query. It lives outside the panel so typing filters rows in place
/// instead of rebuilding the panel, which would drop the search field's keyboard focus.
#[derive(Resource, Default)]
pub(crate) struct ModuleStackFilter {
    pub(super) query: String,
}

/// Marks the Module Stack's filter field.
#[derive(Component)]
pub(super) struct ModuleStackSearch;

/// A stack row's lowercased searchable text: its title, summary and type id.
#[derive(Component)]
pub(super) struct StackRowSearchText(String);

impl StackRowSearchText {
    pub(super) fn new(title: &str, summary: &str, type_id: &str) -> Self {
        Self(format!("{title} {summary} {type_id}").to_lowercase())
    }
}

/// Whether a row's searchable text contains every whitespace-separated term of the query.
pub(super) fn stack_row_matches(text: &str, query: &str) -> bool {
    query
        .split_whitespace()
        .all(|term| text.contains(&term.to_lowercase()))
}

pub(super) fn change_module_stack_filter(
    event: On<ValueChange<String>>,
    inputs: Query<(), With<ModuleStackSearch>>,
    mut filter: ResMut<ModuleStackFilter>,
) {
    if inputs.contains(event.source) && filter.query != event.value {
        filter.query.clone_from(&event.value);
    }
}

/// Shows or hides stack rows for the filter query, and seeds a freshly built filter field with the
/// current query (the panel rebuilds on edits; the query survives).
pub(super) fn apply_module_stack_filter(
    filter: Res<ModuleStackFilter>,
    added_rows: Query<(), Added<StackRowSearchText>>,
    mut rows: Query<(&StackRowSearchText, &mut Node)>,
    added_search: Query<&Children, Added<ModuleStackSearch>>,
    mut editable: Query<&mut EditableText, With<bevy::feathers::controls::FeathersTextInput>>,
) {
    for children in &added_search {
        for child in children.iter() {
            if let Ok(mut text) = editable.get_mut(child)
                && text.value().to_string() != filter.query
            {
                text.editor_mut().set_text(&filter.query);
            }
        }
    }
    if !filter.is_changed() && added_rows.is_empty() {
        return;
    }
    for (text, mut node) in &mut rows {
        let display = if stack_row_matches(&text.0, &filter.query) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

/// A section header for an authored simulation stage (extensible-stages M9, §28.3). Unlike the fixed
/// lifecycle stages it carries no add button — authoring modules into a plugin-defined stage arrives
/// with plugin loading — and its title is the authored stage name.
/// A simulation-stage section header; gpu_only labels a stage with no CPU reference.
pub(super) fn spawn_simulation_stage_header(
    parent: &mut ChildSpawnerCommands,
    name: &str,
    gpu_only: Option<&str>,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(24.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                margin: UiRect::top(Val::Px(3.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new("SIMULATION"),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
            row.spawn((
                Text::new(name.to_uppercase()),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::ACCENT),
            ));
            if let Some(label) = gpu_only {
                row.spawn((
                    Text::new(label),
                    TextFont {
                        font_size: FontSize::Px(8.0),
                        ..default()
                    },
                    TextColor(diagnostic_color(DiagnosticSeverity::Warning)),
                ));
            }
        });
}

/// Whether a diagnostic at `diagnostic_path` belongs to the item at `path`: the item itself or
/// something inside it. Matches on path boundaries so `modules[1]` never claims `modules[10]`.
pub(super) fn diagnostic_belongs_to(diagnostic_path: &str, path: &str) -> bool {
    diagnostic_path
        .strip_prefix(path)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '[']))
}

pub(super) fn diagnostic_color(severity: DiagnosticSeverity) -> Color {
    match severity {
        DiagnosticSeverity::Error => Color::srgb(1.0, 0.38, 0.32),
        DiagnosticSeverity::Warning => Color::srgb(1.0, 0.72, 0.28),
        DiagnosticSeverity::Info => theme::TEXT_MUTED,
    }
}

/// A compact diagnostics badge for a stack row: the number of findings on the item, colored by the
/// most severe one, with the messages in its tooltip. Nothing when the item is clean.
pub(super) fn spawn_row_diagnostics_badge(
    parent: &mut ChildSpawnerCommands,
    path: &str,
    session: &EditorSession,
) {
    let findings: Vec<_> = session
        .diagnostics
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic_belongs_to(&diagnostic.path, path))
        .collect();
    let rank = |severity: DiagnosticSeverity| match severity {
        DiagnosticSeverity::Error => 2,
        DiagnosticSeverity::Warning => 1,
        DiagnosticSeverity::Info => 0,
    };
    let Some(worst) = findings
        .iter()
        .map(|diagnostic| diagnostic.severity)
        .max_by_key(|severity| rank(*severity))
    else {
        return;
    };
    let summary = if findings.len() == 1 {
        "1 issue".to_owned()
    } else {
        format!("{} issues", findings.len())
    };
    let details = findings
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    parent
        .spawn((
            RowDiagnosticsBadge,
            EditorTooltip::titled(&summary, &details),
            AccessibleLabel(summary.clone()),
            Node {
                min_width: Val::Px(16.0),
                height: Val::Px(16.0),
                padding: UiRect::horizontal(Val::Px(4.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                border_radius: BorderRadius::all(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(diagnostic_color(worst)),
        ))
        .with_child((
            Text::new(findings.len().to_string()),
            TextColor(theme::PANEL_DARK),
            TextFont {
                font_size: FontSize::Px(9.0),
                ..default()
            },
            Pickable::IGNORE,
        ));
}

/// Marks a stack row's diagnostics badge.
#[derive(Component)]
pub(super) struct RowDiagnosticsBadge;
