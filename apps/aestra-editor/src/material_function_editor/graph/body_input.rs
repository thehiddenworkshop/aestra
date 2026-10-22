//! Production function-body action and constant-edit observers.
#[cfg(test)]
#[path = "body_input/tests.rs"]
mod tests;
use crate::feathers::node_graph::{
    GraphViewportMemory, NODE_HEADER_HEIGHT, NODE_WIDTH,
    geometry::{GraphDocumentKey, GraphViewKey},
    placement,
};
use crate::material_function_editor::graph::{
    clipboard_context::activate_function_graph_target,
    estimated_size,
    model::{BodyAction, BodyActionKind, Constant},
};
use crate::material_function_editor::{FunctionEditor, signature_input::finish};
use crate::{EditorSession, ProjectEffectCatalog};
use aestra_authoring::MaterialFunctionBodyCommand as Edit;
use aestra_compiler::{MaterialCompiler, MaterialFunctionBodyProjection};
use aestra_core::{
    MaterialExpressionId,
    material::{MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialValue},
};
use bevy::{
    prelude::*,
    ui_widgets::{Activate, ValueChange},
};

pub(super) fn register_actions(app: &mut App) {
    app.add_observer(action);
}
pub(super) fn register_constants(app: &mut App) {
    app.add_observer(constant_text)
        .add_observer(constant_number);
}

pub(super) fn create_edits(
    function: &MaterialFunction,
    action: BodyActionKind,
    library: &aestra_compiler::MaterialFunctionLibrary,
) -> Result<Vec<Edit>, String> {
    if let BodyActionKind::Remove(expression) = action {
        return Ok(vec![Edit::Remove { expression }]);
    }
    if let BodyActionKind::Boolean(expression, value) = action {
        return Ok(vec![Edit::Replace {
            expression,
            replacement: MaterialExpression {
                id: expression,
                kind: MaterialExpressionKind::Constant(MaterialValue::Bool(value)),
            },
        }]);
    }
    let expressions = match action {
        BodyActionKind::Create(kind) => MaterialCompiler
            .function_graph_node_expressions(function, kind, library)
            .map_err(|error| error.to_string())?,
        BodyActionKind::Input(id) => vec![MaterialExpression {
            id: MaterialExpressionId::new(),
            kind: MaterialExpressionKind::FunctionInput(id),
        }],
        _ => return Err("Not a creation action".into()),
    };
    Ok(expressions
        .into_iter()
        .enumerate()
        .map(|(index, expression)| Edit::Add {
            expression,
            index: function.expressions.len() + index,
        })
        .collect())
}

