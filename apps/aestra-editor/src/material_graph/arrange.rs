//! Bounded asynchronous full and targeted arrangement with stale-result rejection.

use super::layout_adapter::LayoutAdapter;
use super::*;
use crate::document::{DocumentId, DocumentKey};
use crate::feathers::{
    graph_layout::{
        GraphLayoutEngine, GraphLayoutError, GraphLayoutNodeState, GraphLayoutRegion,
        GraphLayoutResult,
        native::AestraLayeredLayout,
        partial::{PartialLayoutPlan, PartialLayoutScope},
    },
    node_graph::geometry::{GraphGeometryRegistry, GraphGeometryView},
};
use bevy::tasks::{AsyncComputeTaskPool, Task, futures_lite::future};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArrangeScope {
    Graph,
    Selection,
    Upstream,
    Downstream,
}

impl ArrangeScope {
    fn label(self) -> &'static str {
        match self {
            Self::Graph => "Arrange Graph",
            Self::Selection => "Arrange Selection",
            Self::Upstream => "Arrange Upstream",
            Self::Downstream => "Arrange Downstream",
        }
    }

    fn partial(self) -> Option<PartialLayoutScope> {
        match self {
            Self::Graph => None,
            Self::Selection => Some(PartialLayoutScope::Selection),
            Self::Upstream => Some(PartialLayoutScope::Upstream),
            Self::Downstream => Some(PartialLayoutScope::Downstream),
        }
    }
}

#[derive(Event, Debug, Clone)]
pub(crate) struct ArrangeGraph {
    pub view: GraphViewKey,
    pub scope: ArrangeScope,
    pub seeds: BTreeSet<GraphNodeKey>,
}

impl ArrangeGraph {
    pub(crate) fn full(view: GraphViewKey) -> Self {
        Self {
            view,
            scope: ArrangeScope::Graph,
            seeds: BTreeSet::new(),
        }
    }
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
    scope: ArrangeScope,
    task: Task<Result<GraphLayoutResult, GraphLayoutError>>,
}

enum LayoutWork {
    Full(crate::feathers::graph_layout::GraphLayoutInput),
    Partial(PartialLayoutPlan),
}

impl LayoutWork {
    fn run(self) -> Result<GraphLayoutResult, GraphLayoutError> {
        match self {
            Self::Full(input) => AestraLayeredLayout.layout(&input),
            Self::Partial(plan) => plan.layout(&AestraLayeredLayout),
        }
    }
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
        session.status = "A graph arrangement is already running".into();
        return;
    }
    let scope = event.event().scope;
    let result = prepare(
        event.event().view.clone(),
        scope,
        &event.event().seeds,
        &catalog,
        &session,
        &memory,
        &registry,
        &views,
    );
    let (view, viewport, snapshot, graph, adapter, before, work) = match result {
        Ok(prepared) => prepared,
        Err(error) => {
            session.status = format!("{} unavailable: {error}", scope.label());
            return;
        }
    };
    let task = AsyncComputeTaskPool::get().spawn(async move { work.run() });
    state.job = Some(ArrangeJob {
        view,
        viewport,
        document_generation: snapshot.document_generation,
        geometry_revision: snapshot.geometry_revision,
        placement_revision: memory.placement_revision(&graph),
        graph,
        adapter,
        before,
        scope,
        task,
    });
    session.status = format!("{}…", scope.label());
}

type Prepared = (
    GraphViewKey,
    Entity,
    crate::feathers::node_graph::geometry::GraphGeometrySnapshot,
    String,
    LayoutAdapter,
    presentation::Snapshot,
    LayoutWork,
);

fn prepare(
    view: GraphViewKey,
    scope: ArrangeScope,
    seeds: &BTreeSet<GraphNodeKey>,
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
                    selected: seeds.contains(key),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let (graph, mut adapter) = match view.document.asset {
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
    let work = match scope.partial() {
        None => LayoutWork::Full(adapter.input.clone()),
        Some(partial_scope) => {
            if seeds.is_empty() {
                return Err("select one or more graph nodes first".into());
            }
            let layout_seeds = seeds
                .iter()
                .map(|seed| {
                    adapter
                        .layout_key(*seed)
                        .ok_or("the selection contains a node outside the current graph")
                })
                .collect::<Result<BTreeSet<_>, _>>()?;
            let plan = PartialLayoutPlan::extract(&adapter.input, &layout_seeds, partial_scope)
                .map_err(|error| error.to_string())?;
            adapter.input.region = GraphLayoutRegion::Nodes(plan.region.clone());
            LayoutWork::Partial(plan)
        }
    };
    let before = presentation::Snapshot::capture(&graph, catalog, session, memory)
        .ok_or("the graph presentation is unavailable")?;
    Ok((view, viewport, snapshot, graph, adapter, before, work))
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
            session.status = format!("{} failed: {error}", job.scope.label());
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
            session.status = format!("{} failed: {error}", job.scope.label());
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
