//! Bounded insertion placement shared by graph adapters. Existing nodes are obstacles,
//! never writable outputs. New node extents are bootstrap estimates until first layout.
use super::{geometry::*, *};
use bevy::ecs::system::SystemParam;
use std::collections::BTreeMap;

const GAP: f32 = 24.0;
const MAX_OBSTACLES: usize = 512;
const MAX_CANDIDATES: usize = 256;
const MAX_DISTANCE: f32 = 1024.0;

#[derive(Clone, Copy, Default)]
pub(crate) enum Neighborhood {
    #[default]
    Cursor,
    After(GraphNodeKey),
    Before(GraphNodeKey),
}

#[derive(Default)]
pub(crate) struct Area {
    nodes: BTreeMap<GraphNodeKey, Rect>,
    reserved: Vec<Rect>,
    measured: bool,
}

pub(crate) struct Placement {
    pub position: Vec2,
    /// True only when measured obstacles and the bounded search establish spacing.
    pub assisted: bool,
}

impl Area {
    /// Estimated rectangles are a fallback, never evidence of measured clearance.
    pub(crate) fn seed(&mut self, key: GraphNodeKey, position: Vec2, size: Vec2) {
        if valid_rect(position, size)
            && let std::collections::btree_map::Entry::Vacant(entry) = self.nodes.entry(key)
        {
            entry.insert(Rect::from_corners(position, position + size));
            self.measured = false;
        }
    }

    pub(crate) fn rect(&self, key: GraphNodeKey) -> Option<Rect> {
        self.nodes.get(&key).copied()
    }

    pub(crate) fn place_node(
        &mut self,
        key: GraphNodeKey,
        cursor: Vec2,
        size: Vec2,
        neighborhood: Neighborhood,
    ) -> Placement {
        let placed = self.place(cursor, size, neighborhood);
        if valid_rect(placed.position, size) {
            self.reserved.pop();
            self.nodes.insert(
                key,
                Rect::from_corners(placed.position, placed.position + size),
            );
        }
        placed
    }

    pub(crate) fn place(
        &mut self,
        cursor: Vec2,
        size: Vec2,
        neighborhood: Neighborhood,
    ) -> Placement {
        let mut preferred = cursor;
        let anchor = match neighborhood {
            Neighborhood::Cursor => None,
            Neighborhood::After(key) | Neighborhood::Before(key) => self.nodes.get(&key),
        };
        if let Some(anchor) = anchor {
            preferred.x = match neighborhood {
                Neighborhood::After(_) => cursor.x.max(anchor.max.x + GAP),
                Neighborhood::Before(_) => cursor.x.min(anchor.min.x - GAP - size.x),
                Neighborhood::Cursor => cursor.x,
            };
        }
        let obstacles = self
            .nodes
            .values()
            .chain(&self.reserved)
            .copied()
            .collect::<Vec<_>>();
        let result = find_space(preferred, size, &obstacles, |position| {
            match (neighborhood, anchor) {
                (Neighborhood::After(_), Some(anchor)) => position.x >= anchor.max.x + GAP,
                (Neighborhood::Before(_), Some(anchor)) => {
                    position.x + size.x <= anchor.min.x - GAP
                }
                _ => true,
            }
        });
        let position = result.unwrap_or(preferred);
        if valid_rect(position, size) {
            self.reserved
                .push(Rect::from_corners(position, position + size));
        }
        Placement {
            position,
            assisted: self.measured && result.is_some(),
        }
    }
}

fn valid_rect(position: Vec2, size: Vec2) -> bool {
    position.is_finite()
        && size.is_finite()
        && size.min_element() > 0.0
        && (position + size).is_finite()
}

fn find_space(
    preferred: Vec2,
    size: Vec2,
    obstacles: &[Rect],
    allowed: impl Fn(Vec2) -> bool,
) -> Option<Vec2> {
    if !valid_rect(preferred, size)
        || obstacles.len() > MAX_OBSTACLES
        || obstacles
            .iter()
            .any(|rect| !valid_rect(rect.min, rect.size()))
    {
        return None;
    }
    let free = |position: Vec2| {
        valid_rect(position, size)
            && allowed(position)
            && obstacles.iter().all(|rect| {
                position.x + size.x + GAP <= rect.min.x
                    || position.x >= rect.max.x + GAP
                    || position.y + size.y + GAP <= rect.min.y
                    || position.y >= rect.max.y + GAP
            })
    };
    if free(preferred) {
        return Some(preferred);
    }
    let mut candidates = Vec::new();
    for rect in obstacles {
        for x in [preferred.x, rect.min.x - size.x - GAP, rect.max.x + GAP] {
            for y in [preferred.y, rect.min.y - size.y - GAP, rect.max.y + GAP] {
                let position = Vec2::new(x, y);
                if position.is_finite()
                    && position.distance_squared(preferred) <= MAX_DISTANCE * MAX_DISTANCE
                {
                    candidates.push(position);
                }
            }
        }
    }
    candidates.sort_by(|a, b| {
        a.distance_squared(preferred)
            .total_cmp(&b.distance_squared(preferred))
            .then_with(|| a.x.total_cmp(&b.x))
            .then_with(|| a.y.total_cmp(&b.y))
    });
    candidates.dedup();
    candidates
        .into_iter()
        .take(MAX_CANDIDATES)
        .find(|position| free(*position))
}

