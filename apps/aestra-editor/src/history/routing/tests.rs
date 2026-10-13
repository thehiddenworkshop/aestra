use super::*;
use crate::{
    document_registry::{DocumentKey, DocumentManager},
    editor_view_registry::{
        ActiveEditorContext, EditorViewKind, EditorViewManager, open_document_view,
    },
    feathers::code_editor::{ActiveCodeEditor, CodeEditor},
    feathers::node_graph::GraphViewportMemory,
    material_function_editor::FunctionEditor,
    material_graph::{keys::material_graph_view_key, presentation::Snapshot},
    material_target::MaterialEditingTarget,
    persistence::DocumentProtectionState,
    project_content::io,
};
use aestra_core::material::MaterialProgram;
use std::fs;

#[derive(Resource, Default)]
struct Requests(Vec<(DeletedSource, bool, u64)>);

// This deliberately observes dispatch, not the native recovery worker's filesystem/UI effects.
fn capture_delete(world: &mut World, item: DeletedSource, undo: bool, revision: u64) {
    assert!(
        world
            .resource::<asset_order::AssetOrder>()
            .matches_delete(undo, revision)
    );
    // These mutable borrows would fail if recovery ran while Routing still borrowed its resources.
    world.resource_mut::<EditorSession>();
    world.resource_mut::<ProjectEffectCatalog>();
    world
        .resource_mut::<Requests>()
        .0
        .push((item, undo, revision));
}

fn observe(action: On<HistoryAction>, routing: Routing, mut commands: Commands) {
    routing.execute(*action, &mut commands, capture_delete);
}

struct Fixture {
    root: tempfile::TempDir,
    app: App,
    programs: [MaterialProgram; 2],
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let programs = [
            MaterialProgram::additive_sprite("First").normalized(),
            MaterialProgram::additive_sprite("Second").normalized(),
        ];
        for (index, program) in programs.iter().enumerate() {
            program
                .save_ron(root.path().join(format!("{index}.aestra.material.ron")))
                .unwrap();
        }
        let mut app = App::new();
        app.insert_resource(crate::test_support::session_with_timing_slack())
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<FunctionEditor>()
            .init_resource::<Requests>()
            .add_observer(observe);
        Self {
            root,
            app,
            programs,
        }
    }
    fn step(&mut self, action: HistoryAction) {
        self.app.world_mut().trigger(action);
        self.app.world_mut().flush();
    }
    fn current(&self, index: usize) -> MaterialProgram {
        self.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_program(self.programs[index].id)
            .unwrap()
    }
    fn target(&self, index: usize) -> MaterialEditingTarget {
        MaterialEditingTarget::Program {
            root: self.root.path().into(),
            id: self.programs[index].id,
        }
    }
    fn select(&mut self, index: usize) {
        let target = self.target(index);
        asset_order::Context::Material(target)
            .select(&mut self.app.world_mut().resource_mut::<EditorSession>());
    }
    fn focus(&mut self, index: usize) {
        let mut documents = DocumentManager::default();
        let mut views = EditorViewManager::default();
        let mut active = ActiveEditorContext::default();
        open_document_view(
            &mut documents,
            &mut views,
            &mut active,
            DocumentKey::MaterialProgram(self.programs[index].id),
            EditorViewKind::MaterialGraph,
        );
        self.app
            .insert_resource(documents)
            .insert_resource(views)
            .insert_resource(active);
    }
    fn edit(&mut self, index: usize, name: &str) {
        let mut session = self
            .app
            .world_mut()
            .remove_resource::<EditorSession>()
            .unwrap();
        let mut catalog = self
            .app
            .world_mut()
            .remove_resource::<ProjectEffectCatalog>()
            .unwrap();
        let before = catalog.material_program(self.programs[index].id).unwrap();
        session.open_material_program(&catalog, before.id).unwrap();
        let mut after = before.clone();
        after.name = name.into();
        self.app
            .world_mut()
            .resource_mut::<MaterialProgramEditHistory>()
            .execute_replacement(&mut session, &mut catalog, name, before, after)
            .unwrap();
        self.app
            .world_mut()
            .resource_mut::<EditorHistoryLedger>()
            .record_material_edit(&mut session);
        self.app.insert_resource(session).insert_resource(catalog);
    }
    fn delete(&mut self) -> DeletedSource {
        fs::write(self.root.path().join("unused.png"), b"exact recovery bytes").unwrap();
        let content = aestra_project::ProjectContent::scan(self.root.path());
        let source = content
            .source_tree()
            .at_relative_path("unused.png")
            .unwrap()
            .id;
        let item = content
            .plan_delete_source(source, &[], true)
            .unwrap()
            .apply()
            .unwrap();
        let mut order = self
            .app
            .world_mut()
            .remove_resource::<asset_order::AssetOrder>()
            .unwrap_or_default();
        order.record(
            item.clone(),
            self.app.world().resource::<EditorSession>(),
            self.app.world().resource::<ProjectEffectCatalog>(),
        );
        self.app.insert_resource(order);
        item
    }
    fn revision(&self) -> u64 {
        self.app
            .world()
            .resource::<asset_order::AssetOrder>()
            .revision
    }
    fn no_requests(&self) {
        assert!(self.app.world().resource::<Requests>().0.is_empty());
    }
}

