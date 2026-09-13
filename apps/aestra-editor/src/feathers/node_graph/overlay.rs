//! Authoritative stable measurements -> session overlays -> next UI layout.
use super::{geometry::*, *};
use std::collections::{BTreeMap, BTreeSet};

mod model;

#[derive(Default)]
struct DocumentOverlay {
    graph: String,
    generation: Option<crate::document::DocumentId>,
    model: model::Overlay,
    last: Option<(BTreeMap<String, model::Input>, u64)>,
    conflict: Option<model::Conflict>,
}

#[derive(Component)]
pub(super) struct ConflictNotice;

pub(super) fn sync_notice(
    mut commands: Commands,
    overlays: Res<GraphOverlays>,
    views: Query<(Entity, &GraphGeometryView)>,
    notices: Query<(Entity, &ChildOf), With<ConflictNotice>>,
    localizer: Option<Res<crate::Localizer>>,
) {
    let Some(localizer) = localizer else {
        return;
    };
    for (entity, view) in &views {
        let existing = notices.iter().find(|(_, parent)| parent.parent() == entity);
        let conflict = overlays
            .documents
            .get(&view.key.document)
            .is_some_and(|state| state.conflict.is_some());
        if let Some((notice, _)) = existing {
            if !conflict || localizer.is_changed() {
                commands.entity(notice).despawn();
            }
            if !localizer.is_changed() {
                continue;
            }
        }
        if conflict {
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    ConflictNotice,
                    Text::new(localizer.text("graph-layout-overlap-conflict")),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                    BackgroundColor(theme::PANEL_LIGHT),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(8.0),
                        bottom: Val::Px(8.0),
                        max_width: Val::Percent(90.0),
                        padding: UiRect::all(Val::Px(6.0)),
                        ..default()
                    },
                    ZIndex(40),
                    Pickable::IGNORE,
                ));
            });
        }
    }
}

#[derive(Resource, Default)]
pub(super) struct GraphOverlays {
    project: Option<std::path::PathBuf>,
    documents: BTreeMap<GraphDocumentKey, DocumentOverlay>,
}

