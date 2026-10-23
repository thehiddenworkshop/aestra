//! Production function socket drag/drop lifecycle with engine-native event transport.
#[cfg(test)]
#[path = "../tests/graph_connection.rs"]
mod tests;
use crate::material_function_editor::graph::{
    clipboard_context::{View, activate_function_graph_target},
    menu_state::Target,
    model::{Socket, SocketKind},
    socket_palette,
};
use crate::material_function_editor::{FunctionEditor, signature_input::finish};
use crate::material_graph::drag_input::{
    GraphDrag, PointerDrag, PointerDragDrop, PointerDragEnd, PointerDragStart,
};
use crate::{EditorSession, ProjectEffectCatalog};
use aestra_authoring::MaterialFunctionBodyCommand as Edit;
use aestra_core::MaterialExpressionId;
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(super) struct ConnectionPreview(
    pub(super) Option<(Entity, Vec2)>,
    pub(super) std::collections::BTreeSet<Entity>,
    pub(super) Option<Entity>,
);

pub(super) fn register_drag(app: &mut App) {
    app.add_observer(start_connection)
        .add_observer(move_connection)
        .add_observer(end_connection);
}
pub(super) fn register_drop(app: &mut App) {
    app.add_observer(drop_socket);
}

// Picking may target a child of the socket hit area (for example its visual dot).
// Resolve the semantic endpoint consistently for every phase of the gesture.
pub(super) fn socket_entity(
    mut entity: Entity,
    sockets: &Query<(Entity, &Socket)>,
    parents: &Query<&ChildOf>,
) -> Option<Entity> {
    loop {
        if sockets.contains(entity) {
            return Some(entity);
        }
        entity = parents.get(entity).ok()?.parent();
    }
}

