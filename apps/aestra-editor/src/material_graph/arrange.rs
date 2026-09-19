//! Bounded asynchronous whole-graph arrangement with stale-result rejection.

use super::layout_adapter::LayoutAdapter;
use super::*;
use crate::document::{DocumentId, DocumentKey};
use crate::feathers::{
    graph_layout::{
        ElkLayeredLayout, GraphLayoutEngine, GraphLayoutError, GraphLayoutNodeState,
        GraphLayoutResult,
    },
    node_graph::geometry::{GraphGeometryRegistry, GraphGeometryView},
};
use bevy::tasks::{AsyncComputeTaskPool, Task, futures_lite::future};

#[derive(Event, Debug, Clone)]
pub(crate) struct ArrangeGraph {
    pub view: GraphViewKey,
}

#[derive(Resource, Default)]
struct ArrangeState {
    job: Option<ArrangeJob>,
}

struct ArrangeJob {
    view: GraphViewKey,
    viewport: Entity,
    document_generation: Option<DocumentId>,
    geometry_revision: u64,
    graph: String,
    placement_revision: u64,
    adapter: LayoutAdapter,
    before: presentation::Snapshot,
    task: Task<Result<GraphLayoutResult, GraphLayoutError>>,
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<ArrangeState>()
        .add_observer(request)
        .add_systems(Update, poll);
}

fn request(
    event: On<ArrangeGraph>,
    mut state: ResMut<ArrangeState>,
    catalog: Res<ProjectEffectCatalog>,
    mut session: ResMut<EditorSession>,
    memory: Res<GraphViewportMemory>,
    registry: Res<GraphGeometryRegistry>,
    views: Query<(Entity, &GraphGeometryView)>,
) {
    if state.job.is_some() {
        session.status = "Arrange Graph is already running".into();
        return;
    }
    let result = prepare(
        event.event().view.clone(),
        &catalog,
        &session,
        &memory,
        &registry,
        &views,
    );
    let (view, viewport, snapshot, graph, adapter, before) = match result {
        Ok(prepared) => prepared,
        Err(error) => {
            session.status = format!("Arrange Graph unavailable: {error}");
            return;
        }
    };
    let input = adapter.input.clone();
    let task = AsyncComputeTaskPool::get().spawn(async move { ElkLayeredLayout.layout(&input) });
    state.job = Some(ArrangeJob {
        view,
        viewport,
        document_generation: snapshot.document_generation,
        geometry_revision: snapshot.geometry_revision,
        placement_revision: memory.placement_revision(&graph),
        graph,
        adapter,
        before,
        task,
    });
    session.status = "Arranging graph…".into();
}

type Prepared = (
    GraphViewKey,
    Entity,
    crate::feathers::node_graph::geometry::GraphGeometrySnapshot,
    String,
    LayoutAdapter,
    presentation::Snapshot,
);

