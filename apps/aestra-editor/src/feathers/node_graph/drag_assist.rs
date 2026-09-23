//! Shared, session-only drag assistance. Geometry is captured from the dragged view;
//! stale/rebuilt targets disable alignment for the remainder of that gesture.
use super::{geometry::*, *};
use bevy::{ecs::system::SystemParam, feathers::controls::ButtonVariant, ui::Selected};
use std::collections::BTreeMap;

mod model;

#[derive(Component, Clone, Copy)]
pub(super) enum Toggle {
    Grid,
    Alignment,
}

#[derive(Clone, Debug)]
struct Captured {
    key: GraphNodeKey,
    shape: model::Shape,
    content: u64,
    collapsed: bool,
    preview: bool,
}

#[derive(Debug)]
struct Gesture {
    raw: Vec2,
    view: Entity,
    identity: Option<GraphViewKey>,
    epoch: u64,
    graph: String,
    geometry: BTreeMap<Entity, Captured>,
    guides: Vec<model::Guide>,
}

#[derive(Resource)]
pub(super) struct State {
    grid: bool,
    alignment: bool,
    gestures: HashMap<Entity, Gesture>,
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

/// Uses existing shared icon-button chrome. Labels are localized by sync_controls,
/// including function graph hosts that do not currently pass a Localizer to spawn.
pub(crate) fn spawn_controls(parent: &mut ChildSpawnerCommands, assets: &AssetServer) {
    for (action, icon, label) in [
        (
            Toggle::Grid,
            "icons/grid.svg",
            "Grid snapping (hold Alt for free movement)",
        ),
        (
            Toggle::Alignment,
            "icons/graph-align.svg",
            "Alignment snapping and guides (hold Alt for free movement)",
        ),
    ] {
        spawn_graph_tool_button(parent, assets, icon, label.into(), action);
    }
}

pub(super) fn toggle(event: On<Activate>, buttons: Query<&Toggle>, mut state: ResMut<State>) {
    if let Ok(action) = buttons.get(event.entity) {
        match action {
            Toggle::Grid => state.grid = !state.grid,
            Toggle::Alignment => state.alignment = !state.alignment,
        }
    }
}

pub(super) fn sync_controls(
    mut commands: Commands,
    state: Res<State>,
    localizer: Option<Res<crate::Localizer>>,
    buttons: Query<(Entity, Ref<Toggle>, Has<Selected>), With<FeathersActionButton>>,
) {
    for (entity, action, selected) in &buttons {
        let (enabled, label) = match *action {
            Toggle::Grid => (state.grid, "graph-snap-grid"),
            Toggle::Alignment => (state.alignment, "graph-snap-alignment"),
        };
        if enabled != selected {
            if enabled {
                commands
                    .entity(entity)
                    .insert((Selected, ButtonVariant::Primary));
            } else {
                commands
                    .entity(entity)
                    .remove::<Selected>()
                    .insert(ButtonVariant::Plain);
            }
        }
        if let Some(localizer) = &localizer {
            // Only queue changed values; do not repeatedly invalidate hover tooltips.
            if action.is_added() || localizer.is_changed() || enabled != selected {
                let text = localizer.text(label);
                commands.entity(entity).insert((
                    AccessibleLabel(text.clone()),
                    EditorTooltip::description(text),
                ));
            }
        }
    }
}

#[derive(SystemParam)]
pub(super) struct Context<'w, 's> {
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
    pub(super) fn viewport(
        &self,
        entity: Entity,
        parents: &Query<&ChildOf>,
    ) -> Option<(Entity, f32)> {
        std::iter::once(entity)
            .chain(parents.iter_ancestors(entity))
            .find_map(|id| self.views.get(id).ok().map(|(view, _)| (id, view.zoom)))
    }

    pub(super) fn begin(
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

    pub(super) fn motion(
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

    pub(super) fn end(&mut self, entity: Entity) {
        self.state.gestures.remove(&entity);
    }
}

#[derive(Component)]
pub(super) struct GuideLine;

pub(super) fn sync_guides(
    mut commands: Commands,
    mut state: ResMut<State>,
    nodes: Query<&FeathersGraphNode>,
    views: Query<&FeathersGraphViewport>,
    lines: Query<Entity, With<GuideLine>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    // At most two passive, clipped lines in the originating view. No hit-test interception.
    for entity in &lines {
        commands.entity(entity).despawn();
    }
    state.gestures.retain(|entity, gesture| {
        nodes.get(*entity).is_ok_and(|node| node.dragging) && views.contains(gesture.view)
    });
    if keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight) {
        return;
    }
    for gesture in state.gestures.values() {
        let Ok(view) = views.get(gesture.view) else {
            continue;
        };
        for guide in &gesture.guides {
            let axis = guide.axis;
            let mut start = Vec2::ZERO;
            start[axis] = guide.coordinate;
            start[1 - axis] = guide.start;
            let position = view.project_graph_point(start);
            let length = ((guide.end - guide.start) * view.zoom).max(1.0);
            commands.spawn((
                GuideLine,
                ChildOf(gesture.view),
                Pickable::IGNORE,
                ZIndex(30),
                BackgroundColor(theme::ACCENT),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(position.x),
                    top: Val::Px(position.y),
                    width: Val::Px(if axis == 0 { 1.0 } else { length }),
                    height: Val::Px(if axis == 1 { 1.0 } else { length }),
                    ..default()
                },
            ));
        }
    }
}

#[cfg(test)]
mod tests;
