//! Real pointer events through the actual menu-opening and wire-picking consumers.
use super::*;
use crate::{
    docking::EditorViewId,
    feathers::node_graph::GraphSelectionMode,
    material_document::MaterialEditingTarget,
    material_graph::{
        MaterialSelectionScope, click_input, keys::material_graph_expression_node_key,
    },
};
use aestra_authoring::{MaterialConnectionTarget, MaterialOutputSocket};
use aestra_core::{EffectAsset, MaterialExpressionId, MaterialProgramId};
use bevy::{ecs::system::RunSystemOnce, math::Affine2};

#[derive(Resource, Default)]
struct Bubbled(usize);
struct Fixture {
    global: bool,
    observed: std::collections::BTreeSet<Entity>,
    app: App,
    window: Entity,
    root: Entity,
    viewport: Entity,
    canvas: Entity,
    program: MaterialProgramId,
    scope: MaterialSelectionScope,
    effect: EffectAsset,
}
impl Fixture {
    fn new() -> Self {
        let session = crate::test_support::session_with_timing_slack();
        let effect = session.effect.clone();
        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<MaterialGraphPaletteState>()
            .init_resource::<MaterialGraphSelectionState>()
            .init_resource::<MaterialStackInspectorState>()
            .init_resource::<Bubbled>();
        let window = app.world_mut().spawn(Window::default()).id();
        let root = app
            .world_mut()
            .spawn_empty()
            .observe(|_: On<PointerClick>, mut seen: ResMut<Bubbled>| seen.0 += 1)
            .id();
        let program = MaterialProgramId::new();
        let scope = Some(EditorViewId(7));
        let viewport = app
            .world_mut()
            .spawn((
                ChildOf(root),
                MaterialGraphViewport {
                    program,
                    scope,
                    editing_target: MaterialEditingTarget::Program {
                        root: "standalone".into(),
                        id: program,
                    },
                },
                FeathersGraphViewport::clipboard_fixture(
                    material_graph_view_key(program),
                    Vec2::new(20.0, 10.0),
                    2.0,
                ),
                ComputedNode {
                    size: Vec2::new(400.0, 200.0),
                    ..default()
                },
                UiGlobalTransform::from(Affine2::from_translation(Vec2::new(300.0, 200.0))),
            ))
            .id();
        let canvas = app.world_mut().spawn(ChildOf(viewport)).id();
        Self {
            global: false,
            observed: default(),
            app,
            window,
            root,
            viewport,
            canvas,
            program,
            scope,
            effect,
        }
    }
    fn click(&mut self, entity: Entity, position: Vec2, button: PointerButton) {
        // Guard tests isolate one delivery while keeping real event propagation.
        // Explicit global cases below exercise repeated ancestor delivery too.
        if !self.global && self.observed.insert(entity) {
            self.app
                .world_mut()
                .entity_mut(entity)
                .observe(open_material_graph_palette)
                .observe(open_material_graph_node_menu)
                .observe(open_material_graph_pin_menu)
                .observe(select_material_graph_canvas);
        }
        let event = click_input::click(entity, self.window, position, button);
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn secondary(&mut self, entity: Entity) {
        self.click(entity, Vec2::new(180.0, 150.0), PointerButton::Secondary);
    }
    fn global() -> Self {
        let mut fixture = Self::new();
        fixture.global = true;
        fixture
            .app
            .add_observer(open_material_graph_palette)
            .add_observer(open_material_graph_node_menu)
            .add_observer(open_material_graph_pin_menu)
            .add_observer(select_material_graph_canvas);
        fixture
    }
    fn node(&mut self, expression: MaterialExpressionId, position: Vec2) -> Entity {
        self.app
            .world_mut()
            .spawn((
                ChildOf(self.canvas),
                MaterialGraphAction {
                    program: self.program,
                    expression,
                },
                FeathersGraphNode::clipboard_fixture(
                    material_graph_view_key(self.program),
                    material_graph_expression_node_key(expression),
                    position,
                ),
                ComputedNode {
                    size: Vec2::new(224.0, 80.0),
                    ..default()
                },
                UiGlobalTransform::default(),
            ))
            .id()
    }
    fn socket(
        &mut self,
        node: Entity,
        kind: MaterialGraphSocketKind,
        offset: Option<Vec2>,
    ) -> Entity {
        self.app
            .world_mut()
            .spawn((
                ChildOf(node),
                MaterialGraphSocket {
                    program: self.program,
                    kind,
                },
                MaterialGraphSocketAnchor { node, offset },
                UiGlobalTransform::default(),
            ))
            .id()
    }
    fn wire(&mut self, source: MaterialExpressionId, target: MaterialConnectionTarget) -> Entity {
        self.app
            .world_mut()
            .spawn(MaterialGraphWire {
                program: self.program,
                source,
                target,
                color: Vec4::ONE,
            })
            .id()
    }
    fn line(
        &mut self,
    ) -> (
        MaterialExpressionId,
        MaterialConnectionTarget,
        Entity,
        Entity,
    ) {
        let source = MaterialExpressionId::new();
        let dest = MaterialExpressionId::new();
        let a = self.node(source, Vec2::ZERO);
        let b = self.node(dest, Vec2::new(100.0, 0.0));
        let output = self.socket(
            a,
            MaterialGraphSocketKind::ExpressionOutput(source),
            Some(Vec2::ZERO),
        );
        let target = MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha);
        let input = self.socket(
            b,
            MaterialGraphSocketKind::ConnectionInput(target),
            Some(Vec2::ZERO),
        );
        self.wire(source, target);
        (source, target, output, input)
    }
    fn palette(&self) -> &MaterialGraphPaletteState {
        self.app.world().resource()
    }
    fn revision(&self) -> u64 {
        self.app.world().resource::<EditorSession>().ui_revision
    }
    fn selected(&self) -> &crate::material_graph::selection::ScopeSelection {
        self.app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .get(self.scope)
            .unwrap()
    }
    fn select(&mut self, ids: &[MaterialExpressionId]) {
        self.app
            .world_mut()
            .resource_mut::<MaterialGraphSelectionState>()
            .select_material_expressions(
                self.scope,
                self.program,
                &ids.iter().copied().collect(),
                GraphSelectionMode::Replace,
            );
    }
    fn unchanged(&self) {
        let session = self.app.world().resource::<EditorSession>();
        assert_eq!(session.effect, self.effect);
        assert_eq!(session.operation_order.edit_serial(), 0);
    }
}
#[test]
fn canvas_menu_retains_target_scope_coordinates_and_stops_propagation() {
    let mut f = Fixture::new();
    let revision = f.revision();
    f.secondary(f.canvas);
    assert!(f.app.world().get_entity(f.root).is_ok());
    let open = f.palette().open.as_ref().unwrap();
    assert_eq!(open.program, f.program);
    assert_eq!(open.scope, f.scope);
    assert_eq!(
        open.editing_target,
        MaterialEditingTarget::Program {
            root: "standalone".into(),
            id: f.program
        }
    );
    assert_eq!(open.menu_position, Vec2::new(80.0, 50.0));
    assert_eq!(open.graph_position, Vec2::new(30.0, 20.0));
    assert_eq!(open.graph_key, material_graph_view_key(f.program));
    assert!(open.connection.is_none());
    assert_eq!(f.revision(), revision + 1);
    assert_eq!(f.app.world().resource::<Bubbled>().0, 0);
    f.unchanged();
}
#[test]
fn canvas_menu_clears_competing_menus_and_query_without_clearing_node_selection() {
    let mut f = Fixture::new();
    let id = MaterialExpressionId::new();
    f.select(&[id]);
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphPaletteState>()
        .query = "old".into();
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphPaletteState>()
        .node_menu = Some(MaterialGraphNodeMenuOpen {
        program: f.program,
        scope: f.scope,
        menu_position: Vec2::ZERO,
    });
    f.secondary(f.canvas);
    assert!(
        f.palette().node_menu.is_none()
            && f.palette().connection_menu.is_none()
            && f.palette().query.is_empty()
    );
    assert!(f.selected().expressions.contains(&id));
    f.unchanged();
}
#[test]
fn middle_click_is_unhandled_and_does_not_change_menu_or_revision() {
    let mut f = Fixture::new();
    let revision = f.revision();
    f.click(f.canvas, Vec2::ZERO, PointerButton::Middle);
    assert!(!f.palette().is_open());
    assert_eq!(f.revision(), revision);
    assert_eq!(f.app.world().resource::<Bubbled>().0, 1);
    f.unchanged();
}
#[test]
fn menu_descendants_do_not_reopen_canvas_palette() {
    for kind in 0..4 {
        let mut f = Fixture::new();
        let surface = f.app.world_mut().spawn(ChildOf(f.viewport)).id();
        match kind {
            0 => {
                f.app
                    .world_mut()
                    .entity_mut(surface)
                    .insert(MaterialGraphPalette);
            }
            1 => {
                f.app
                    .world_mut()
                    .entity_mut(surface)
                    .insert(MaterialGraphNodeMenu);
            }
            2 => {
                f.app
                    .world_mut()
                    .entity_mut(surface)
                    .insert(MaterialGraphConnectionMenu);
            }
            _ => {
                f.app
                    .world_mut()
                    .entity_mut(surface)
                    .insert(PointerContextSubmenuSurface);
            }
        }
        let child = f.app.world_mut().spawn(ChildOf(surface)).id();
        let revision = f.revision();
        f.secondary(child);
        assert!(!f.palette().is_open());
        assert_eq!(f.revision(), revision);
        f.unchanged();
    }
}
#[test]
fn node_descendant_opens_scoped_menu_selecting_only_unselected_node() {
    let mut f = Fixture::new();
    let old = MaterialExpressionId::new();
    let id = MaterialExpressionId::new();
    f.select(&[old]);
    let node = f.node(id, Vec2::ZERO);
    let child = f.app.world_mut().spawn(ChildOf(node)).id();
    f.secondary(child);
    let menu = f.palette().node_menu.as_ref().unwrap();
    assert_eq!(menu.program, f.program);
    assert_eq!(menu.scope, f.scope);
    assert_eq!(menu.menu_position, Vec2::new(80.0, 50.0));
    assert_eq!(f.selected().expressions, [id].into_iter().collect());
    assert_eq!(
        f.app
            .world()
            .resource::<MaterialStackInspectorState>()
            .selected,
        Some((f.program, id))
    );
    assert_eq!(f.app.world().resource::<Bubbled>().0, 0);
    f.unchanged();
}
#[test]
fn selected_node_preserves_multi_selection_and_other_view() {
    let mut f = Fixture::new();
    let a = MaterialExpressionId::new();
    let b = MaterialExpressionId::new();
    f.select(&[a, b]);
    let other = Some(EditorViewId(99));
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .select_single(other, f.program, b);
    let node = f.node(a, Vec2::ZERO);
    f.secondary(node);
    assert_eq!(f.selected().expressions, [a, b].into_iter().collect());
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .is_expression_selected(other, f.program, b)
    );
    f.unchanged();
}
#[test]
fn node_with_mismatched_program_cannot_open_a_menu() {
    let mut f = Fixture::new();
    let node = f.node(MaterialExpressionId::new(), Vec2::ZERO);
    f.app
        .world_mut()
        .get_mut::<MaterialGraphAction>(node)
        .unwrap()
        .program = MaterialProgramId::new();
    let revision = f.revision();
    f.secondary(node);
    assert!(!f.palette().is_open());
    assert_eq!(f.revision(), revision);
    f.unchanged();
}
#[test]
fn detached_node_or_socket_never_manufactures_a_viewport() {
    let mut f = Fixture::new();
    let id = MaterialExpressionId::new();
    let node = f.node(id, Vec2::ZERO);
    f.app.world_mut().entity_mut(node).remove::<ChildOf>();
    let socket = f.socket(
        node,
        MaterialGraphSocketKind::ExpressionOutput(id),
        Some(Vec2::ZERO),
    );
    f.secondary(node);
    f.secondary(socket);
    assert!(!f.palette().is_open());
    f.unchanged();
}
#[test]
fn unconnected_pin_does_not_fall_through_to_node_or_canvas_menu() {
    let mut f = Fixture::new();
    let id = MaterialExpressionId::new();
    let node = f.node(id, Vec2::ZERO);
    let pin = f.socket(
        node,
        MaterialGraphSocketKind::ExpressionOutput(id),
        Some(Vec2::ZERO),
    );
    let leaf = f.app.world_mut().spawn(ChildOf(pin)).id();
    let revision = f.revision();
    f.secondary(leaf);
    assert!(!f.palette().is_open());
    assert_eq!(f.revision(), revision);
    f.unchanged();
}
#[test]
fn connected_input_pin_opens_exact_connection_and_clears_node_selection() {
    let mut f = Fixture::new();
    let (source, target, _, input) = f.line();
    f.select(&[source]);
    let leaf = f.app.world_mut().spawn(ChildOf(input)).id();
    f.secondary(leaf);
    let menu = f.palette().connection_menu.as_ref().unwrap();
    let connection = MaterialGraphConnection {
        program: f.program,
        source,
        target,
    };
    assert_eq!(menu.connections, vec![connection]);
    assert_eq!(menu.scope, f.scope);
    assert_eq!(menu.menu_position, Vec2::new(80.0, 50.0));
    assert_eq!(f.selected().connection, Some(connection));
    assert!(f.selected().expressions.is_empty());
    assert!(
        f.app
            .world()
            .resource::<MaterialStackInspectorState>()
            .selected
            .is_none()
    );
    assert!(f.palette().node_menu.is_none() && f.palette().open.is_none());
    assert_eq!(f.app.world().resource::<Bubbled>().0, 0);
    f.unchanged();
}
#[test]
fn output_pin_collects_all_matching_connections_and_ignores_foreign_program() {
    let mut f = Fixture::new();
    let (source, target, output, _) = f.line();
    let second = MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color);
    f.wire(source, second);
    let foreign = f.wire(source, target);
    assert_eq!(
        f.app
            .world()
            .get::<MaterialGraphWire>(foreign)
            .unwrap()
            .color,
        Vec4::ONE
    );
    f.app
        .world_mut()
        .get_mut::<MaterialGraphWire>(foreign)
        .unwrap()
        .program = MaterialProgramId::new();
    f.secondary(output);
    let menu = f.palette().connection_menu.as_ref().unwrap();
    assert_eq!(menu.connections.len(), 2);
    assert!(menu.connections.iter().any(|c| c.target == target));
    assert!(menu.connections.iter().any(|c| c.target == second));
    f.unchanged();
}
#[test]
fn pin_with_mismatched_program_is_unhandled() {
    let mut f = Fixture::new();
    let (_, _, _, input) = f.line();
    f.app
        .world_mut()
        .get_mut::<MaterialGraphSocket>(input)
        .unwrap()
        .program = MaterialProgramId::new();
    let revision = f.revision();
    f.secondary(input);
    assert!(!f.palette().is_open());
    assert_eq!(f.revision(), revision);
    f.unchanged();
}
#[test]
fn right_click_wire_uses_actual_bezier_pick_and_opens_connection_menu() {
    let mut f = Fixture::new();
    let (source, target, _, _) = f.line();
    f.click(f.canvas, Vec2::new(220.0, 110.0), PointerButton::Secondary);
    let connection = MaterialGraphConnection {
        program: f.program,
        source,
        target,
    };
    assert_eq!(
        f.palette().connection_menu.as_ref().unwrap().connections,
        vec![connection]
    );
    assert_eq!(f.selected().connection, Some(connection));
    assert!(f.palette().open.is_none());
    f.unchanged();
}
#[test]
fn outside_wire_pick_radius_opens_add_node_instead() {
    let mut f = Fixture::new();
    f.line();
    f.click(f.canvas, Vec2::new(220.0, 120.0), PointerButton::Secondary);
    assert!(f.palette().open.is_some());
    assert!(f.palette().connection_menu.is_none());
    f.unchanged();
}
#[test]
fn primary_wire_click_selects_without_opening_a_menu_and_repeated_click_is_idle() {
    let mut f = Fixture::new();
    let (source, target, _, _) = f.line();
    let revision = f.revision();
    f.click(f.canvas, Vec2::new(220.0, 110.0), PointerButton::Primary);
    assert_eq!(
        f.selected().connection,
        Some(MaterialGraphConnection {
            program: f.program,
            source,
            target
        })
    );
    assert!(!f.palette().is_open());
    assert_eq!(f.revision(), revision + 1);
    f.click(f.canvas, Vec2::new(220.0, 110.0), PointerButton::Primary);
    assert_eq!(f.revision(), revision + 1);
    assert_eq!(f.app.world().resource::<Bubbled>().0, 0);
    f.unchanged();
}
#[test]
fn empty_primary_canvas_clears_selection_and_inspector_only_in_own_scope() {
    let mut f = Fixture::new();
    let id = MaterialExpressionId::new();
    f.select(&[id]);
    f.app
        .world_mut()
        .resource_mut::<MaterialStackInspectorState>()
        .selected = Some((f.program, id));
    let other = Some(EditorViewId(99));
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .select_single(other, f.program, id);
    f.click(f.canvas, Vec2::new(180.0, 150.0), PointerButton::Primary);
    assert!(f.selected().expressions.is_empty() && f.selected().connection.is_none());
    assert!(
        f.app
            .world()
            .resource::<MaterialStackInspectorState>()
            .selected
            .is_none()
    );
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .is_expression_selected(other, f.program, id)
    );
    f.unchanged();
}
#[test]
fn primary_click_in_node_or_menu_does_not_clear_selection() {
    for menu in [false, true] {
        let mut f = Fixture::new();
        let id = MaterialExpressionId::new();
        f.select(&[id]);
        let target = if menu {
            f.app
                .world_mut()
                .spawn((ChildOf(f.viewport), MaterialGraphNodeMenu))
                .id()
        } else {
            f.node(id, Vec2::ZERO)
        };
        let revision = f.revision();
        f.click(target, Vec2::new(180.0, 150.0), PointerButton::Primary);
        assert!(f.selected().expressions.contains(&id));
        assert_eq!(f.revision(), revision);
        f.unchanged();
    }
}
#[test]
fn socket_geometry_uses_original_measured_fallback_and_anchor_offsets() {
    let mut f = Fixture::new();
    let a = MaterialExpressionId::new();
    let node = f.node(a, Vec2::new(10.0, 20.0));
    let output = f.socket(
        node,
        MaterialGraphSocketKind::ExpressionOutput(a),
        Some(Vec2::new(224.0, 35.0)),
    );
    f.app
        .world_mut()
        .entity_mut(output)
        .insert(UiGlobalTransform::from(Affine2::from_translation(
            Vec2::new(123.0, 0.0),
        )));
    f.app.world_mut().run_system_once(move |sockets:Query<(&MaterialGraphSocket,&UiGlobalTransform,&MaterialGraphSocketAnchor)>,nodes:Query<(&FeathersGraphNode,&ComputedNode,&UiGlobalTransform)>|{
 let positions=collect_socket_positions(&sockets,&nodes);
 assert_eq!(positions.len(),1);let socket=positions[0];
 assert_eq!(socket.graph,Vec2::new(234.0,55.0));assert_eq!(socket.world,Vec2::new(123.0,0.0));assert!(socket.interactive);
 assert_eq!(socket_graph_position(&positions,f.program,MaterialGraphSocketKind::ExpressionOutput(a)),Some(socket.graph));
 }).unwrap();
    f.unchanged();
}
#[test]
fn local_position_accounts_for_transform_and_inverse_scale() {
    let computed = ComputedNode {
        size: Vec2::new(400.0, 200.0),
        inverse_scale_factor: 0.5,
        ..default()
    };
    let transform = UiGlobalTransform::from(Affine2::from_translation(Vec2::new(300.0, 200.0)));
    assert_eq!(
        viewport_local_position(&computed, &transform, Vec2::new(340.0, 220.0)),
        Vec2::new(120.0, 60.0)
    );
    assert!(distance_to_graph_wire(Vec2::new(50.0, 0.0), Vec2::ZERO, Vec2::new(100.0, 0.0)) < 0.01);
}