#[derive(SystemParam)]
pub(crate) struct Context<'w, 's> {
    localizer: Option<Res<'w, crate::Localizer>>,
    registry: Option<Res<'w, GraphGeometryRegistry>>,
    views: Query<
        'w,
        's,
        (
            Entity,
            &'static GraphGeometryView,
            &'static FeathersGraphViewport,
            &'static ComputedNode,
        ),
    >,
    nodes: Query<
        'w,
        's,
        (
            Entity,
            &'static GraphGeometryNode,
            &'static FeathersGraphNode,
            &'static ComputedNode,
        ),
    >,
    parents: Query<'w, 's, &'static ChildOf>,
}

impl Context<'_, '_> {
    /// Non-pointer commands use a deterministic mounted view, never a different document.
    pub(crate) fn document_view(&self, document: &GraphDocumentKey) -> Option<GraphViewKey> {
        self.views
            .iter()
            .filter(|(_, meta, _, computed)| {
                &meta.key.document == document && computed.size().min_element() > 0.0
            })
            .map(|(_, meta, _, _)| meta.key.clone())
            .min()
    }
    /// Freeze bootstrap positions only after semantic creation succeeds. Never bake
    /// a temporary offset into an existing base, and never touch another view's nodes.
    pub(crate) fn preserve_existing(&self, key: &GraphViewKey, memory: &mut GraphViewportMemory) {
        let Some((entity, _, _, _)) = self.views.iter().find(|(_, meta, _, _)| &meta.key == key)
        else {
            return;
        };
        for (id, _, node, _) in &self.nodes {
            if self
                .parents
                .iter_ancestors(id)
                .any(|parent| parent == entity)
                && node.position.is_finite()
                && memory.node(&node.graph_key, &node.node_key).is_none()
            {
                memory.set_node(
                    &node.graph_key,
                    &node.node_key,
                    node.position,
                    node.collapsed,
                );
            }
        }
    }
    pub(crate) fn notice(&self) -> String {
        self.localizer.as_ref().map_or_else(
            || "Local spacing unavailable; position the new node manually".into(),
            |localizer| localizer.text("graph-placement-unavailable"),
        )
    }
    pub(crate) fn center(&self, key: &GraphViewKey) -> Option<Vec2> {
        let (_, _, view, computed) = self.views.iter().find(|(_, meta, _, _)| &meta.key == key)?;
        let size = computed.size() * computed.inverse_scale_factor;
        (size.is_finite() && size.min_element() > 0.0)
            .then(|| view.unproject_viewport_point(size * 0.5))
    }

    pub(crate) fn capture(&self, key: &GraphViewKey, memory: &GraphViewportMemory) -> Area {
        let Some((entity, _, _, _)) = self.views.iter().find(|(_, meta, _, _)| &meta.key == key)
        else {
            return Area::default();
        };
        let Some(snapshot) = self
            .registry
            .as_ref()
            .and_then(|registry| registry.mounted_view_snapshot(key, entity))
        else {
            return Area::default();
        };
        if snapshot.nodes.len() > MAX_OBSTACLES {
            return Area::default();
        }
        let mut nodes = BTreeMap::new();
        for (id, meta, node, computed) in &self.nodes {
            if !self
                .parents
                .iter_ancestors(id)
                .any(|parent| parent == entity)
            {
                continue;
            }
            let Some(measured) = snapshot.nodes.get(&meta.key) else {
                return Area::default();
            };
            let current = memory
                .node_position(&node.graph_key, &node.node_key)
                .unwrap_or(node.position);
            if !valid_rect(current, computed.size() * computed.inverse_scale_factor)
                || current.distance(measured.effective_position) > 0.5
                || (computed.size() * computed.inverse_scale_factor - measured.size)
                    .abs()
                    .max_element()
                    > 0.5
                || node.collapsed != measured.collapsed
                || meta.content_stamp() != measured.content
                || meta.is_preview() != measured.preview
            {
                return Area::default();
            }
            nodes.insert(
                meta.key,
                Rect::from_corners(current, current + measured.size),
            );
        }
        if nodes.len() != snapshot.nodes.len() {
            return Area::default();
        }
        Area {
            nodes,
            reserved: vec![],
            measured: true,
        }
    }
}

#[cfg(test)]
mod tests;
