//! Read-only material graph workspace backed by the compiler projection.

use crate::feathers::panel::spawn_panel_empty_state;
use crate::*;
use aestra_compiler::{
    MaterialCompiler, MaterialGraphNode, MaterialGraphOutput, MaterialGraphProjection,
};
use aestra_core::{MaterialExpressionId, MaterialProgramId};

pub(crate) struct EditorMaterialGraphPlugin;

impl Plugin for EditorMaterialGraphPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, handle_material_graph_actions);
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct MaterialGraphAction {
    program: MaterialProgramId,
    expression: MaterialExpressionId,
}

fn handle_material_graph_actions(
    mut actions: Query<
        (&Interaction, &MaterialGraphAction, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
    mut inspector: ResMut<MaterialStackInspectorState>,
    mut session: ResMut<EditorSession>,
    mut layout: ResMut<WorkspaceLayout>,
) {
    for (interaction, action, mut background) in &mut actions {
        match *interaction {
            Interaction::Hovered => background.0 = theme::BUTTON_HOVER,
            Interaction::None => {
                background.0 = if inspector.selected == Some((action.program, action.expression)) {
                    theme::SELECTION
                } else {
                    theme::PANEL
                };
            }
            Interaction::Pressed => {
                background.0 = theme::ACCENT_DIM;
                if inspector.selected != Some((action.program, action.expression)) {
                    inspector.selected = Some((action.program, action.expression));
                    session.ui_revision += 1;
                }
                reveal_dock_panel(&mut layout, &mut session, DockPanel::Properties);
            }
        }
    }
}

pub(crate) fn spawn_material_graph_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    inspector: &MaterialStackInspectorState,
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
        .with_children(|panel| {
            let projection = selected_projection(session, catalog);
            spawn_header(panel, projection.as_ref().ok(), localizer);
            let Ok((program_name, projection)) = projection else {
                spawn_panel_empty_state(
                    panel,
                    &localizer.text("material-graph-empty"),
                    &localizer.text("material-graph-empty-description"),
                    theme::ACCENT,
                );
                return;
            };
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    ..default()
                })
                .with_children(|body| {
                    spawn_vertical_scroll_area(
                        body,
                        ScrollMemoryKey::MaterialGraph,
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(Val::Px(10.0)),
                            row_gap: Val::Px(8.0),
                            ..default()
                        },
                        |content| {
                            spawn_summary(content, &program_name, &projection, localizer);
                            for node in &projection.nodes {
                                spawn_graph_node(content, projection.program, node, inspector);
                            }
                            spawn_outputs(content, &projection.outputs, localizer);
                        },
                    );
                });
        });
}

fn selected_projection(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
) -> Result<(String, MaterialGraphProjection), String> {
    let selected_renderer = match session.selection.primary {
        SemanticTarget::Renderer(id) => session
            .effect
            .emitters
            .iter()
            .flat_map(|emitter| &emitter.renderers)
            .find(|renderer| renderer.id == id),
        _ => session
            .selection
            .emitter(&session.effect)
            .and_then(|id| {
                session
                    .effect
                    .emitters
                    .iter()
                    .find(|emitter| emitter.id == id)
            })
            .and_then(|emitter| {
                emitter
                    .renderers
                    .iter()
                    .find(|renderer| renderer.enabled)
                    .or_else(|| emitter.renderers.first())
            }),
    }
    .ok_or_else(|| "selected emitter has no renderer".to_owned())?;
    let instance = session
        .effect
        .material_instances
        .iter()
        .find(|instance| instance.id == selected_renderer.material)
        .ok_or_else(|| "selected renderer does not use a semantic material".to_owned())?;
    let programs = catalog.material_programs_for_effect(&session.effect)?;
    let program = programs
        .iter()
        .find(|program| program.id == instance.program.id())
        .ok_or_else(|| "selected material program is unavailable".to_owned())?;
    let compiler = MaterialCompiler;
    let ir = compiler.compile(program).ok();
    Ok((
        program.name.clone(),
        compiler.project_graph(program, ir.as_ref()),
    ))
}

