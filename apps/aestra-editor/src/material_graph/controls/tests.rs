//! Shared production action/value/focus consumers, actual presentation and semantic history.
//! These tests do not install the complete panel/picking/render schedule.
use super::*;
use crate::{
    docking::EditorViewId,
    feathers::action_activation::{FeathersActionButton, PendingFeathersActivation},
    feathers::node_graph::{GraphSelectionMode, GraphViewportMemory, geometry::GraphNodeKey},
    history::{HistoryAction, routing::Routing},
    material_document::MaterialEditingTarget,
    material_function_editor::FunctionEditor,
    material_graph::{
        MaterialSelectionScope, click_input,
        menu_actions::queue_material_graph_menu_action_activation,
        node_controls::MaterialGraphNodeScope,
    },
};
use aestra_core::material::{MaterialExpression, MaterialProgram};
use aestra_project::ProjectAssetId;
use bevy::ui_widgets::Activate;
use std::collections::BTreeSet;

#[derive(Resource, Default)]
struct Observed {
    locate: Vec<ProjectAssetId>,
    arrange: Vec<arrange::ArrangeGraph>,
    updates: Vec<(Entity, NumberInputValue)>,
    bubbled: usize,
}
struct Fixture {
    root: tempfile::TempDir,
    app: App,
    original: MaterialProgram,
    ids: [MaterialExpressionId; 3],
    scope: MaterialSelectionScope,
    viewport: Entity,
    window: Entity,
    parent: Entity,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let ids = std::array::from_fn(|_| MaterialExpressionId::new());
        let mut original = MaterialProgram::additive_sprite("Control actions").normalized();
        original.expressions.extend([
            MaterialExpression {
                id: ids[0],
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
            },
            MaterialExpression {
                id: ids[1],
                kind: MaterialExpressionKind::Constant(MaterialValue::Vec3([0.1, 0.2, 0.3])),
            },
            MaterialExpression {
                id: ids[2],
                kind: MaterialExpressionKind::Constant(MaterialValue::Bool(true)),
            },
        ]);
        // Keep these explicit constant nodes in the authored graph during planner cleanup.
        original.node_constants.extend(ids);
        original = original.normalized();
        original
            .save_ron(root.path().join("controls.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session
            .open_material_program(&catalog, original.id)
            .unwrap();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<FunctionEditor>()
            .init_resource::<GraphViewportMemory>()
            .init_resource::<MaterialGraphSelectionState>()
            .init_resource::<MaterialGraphPreviewState>()
            .init_resource::<MaterialGraphPaletteState>()
            .init_resource::<InputFocus>()
            .init_resource::<Observed>()
            .add_observer(queue_material_graph_menu_action_activation)
            .add_observer(handle_material_graph_default_number_change)
            .add_observer(handle_material_graph_default_toggle_change)
            .add_observer(
                |e: On<crate::asset_browser::LocateInAssets>, mut o: ResMut<Observed>| {
                    o.locate.push(e.0)
                },
            )
            .add_observer(|e: On<arrange::ArrangeGraph>, mut o: ResMut<Observed>| {
                o.arrange.push(e.event().clone())
            })
            .add_observer(|e: On<UpdateNumberInput>, mut o: ResMut<Observed>| {
                o.updates.push((e.entity, e.value))
            })
            .add_observer(
                |e: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*e, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history")
                    });
                },
            )
            .add_systems(
                Update,
                (
                    handle_material_graph_toolbar_actions,
                    handle_material_graph_preview_actions,
                    sync_material_graph_default_number_inputs,
                )
                    .chain(),
            );
        let window = app.world_mut().spawn(Window::default()).id();
        let parent = app
            .world_mut()
            .spawn_empty()
            .observe(|_: On<PointerClick>, mut o: ResMut<Observed>| o.bubbled += 1)
            .observe(|_: On<PointerPress>, mut o: ResMut<Observed>| o.bubbled += 1)
            .id();
        let scope = Some(EditorViewId(7));
        let viewport = app
            .world_mut()
            .spawn((
                MaterialGraphViewport {
                    program: original.id,
                    scope,
                    editing_target: MaterialEditingTarget::Program {
                        root: root.path().into(),
                        id: original.id,
                    },
                },
                FeathersGraphViewport::clipboard_fixture(
                    material_graph_view_key(original.id),
                    Vec2::new(40.0, 20.0),
                    2.0,
                ),
                ComputedNode {
                    size: Vec2::new(800.0, 600.0),
                    inverse_scale_factor: 0.5,
                    ..default()
                },
            ))
            .id();
        for expression in ids {
            app.world_mut().spawn((
                MaterialGraphAction {
                    program: original.id,
                    expression,
                },
                MaterialGraphNodeScope(scope),
            ));
        }
        app.update();
        Self {
            root,
            app,
            original,
            ids,
            scope,
            viewport,
            window,
            parent,
        }
    }
    fn target(&self) -> MaterialEditingTarget {
        MaterialEditingTarget::Program {
            root: self.root.path().into(),
            id: self.original.id,
        }
    }
    fn revision(&self) -> u64 {
        self.app.world().resource::<EditorSession>().ui_revision
    }
    fn button<A: Component>(&mut self, action: A) -> Entity {
        self.app
            .world_mut()
            .spawn((FeathersActionButton, action, ChildOf(self.parent)))
            .id()
    }
    fn toolbar(&mut self, action: MaterialGraphToolbarAction) -> Entity {
        let button = self.button(action);
        self.activate(button);
        button
    }
    fn activate(&mut self, entity: Entity) {
        self.app.world_mut().trigger(Activate { entity });
        self.app.update();
        assert!(
            self.app
                .world()
                .get::<PendingFeathersActivation>(entity)
                .is_none()
        );
        assert!(!control_input::is_pressed(self.app.world(), entity));
    }
    fn preview(&mut self, target: MaterialGraphPreviewTarget) -> Entity {
        let action = MaterialGraphPreviewToggle {
            program: self.original.id,
            editing_target: self.target(),
            target,
        };
        let button = self.button(action);
        self.app
            .world_mut()
            .entity_mut(button)
            .insert(FeathersGraphNodePreviewToggle);
        button
    }
    fn number(&mut self, index: usize, value: MaterialValue, component: u8) -> Entity {
        let control = MaterialGraphDefaultNumberControl {
            program: self.original.id,
            editing_target: self.target(),
            expression: self.ids[index],
            value,
            component,
        };
        self.app
            .world_mut()
            .spawn((control, Node::default(), ChildOf(self.parent)))
            .id()
    }
    fn numeric(&mut self, source: Entity, value: f32, is_final: bool) {
        self.app.world_mut().trigger(ValueChange {
            source,
            value,
            is_final,
        });
        self.app.world_mut().flush();
    }
    fn toggle(&mut self) -> Entity {
        let editing_target = self.target();
        self.app
            .world_mut()
            .spawn(MaterialGraphDefaultToggleControl {
                program: self.original.id,
                editing_target,
                expression: self.ids[2],
                value: true,
            })
            .id()
    }
    fn boolean(&mut self, source: Entity, value: bool, is_final: bool) {
        self.app.world_mut().trigger(ValueChange {
            source,
            value,
            is_final,
        });
        self.app.world_mut().flush();
    }
    fn current(&self) -> MaterialProgram {
        self.app
            .world()
            .resource::<EditorSession>()
            .graph_authoring_document_for(
                &self.target(),
                self.app.world().resource::<ProjectEffectCatalog>(),
            )
            .unwrap()
            .programs
            .into_iter()
            .find(|p| p.id == self.original.id)
            .unwrap()
    }
    fn value(&self, index: usize) -> MaterialValue {
        let program = self.current();
        let MaterialExpressionKind::Constant(value) = &program
            .expressions
            .iter()
            .find(|e| e.id == self.ids[index])
            .unwrap()
            .kind
        else {
            panic!("Expected constant")
        };
        value.clone()
    }
    fn step(&mut self, action: HistoryAction) {
        self.app.world_mut().trigger(action);
        self.app.world_mut().flush();
    }
    fn disk_unchanged(&self) {
        assert_eq!(
            MaterialProgram::load_ron(self.root.path().join("controls.aestra.material.ron"))
                .unwrap(),
            self.original
        );
    }
    fn unchanged(&self) {
        assert_eq!(self.current(), self.original);
        assert_eq!(
            self.app
                .world()
                .resource::<EditorSession>()
                .operation_order
                .edit_serial(),
            0
        );
        self.disk_unchanged();
    }
    fn visible(&self, target: MaterialGraphPreviewTarget) -> bool {
        self.app
            .world()
            .resource::<MaterialGraphPreviewState>()
            .is_visible(self.original.id, target)
    }
}