pub(super) fn reconcile(
    mut overlays: ResMut<GraphOverlays>,
    registry: Res<GraphGeometryRegistry>,
    mut memory: ResMut<GraphViewportMemory>,
    views: Query<(Entity, &GraphGeometryView)>,
    nodes: Query<(Entity, &GraphGeometryNode, &FeathersGraphNode)>,
    parents: Query<&ChildOf>,
    catalog: Option<Res<crate::ProjectEffectCatalog>>,
) {
    let project = catalog
        .as_ref()
        .map(|catalog| catalog.root().to_owned())
        .or_else(|| {
            registry
                .snapshots()
                .next()
                .map(|snapshot| snapshot.measured_in.document.project.clone())
        });
    if let Some(project) = project
        && overlays.project.as_ref() != Some(&project)
    {
        overlays.documents.clear();
        overlays.project = Some(project);
    }
    // Removed/project-switched memory cannot keep a displacement journal alive. Closed
    // tabs retain their memory and can safely reopen with the same session overlay.
    overlays
        .documents
        .retain(|_, state| memory.nodes.keys().any(|(graph, _)| graph == &state.graph));
    for snapshot in registry.snapshots() {
        let Some((entity, view)) = views.iter().find(|(id, view)| {
            view.key == snapshot.measured_in
                && registry.mounted_view_snapshot(&view.key, *id).is_some()
        }) else {
            continue;
        };
        let live = nodes
            .iter()
            .filter(|(id, _, _)| parents.iter_ancestors(*id).any(|parent| parent == entity))
            .collect::<Vec<_>>();
        if live.len() != snapshot.nodes.len() || live.iter().any(|(_, _, node)| node.dragging) {
            continue;
        }
        let Some((_, _, first)) = live.first() else {
            continue;
        };
        let graph = first.graph_key.clone();
        // Rebuilds and early-frame input may have made last frame's geometry stale.
        if live.iter().any(|(_, marker, node)| {
            snapshot.nodes.get(&marker.key).is_none_or(|measured| {
                node.graph_key != graph
                    || node.collapsed != measured.collapsed
                    || marker.is_preview() != measured.preview
                    || marker.content_stamp() != measured.content
                    || memory
                        .node_position(&graph, &node.node_key)
                        .unwrap_or(node.position)
                        != measured.effective_position
            })
        }) {
            continue;
        }
        let mapping = live
            .iter()
            .map(|(_, marker, node)| (marker.key, node.node_key.clone()))
            .collect::<BTreeMap<_, _>>();
        // Bootstrap placements are authored bases, not inferred from already-offset UI.
        for (_, _, node) in &live {
            if memory.node(&graph, &node.node_key).is_none() {
                memory.set_node(&graph, &node.node_key, node.position, node.collapsed);
            }
        }
        let epoch = memory
            .offset_epochs
            .get(&graph)
            .copied()
            .unwrap_or_default();
        let input = snapshot
            .nodes
            .iter()
            .map(|(key, measured)| {
                let name = &mapping[key];
                (
                    name.clone(),
                    model::Input {
                        base: memory.node(&graph, name).unwrap().0,
                        size: measured.size,
                        compact: measured.compact_size,
                        collapsed: measured.collapsed,
                        revision: memory
                            .placement_revisions
                            .get(&(graph.clone(), name.clone()))
                            .copied()
                            .unwrap_or_default(),
                        frozen: false,
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();
        let state = overlays
            .documents
            .entry(view.key.document.clone())
            .or_default();
        if state.generation != snapshot.document_generation {
            *state = DocumentOverlay::default();
            state.generation = snapshot.document_generation;
        }
        state.graph = graph.clone();
        // A handoff only changes the measurement owner, never a resize cause. Keep
        // the current offsets; the next explicit mode/content change uses new geometry.
        let content = registry
            .changes()
            .iter()
            .filter_map(|event| {
                if event.view != view.key || event.geometry_revision != snapshot.geometry_revision {
                    return None;
                }
                match event.change {
                    GraphGeometryChange::Resized {
                        node,
                        reason: GraphResizeReason::ContentChanged,
                        ..
                    } => mapping.get(&node).cloned(),
                    _ => None,
                }
            })
            .collect::<BTreeSet<_>>();
        if content.is_empty()
            && state.last.as_ref().is_some_and(|(old, old_epoch)| {
                *old_epoch == epoch
                    && old.len() == input.len()
                    && old.iter().all(|(key, old)| {
                        input.get(key).is_some_and(|new| {
                            old.base == new.base
                                && old.revision == new.revision
                                && old.collapsed == new.collapsed
                                && ((old.size - old.compact) - (new.size - new.compact))
                                    .abs()
                                    .max_element()
                                    <= 0.5
                        })
                    })
            })
        {
            state.last = Some((input, epoch));
            continue;
        }
        if state.last.as_ref() == Some(&(input.clone(), epoch)) {
            continue;
        }
        state.last = Some((input.clone(), epoch));
        state.conflict = state.model.update(input, epoch, &content).err();
        // Validate the whole replacement before touching memory; failures cannot publish
        // a partial solve. This resource borrow is the atomic placement-application phase.
        if state.model.offsets.iter().all(|(key, delta)| {
            delta.is_finite()
                && memory
                    .node(&graph, key)
                    .is_some_and(|(base, _)| (base + *delta).is_finite())
        }) {
            let current = memory
                .offsets
                .iter()
                .filter(|((key, _), _)| key == &graph)
                .map(|((_, key), offset)| (key.clone(), *offset))
                .collect::<BTreeMap<_, _>>();
            if current != state.model.offsets {
                memory.offsets.retain(|(key, _), _| key != &graph);
                for (key, offset) in &state.model.offsets {
                    memory.offsets.insert((graph.clone(), key.clone()), *offset);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