#[test]
fn effect_round_trip_works_without_optional_ui_order_or_canvas_resources() {
    let mut f = Fixture::new();
    let before = f.app.world().resource::<EditorSession>().effect.clone();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .set_effect_name("Effect edit");
    let after = f.app.world().resource::<EditorSession>().effect.clone();
    f.step(HistoryAction::Undo);
    assert_eq!(f.app.world().resource::<EditorSession>().effect, before);
    f.step(HistoryAction::Redo);
    assert_eq!(f.app.world().resource::<EditorSession>().effect, after);
    f.no_requests();
}

#[test]
fn focused_document_overrides_session_target_but_effect_focus_keeps_effect_history() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .set_effect_name("Effect edit");
    f.edit(0, "First edit");
    f.edit(1, "Second edit");
    f.focus(0);
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(0), f.programs[0]);
    assert_eq!(f.current(1).name, "Second edit");
    f.step(HistoryAction::Redo);
    assert_eq!(f.current(0).name, "First edit");
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .material_history_active = false;
    f.step(HistoryAction::Undo);
    assert_ne!(
        f.app.world().resource::<EditorSession>().effect.name,
        "Effect edit"
    );
    assert_eq!(f.current(0).name, "First edit");
    assert_eq!(f.current(1).name, "Second edit");
    f.no_requests();
}

#[test]
fn stale_view_closed_document_and_mismatched_focus_do_not_override_session_target() {
    for invalid in 0..3 {
        let mut f = Fixture::new();
        f.edit(0, "First edit");
        f.edit(1, "Second edit");
        f.focus(0);
        let active = f.app.world().resource::<ActiveEditorContext>();
        let view = active.active_view.unwrap();
        let document = active.active_document.unwrap();
        match invalid {
            0 => {
                f.app
                    .world_mut()
                    .resource_mut::<EditorViewManager>()
                    .close_view(view);
            }
            1 => {
                f.app
                    .world_mut()
                    .resource_mut::<DocumentManager>()
                    .close(document);
            }
            _ => {
                f.app
                    .world_mut()
                    .resource_mut::<ActiveEditorContext>()
                    .active_document = None;
            }
        }
        f.step(HistoryAction::Undo);
        assert_eq!(f.current(0).name, "First edit");
        assert_eq!(f.current(1), f.programs[1]);
        f.no_requests();
    }
}

#[test]
fn failed_focused_step_restores_previous_target_and_can_retry_without_losing_history() {
    let mut f = Fixture::new();
    f.edit(0, "First edit");
    let edited = f.current(0);
    f.edit(1, "Second edit");
    f.focus(0);
    let previous_target = f
        .app
        .world()
        .resource::<EditorSession>()
        .material_target
        .clone();
    let mut conflict = edited.clone();
    conflict.name = "Newer draft".into();
    f.app
        .world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(&edited, &conflict)
        .unwrap();
    f.step(HistoryAction::Undo);
    let session = f.app.world().resource::<EditorSession>();
    assert_eq!(session.material_target, previous_target);
    assert!(session.material_history_active);
    assert_eq!(f.current(0), conflict);
    assert_eq!(f.current(1).name, "Second edit");
    f.app
        .world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(&conflict, &edited)
        .unwrap();
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(0), f.programs[0]);
    f.step(HistoryAction::Redo);
    assert_eq!(f.current(0), edited);
    assert_eq!(
        MaterialProgram::load_ron(f.root.path().join("0.aestra.material.ron")).unwrap(),
        f.programs[0]
    );
}

