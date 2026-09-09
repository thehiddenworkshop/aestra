//! Typed browser function drops shared by material and function graph viewports.
use super::*;
use crate::asset_browser::payload::{AssetPayload, AuthoringDropGuard};
use crate::material_document::MaterialEditingTarget;
use crate::material_function_editor::FunctionEditor;
use aestra_authoring::{MaterialCommand, MaterialFunctionBodyCommand, MaterialTransaction};
use aestra_core::material::{MaterialFunction, MaterialFunctionRef};
use aestra_project::ProjectAssetId;
use bevy::picking::{
    events::{DragEnter, DragLeave},
    pointer::PointerButton,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Program(MaterialProgramId),
    Function(MaterialFunctionId),
}

/// A surviving UI entity must not edit a graph from a previous document/target.
#[derive(Component, Clone)]
pub(crate) struct GraphDropTarget {
    owner: Owner,
    context: MaterialEditingTarget,
    effect: aestra_core::EffectId,
}

impl GraphDropTarget {
    pub(crate) fn program(session: &EditorSession, id: MaterialProgramId) -> Self {
        Self {
            owner: Owner::Program(id),
            context: session.material_target.clone(),
            effect: session.effect.id,
        }
    }
    pub(crate) fn function(session: &EditorSession, id: MaterialFunctionId) -> Self {
        Self {
            owner: Owner::Function(id),
            context: session.material_target.clone(),
            effect: session.effect.id,
        }
    }
    fn check(&self, session: &EditorSession) -> Result<(), String> {
        if session.pending_change.is_some() {
            return Err("Resolve the pending change before adding a function".into());
        }
        if self.context != session.material_target || self.effect != session.effect.id {
            return Err("Graph changed; drag the function again".into());
        }
        Ok(())
    }
    fn key(&self, catalog: &ProjectEffectCatalog) -> String {
        match self.owner {
            Owner::Program(id) => material_graph_view_key(id),
            Owner::Function(id) => format!("function:{}:{id}", catalog.root().display()),
        }
    }
}

enum Replacement {
    Program {
        before: MaterialProgram,
        after: Box<MaterialProgram>,
    },
    Function(MaterialFunction),
}
struct DropPlan {
    replacement: Replacement,
    created: Vec<MaterialExpressionId>,
    calls: Vec<MaterialExpressionId>,
    label: String,
}

