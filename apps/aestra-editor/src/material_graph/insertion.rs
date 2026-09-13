//! One semantic insertion adapter for material programs and function bodies.
use super::*;
use crate::feathers::node_graph::GraphWidgetSync;
use crate::feathers::node_graph::insertion::{self as widget, Candidate, Wire};
use aestra_authoring::{MaterialCommand, MaterialFunctionBodyCommand, MaterialTransaction};
use aestra_compiler::{MaterialFunctionBodyProjection, MaterialFunctionGraphTarget};

pub(super) fn register(app: &mut App) {
    app.init_resource::<widget::State>()
        .add_observer(probe)
        .add_observer(drop_node)
        .add_systems(Update, widget::refresh.after(GraphWidgetSync));
}

pub(crate) fn function_wire(
    source: MaterialExpressionId,
    target: &MaterialFunctionGraphTarget,
) -> Option<Wire> {
    use crate::feathers::node_graph::geometry::GraphGeometryPort;
    Some(match target {
        MaterialFunctionGraphTarget::Output(output) => Wire {
            source: GraphNodeKey::Expression(source),
            target: GraphNodeKey::FunctionOutputs,
            port: GraphGeometryPort::FunctionOutput(*output),
        },
        MaterialFunctionGraphTarget::Argument { expression, input } => Wire::material(
            source,
            MaterialConnectionTarget::ExpressionInput {
                expression: *expression,
                input: MaterialExpressionInput::FunctionArgument(*input),
            },
        ),
        MaterialFunctionGraphTarget::Input { expression, port } => {
            Wire::material(source, input_target(*expression, port)?)
        }
    })
}

enum Replacement {
    Program(MaterialProgram, Box<MaterialProgram>),
    Function(aestra_core::material::MaterialFunction),
}

fn command(
    asset: crate::document::DocumentKey,
    source: MaterialExpressionId,
    target: GraphNodeKey,
    port: crate::feathers::node_graph::geometry::GraphGeometryPort,
) -> Result<MaterialCommand, String> {
    use crate::document::DocumentKey;
    use crate::feathers::node_graph::geometry::GraphGeometryPort as Port;
    match (asset, target, port) {
        (
            DocumentKey::MaterialProgram(program),
            GraphNodeKey::Expression(expression),
            Port::Input(input),
        ) => Ok(MaterialCommand::RewireMaterialExpressionInput {
            program,
            expression,
            input,
            source,
        }),
        (
            DocumentKey::MaterialProgram(program),
            GraphNodeKey::MaterialOutputs,
            Port::MaterialOutput(output),
        ) => Ok(MaterialCommand::SetMaterialOutput {
            program,
            output,
            expression: source,
        }),
        (
            DocumentKey::MaterialFunction(function),
            GraphNodeKey::Expression(expression),
            Port::Input(input),
        ) => Ok(MaterialCommand::EditMaterialFunctionBody {
            function,
            edit: MaterialFunctionBodyCommand::Rewire {
                expression,
                input,
                source,
            },
        }),
        (
            DocumentKey::MaterialFunction(function),
            GraphNodeKey::FunctionOutputs,
            Port::FunctionOutput(output),
        ) => Ok(MaterialCommand::EditMaterialFunctionBody {
            function,
            edit: MaterialFunctionBodyCommand::SetOutput { output, source },
        }),
        _ => Err("Unsupported insertion target".into()),
    }
}

