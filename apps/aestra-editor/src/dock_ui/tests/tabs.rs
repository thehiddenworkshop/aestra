//! Real dock-strip composition and native input, without writing the user's layout.
use super::*;
use crate::feathers::text_input::tests::{app_with, focus, tap};
use bevy::{
    input::keyboard::Key,
    input_focus::tab_navigation::TabIndex,
    picking::{
        backend::HitData,
        events::Pointer,
        pointer::{Location, PointerButton, PointerId},
    },
    window::{PrimaryWindow, WindowRef},
};

#[derive(Resource, Default)]
struct Requests(Vec<Entity>);

fn app() -> App {
    app_with(|app| {
        let mut layout = WorkspaceLayout::default();
        layout.root = DockNode::Tabs {
            id: DockNodeId(88),
            stack: DockStack::new(
                [
                    ToolPanel::Timeline.into(),
                    ToolPanel::MaterialGraph.into(),
                    ToolPanel::Assets.into(),
                ],
                ToolPanel::Timeline.into(),
            ),
        };
        app.insert_resource(layout)
            .insert_resource(crate::test_support::session_with_timing_slack())
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<ProjectEffectCatalog>()
            .init_resource::<crate::document::DocumentManager>()
            .init_resource::<crate::editor_view::EditorViewManager>()
            .init_resource::<crate::editor_view::ActiveEditorContext>()
            .init_resource::<crate::wesl_document::WeslDocuments>()
            .init_resource::<MenuState>()
            .init_resource::<DockDragState>()
            .init_resource::<DockTabFocusMemory>()
            .init_resource::<Requests>()
            // Observe the actual bridge; controlled selection is deliberately not
            // self-updated here. Host routing is tested separately on reselect.
            .add_observer(|event: On<SelectDockTab>, mut requests: ResMut<Requests>| {
                requests.0.push(event.entity)
            })
            .add_systems(
                PostUpdate,
                (sync_selection, restore_focus)
                    .chain()
                    .before(MenuFocusSystem),
            )
            .add_systems(
                PostUpdate,
                sync_style.after(InputFocusSystems::FocusChangeEvents),
            )
            .add_systems(
                PostUpdate,
                reveal_focused_tab.after(bevy::ui::UiSystems::PostLayout),
            );
    })
}

fn compose(app: &mut App) -> (Entity, Vec<Entity>) {
    let stack = match &app.world().resource::<WorkspaceLayout>().root {
        DockNode::Tabs { stack, .. } => stack.clone(),
        _ => panic!("fixture uses one stack"),
    };
    let documents = crate::document::DocumentManager::default();
    let views = crate::editor_view::EditorViewManager::default();
    let catalog = ProjectEffectCatalog::default();
    let wesl = crate::wesl_document::WeslDocuments::default();
    let localizer = Localizer::new("en-US").unwrap();
    app.world_mut()
        .commands()
        .spawn(Node {
            width: px(180),
            height: px(60),
            ..default()
        })
        .with_children(|parent| {
            super::super::spawn_dock_tab_bar(
                parent,
                DockNodeId(88),
                &stack,
                true,
                &documents,
                &views,
                &catalog,
                &wesl,
                &localizer,
            )
        });
    app.world_mut().flush();
    for _ in 0..3 {
        app.update();
    }
    let world = app.world_mut();
    let strip = world
        .query_filtered::<Entity, With<DockTabStrip>>()
        .iter(world)
        .last()
        .unwrap();
    let tabs = world
        .get::<Children>(strip)
        .unwrap()
        .iter()
        .filter(|entity| world.get::<Tab>(*entity).is_some())
        .collect();
    (strip, tabs)
}

fn click(app: &mut App, entity: Entity, button: PointerButton) {
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    app.world_mut().trigger(PointerClick {
        entity,
        pointer: Pointer::new(
            PointerId::Mouse,
            Location {
                target: RenderTarget::Window(WindowRef::Entity(window))
                    .normalize(None)
                    .unwrap(),
                position: Vec2::ZERO,
            },
        ),
        button,
        hit: HitData::new(window, 0.0, None, None),
        duration: std::time::Duration::ZERO,
        count: 1,
    });
    app.world_mut().flush();
    app.update();
}