#[test]
fn pointer_pressed_without_activate_cannot_dispatch_toolbar_or_preview() {
    let mut f = Fixture::new();
    let a = f.button(MaterialGraphToolbarAction::AddNode(f.original.id, f.scope));
    let b = f.preview(MaterialGraphPreviewTarget::Output);
    for entity in [a, b] {
        control_input::set_pressed(f.app.world_mut(), entity, true);
    }
    let revision = f.revision();
    f.app.update();
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPaletteState>()
            .open
            .is_none()
    );
    assert!(!f.visible(MaterialGraphPreviewTarget::Output));
    assert_eq!(f.revision(), revision);
    f.unchanged();
}
#[test]
fn add_node_activate_centers_with_inverse_scale_and_retained_view_target_once() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .material_target = MaterialEditingTarget::EffectInstance;
    let before = f.revision();
    f.toolbar(MaterialGraphToolbarAction::AddNode(f.original.id, f.scope));
    let palette = f.app.world().resource::<MaterialGraphPaletteState>();
    let open = palette.open.as_ref().unwrap();
    assert_eq!(open.menu_position, Vec2::new(200.0, 150.0));
    assert_eq!(open.graph_position, Vec2::new(80.0, 65.0));
    assert_eq!(open.editing_target, f.target());
    assert_eq!(open.scope, f.scope);
    assert!(open.connection.is_none());
    assert!(palette.query.is_empty());
    assert_eq!(f.revision(), before + 1);
    f.app.update();
    assert_eq!(f.revision(), before + 1);
    f.unchanged();
}
#[test]
fn toolbar_add_resolves_the_requested_scope_not_the_first_program_view() {
    let mut f = Fixture::new();
    let other_scope = Some(EditorViewId(88));
    let mut marker = f
        .app
        .world()
        .get::<MaterialGraphViewport>(f.viewport)
        .unwrap()
        .clone();
    marker.scope = other_scope;
    f.app.world_mut().spawn((
        marker,
        FeathersGraphViewport::clipboard_fixture(
            material_graph_view_key(f.original.id),
            Vec2::ZERO,
            1.0,
        ),
        ComputedNode {
            size: Vec2::new(100.0, 100.0),
            ..default()
        },
    ));
    f.toolbar(MaterialGraphToolbarAction::AddNode(
        f.original.id,
        other_scope,
    ));
    let open = f
        .app
        .world()
        .resource::<MaterialGraphPaletteState>()
        .open
        .as_ref()
        .unwrap();
    assert_eq!(open.scope, other_scope);
    assert_eq!(open.graph_position, Vec2::splat(50.0));
}
#[test]
fn missing_toolbar_view_consumes_activation_without_opening_or_revision() {
    let mut f = Fixture::new();
    let before = f.revision();
    f.toolbar(MaterialGraphToolbarAction::AddNode(
        f.original.id,
        Some(EditorViewId(99)),
    ));
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPaletteState>()
            .open
            .is_none()
    );
    assert_eq!(f.revision(), before);
    f.unchanged();
}
#[test]
fn arrange_emits_exact_retained_document_scope_and_seeds_without_rebuild() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .select_material_expressions(
            f.scope,
            f.original.id,
            &BTreeSet::from([f.ids[1]]),
            GraphSelectionMode::Replace,
        );
    let before = f.revision();
    f.toolbar(MaterialGraphToolbarAction::Arrange(
        f.original.id,
        f.scope,
        arrange::ArrangeScope::Selection,
    ));
    let o = f.app.world().resource::<Observed>();
    assert_eq!(o.arrange.len(), 1);
    let e = &o.arrange[0];
    assert_eq!(e.editing_target, f.target());
    assert_eq!(e.view.view, f.scope);
    assert_eq!(e.view.document.project, f.root.path());
    assert_eq!(
        e.view.document.asset,
        crate::document::DocumentKey::MaterialProgram(f.original.id)
    );
    assert_eq!(
        e.seeds,
        BTreeSet::from([GraphNodeKey::Expression(f.ids[1])])
    );
    assert_eq!(e.scope, arrange::ArrangeScope::Selection);
    assert_eq!(f.revision(), before);
    f.unchanged();
}
#[test]
fn missing_arrange_view_reports_unavailable_without_request_or_rebuild() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .entity_mut(f.viewport)
        .remove::<MaterialGraphViewport>();
    let before = f.revision();
    f.toolbar(MaterialGraphToolbarAction::Arrange(
        f.original.id,
        f.scope,
        arrange::ArrangeScope::Graph,
    ));
    assert!(f.app.world().resource::<Observed>().arrange.is_empty());
    assert_eq!(f.revision(), before);
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .starts_with("Arrange Graph unavailable:")
    );
}
#[test]
fn locate_source_emits_actual_asset_intent_once_without_rebuilding() {
    let mut f = Fixture::new();
    let before = f.revision();
    f.toolbar(MaterialGraphToolbarAction::LocateSource(f.original.id));
    assert_eq!(
        f.app.world().resource::<Observed>().locate,
        vec![ProjectAssetId::MaterialProgram(f.original.id)]
    );
    assert_eq!(f.revision(), before);
    f.app.update();
    assert_eq!(f.app.world().resource::<Observed>().locate.len(), 1);
    f.unchanged();
}
#[test]
fn effect_context_returns_to_effect_and_closes_open_palette() {
    let mut f = Fixture::new();
    f.toolbar(MaterialGraphToolbarAction::AddNode(f.original.id, f.scope));
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPaletteState>()
            .open
            .is_some()
    );
    f.toolbar(MaterialGraphToolbarAction::EffectContext);
    assert_eq!(
        f.app.world().resource::<EditorSession>().material_target,
        MaterialEditingTarget::EffectInstance
    );
    let p = f.app.world().resource::<MaterialGraphPaletteState>();
    assert!(p.open.is_none() && p.node_menu.is_none() && p.connection_menu.is_none());
    f.unchanged();
}
#[test]
fn preview_activation_records_actual_layout_history_and_undo_redo_not_semantics() {
    let mut f = Fixture::new();
    let target = MaterialGraphPreviewTarget::Expression(f.ids[0]);
    let b = f.preview(target);
    f.activate(b);
    assert!(f.visible(target));
    assert_eq!(f.current(), f.original);
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        1
    );
    f.step(HistoryAction::Undo);
    assert!(!f.visible(target));
    f.step(HistoryAction::Redo);
    assert!(f.visible(target));
    f.app.update();
    assert!(f.visible(target));
    f.disk_unchanged();
}
#[test]
fn preview_consumer_requires_original_node_toggle_marker() {
    let mut f = Fixture::new();
    let b = f.preview(MaterialGraphPreviewTarget::Output);
    f.app
        .world_mut()
        .entity_mut(b)
        .remove::<FeathersGraphNodePreviewToggle>();
    f.app.world_mut().trigger(Activate { entity: b });
    f.app.update();
    assert!(f.app.world().get::<PendingFeathersActivation>(b).is_some());
    assert!(!f.visible(MaterialGraphPreviewTarget::Output));
    f.unchanged();
}
#[test]
fn toggle_all_previews_shows_then_hides_only_target_program_with_real_history() {
    let mut f = Fixture::new();
    let other = aestra_core::MaterialProgramId::new();
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphPreviewState>()
        .set_visible(other, [MaterialGraphPreviewTarget::Output], true);
    let action = MaterialGraphToolbarAction::ToggleAllPreviews(f.original.id, f.scope);
    f.toolbar(action);
    assert!(f.visible(MaterialGraphPreviewTarget::Output));
    for id in f.ids {
        assert!(f.visible(MaterialGraphPreviewTarget::Expression(id)));
    }
    f.toolbar(action);
    assert!(!f.visible(MaterialGraphPreviewTarget::Output));
    for id in f.ids {
        assert!(!f.visible(MaterialGraphPreviewTarget::Expression(id)));
    }
    f.step(HistoryAction::Undo);
    assert!(f.visible(MaterialGraphPreviewTarget::Output));
    f.step(HistoryAction::Redo);
    assert!(!f.visible(MaterialGraphPreviewTarget::Output));
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPreviewState>()
            .is_visible(other, MaterialGraphPreviewTarget::Output)
    );
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}
#[test]
fn missing_toggle_all_view_reports_unavailable_without_visibility_change() {
    let mut f = Fixture::new();
    let before = f.revision();
    f.toolbar(MaterialGraphToolbarAction::ToggleAllPreviews(
        f.original.id,
        Some(EditorViewId(99)),
    ));
    assert!(!f.visible(MaterialGraphPreviewTarget::Output));
    assert_eq!(f.revision(), before);
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .starts_with("Preview controls unavailable:")
    );
    f.unchanged();
}
#[test]
fn preview_click_consumption_keeps_original_exact_target_and_primary_gate() {
    let mut f = Fixture::new();
    let b = f.preview(MaterialGraphPreviewTarget::Output);
    f.app
        .world_mut()
        .entity_mut(b)
        .observe(stop_material_graph_preview_toggle_click);
    for button in [PointerButton::Primary, PointerButton::Secondary] {
        f.app
            .world_mut()
            .trigger(click_input::click(b, f.window, Vec2::ZERO, button));
    }
    assert_eq!(f.app.world().resource::<Observed>().bubbled, 1);
    assert!(!f.visible(MaterialGraphPreviewTarget::Output));
}
#[test]
fn final_number_commits_retained_target_once_and_real_undo_redo_preserves_disk() {
    let mut f = Fixture::new();
    let c = f.number(0, MaterialValue::Float(0.5), 0);
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .material_target = MaterialEditingTarget::EffectInstance;
    f.numeric(c, 0.75, true);
    assert_eq!(f.value(0), MaterialValue::Float(0.75));
    assert_eq!(
        f.app.world().resource::<EditorSession>().material_target,
        f.target()
    );
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        1
    );
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), f.original);
    f.step(HistoryAction::Redo);
    assert_eq!(f.value(0), MaterialValue::Float(0.75));
    f.disk_unchanged();
}
#[test]
fn final_vector_number_changes_only_selected_component() {
    let mut f = Fixture::new();
    let c = f.number(1, MaterialValue::Vec3([0.1, 0.2, 0.3]), 1);
    f.numeric(c, 0.9, true);
    assert_eq!(f.value(1), MaterialValue::Vec3([0.1, 0.9, 0.3]));
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}
#[test]
fn numeric_live_noop_nonfinite_invalid_component_and_unmarked_source_do_not_commit() {
    let mut f = Fixture::new();
    let c = f.number(0, MaterialValue::Float(0.5), 0);
    f.numeric(c, 0.8, false);
    f.numeric(c, 0.5, true);
    f.numeric(c, f32::NAN, true);
    f.numeric(c, f32::INFINITY, true);
    let invalid = f.number(0, MaterialValue::Float(0.5), 1);
    f.numeric(invalid, 0.8, true);
    let unsupported = f.number(2, MaterialValue::Bool(true), 0);
    f.numeric(unsupported, 0.8, true);
    let plain = f.app.world_mut().spawn_empty().id();
    f.numeric(plain, 0.8, true);
    f.unchanged();
}
#[test]
fn final_toggle_commits_and_undo_redo_uses_actual_semantic_history() {
    let mut f = Fixture::new();
    let c = f.toggle();
    f.boolean(c, false, true);
    assert_eq!(f.value(2), MaterialValue::Bool(false));
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        1
    );
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), f.original);
    f.step(HistoryAction::Redo);
    assert_eq!(f.value(2), MaterialValue::Bool(false));
    f.disk_unchanged();
}
#[test]
fn live_unchanged_and_unmarked_toggles_do_not_commit() {
    let mut f = Fixture::new();
    let c = f.toggle();
    f.boolean(c, false, false);
    f.boolean(c, true, true);
    let plain = f.app.world_mut().spawn_empty().id();
    f.boolean(plain, false, true);
    f.unchanged();
}
#[test]
fn missing_expression_rejects_numeric_publication_and_history() {
    let mut f = Fixture::new();
    let c = f.number(0, MaterialValue::Float(0.5), 0);
    f.app
        .world_mut()
        .get_mut::<MaterialGraphDefaultNumberControl>(c)
        .unwrap()
        .expression = MaterialExpressionId::new();
    f.numeric(c, 0.75, true);
    f.unchanged();
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .starts_with("Could not edit material input default:")
    );
}
#[test]
fn mismatched_retained_program_rejects_numeric_edit_without_history() {
    let mut f = Fixture::new();
    let c = f.number(0, MaterialValue::Float(0.5), 0);
    f.app
        .world_mut()
        .get_mut::<MaterialGraphDefaultNumberControl>(c)
        .unwrap()
        .program = aestra_core::MaterialProgramId::new();
    let before = f.revision();
    f.numeric(c, 0.75, true);
    f.unchanged();
    assert_eq!(f.revision(), before + 1);
}
#[test]
fn value_component_helpers_preserve_type_other_components_and_bounds() {
    for original in [
        MaterialValue::Float(0.1),
        MaterialValue::Vec2([0.1, 0.2]),
        MaterialValue::Vec3([0.1, 0.2, 0.3]),
        MaterialValue::Vec4([0.1, 0.2, 0.3, 0.4]),
        MaterialValue::ColorSrgb([0.1, 0.2, 0.3, 0.4]),
        MaterialValue::Bool(true),
    ] {
        for component in 0..5 {
            let mut value = original.clone();
            let old = material_graph_value_component(&value, component);
            assert_eq!(
                set_material_graph_value_component(&mut value, component, 0.9),
                old.is_some()
            );
            if old.is_some() {
                assert_eq!(material_graph_value_component(&value, component), Some(0.9));
            } else {
                assert_eq!(value, original);
            }
            for other in 0..5 {
                if other != component {
                    assert_eq!(
                        material_graph_value_component(&value, other),
                        material_graph_value_component(&original, other)
                    );
                }
            }
            assert_eq!(
                std::mem::discriminant(&value),
                std::mem::discriminant(&original)
            );
        }
    }
}
#[test]
fn primary_numeric_descendant_press_focuses_real_editable_and_consumes() {
    let mut f = Fixture::new();
    let c = f.number(0, MaterialValue::Float(0.5), 0);
    let text = f
        .app
        .world_mut()
        .spawn((EditableText::new("0.5"), ChildOf(c)))
        .id();
    let surface = f.app.world_mut().spawn(ChildOf(c)).id();
    f.app
        .world_mut()
        .entity_mut(surface)
        .observe(focus_material_graph_number_input);
    f.app.world_mut().trigger(click_input::press(
        surface,
        f.window,
        PointerButton::Primary,
    ));
    assert_eq!(f.app.world().resource::<InputFocus>().get(), Some(text));
    assert_eq!(f.app.world().resource::<Observed>().bubbled, 0);
    f.unchanged();
}
#[test]
fn secondary_or_missing_editable_numeric_press_keeps_focus_and_bubbles() {
    let mut f = Fixture::new();
    let c = f.number(0, MaterialValue::Float(0.5), 0);
    f.app
        .world_mut()
        .entity_mut(c)
        .observe(focus_material_graph_number_input);
    f.app
        .world_mut()
        .trigger(click_input::press(c, f.window, PointerButton::Primary));
    f.app
        .world_mut()
        .trigger(click_input::press(c, f.window, PointerButton::Secondary));
    assert_eq!(f.app.world().resource::<InputFocus>().get(), None);
    assert_eq!(f.app.world().resource::<Observed>().bubbled, 2);
    f.unchanged();
}
#[test]
fn added_numeric_control_syncs_typed_value_once_and_constrains_real_descendant() {
    let mut f = Fixture::new();
    let c = f.number(1, MaterialValue::Vec3([0.1, 0.2, 0.3]), 2);
    let text = f
        .app
        .world_mut()
        .spawn((
            EditableText::new("old"),
            Node::default(),
            TextLayout::default(),
            ChildOf(c),
        ))
        .id();
    control_input::add_legacy_text_clip(f.app.world_mut(), text);
    f.app.update();
    let o = f.app.world().resource::<Observed>();
    assert_eq!(o.updates.len(), 1);
    assert_eq!(o.updates[0].0, c);
    assert_eq!(o.updates[0].1, NumberInputValue::F32(0.3));
    for entity in [c, text] {
        let n = f.app.world().get::<Node>(entity).unwrap();
        assert_eq!(n.width, Val::Percent(100.0));
        assert_eq!(n.min_width, Val::Px(0.0));
        assert_eq!(n.max_width, Val::Percent(100.0));
        assert_eq!(n.overflow, Overflow::clip());
    }
    assert!(control_input::legacy_text_clip_absent(f.app.world(), text));
    assert_eq!(
        f.app.world().get::<TextLayout>(text).unwrap().justify,
        Justify::Center
    );
    f.app.update();
    assert_eq!(f.app.world().resource::<Observed>().updates.len(), 1);
    f.unchanged();
}
#[test]
fn unsupported_added_numeric_value_is_constrained_without_sync_or_semantic_edit() {
    let mut f = Fixture::new();
    let c = f.number(2, MaterialValue::Bool(true), 0);
    f.app.update();
    assert!(f.app.world().resource::<Observed>().updates.is_empty());
    assert_eq!(
        f.app.world().get::<Node>(c).unwrap().width,
        Val::Percent(100.0)
    );
    f.unchanged();
}
