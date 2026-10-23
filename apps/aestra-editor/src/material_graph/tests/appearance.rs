use super::*;
use crate::{
    docking::EditorViewId,
    material_graph::{MaterialGraphAction, node_controls::MaterialGraphNodeScope},
};
use aestra_core::{MaterialExpressionId, MaterialProgramId};

struct Fixture {
    app: App,
    node: Entity,
    program: MaterialProgramId,
    expression: MaterialExpressionId,
}
impl Fixture {
    fn new(selected: bool) -> Self {
        let mut app = App::new();
        app.init_resource::<MaterialGraphSelectionState>()
            .add_systems(Update, handle_material_graph_actions);
        let program = MaterialProgramId::new();
        let expression = MaterialExpressionId::new();
        if selected {
            app.world_mut()
                .resource_mut::<MaterialGraphSelectionState>()
                .select_single(Some(EditorViewId(7)), program, expression);
        }
        let node = app
            .world_mut()
            .spawn((
                appearance_input::NodeButton,
                MaterialGraphAction {
                    program,
                    expression,
                },
                MaterialGraphNodeScope(Some(EditorViewId(7))),
                BackgroundColor(Color::WHITE),
            ))
            .id();
        appearance_input::set_phase(app.world_mut(), node, Phase::None);
        app.update();
        Self {
            app,
            node,
            program,
            expression,
        }
    }
    fn phase(&mut self, phase: Phase) {
        appearance_input::set_phase(self.app.world_mut(), self.node, phase);
        self.app.update();
    }
    fn color(&self) -> Color {
        self.app
            .world()
            .get::<BackgroundColor>(self.node)
            .unwrap()
            .0
    }
}
#[test]
fn idle_selected_and_unselected_nodes_keep_exact_theme_backgrounds() {
    for selected in [false, true] {
        let f = Fixture::new(selected);
        assert_eq!(
            f.color(),
            if selected {
                theme::SELECTION
            } else {
                theme::PANEL
            }
        );
    }
}
#[test]
fn hover_press_release_leave_restore_selected_and_unselected_backgrounds() {
    for selected in [false, true] {
        let mut f = Fixture::new(selected);
        f.phase(Phase::Hovered);
        assert_eq!(f.color(), theme::BUTTON_HOVER);
        f.phase(Phase::Pressed);
        assert_eq!(f.color(), theme::ACCENT_DIM);
        f.phase(Phase::Hovered);
        assert_eq!(f.color(), theme::BUTTON_HOVER);
        appearance_input::clear_hover(f.app.world_mut(), f.node);
        f.app.update();
        assert_eq!(
            f.color(),
            if selected {
                theme::SELECTION
            } else {
                theme::PANEL
            }
        );
    }
}
#[test]
fn another_view_or_program_selection_does_not_color_this_node() {
    let mut f = Fixture::new(false);
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .select_single(Some(EditorViewId(88)), f.program, f.expression);
    f.phase(Phase::None);
    assert_eq!(f.color(), theme::PANEL);
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .select_single(
            Some(EditorViewId(7)),
            MaterialProgramId::new(),
            f.expression,
        );
    f.phase(Phase::None);
    assert_eq!(f.color(), theme::PANEL);
}
#[test]
fn absent_scope_falls_back_to_tool_selection() {
    let mut f = Fixture::new(false);
    f.app
        .world_mut()
        .entity_mut(f.node)
        .remove::<MaterialGraphNodeScope>();
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .select_single(None, f.program, f.expression);
    f.phase(Phase::None);
    assert_eq!(f.color(), theme::SELECTION);
}
#[test]
fn unclassified_action_entity_is_not_restyled() {
    let mut f = Fixture::new(false);
    let e = f
        .app
        .world_mut()
        .spawn((
            MaterialGraphAction {
                program: f.program,
                expression: f.expression,
            },
            BackgroundColor(Color::WHITE),
        ))
        .id();
    appearance_input::set_phase(f.app.world_mut(), e, Phase::Hovered);
    f.app.update();
    assert_eq!(
        f.app.world().get::<BackgroundColor>(e).unwrap().0,
        Color::WHITE
    );
}
#[test]
fn idle_updates_do_not_rewrite_background_or_selection() {
    let mut f = Fixture::new(true);
    f.phase(Phase::Hovered);
    let tick = f
        .app
        .world()
        .entity(f.node)
        .get_ref::<BackgroundColor>()
        .unwrap()
        .last_changed();
    f.app.update();
    f.app.update();
    assert_eq!(
        f.app
            .world()
            .entity(f.node)
            .get_ref::<BackgroundColor>()
            .unwrap()
            .last_changed(),
        tick
    );
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .is_expression_selected(Some(EditorViewId(7)), f.program, f.expression)
    );
}