fn plan(
    payload: &AssetPayload,
    target: &GraphDropTarget,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
) -> Result<DropPlan, String> {
    target.check(session)?;
    let Some(ProjectAssetId::MaterialFunction(id)) = payload.resolve(catalog)? else {
        return Err("Drop a material function to add a call node".into());
    };
    let library = catalog
        .material_function_library()
        .map_err(|error| error.to_string())?;
    let mut document = session.graph_authoring_document(catalog)?;
    let function = document
        .material_functions
        .iter()
        .find(|function| function.id == id)
        .cloned()
        .ok_or("Function is unavailable")?;
    if function.outputs.is_empty() {
        return Err("Function has no outputs".into());
    }
    let kinds = function
        .outputs
        .iter()
        .map(|output| MaterialGraphCreateKind::FunctionCall {
            function: MaterialFunctionRef::Project(id),
            output: output.id,
        });
    let mut created = Vec::new();
    // The graph represents one expression per output, just like the Add Node menu.
    // Add every output in one history entry rather than silently losing outputs.
    let replacement = match target.owner {
        Owner::Program(program) => {
            if selected_projection(session, catalog)?.1.program != program {
                return Err("Material selection changed; drag onto the current graph".into());
            }
            let before = document
                .programs
                .iter()
                .find(|value| value.id == program)
                .cloned()
                .ok_or("Material graph is unavailable")?;
            for kind in kinds {
                let plan = MaterialToolPlanner::plan(
                    &document,
                    MaterialToolCommand::CreateMaterialGraphNode {
                        program,
                        kind,
                        source: None,
                        target: None,
                    },
                )
                .map_err(|error| error.to_string())?;
                MaterialCommandExecutor::execute(&mut document, &plan.transaction)
                    .map_err(|error| error.to_string())?;
                created.extend(plan.created_expressions);
            }
            let after = document
                .programs
                .into_iter()
                .find(|value| value.id == program)
                .ok_or("Material disappeared")?;
            Replacement::Program {
                before,
                after: Box::new(after),
            }
        }
        Owner::Function(owner) => {
            let mut after = session.graph_function(catalog)?;
            if after.id != owner {
                return Err("Function graph changed".into());
            }
            if owner == id {
                return Err("A function cannot call itself".into());
            }
            if after.custom_wesl.is_some() {
                return Err("Custom WESL bodies cannot accept graph nodes".into());
            }
            let mut edits = Vec::new();
            for kind in kinds {
                let expressions = MaterialCompiler
                    .function_graph_node_expressions(&after, kind, &library)
                    .map_err(|error| error.to_string())?;
                for expression in expressions {
                    created.push(expression.id);
                    edits.push(MaterialCommand::EditMaterialFunctionBody {
                        function: owner,
                        edit: MaterialFunctionBodyCommand::Add {
                            index: after.expressions.len(),
                            expression: expression.clone(),
                        },
                    });
                    after.expressions.push(expression);
                }
            }
            MaterialCommandExecutor::execute(
                &mut document,
                &MaterialTransaction::new("Drop material function", edits),
            )
            .map_err(|error| error.to_string())?;
            // Match FunctionEditor validation: all indexed callers and drafts, not just
            // the visible graph. This also rejects indirect recursive dependencies.
            let programs = catalog
                .content()
                .asset_index()
                .material_programs()
                .iter()
                .filter_map(|entry| entry.reference.map(|reference| reference.id()))
                .map(|id| catalog.material_program(id))
                .collect::<Result<Vec<_>, _>>()?;
            let all = MaterialAuthoringDocument::standalone(programs)
                .with_material_functions(catalog.material_functions()?);
            let edit = all
                .plan_function_edit(owner, after.clone())
                .map_err(|error| error.to_string())?;
            if !edit.diagnostics.is_valid() {
                return Err(edit.diagnostics.to_string());
            }
            Replacement::Function(after)
        }
    };
    let expressions = match &replacement {
        Replacement::Program { after, .. } => &after.expressions,
        Replacement::Function(after) => &after.expressions,
    };
    let calls = expressions
        .iter()
        .filter(|expression| {
            created.contains(&expression.id)
                && matches!(expression.kind, MaterialExpressionKind::FunctionCall { .. })
        })
        .map(|expression| expression.id)
        .collect();
    Ok(DropPlan {
        replacement,
        created,
        calls,
        label: function.name,
    })
}

#[derive(Component)]
struct Feedback {
    payload: AssetPayload,
    target: Entity,
}

pub(super) fn register(app: &mut App) {
    app.add_observer(hover)
        .add_observer(leave)
        .add_observer(drop_function)
        .add_systems(Update, cleanup);
}

fn source(
    entity: Entity,
    sources: &Query<&AssetPayload>,
    parents: &Query<&ChildOf>,
) -> Option<AssetPayload> {
    std::iter::once(entity)
        .chain(parents.iter_ancestors(entity))
        .find_map(|entity| sources.get(entity).ok().cloned())
}
fn destination(
    entity: Entity,
    targets: &Query<&GraphDropTarget>,
    parents: &Query<&ChildOf>,
) -> Option<(Entity, GraphDropTarget)> {
    std::iter::once(entity)
        .chain(parents.iter_ancestors(entity))
        .find_map(|entity| {
            targets
                .get(entity)
                .ok()
                .cloned()
                .map(|target| (entity, target))
        })
}

#[allow(clippy::too_many_arguments)]
fn hover(
    mut event: On<Pointer<DragEnter>>,
    sources: Query<&AssetPayload>,
    targets: Query<&GraphDropTarget>,
    parents: Query<&ChildOf>,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    guard: AuthoringDropGuard,
    feedback: Query<Entity, With<Feedback>>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    let Some(payload) = source(event.dragged, &sources, &parents) else {
        return;
    };
    let Some((entity, target)) = destination(event.entity, &targets, &parents) else {
        return;
    };
    event.propagate(false);
    for entity in &feedback {
        commands.entity(entity).try_despawn();
    }
    let result = guard
        .check()
        .and_then(|()| plan(&payload, &target, &session, &catalog));
    let color = if result.is_ok() {
        theme::ACCENT
    } else {
        Color::srgb(0.95, 0.3, 0.3)
    };
    let label = result.map_or_else(
        |error| error,
        |plan| {
            if plan.calls.len() > 1 {
                format!(
                    "Release to add {} · {} outputs",
                    plan.label,
                    plan.calls.len()
                )
            } else {
                format!("Release to add {}", plan.label)
            }
        },
    );
    commands.entity(entity).with_children(|parent| {
        parent
            .spawn((
                Feedback {
                    payload,
                    target: entity,
                },
                Pickable::IGNORE,
                GlobalZIndex(275),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BorderColor::all(color),
            ))
            .with_child((
                Text::new(label),
                TextColor(theme::TEXT),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                Pickable::IGNORE,
                BackgroundColor(theme::PANEL),
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(0.0),
                    right: Val::Px(0.0),
                    max_width: Val::Percent(100.0),
                    padding: UiRect::all(Val::Px(5.0)),
                    ..default()
                },
            ));
    });
}