#[test]
fn active_code_buffer_precedes_global_filesystem_history_even_with_empty_buffer_stack() {
    let mut f = Fixture::new();
    f.edit(0, "First edit");
    f.delete();
    let revision = f.revision();
    let mut editor = CodeEditor::new("original".into());
    editor.insert_for_test(" edited");
    let entity = f.app.world_mut().spawn(editor).id();
    f.app.insert_resource(ActiveCodeEditor(Some(entity)));
    for (action, expected) in [
        (HistoryAction::Undo, "original"),
        (HistoryAction::Undo, "original"),
        (HistoryAction::Redo, "original edited"),
        (HistoryAction::Redo, "original edited"),
    ] {
        f.step(action);
        assert_eq!(
            f.app.world().get::<CodeEditor>(entity).unwrap().text,
            expected
        );
        assert_eq!(f.revision(), revision);
        assert_eq!(f.current(0).name, "First edit");
        f.no_requests();
    }
    f.app.world_mut().entity_mut(entity).despawn();
    f.step(HistoryAction::Undo);
    assert_eq!(f.app.world().resource::<Requests>().0.len(), 1);
}

#[test]
fn every_modal_branch_blocks_code_global_delete_and_document_history() {
    let mut f = Fixture::new();
    f.edit(0, "First edit");
    f.delete();
    let revision = f.revision();
    let mut editor = CodeEditor::new("original".into());
    editor.insert_for_test(" edited");
    let entity = f.app.world_mut().spawn(editor).id();
    for protection in DocumentProtectionState::blocking_states_for_test() {
        f.app.insert_resource(protection);
        for active in [None, Some(entity)] {
            f.app.insert_resource(ActiveCodeEditor(active));
            f.step(HistoryAction::Undo);
            f.step(HistoryAction::Redo);
            assert_eq!(f.revision(), revision);
            assert_eq!(f.current(0).name, "First edit");
            assert_eq!(
                f.app.world().get::<CodeEditor>(entity).unwrap().text,
                "original edited"
            );
            f.no_requests();
        }
    }
    f.app.insert_resource(DocumentProtectionState::default());
    f.app.insert_resource(ActiveCodeEditor(None));
    f.step(HistoryAction::Undo);
    assert_eq!(f.app.world().resource::<Requests>().0.len(), 1);
}

#[test]
fn unapplied_io_completion_blocks_global_dispatch_then_releases_it() {
    let mut f = Fixture::new();
    f.app
        .insert_resource(crate::localization::Localizer::new("en-US").unwrap());
    f.delete();
    let revision = f.revision();
    let guard = io::IoGuard::capture(
        f.app.world().resource::<ProjectEffectCatalog>(),
        f.app.world().resource::<EditorSession>(),
    );
    io::enqueue(&mut f.app.world_mut().commands(), guard, || {
        io::completion(|_| {})
    });
    let mut completion = io::prepared_completion(f.app.world_mut());
    f.step(HistoryAction::Undo);
    f.step(HistoryAction::Redo);
    f.no_requests();
    assert_eq!(f.revision(), revision);
    completion.apply(f.app.world_mut());
    f.step(HistoryAction::Undo);
    assert_eq!(f.app.world().resource::<Requests>().0.len(), 1);
}

#[test]
fn global_document_order_overrides_focus_and_defers_delete_without_popping_it() {
    let mut f = Fixture::new();
    let item = f.delete();
    f.edit(0, "First edit");
    f.focus(1);
    f.select(1);
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(0), f.programs[0]);
    assert_eq!(
        f.app.world().resource::<EditorSession>().material_target,
        f.target(0)
    );
    f.no_requests();
    let revision = f.revision();
    f.step(HistoryAction::Undo);
    let requests = &f.app.world().resource::<Requests>().0;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0.journal(), item.journal());
    assert!(requests[0].1);
    assert_eq!(requests[0].2, revision);
    assert_eq!(f.revision(), revision);
    assert!(
        f.app
            .world()
            .resource::<asset_order::AssetOrder>()
            .matches_delete(true, revision)
    );
    assert!(!f.root.path().join("unused.png").exists());
}

