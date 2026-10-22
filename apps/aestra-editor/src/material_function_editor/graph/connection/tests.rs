//! Native pointer events through actual shared observers and composed function sockets.
use super::*;
use crate::feathers::node_graph::{FeathersGraphNode, GraphMarqueeSelection, GraphSelectionMode};
use crate::material_function_editor::graph::{
    clipboard_context::FunctionGraphNodeAction, socket_palette::tests::setup_with_function,
};
use crate::material_graph::{
    MaterialGraphSelectionState, click_input as click, drag_input::tests as drag,
};
use aestra_core::{
    MaterialFunctionId,
    material::{MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialValue},
};
use bevy::picking::pointer::PointerButton;
use std::collections::BTreeSet;

fn function() -> MaterialFunction {
    let mut f = MaterialFunction::from_ron(include_str!(
        "../../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    f.custom_wesl = None;
    f.expressions = vec![
        MaterialExpression {
            id: f.outputs[0].expression,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: MaterialExpressionId::new(),
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(2.0)),
        },
    ];
    f.normalized()
}
struct Fixture {
    app: App,
    root: tempfile::TempDir,
    original: MaterialFunction,
    viewport: Entity,
    source: Entity,
    target: Entity,
}
impl Fixture {
    fn new(scale: f32, zoom: f32) -> Self {
        let original = function();
        let source_id = original
            .expressions
            .iter()
            .find(|e| e.id != original.outputs[0].expression)
            .unwrap()
            .id;
        let (mut app, root, viewport, _, target) =
            setup_with_function(scale, zoom, original.clone());
        let source = app
            .world_mut()
            .query::<(Entity, &Socket)>()
            .iter(app.world())
            .find(|(_, s)| matches!(s.kind, SocketKind::Source(id) if id == source_id))
            .unwrap()
            .0;
        Self {
            app,
            root,
            original,
            viewport,
            source,
            target,
        }
    }
    fn current(&self) -> MaterialFunction {
        self.app
            .world()
            .resource::<EditorSession>()
            .graph_function(self.app.world().resource::<ProjectEffectCatalog>())
            .unwrap()
    }
    fn serial(&self) -> u64 {
        self.app
            .world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial()
    }
    fn target_position(&self) -> Vec2 {
        let computed = self.app.world().get::<ComputedNode>(self.target).unwrap();
        self.app
            .world()
            .get::<UiGlobalTransform>(self.target)
            .unwrap()
            .translation
            .trunc()
            * computed.inverse_scale_factor
    }
    fn trigger<E: EntityEvent>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn start(&mut self, source: Entity) {
        self.trigger(drag::start(
            source,
            Entity::PLACEHOLDER,
            Vec2::new(300.0, 200.0),
            PointerButton::Primary,
        ));
    }
    fn assert_unchanged(&self) {
        assert_eq!(self.current(), self.original);
        assert_eq!(self.serial(), 0);
        assert_eq!(
            MaterialFunction::load_ron(self.root.path().join("body.aestra.material-function.ron"))
                .unwrap(),
            self.original
        );
    }
    fn assert_connected_and_undo(&mut self) {
        let source = match self.app.world().get::<Socket>(self.source).unwrap().kind {
            SocketKind::Source(id) => id,
            _ => unreachable!(),
        };
        assert_eq!(self.current().outputs[0].expression, source);
        assert_eq!(self.serial(), 1);
        let after = self.current();
        self.app
            .world_mut()
            .trigger(crate::history::HistoryAction::Undo);
        self.app.world_mut().flush();
        assert_eq!(self.current(), self.original);
        self.app
            .world_mut()
            .trigger(crate::history::HistoryAction::Redo);
        self.app.world_mut().flush();
        assert_eq!(self.current(), after);
    }
    fn click(&mut self, entity: Entity, button: PointerButton) {
        self.trigger(click::click(
            entity,
            Entity::PLACEHOLDER,
            Vec2::ZERO,
            button,
        ));
    }
    fn selected(&self, expression: MaterialExpressionId) -> bool {
        self.app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .is_function_expression_selected(
                Some(crate::docking::EditorViewId(9)),
                self.original.id,
                expression,
            )
    }
    fn nodes(&mut self) -> Vec<(Entity, MaterialExpressionId)> {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &FunctionGraphNodeAction)>()
            .iter(world)
            .map(|(e, a)| (e, a.expression))
            .collect()
    }
}

