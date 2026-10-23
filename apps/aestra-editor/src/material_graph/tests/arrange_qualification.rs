//! Real measured canvases and the production asynchronous arrangement lifecycle.
use super::*;
use crate::feathers::node_graph::geometry;
use crate::material_document::MaterialEditingTarget;
use crate::material_graph::MaterialGraphPreviewState;
use aestra_core::{
    MaterialExpressionId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialProgram,
        MaterialValue,
    },
};
use bevy::ecs::system::RunSystemOnce;
use std::time::{Duration, Instant};

struct Fixture {
    app: App,
    root: tempfile::TempDir,
    viewport: Entity,
    view: GraphViewKey,
    target: MaterialEditingTarget,
    graph: String,
}
impl Fixture {
    fn function() -> Self {
        let mut function = MaterialFunction::from_ron(include_str!(
            "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
        ))
        .unwrap();
        function.inputs.clear();
        function.custom_wesl = None;
        let [a, b, sum, product] = [10, 20, 30, 40].map(MaterialExpressionId::from_u128);
        function.expressions = vec![
            MaterialExpression {
                id: a,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
            },
            MaterialExpression {
                id: b,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(2.0)),
            },
            MaterialExpression {
                id: sum,
                kind: MaterialExpressionKind::Add(a, b),
            },
            MaterialExpression {
                id: product,
                kind: MaterialExpressionKind::Multiply(sum, b),
            },
        ];
        function.outputs[0].expression = product;
        let (mut app, root, viewport, _, _) =
            crate::material_function_editor::graph::setup_with_registration_for_test(
                1.25,
                0.75,
                function,
                crate::material_function_editor::register,
            );
        // Function composition expects the production panel's vertical toolbar/canvas layout.
        let host = app.world().get::<ChildOf>(viewport).unwrap().parent();
        app.world_mut()
            .get_mut::<Node>(host)
            .unwrap()
            .flex_direction = FlexDirection::Column;
        let target = app
            .world()
            .resource::<EditorSession>()
            .material_target
            .clone();
        Self::finish(app, root, viewport, target)
    }
    fn program() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut program = MaterialProgram::additive_sprite("Asynchronous arrangement");
        program.node_constants = program.expressions.iter().map(|node| node.id).collect();
        let program = program.normalized();
        program
            .save_ron(root.path().join("layout.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let target = MaterialEditingTarget::Program {
            root: root.path().to_owned(),
            id: program.id,
        };
        let (mut app, host) = geometry::tests::layout_app(1.0);
        app.insert_resource(catalog)
            .insert_resource(crate::test_support::session_with_timing_slack())
            .init_resource::<crate::history::MaterialProgramEditHistory>()
            .init_resource::<crate::history::EditorHistoryLedger>()
            .init_resource::<crate::material_function_editor::FunctionEditor>()
            .add_observer(crate::history::execute_history_action);
        let target_for_canvas = target.clone();
        app.world_mut()
            .run_system_once(
                move |mut commands: Commands,
                      assets: Res<AssetServer>,
                      catalog: Res<ProjectEffectCatalog>,
                      memory: Res<GraphViewportMemory>| {
                    let compiled = MaterialCompiler.compile(&program).ok();
                    let projection = MaterialCompiler.project_graph(&program, compiled.as_ref());
                    commands.entity(host).with_children(|parent| {
                        crate::material_graph::composition::spawn_material_graph_canvas(
                            parent,
                            catalog.root(),
                            &projection,
                            &program,
                            &target_for_canvas,
                            Some(crate::docking::EditorViewId(8)),
                            "async-program-view",
                            None,
                            &default(),
                            &default(),
                            &memory,
                            &crate::Localizer::new("en-US").unwrap(),
                            &assets,
                        );
                    });
                },
            )
            .unwrap();
        let viewport = app
            .world_mut()
            .query_filtered::<Entity, With<GraphGeometryView>>()
            .single(app.world())
            .unwrap();
        Self::finish(app, root, viewport, target)
    }
    fn finish(
        mut app: App,
        root: tempfile::TempDir,
        viewport: Entity,
        target: MaterialEditingTarget,
    ) -> Self {
        app.init_resource::<MaterialGraphPreviewState>();
        crate::material_graph::presentation::register(&mut app);
        register(&mut app);
        for _ in 0..8 {
            app.update();
        }
        let view = app
            .world()
            .get::<GraphGeometryView>(viewport)
            .unwrap()
            .key
            .clone();
        let graph = match view.document.asset {
            DocumentKey::MaterialProgram(id) => material_graph_view_key(id),
            DocumentKey::MaterialFunction(id) => function_graph_memory_key(root.path(), id),
            _ => unreachable!(),
        };
        let snapshot = app
            .world()
            .resource::<GraphGeometryRegistry>()
            .mounted_view_snapshot(&view, viewport)
            .unwrap_or_else(|| {
                panic!(
                    "Actual composed graph did not become measurable: {:#?}",
                    app.world().resource::<GraphGeometryRegistry>()
                )
            })
            .clone();
        // Exercise explicitly positioned graphs. Bootstrap geometry is not assumed to be saved.
        for (index, key) in snapshot.nodes.keys().enumerate() {
            app.world_mut()
                .resource_mut::<GraphViewportMemory>()
                .set_node(
                    &graph,
                    memory_key(view.document.asset, *key).unwrap(),
                    Vec2::new(index as f32 * 800.0, 100.0),
                    false,
                );
        }
        for _ in 0..4 {
            app.update();
        }
        let mut fixture = Self {
            app,
            root,
            viewport,
            view,
            target,
            graph,
        };
        fixture.settle_geometry();
        fixture
    }
    fn request(&mut self, scope: ArrangeScope, seeds: BTreeSet<GraphNodeKey>) {
        self.app.world_mut().trigger(ArrangeGraph {
            view: self.view.clone(),
            editing_target: self.target.clone(),
            scope,
            seeds,
        });
        self.app.world_mut().flush();
    }
    fn complete(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.app.world().resource::<ArrangeState>().job.is_some() {
            assert!(
                Instant::now() < deadline,
                "actual asynchronous layout did not complete"
            );
            self.app.update();
            std::thread::yield_now();
        }
    }
    fn complete_successfully(&mut self) {
        let revision = self.app.world().resource::<EditorSession>().ui_revision;
        self.complete();
        let session = self.app.world().resource::<EditorSession>();
        assert!(
            session.ui_revision > revision,
            "the result installer did not accept the worker output: {}",
            session.status
        );
        assert!(
            !["failed", "ignored", "stale"]
                .iter()
                .any(|word| session.status.contains(word)),
            "{}",
            session.status
        );
        // A finished worker has installed placement memory, not necessarily measured the new
        // canvas yet. Wait for that layout before another request: otherwise task timing decides
        // whether its input snapshot predates the first arrangement's node synchronization.
        self.settle_geometry();
    }
    fn settle_geometry(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut previous = None;
        loop {
            assert!(Instant::now() < deadline, "arranged canvas did not settle");
            self.app.update();
            let registry = self.app.world().resource::<GraphGeometryRegistry>();
            let memory = self.app.world().resource::<GraphViewportMemory>();
            let ready = registry
                .mounted_view_snapshot(&self.view, self.viewport)
                .filter(|snapshot| {
                    snapshot.nodes.iter().all(|(key, node)| {
                        memory_key(self.view.document.asset, *key).is_ok_and(|key| {
                            memory.node_position(&self.graph, &key) == Some(node.effective_position)
                        })
                    })
                })
                .map(|snapshot| snapshot.geometry_revision);
            if ready.is_some() && ready == previous {
                return;
            }
            previous = ready;
            std::thread::yield_now();
        }
    }
    fn positions(&self) -> BTreeMap<String, (Vec2, bool)> {
        self.app
            .world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&self.graph)
    }
    fn serial(&self) -> u64 {
        self.app
            .world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial()
    }
    fn step(&mut self, undo: bool) {
        self.app.world_mut().trigger(if undo {
            crate::history::HistoryAction::Undo
        } else {
            crate::history::HistoryAction::Redo
        });
        self.app.world_mut().flush();
    }
    fn poll_after_geometry_update(&mut self) {
        // Real layout and collection, without running the coordinator's Update poll yet.
        for _ in 0..4 {
            self.app.world_mut().run_schedule(PostUpdate);
        }
        self.complete();
    }
}