#[test]
fn empty_global_order_does_not_fall_through_to_retained_local_history() {
    let mut f = Fixture::new();
    let item = f.delete();
    let revision = f.revision();
    // Simulate only the completed journal transition. Native recovery effects are not under test.
    assert!(
        f.app
            .world_mut()
            .resource_mut::<asset_order::AssetOrder>()
            .finish_delete(true, revision, item)
    );
    f.edit(0, "Retained local edit");
    {
        let mut order = f
            .app
            .world_mut()
            .remove_resource::<asset_order::AssetOrder>()
            .unwrap();
        order.sync(
            f.app.world().resource::<EditorSession>(),
            f.app.world().resource::<ProjectEffectCatalog>(),
        );
        order.clear_context(&asset_order::Context::Material(f.target(0)));
        f.app.insert_resource(order);
    }
    let revision = f.revision();
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(0).name, "Retained local edit");
    assert_eq!(
        f.app.world().resource::<EditorSession>().status,
        "Nothing to undo"
    );
    assert_eq!(f.revision(), revision);
    f.no_requests();
}

#[test]
fn project_switch_invalidates_global_delete_before_document_routing() {
    let mut f = Fixture::new();
    f.delete();
    let other = tempfile::tempdir().unwrap();
    f.app
        .insert_resource(ProjectEffectCatalog::scan(other.path()));
    let before = f.app.world().resource::<EditorSession>().effect.clone();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .set_effect_name("New project edit");
    f.step(HistoryAction::Undo);
    assert_eq!(f.app.world().resource::<EditorSession>().effect, before);
    assert!(!f.app.world().resource::<asset_order::AssetOrder>().active());
    f.no_requests();
}

#[test]
fn focused_compound_step_validates_layout_before_semantics_and_restores_target_on_failure() {
    let mut f = Fixture::new();
    f.select(0);
    let graph = material_graph_view_key(f.programs[0].id);
    let mut memory = GraphViewportMemory::default();
    memory.set_node(&graph, "output", Vec2::ZERO, false);
    let snapshot = Snapshot::capture(
        &graph,
        f.app.world().resource::<ProjectEffectCatalog>(),
        f.app.world().resource::<EditorSession>(),
        &memory,
    )
    .unwrap();
    f.app.insert_resource(memory);
    f.edit(0, "Compound edit");
    let mut memory = f
        .app
        .world_mut()
        .remove_resource::<GraphViewportMemory>()
        .unwrap();
    memory.set_node(&graph, "output", Vec2::splat(80.0), true);
    let mut session = f
        .app
        .world_mut()
        .remove_resource::<EditorSession>()
        .unwrap();
    snapshot.attach(
        f.app.world().resource::<ProjectEffectCatalog>(),
        &mut session,
        &mut memory,
    );
    f.app.insert_resource(session);
    f.select(1);
    f.focus(0);
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(0).name, "Compound edit");
    assert_eq!(
        f.app.world().resource::<EditorSession>().material_target,
        f.target(1)
    );
    assert_eq!(
        f.app.world().resource::<EditorSession>().status,
        "Graph layout is unavailable"
    );
    f.app.insert_resource(memory);
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(0), f.programs[0]);
    assert_eq!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .node(&graph, "output"),
        Some((Vec2::ZERO, false))
    );
    f.step(HistoryAction::Redo);
    assert_eq!(f.current(0).name, "Compound edit");
    assert_eq!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .node(&graph, "output"),
        Some((Vec2::splat(80.0), true))
    );
    f.no_requests();
}

#[test]
fn filesystem_redo_dispatch_uses_current_direction_revision_and_journal_without_advancing() {
    let mut f = Fixture::new();
    let item = f.delete();
    let revision = f.revision();
    item.clone().restore(&[], true).unwrap();
    assert!(
        f.app
            .world_mut()
            .resource_mut::<asset_order::AssetOrder>()
            .finish_delete(true, revision, item.clone())
    );
    let revision = f.revision();
    f.step(HistoryAction::Redo);
    let requests = &f.app.world().resource::<Requests>().0;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0.journal(), item.journal());
    assert!(!requests[0].1);
    assert_eq!(requests[0].2, revision);
    assert_eq!(f.revision(), revision);
    assert_eq!(
        fs::read(f.root.path().join("unused.png")).unwrap(),
        b"exact recovery bytes"
    );
}

