//! Presentation deltas share the existing document chronology, but never shader state.
use super::*;
use crate::feathers::node_graph::{
    GraphPinEdit, GraphPresentationBatchEdit, GraphPresentationEdit,
};
use crate::history::asset_order::Context;
use crate::material_document::MaterialEditingTarget;
use std::hash::{DefaultHasher, Hash, Hasher};

type Nodes = BTreeMap<String, (Vec2, bool)>;

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct State<'w> {
    memory: Option<ResMut<'w, GraphViewportMemory>>,
    previews: Option<ResMut<'w, MaterialGraphPreviewState>>,
}
impl State<'_> {
    pub(crate) fn validate(
        &self,
        transaction: &Transaction,
        undo: bool,
        catalog: &ProjectEffectCatalog,
        session: &EditorSession,
    ) -> Result<(), String> {
        let memory = self
            .memory
            .as_deref()
            .ok_or("Graph layout is unavailable")?;
        if let Some((program, before, after)) = &transaction.previews {
            let previews = self
                .previews
                .as_deref()
                .ok_or("Graph previews are unavailable")?;
            if visible(previews, *program) != *(if undo { after } else { before }) {
                return Err("Preview visibility changed since this action".into());
            }
        }
        transaction.validate(undo, catalog, session, memory)
    }
    pub(crate) fn apply(&mut self, transaction: &Transaction, undo: bool) {
        // Canvas-less semantic hosts have no preview state. Validation only permits this for
        // transactions without a preview delta; their placement still applies normally.
        let mut no_previews = MaterialGraphPreviewState::default();
        transaction.apply(
            undo,
            self.memory.as_deref_mut().unwrap(),
            self.previews.as_deref_mut().unwrap_or(&mut no_previews),
        );
    }
}

#[derive(Clone)]
pub(crate) struct Snapshot {
    root: PathBuf,
    generation: u64,
    graph: String,
    stamp: u64,
    keys: BTreeSet<String>,
    nodes: Nodes,
    pinned: BTreeSet<String>,
    order_serial: u64,
}

#[derive(Clone)]
pub(crate) struct Transaction {
    before: Snapshot,
    after: Snapshot,
    previews: Option<(
        MaterialProgramId,
        BTreeSet<MaterialGraphPreviewTarget>,
        BTreeSet<MaterialGraphPreviewTarget>,
    )>,
    invalidated: bool,
}

impl Snapshot {
    pub(crate) fn capture(
        graph: &str,
        catalog: &ProjectEffectCatalog,
        session: &EditorSession,
        memory: &GraphViewportMemory,
    ) -> Option<Self> {
        let (stamp, keys) = semantic(graph, catalog, session)?;
        let mut nodes = memory.base_nodes(graph);
        nodes.retain(|key, _| keys.contains(key));
        let mut pinned = memory.pinned_nodes(graph);
        pinned.retain(|key| keys.contains(key));
        Some(Self {
            root: catalog.root().to_owned(),
            generation: catalog.content_revision().generation,
            graph: graph.into(),
            stamp,
            keys,
            nodes,
            pinned,
            order_serial: session.operation_order.edit_serial(),
        })
    }

    /// Attach to the command that just succeeded. No second Undo item for insertion placement.
    pub(crate) fn attach(
        self,
        catalog: &ProjectEffectCatalog,
        session: &mut EditorSession,
        memory: &mut GraphViewportMemory,
    ) {
        if let Some(transaction) = self.transaction(catalog, session, memory) {
            session
                .operation_order
                .attach_layout(Context::current(session), transaction);
        }
    }

    pub(crate) fn attach_with_previews(
        self,
        catalog: &ProjectEffectCatalog,
        session: &mut EditorSession,
        memory: &mut GraphViewportMemory,
        previews: &mut MaterialGraphPreviewState,
        program: MaterialProgramId,
    ) {
        if let Some(mut transaction) = self.transaction(catalog, session, memory) {
            let before = visible(previews, program);
            let after = before
                .iter()
                .filter(|target| match target {
                    MaterialGraphPreviewTarget::Output => true,
                    MaterialGraphPreviewTarget::Expression(id) => transaction
                        .after
                        .keys
                        .contains(&material_graph_expression_node_key(*id)),
                })
                .copied()
                .collect::<BTreeSet<_>>();
            if before != after {
                previews
                    .visible
                    .retain(|(id, target)| *id != program || after.contains(target));
                transaction.previews = Some((program, before, after));
            }
            session
                .operation_order
                .attach_layout(Context::current(session), transaction);
        }
    }

