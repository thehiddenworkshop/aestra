//! Real graph toolbar composition, frame routing and snapping controls together.
use super::*;
use crate::feathers::{
    icon::SvgPlugin,
    node_graph::drag_assist::{State, Toggle, spawn_controls, sync_controls, toggle},
    text_input::tests::app_with,
};
use bevy::{feathers::controls::ButtonVariant, ui::Selected};

fn app() -> App {
    app_with(|a| {
        a.add_plugins(SvgPlugin)
            .init_resource::<State>()
            .insert_resource(crate::Localizer::new("en-US").unwrap())
            .add_observer(queue_graph_frame_activation)
            .add_observer(toggle)
            .add_systems(Update, (handle_graph_frame_buttons, sync_controls).chain());
    })
}
fn frame(app: &mut App, key: &str, target: GraphFrameTarget) -> Entity {
    let assets = app.world().resource::<AssetServer>().clone();
    let mut button = Entity::PLACEHOLDER;
    app.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|p| {
            button = spawn_graph_frame_button(
                p,
                &assets,
                "icons/frame-all.svg",
                "Frame".into(),
                GraphFrameAction::new(key, target),
            );
        });
    app.world_mut().flush();
    button
}
#[test]
fn composed_frame_button_requires_activate_and_targets_only_matching_views() {
    let mut a = app();
    let button = frame(&mut a, "first", GraphFrameTarget::Selection);
    let matching = a
        .world_mut()
        .spawn(FeathersGraphViewport::clipboard_fixture(
            "first".into(),
            Vec2::ZERO,
            1.0,
        ))
        .id();
    let foreign = a
        .world_mut()
        .spawn(FeathersGraphViewport::clipboard_fixture(
            "other".into(),
            Vec2::ZERO,
            1.0,
        ))
        .id();
    toolbar_input::set_pressed(a.world_mut(), button);
    a.update();
    assert_eq!(
        a.world()
            .get::<FeathersGraphViewport>(matching)
            .unwrap()
            .frame_request_for_test(),
        None
    );
    a.world_mut().trigger(Activate { entity: button });
    a.update();
    assert!(a.world().get::<PendingFeathersActivation>(button).is_none());
    assert_eq!(
        a.world()
            .get::<FeathersGraphViewport>(matching)
            .unwrap()
            .frame_request_for_test(),
        Some(GraphFrameTarget::Selection)
    );
    assert_eq!(
        a.world()
            .get::<FeathersGraphViewport>(foreign)
            .unwrap()
            .frame_request_for_test(),
        None
    );
}
#[test]
fn missing_view_still_consumes_frame_activation() {
    let mut a = app();
    let b = frame(&mut a, "missing", GraphFrameTarget::All);
    a.world_mut().trigger(Activate { entity: b });
    a.update();
    assert!(a.world().get::<PendingFeathersActivation>(b).is_none());
}
#[test]
fn unclassified_frame_action_cannot_be_queued() {
    let mut a = app();
    let b = a
        .world_mut()
        .spawn(GraphFrameAction::new("first", GraphFrameTarget::All))
        .id();
    a.world_mut().trigger(Activate { entity: b });
    a.world_mut().flush();
    assert!(a.world().get::<PendingFeathersActivation>(b).is_none());
}
#[test]
fn real_snap_buttons_keep_default_selection_and_toggle_independently() {
    let mut a = app();
    let assets = a.world().resource::<AssetServer>().clone();
    a.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|p| spawn_controls(p, &assets));
    a.world_mut().flush();
    a.update();
    let buttons = a
        .world_mut()
        .query::<(Entity, &Toggle)>()
        .iter(a.world())
        .map(|(e, t)| (e, *t))
        .collect::<Vec<_>>();
    assert_eq!(buttons.len(), 2);
    let grid = buttons
        .iter()
        .find(|(_, t)| matches!(t, Toggle::Grid))
        .unwrap()
        .0;
    let alignment = buttons
        .iter()
        .find(|(_, t)| matches!(t, Toggle::Alignment))
        .unwrap()
        .0;
    assert!(a.world().get::<Selected>(grid).is_none());
    assert!(a.world().get::<Selected>(alignment).is_some());
    a.world_mut().trigger(Activate { entity: grid });
    a.update();
    assert!(a.world().get::<Selected>(grid).is_some());
    assert!(a.world().get::<Selected>(alignment).is_some());
    assert!(matches!(
        a.world().get::<ButtonVariant>(grid),
        Some(ButtonVariant::Primary)
    ));
    a.world_mut().trigger(Activate { entity: alignment });
    a.update();
    assert!(a.world().get::<Selected>(alignment).is_none());
    assert!(a.world().get::<Selected>(grid).is_some());
    a.world_mut().trigger(Activate { entity: grid });
    a.update();
    assert!(a.world().get::<Selected>(grid).is_none());
}
#[test]
fn snap_labels_update_for_locale_without_rewriting_idle_tooltips() {
    let mut a = app();
    let assets = a.world().resource::<AssetServer>().clone();
    a.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|p| spawn_controls(p, &assets));
    a.world_mut().flush();
    a.update();
    let buttons = a
        .world_mut()
        .query::<(Entity, &Toggle)>()
        .iter(a.world())
        .map(|(e, t)| (e, *t))
        .collect::<Vec<_>>();
    a.insert_resource(crate::Localizer::new("fr-FR").unwrap());
    a.update();
    for (e, t) in buttons {
        let key = if matches!(t, Toggle::Grid) {
            "graph-snap-grid"
        } else {
            "graph-snap-alignment"
        };
        assert_eq!(
            a.world().get::<AccessibleLabel>(e).unwrap().0,
            a.world().resource::<crate::Localizer>().text(key)
        );
        let tick = a
            .world()
            .entity(e)
            .get_ref::<EditorTooltip>()
            .unwrap()
            .last_changed();
        a.update();
        assert_eq!(
            a.world()
                .entity(e)
                .get_ref::<EditorTooltip>()
                .unwrap()
                .last_changed(),
            tick
        );
    }
}
#[test]
fn summary_helper_retains_passive_text_style() {
    let mut a = app();
    a.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|p| spawn_graph_toolbar_summary(p, "Summary".into()));
    a.world_mut().flush();
    let (e, t) = a
        .world_mut()
        .query::<(Entity, &Text)>()
        .iter(a.world())
        .find(|(_, t)| t.0 == "Summary")
        .unwrap();
    assert_eq!(t.0, "Summary");
    assert_eq!(a.world().get::<TextColor>(e).unwrap().0, theme::TEXT_FAINT);
}