#[test]
fn a_new_global_edit_retires_redo_for_every_program_before_routing_the_new_step() {
    let mut f = Fixture::new();
    f.delete();
    f.edit(0, "First edit");
    f.edit(1, "Second edit");
    f.step(HistoryAction::Undo);
    f.step(HistoryAction::Undo);
    let available = |f: &Fixture, index: usize| {
        f.app
            .world()
            .resource::<MaterialProgramEditHistory>()
            .available_for_program(f.root.path(), f.programs[index].id, false)
    };
    assert!(available(&f, 0) && available(&f, 1));
    let effect = f.app.world().resource::<EditorSession>().effect.clone();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .set_effect_name("New global branch");
    f.focus(1);
    f.step(HistoryAction::Undo);
    assert!(!available(&f, 0) && !available(&f, 1));
    assert_eq!(f.app.world().resource::<EditorSession>().effect, effect);
    assert!(matches!(
        f.app
            .world()
            .resource::<asset_order::AssetOrder>()
            .top(false),
        Some(asset_order::Entry::Document(asset_order::Context::Effect))
    ));
    f.step(HistoryAction::Redo);
    assert_eq!(
        f.app.world().resource::<EditorSession>().effect.name,
        "New global branch"
    );
    assert_eq!(f.current(0), f.programs[0]);
    assert_eq!(f.current(1), f.programs[1]);
    f.no_requests();
}

#[test]
fn focused_function_round_trip_uses_real_function_history_not_the_session_program() {
    use aestra_core::{
        MaterialExpressionId, MaterialFunctionId, MaterialFunctionOutputId,
        material::{
            MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialFunctionOutput,
            MaterialSchemaVersion, MaterialValue, MaterialValueType,
        },
    };
    let mut f = Fixture::new();
    let path = f.root.path().join("function.aestra.material-function.ron");
    let expression = MaterialExpressionId::new();
    let function = MaterialFunction {
        schema_version: MaterialSchemaVersion::CURRENT,
        id: MaterialFunctionId::new(),
        name: "Function".into(),
        inputs: vec![],
        outputs: vec![MaterialFunctionOutput {
            id: MaterialFunctionOutputId::new(),
            name: "Value".into(),
            value_type: MaterialValueType::Float,
            expression,
        }],
        expressions: vec![MaterialExpression {
            id: expression,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        }],
        custom_wesl: None,
    };
    function.save_ron(&path).unwrap();
    f.app
        .world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    let mut session = f
        .app
        .world_mut()
        .remove_resource::<EditorSession>()
        .unwrap();
    let mut catalog = f
        .app
        .world_mut()
        .remove_resource::<ProjectEffectCatalog>()
        .unwrap();
    session
        .open_material_function(&catalog, function.id)
        .unwrap();
    let mut after = function.clone();
    after.name = "Function edit".into();
    f.app
        .world_mut()
        .resource_mut::<FunctionEditor>()
        .edit(&mut session, &mut catalog, after.clone())
        .unwrap();
    f.app.insert_resource(session).insert_resource(catalog);
    f.edit(0, "Program edit");
    let mut documents = DocumentManager::default();
    let mut views = EditorViewManager::default();
    let mut active = ActiveEditorContext::default();
    open_document_view(
        &mut documents,
        &mut views,
        &mut active,
        DocumentKey::MaterialFunction(function.id),
        EditorViewKind::MaterialFunctionGraph,
    );
    f.app
        .insert_resource(documents)
        .insert_resource(views)
        .insert_resource(active);
    f.step(HistoryAction::Undo);
    assert_eq!(
        f.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_functions()
            .unwrap()
            .as_slice(),
        std::slice::from_ref(&function)
    );
    assert_eq!(f.current(0).name, "Program edit");
    f.step(HistoryAction::Redo);
    assert_eq!(
        f.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_functions()
            .unwrap(),
        [after]
    );
    assert_eq!(f.current(0).name, "Program edit");
    assert_eq!(MaterialFunction::load_ron(path).unwrap(), function);
    f.no_requests();
}