#[allow(
    clippy::too_many_arguments,
    reason = "Preserve the production observer dependencies and shipping lint policy"
)]
fn action(
    event: On<Activate>,
    actions: Query<&BodyAction>,
    selection: Res<crate::material_graph::MaterialGraphSelectionState>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut commands: Commands,
    mut memory: Option<ResMut<GraphViewportMemory>>,
    placement: placement::Context,
) {
    let Ok(action) = actions.get(event.entity) else {
        return;
    };
    if !activate_function_graph_target(&mut session, &catalog, action.owner) {
        return;
    }
    if matches!(action.kind, BodyActionKind::Back) {
        session.return_to_effect_material();
        return;
    }
    if matches!(action.kind, BodyActionKind::Locate) {
        commands.trigger(crate::asset_browser::LocateInAssets(
            aestra_project::ProjectAssetId::MaterialFunction(action.owner),
        ));
        return;
    }
    if let BodyActionKind::Arrange(arrange_scope) = action.kind {
        commands.trigger(crate::material_graph::arrange::ArrangeGraph {
            view: GraphViewKey {
                document: GraphDocumentKey {
                    project: catalog.root().to_owned(),
                    asset: crate::document::DocumentKey::MaterialFunction(action.owner),
                },
                view: action.scope,
            },
            editing_target: crate::material_document::MaterialEditingTarget::Function {
                root: catalog.root().to_owned(),
                id: action.owner,
            },
            scope: arrange_scope,
            seeds: selection.function_arrange_seeds(action.scope, action.owner),
        });
        return;
    }
    let graph_key = format!("function:{}:{}", catalog.root().display(), action.owner);
    let view_key = GraphViewKey {
        document: GraphDocumentKey {
            project: catalog.root().to_owned(),
            asset: crate::document::DocumentKey::MaterialFunction(action.owner),
        },
        view: action.scope,
    };
    let mut area = memory
        .as_deref()
        .map(|memory| placement.capture(&view_key, memory))
        .unwrap_or_default();
    let preferred = placement
        .center(&view_key)
        .map(|center| center - Vec2::new(NODE_WIDTH * 0.5, NODE_HEADER_HEIGHT * 0.5))
        .unwrap_or(Vec2::new(34.0, 68.0));
    let layout_before = memory.as_deref().and_then(|memory| {
        crate::material_graph::presentation::Snapshot::capture(
            &graph_key, &catalog, &session, memory,
        )
    });
    let mut created = Vec::new();
    let result = session.graph_function(&catalog).and_then(|function| {
        let library = catalog
            .material_function_library()
            .map_err(|error| error.to_string())?;
        let edits = create_edits(&function, action.kind, &library)?;
        created = edits
            .iter()
            .filter_map(|edit| match edit {
                Edit::Add { expression, .. } => Some(expression.id),
                _ => None,
            })
            .collect();
        editor.edit_body(&mut session, &mut catalog, edits)
    });
    if result.is_ok()
        && !created.is_empty()
        && let Some(memory) = memory.as_deref_mut()
        && let Ok(function) = session.graph_function(&catalog)
        && let Ok(library) = catalog.material_function_library()
        && let MaterialFunctionBodyProjection::Graph { nodes, edges } = MaterialCompiler
            .project_function_graph(&function, &library)
            .body
    {
        let mut unassisted = false;
        placement.preserve_existing(&view_key, memory);
        for id in created {
            let placed = area.place(
                preferred,
                estimated_size(id, &nodes, &edges),
                placement::Neighborhood::Cursor,
            );
            memory.place_node(&graph_key, id.to_string(), placed.position);
            unassisted |= !placed.assisted;
        }
        if unassisted {
            session
                .status
                .push_str(&format!(" · {}", placement.notice()));
        }
    }
    if result.is_ok()
        && let Some(before) = layout_before
        && let Some(memory) = memory.as_deref_mut()
    {
        before.attach(&catalog, &mut session, memory);
    }
    finish(&mut session, result);
}

fn replace_constant(
    control: &Constant,
    value: f32,
    editor: &mut FunctionEditor,
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
) {
    if !activate_function_graph_target(session, catalog, control.owner) {
        return;
    }
    let result = session.graph_function(catalog).and_then(|function| {
        let mut replacement = function
            .expressions
            .iter()
            .find(|expression| expression.id == control.expression)
            .cloned()
            .ok_or("Constant is no longer available")?;
        let MaterialExpressionKind::Constant(literal) = &mut replacement.kind else {
            return Err("Expression is no longer a constant".into());
        };
        set_component(literal, control.component, value)?;
        editor.edit_body(
            session,
            catalog,
            vec![Edit::Replace {
                expression: control.expression,
                replacement,
            }],
        )
    });
    finish(session, result);
}

pub(super) fn set_component(
    literal: &mut MaterialValue,
    component: usize,
    value: f32,
) -> Result<(), String> {
    if !value.is_finite() {
        return Err("Constant must be finite".into());
    }
    let values: &mut [f32] = match literal {
        MaterialValue::Float(v) => std::slice::from_mut(v),
        MaterialValue::Vec2(v) => v,
        MaterialValue::Vec3(v) => v,
        MaterialValue::Vec4(v) | MaterialValue::ColorSrgb(v) => v,
        MaterialValue::Bool(v) if component == 0 && (value == 0.0 || value == 1.0) => {
            *v = value == 1.0;
            return Ok(());
        }
        _ => return Err("Unsupported constant component".into()),
    };
    *values
        .get_mut(component)
        .ok_or("Missing constant component")? = value;
    Ok(())
}
fn constant_text(
    change: On<ValueChange<String>>,
    controls: Query<&Constant>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    match change.value.trim().parse::<f32>() {
        Ok(value) => replace_constant(control, value, &mut editor, &mut session, &mut catalog),
        Err(_) => finish(&mut session, Err("Enter a numeric constant".into())),
    }
}
fn constant_number(
    change: On<ValueChange<f32>>,
    controls: Query<&Constant>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    replace_constant(
        control,
        change.value,
        &mut editor,
        &mut session,
        &mut catalog,
    );
}