pub(super) fn start_connection(
    mut event: On<PointerDragStart>,
    sockets: Query<(Entity, &Socket)>,
    mut preview: ResMut<ConnectionPreview>,
    session: Option<Res<EditorSession>>,
    catalog: Option<Res<ProjectEffectCatalog>>,
    parents: Query<&ChildOf>,
    views: Query<(), With<View>>,
) {
    if event.button == bevy::picking::pointer::PointerButton::Primary
        && let Some(entity) = socket_entity(event.entity, &sockets, &parents)
    {
        preview.0 = Some((entity, event.graph_drag_position()));
        preview.1.clear();
        preview.2 = None;
        if let (Some(session), Some(catalog), Ok((_, origin))) =
            (session, catalog, sockets.get(entity))
            && let Ok(document) = session.graph_authoring_document_for(
                &crate::material_document::MaterialEditingTarget::Function {
                    root: catalog.root().to_owned(),
                    id: origin.owner,
                },
                &catalog,
            )
        {
            for (entity, candidate) in &sockets {
                if parents
                    .iter_ancestors(origin.node)
                    .find(|id| views.contains(*id))
                    != parents
                        .iter_ancestors(candidate.node)
                        .find(|id| views.contains(*id))
                {
                    continue;
                }
                if let Some((source, target)) = socket_pair(origin, candidate) {
                    let mut candidate_document = document.clone();
                    let transaction = aestra_authoring::MaterialTransaction::new(
                        "Check connection",
                        vec![
                            aestra_authoring::MaterialCommand::EditMaterialFunctionBody {
                                function: origin.owner,
                                edit: connection_edit(source, target),
                            },
                        ],
                    );
                    if aestra_authoring::MaterialCommandExecutor::execute(
                        &mut candidate_document,
                        &transaction,
                    )
                    .is_ok()
                    {
                        preview.1.insert(entity);
                    }
                }
            }
        }
        event.propagate(false);
    }
}
pub(super) fn move_connection(
    mut event: On<PointerDrag>,
    mut preview: ResMut<ConnectionPreview>,
    sockets: Query<(Entity, &Socket)>,
    parents: Query<&ChildOf>,
) {
    if let Some((entity, cursor)) = &mut preview.0
        && Some(*entity) == socket_entity(event.entity, &sockets, &parents)
    {
        *cursor = event.graph_drag_position();
        event.propagate(false);
    }
}
#[allow(
    clippy::too_many_arguments,
    reason = "Preserve production observer dependencies and shipping lint policy"
)]
pub(super) fn end_connection(
    mut event: On<PointerDragEnd>,
    mut preview: ResMut<ConnectionPreview>,
    sockets: Query<(Entity, &Socket)>,
    parents: Query<&ChildOf>,
    geometry: Query<(Entity, &ComputedNode, &UiGlobalTransform), With<Socket>>,
    editor: Option<ResMut<FunctionEditor>>,
    session: Option<ResMut<EditorSession>>,
    catalog: Option<ResMut<ProjectEffectCatalog>>,
    mut commands: Commands,
    keys: Option<Res<ButtonInput<KeyCode>>>,
) {
    if let Some((entity, _)) = preview.0
        && Some(entity) == socket_entity(event.entity, &sockets, &parents)
    {
        if keys
            .as_ref()
            .is_some_and(|keys| keys.pressed(KeyCode::Escape))
        {
            *preview = default();
            event.propagate(false);
            return;
        }
        // Re-evaluate the release position; a fast last movement may precede PostUpdate.
        let snap = geometry
            .iter()
            .filter(|(entity, _, _)| preview.1.contains(entity))
            .filter_map(|(entity, computed, transform)| {
                let distance = (transform.translation.trunc() * computed.inverse_scale_factor)
                    .distance(event.graph_drag_position());
                (distance <= 18.0).then_some((entity, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(entity, _)| entity);
        if let (Some(target), Some(mut editor), Some(mut session), Some(mut catalog)) =
            (snap, editor, session, catalog)
            && let (Ok((_, from)), Ok((_, to))) = (sockets.get(entity), sockets.get(target))
            && activate_function_graph_target(&mut session, &catalog, from.owner)
            && let Some((source, target)) = socket_pair(from, to)
        {
            let result = editor.edit_body(
                &mut session,
                &mut catalog,
                vec![connection_edit(source, target)],
            );
            finish(&mut session, result);
        } else if snap.is_none() {
            let pointer = event.graph_drag_position();
            commands.queue(move |world: &mut World| socket_palette::open(world, entity, pointer));
        }
        *preview = ConnectionPreview::default();
        event.propagate(false);
    }
}

fn socket_pair(from: &Socket, to: &Socket) -> Option<(MaterialExpressionId, Target)> {
    if from.owner != to.owner {
        return None;
    }
    match (from.kind, to.kind) {
        (SocketKind::Source(source), SocketKind::Target(target))
        | (SocketKind::Target(target), SocketKind::Source(source)) => Some((source, target)),
        _ => None,
    }
}

pub(super) fn connection_edit(source: MaterialExpressionId, target: Target) -> Edit {
    match target {
        Target::Output(output) => Edit::SetOutput { output, source },
        Target::Input(expression, input) => Edit::Rewire {
            expression,
            input,
            source,
        },
    }
}

fn drop_socket(
    mut event: On<PointerDragDrop>,
    sockets: Query<(Entity, &Socket)>,
    parents: Query<&ChildOf>,
    mut editor: ResMut<FunctionEditor>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut preview: ResMut<ConnectionPreview>,
    views: Query<(), With<View>>,
) {
    if event.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    let (Some(source), Some(destination)) = (
        socket_entity(event.dropped, &sockets, &parents),
        socket_entity(event.entity, &sockets, &parents),
    ) else {
        return;
    };
    let (Ok((_, from)), Ok((_, to))) = (sockets.get(source), sockets.get(destination)) else {
        return;
    };
    if from.owner != to.owner || !activate_function_graph_target(&mut session, &catalog, from.owner)
    {
        return;
    }
    if parents
        .iter_ancestors(from.node)
        .find(|id| views.contains(*id))
        != parents
            .iter_ancestors(to.node)
            .find(|id| views.contains(*id))
    {
        return;
    }
    let pair = match (from.kind, to.kind) {
        (SocketKind::Source(source), SocketKind::Target(target))
        | (SocketKind::Target(target), SocketKind::Source(source)) => Some((source, target)),
        _ => None,
    };
    let Some((source, target)) = pair else {
        return;
    };
    event.propagate(false);
    *preview = ConnectionPreview::default();
    let result = editor.edit_body(
        &mut session,
        &mut catalog,
        vec![connection_edit(source, target)],
    );
    finish(&mut session, result);
}