#[test]
fn registered_async_full_layout_uses_real_measurements_and_one_global_undo() {
    for mut f in [Fixture::program(), Fixture::function()] {
        let before = f.positions();
        let effect = f.app.world().resource::<EditorSession>().effect.clone();
        let document_revision = f
            .app
            .world()
            .resource::<EditorSession>()
            .document_revision();
        let content_revision = f
            .app
            .world()
            .resource::<ProjectEffectCatalog>()
            .content_revision();
        let order = f.serial();
        f.app
            .world_mut()
            .resource_mut::<EditorSession>()
            .return_to_effect_material();
        f.request(ArrangeScope::Graph, default());
        assert!(f.app.world().resource::<ArrangeState>().job.is_some());
        f.complete_successfully();
        let after = f.positions();
        assert_ne!(after, before);
        assert_eq!(
            after.keys().collect::<Vec<_>>(),
            before.keys().collect::<Vec<_>>()
        );
        assert!(after.values().all(|(p, _)| p.is_finite()));
        assert_eq!(f.serial(), order + 1);
        assert_eq!(
            f.app.world().resource::<EditorSession>().material_target,
            f.target
        );
        f.step(true);
        assert_eq!(f.positions(), before);
        f.step(false);
        assert_eq!(f.positions(), after);
        assert_eq!(f.app.world().resource::<EditorSession>().effect, effect);
        assert_eq!(
            f.app
                .world()
                .resource::<EditorSession>()
                .document_revision(),
            document_revision
        );
        assert_eq!(
            f.app
                .world()
                .resource::<ProjectEffectCatalog>()
                .content_revision(),
            content_revision
        );
    }
}

