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