#[test]
fn actual_dock_strip_has_native_semantics_one_roving_entry_and_no_button_controller() {
    let mut app = app();
    let (strip, tabs) = compose(&mut app);
    assert_eq!(
        app.world().get::<SelectedTab>(strip).unwrap().0,
        Some(tabs[0])
    );
    for (index, tab) in tabs.iter().enumerate() {
        assert!(app.world().get::<Button>(*tab).is_none());
        assert!(app.world().get::<DockingAction>(*tab).is_none());
        assert_eq!(
            app.world().get::<TabIndex>(*tab).unwrap().0,
            if index == 0 { 0 } else { -1 }
        );
        assert!(
            app.world()
                .get::<bevy::a11y::AccessibilityNode>(*tab)
                .is_some()
        );
        assert!(
            !app.world()
                .get::<AccessibleLabel>(*tab)
                .unwrap()
                .0
                .is_empty()
        );
    }
    assert!(
        app.world()
            .get::<bevy::a11y::AccessibilityNode>(strip)
            .is_some()
    );
    let dirty = app
        .world_mut()
        .query::<(&DockTabDirty, &Node)>()
        .iter(app.world())
        .find(|(tab, _)| tab.0 == ToolPanel::MaterialGraph.into())
        .unwrap()
        .1;
    assert_eq!(dirty.display, Display::Flex);
    assert!(
        app.world()
            .get::<Children>(strip)
            .unwrap()
            .iter()
            .any(|entity| app.world().get::<DockTabAppendZone>(entity).is_some())
    );
}

#[test]
fn native_keyboard_is_manual_skips_disabled_wraps_and_commits_once() {
    let mut app = app();
    let (strip, tabs) = compose(&mut app);
    app.world_mut()
        .entity_mut(tabs[1])
        .insert(InteractionDisabled);
    focus(&mut app, tabs[0]);
    tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight, None);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(tabs[2]));
    assert!(app.world().resource::<Requests>().0.is_empty());
    assert_eq!(
        app.world().get::<SelectedTab>(strip).unwrap().0,
        Some(tabs[0])
    );
    assert!(app.world().get::<Outline>(tabs[2]).is_some());
    tap(&mut app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(app.world().resource::<Requests>().0, [tabs[2]]);
    // Only accepting the request in the domain model updates controlled selection.
    app.world_mut()
        .resource_mut::<WorkspaceLayout>()
        .activate(ToolPanel::Assets);
    app.update();
    assert_eq!(
        app.world().get::<SelectedTab>(strip).unwrap().0,
        Some(tabs[2])
    );
    tap(&mut app, KeyCode::Space, Key::Space, None);
    assert_eq!(
        app.world().resource::<Requests>().0,
        [tabs[2]],
        "selected tabs must not emit twice"
    );
    tap(&mut app, KeyCode::ArrowRight, Key::ArrowRight, None);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(tabs[0]));
    tap(&mut app, KeyCode::End, Key::End, None);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(tabs[2]));
    tap(&mut app, KeyCode::Home, Key::Home, None);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(tabs[0]));
}

#[test]
fn pointer_content_reselect_and_nested_close_have_distinct_routes() {
    let mut app = app();
    let (_, tabs) = compose(&mut app);
    let title = app.world().get::<Children>(tabs[1]).unwrap()[0];
    click(&mut app, title, PointerButton::Primary);
    assert_eq!(app.world().resource::<Requests>().0, [tabs[1]]);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(tabs[1]));
    click(&mut app, tabs[0], PointerButton::Primary);
    assert_eq!(app.world().resource::<Requests>().0, [tabs[1], tabs[0]]);
    let close = app
        .world()
        .get::<Children>(tabs[1])
        .unwrap()
        .iter()
        .find(|entity| app.world().get::<DockCloseButton>(*entity).is_some())
        .unwrap();
    click(&mut app, close, PointerButton::Primary);
    click(&mut app, tabs[1], PointerButton::Middle);
    assert_eq!(app.world().resource::<Requests>().0, [tabs[1], tabs[0]]);
}

#[test]
fn bridge_rejects_foreign_hidden_disabled_nonfinal_and_model_stale_requests() {
    let mut app = app();
    let (strip, tabs) = compose(&mut app);
    let foreign = app.world_mut().spawn(Tab).id();
    for (entity, is_final) in [(foreign, true), (tabs[1], false)] {
        app.world_mut().trigger(ValueChange {
            source: strip,
            value: Some(entity),
            is_final,
        });
    }
    for disabled in [tabs[1], strip] {
        app.world_mut()
            .entity_mut(disabled)
            .insert(InteractionDisabled);
        app.world_mut().trigger(ValueChange {
            source: strip,
            value: Some(tabs[1]),
            is_final: true,
        });
        app.world_mut().flush();
        app.world_mut()
            .entity_mut(disabled)
            .remove::<InteractionDisabled>();
    }
    app.world_mut().entity_mut(strip).insert(Visibility::Hidden);
    app.world_mut().trigger(ValueChange {
        source: strip,
        value: Some(tabs[1]),
        is_final: true,
    });
    app.world_mut().flush();
    app.world_mut().entity_mut(strip).remove::<Visibility>();
    app.world_mut()
        .resource_mut::<WorkspaceLayout>()
        .close(ToolPanel::MaterialGraph);
    app.world_mut().trigger(ValueChange {
        source: strip,
        value: Some(tabs[1]),
        is_final: true,
    });
    app.world_mut().flush();
    assert!(app.world().resource::<Requests>().0.is_empty());
}

