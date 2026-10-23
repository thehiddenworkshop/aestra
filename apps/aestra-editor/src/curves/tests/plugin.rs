//! Install the real plugin: resource ownership, executable schedule and activation delivery.
use super::super::{control_input, controls::Phase, host_editing};
use super::*;
use crate::{
    EditorModuleRegistry, EditorSession, ToolPanel, WorkspaceLayout,
    curves::CurvesAction,
    feathers::action_activation::{FeathersActionButton, PendingFeathersActivation},
    test_support,
    timeline::TimelineState,
};
use aestra_core::ModuleId;
use bevy::ui_widgets::Activate;

fn app() -> (App, ModuleId, u8) {
    let session = test_support::session_with_timing_slack();
    let registry = EditorModuleRegistry::default();
    let module = session
        .selected_layer()
        .unwrap()
        .modules
        .iter()
        .find(|m| m.parameter_value("size").is_some())
        .unwrap();
    let module_id = module.id;
    let input = registry
        .0
        .get(&module.module_type)
        .unwrap()
        .inputs
        .iter()
        .position(|i| i.name == "size")
        .unwrap() as u8;
    let mut layout = WorkspaceLayout::default();
    layout.show(ToolPanel::Curves);
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(registry)
        .insert_resource(layout)
        .init_resource::<TimelineState>()
        .add_plugins(EditorCurvesPlugin);
    (app, module_id, input)
}

#[test]
fn installation_initializes_missing_state_and_preserves_existing_selection() {
    let (mut first, module, input) = app();
    assert!(first.world().contains_resource::<CurvesState>());
    assert!(
        first
            .world()
            .resource::<CurvesState>()
            .selected_key()
            .is_none()
    );
    let mut state = CurvesState::default();
    state.select_key_channel(module, input, 2, Some(2));
    let mut second = App::new();
    second
        .insert_resource(state)
        .add_plugins(EditorCurvesPlugin);
    let preserved = second.world().resource::<CurvesState>();
    assert_eq!(preserved.selected_key().unwrap().key, 2);
    assert_eq!(preserved.selected_vector_channel(), 2);
    first.world_mut().run_schedule(Update);
    assert!(!first.world().resource::<EditorSession>().dirty);
}

#[test]
fn installed_schedule_has_exact_systems_membership_and_original_dependencies() {
    let (mut app, _, _) = app();
    app.world_mut().schedule_scope(Update, |world, schedule| {
        schedule.initialize(world).unwrap();
        let order = schedule
            .systems()
            .unwrap()
            .map(|(key, _)| key)
            .collect::<Vec<_>>();
        assert_eq!(order.len(), 3);
        macro_rules! index {
            ($system:expr) => {{
                let keys = schedule
                    .graph()
                    .systems_in_set($system.into_system_set().intern())
                    .unwrap();
                assert_eq!(keys.len(), 1);
                order
                    .iter()
                    .position(|key| key == keys.first().unwrap())
                    .unwrap()
            }};
        }
        assert!(index!(curves_keyboard_input) < index!(handle_curves_actions));
        assert!(index!(host_editing::cancel_drag) < index!(handle_curves_actions));
        let members = schedule
            .graph()
            .systems_in_set(CurvesSet::Actions.intern())
            .unwrap();
        assert_eq!(members.len(), 3);
        assert!(order.iter().all(|key| members.contains(key)));
    });
}

#[derive(Resource)]
struct Enabled(bool);
#[test]
fn installed_set_gate_defers_activation_then_consumes_it_exactly_once() {
    let (mut app, module, input) = app();
    app.insert_resource(Enabled(false)).configure_sets(
        Update,
        CurvesSet::Actions.run_if(|gate: Res<Enabled>| gate.0),
    );
    let before = app.world().resource::<EditorSession>().effect.clone();
    let button = app
        .world_mut()
        .spawn((
            control_input::NativeButton,
            FeathersActionButton,
            CurvesAction::OpenInput(module, input),
            BackgroundColor::default(),
        ))
        .id();
    control_input::set_phase(app.world_mut(), button, Phase::None);
    app.world_mut().trigger(Activate { entity: button });
    app.world_mut().flush();
    app.world_mut().run_schedule(Update);
    assert!(
        app.world()
            .resource::<CurvesState>()
            .selected_key()
            .is_none()
    );
    assert!(
        app.world()
            .get::<PendingFeathersActivation>(button)
            .is_some()
    );
    assert!(control_input::is_pressed(app.world(), button));
    app.world_mut().resource_mut::<Enabled>().0 = true;
    app.world_mut().run_schedule(Update);
    assert_eq!(
        app.world()
            .resource::<CurvesState>()
            .selected_key()
            .unwrap()
            .module,
        module
    );
    assert!(
        app.world()
            .get::<PendingFeathersActivation>(button)
            .is_none()
    );
    assert!(!control_input::is_pressed(app.world(), button));
    let revision = app.world().resource::<EditorSession>().ui_revision;
    for _ in 0..3 {
        app.world_mut().run_schedule(Update);
    }
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision
    );
    assert_eq!(app.world().resource::<EditorSession>().effect, before);
    assert!(!app.world().resource::<EditorSession>().dirty);
}