fn leave(
    event: On<Pointer<DragLeave>>,
    parents: Query<&ChildOf>,
    feedback: Query<(Entity, &Feedback)>,
    mut commands: Commands,
) {
    for (entity, marker) in &feedback {
        if event.entity == marker.target
            || parents
                .iter_ancestors(event.entity)
                .any(|entity| entity == marker.target)
        {
            commands.entity(entity).try_despawn();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn drop_function(
    mut event: On<Pointer<DragDrop>>,
    sources: Query<&AssetPayload>,
    targets: Query<&GraphDropTarget>,
    parents: Query<&ChildOf>,
    geometry: Query<(&FeathersGraphViewport, &ComputedNode, &UiGlobalTransform)>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    guard: AuthoringDropGuard,
    mut history: ResMut<MaterialProgramEditHistory>,
    mut ledger: ResMut<EditorHistoryLedger>,
    mut functions: ResMut<FunctionEditor>,
    mut memory: ResMut<GraphViewportMemory>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    feedback: Query<Entity, With<Feedback>>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    let Some(payload) = source(event.dropped, &sources, &parents) else {
        return;
    };
    let Some((entity, target)) = destination(event.entity, &targets, &parents) else {
        return;
    };
    event.propagate(false);
    for entity in &feedback {
        commands.entity(entity).try_despawn();
    }
    let result = (|| {
        if keys
            .as_ref()
            .is_some_and(|keys| keys.just_pressed(KeyCode::Escape))
        {
            return Err("Function drop cancelled".into());
        }
        guard.check()?;
        let (viewport, computed, transform) = geometry
            .get(entity)
            .map_err(|_| "Graph viewport is unavailable")?;
        let position = viewport.unproject_viewport_point(pointer_position_in_node(
            event.pointer_location.position,
            computed,
            transform,
        ));
        let plan = plan(&payload, &target, &session, &catalog)?;
        match plan.replacement {
            Replacement::Program { before, after } => {
                if session.standalone_material().is_some() {
                    session.material_history_active = true;
                }
                history.execute_replacement(
                    &mut session,
                    &mut catalog,
                    "Drop material function",
                    before,
                    *after,
                )?;
                ledger.record_material_edit(&mut session);
            }
            Replacement::Function(after) => functions.edit(&mut session, &mut catalog, after)?,
        }
        // Put the call under the pointer; keep generated default inputs to its left.
        let key = target.key(&catalog);
        let mut default_row = 0;
        for expression in plan.created {
            let offset = if let Some(row) = plan.calls.iter().position(|id| *id == expression) {
                Vec2::new(0.0, row as f32 * 220.0)
            } else {
                let offset = Vec2::new(-COLUMN_WIDTH, default_row as f32 * 110.0);
                default_row += 1;
                offset
            };
            let node_key = match target.owner {
                Owner::Program(_) => material_graph_expression_node_key(expression),
                Owner::Function(_) => expression.to_string(),
            };
            memory.place_node(
                key.clone(),
                node_key,
                position + offset - Vec2::new(NODE_WIDTH * 0.5, NODE_HEADER_HEIGHT * 0.5),
            );
        }
        Ok::<_, String>(format!("Added {} function call", plan.label))
    })();
    session.status = result.unwrap_or_else(|error| format!("Function drop rejected: {error}"));
    session.ui_revision += 1;
}

fn cleanup(
    sources: Query<&AssetPayload>,
    feedback: Query<(Entity, &Feedback)>,
    targets: Query<&GraphDropTarget>,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut commands: Commands,
) {
    for (entity, marker) in &feedback {
        if keys
            .as_ref()
            .is_some_and(|keys| keys.just_pressed(KeyCode::Escape))
            || marker.payload.resolve(&catalog).is_err()
            || targets
                .get(marker.target)
                .map_or(true, |target| target.check(&session).is_err())
            || !sources.iter().any(|source| *source == marker.payload)
        {
            commands.entity(entity).try_despawn();
        }
    }
}

#[cfg(test)]
mod tests;
