//! Full production registrar, actual composed sockets, native events and real wire assets.
use super::*;
use crate::feathers::node_graph::GraphViewportMemory;
use crate::material_function_editor::{
    FunctionEditor,
    graph::{
        clipboard_context::{FunctionGraphNodeAction, ViewScope},
        menu_state::FunctionGraphMenuState,
        model::{BodyAction, BodyActionKind, Constant},
        socket_palette::tests::setup_with_registration,
    },
};
use crate::material_graph::{
    MaterialGraphSelectionState, click_input as click, drag_input::tests as drag,
};
use crate::{EditorSession, ProjectEffectCatalog};
use aestra_core::{
    MaterialExpressionId,
    material::{MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialValue},
};
use bevy::{
    ecs::system::RunSystemOnce,
    picking::pointer::PointerButton,
    ui::RelativeCursorPosition,
    ui_widgets::{Activate, ValueChange},
};

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
    fn new(scale: f32, zoom: f32, assets: bool) -> Self {
        let original = function();
        let (mut app, root, viewport, _, target) =
            setup_with_registration(scale, zoom, original.clone(), |app| {
                // The full production root must own these resources, not the focused fixture.
                app.world_mut().remove_resource::<ConnectionPreview>();
                app.world_mut().remove_resource::<GraphViewportMemory>();
                app.world_mut()
                    .remove_resource::<MaterialGraphSelectionState>();
                app.world_mut().remove_resource::<FunctionEditor>();
                crate::material_function_editor::register(app);
            });
        let unused = original
            .expressions
            .iter()
            .find(|e| e.id != original.outputs[0].expression)
            .unwrap()
            .id;
        let source = app
            .world_mut()
            .query::<(Entity, &Socket)>()
            .iter(app.world())
            .find(|(_, s)| s.kind == SocketKind::Source(unused))
            .unwrap()
            .0;
        let mut f = Self {
            app,
            root,
            original,
            viewport,
            source,
            target,
        };
        if assets {
            f.app.init_resource::<Assets<GraphWireMaterial>>();
            f.advance();
        }
        f
    }
    fn advance(&mut self) {
        for _ in 0..4 {
            self.app.update();
        }
    }
    fn current(&self) -> MaterialFunction {
        self.app
            .world()
            .resource::<EditorSession>()
            .graph_function(self.app.world().resource::<ProjectEffectCatalog>())
            .unwrap()
    }
    fn point(&self, entity: Entity) -> Vec2 {
        let world = self.app.world();
        world
            .get::<UiGlobalTransform>(entity)
            .unwrap()
            .translation
            .trunc()
            * world
                .get::<ComputedNode>(self.viewport)
                .unwrap()
                .inverse_scale_factor
    }
    fn material(&self, entity: Entity) -> GraphWireMaterial {
        let world = self.app.world();
        let handle = &world
            .get::<MaterialNode<GraphWireMaterial>>(entity)
            .unwrap()
            .0;
        world
            .resource::<Assets<GraphWireMaterial>>()
            .get(handle)
            .unwrap()
            .clone()
    }
    fn wire(&mut self) -> Entity {
        self.app
            .world_mut()
            .query_filtered::<Entity, With<Wire>>()
            .iter(self.app.world())
            .find(|e| belongs(self.app.world(), *e, self.viewport))
            .unwrap()
    }
    fn ghost(&mut self, viewport: Entity) -> Entity {
        self.app
            .world_mut()
            .query_filtered::<Entity, With<PreviewWire>>()
            .iter(self.app.world())
            .find(|e| belongs(self.app.world(), *e, viewport))
            .unwrap()
    }
    fn start(&mut self) {
        let position = self.point(self.source);
        self.app.world_mut().trigger(drag::start(
            self.source,
            Entity::PLACEHOLDER,
            position,
            PointerButton::Primary,
        ));
    }
    fn assert_unchanged(&self) {
        assert_eq!(self.current(), self.original);
        let saved =
            MaterialFunction::load_ron(self.root.path().join("body.aestra.material-function.ron"))
                .unwrap();
        assert_eq!(saved, self.original);
    }
    fn remount(&mut self, pan: Vec2, zoom: f32) {
        let host = self
            .app
            .world()
            .get::<ChildOf>(self.viewport)
            .unwrap()
            .parent();
        self.app.world_mut().entity_mut(host).despawn_children();
        let key =
            crate::material_graph::function_graph_memory_key(self.root.path(), self.original.id);
        self.app
            .world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_view(format!("{key}#view:9"), pan, zoom);
        self.app
            .world_mut()
            .run_system_once(
                move |mut commands: Commands,
                      session: Res<EditorSession>,
                      catalog: Res<ProjectEffectCatalog>,
                      assets: Res<AssetServer>,
                      memory: Res<GraphViewportMemory>,
                      selection: Res<MaterialGraphSelectionState>,
                      menus: Res<FunctionGraphMenuState>| {
                    commands.entity(host).with_children(|parent| {
                        crate::material_function_editor::spawn_graph(
                            parent,
                            &session,
                            &catalog,
                            &assets,
                            &memory,
                            &selection,
                            &menus,
                            &session.material_target,
                            Some(crate::docking::EditorViewId(9)),
                        )
                    });
                },
            )
            .unwrap();
        self.advance();
        self.viewport = self
            .app
            .world_mut()
            .query_filtered::<Entity, With<View>>()
            .single(self.app.world())
            .unwrap();
        self.target = self
            .app
            .world_mut()
            .query::<(Entity, &Socket)>()
            .iter(self.app.world())
            .find(|(_, s)| matches!(s.kind, SocketKind::Target(_)))
            .unwrap()
            .0;
    }
    fn second_view(&mut self) -> Entity {
        let host = self
            .app
            .world()
            .get::<ChildOf>(self.viewport)
            .unwrap()
            .parent();
        let key =
            crate::material_graph::function_graph_memory_key(self.root.path(), self.original.id);
        self.app
            .world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_view(format!("{key}#view:73"), Vec2::new(-45.0, 65.0), 1.4);
        self.app
            .world_mut()
            .run_system_once(
                move |mut commands: Commands,
                      session: Res<EditorSession>,
                      catalog: Res<ProjectEffectCatalog>,
                      assets: Res<AssetServer>,
                      memory: Res<GraphViewportMemory>,
                      selection: Res<MaterialGraphSelectionState>,
                      menus: Res<FunctionGraphMenuState>| {
                    commands.entity(host).with_children(|parent| {
                        crate::material_function_editor::spawn_graph(
                            parent,
                            &session,
                            &catalog,
                            &assets,
                            &memory,
                            &selection,
                            &menus,
                            &session.material_target,
                            Some(crate::docking::EditorViewId(73)),
                        )
                    });
                },
            )
            .unwrap();
        let other = self
            .app
            .world_mut()
            .query::<(Entity, &ViewScope)>()
            .iter(self.app.world())
            .find(|(_, scope)| scope.0 == Some(crate::docking::EditorViewId(73)))
            .unwrap()
            .0;
        for (view, left) in [(self.viewport, 5.0), (other, 480.0)] {
            self.app.world_mut().entity_mut(view).insert(Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(30.0),
                width: px(450.0),
                height: px(450.0),
                ..default()
            });
        }
        self.advance();
        other
    }
}
fn belongs(world: &World, mut entity: Entity, viewport: Entity) -> bool {
    while let Some(parent) = world.get::<ChildOf>(entity) {
        entity = parent.parent();
        if entity == viewport {
            return true;
        }
    }
    false
}
fn close(actual: Vec2, expected: Vec2) {
    assert!(
        actual.distance(expected) < 0.1,
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn complete_registrar_owns_function_resources_without_a_widget_plugin() {
    let mut app = App::new();
    crate::material_function_editor::register(&mut app);
    assert!(app.world().contains_resource::<ConnectionPreview>());
    assert!(app.world().contains_resource::<GraphViewportMemory>());
    assert!(
        app.world()
            .contains_resource::<MaterialGraphSelectionState>()
    );
    assert!(app.world().contains_resource::<FunctionGraphMenuState>());
    assert!(
        app.world()
            .contains_resource::<crate::material_graph::clipboard::GraphClipboard>()
    );
    assert!(app.world().contains_resource::<FunctionEditor>());
}

#[test]
fn missing_optional_wire_assets_recover_and_attach_once() {
    let mut f = Fixture::new(1.25, 0.75, false);
    let entities = f
        .app
        .world_mut()
        .query_filtered::<Entity, Or<(With<Wire>, With<PreviewWire>)>>()
        .iter(f.app.world())
        .collect::<Vec<_>>();
    assert!(!entities.is_empty());
    for e in &entities {
        assert!(
            f.app
                .world()
                .get::<MaterialNode<GraphWireMaterial>>(*e)
                .is_none()
        );
    }
    f.app.init_resource::<Assets<GraphWireMaterial>>();
    f.advance();
    let handles = entities
        .iter()
        .map(|e| {
            f.app
                .world()
                .get::<MaterialNode<GraphWireMaterial>>(*e)
                .unwrap()
                .0
                .id()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        f.app.world().resource::<Assets<GraphWireMaterial>>().len(),
        handles.len()
    );
    f.advance();
    assert_eq!(
        handles,
        entities
            .iter()
            .map(|e| f
                .app
                .world()
                .get::<MaterialNode<GraphWireMaterial>>(*e)
                .unwrap()
                .0
                .id())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        f.app.world().resource::<Assets<GraphWireMaterial>>().len(),
        handles.len()
    );
    f.assert_unchanged();
}

#[test]
fn actual_wire_points_follow_measured_sockets_pan_zoom_and_dpi() {
    for scale in [1.0, 1.25, 2.0] {
        for zoom in [0.5, 1.0] {
            let mut f = Fixture::new(scale, zoom, true);
            for pan in [Vec2::new(10.0, 20.0), Vec2::new(80.0, -30.0)] {
                // Saved camera memory seeds a newly composed viewport; live navigation owns
                // an already-mounted viewport's camera, not subsequent memory writes.
                f.remount(pan, zoom);
                let wire = f.wire();
                let source_id = f.app.world().get::<Wire>(wire).unwrap().source;
                let source = f
                    .app
                    .world_mut()
                    .query::<(Entity, &Socket)>()
                    .iter(f.app.world())
                    .find(|(_, s)| s.kind == SocketKind::Source(source_id))
                    .unwrap()
                    .0;
                let material = f.material(wire);
                for (socket_entity, actual) in [(source, material.start), (f.target, material.end)]
                {
                    let socket = f.app.world().get::<Socket>(socket_entity).unwrap();
                    let node = f.app.world().get::<FeathersGraphNode>(socket.node).unwrap();
                    let offset = f
                        .app
                        .world()
                        .get::<Anchor>(socket_entity)
                        .unwrap()
                        .0
                        .unwrap();
                    close(actual, pan + (node.position() + offset) * zoom);
                    let viewport = f.app.world().get::<UiGlobalTransform>(f.viewport).unwrap();
                    let computed = f.app.world().get::<ComputedNode>(f.viewport).unwrap();
                    let origin = viewport.translation.trunc() - computed.size() * 0.5;
                    // UI layout rounds node/socket pixels independently. Graph projection above
                    // remains exact; the rendered socket center may differ by a physical pixel.
                    let measured = f.point(socket_entity) - origin * computed.inverse_scale_factor;
                    assert!(
                        actual.distance(measured) <= 1.5 * computed.inverse_scale_factor,
                        "{actual:?} != {measured:?} at scale {scale}, zoom {zoom}"
                    );
                }
                assert_eq!(material.width, 2.0);
                assert_eq!(material.inverse_scale, 1.0 / scale);
                assert_eq!(material.color, Vec4::new(0.4, 0.8, 1.0, 1.0));
            }
            f.assert_unchanged();
        }
    }
}

#[test]
fn split_function_views_project_only_their_own_sockets_and_ghosts() {
    let mut f = Fixture::new(1.25, 0.65, true);
    let other = f.second_view();
    let wires = f
        .app
        .world_mut()
        .query_filtered::<Entity, With<Wire>>()
        .iter(f.app.world())
        .collect::<Vec<_>>();
    assert_eq!(wires.len(), 2);
    let a = f.material(wires[0]);
    let b = f.material(wires[1]);
    assert_ne!(a.start, b.start);
    for wire in wires {
        let view = if belongs(f.app.world(), wire, f.viewport) {
            f.viewport
        } else {
            other
        };
        let source = f.app.world().get::<Wire>(wire).unwrap().source;
        let socket = f
            .app
            .world_mut()
            .query::<(Entity, &Socket)>()
            .iter(f.app.world())
            .find(|(_, s)| {
                s.kind == SocketKind::Source(source) && belongs(f.app.world(), s.node, view)
            })
            .unwrap()
            .0;
        let node = f.app.world().get::<Socket>(socket).unwrap().node;
        let p = f
            .app
            .world()
            .get::<FeathersGraphNode>(node)
            .unwrap()
            .position()
            + f.app.world().get::<Anchor>(socket).unwrap().0.unwrap();
        close(
            f.material(wire).start,
            f.app
                .world()
                .get::<FeathersGraphViewport>(view)
                .unwrap()
                .project_graph_point(p),
        );
    }
    f.start();
    f.advance();
    let active = f.ghost(f.viewport);
    let inactive = f.ghost(other);
    assert_eq!(f.material(active).color.w, 1.0);
    assert_eq!(f.material(inactive).color.w, 0.0);
    f.assert_unchanged();
}

#[test]
fn native_drag_snaps_at_logical_radius_and_release_clears_feedback() {
    for scale in [1.0, 1.25, 2.0] {
        let mut f = Fixture::new(scale, 0.75, true);
        f.start();
        let target = f.point(f.target);
        for (offset, snapped) in [(17.0, true), (19.0, false)] {
            f.app.world_mut().trigger(drag::motion(
                f.source,
                Entity::PLACEHOLDER,
                target + Vec2::X * offset,
                PointerButton::Primary,
            ));
            f.advance();
            assert_eq!(
                f.app.world().resource::<ConnectionPreview>().2,
                snapped.then_some(f.target)
            );
            let color = f
                .app
                .world()
                .get::<BackgroundColor>(f.target)
                .unwrap()
                .0
                .to_srgba();
            assert_eq!(color.alpha, if snapped { 0.65 } else { 0.2 });
            let ghost = f.ghost(f.viewport);
            assert_eq!(f.material(ghost).color.w, 1.0);
        }
        f.app.world_mut().trigger(drag::end(
            f.source,
            Entity::PLACEHOLDER,
            target,
            PointerButton::Primary,
        ));
        f.advance();
        assert!(f.app.world().resource::<ConnectionPreview>().0.is_none());
        let ghost = f.ghost(f.viewport);
        assert_eq!(f.material(ghost).color.w, 0.0);
        assert_eq!(
            f.app.world().get::<BackgroundColor>(f.target).unwrap().0,
            Color::NONE
        );
        // Release re-evaluates the pointer and commits a valid connection even if the
        // last PostLayout snapshot was outside the snap radius. Undo restores one edit.
        assert_ne!(f.current(), f.original);
        let SocketKind::Source(source) = f.app.world().get::<Socket>(f.source).unwrap().kind else {
            unreachable!()
        };
        assert_eq!(f.current().outputs[0].expression, source);
        f.app
            .world_mut()
            .trigger(crate::history::HistoryAction::Undo);
        f.app.world_mut().flush();
        f.assert_unchanged();
    }
}

#[test]
fn ghost_direction_and_vanished_origin_cleanup_use_the_registered_post_layout_system() {
    let mut f = Fixture::new(2.0, 0.5, true);
    let cursor = f.point(f.target) + Vec2::new(100.0, 80.0);
    f.app.world_mut().trigger(drag::start(
        f.target,
        Entity::PLACEHOLDER,
        cursor,
        PointerButton::Primary,
    ));
    f.advance();
    let ghost = f.ghost(f.viewport);
    let material = f.material(ghost);
    let computed = f.app.world().get::<ComputedNode>(f.viewport).unwrap();
    let transform = f.app.world().get::<UiGlobalTransform>(f.viewport).unwrap();
    let origin =
        (transform.translation.trunc() - computed.size() * 0.5) * computed.inverse_scale_factor;
    close(material.start, cursor - origin);
    close(material.end, f.point(f.target) - origin);
    f.app.world_mut().entity_mut(f.target).despawn();
    f.advance();
    assert!(f.app.world().resource::<ConnectionPreview>().0.is_none());
    assert!(f.app.world().resource::<ConnectionPreview>().1.is_empty());
    assert_eq!(f.material(ghost).color.w, 0.0);
    f.assert_unchanged();
}

#[test]
fn complete_registrar_keeps_constant_body_and_signature_consumers_distinct_with_real_undo() {
    let mut f = Fixture::new(1.0, 0.75, true);
    let effect = f.app.world().resource::<EditorSession>().effect.clone();
    let name = f
        .app
        .world_mut()
        .spawn(crate::material_function_editor::signature_model::Field {
            owner: f.original.id,
            kind: crate::material_function_editor::signature_model::FieldKind::Name,
        })
        .id();
    f.app.world_mut().trigger(ValueChange {
        source: name,
        value: "Registered signature".to_string(),
        is_final: true,
    });
    f.app.world_mut().flush();
    assert_eq!(f.current().name, "Registered signature");
    f.app
        .world_mut()
        .trigger(crate::history::HistoryAction::Undo);
    f.app.world_mut().flush();
    assert_eq!(f.current(), f.original);
    let control = f
        .app
        .world_mut()
        .query_filtered::<Entity, With<Constant>>()
        .iter(f.app.world())
        .next()
        .unwrap();
    f.app.world_mut().trigger(ValueChange {
        source: control,
        value: 4.5_f32,
        is_final: true,
    });
    f.app.world_mut().flush();
    let changed = f.current();
    assert_ne!(changed, f.original);
    f.app
        .world_mut()
        .trigger(crate::history::HistoryAction::Undo);
    f.app.world_mut().flush();
    assert_eq!(f.current(), f.original);
    let button = f
        .app
        .world_mut()
        .spawn(BodyAction {
            owner: f.original.id,
            scope: Some(crate::docking::EditorViewId(9)),
            kind: BodyActionKind::Create(aestra_compiler::MaterialGraphCreateKind::Constant(
                aestra_core::material::MaterialValueType::Float,
            )),
        })
        .id();
    f.app.world_mut().trigger(Activate { entity: button });
    f.app.world_mut().flush();
    let created = f.current();
    assert_eq!(created.expressions.len(), f.original.expressions.len() + 1);
    f.app
        .world_mut()
        .trigger(crate::history::HistoryAction::Undo);
    f.app.world_mut().flush();
    assert_eq!(f.current(), f.original);
    f.app
        .world_mut()
        .trigger(crate::history::HistoryAction::Redo);
    f.app.world_mut().flush();
    assert_eq!(f.current(), created);
    assert_eq!(f.app.world().resource::<EditorSession>().effect, effect);
}

#[test]
fn complete_registrar_routes_selection_keyboard_clipboard_and_context_dismissal() {
    let mut f = Fixture::new(1.0, 0.75, true);
    let scope = Some(crate::docking::EditorViewId(9));
    let (node, expression) = f
        .app
        .world_mut()
        .query::<(Entity, &FunctionGraphNodeAction)>()
        .iter(f.app.world())
        .next()
        .map(|(e, n)| (e, n.expression))
        .unwrap();
    f.app.world_mut().trigger(click::click(
        node,
        Entity::PLACEHOLDER,
        Vec2::ZERO,
        PointerButton::Primary,
    ));
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .is_function_expression_selected(scope, f.original.id, expression)
    );
    f.app
        .world_mut()
        .entity_mut(f.viewport)
        .insert(RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        });
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyD);
    f.app.update();
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    assert_eq!(
        f.current().expressions.len(),
        f.original.expressions.len() + 1
    );
    f.app
        .world_mut()
        .trigger(crate::history::HistoryAction::Undo);
    f.app.world_mut().flush();
    assert_eq!(f.current(), f.original);
    let p = f.point(f.target);
    f.app.world_mut().trigger(click::click(
        f.target,
        Entity::PLACEHOLDER,
        p,
        PointerButton::Secondary,
    ));
    assert!(
        f.app
            .world()
            .resource::<FunctionGraphMenuState>()
            .open
            .is_some()
    );
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    f.app.update();
    assert!(
        f.app
            .world()
            .resource::<FunctionGraphMenuState>()
            .open
            .is_none()
    );
    f.assert_unchanged();
}