fn prepare(
    view: GraphViewKey,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
    memory: &GraphViewportMemory,
    registry: &GraphGeometryRegistry,
    views: &Query<(Entity, &GraphGeometryView)>,
) -> Result<Prepared, String> {
    if view.document.project != catalog.root() {
        return Err("the graph belongs to another project".into());
    }
    let viewport = views
        .iter()
        .find_map(|(entity, marker)| (marker.key == view).then_some(entity))
        .ok_or("the graph view is no longer mounted")?;
    let snapshot = registry
        .mounted_view_snapshot(&view, viewport)
        .cloned()
        .ok_or("node measurements are not stable yet")?;
    let states = snapshot
        .nodes
        .iter()
        .map(|(key, node)| {
            (
                *key,
                GraphLayoutNodeState {
                    position: node.effective_position,
                    size: node.size,
                    pinned: false,
                    selected: false,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let (graph, adapter) = match view.document.asset {
        DocumentKey::MaterialProgram(id) => {
            let document = session.graph_authoring_document(catalog)?;
            let program = document
                .programs
                .iter()
                .find(|program| program.id == id)
                .ok_or("the material is no longer available in this document")?;
            let functions = document.material_function_library();
            let compiler = MaterialCompiler;
            let ir = compiler.compile_with_functions(program, &functions).ok();
            let projection =
                compiler.project_graph_with_functions(program, ir.as_ref(), &functions);
            (
                material_graph_view_key(id),
                LayoutAdapter::program(&projection, &states, None)
                    .map_err(|error| error.to_string())?,
            )
        }
        DocumentKey::MaterialFunction(id) => {
            let target = crate::material_document::MaterialEditingTarget::Function {
                root: catalog.root().to_owned(),
                id,
            };
            let function = session.graph_function_for(&target, catalog)?;
            let library = catalog.material_function_library()?;
            let projection = MaterialCompiler.project_function_graph(&function, &library);
            (
                function_graph_memory_key(catalog.root(), id),
                LayoutAdapter::function(&projection, &states, None)
                    .map_err(|error| error.to_string())?,
            )
        }
        DocumentKey::WeslSource(_) => return Err("WESL documents do not have a node graph".into()),
    };
    let before = presentation::Snapshot::capture(&graph, catalog, session, memory)
        .ok_or("the graph presentation is unavailable")?;
    Ok((view, viewport, snapshot, graph, adapter, before))
}

fn poll(
    mut commands: Commands,
    mut state: ResMut<ArrangeState>,
    registry: Res<GraphGeometryRegistry>,
    memory: Res<GraphViewportMemory>,
    mut session: ResMut<EditorSession>,
) {
    let Some(job) = state.job.as_mut() else {
        return;
    };
    let Some(result) = future::block_on(future::poll_once(&mut job.task)) else {
        return;
    };
    let job = state.job.take().unwrap();
    let current = registry.mounted_view_snapshot(&job.view, job.viewport);
    if current.is_none_or(|snapshot| {
        snapshot.geometry_revision != job.geometry_revision
            || snapshot.document_generation != job.document_generation
    }) || memory.placement_revision(&job.graph) != job.placement_revision
    {
        session.status = "Arrange result was ignored because the graph changed".into();
        return;
    }
    let result = result.and_then(|result| job.adapter.resolve(&result));
    let semantic_positions = match result {
        Ok(result) => result,
        Err(error) => {
            session.status = format!("Arrange Graph failed: {error}");
            return;
        }
    };
    let asset = job.view.document.asset;
    let positions = semantic_positions
        .into_iter()
        .map(|(key, position)| memory_key(asset, key).map(|key| (key, position)))
        .collect::<Result<BTreeMap<_, _>, _>>();
    let positions = match positions {
        Ok(positions) => positions,
        Err(error) => {
            session.status = format!("Arrange Graph failed: {error}");
            return;
        }
    };
    commands.queue(move |world: &mut World| {
        match presentation::arrange(world, job.before, job.placement_revision, positions) {
            Ok(()) => {
                world.resource_mut::<EditorSession>().ui_revision += 1;
            }
            Err(error) => {
                world.resource_mut::<EditorSession>().status = error;
            }
        }
    });
}

fn memory_key(asset: DocumentKey, node: GraphNodeKey) -> Result<String, &'static str> {
    match (asset, node) {
        (DocumentKey::MaterialProgram(_), GraphNodeKey::Expression(id)) => {
            Ok(material_graph_expression_node_key(id))
        }
        (DocumentKey::MaterialProgram(_), GraphNodeKey::MaterialOutputs) => {
            Ok(MATERIAL_GRAPH_OUTPUT_NODE_KEY.into())
        }
        (DocumentKey::MaterialFunction(_), GraphNodeKey::Expression(id)) => Ok(id.to_string()),
        (DocumentKey::MaterialFunction(_), GraphNodeKey::FunctionOutputs) => Ok("outputs".into()),
        _ => Err("layout node kind does not match the graph document"),
    }
}