#[test]
fn global_delivery_preserves_existing_unconnected_pin_ancestor_fallback() {
    let mut f = Fixture::global();
    let id = MaterialExpressionId::new();
    let node = f.node(id, Vec2::ZERO);
    let pin = f.socket(
        node,
        MaterialGraphSocketKind::ExpressionOutput(id),
        Some(Vec2::ZERO),
    );
    f.secondary(pin);
    // The pin handler declines; a later global delivery can open the owning node.
    // Native scene blockers/full-plugin acceptance remain separate from this parity gate.
    assert!(f.palette().connection_menu.is_none());
    assert_eq!(f.palette().node_menu.as_ref().unwrap().program, f.program);
    assert!(f.selected().expressions.contains(&id));
    f.unchanged();
}

#[test]
fn global_delivery_preserves_existing_marker_only_popup_ancestor_fallback() {
    let mut f = Fixture::global();
    let surface = f
        .app
        .world_mut()
        .spawn((ChildOf(f.viewport), MaterialGraphNodeMenu))
        .id();
    f.secondary(surface);
    // A marker alone is not the real popup's event-blocking composition.
    assert!(f.palette().open.is_some());
    f.unchanged();
}

#[test]
fn socket_fallback_uses_measured_node_geometry_and_skips_missing_anchors() {
    let mut f = Fixture::new();
    let source = MaterialExpressionId::new();
    let node = f.node(source, Vec2::new(10.0, 20.0));
    let socket = f.socket(
        node,
        MaterialGraphSocketKind::ExpressionOutput(source),
        None,
    );
    f.app
        .world_mut()
        .entity_mut(socket)
        .insert(UiGlobalTransform::from(Affine2::from_translation(
            Vec2::new(123.0, 0.0),
        )));
    f.app.world_mut().spawn((
        MaterialGraphSocket {
            program: f.program,
            kind: MaterialGraphSocketKind::ExpressionOutput(MaterialExpressionId::new()),
        },
        MaterialGraphSocketAnchor {
            node: Entity::PLACEHOLDER,
            offset: Some(Vec2::ZERO),
        },
        UiGlobalTransform::default(),
    ));
    let program = f.program;
    f.app.world_mut().run_system_once(move |sockets:Query<(&MaterialGraphSocket,&UiGlobalTransform,&MaterialGraphSocketAnchor)>, nodes:Query<(&FeathersGraphNode,&ComputedNode,&UiGlobalTransform)>| {
        let positions = collect_socket_positions(&sockets,&nodes);
        assert_eq!(positions.len(),1);
        assert_eq!(positions[0].graph,Vec2::new(245.0,60.0));
        assert!(socket_graph_position(&positions,MaterialProgramId::new(),MaterialGraphSocketKind::ExpressionOutput(source)).is_none());
        assert!(socket_graph_position(&positions,program,MaterialGraphSocketKind::ExpressionOutput(MaterialExpressionId::new())).is_none());
    }).unwrap();
}

