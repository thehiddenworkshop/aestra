//! Shared actual palette consumers and real Feathers composition.
use super::*;
use crate::{docking::EditorViewId, material_document::MaterialEditingTarget, test_support};
use aestra_authoring::MaterialOutputSocket;
use aestra_core::material::{MaterialProgram, MaterialValueType};
fn option(label: &str, category: &str) -> MaterialGraphPaletteOption {
    MaterialGraphPaletteOption {
        descriptor: MaterialGraphNodeDescriptor {
            kind: MaterialGraphCreateKind::Constant(MaterialValueType::Float),
            label: label.into(),
            category: category.into(),
        },
        source: None,
        target: None,
    }
}
fn opened(program: MaterialProgramId) -> MaterialGraphPaletteOpen {
    MaterialGraphPaletteOpen {
        program,
        editing_target: MaterialEditingTarget::EffectInstance,
        scope: Some(EditorViewId(7)),
        menu_position: Vec2::new(120.0, 70.0),
        graph_position: Vec2::new(300.0, 180.0),
        graph_key: "material:palette".into(),
        connection: None,
    }
}
fn app() -> App {
    let mut a = App::new();
    a.insert_resource(test_support::session_with_timing_slack())
        .init_resource::<MaterialGraphPaletteState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<InputFocus>()
        .add_observer(update_material_graph_palette_search)
        .add_systems(
            Update,
            (
                open_material_graph_palette_from_keyboard,
                dismiss_material_graph_palette,
                focus_material_graph_palette_search,
            )
                .chain(),
        );
    a
}
fn viewport(a: &mut App, cursor: Option<Vec2>, hovered: bool) -> MaterialProgramId {
    let id = MaterialProgramId::new();
    a.world_mut().spawn((
        MaterialGraphViewport {
            program: id,
            editing_target: MaterialEditingTarget::Program {
                root: "standalone".into(),
                id,
            },
            scope: Some(EditorViewId(7)),
        },
        FeathersGraphViewport::clipboard_fixture("graph".into(), Vec2::new(20.0, 10.0), 2.0),
        RelativeCursorPosition {
            cursor_over: hovered,
            normalized: cursor,
        },
        ComputedNode {
            size: Vec2::new(400.0, 200.0),
            ..default()
        },
    ));
    id
}
#[test]
fn add_node_palette_groups_options_by_category_in_catalog_order() {
    let option = |label: &str, category: &str| MaterialGraphPaletteOption {
        descriptor: MaterialGraphNodeDescriptor {
            kind: MaterialGraphCreateKind::Constant(MaterialValueType::Float),
            label: label.to_owned(),
            category: category.to_owned(),
        },
        source: None,
        target: None,
    };
    let options = vec![
        option("Float", "Constants"),
        option("UV", "Inputs"),
        option("Color", "Constants"),
        option("Add", "Math"),
    ];

    let categories = material_graph_palette_categories(&options);

    assert_eq!(
        categories
            .iter()
            .map(|(category, _)| *category)
            .collect::<Vec<_>>(),
        vec!["Constants", "Inputs", "Math"]
    );
    assert_eq!(
        categories[0]
            .1
            .iter()
            .map(|option| option.descriptor.label.as_str())
            .collect::<Vec<_>>(),
        vec!["Float", "Color"]
    );
}
#[test]
fn tab_preserves_hovered_view_coordinates_target_and_scope() {
    let mut a = app();
    let id = viewport(&mut a, Some(Vec2::new(0.25, -0.25)), true);
    a.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Tab);
    a.update();
    let p = a.world().resource::<MaterialGraphPaletteState>();
    let o = p.open.as_ref().unwrap();
    assert_eq!(o.program, id);
    assert_eq!(o.scope, Some(EditorViewId(7)));
    assert_eq!(o.menu_position, Vec2::new(300.0, 50.0));
    assert_eq!(o.graph_position, Vec2::new(140.0, 20.0));
    assert_eq!(o.graph_key, material_graph_view_key(id));
    assert!(o.connection.is_none());
    assert_eq!(
        o.editing_target,
        MaterialEditingTarget::Program {
            root: "standalone".into(),
            id
        }
    );
}
#[test]
fn tab_requires_hover_and_normalized_cursor() {
    for (cursor, hover) in [(None, true), (Some(Vec2::ZERO), false)] {
        let mut a = app();
        viewport(&mut a, cursor, hover);
        a.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Tab);
        a.update();
        assert!(!a.world().resource::<MaterialGraphPaletteState>().is_open());
    }
}
#[test]
fn existing_palette_is_not_reanchored() {
    let mut a = app();
    viewport(&mut a, Some(Vec2::ZERO), true);
    let id = MaterialProgramId::new();
    a.world_mut()
        .resource_mut::<MaterialGraphPaletteState>()
        .open = Some(opened(id));
    a.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Tab);
    a.update();
    assert_eq!(
        a.world()
            .resource::<MaterialGraphPaletteState>()
            .open
            .as_ref()
            .unwrap()
            .program,
        id
    );
}
#[test]
fn opening_clears_context_state_and_search_without_semantic_edits() {
    let mut a = app();
    viewport(&mut a, Some(Vec2::ZERO), true);
    let mut state = MaterialGraphPaletteState::blocking_states_for_test().remove(1);
    state.query = "old".into();
    a.insert_resource(state);
    let before = a.world().resource::<EditorSession>().effect.clone();
    let rev = a.world().resource::<EditorSession>().document_revision();
    a.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Tab);
    a.update();
    let p = a.world().resource::<MaterialGraphPaletteState>();
    assert!(
        p.open.is_some()
            && p.node_menu.is_none()
            && p.connection_menu.is_none()
            && p.query.is_empty()
    );
    let s = a.world().resource::<EditorSession>();
    assert_eq!(s.effect, before);
    assert_eq!(s.document_revision(), rev);
    assert!(!s.dirty && !s.can_undo());
}
#[test]
fn escape_clears_each_open_state_and_does_not_repeat_ui_changes() {
    for mut state in MaterialGraphPaletteState::blocking_states_for_test() {
        state.query = "old".into();
        let mut a = app();
        a.insert_resource(state);
        a.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        a.update();
        assert!(!a.world().resource::<MaterialGraphPaletteState>().is_open());
        assert!(
            a.world()
                .resource::<MaterialGraphPaletteState>()
                .query
                .is_empty()
        );
        let rev = a.world().resource::<EditorSession>().ui_revision;
        a.update();
        assert_eq!(a.world().resource::<EditorSession>().ui_revision, rev);
    }
}
#[test]
fn hover_on_every_palette_or_submenu_surface_prevents_left_click_dismissal() {
    for kind in 0..4 {
        let mut a = app();
        a.world_mut()
            .resource_mut::<MaterialGraphPaletteState>()
            .open = Some(opened(MaterialProgramId::new()));
        let e = a
            .world_mut()
            .spawn(RelativeCursorPosition {
                cursor_over: true,
                normalized: None,
            })
            .id();
        match kind {
            0 => {
                a.world_mut().entity_mut(e).insert(MaterialGraphPalette);
            }
            1 => {
                a.world_mut().entity_mut(e).insert(MaterialGraphNodeMenu);
            }
            2 => {
                a.world_mut()
                    .entity_mut(e)
                    .insert(MaterialGraphConnectionMenu);
            }
            _ => {
                a.world_mut()
                    .entity_mut(e)
                    .insert(PointerContextSubmenuSurface);
            }
        }
        a.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        a.update();
        assert!(a.world().resource::<MaterialGraphPaletteState>().is_open());
        a.world_mut()
            .get_mut::<RelativeCursorPosition>(e)
            .unwrap()
            .cursor_over = false;
        a.update();
        assert!(!a.world().resource::<MaterialGraphPaletteState>().is_open());
    }
}
#[test]
fn right_click_keeps_palette_but_escape_dismisses_even_when_hovered() {
    let mut a = app();
    a.world_mut()
        .resource_mut::<MaterialGraphPaletteState>()
        .open = Some(opened(MaterialProgramId::new()));
    a.world_mut().spawn((
        MaterialGraphPalette,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: None,
        },
    ));
    a.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    a.update();
    assert!(a.world().resource::<MaterialGraphPaletteState>().is_open());
    a.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    a.update();
    assert!(!a.world().resource::<MaterialGraphPaletteState>().is_open());
}
fn action(searchable: &str) -> MaterialGraphPaletteAction {
    let o = opened(MaterialProgramId::new());
    MaterialGraphPaletteAction {
        program: o.program,
        editing_target: o.editing_target,
        scope: o.scope,
        kind: MaterialGraphCreateKind::Constant(MaterialValueType::Float),
        source: None,
        target: None,
        label: searchable.into(),
        graph_position: o.graph_position,
        graph_key: o.graph_key,
        searchable: searchable.into(),
    }
}
fn rows(a: &mut App) -> (Entity, Entity, Entity, Entity) {
    let s = a.world_mut().spawn(MaterialGraphPaletteSearch).id();
    let yes = a
        .world_mut()
        .spawn((Node::default(), action("math add")))
        .id();
    let no = a
        .world_mut()
        .spawn((Node::default(), action("constants float")))
        .id();
    let empty = a
        .world_mut()
        .spawn((Node::default(), MaterialGraphPaletteEmptySearch))
        .id();
    (s, yes, no, empty)
}
fn search(a: &mut App, e: Entity, q: &str) {
    a.world_mut().trigger(ValueChange::<String> {
        source: e,
        value: q.into(),
        is_final: false,
    });
    a.world_mut().flush();
}
#[test]
fn search_normalizes_filters_categories_and_keeps_entities() {
    let mut a = app();
    let (s, yes, no, empty) = rows(&mut a);
    let category = a
        .world_mut()
        .spawn((
            Node::default(),
            MaterialGraphPaletteCategory {
                searchable: "math add".into(),
            },
        ))
        .id();
    let count = a.world().entities().len();
    search(&mut a, s, "  MaTh  ");
    assert_eq!(
        a.world().resource::<MaterialGraphPaletteState>().query,
        "math"
    );
    for (e, display) in [
        (yes, Display::Flex),
        (no, Display::None),
        (empty, Display::None),
        (category, Display::Flex),
    ] {
        assert_eq!(a.world().get::<Node>(e).unwrap().display, display);
    }
    assert_eq!(a.world().entities().len(), count);
}
#[test]
fn unmatched_query_shows_empty_and_clear_restores_all_rows() {
    let mut a = app();
    let (s, yes, no, empty) = rows(&mut a);
    search(&mut a, s, "no match");
    assert_eq!(a.world().get::<Node>(empty).unwrap().display, Display::Flex);
    assert_eq!(a.world().get::<Node>(yes).unwrap().display, Display::None);
    search(&mut a, s, "");
    assert_eq!(a.world().get::<Node>(empty).unwrap().display, Display::None);
    for e in [yes, no] {
        assert_eq!(a.world().get::<Node>(e).unwrap().display, Display::Flex);
    }
}
#[test]
fn unrelated_value_change_does_not_filter() {
    let mut a = app();
    let (_, yes, _, _) = rows(&mut a);
    let e = a.world_mut().spawn_empty().id();
    search(&mut a, e, "unrelated");
    assert!(
        a.world()
            .resource::<MaterialGraphPaletteState>()
            .query
            .is_empty()
    );
    assert_eq!(a.world().get::<Node>(yes).unwrap().display, Display::Flex);
}
#[test]
fn search_focus_is_added_only_and_does_not_steal_later_focus() {
    let mut a = app();
    let e = a.world_mut().spawn(MaterialGraphPaletteSearch).id();
    a.update();
    assert_eq!(a.world().resource::<InputFocus>().get(), Some(e));
    let other = a.world_mut().spawn_empty().id();
    a.world_mut()
        .resource_mut::<InputFocus>()
        .set(other, FocusCause::Navigated);
    a.update();
    assert_eq!(a.world().resource::<InputFocus>().get(), Some(other));
}
fn semantic_fixture() -> (
    MaterialProgram,
    MaterialAuthoringDocument,
    MaterialGraphProjection,
    Vec<MaterialGraphNodeDescriptor>,
) {
    let p = MaterialProgram::additive_sprite("Palette").normalized();
    let d =
        MaterialAuthoringDocument::new(test_support::effect_with_timing_slack(), vec![p.clone()]);
    let graph = MaterialCompiler.project_graph(&p, None);
    let descriptors = MaterialCompiler.graph_node_catalog(&p);
    (p, d, graph, descriptors)
}
#[test]
fn plain_catalog_is_ordered_and_contains_only_real_semantically_valid_commands() {
    let (p, d, g, descriptors) = semantic_fixture();
    let options = select_palette_operations(&d, p.id, &g, &descriptors, None);
    assert!(!options.is_empty());
    let expected = descriptors
        .iter()
        .filter(|x| {
            MaterialToolPlanner::plan(
                &d,
                MaterialToolCommand::CreateMaterialGraphNode {
                    program: p.id,
                    kind: x.kind,
                    source: None,
                    target: None,
                },
            )
            .is_ok()
        })
        .map(|x| x.label.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        options
            .iter()
            .map(|x| x.descriptor.label.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(
        options
            .iter()
            .all(|x| x.source.is_none() && x.target.is_none())
    );
}
#[test]
fn output_socket_options_only_consume_the_original_source() {
    let (p, d, g, descriptors) = semantic_fixture();
    let options = select_palette_operations(
        &d,
        p.id,
        &g,
        &descriptors,
        Some(MaterialGraphPaletteConnection::FromOutput(p.outputs.color)),
    );
    assert!(!options.is_empty());
    assert!(options.iter().all(|x| x.descriptor.kind.consumes_source()
        && x.source == Some(p.outputs.color)
        && x.target.is_none()));
}
#[test]
fn input_socket_options_carry_the_target_and_reuse_source_only_for_consumers() {
    let (p, d, g, descriptors) = semantic_fixture();
    let target = MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color);
    let options = select_palette_operations(
        &d,
        p.id,
        &g,
        &descriptors,
        Some(MaterialGraphPaletteConnection::FromInput(target)),
    );
    assert!(!options.is_empty());
    assert!(options.iter().all(|x| x.target == Some(target)));
    assert!(
        options
            .iter()
            .any(|x| x.descriptor.kind.consumes_source() && x.source == Some(p.outputs.color))
    );
    assert!(
        options
            .iter()
            .any(|x| !x.descriptor.kind.consumes_source() && x.source.is_none())
    );
}
#[test]
fn stale_program_and_output_source_produce_no_options() {
    let (p, d, g, descriptors) = semantic_fixture();
    assert!(
        select_palette_operations(&d, MaterialProgramId::new(), &g, &descriptors, None).is_empty()
    );
    assert!(
        select_palette_operations(
            &d,
            p.id,
            &g,
            &descriptors,
            Some(MaterialGraphPaletteConnection::FromOutput(
                MaterialExpressionId::new()
            ))
        )
        .is_empty()
    );
}
#[test]
fn restored_target_builds_its_own_catalog_without_switching_focused_effect() {
    let root = tempfile::tempdir().unwrap();
    let p = MaterialProgram::additive_sprite("Restored").normalized();
    p.save_ron(root.path().join("palette.aestra.material.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let s = test_support::session_with_timing_slack();
    let before = s.effect.clone();
    let target = MaterialEditingTarget::Program {
        root: root.path().to_owned(),
        id: p.id,
    };
    let g = MaterialCompiler.project_graph(&p, None);
    assert!(!material_graph_palette_options(&s, &catalog, &target, p.id, &g, None).is_empty());
    assert!(
        material_graph_palette_options(&s, &catalog, &target, MaterialProgramId::new(), &g, None)
            .is_empty()
    );
    assert_eq!(s.material_target, MaterialEditingTarget::EffectInstance);
    assert_eq!(s.effect, before);
    assert!(!s.dirty && !s.can_undo());
}
#[test]
fn edge_mapping_keeps_named_and_output_targets_and_rejects_unknown_ports() {
    use aestra_authoring::MaterialExpressionInput as I;
    use aestra_compiler::{MaterialGraphEdgeTarget as E, MaterialGraphOutputKind as O};
    let id = MaterialExpressionId::new();
    assert_eq!(
        edge_target(&E::Input {
            expression: id,
            port: "left".into()
        }),
        Some(MaterialConnectionTarget::ExpressionInput {
            expression: id,
            input: I::Left
        })
    );
    assert_eq!(
        edge_target(&E::Input {
            expression: id,
            port: "unknown".into()
        }),
        None
    );
    for (o, socket) in [
        (O::Color, MaterialOutputSocket::Color),
        (O::Alpha, MaterialOutputSocket::Alpha),
        (O::VertexOffset, MaterialOutputSocket::VertexOffset),
    ] {
        assert_eq!(
            edge_target(&E::Output(o)),
            Some(MaterialConnectionTarget::ProgramOutput(socket))
        );
    }
}
struct Composition {
    app: App,
    open: MaterialGraphPaletteOpen,
}
impl Composition {
    fn new(locale: &str, options: Vec<MaterialGraphPaletteOption>) -> Self {
        let mut a = crate::feathers::text_input::tests::app_with(|a| {
            a.init_resource::<MaterialGraphPaletteState>()
                .add_observer(update_material_graph_palette_search)
                .add_observer(crate::feathers::search_field::clear_search_field)
                .add_systems(
                    Update,
                    (
                        focus_material_graph_palette_search,
                        crate::feathers::search_field::sync_search_clear_visibility,
                    ),
                );
        });
        let open = opened(MaterialProgramId::new());
        let l = Localizer::new(locale).unwrap();
        a.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|p| spawn_material_graph_palette(p, &open, &options, "", &l));
        a.world_mut().flush();
        for _ in 0..3 {
            a.update();
        }
        Self { app: a, open }
    }
    fn search(&mut self) -> Entity {
        let w = self.app.world_mut();
        w.query_filtered::<Entity, With<MaterialGraphPaletteSearch>>()
            .single(w)
            .unwrap()
    }
}
#[test]
fn popup_has_real_feathers_menu_items_search_scroll_and_action_payloads() {
    let mut f = Composition::new(
        "en-US",
        vec![option("Float", "Constants"), option("Add", "Math")],
    );
    let w = f.app.world_mut();
    let (e, node) = w
        .query_filtered::<(Entity, &Node), With<MaterialGraphPalette>>()
        .single(w)
        .unwrap();
    assert_eq!(node.width, Val::Px(268.0));
    assert!(w.get::<FeathersGraphNavigationBlocker>(e).is_some());
    assert!(w.get::<bevy::ui_widgets::MenuPopup>(e).is_some());
    let rows = w
        .query::<(
            &MaterialGraphPaletteAction,
            &bevy::ui_widgets::MenuItem,
            &crate::feathers::action_activation::FeathersActionButton,
        )>()
        .iter(w)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    for (a, _, _) in rows {
        assert_eq!(a.program, f.open.program);
        assert_eq!(a.editing_target, f.open.editing_target);
        assert_eq!(
            a.kind,
            MaterialGraphCreateKind::Constant(MaterialValueType::Float)
        );
        assert!(a.source.is_none() && a.target.is_none());
        assert!(a.label == "Float" || a.label == "Add");
        assert_eq!(a.scope, Some(EditorViewId(7)));
        assert_eq!(a.graph_position, Vec2::new(300.0, 180.0));
        assert_eq!(a.graph_key, "material:palette");
        assert!(!a.searchable.is_empty());
    }
    assert_eq!(
        w.query::<&bevy::ui_widgets::ScrollArea>().iter(w).count(),
        1
    );
}
#[test]
fn popup_accessible_search_and_empty_messages_use_both_locales() {
    for locale in ["en-US", "fr-FR"] {
        let l = Localizer::new(locale).unwrap();
        let mut f = Composition::new(locale, vec![]);
        let e = f.search();
        assert_eq!(
            f.app.world().get::<AccessibleLabel>(e).unwrap().0,
            l.text("material-graph-search-nodes")
        );
        assert!(
            f.app
                .world_mut()
                .query::<&Text>()
                .iter(f.app.world())
                .any(|t| t.0 == l.text("material-graph-no-compatible-nodes"))
        );
        let mut f = Composition::new(locale, vec![option("Float", "Constants")]);
        assert!(
            f.app
                .world_mut()
                .query::<&Text>()
                .iter(f.app.world())
                .any(|t| t.0 == l.text("material-graph-no-matching-nodes"))
        );
    }
}
#[test]
fn real_keyboard_search_filters_and_clear_restores_retained_rows_with_one_value_event() {
    use crate::feathers::text_input::tests::{Changes, focus, tap};
    use bevy::input::keyboard::Key;
    let mut f = Composition::new(
        "en-US",
        vec![option("Float", "Constants"), option("Add", "Math")],
    );
    let search = f.search();
    let input = f.app.world().get::<Children>(search).unwrap()[0];
    let before = f
        .app
        .world_mut()
        .query_filtered::<Entity, With<MaterialGraphPaletteAction>>()
        .iter(f.app.world())
        .collect::<Vec<_>>();
    focus(&mut f.app, input);
    tap(
        &mut f.app,
        KeyCode::KeyM,
        Key::Character("math".into()),
        Some("math"),
    );
    assert_eq!(
        f.app.world().resource::<MaterialGraphPaletteState>().query,
        "math"
    );
    for (a, n) in f
        .app
        .world_mut()
        .query::<(&MaterialGraphPaletteAction, &Node)>()
        .iter(f.app.world())
    {
        assert_eq!(
            n.display,
            if a.label == "Add" {
                Display::Flex
            } else {
                Display::None
            }
        );
    }
    let clear = {
        let w = f.app.world_mut();
        w.query_filtered::<Entity, With<crate::feathers::search_field::SearchFieldClear>>()
            .single(w)
            .unwrap()
    };
    f.app.world_mut().resource_mut::<Changes>().0.clear();
    f.app
        .world_mut()
        .trigger(bevy::ui_widgets::Activate { entity: clear });
    for _ in 0..3 {
        f.app.update();
    }
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPaletteState>()
            .query
            .is_empty()
    );
    assert_eq!(
        f.app.world().resource::<Changes>().0,
        vec![(search, String::new(), false)]
    );
    for (_, n) in f
        .app
        .world_mut()
        .query::<(&MaterialGraphPaletteAction, &Node)>()
        .iter(f.app.world())
    {
        assert_eq!(n.display, Display::Flex);
    }
    assert_eq!(
        f.app
            .world_mut()
            .query_filtered::<Entity, With<MaterialGraphPaletteAction>>()
            .iter(f.app.world())
            .collect::<Vec<_>>(),
        before
    );
}
