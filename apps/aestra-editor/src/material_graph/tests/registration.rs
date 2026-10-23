//! Complete production plugin running with real layout, input and saved source fixtures.
use crate::feathers::node_graph::{
    GraphViewportMemory, GraphWireMaterial,
    geometry::{self, GraphGeometryRegistry, GraphGeometryView},
};
use crate::material_graph::EditorMaterialGraphPlugin;
use crate::material_graph::{
    MaterialGraphAction, MaterialGraphPaletteState, MaterialGraphSelectionState, click_input,
};
use crate::{EditorSession, ProjectEffectCatalog};
use aestra_core::material::{MaterialFunction, MaterialProgram};
use bevy::{ecs::system::RunSystemOnce, picking::pointer::PointerButton, prelude::*};

struct Fixture {
    app: App,
    root: tempfile::TempDir,
    viewport: Entity,
    program: MaterialProgram,
    source: Entity,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Complete material plugin").normalized();
        program
            .save_ron(root.path().join("main.aestra.material.ron"))
            .unwrap();
        let function = MaterialFunction::from_ron(include_str!(
            "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
        ))
        .unwrap();
        function
            .save_ron(root.path().join("callee.aestra.material-function.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let payload = crate::asset_drop::AssetPayload::capture(
            &catalog,
            catalog
                .content()
                .source_tree()
                .at_relative_path("callee.aestra.material-function.ron")
                .unwrap()
                .id,
        );
        let mut session = crate::test_support::session_with_timing_slack();
        session.open_material_program(&catalog, program.id).unwrap();
        let target = session.material_target.clone();
        let (mut app, host) = geometry::tests::layout_app(1.25);
        app.insert_resource(catalog)
            .insert_resource(session)
            .insert_resource(crate::Localizer::new("en-US").unwrap())
            .init_asset::<GraphWireMaterial>()
            .init_resource::<crate::properties::MaterialStackInspectorState>()
            .init_resource::<crate::docking::WorkspaceLayout>()
            .init_resource::<crate::history::EditorHistoryLedger>()
            .init_resource::<crate::history::MaterialProgramEditHistory>()
            .init_resource::<crate::material_function_editor::FunctionEditor>()
            .add_observer(crate::history::execute_history_action)
            .add_plugins((
                bevy::input::InputPlugin,
                bevy::input_focus::InputFocusPlugin,
                bevy::input_focus::InputDispatchPlugin,
                crate::input::EditorKeyboardPlugin,
                EditorMaterialGraphPlugin,
            ));
        // Also register the actual function editor, as the shipping application does.
        crate::material_function_editor::register(&mut app);
        app.world_mut().run_system_once(
            move |mut commands: Commands, assets: Res<AssetServer>, catalog: Res<ProjectEffectCatalog>,
                  session: Res<EditorSession>, memory: Res<GraphViewportMemory>,
                  palette: Res<MaterialGraphPaletteState>, selection: Res<MaterialGraphSelectionState>,
                  previews: Res<crate::material_graph::MaterialGraphPreviewState>| {
                commands.entity(host).with_children(|parent| {
                    crate::material_graph::workspace::spawn(parent, &target,
                        Some(crate::docking::EditorViewId(8)), &session, &catalog, &palette,
                        &selection, &previews, &memory, &crate::Localizer::new("en-US").unwrap(), &assets);
                });
            }
        ).unwrap();
        let source = app.world_mut().spawn(payload).id();
        for _ in 0..8 {
            app.update();
        }
        let viewport = app
            .world_mut()
            .query_filtered::<Entity, With<GraphGeometryView>>()
            .single(app.world())
            .unwrap();
        Self {
            app,
            root,
            viewport,
            program,
            source,
        }
    }

    fn current(&self) -> MaterialProgram {
        self.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_program(self.program.id)
            .unwrap()
    }

    fn point(&self) -> Vec2 {
        let computed = self.app.world().get::<ComputedNode>(self.viewport).unwrap();
        let transform = self
            .app
            .world()
            .get::<UiGlobalTransform>(self.viewport)
            .unwrap();
        (transform.translation.trunc() - computed.size() * 0.5
            + computed.size() * Vec2::new(0.8, 0.85))
            * computed.inverse_scale_factor
    }
}

