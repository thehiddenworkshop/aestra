//! Placement at the non-pointer semantic commit boundary. No background reconciliation:
//! failed commands, reloads and Undo never accidentally create a second layout action.
use super::*;
use crate::{
    document::DocumentKey,
    feathers::node_graph::placement::{self, Neighborhood},
};
use aestra_core::material::MaterialFunction;

struct Node {
    initial: Vec2,
    size: Vec2,
    sources: Vec<GraphNodeKey>,
    semantic: Option<MaterialExpressionKind>,
}
struct Model {
    asset: DocumentKey,
    graph: String,
    nodes: BTreeMap<GraphNodeKey, Node>,
}

impl Model {
    fn program(
        program: &MaterialProgram,
        catalog: &ProjectEffectCatalog,
        previews: &MaterialGraphPreviewState,
    ) -> Result<Self, String> {
        let projection = MaterialCompiler.project_graph_with_functions(
            program,
            None,
            &catalog.material_function_library()?,
        );
        let layout = layout_graph(&projection, previews);
        let inline = program.inline_constants();
        let semantics = program
            .expressions
            .iter()
            .map(|expression| (expression.id, expression.kind.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut nodes = projection
            .nodes
            .iter()
            .filter(|node| !inline.contains(&node.expression))
            .map(|node| {
                (
                    GraphNodeKey::Expression(node.expression),
                    Node {
                        initial: layout.nodes[&node.expression],
                        size: Vec2::new(
                            NODE_WIDTH,
                            node_height(
                                material_graph_node_row_count(node),
                                node.disabled || !node.reachable,
                                previews.is_visible(
                                    program.id,
                                    MaterialGraphPreviewTarget::Expression(node.expression),
                                ),
                            ),
                        ),
                        sources: node
                            .inputs
                            .iter()
                            .map(|port| GraphNodeKey::Expression(port.source))
                            .collect(),
                        semantic: semantics.get(&node.expression).cloned(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        nodes.insert(
            GraphNodeKey::MaterialOutputs,
            Node {
                initial: layout.output,
                size: Vec2::new(
                    NODE_WIDTH,
                    130.0
                        + if previews.is_visible(program.id, MaterialGraphPreviewTarget::Output) {
                            MATERIAL_PREVIEW_LAYOUT_HEIGHT
                        } else {
                            0.0
                        },
                ),
                sources: projection
                    .outputs
                    .iter()
                    .map(|output| GraphNodeKey::Expression(output.source))
                    .collect(),
                semantic: None,
            },
        );
        Ok(Self {
            asset: DocumentKey::MaterialProgram(program.id),
            graph: material_graph_view_key(program.id),
            nodes,
        })
    }

    fn function(
        function: &MaterialFunction,
        catalog: &ProjectEffectCatalog,
    ) -> Result<Self, String> {
        use crate::material_function_editor::graph::{bootstrap_layout, estimated_size};
        use aestra_compiler::{MaterialFunctionBodyProjection, MaterialFunctionGraphTarget};
        let function = function.normalized();
        let projection = MaterialCompiler
            .project_function_graph(&function, &catalog.material_function_library()?);
        let MaterialFunctionBodyProjection::Graph { nodes, edges } = projection.body else {
            return Err("Custom WESL has no node placement".into());
        };
        let semantics = function
            .expressions
            .iter()
            .map(|expression| (expression.id, expression.kind.clone()))
            .collect::<BTreeMap<_, _>>();
        let (positions, output, _) = bootstrap_layout(&nodes, &edges);
        let mut layout = nodes
            .iter()
            .map(|node| {
                (
                    GraphNodeKey::Expression(node.id),
                    Node {
                        initial: positions[&node.id],
                        size: estimated_size(node.id, &nodes, &edges),
                        sources: Vec::new(),
                        semantic: semantics.get(&node.id).cloned(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        layout.insert(
            GraphNodeKey::FunctionOutputs,
            Node {
                initial: output,
                size: Vec2::new(248.0, 62.0 + function.outputs.len().max(1) as f32 * 28.0),
                sources: Vec::new(),
                semantic: None,
            },
        );
        for edge in edges {
            let target = match edge.target {
                MaterialFunctionGraphTarget::Input { expression, .. }
                | MaterialFunctionGraphTarget::Argument { expression, .. } => {
                    GraphNodeKey::Expression(expression)
                }
                MaterialFunctionGraphTarget::Output(_) => GraphNodeKey::FunctionOutputs,
            };
            if let Some(node) = layout.get_mut(&target) {
                node.sources.push(GraphNodeKey::Expression(edge.source));
            }
        }
        Ok(Self {
            asset: DocumentKey::MaterialFunction(function.id),
            graph: function_graph_memory_key(catalog.root(), function.id),
            nodes: layout,
        })
    }

    fn node_key(&self, key: GraphNodeKey) -> String {
        match key {
            GraphNodeKey::Expression(id) => match self.asset {
                DocumentKey::MaterialProgram(_) => material_graph_expression_node_key(id),
                _ => id.to_string(),
            },
            GraphNodeKey::MaterialOutputs => MATERIAL_GRAPH_OUTPUT_NODE_KEY.into(),
            GraphNodeKey::FunctionOutputs => "outputs".into(),
        }
    }
}

/// Structural delta at the semantic commit boundary. A node may be both replaced and rewired when
/// its operation and its dependency set change in the same command.
#[derive(Debug, Default, PartialEq, Eq)]
struct EditImpact {
    created: BTreeSet<GraphNodeKey>,
    removed: BTreeSet<GraphNodeKey>,
    replaced: BTreeSet<GraphNodeKey>,
    rewired: BTreeSet<GraphNodeKey>,
    resized: BTreeSet<GraphNodeKey>,
}

impl EditImpact {
    fn between(before: &Model, after: &Model) -> Self {
        let mut impact = Self {
            created: after
                .nodes
                .keys()
                .filter(|key| !before.nodes.contains_key(key))
                .copied()
                .collect(),
            removed: before
                .nodes
                .keys()
                .filter(|key| !after.nodes.contains_key(key))
                .copied()
                .collect(),
            ..default()
        };
        // One sentinel strips edge identity while retaining operation type, parameters and other
        // semantic payload. This separates a replacement from a pure rewire deterministically.
        let sentinel = MaterialExpressionId::new();
        for (key, previous) in &before.nodes {
            let Some(next) = after.nodes.get(key) else {
                continue;
            };
            if previous.sources != next.sources {
                impact.rewired.insert(*key);
            }
            let previous_shape = previous
                .semantic
                .as_ref()
                .map(|kind| semantic_shape(kind, sentinel));
            let next_shape = next
                .semantic
                .as_ref()
                .map(|kind| semantic_shape(kind, sentinel));
            if previous_shape != next_shape {
                impact.replaced.insert(*key);
            }
            if previous.size != next.size {
                impact.resized.insert(*key);
            }
        }
        impact
    }

    fn changes_layout_inputs(&self) -> bool {
        !self.created.is_empty()
            || !self.removed.is_empty()
            || !self.replaced.is_empty()
            || !self.rewired.is_empty()
            || !self.resized.is_empty()
    }
}

fn semantic_shape(
    kind: &MaterialExpressionKind,
    sentinel: MaterialExpressionId,
) -> MaterialExpressionKind {
    let mut shape = kind.clone();
    let remapped = kind
        .dependencies()
        .into_iter()
        .map(|source| (source, sentinel))
        .collect();
    aestra_authoring::remap_expression_sources(&mut shape, &remapped);
    shape
}

struct Prepared {
    before: presentation::Snapshot,
    view: Option<GraphViewKey>,
    model: Model,
    area: placement::Area,
}

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Context<'w, 's> {
    memory: Option<ResMut<'w, GraphViewportMemory>>,
    previews: Option<Res<'w, MaterialGraphPreviewState>>,
    placement: placement::Context<'w, 's>,
}

impl Context<'_, '_> {
    fn prepare(
        &self,
        model: Model,
        session: &EditorSession,
        catalog: &ProjectEffectCatalog,
    ) -> Result<Option<Prepared>, String> {
        let Some(memory) = self.memory.as_deref() else {
            return Ok(None);
        };
        let before = presentation::Snapshot::capture(&model.graph, catalog, session, memory)
            .ok_or("Graph placement identity is unavailable; command was not applied")?;
        let document = GraphDocumentKey {
            project: catalog.root().to_owned(),
            asset: model.asset,
        };
        let view = self.placement.document_view(&document);
        let mut area = view
            .as_ref()
            .map(|view| self.placement.capture(view, memory))
            .unwrap_or_default();
        for (key, node) in &model.nodes {
            let position = memory
                .node_position(&model.graph, &model.node_key(*key))
                .unwrap_or(node.initial);
            area.seed(*key, position, node.size);
        }
        Ok(Some(Prepared {
            before,
            view,
            model,
            area,
        }))
    }

    fn finish(
        &mut self,
        prepared: Option<Prepared>,
        after: Model,
        session: &mut EditorSession,
        catalog: &ProjectEffectCatalog,
    ) {
        let (Some(mut prepared), Some(memory)) = (prepared, self.memory.as_deref_mut()) else {
            return;
        };
        let impact = EditImpact::between(&prepared.model, &after);
        // Every structural semantic edit freezes retained bootstrap bases. Otherwise deletion,
        // replacement or rewiring can change dependency depth and shift an unrelated node on the
        // next rebuild even when the command created no node. Never store temporary offsets.
        if impact.changes_layout_inputs() {
            if let Some(view) = &prepared.view {
                self.placement.preserve_existing(view, memory);
            }
            for (key, node) in &prepared.model.nodes {
                if !after.nodes.contains_key(key) {
                    continue;
                }
                let key = prepared.model.node_key(*key);
                if memory.node(&after.graph, &key).is_none() {
                    memory.set_node(&after.graph, key, node.initial, false);
                }
            }
        }
        let placements = place_batch(&mut prepared.area, &after, impact.created);
        let unassisted = placements.iter().any(|(_, placed)| !placed.assisted);
        for (key, placed) in placements {
            memory.place_node(&after.graph, after.node_key(key), placed.position);
        }
        prepared.before.attach(catalog, session, memory);
        if unassisted {
            session
                .status
                .push_str(&format!(" · {}", self.placement.notice()));
        }
    }

    /// Non-pointer program commands (including multi-node presets) enter here once.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn program(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
        history: &mut MaterialProgramEditHistory,
        label: &str,
        before: MaterialProgram,
        after: MaterialProgram,
    ) -> Result<(), String> {
        if before.normalized() == after.normalized() {
            return Ok(());
        }
        if before.id != after.id {
            return Err("Material command changed graph identity".into());
        }
        let no_previews = MaterialGraphPreviewState::default();
        let previews = self.previews.as_deref().unwrap_or(&no_previews);
        let previous = Model::program(&before, catalog, previews)?;
        let next = Model::program(&after, catalog, previews)?;
        validate_batch(&previous, &next)?;
        let prepared = self.prepare(previous, session, catalog)?;
        history.execute_replacement(session, catalog, label, before, after)?;
        session.status = label.into();
        self.finish(prepared, next, session, catalog);
        Ok(())
    }

    pub(crate) fn function(
        &mut self,
        session: &mut EditorSession,
        catalog: &mut ProjectEffectCatalog,
        editor: &mut crate::material_function_editor::FunctionEditor,
        after: MaterialFunction,
    ) -> Result<(), String> {
        let before = session.graph_function(catalog)?;
        if before.normalized() == after.normalized() {
            return Ok(());
        }
        if before.id != after.id {
            return Err("Function command changed graph identity".into());
        }
        let previous = Model::function(&before, catalog)?;
        let next = Model::function(&after, catalog)?;
        validate_batch(&previous, &next)?;
        let prepared = self.prepare(previous, session, catalog)?;
        editor.edit(session, catalog, after)?;
        self.finish(prepared, next, session, catalog);
        Ok(())
    }
}

fn validate_batch(before: &Model, after: &Model) -> Result<(), String> {
    if EditImpact::between(before, after).created.len() > 512 {
        return Err("Node creation exceeds the 512-node placement batch limit".into());
    }
    Ok(())
}

fn place_batch(
    area: &mut placement::Area,
    model: &Model,
    mut pending: BTreeSet<GraphNodeKey>,
) -> Vec<(GraphNodeKey, placement::Placement)> {
    let mut placed = Vec::new();
    while !pending.is_empty() {
        // Stable topological order, independent of command vector order. Invalid cycles
        // cannot reach this path after semantic validation; still keep planning bounded.
        let key = pending
            .iter()
            .find(|key| {
                model.nodes[key]
                    .sources
                    .iter()
                    .all(|source| !pending.contains(source))
            })
            .copied()
            .unwrap_or(*pending.first().unwrap());
        pending.remove(&key);
        let node = &model.nodes[&key];
        let consumer = model
            .nodes
            .iter()
            .filter(|(_, node)| node.sources.contains(&key))
            .find_map(|(key, _)| area.rect(*key).map(|rect| (*key, rect)));
        let source = node
            .sources
            .iter()
            .filter_map(|key| area.rect(*key).map(|rect| (*key, rect)))
            .max_by(|a, b| a.1.max.x.total_cmp(&b.1.max.x).then_with(|| a.0.cmp(&b.0)));
        let (preferred, neighborhood) = if let Some((target, rect)) = consumer {
            (
                Vec2::new(rect.min.x - node.size.x - 24.0, rect.min.y),
                Neighborhood::Before(target),
            )
        } else if let Some((source, rect)) = source {
            (
                Vec2::new(rect.max.x + 24.0, rect.min.y),
                Neighborhood::After(source),
            )
        } else {
            (node.initial, Neighborhood::Cursor)
        };
        placed.push((
            key,
            area.place_node(key, preferred, node.size, neighborhood),
        ));
    }
    placed
}

#[cfg(test)]
mod tests;