    fn transaction(
        self,
        catalog: &ProjectEffectCatalog,
        session: &EditorSession,
        memory: &mut GraphViewportMemory,
    ) -> Option<Transaction> {
        // A no-op semantic command must not overwrite an earlier history entry's layout.
        if session.operation_order.edit_serial() != self.order_serial + 1 {
            return None;
        }
        let mut after = Self::capture(&self.graph, catalog, session, memory)?;
        after.nodes.retain(|node, _| after.keys.contains(node));
        memory.retain_nodes(&self.graph, |node| after.keys.contains(node));
        Some(Transaction {
            before: self,
            after,
            previews: None,
            invalidated: false,
        })
    }

    fn is_current(&self, catalog: &ProjectEffectCatalog, session: &EditorSession) -> bool {
        self.root == catalog.root()
            && self.generation == catalog.content_revision().generation
            && semantic(&self.graph, catalog, session)
                .is_some_and(|(stamp, keys)| stamp == self.stamp && keys == self.keys)
    }
}

/// Atomically installs a validated asynchronous arrangement and records one presentation-only
/// history entry. Any semantic or manual-placement change since dispatch rejects the result.
pub(crate) fn arrange(
    world: &mut World,
    before: Snapshot,
    expected_placement_revision: u64,
    positions: BTreeMap<String, Vec2>,
    editing_target: MaterialEditingTarget,
) -> Result<(), String> {
    if !before.is_current(
        world.resource::<ProjectEffectCatalog>(),
        world.resource::<EditorSession>(),
    ) {
        return Err("Arrange result is stale: the graph changed while layout was running".into());
    }
    if positions.keys().ne(before.keys.iter())
        || positions.values().any(|position| !position.is_finite())
    {
        return Err("Arrange result does not match the current graph".into());
    }
    let graph = before.graph.clone();
    if world
        .resource::<GraphViewportMemory>()
        .placement_revision(&graph)
        != expected_placement_revision
    {
        return Err("Arrange result was ignored because node placement changed".into());
    }

    {
        let mut memory = world.resource_mut::<GraphViewportMemory>();
        for (node, position) in positions {
            let collapsed = memory
                .node(&graph, &node)
                .map(|(_, collapsed)| collapsed)
                .unwrap_or(false);
            memory.set_node(&graph, node, position, collapsed);
        }
        memory.clear_offsets(&graph);
    }
    let after = Snapshot::capture(
        &graph,
        world.resource::<ProjectEffectCatalog>(),
        world.resource::<EditorSession>(),
        world.resource::<GraphViewportMemory>(),
    )
    .ok_or("Arrange result became stale before it could be applied")?;
    if before.nodes != after.nodes || before.pinned != after.pinned {
        record_in_context(
            world,
            match editing_target {
                MaterialEditingTarget::EffectInstance => Context::Effect,
                target => Context::Material(target),
            },
            Transaction {
                before,
                after,
                previews: None,
                invalidated: false,
            },
        );
    }
    Ok(())
}

fn stamp(value: &impl std::fmt::Debug) -> u64 {
    let mut hash = DefaultHasher::new();
    format!("{value:?}").hash(&mut hash);
    hash.finish()
}