#[test]
fn descendant_socket_drag_resolves_origin_updates_cursor_and_filters_foreign_view() {
    let mut f = Fixture::new(1.25, 0.75);
    let child = f.app.world_mut().spawn(ChildOf(f.source)).id();
    let socket = *f.app.world().get::<Socket>(f.target).unwrap();
    let foreign_view = f.app.world_mut().spawn(View(socket.owner)).id();
    let foreign_node = f.app.world_mut().spawn(ChildOf(foreign_view)).id();
    let foreign = f
        .app
        .world_mut()
        .spawn((
            Socket {
                node: foreign_node,
                ..socket
            },
            ChildOf(foreign_node),
        ))
        .id();
    f.start(child);
    assert_eq!(
        f.app.world().resource::<ConnectionPreview>().0.unwrap().0,
        f.source
    );
    assert!(
        f.app
            .world()
            .resource::<ConnectionPreview>()
            .1
            .contains(&f.target)
    );
    assert!(
        !f.app
            .world()
            .resource::<ConnectionPreview>()
            .1
            .contains(&foreign)
    );
    let next = Vec2::new(555.0, 321.0);
    f.trigger(drag::motion(
        child,
        Entity::PLACEHOLDER,
        next,
        PointerButton::Primary,
    ));
    assert_eq!(
        f.app.world().resource::<ConnectionPreview>().0,
        Some((f.source, next))
    );
    f.assert_unchanged();
}

#[test]
fn release_uses_latest_socket_position_at_multiple_dpi_and_zoom_and_commits_once() {
    for (scale, zoom) in [(1.0, 1.0), (1.25, 0.75), (1.5, 1.5)] {
        let mut f = Fixture::new(scale, zoom);
        f.start(f.source);
        // No intermediate motion or layout pass: release must re-evaluate snapping.
        f.trigger(drag::end(
            f.source,
            Entity::PLACEHOLDER,
            f.target_position(),
            PointerButton::Primary,
        ));
        assert!(f.app.world().resource::<ConnectionPreview>().0.is_none());
        f.assert_connected_and_undo();
    }
}

#[test]
fn actual_drop_on_socket_descendant_commits_once_and_trailing_end_cannot_duplicate() {
    let mut f = Fixture::new(1.0, 1.0);
    let from = f.app.world_mut().spawn(ChildOf(f.source)).id();
    let to = f.app.world_mut().spawn(ChildOf(f.target)).id();
    f.start(from);
    f.trigger(drag::drop(
        to,
        from,
        Entity::PLACEHOLDER,
        f.target_position(),
        PointerButton::Primary,
    ));
    assert!(f.app.world().resource::<ConnectionPreview>().0.is_none());
    f.trigger(drag::end(
        from,
        Entity::PLACEHOLDER,
        f.target_position(),
        PointerButton::Primary,
    ));
    f.assert_connected_and_undo();
}

#[test]
fn escape_cancels_release_and_unrelated_or_secondary_starts_do_not_edit() {
    let mut f = Fixture::new(1.0, 1.0);
    f.trigger(drag::start(
        f.source,
        Entity::PLACEHOLDER,
        Vec2::ZERO,
        PointerButton::Secondary,
    ));
    assert!(f.app.world().resource::<ConnectionPreview>().0.is_none());
    let unrelated = f.app.world_mut().spawn_empty().id();
    f.start(unrelated);
    assert!(f.app.world().resource::<ConnectionPreview>().0.is_none());
    f.start(f.source);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    f.trigger(drag::end(
        f.source,
        Entity::PLACEHOLDER,
        f.target_position(),
        PointerButton::Primary,
    ));
    assert!(f.app.world().resource::<ConnectionPreview>().0.is_none());
    f.assert_unchanged();
}

#[test]
fn cross_view_or_other_owner_drops_do_not_rewire() {
    for owner in [None, Some(MaterialFunctionId::new())] {
        let mut f = Fixture::new(1.0, 1.0);
        let mut socket = *f.app.world().get::<Socket>(f.target).unwrap();
        socket.owner = owner.unwrap_or(socket.owner);
        let view = f.app.world_mut().spawn(View(socket.owner)).id();
        let node = f.app.world_mut().spawn(ChildOf(view)).id();
        socket.node = node;
        let destination = f.app.world_mut().spawn((socket, ChildOf(node))).id();
        f.start(f.source);
        f.trigger(drag::drop(
            destination,
            f.source,
            Entity::PLACEHOLDER,
            Vec2::ZERO,
            PointerButton::Primary,
        ));
        f.assert_unchanged();
    }
}

