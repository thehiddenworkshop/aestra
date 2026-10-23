use super::*;
use bevy::ecs::system::RunSystemOnce;

#[derive(Resource, Default)]
struct Requests(Vec<arrange::ArrangeGraph>);

#[test]
fn node_context_arrangement_uses_owning_selection_without_rebuilding_graph() {
    for arrange_scope in [
        arrange::ArrangeScope::Selection,
        arrange::ArrangeScope::Upstream,
        arrange::ArrangeScope::Downstream,
    ] {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgramId::new();
        let scope = Some(crate::docking::EditorViewId(7));
        let selected = BTreeSet::from([MaterialExpressionId::new(), MaterialExpressionId::new()]);
        let target = crate::material_document::MaterialEditingTarget::Program {
            root: root.path().to_owned(),
            id: program,
        };
        let mut app = App::new();
        app.insert_resource(crate::test_support::session_with_timing_slack())
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .init_resource::<MaterialGraphSelectionState>()
            .init_resource::<clipboard::GraphClipboard>()
            .init_resource::<MaterialGraphPreviewState>()
            .init_resource::<MaterialGraphPaletteState>()
            .init_resource::<MaterialStackInspectorState>()
            .init_resource::<GraphViewportMemory>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<Requests>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_observer(
                |event: On<arrange::ArrangeGraph>, mut requests: ResMut<Requests>| {
                    requests.0.push(event.event().clone());
                },
            );
        app.world_mut()
            .resource_mut::<MaterialGraphSelectionState>()
            .select_material_expressions(scope, program, &selected, GraphSelectionMode::Replace);
        let viewport = app
            .world_mut()
            .spawn(MaterialGraphViewport {
                program,
                scope,
                editing_target: target.clone(),
            })
            .id();
        let anchor = app.world_mut().spawn_empty().id();
        app.world_mut()
            .spawn((MaterialGraphNodeMenu, ChildOf(anchor)));
        app.world_mut()
            .resource_mut::<MaterialGraphPaletteState>()
            .node_menu = Some(MaterialGraphNodeMenuOpen {
            program,
            scope,
            menu_position: Vec2::ZERO,
        });
        app.world_mut().spawn((
            MaterialGraphContextAction::Arrange(program, arrange_scope),
            FeathersActionButton,
            PendingFeathersActivation,
            Interaction::Pressed,
        ));
        // A flyout extends outside the root surface. Its clicks must not close the menu first.
        app.world_mut().spawn((
            PointerContextSubmenuSurface,
            ChildOf(anchor),
            RelativeCursorPosition {
                cursor_over: true,
                normalized: None,
            },
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut()
            .run_system_once(dismiss_material_graph_palette)
            .unwrap();
        assert!(
            app.world()
                .resource::<MaterialGraphPaletteState>()
                .node_menu
                .is_some()
        );
        let revision = app.world().resource::<EditorSession>().ui_revision;
        app.world_mut()
            .run_system_once(handle_material_graph_context_actions)
            .unwrap();
        app.world_mut().flush();
        let requests = &app.world().resource::<Requests>().0;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].scope, arrange_scope);
        assert_eq!(requests[0].view.view, scope);
        assert_eq!(requests[0].editing_target, target);
        assert_eq!(
            requests[0].seeds,
            selected.into_iter().map(GraphNodeKey::Expression).collect()
        );
        assert_eq!(
            app.world().resource::<EditorSession>().ui_revision,
            revision
        );
        assert!(app.world().get_entity(viewport).is_ok());
        assert!(app.world().get_entity(anchor).is_err());
        assert!(
            app.world()
                .resource::<MaterialGraphPaletteState>()
                .node_menu
                .is_none()
        );
    }
}