#[test]
fn canvas_opening_uses_owning_view_not_another_view_of_same_program() {
    let mut f = Fixture::new();
    let scope = Some(EditorViewId(88));
    let second = f
        .app
        .world_mut()
        .spawn((
            ChildOf(f.root),
            MaterialGraphViewport {
                program: f.program,
                scope,
                editing_target: MaterialEditingTarget::EffectInstance,
            },
            FeathersGraphViewport::clipboard_fixture(
                material_graph_view_key(f.program),
                Vec2::new(-20.0, 30.0),
                0.5,
            ),
            ComputedNode {
                size: Vec2::new(200.0, 100.0),
                ..default()
            },
            UiGlobalTransform::from(Affine2::from_translation(Vec2::new(800.0, 400.0))),
        ))
        .id();
    let canvas = f.app.world_mut().spawn(ChildOf(second)).id();
    f.click(canvas, Vec2::new(740.0, 380.0), PointerButton::Secondary);
    let open = f.palette().open.as_ref().unwrap();
    assert_eq!(open.scope, scope);
    assert_eq!(open.editing_target, MaterialEditingTarget::EffectInstance);
    assert_eq!(open.menu_position, Vec2::new(40.0, 30.0));
    assert_eq!(open.graph_position, Vec2::new(120.0, 0.0));
    f.unchanged();
}