#[test]
fn primary_child_click_selects_owner_node_with_control_toggle_and_shift_extend() {
    let mut f = Fixture::new(1.0, 1.0);
    let nodes = f.nodes();
    let child = f.app.world_mut().spawn(ChildOf(nodes[0].0)).id();
    f.click(child, PointerButton::Primary);
    assert!(f.selected(nodes[0].1));
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    f.click(nodes[1].0, PointerButton::Primary);
    assert!(f.selected(nodes[0].1) && f.selected(nodes[1].1));
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlRight);
    f.click(nodes[0].0, PointerButton::Primary);
    assert!(!f.selected(nodes[0].1) && f.selected(nodes[1].1));
    f.assert_unchanged();
}

#[test]
fn socket_and_constant_descendants_do_not_select_their_node_and_space_blocks_node_selection() {
    let mut f = Fixture::new(1.0, 1.0);
    f.click(f.source, PointerButton::Primary);
    let world = f.app.world_mut();
    let constant = world
        .query_filtered::<Entity, With<crate::material_function_editor::graph::model::Constant>>()
        .iter(world)
        .next()
        .unwrap();
    let child = f.app.world_mut().spawn(ChildOf(constant)).id();
    f.click(child, PointerButton::Primary);
    let nodes = f.nodes();
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    f.click(nodes[0].0, PointerButton::Primary);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    f.click(nodes[0].0, PointerButton::Secondary);
    for (_, id) in nodes {
        assert!(!f.selected(id));
    }
    f.assert_unchanged();
}

#[test]
fn canvas_click_clears_only_its_view_and_marquee_uses_real_node_keys() {
    let mut f = Fixture::new(1.0, 1.0);
    let nodes = f.nodes();
    let other = Some(crate::docking::EditorViewId(22));
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .select_function_expression(other, f.original.id, nodes[0].1, false, false);
    let key = f
        .app
        .world()
        .get::<FeathersGraphNode>(nodes[1].0)
        .unwrap()
        .node_key()
        .to_owned();
    f.app.world_mut().trigger(GraphMarqueeSelection {
        viewport: f.viewport,
        nodes: BTreeSet::from([key]),
        mode: GraphSelectionMode::Replace,
    });
    f.app.world_mut().flush();
    assert!(f.selected(nodes[1].1));
    f.click(f.viewport, PointerButton::Primary);
    assert!(!f.selected(nodes[1].1));
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .is_function_expression_selected(other, f.original.id, nodes[0].1)
    );
    f.assert_unchanged();
}

#[test]
fn node_click_consumes_bubbling_but_control_click_does_not_clear_selection() {
    #[derive(Resource, Default)]
    struct Bubbled(u32);
    let mut f = Fixture::new(1.0, 1.0);
    let nodes = f.nodes();
    f.app.init_resource::<Bubbled>();
    let ancestor = f
        .app
        .world_mut()
        .spawn(Node::default())
        .observe(|_: On<click::PointerClick>, mut count: ResMut<Bubbled>| {
            count.0 += 1;
        })
        .id();
    f.app
        .world_mut()
        .entity_mut(f.viewport)
        .insert(ChildOf(ancestor));
    // Use the node's existing parent chain, not a replacement event callback.
    f.click(nodes[0].0, PointerButton::Primary);
    assert_eq!(f.app.world().resource::<Bubbled>().0, 0);
    assert!(f.selected(nodes[0].1));
    let world = f.app.world_mut();
    let control = world
        .query::<(
            Entity,
            &crate::material_function_editor::graph::model::Constant,
        )>()
        .iter(world)
        .find(|(_, c)| c.expression == nodes[1].1)
        .unwrap()
        .0;
    let input = f.app.world().get::<Children>(control).unwrap()[0];
    f.click(input, PointerButton::Primary);
    assert!(f.selected(nodes[0].1));
    f.assert_unchanged();
}

#[test]
fn actual_empty_canvas_palette_entry_does_not_edit_before_a_choice() {
    let mut f = Fixture::new(1.25, 0.75);
    let (screen, _) = crate::material_function_editor::graph::socket_palette::tests::pointer(
        f.app.world(),
        f.viewport,
    );
    socket_palette::open_canvas(f.app.world_mut(), f.viewport, screen);
    f.app.world_mut().flush();
    let world = f.app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<socket_palette::Surface>>()
            .iter(world)
            .count(),
        1
    );
    f.assert_unchanged();
}
