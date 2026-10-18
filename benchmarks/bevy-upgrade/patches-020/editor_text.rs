//! Real custom code/search compositions and context menus, not replacement widgets.
#[path = "../../../apps/aestra-editor/src/feathers/action_activation.rs"]
pub(crate) mod action_activation;
#[path = "../../../apps/aestra-editor/src/theme/bevy_020.rs"]
pub mod boundary;
#[path = "../../../apps/aestra-editor/src/history/action.rs"]
mod history_action;
#[path = "../../../apps/aestra-editor/src/theme/palette.rs"]
pub mod palette;
mod theme {
    pub(crate) use super::boundary::feathers_theme;
    pub(crate) use super::palette::*;
}
#[path = "../../../apps/aestra-editor/src/feathers/button.rs"]
#[allow(
    dead_code,
    reason = "Application action dispatch is outside this widget fixture"
)]
mod button;
#[path = "../../../apps/aestra-editor/src/feathers/button/activation_020.rs"]
mod button_activation;

#[test]
fn shared_tool_button_reexport_keeps_real_scene_and_action_classification() {
    use bevy::{asset::AssetPlugin, prelude::*, scene::ScenePlugin, text::TextPlugin};
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        ScenePlugin,
        TextPlugin,
    ))
    .add_systems(Startup, |mut commands: Commands| {
        commands.spawn(Node::default()).with_children(|parent| {
            button::spawn_tool_button(parent, "Disclosure", EditorAction);
        });
    });
    app.update();
    let world = app.world_mut();
    let (entity, node, label) = world
        .query_filtered::<(Entity, &Node, &AccessibleLabel), With<EditorAction>>()
        .single(world)
        .unwrap();
    assert_eq!(node.width, Val::Px(28.0));
    assert_eq!(node.height, Val::Px(24.0));
    assert_eq!(label.0, "Disclosure");
    assert!(world.get::<bevy::ui_widgets::Button>(entity).is_some());
    assert!(world.get::<button::FeathersActionButton>(entity).is_some());
    assert!(world.get::<button::EditorNativeControl>(entity).is_none());
    let text = world.get::<Children>(entity).unwrap()[0];
    assert_eq!(world.get::<Text>(text).unwrap().0, "Disclosure");
    assert!(
        world
            .get::<bevy::feathers::theme::ThemedText>(text)
            .is_some()
    );
}
// Keep the native button module's public reexport exercised as well as the direct
// production constructor used by the editor graph fixture.
#[test]
fn shared_action_button_reexport_preserves_real_primary_and_secondary_scenes() {
    use bevy::{
        asset::AssetPlugin, feathers::controls::ButtonVariant, prelude::*, scene::ScenePlugin,
        text::TextPlugin,
    };
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        ScenePlugin,
        TextPlugin,
    ))
    .add_systems(Startup, |mut commands: Commands| {
        commands.spawn(Node::default()).with_children(|parent| {
            button::spawn_action_button(parent, "Primary", EditorAction, true);
            button::spawn_action_button(parent, "Secondary", EditorAction, false);
        });
    });
    app.update();
    let w = app.world_mut();
    let mut query =
        w.query_filtered::<(Entity, &AccessibleLabel, &ButtonVariant), With<EditorAction>>();
    assert_eq!(query.iter(w).count(), 2);
    for (e, label, variant) in query.iter(w) {
        assert!(w.get::<button::FeathersActionButton>(e).is_some());
        assert_eq!(
            *variant,
            if label.0 == "Primary" {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Normal
            }
        );
        let child = w.get::<Children>(e).unwrap()[0];
        assert_eq!(w.get::<Text>(child).unwrap().0, label.0);
        assert!(w.get::<bevy::feathers::theme::ThemedText>(child).is_some());
    }
}
#[path = "../../../apps/aestra-editor/src/feathers/code_editor.rs"]
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Same system-parameter lint policy as the shipping editor"
)]
mod code_editor;
#[path = "../../../apps/aestra-editor/src/feathers/context_menu.rs"]
#[allow(dead_code, reason = "Other panels use the remaining menu helpers")]
mod context_menu;
#[path = "../../../apps/aestra-editor/src/feathers/pointer_020.rs"]
mod pointer;
#[path = "../../../apps/aestra-editor/src/feathers/scenes/common.rs"]
mod scenes;
#[path = "../../../apps/aestra-editor/src/feathers/search_field.rs"]
mod search_field;
#[path = "../../../apps/aestra-editor/src/feathers/text_input.rs"]
mod text_input;
mod node_graph {
    // Host-only classification marker; graph behavior is not substituted here.
    #[derive(bevy::prelude::Component)]
    pub(crate) struct FeathersGraphNavigationBlocker;
}
mod feathers {
    pub(crate) use super::{action_activation, context_menu, text_input};
    pub(crate) use super::{node_graph, pointer, scenes};
}
// Host-only action/history identities. All widget behavior above is production source.
#[derive(bevy::prelude::Component)]
pub(crate) struct EditorAction;

#[test]
fn candidate_action_bridge_uses_native_pressed_marker_and_ignores_non_actions() {
    use bevy::{prelude::*, ui::Pressed, ui_widgets::Activate};
    let mut app = App::new();
    app.add_observer(button::queue_action_activation);
    let action = app
        .world_mut()
        .spawn((EditorAction, button::FeathersActionButton))
        .id();
    let unrelated = app.world_mut().spawn(button::FeathersActionButton).id();
    app.world_mut().trigger(Activate { entity: unrelated });
    app.world_mut().trigger(Activate { entity: action });
    app.world_mut().flush();
    assert!(
        app.world()
            .get::<button::PendingFeathersActivation>(action)
            .is_some()
    );
    assert!(app.world().get::<Pressed>(action).is_some());
    assert!(
        app.world()
            .get::<button::PendingFeathersActivation>(unrelated)
            .is_none()
    );
    assert!(app.world().get::<Pressed>(unrelated).is_none());
}
mod history {
    pub(crate) use crate::history_action::{HistoryAction, queue_history_action_activation};
    #[derive(bevy::prelude::Component)]
    pub(crate) enum HistoryScope {
        Effect,
    }
}
