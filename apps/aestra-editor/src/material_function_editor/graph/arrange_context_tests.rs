use super::*;
use crate::material_graph::{
    MaterialGraphSelectionState,
    arrange::{ArrangeGraph, ArrangeScope},
};
use bevy::ecs::system::RunSystemOnce;

#[derive(Resource, Default)]
struct Requests(Vec<ArrangeGraph>);

#[test]
fn function_context_arrangement_keeps_view_and_explicit_target_without_focus_rebuild() {
    for scope in [
        ArrangeScope::Selection,
        ArrangeScope::Upstream,
        ArrangeScope::Downstream,
    ] {
        let root = tempfile::tempdir().unwrap();
        let owner = MaterialFunctionId::new();
        let view = Some(crate::docking::EditorViewId(9));
        let selected = BTreeSet::from([MaterialExpressionId::new(), MaterialExpressionId::new()]);
        let mut app = App::new();
        app.insert_resource(crate::test_support::session_with_timing_slack())
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .init_resource::<MaterialGraphSelectionState>()
            .init_resource::<crate::material_graph::clipboard::GraphClipboard>()
            .init_resource::<FunctionGraphMenuState>()
            .init_resource::<GraphViewportMemory>()
            .init_resource::<FunctionEditor>()
            .init_resource::<Requests>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_observer(handle_function_graph_context_action)
            .add_observer(|event: On<ArrangeGraph>, mut requests: ResMut<Requests>| {
                requests.0.push(event.event().clone());
            });
        app.world_mut()
            .resource_mut::<MaterialGraphSelectionState>()
            .select_function_expressions(view, owner, &selected, GraphSelectionMode::Replace);
        let viewport = app.world_mut().spawn((View(owner), ViewScope(view))).id();
        let anchor = app.world_mut().spawn(ChildOf(viewport)).id();
        app.world_mut()
            .spawn((FunctionGraphContextMenu, ChildOf(anchor)));
        app.world_mut()
            .resource_mut::<FunctionGraphMenuState>()
            .open = Some(FunctionGraphMenuOpen {
            owner,
            scope: view,
            position: Vec2::ZERO,
            kind: FunctionGraphMenuKind::Node(*selected.first().unwrap()),
        });
        let action = app
            .world_mut()
            .spawn(FunctionGraphContextAction::Arrange(scope))
            .id();
        let revision = app.world().resource::<EditorSession>().ui_revision;
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
            .run_system_once(dismiss_function_graph_menu)
            .unwrap();
        assert!(
            app.world()
                .resource::<FunctionGraphMenuState>()
                .open
                .is_some()
        );
        app.world_mut().trigger(Activate { entity: action });
        app.world_mut().flush();
        let requests = &app.world().resource::<Requests>().0;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].scope, scope);
        assert_eq!(requests[0].view.view, view);
        assert_eq!(
            requests[0].editing_target,
            crate::material_document::MaterialEditingTarget::Function {
                root: root.path().to_owned(),
                id: owner,
            }
        );
        assert_eq!(
            requests[0].seeds,
            selected.into_iter().map(GraphNodeKey::Expression).collect()
        );
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.ui_revision, revision);
        assert_eq!(
            session.material_target,
            crate::material_document::MaterialEditingTarget::EffectInstance
        );
        assert!(app.world().get_entity(viewport).is_ok());
        assert!(app.world().get_entity(anchor).is_err());
        assert!(
            app.world()
                .resource::<FunctionGraphMenuState>()
                .open
                .is_none()
        );
    }
}