fn spawn_header(
    parent: &mut ChildSpawnerCommands,
    projection: Option<&(String, MaterialGraphProjection)>,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(38.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(14.0)),
                column_gap: Val::Px(10.0),
                ..default()
            },
            BackgroundColor(theme::PANEL_LIGHT),
        ))
        .with_children(|header| {
            header.spawn((
                Text::new(localizer.text("material-graph-read-only")),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
            ));
            header.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if let Some((_, graph)) = projection {
                header.spawn((
                    Text::new(format!(
                        "{} NODES  ·  {} LINKS",
                        graph.nodes.len(),
                        graph.edges.len()
                    )),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_FAINT),
                ));
            }
        });
}

fn spawn_summary(
    parent: &mut ChildSpawnerCommands,
    name: &str,
    graph: &MaterialGraphProjection,
    localizer: &Localizer,
) {
    parent.spawn((
        Text::new(name),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(theme::TEXT),
    ));
    let unreachable = graph.nodes.iter().filter(|node| !node.reachable).count();
    let disabled = graph.nodes.iter().filter(|node| node.disabled).count();
    parent.spawn((
        Text::new(format!(
            "{}  ·  {unreachable} {}  ·  {disabled} {}",
            if graph.diagnostics.is_valid() {
                localizer.text("material-graph-valid")
            } else {
                localizer.text("material-graph-invalid")
            },
            localizer.text("material-graph-unreachable"),
            localizer.text("material-graph-disabled")
        )),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(if graph.diagnostics.is_valid() {
            theme::TEXT_MUTED
        } else {
            Color::srgb(1.0, 0.58, 0.30)
        }),
    ));
}

fn spawn_graph_node(
    parent: &mut ChildSpawnerCommands,
    program: MaterialProgramId,
    node: &MaterialGraphNode,
    inspector: &MaterialStackInspectorState,
) {
    let selected = inspector.selected == Some((program, node.expression));
    parent
        .spawn((
            Button,
            MaterialGraphAction {
                program,
                expression: node.expression,
            },
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                row_gap: Val::Px(5.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(if selected {
                theme::SELECTION
            } else {
                theme::PANEL
            }),
            BorderColor::all(if selected {
                theme::ACCENT
            } else {
                theme::BORDER
            }),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(format!("{}  ·  {}", node.label, type_domain(node))),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(if node.reachable {
                    theme::TEXT
                } else {
                    theme::TEXT_FAINT
                }),
            ));
            let state = match (node.disabled, node.reachable) {
                (true, false) => "DISABLED  ·  UNREACHABLE",
                (true, true) => "DISABLED",
                (false, false) => "UNREACHABLE",
                (false, true) => "",
            };
            if !state.is_empty() {
                card.spawn((
                    Text::new(state),
                    TextFont {
                        font_size: FontSize::Px(8.0),
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.58, 0.30)),
                ));
            }
            for port in &node.inputs {
                card.spawn((
                    Text::new(format!(
                        "{}  ←  {}  ·  {:?} / {:?}",
                        port.name,
                        short_id(port.source),
                        port.value_type,
                        port.evaluation_domain
                    )),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                ));
            }
            card.spawn((
                Text::new(format!(
                    "EXPR {}  ·  IR {:?}",
                    short_id(node.expression),
                    node.ir_value
                )),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(theme::TEXT_FAINT),
            ));
        });
}

fn spawn_outputs(
    parent: &mut ChildSpawnerCommands,
    outputs: &[MaterialGraphOutput],
    localizer: &Localizer,
) {
    parent.spawn((
        Text::new(localizer.text("material-graph-outputs")),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(theme::ACCENT),
        Node {
            margin: UiRect::top(Val::Px(4.0)),
            ..default()
        },
    ));
    for output in outputs {
        parent.spawn((
            Text::new(format!(
                "{:?}  ←  {}  ·  {:?} / {:?}",
                output.kind,
                short_id(output.source),
                output.value_type,
                output.evaluation_domain
            )),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(theme::TEXT),
            Node {
                padding: UiRect::all(Val::Px(7.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_LIGHT),
        ));
    }
}

fn type_domain(node: &MaterialGraphNode) -> String {
    match (node.value_type, node.evaluation_domain) {
        (Some(value_type), Some(domain)) => format!("{value_type:?} / {domain:?}"),
        _ => "UNRESOLVED".into(),
    }
}

fn short_id(id: MaterialExpressionId) -> String {
    let id = id.to_string();
    id.chars()
        .rev()
        .take(8)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}