#[test]
fn selection_upstream_and_downstream_async_layout_freeze_every_node_outside_the_region() {
    for scope in [
        ArrangeScope::Selection,
        ArrangeScope::Upstream,
        ArrangeScope::Downstream,
    ] {
        let mut f = Fixture::function();
        let before = f.positions();
        f.request(
            scope,
            BTreeSet::from([GraphNodeKey::Expression(MaterialExpressionId::from_u128(
                30,
            ))]),
        );
        let job = f
            .app
            .world()
            .resource::<ArrangeState>()
            .job
            .as_ref()
            .unwrap();
        let GraphLayoutRegion::Nodes(region) = &job.adapter.input.region else {
            panic!("partial request must retain its region")
        };
        let movable = f
            .app
            .world()
            .get::<GraphGeometryView>(f.viewport)
            .unwrap()
            .nodes
            .iter()
            .filter(|key| region.contains(&job.adapter.layout_key(**key).unwrap()))
            .map(|key| memory_key(f.view.document.asset, *key).unwrap())
            .collect::<BTreeSet<_>>();
        assert!(!movable.is_empty());
        assert!(movable.len() < before.len());
        f.complete_successfully();
        assert!(
            !f.app
                .world()
                .resource::<EditorSession>()
                .status
                .contains("failed")
        );
        let after = f.positions();
        for (node, position) in &before {
            if !movable.contains(node) {
                assert_eq!(after[node], *position);
            }
        }
        if after != before {
            f.step(true);
            assert_eq!(f.positions(), before);
        }
    }
}

#[test]
fn full_async_layout_respects_pinned_anchors_and_preserves_camera_and_collapse() {
    let mut f = Fixture::function();
    let camera = format!("{}#view:9", f.graph);
    f.app
        .world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_view(&camera, Vec2::new(35.0, 25.0), 0.75);
    let anchor = f.positions()["outputs"].0;
    f.app
        .world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&f.graph, "outputs", anchor, true);
    f.app
        .world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_pinned(&f.graph, "outputs", true);
    for _ in 0..4 {
        f.app.update();
    }
    let before = f.positions();
    f.request(ArrangeScope::Graph, default());
    assert!(matches!(
        f.app
            .world()
            .resource::<ArrangeState>()
            .job
            .as_ref()
            .unwrap()
            .adapter
            .input
            .region,
        GraphLayoutRegion::Nodes(_)
    ));
    f.complete_successfully();
    let memory = f.app.world().resource::<GraphViewportMemory>();
    assert_eq!(memory.node(&f.graph, "outputs"), Some((anchor, true)));
    assert!(memory.is_pinned(&f.graph, "outputs"));
    assert_eq!(memory.view(&camera), Some((Vec2::new(35.0, 25.0), 0.75)));
    assert!(
        !f.app
            .world()
            .resource::<EditorSession>()
            .status
            .contains("failed")
    );
    if f.positions() != before {
        f.step(true);
        assert_eq!(f.positions(), before);
    }
}

#[test]
fn a_second_request_cannot_replace_an_inflight_task_and_idle_state_accepts_the_next() {
    let mut f = Fixture::program();
    let order = f.serial();
    f.request(ArrangeScope::Graph, default());
    let viewport = f
        .app
        .world()
        .resource::<ArrangeState>()
        .job
        .as_ref()
        .unwrap()
        .viewport;
    f.request(ArrangeScope::Selection, default());
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .contains("already running")
    );
    assert_eq!(
        f.app
            .world()
            .resource::<ArrangeState>()
            .job
            .as_ref()
            .unwrap()
            .viewport,
        viewport
    );
    assert!(matches!(
        f.app
            .world()
            .resource::<ArrangeState>()
            .job
            .as_ref()
            .unwrap()
            .scope,
        ArrangeScope::Graph
    ));
    f.complete_successfully();
    assert_eq!(f.serial(), order + 1);
    f.request(ArrangeScope::Graph, default());
    assert!(f.app.world().resource::<ArrangeState>().job.is_some());
    f.complete_successfully();
    assert_eq!(
        f.serial(),
        order + 1,
        "an identical second arrangement is a no-op"
    );
}

