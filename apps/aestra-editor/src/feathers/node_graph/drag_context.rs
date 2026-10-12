//! Shared captured geometry and snapping state; no toolbar or document substitutes.
use super::drag_assist::model;
use super::{
    geometry::*, memory::GraphViewportMemory, navigation::FeathersGraphViewport,
    node::FeathersGraphNode,
};
use bevy::{ecs::system::SystemParam, prelude::*};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Debug)]
pub(super) struct Captured {
    key: GraphNodeKey,
    shape: model::Shape,
    content: u64,
    collapsed: bool,
    preview: bool,
}

#[derive(Debug)]
pub(super) struct Gesture {
    pub(super) raw: Vec2,
    pub(super) view: Entity,
    pub(super) identity: Option<GraphViewKey>,
    pub(super) epoch: u64,
    pub(super) graph: String,
    pub(super) geometry: BTreeMap<Entity, Captured>,
    pub(super) guides: Vec<model::Guide>,
}

#[derive(Resource)]
pub(crate) struct State {
    pub(super) grid: bool,
    pub(super) alignment: bool,
    pub(super) gestures: HashMap<Entity, Gesture>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            grid: false,
            alignment: true,
            gestures: HashMap::new(),
        }
    }
}

#[derive(SystemParam)]
pub(crate) struct Context<'w, 's> {
    state: ResMut<'w, State>,
    registry: Option<Res<'w, GraphGeometryRegistry>>,
    metadata: Query<'w, 's, (Entity, &'static GraphGeometryNode)>,
    views: Query<
        'w,
        's,
        (
            &'static FeathersGraphViewport,
            Option<&'static GraphGeometryView>,
        ),
    >,
    keys: Option<Res<'w, ButtonInput<KeyCode>>>,
}

impl Context<'_, '_> {
    pub(crate) fn viewport(
        &self,
        entity: Entity,
        parents: &Query<&ChildOf>,
    ) -> Option<(Entity, f32)> {
        std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .find_map(|id| self.views.get(id).ok().map(|(view, _)| (id, view.zoom)))
    }

    pub(crate) fn begin(
        &mut self,
        entity: Entity,
        node: &FeathersGraphNode,
        parents: &Query<&ChildOf>,
        memory: &GraphViewportMemory,
    ) {
        let Some((view, _)) = self.viewport(entity, parents) else {
            return;
        };
        let identity = self
            .views
            .get(view)
            .ok()
            .and_then(|(_, meta)| meta.map(|meta| meta.key.clone()));
        let snapshot = identity
            .as_ref()
            .and_then(|key| self.registry.as_ref()?.mounted_view_snapshot(key, view));
        let mut geometry = BTreeMap::new();
        if let Some(snapshot) = snapshot.filter(|snapshot| snapshot.nodes.len() <= model::MAX_NODES)
        {
            for (id, meta) in &self.metadata {
                if self.viewport(id, parents).map(|(id, _)| id) != Some(view) {
                    continue;
                }
                if let Some(measured) = snapshot.nodes.get(&meta.key) {
                    geometry.insert(
                        id,
                        Captured {
                            key: meta.key,
                            shape: model::Shape {
                                rect: Rect::from_corners(
                                    measured.effective_position,
                                    measured.effective_position + measured.size,
                                ),
                            },
                            content: measured.content,
                            collapsed: measured.collapsed,
                            preview: measured.preview,
                        },
                    );
                }
            }
            if geometry.len() != snapshot.nodes.len() {
                geometry.clear();
            }
        }
        self.state.gestures.insert(
            entity,
            Gesture {
                raw: node.position,
                view,
                identity,
                graph: node.graph_key.clone(),
                epoch: memory
                    .offset_epochs
                    .get(&node.graph_key)
                    .copied()
                    .unwrap_or_default(),
                geometry,
                guides: vec![],
            },
        );
    }

    pub(crate) fn motion(
        &mut self,
        entity: Entity,
        delta: Vec2,
        fallback: Vec2,
        zoom: f32,
        memory: &GraphViewportMemory,
        moving_entities: &BTreeSet<Entity>,
        valid: impl Fn(Entity, Rect, bool) -> bool,
    ) -> Vec2 {
        let (grid, alignment) = (self.state.grid, self.state.alignment);
        let Some(gesture) = self.state.gestures.get_mut(&entity) else {
            return fallback + delta;
        };
        if !delta.is_finite() || !(gesture.raw + delta).is_finite() {
            return fallback;
        }
        gesture.raw += delta;
        gesture.guides.clear();
        let current = self.views.get(gesture.view).ok().and_then(|(_, meta)| meta);
        let stale = current.map(|meta| &meta.key) != gesture.identity.as_ref()
            || current.is_some_and(|meta| meta.nodes.len() != gesture.geometry.len())
            || memory
                .offset_epochs
                .get(&gesture.graph)
                .copied()
                .unwrap_or_default()
                != gesture.epoch
            || gesture.geometry.iter().any(|(id, capture)| {
                !valid(*id, capture.shape.rect, capture.collapsed)
                    || self.metadata.get(*id).map_or(true, |(_, meta)| {
                        meta.key != capture.key
                            || meta.content_stamp() != capture.content
                            || meta.is_preview() != capture.preview
                    })
            });
        if stale {
            gesture.geometry.clear();
        }
        let disabled = self
            .keys
            .as_ref()
            .is_some_and(|keys| keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight));
        if disabled {
            return gesture.raw;
        }
        let moving = gesture
            .geometry
            .get(&entity)
            .map(|capture| capture.shape.clone())
            .unwrap_or(model::Shape {
                rect: Rect::from_corners(gesture.raw, gesture.raw),
            });
        let targets = gesture
            .geometry
            .iter()
            .filter(|(id, _)| !moving_entities.contains(id))
            .map(|(_, capture)| capture.shape.clone())
            .collect::<Vec<_>>();
        let (position, guides) = model::snap(gesture.raw, &moving, &targets, zoom, grid, alignment);
        gesture.guides = guides;
        position
    }

    pub(crate) fn end(&mut self, entity: Entity) {
        self.state.gestures.remove(&entity);
    }
}