fn semantic(
    graph: &str,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Option<(u64, BTreeSet<String>)> {
    if graph.starts_with("material:") {
        let program = layout_lifecycle::programs(catalog, session)
            .into_iter()
            .find(|p| material_graph_view_key(p.id) == graph)?;
        if catalog.material_program(program.id).is_err()
            && MaterialProgram::built_in(aestra_core::material::MaterialProgramRef::BuiltIn(
                program.id,
            ))
            .is_none()
        {
            return None;
        }
        // Presentation history follows projected canvas nodes, not every semantic expression.
        // Single-use constants rendered inline on a socket have no independent geometry or base
        // position and must not make a valid whole-graph layout look incomplete.
        let inline = program.inline_constants();
        let keys = program
            .expressions
            .iter()
            .filter(|expression| !inline.contains(&expression.id))
            .map(|e| material_graph_expression_node_key(e.id))
            .chain(std::iter::once(MATERIAL_GRAPH_OUTPUT_NODE_KEY.into()))
            .collect();
        Some((stamp(&program), keys))
    } else {
        let function = catalog.material_functions().ok()?.into_iter().find(|f| {
            function_graph_memory_key(catalog.root(), f.id) == graph && f.custom_wesl.is_none()
        })?;
        catalog
            .content()
            .cached_material_function(aestra_core::material::MaterialFunctionRef::Project(
                function.id,
            ))
            .ok()?;
        let keys = function
            .expressions
            .iter()
            .map(|e| e.id.to_string())
            .chain(std::iter::once("outputs".into()))
            .collect();
        Some((stamp(&function), keys))
    }
}

impl Transaction {
    pub(crate) fn invalidate(&mut self, graph: Option<&str>) {
        if graph.is_none_or(|graph| self.before.graph == graph) {
            self.invalidated = true;
        }
    }

    /// Validate before touching semantics or presentation. Removed identities are only restored
    /// by a compound command whose semantic history restores them first.
    pub(crate) fn validate(
        &self,
        undo: bool,
        catalog: &ProjectEffectCatalog,
        session: &EditorSession,
        memory: &GraphViewportMemory,
    ) -> Result<(), String> {
        let expected = if undo { &self.after } else { &self.before };
        if self.invalidated
            || expected.root != catalog.root()
            || expected.generation != catalog.content_revision().generation
            || semantic(&expected.graph, catalog, session)
                .is_none_or(|(stamp, _)| stamp != expected.stamp)
        {
            return Err("Layout history is stale: the graph was removed or reloaded".into());
        }
        for key in self.changed_nodes() {
            if self
                .before
                .nodes
                .get(&key)
                .into_iter()
                .chain(self.after.nodes.get(&key))
                .any(|(position, _)| !position.is_finite())
            {
                return Err("Invalid layout position".into());
            }
            if memory.node(&expected.graph, &key) != expected.nodes.get(&key).copied() {
                return Err("Layout changed since this action; history was not applied".into());
            }
            if memory.is_pinned(&expected.graph, &key) != expected.pinned.contains(&key) {
                return Err(
                    "Layout pinning changed since this action; history was not applied".into(),
                );
            }
        }
        Ok(())
    }

    fn changed_nodes(&self) -> BTreeSet<String> {
        self.before
            .nodes
            .keys()
            .chain(self.after.nodes.keys())
            .filter(|key| self.before.nodes.get(*key) != self.after.nodes.get(*key))
            .chain(self.before.pinned.symmetric_difference(&self.after.pinned))
            .cloned()
            .collect()
    }

    pub(crate) fn apply(
        &self,
        undo: bool,
        memory: &mut GraphViewportMemory,
        previews: &mut MaterialGraphPreviewState,
    ) {
        let target = if undo { &self.before } else { &self.after };
        for key in self.changed_nodes() {
            if let Some((position, collapsed)) = target.nodes.get(&key) {
                memory.set_node(&target.graph, &key, *position, *collapsed);
                memory.set_pinned(&target.graph, &key, target.pinned.contains(&key));
            } else {
                memory.remove_node(&target.graph, &key);
            }
        }
        memory.clear_offsets(&target.graph);
        if let Some((program, before, after)) = &self.previews {
            previews.visible.retain(|(id, _)| id != program);
            previews.visible.extend(
                (if undo { before } else { after })
                    .iter()
                    .map(|target| (*program, *target)),
            );
        }
    }
}

pub(super) fn pin_edit(event: On<GraphPinEdit>, mut commands: Commands) {
    let edit = event.event().clone();
    commands.queue(move |world: &mut World| {
        let Some(mut after) = Snapshot::capture(
            &edit.graph,
            world.resource::<ProjectEffectCatalog>(),
            world.resource::<EditorSession>(),
            world.resource::<GraphViewportMemory>(),
        ) else {
            return;
        };
        if !after.keys.contains(&edit.node) || edit.before == edit.after {
            return;
        }
        if edit.after {
            after.pinned.insert(edit.node.clone());
        } else {
            after.pinned.remove(&edit.node);
        }
        let mut before = after.clone();
        if edit.before {
            before.pinned.insert(edit.node);
        } else {
            before.pinned.remove(&edit.node);
        }
        record(
            world,
            Transaction {
                before,
                after,
                previews: None,
                invalidated: false,
            },
            edit.origin,
        );
    });
}

fn context(graph: &str, catalog: &ProjectEffectCatalog, session: &EditorSession) -> Context {
    if let Some(id) = session.standalone_function()
        && let Ok(functions) = catalog.material_functions()
        && let Some(function) = functions
            .iter()
            .find(|f| f.id == id && function_graph_memory_key(catalog.root(), f.id) == graph)
    {
        return Context::Material(MaterialEditingTarget::Function {
            root: catalog.root().to_owned(),
            id: function.id,
        });
    }
    if let Some(id) = session.standalone_material()
        && material_graph_view_key(id) == graph
        && catalog.material_program(id).is_ok()
    {
        Context::Material(MaterialEditingTarget::Program {
            root: catalog.root().to_owned(),
            id,
        })
    } else {
        Context::Effect
    }
}

fn context_for_origin(world: &World, mut origin: Entity) -> Option<Context> {
    loop {
        if let Some(marker) = world.get::<super::asset_drop::GraphDropTarget>(origin) {
            return Some(match marker.editing_target() {
                MaterialEditingTarget::EffectInstance => Context::Effect,
                target => Context::Material(target.clone()),
            });
        }
        origin = world.get::<ChildOf>(origin)?.parent();
    }
}

fn record(world: &mut World, transaction: Transaction, origin: Option<Entity>) {
    let context = origin
        .and_then(|entity| context_for_origin(world, entity))
        .unwrap_or_else(|| {
            context(
                &transaction.before.graph,
                world.resource::<ProjectEffectCatalog>(),
                world.resource::<EditorSession>(),
            )
        });
    record_in_context(world, context, transaction);
}

fn record_in_context(world: &mut World, context: Context, transaction: Transaction) {
    // Fork the existing redo branch as well as the presentation branch. Layout does not execute
    // a material command, install compiled data, or change a document content revision.
    crate::history::clear_presentation_redo(world, &context);
    let mut session = world.resource_mut::<EditorSession>();
    context.select(&mut session);
    session.operation_order.record_layout(context, transaction);
    session.status = "Changed graph layout".into();
}

pub(super) fn node_edit(event: On<GraphPresentationEdit>, mut commands: Commands) {
    let edit = event.event().clone();
    commands.queue(move |world: &mut World| {
        let Some(mut after) = Snapshot::capture(
            &edit.graph,
            world.resource::<ProjectEffectCatalog>(),
            world.resource::<EditorSession>(),
            world.resource::<GraphViewportMemory>(),
        ) else {
            return;
        };
        if !after.keys.contains(&edit.node) || edit.before == edit.after {
            return;
        }
        after.nodes.insert(edit.node.clone(), edit.after);
        let mut before = after.clone();
        before.nodes.insert(edit.node, edit.before);
        record(
            world,
            Transaction {
                before,
                after,
                previews: None,
                invalidated: false,
            },
            edit.origin,
        );
    });
}

pub(super) fn batch_edit(event: On<GraphPresentationBatchEdit>, mut commands: Commands) {
    let edit = event.event().clone();
    commands.queue(move |world: &mut World| {
        let Some(after) = Snapshot::capture(
            &edit.graph,
            world.resource::<ProjectEffectCatalog>(),
            world.resource::<EditorSession>(),
            world.resource::<GraphViewportMemory>(),
        ) else {
            return;
        };
        let mut before = after.clone();
        for (node, state) in edit.before {
            if before.keys.contains(&node) {
                before.nodes.insert(node, state);
            }
        }
        if before.nodes != after.nodes {
            record(
                world,
                Transaction {
                    before,
                    after,
                    previews: None,
                    invalidated: false,
                },
                edit.origin,
            );
        }
    });
}

pub(super) fn preview_edit(
    commands: &mut Commands,
    program: MaterialProgramId,
    editing_target: MaterialEditingTarget,
    before: BTreeSet<MaterialGraphPreviewTarget>,
    after: BTreeSet<MaterialGraphPreviewTarget>,
) {
    if before == after {
        return;
    }
    commands.queue(move |world: &mut World| {
        let Some(snapshot) = Snapshot::capture(
            &material_graph_view_key(program),
            world.resource::<ProjectEffectCatalog>(),
            world.resource::<EditorSession>(),
            world.resource::<GraphViewportMemory>(),
        ) else {
            return;
        };
        record_in_context(
            world,
            match editing_target {
                MaterialEditingTarget::EffectInstance => Context::Effect,
                target => Context::Material(target),
            },
            Transaction {
                before: snapshot.clone(),
                after: snapshot,
                previews: Some((program, before, after)),
                invalidated: false,
            },
        );
    });
}

pub(super) fn visible(
    previews: &MaterialGraphPreviewState,
    program: MaterialProgramId,
) -> BTreeSet<MaterialGraphPreviewTarget> {
    previews
        .visible
        .iter()
        .filter(|(id, _)| *id == program)
        .map(|(_, target)| *target)
        .collect()
}

#[cfg(test)]
mod tests;