#[test]
fn invalid_requests_fail_before_spawning_without_changing_placement_or_history() {
    for invalid in 0..6 {
        let mut f = Fixture::program();
        let before = f.positions();
        let order = f.serial();
        let mut request = ArrangeGraph {
            view: f.view.clone(),
            editing_target: f.target.clone(),
            scope: ArrangeScope::Graph,
            seeds: default(),
        };
        match invalid {
            0 => {
                request.view.document.project = f.root.path().join("different-project");
            }
            1 => {
                request.view.view = Some(crate::docking::EditorViewId(999));
            }
            2 => {
                f.app.insert_resource(GraphGeometryRegistry::default());
            }
            3 => {
                request.editing_target = MaterialEditingTarget::Program {
                    root: f.root.path().to_owned(),
                    id: MaterialProgramId::new(),
                };
            }
            4 => {
                request.scope = ArrangeScope::Selection;
            }
            _ => {
                request.scope = ArrangeScope::Selection;
                request
                    .seeds
                    .insert(GraphNodeKey::Expression(MaterialExpressionId::new()));
            }
        }
        f.app.world_mut().trigger(request);
        assert!(f.app.world().resource::<ArrangeState>().job.is_none());
        assert!(
            f.app
                .world()
                .resource::<EditorSession>()
                .status
                .contains("unavailable")
        );
        assert_eq!(f.positions(), before);
        assert_eq!(f.serial(), order);
    }
}

#[test]
fn placement_or_pin_changes_before_poll_reject_a_completed_worker_result() {
    for pin in [false, true] {
        let mut f = Fixture::program();
        f.request(ArrangeScope::Graph, default());
        let order = f.serial();
        if pin {
            f.app
                .world_mut()
                .resource_mut::<GraphViewportMemory>()
                .set_pinned(&f.graph, "output", true);
        } else {
            f.app
                .world_mut()
                .resource_mut::<GraphViewportMemory>()
                .set_node(&f.graph, "output", Vec2::splat(2000.0), false);
        }
        let manual = f.positions();
        f.complete();
        assert_eq!(f.positions(), manual);
        assert_eq!(
            f.app
                .world()
                .resource::<GraphViewportMemory>()
                .is_pinned(&f.graph, "output"),
            pin
        );
        assert!(
            f.app
                .world()
                .resource::<EditorSession>()
                .status
                .contains("graph changed")
        );
        assert_eq!(f.serial(), order);
    }
}

#[test]
fn a_semantic_change_after_dispatch_is_rejected_by_the_actual_result_installer() {
    let mut f = Fixture::program();
    f.request(ArrangeScope::Graph, default());
    let before = f.positions();
    let order = f.serial();
    let program = f
        .app
        .world()
        .resource::<ProjectEffectCatalog>()
        .material_program(f.target.program().unwrap())
        .unwrap();
    let mut changed = program.clone();
    changed.name.push_str(" newer draft");
    f.app
        .world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(&program, &changed)
        .unwrap();
    f.complete();
    assert_eq!(f.positions(), before);
    assert_eq!(f.serial(), order);
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .contains("stale")
    );
    assert_eq!(
        f.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_program(program.id)
            .unwrap(),
        changed
    );
}

#[test]
fn retired_or_remeasured_views_invalidate_the_captured_geometry_before_installation() {
    for retired in [false, true] {
        let mut f = Fixture::function();
        f.request(ArrangeScope::Graph, default());
        let before = f.positions();
        let order = f.serial();
        if retired {
            f.app.world_mut().entity_mut(f.viewport).despawn();
        } else {
            let node = f
                .app
                .world_mut()
                .query::<(Entity, &geometry::GraphGeometryNode)>()
                .iter(f.app.world())
                .find(|(_, node)| node.key == GraphNodeKey::FunctionOutputs)
                .unwrap()
                .0;
            f.app.world_mut().get_mut::<Node>(node).unwrap().height = px(600.0);
        }
        f.poll_after_geometry_update();
        assert_eq!(f.positions(), before);
        assert_eq!(f.serial(), order);
        assert!(
            f.app
                .world()
                .resource::<EditorSession>()
                .status
                .contains("graph changed")
        );
    }
}
