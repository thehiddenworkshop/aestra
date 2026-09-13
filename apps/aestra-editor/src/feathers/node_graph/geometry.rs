//! Observations only: no writes to placement memory, semantic assets, or history.
//! M2 consumes the snapshots; M4 consumes changes after presentation Undo exists.
#![allow(dead_code)] // Public observation API is collected now, consumed by M2/M4.

use crate::{
    docking::EditorViewId,
    document::{DocumentId, DocumentKey},
};
use aestra_authoring::{MaterialConnectionTarget, MaterialExpressionInput, MaterialOutputSocket};
use aestra_core::{MaterialExpressionId, MaterialFunctionOutputId};
use bevy::prelude::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{DefaultHasher, Hash, Hasher},
    path::PathBuf,
};

mod collect;
#[cfg(test)]
pub(crate) mod tests;

/// Logical units; tolerates sub-pixel rounding without quantizing saved/manual positions.
const GEOMETRY_EPSILON: f32 = 0.5;
const STABLE_FRAMES: u8 = 2;

pub(super) fn register(app: &mut App) {
    app.init_resource::<GraphGeometryRegistry>().add_systems(
        PostUpdate,
        collect::collect_graph_geometry.after(bevy::ui::UiSystems::PostLayout),
    );
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GraphDocumentKey {
    pub project: PathBuf,
    pub asset: DocumentKey,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GraphViewKey {
    pub document: GraphDocumentKey,
    /// None is the graph tool panel; Some is a distinct docked editor view.
    pub view: Option<EditorViewId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum GraphNodeKey {
    Expression(MaterialExpressionId),
    MaterialOutputs,
    FunctionOutputs,
}

/// Adapter-declared topology distinguishes real insertion/removal from missing UI measurements.
#[derive(Component, Debug, Clone)]
pub(crate) struct GraphGeometryView {
    pub key: GraphViewKey,
    pub nodes: BTreeSet<GraphNodeKey>,
}

#[derive(Component, Debug, Clone)]
pub(crate) struct GraphGeometryNode {
    pub key: GraphNodeKey,
    content: u64,
    preview: bool,
}

impl GraphGeometryNode {
    pub(crate) fn new(key: GraphNodeKey, content: &impl std::fmt::Debug, preview: bool) -> Self {
        // Session-only presentation fingerprint; never an asset identity or persisted hash.
        let mut hash = DefaultHasher::new();
        format!("{content:?}").hash(&mut hash);
        Self {
            key,
            content: hash.finish(),
            preview,
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphGeometryPort {
    Input(MaterialExpressionInput),
    Output,
    MaterialOutput(MaterialOutputSocket),
    FunctionOutput(MaterialFunctionOutputId),
}

impl From<MaterialConnectionTarget> for GraphGeometryPort {
    fn from(target: MaterialConnectionTarget) -> Self {
        match target {
            MaterialConnectionTarget::ExpressionInput { input, .. } => Self::Input(input),
            MaterialConnectionTarget::ProgramOutput(output) => Self::MaterialOutput(output),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GraphPortGeometry {
    pub key: GraphGeometryPort,
    /// Offset from the node's top-left, in unzoomed logical graph units.
    pub offset: Vec2,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphNodeGeometry {
    pub effective_position: Vec2,
    pub size: Vec2,
    pub ports: Vec<GraphPortGeometry>,
    pub geometry_revision: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct GraphGeometrySnapshot {
    pub measured_in: GraphViewKey,
    pub document_generation: Option<DocumentId>,
    pub geometry_revision: u64,
    pub nodes: BTreeMap<GraphNodeKey, GraphNodeGeometry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphResizeReason {
    PreviewOpened,
    PreviewClosed,
    Collapsed,
    Expanded,
    ContentChanged,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GraphGeometryChange {
    Moved {
        node: GraphNodeKey,
        position: Vec2,
    },
    Resized {
        node: GraphNodeKey,
        old_size: Vec2,
        new_size: Vec2,
        reason: GraphResizeReason,
    },
    Inserted {
        node: GraphNodeKey,
    },
    Removed {
        node: GraphNodeKey,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct GraphGeometryEvent {
    pub view: GraphViewKey,
    pub document_generation: Option<DocumentId>,
    pub geometry_revision: u64,
    pub change: GraphGeometryChange,
}

#[derive(Debug, Clone)]
struct NodeObservation {
    entity: Entity,
    geometry: GraphNodeGeometry,
    content: u64,
    preview: bool,
    collapsed: bool,
    inverse_scale: f32,
}

#[derive(Debug, Clone)]
struct ViewObservation {
    entity: Entity,
    generation: Option<DocumentId>,
    expected: BTreeSet<GraphNodeKey>,
    nodes: BTreeMap<GraphNodeKey, NodeObservation>,
    complete: bool,
}

impl ViewObservation {
    fn ready(&self) -> bool {
        self.complete && self.expected.iter().eq(self.nodes.keys())
    }

    fn equivalent(&self, other: &Self) -> bool {
        self.entity == other.entity
            && self.generation == other.generation
            && self.expected == other.expected
            && self.ready()
            && other.ready()
            && self.nodes.iter().all(|(key, node)| {
                let previous = &other.nodes[key];
                node.entity == previous.entity
                    && node.content == previous.content
                    && node.preview == previous.preview
                    && node.collapsed == previous.collapsed
                    && node.inverse_scale == previous.inverse_scale
                    && geometry_close(&node.geometry, &previous.geometry)
            })
    }
}

fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).abs().max_element() <= GEOMETRY_EPSILON
}

fn geometry_close(a: &GraphNodeGeometry, b: &GraphNodeGeometry) -> bool {
    close(a.effective_position, b.effective_position)
        && close(a.size, b.size)
        && a.ports.len() == b.ports.len()
        && a.ports.iter().all(|port| {
            b.ports
                .iter()
                .any(|other| port.key == other.key && close(port.offset, other.offset))
        })
}

#[derive(Debug)]
struct ViewState {
    sample: ViewObservation,
    stable_frames: u8,
    snapshot: Option<GraphGeometrySnapshot>,
}

#[derive(Debug, Default)]
struct DocumentState {
    owner: Option<GraphViewKey>,
    baseline: Option<ViewObservation>,
    current: Option<GraphGeometrySnapshot>,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct GraphGeometryRegistry {
    views: BTreeMap<GraphViewKey, ViewState>,
    documents: BTreeMap<GraphDocumentKey, DocumentState>,
    revision: u64,
    /// Coalesced PostUpdate changes, available through the following Update. Not a work queue.
    changes: Vec<GraphGeometryEvent>,
}

impl GraphGeometryRegistry {
    pub(crate) fn snapshot(&self, document: &GraphDocumentKey) -> Option<&GraphGeometrySnapshot> {
        self.documents.get(document)?.current.as_ref()
    }

    pub(crate) fn view_snapshot(&self, view: &GraphViewKey) -> Option<&GraphGeometrySnapshot> {
        self.views.get(view)?.snapshot.as_ref()
    }

    pub(crate) fn changes(&self) -> &[GraphGeometryEvent] {
        &self.changes
    }

    fn observe(
        &mut self,
        incoming: BTreeMap<GraphViewKey, ViewObservation>,
        focused: Option<EditorViewId>,
    ) {
        self.changes.clear();
        self.views.retain(|key, _| incoming.contains_key(key));
        for (key, raw) in incoming {
            let state = self.views.entry(key.clone()).or_insert_with(|| ViewState {
                sample: raw.clone(),
                stable_frames: 0,
                snapshot: None,
            });
            if raw.equivalent(&state.sample) {
                state.stable_frames = state.stable_frames.saturating_add(1);
                // Retain the comparison anchor so cumulative sub-tolerance drift is detected.
            } else {
                state.stable_frames = u8::from(raw.ready());
                state.sample = raw;
                state.snapshot = None;
            }
            if state.stable_frames >= STABLE_FRAMES && state.snapshot.is_none() {
                self.revision += 1;
                state.snapshot = Some(GraphGeometrySnapshot {
                    measured_in: key,
                    document_generation: state.sample.generation,
                    geometry_revision: self.revision,
                    nodes: state
                        .sample
                        .nodes
                        .iter()
                        .map(|(key, node)| {
                            let mut geometry = node.geometry.clone();
                            geometry.geometry_revision = self.revision;
                            (*key, geometry)
                        })
                        .collect(),
                });
            }
        }
        let documents = self
            .views
            .keys()
            .map(|key| key.document.clone())
            .collect::<BTreeSet<_>>();
        self.documents.retain(|key, _| documents.contains(key));
        for document in documents {
            let state = self.documents.entry(document.clone()).or_default();
            let valid = self
                .views
                .iter()
                .filter(|(key, view)| key.document == document && view.snapshot.is_some())
                .collect::<Vec<_>>();
            let chosen = valid
                .iter()
                .find(|(key, _)| focused.is_some() && key.view == focused)
                .or_else(|| {
                    valid
                        .iter()
                        .find(|(key, _)| state.owner.as_ref() == Some(*key))
                })
                .or_else(|| valid.first());
            let Some((key, view)) = chosen else {
                // Unmeasured/rebuilt entities cannot provide actionable geometry. Retain only the
                // comparison baseline while the same visible view lifetime remains present.
                state.current = None;
                continue;
            };
            let snapshot = view.snapshot.as_ref().unwrap();
            if state
                .current
                .as_ref()
                .is_some_and(|old| old.geometry_revision == snapshot.geometry_revision)
            {
                continue;
            }
            if state.owner.as_ref() == Some(key)
                && let Some(previous) = &state.baseline
                && previous.generation == view.sample.generation
            {
                self.changes
                    .extend(
                        changes_between(previous, &view.sample)
                            .into_iter()
                            .map(|change| GraphGeometryEvent {
                                view: (*key).clone(),
                                document_generation: snapshot.document_generation,
                                geometry_revision: snapshot.geometry_revision,
                                change,
                            }),
                    );
            }
            // Owner handoff / initial measurement / reopened document establishes a baseline.
            state.owner = Some((*key).clone());
            state.baseline = Some(view.sample.clone());
            state.current = Some(snapshot.clone());
        }
    }
}

fn changes_between(old: &ViewObservation, new: &ViewObservation) -> Vec<GraphGeometryChange> {
    let mut changes = Vec::new();
    for key in old.expected.difference(&new.expected) {
        changes.push(GraphGeometryChange::Removed { node: *key });
    }
    for (key, node) in &new.nodes {
        let Some(previous) = old.nodes.get(key) else {
            changes.push(GraphGeometryChange::Inserted { node: *key });
            continue;
        };
        let rebuilt = old.entity != new.entity || previous.entity != node.entity;
        if !rebuilt
            && !close(
                previous.geometry.effective_position,
                node.geometry.effective_position,
            )
        {
            changes.push(GraphGeometryChange::Moved {
                node: *key,
                position: node.geometry.effective_position,
            });
        }
        let reason = if node.preview != previous.preview {
            Some(if node.preview {
                GraphResizeReason::PreviewOpened
            } else {
                GraphResizeReason::PreviewClosed
            })
        } else if node.collapsed != previous.collapsed {
            Some(if node.collapsed {
                GraphResizeReason::Collapsed
            } else {
                GraphResizeReason::Expanded
            })
        } else if node.content != previous.content
            || (!rebuilt && node.inverse_scale == previous.inverse_scale)
        {
            Some(GraphResizeReason::ContentChanged)
        } else {
            None
        };
        if !close(previous.geometry.size, node.geometry.size)
            && let Some(reason) = reason
        {
            changes.push(GraphGeometryChange::Resized {
                node: *key,
                old_size: previous.geometry.size,
                new_size: node.geometry.size,
                reason,
            });
        }
    }
    changes
}