#[test]
fn starting_a_drag_keeps_the_tab_alive_and_blocks_competing_selection() {
    let mut app = app();
    let (strip, tabs) = compose(&mut app);
    let window = app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(app.world())
        .unwrap();
    let revision = app.world().resource::<EditorSession>().ui_revision;
    app.world_mut().trigger(PointerDragStart {
        entity: tabs[1],
        pointer: Pointer::new(
            PointerId::Mouse,
            Location {
                target: RenderTarget::Window(WindowRef::Entity(window))
                    .normalize(None)
                    .unwrap(),
                position: Vec2::ZERO,
            },
        ),
        button: PointerButton::Primary,
        hit: HitData::new(window, 0.0, None, None),
    });
    app.world_mut().flush();
    assert_eq!(
        app.world().resource::<DockDragState>().0,
        Some(ToolPanel::MaterialGraph.into())
    );
    assert!(app.world().get_entity(tabs[1]).is_ok());
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision
    );
    app.world_mut().trigger(ValueChange {
        source: strip,
        value: Some(tabs[1]),
        is_final: true,
    });
    app.world_mut().flush();
    assert!(app.world().resource::<Requests>().0.is_empty());
}

#[test]
fn keyboard_focus_survives_rebuild_without_stealing_other_control_focus() {
    let mut app = app();
    let (strip, tabs) = compose(&mut app);
    focus(&mut app, tabs[2]);
    app.world_mut().entity_mut(strip).despawn();
    let (_, rebuilt) = compose(&mut app);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(rebuilt[2]));
    assert_eq!(app.world().get::<TabIndex>(rebuilt[2]).unwrap().0, 0);
    let other = app.world_mut().spawn(bevy::ui_widgets::Button).id();
    focus(&mut app, other);
    app.world_mut().entity_mut(rebuilt[2]).despawn();
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(other));
}

#[test]
fn real_layout_keyboard_reveal_keeps_the_existing_horizontal_strip() {
    let mut app = app();
    app.world_mut().spawn(Camera2d);
    let (strip, tabs) = compose(&mut app);
    focus(&mut app, tabs[0]);
    tap(&mut app, KeyCode::End, Key::End, None);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(tabs[2]));
    assert!(app.world().get::<ScrollPosition>(strip).unwrap().x > 0.0);
    assert!(
        app.world()
            .get::<bevy::ui_widgets::ScrollArea>(strip)
            .is_none(),
        "do not install a competing wheel controller"
    );
    assert!(app.world().resource::<Requests>().0.is_empty());
}

#[test]
fn reselect_uses_actual_document_focus_route_even_when_layout_selection_did_not_change() {
    let mut app = app();
    let key = crate::document::DocumentKey::WeslSource(
        crate::editor_identity::WeslSourceId::for_relative_path(std::path::Path::new(
            "shaders/test.wesl",
        )),
    );
    let document = app
        .world_mut()
        .resource_mut::<crate::document::DocumentManager>()
        .open(key);
    let view = app
        .world_mut()
        .resource_mut::<crate::editor_view::EditorViewManager>()
        .create_view(document, crate::editor_view::EditorViewKind::WeslSource);
    let tab = DockTab::Editor(view);
    app.world_mut().resource_mut::<WorkspaceLayout>().root = DockNode::Tabs {
        id: DockNodeId(88),
        stack: DockStack::new([tab], tab),
    };
    let (_, tabs) = compose(&mut app);
    app.add_observer(super::super::select_dock_tab);
    let revision = app.world().resource::<EditorSession>().ui_revision;
    click(&mut app, tabs[0], PointerButton::Primary);
    let active = app
        .world()
        .resource::<crate::editor_view::ActiveEditorContext>();
    assert_eq!(active.active_view, Some(view));
    assert_eq!(active.active_document, Some(document));
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision + 1
    );
    assert_eq!(app.world().resource::<Requests>().0, [tabs[0]]);
}