#[test]
fn complete_plugin_and_function_editor_run_idle_over_a_real_measured_canvas() {
    let mut fixture = Fixture::new();
    assert!(fixture.app.is_plugin_added::<EditorMaterialGraphPlugin>());
    let view = fixture
        .app
        .world()
        .get::<GraphGeometryView>(fixture.viewport)
        .unwrap();
    let snapshot = fixture
        .app
        .world()
        .resource::<GraphGeometryRegistry>()
        .mounted_view_snapshot(&view.key, fixture.viewport)
        .expect("production canvas must be measured");
    assert!(!snapshot.nodes.is_empty());
    assert!(
        snapshot
            .nodes
            .values()
            .all(|node| node.size.is_finite() && node.size.min_element() > 0.0)
    );
    assert!(
        fixture
            .app
            .world()
            .contains_resource::<crate::material_graph::clipboard::GraphClipboard>()
    );
    assert!(fixture.app.world().contains_resource::<crate::material_graph::preset_previews::MaterialPresetPreviewState>());
    assert!(
        fixture
            .app
            .world()
            .contains_resource::<crate::feathers::node_graph::insertion::State>()
    );
    let bytes = std::fs::read(fixture.root.path().join("main.aestra.material.ron")).unwrap();
    for _ in 0..8 {
        fixture.app.update();
    }
    assert_eq!(fixture.current(), fixture.program);
    assert_eq!(
        std::fs::read(fixture.root.path().join("main.aestra.material.ron")).unwrap(),
        bytes
    );
}

#[test]
fn complete_plugin_routes_node_clicks_and_canvas_menu_without_duplicate_consumers() {
    let mut fixture = Fixture::new();
    let (node, expression) = fixture
        .app
        .world_mut()
        .query::<(Entity, &MaterialGraphAction)>()
        .iter(fixture.app.world())
        .next()
        .map(|(entity, action)| (entity, action.expression))
        .unwrap();
    let revision = fixture.app.world().resource::<EditorSession>().ui_revision;
    fixture.app.world_mut().trigger(click_input::click(
        node,
        Entity::PLACEHOLDER,
        Vec2::ZERO,
        PointerButton::Primary,
    ));
    fixture.app.world_mut().flush();
    assert!(
        fixture
            .app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .is_expression_selected(
                Some(crate::docking::EditorViewId(8)),
                fixture.program.id,
                expression
            )
    );
    assert_eq!(
        fixture.app.world().resource::<EditorSession>().ui_revision,
        revision + 1
    );
    let point = fixture.point();
    fixture.app.world_mut().trigger(click_input::click(
        fixture.viewport,
        Entity::PLACEHOLDER,
        point,
        PointerButton::Secondary,
    ));
    fixture.app.world_mut().flush();
    assert!(
        fixture
            .app
            .world()
            .resource::<MaterialGraphPaletteState>()
            .open
            .is_some()
    );
    let window = fixture
        .app
        .world_mut()
        .query_filtered::<Entity, With<Window>>()
        .single(fixture.app.world())
        .unwrap();
    fixture
        .app
        .world_mut()
        .write_message(bevy::input::keyboard::KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: bevy::input::keyboard::Key::Escape,
            state: bevy::input::ButtonState::Pressed,
            repeat: false,
            text: None,
            window,
        });
    fixture.app.update();
    assert!(
        !fixture
            .app
            .world()
            .resource::<MaterialGraphPaletteState>()
            .is_open()
    );
    assert_eq!(fixture.current(), fixture.program);
}

#[test]
fn complete_plugin_browser_drop_commits_once_and_global_undo_restores_source() {
    let mut fixture = Fixture::new();
    let before = fixture.current();
    let point = fixture.point();
    let location = bevy::picking::pointer::Location {
        target: bevy::camera::NormalizedRenderTarget::None {
            width: 1200,
            height: 800,
        },
        position: point,
    };
    fixture
        .app
        .world_mut()
        .trigger(crate::asset_drop::input::tests::enter(
            fixture.viewport,
            fixture.source,
            location.clone(),
            PointerButton::Primary,
        ));
    fixture.app.world_mut().flush();
    assert_eq!(fixture.app.world_mut().query::<&crate::asset_drop::Feedback<crate::material_graph::asset_drop::GraphDropTarget>>()
        .iter(fixture.app.world()).count(), 1);
    fixture
        .app
        .world_mut()
        .trigger(crate::asset_drop::input::tests::drop(
            fixture.viewport,
            fixture.source,
            location,
            PointerButton::Primary,
        ));
    fixture.app.world_mut().flush();
    assert!(
        fixture
            .app
            .world()
            .resource::<EditorSession>()
            .status
            .starts_with("Added ")
    );
    let after = fixture.current();
    assert!(after.expressions.len() > before.expressions.len());
    for _ in 0..4 {
        fixture.app.update();
    }
    assert_eq!(fixture.current(), after);
    fixture
        .app
        .world_mut()
        .trigger(crate::history::HistoryAction::Undo);
    fixture.app.world_mut().flush();
    assert_eq!(fixture.current(), before);
    fixture
        .app
        .world_mut()
        .trigger(crate::history::HistoryAction::Undo);
    fixture.app.world_mut().flush();
    assert_eq!(
        fixture.app.world().resource::<EditorSession>().status,
        "No material history in this direction"
    );
    fixture
        .app
        .world_mut()
        .trigger(crate::history::HistoryAction::Redo);
    fixture.app.world_mut().flush();
    assert_eq!(fixture.current(), after);
    assert_eq!(
        MaterialProgram::load_ron(fixture.root.path().join("main.aestra.material.ron")).unwrap(),
        before
    );
}