fn plan(
    document: &MaterialAuthoringDocument,
    candidate: &Candidate,
) -> Result<Replacement, String> {
    use crate::document::DocumentKey;
    use crate::feathers::node_graph::geometry::GraphGeometryPort as Port;
    let GraphNodeKey::Expression(inserted) = candidate.node else {
        return Err("Only expression nodes can be inserted".into());
    };
    let GraphNodeKey::Expression(source) = candidate.wire.source else {
        return Err("Wire source disappeared".into());
    };
    if candidate.wire.source == candidate.node || candidate.wire.target == candidate.node {
        return Err("Cannot insert a node onto its own wire".into());
    }
    let library =
        aestra_compiler::MaterialFunctionLibrary::new(document.material_functions.clone());
    let asset = candidate.view.document.asset;
    let (expressions, edges) = match asset {
        DocumentKey::MaterialProgram(id) => {
            let program = document
                .programs
                .iter()
                .find(|p| p.id == id)
                .ok_or("Material changed")?;
            let graph = MaterialCompiler.project_graph_with_functions(program, None, &library);
            (
                &program.expressions,
                graph
                    .edges
                    .iter()
                    .filter_map(|edge| {
                        edge_target(&edge.target).map(|target| Wire::material(edge.source, target))
                    })
                    .collect::<Vec<_>>(),
            )
        }
        DocumentKey::MaterialFunction(id) => {
            let function = document
                .material_functions
                .iter()
                .find(|f| f.id == id)
                .ok_or("Function changed")?;
            let MaterialFunctionBodyProjection::Graph { edges, .. } = MaterialCompiler
                .project_function_graph(function, &library)
                .body
            else {
                return Err("Custom WESL bodies are read-only".into());
            };
            (
                &function.expressions,
                edges
                    .iter()
                    .filter_map(|edge| function_wire(edge.source, &edge.target))
                    .collect(),
            )
        }
        _ => return Err("Unsupported graph".into()),
    };
    if !edges.contains(&candidate.wire) {
        return Err("The wire changed; try again".into());
    }
    if edges.iter().any(|edge| edge.source == candidate.node) {
        return Err("Use a node whose output is not already connected".into());
    }
    if candidate.inputs.len() > 16 {
        return Err("Too many input choices; connect the sockets manually".into());
    }
    let mut valid = Vec::new();
    let mut seen = Vec::new();
    for input in candidate.inputs.iter().copied() {
        if seen.contains(&input) {
            continue;
        }
        seen.push(input);
        // Never silently replace a non-literal input branch. Default literals can be replaced.
        let existing = edges
            .iter()
            .find(|edge| edge.target == candidate.node && edge.port == Port::Input(input));
        if existing.is_some_and(|edge| {
            edge.source != candidate.wire.source
                && !expressions.iter().any(|e| {
                    GraphNodeKey::Expression(e.id) == edge.source
                        && matches!(e.kind, MaterialExpressionKind::Constant(_))
                })
        }) {
            continue;
        }
        let mut preview = document.clone();
        let transaction = MaterialTransaction::new(
            "Insert node on wire",
            vec![
                command(asset, source, candidate.node, Port::Input(input))?,
                command(asset, inserted, candidate.wire.target, candidate.wire.port)?,
            ],
        );
        if MaterialCommandExecutor::execute(&mut preview, &transaction).is_ok() {
            valid.push(preview);
        }
    }
    if valid.len() != 1 {
        return Err(if valid.is_empty() {
            "No compatible free input; connect the sockets manually"
        } else {
            "Several inputs are compatible; connect the sockets manually"
        }
        .into());
    }
    let mut preview = valid.pop().unwrap();
    match asset {
        DocumentKey::MaterialProgram(id) => Ok(Replacement::Program(
            document
                .programs
                .iter()
                .find(|p| p.id == id)
                .unwrap()
                .clone(),
            preview
                .programs
                .remove(preview.programs.iter().position(|p| p.id == id).unwrap())
                .into(),
        )),
        DocumentKey::MaterialFunction(id) => Ok(Replacement::Function(
            preview.material_functions.remove(
                preview
                    .material_functions
                    .iter()
                    .position(|f| f.id == id)
                    .unwrap(),
            ),
        )),
        _ => unreachable!(),
    }
}

fn prepare(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    candidate: &Candidate,
) -> Result<Replacement, String> {
    if candidate.view.document.project != catalog.root() {
        return Err("Project changed".into());
    }
    match candidate.view.document.asset {
        crate::document::DocumentKey::MaterialFunction(id)
            if session.standalone_function() != Some(id) =>
        {
            return Err("Function editing target changed".into());
        }
        crate::document::DocumentKey::MaterialProgram(id)
            if session
                .standalone_material()
                .is_some_and(|active| active != id)
                || session.standalone_function().is_some() =>
        {
            return Err("Material editing target changed".into());
        }
        _ => {}
    }
    plan(&session.graph_authoring_document(catalog)?, candidate)
}

fn probe(
    event: On<widget::Probe>,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    mut state: ResMut<widget::State>,
    mut commands: Commands,
) {
    let message = event
        .0
        .as_ref()
        .map(|candidate| match prepare(&session, &catalog, candidate) {
            Ok(_) => {
                state.allowed = true;
                "Release to insert node · Alt: move only".to_string()
            }
            Err(error) => {
                state.allowed = false;
                format!("Cannot insert: {error} · Alt: move only")
            }
        });
    commands.queue(move |world: &mut World| {
        world.resource_mut::<EditorSession>().status =
            message.unwrap_or_else(|| "Move node".into());
    });
}

fn drop_node(
    event: On<widget::Drop>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut memory: ResMut<GraphViewportMemory>,
    mut history: ResMut<MaterialProgramEditHistory>,
    mut ledger: ResMut<EditorHistoryLedger>,
    functions: Option<ResMut<crate::material_function_editor::FunctionEditor>>,
) {
    let edit = &event.edit;
    // The widget has displayed the drag already. Restore its base before capturing the compound
    // transaction; failed semantic insertion therefore leaves both semantics and placement intact.
    memory.set_node(&edit.graph, &edit.node, edit.before.0, edit.before.1);
    let result = (|| {
        let replacement = prepare(&session, &catalog, &event.candidate)?;
        let before = presentation::Snapshot::capture(&edit.graph, &catalog, &session, &memory)
            .ok_or("Graph history is unavailable")?;
        match replacement {
            Replacement::Program(current, after) => {
                history.execute_replacement(
                    &mut session,
                    &mut catalog,
                    "Insert node on wire",
                    current,
                    *after,
                )?;
                ledger.record_material_edit(&mut session);
            }
            Replacement::Function(after) => functions
                .ok_or("Function editor is unavailable")?
                .edit(&mut session, &mut catalog, after)?,
        }
        memory.set_node(&edit.graph, &edit.node, edit.after.0, edit.after.1);
        before.attach(&catalog, &mut session, &mut memory);
        Ok::<_, String>(())
    })();
    if result.is_err() {
        memory.set_temporary_offset(&edit.graph, &edit.node, event.before_offset);
    }
    session.status = match result {
        Ok(()) => "Inserted node on wire".into(),
        Err(error) => format!("Insertion cancelled: {error}"),
    };
    session.ui_revision += 1;
}

#[cfg(test)]
mod tests;
