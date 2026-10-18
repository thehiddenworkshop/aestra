//! Real viewport action consumers, temporary settings persistence and unchanged history.
use super::super::{ViewportSet, navigation};
use super::*;
use crate::{
    docking::MaximizedPanel,
    menus::{MenuKind, TabContextMenu},
    test_support,
};
use bevy::{gizmos::transform_gizmo::TransformGizmoMode, ui::RelativeCursorPosition};

#[derive(Resource, Default)]
struct Delivered(Vec<ViewportAction>);

struct Fixture {
    app: App,
    root: tempfile::TempDir,
    before: aestra_core::EffectAsset,
    revision: u64,
    undo: usize,
    redo: usize,
}
impl Fixture {
    fn new(locale: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let session = test_support::session_with_timing_slack();
        let before = session.effect.clone();
        let revision = session.document_revision();
        let undo = session.effect_undo_len();
        let redo = session.effect_redo_len();
        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<MenuState>()
            .init_resource::<EditorSettings>()
            .insert_resource(SettingsPersistence::for_test(
                root.path().join("settings.ron"),
            ))
            .insert_resource(Localizer::new(locale).unwrap())
            .init_resource::<PreviewCameraController>()
            .init_resource::<PreviewDisplayState>()
            .init_resource::<TransformGizmoSettings>()
            .init_resource::<MaximizedPanel>()
            .init_resource::<Delivered>()
            .add_observer(queue_viewport_action_activation)
            .add_observer(execute_viewport_action)
            .add_observer(
                |event: On<ViewportAction>, mut delivered: ResMut<Delivered>| {
                    delivered.0.push(*event);
                },
            )
            .add_systems(
                Update,
                (handle_viewport_action_buttons, audit_viewport_controls)
                    .chain()
                    .in_set(ViewportSet::Actions),
            );
        Self {
            app,
            root,
            before,
            revision,
            undo,
            redo,
        }
    }
    fn button(&mut self, action: ViewportAction) -> Entity {
        let button = self
            .app
            .world_mut()
            .spawn((control_input::AuditedButton, FeathersActionButton, action))
            .id();
        control_input::set_pressed(self.app.world_mut(), button, false);
        button
    }
    fn activate(&mut self, button: Entity) {
        self.app.world_mut().trigger(Activate { entity: button });
    }
    fn unchanged_document(&self) {
        let session = self.app.world().resource::<EditorSession>();
        assert_eq!(session.effect, self.before);
        assert_eq!(session.document_revision(), self.revision);
        assert_eq!(session.effect_undo_len(), self.undo);
        assert_eq!(session.effect_redo_len(), self.redo);
        assert!(!session.dirty);
    }
    fn path(&self) -> std::path::PathBuf {
        self.root.path().join("settings.ron")
    }
}

#[test]
fn grid_action_persists_the_complete_settings_and_localized_success_in_both_locales() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::new(locale);
        f.app
            .world_mut()
            .resource_mut::<EditorSettings>()
            .general
            .autosave_interval_seconds = 59;
        let original = f.app.world().resource::<EditorSettings>().clone();
        for show in [false, true] {
            f.app.world_mut().trigger(ViewportAction::ToggleGrid);
            f.app.update();
            let mut expected = original.clone();
            expected.preview.show_grid = show;
            assert_eq!(*f.app.world().resource::<EditorSettings>(), expected);
            assert_eq!(f.app.world().resource::<MenuState>().show_grid, show);
            let saved: EditorSettings =
                ron::from_str(&std::fs::read_to_string(f.path()).unwrap()).unwrap();
            assert_eq!(saved, expected.normalized());
            assert_eq!(
                f.app.world().resource::<EditorSession>().status,
                f.app
                    .world()
                    .resource::<Localizer>()
                    .text("persistence-status-settings-saved")
            );
            f.unchanged_document();
        }
    }
}

#[test]
fn framing_gizmo_and_display_actions_are_transient_without_settings_writes_or_history() {
    let mut f = Fixture::new("en-US");
    f.app.world_mut().trigger(ViewportAction::FramePreview);
    assert!(
        f.app
            .world()
            .resource::<PreviewCameraController>()
            .frame_requested
    );
    for mode in [
        TransformGizmoMode::Translate,
        TransformGizmoMode::Rotate,
        TransformGizmoMode::Scale,
    ] {
        f.app
            .world_mut()
            .trigger(ViewportAction::SetTransformGizmoMode(mode));
        assert_eq!(
            f.app.world().resource::<TransformGizmoSettings>().mode,
            mode
        );
    }
    for mode in [PreviewDisplayMode::Wireframe, PreviewDisplayMode::Rendered] {
        f.app
            .world_mut()
            .trigger(ViewportAction::SetPreviewDisplayMode(mode));
        assert_eq!(f.app.world().resource::<PreviewDisplayState>().mode, mode);
    }
    f.app.update();
    assert!(!f.path().exists());
    f.unchanged_document();
}

#[test]
fn maximize_replaces_another_panel_then_restores_without_persisting_layout() {
    let mut f = Fixture::new("en-US");
    f.app.world_mut().resource_mut::<MaximizedPanel>().0 = Some(ToolPanel::Timeline);
    let revision = f.app.world().resource::<EditorSession>().ui_revision;
    for (i, expected) in [Some(ToolPanel::Viewport), None].into_iter().enumerate() {
        f.app.world_mut().trigger(ViewportAction::ToggleMaximize);
        f.app.update();
        assert_eq!(f.app.world().resource::<MaximizedPanel>().0, expected);
        assert_eq!(
            f.app.world().resource::<EditorSession>().ui_revision,
            revision + i as u64 + 1
        );
    }
    assert!(!f.path().exists());
    f.unchanged_document();
}

#[test]
fn classified_activation_closes_menus_consumes_once_and_keeps_idle_state_clean() {
    let mut f = Fixture::new("en-US");
    let button = f.button(ViewportAction::ToggleMaximize);
    {
        let mut menu = f.app.world_mut().resource_mut::<MenuState>();
        menu.open = Some(MenuKind::View);
        menu.panels_open = true;
        menu.tab_context = Some(TabContextMenu {
            panel: ToolPanel::Timeline,
            position: [12.0, 24.0],
        });
    }
    let revision = f.app.world().resource::<EditorSession>().ui_revision;
    f.activate(button);
    f.app.update();
    let menu = f.app.world().resource::<MenuState>();
    assert!(menu.open.is_none() && !menu.panels_open && menu.tab_context.is_none());
    assert_eq!(
        f.app.world().resource::<EditorSession>().ui_revision,
        revision + 2
    );
    assert!(!control_input::is_pressed(f.app.world(), button));
    assert!(
        f.app
            .world()
            .get::<PendingFeathersActivation>(button)
            .is_none()
    );
    for _ in 0..3 {
        f.app.update();
    }
    assert_eq!(
        f.app.world().resource::<Delivered>().0,
        [ViewportAction::ToggleMaximize]
    );
    assert_eq!(
        f.app.world().resource::<EditorSession>().ui_revision,
        revision + 2
    );
    f.unchanged_document();
}

#[test]
fn activation_observer_is_scoped_to_classified_viewport_actions() {
    let mut f = Fixture::new("en-US");
    let other = f
        .app
        .world_mut()
        .spawn((FeathersActionButton, crate::curves::CurvesAction::AddKey))
        .id();
    let unclassified = f.app.world_mut().spawn(ViewportAction::FramePreview).id();
    for entity in [other, unclassified] {
        f.activate(entity);
        assert!(
            f.app
                .world()
                .get::<PendingFeathersActivation>(entity)
                .is_none()
        );
        assert!(!control_input::is_pressed(f.app.world(), entity));
    }
    f.app.update();
    assert!(f.app.world().resource::<Delivered>().0.is_empty());
    f.unchanged_document();
}

#[test]
fn pressed_without_activation_neither_dispatches_nor_closes_menus() {
    let mut f = Fixture::new("en-US");
    let button = f.button(ViewportAction::ToggleMaximize);
    f.app.world_mut().resource_mut::<MenuState>().open = Some(MenuKind::File);
    control_input::set_pressed(f.app.world_mut(), button, true);
    f.app.update();
    assert!(f.app.world().resource::<Delivered>().0.is_empty());
    assert_eq!(
        f.app.world().resource::<MenuState>().open,
        Some(MenuKind::File)
    );
    assert_eq!(f.app.world().resource::<MaximizedPanel>().0, None);
    f.unchanged_document();
}

#[test]
fn pending_without_pressed_preserves_the_original_gate_until_a_new_press() {
    let mut f = Fixture::new("en-US");
    let button = f.button(ViewportAction::ToggleMaximize);
    f.activate(button);
    f.app.world_mut().flush();
    control_input::set_pressed(f.app.world_mut(), button, false);
    f.app.update();
    assert!(f.app.world().resource::<Delivered>().0.is_empty());
    assert!(
        f.app
            .world()
            .get::<PendingFeathersActivation>(button)
            .is_some()
    );
    control_input::set_pressed(f.app.world_mut(), button, true);
    f.app.update();
    assert_eq!(
        f.app.world().resource::<Delivered>().0,
        [ViewportAction::ToggleMaximize]
    );
    assert!(
        f.app
            .world()
            .get::<PendingFeathersActivation>(button)
            .is_none()
    );
}

#[test]
fn repeated_activations_before_consumption_coalesce_then_a_new_activation_dispatches() {
    let mut f = Fixture::new("en-US");
    let button = f.button(ViewportAction::ToggleMaximize);
    f.activate(button);
    f.activate(button);
    f.app.update();
    assert_eq!(f.app.world().resource::<Delivered>().0.len(), 1);
    assert_eq!(
        f.app.world().resource::<MaximizedPanel>().0,
        Some(ToolPanel::Viewport)
    );
    f.activate(button);
    f.app.update();
    assert_eq!(f.app.world().resource::<Delivered>().0.len(), 2);
    assert_eq!(f.app.world().resource::<MaximizedPanel>().0, None);
    f.unchanged_document();
}

#[test]
fn independent_buttons_keep_payloads_and_do_not_restyle_the_feathers_surface() {
    let mut f = Fixture::new("en-US");
    let actions = [
        ViewportAction::SetTransformGizmoMode(TransformGizmoMode::Scale),
        ViewportAction::SetPreviewDisplayMode(PreviewDisplayMode::Wireframe),
    ];
    for action in actions {
        let button = f.button(action);
        let background = BackgroundColor(Color::srgb(0.12, 0.34, 0.56));
        f.app.world_mut().entity_mut(button).insert(background);
        f.activate(button);
        f.app.update();
        assert_eq!(
            *f.app.world().get::<BackgroundColor>(button).unwrap(),
            background
        );
    }
    assert_eq!(f.app.world().resource::<Delivered>().0, actions);
    assert_eq!(
        f.app.world().resource::<TransformGizmoSettings>().mode,
        TransformGizmoMode::Scale
    );
    assert_eq!(
        f.app.world().resource::<PreviewDisplayState>().mode,
        PreviewDisplayMode::Wireframe
    );
    f.unchanged_document();
}

#[test]
fn failed_settings_write_reports_localized_failure_without_changing_document_or_prior_bytes() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::new(locale);
        let blocker = f.root.path().join("not-a-directory");
        std::fs::write(&blocker, b"preserve me").unwrap();
        f.app
            .insert_resource(SettingsPersistence::for_test(blocker.join("settings.ron")));
        f.app.world_mut().trigger(ViewportAction::ToggleGrid);
        f.app.update();
        let localizer = f.app.world().resource::<Localizer>();
        let mut args = fluent_bundle::FluentArgs::new();
        args.set("error", "ERROR_TOKEN");
        let template = localizer.text_with("persistence-status-settings-save-failed", &args);
        let prefix = template.split("ERROR_TOKEN").next().unwrap();
        assert!(
            f.app
                .world()
                .resource::<EditorSession>()
                .status
                .starts_with(prefix)
        );
        assert!(!f.app.world().resource::<EditorSettings>().preview.show_grid);
        assert_eq!(std::fs::read(&blocker).unwrap(), b"preserve me");
        assert!(!f.path().exists());
        f.unchanged_document();
    }
}

#[test]
fn malformed_settings_remain_write_protected_when_grid_changes() {
    let mut f = Fixture::new("en-US");
    let malformed = b"not valid settings";
    std::fs::write(f.path(), malformed).unwrap();
    let (settings, persistence) = SettingsPersistence::load_for_test(f.path());
    assert!(persistence.diagnostic().unwrap().contains("malformed"));
    f.app.insert_resource(settings).insert_resource(persistence);
    f.app.world_mut().trigger(ViewportAction::ToggleGrid);
    f.app.update();
    assert_eq!(std::fs::read(f.path()).unwrap(), malformed);
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .contains("reset explicitly")
    );
    assert!(
        f.app
            .world()
            .resource::<SettingsPersistence>()
            .diagnostic()
            .unwrap()
            .contains("malformed")
    );
    f.unchanged_document();
}

#[test]
fn action_and_navigation_consumers_frame_the_actual_proxy_without_semantic_edits() {
    let mut f = Fixture::new("en-US");
    f.app
        .init_resource::<navigation::PreviewNavigationState>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<bevy::input::mouse::MouseMotion>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_systems(
            Update,
            navigation::navigate_preview_camera.after(ViewportSet::Actions),
        );
    f.app.world_mut().spawn((
        super::super::PreviewCanvas,
        RelativeCursorPosition::default(),
    ));
    let camera = f
        .app
        .world_mut()
        .spawn((navigation::PreviewRenderCamera, Transform::default()))
        .id();
    let position = Vec3::new(12.0, 38.0, -9.0);
    f.app.world_mut().spawn((
        navigation::EmitterTransformGizmoProxy,
        GlobalTransform::from_translation(position),
    ));
    let button = f.button(ViewportAction::FramePreview);
    f.activate(button);
    f.app.update();
    assert_eq!(
        *f.app.world().get::<Transform>(camera).unwrap(),
        navigation::preview_camera_transform(
            position,
            140.0,
            0.0,
            navigation::DEFAULT_PREVIEW_PITCH
        )
    );
    assert!(
        !f.app
            .world()
            .resource::<PreviewCameraController>()
            .frame_requested
    );
    f.unchanged_document();
}

fn keyboard(f: &mut Fixture) -> Entity {
    use bevy::{
        input::InputPlugin,
        input_focus::{InputDispatchPlugin, InputFocusPlugin},
    };
    f.app
        .add_plugins((
            MinimalPlugins,
            InputPlugin,
            InputFocusPlugin,
            InputDispatchPlugin,
            crate::input::EditorKeyboardPlugin,
        ))
        .init_resource::<crate::ModulePaletteState>();
    super::super::keyboard::register(&mut f.app);
    f.app.world_mut().spawn((
        super::super::PreviewCanvas,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::splat(0.5)),
        },
    ));
    f.app
        .world_mut()
        .spawn((Window::default(), bevy::window::PrimaryWindow))
        .id()
}
fn tap(f: &mut Fixture, window: Entity, code: KeyCode, logical: bevy::input::keyboard::Key) {
    for state in [
        bevy::input::ButtonState::Pressed,
        bevy::input::ButtonState::Released,
    ] {
        f.app
            .world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
                key_code: code,
                logical_key: logical.clone(),
                state,
                repeat: false,
                text: None,
                window,
            });
    }
}

#[test]
fn actual_keyboard_dispatch_reaches_grid_maximize_frame_and_gizmo_consumers() {
    use bevy::input::keyboard::Key;
    let mut f = Fixture::new("en-US");
    let window = keyboard(&mut f);
    f.app.update();
    tap(&mut f, window, KeyCode::KeyG, Key::Character("g".into()));
    tap(&mut f, window, KeyCode::F11, Key::F11);
    tap(&mut f, window, KeyCode::Home, Key::Home);
    tap(&mut f, window, KeyCode::Digit2, Key::Character("é".into()));
    f.app.update();
    assert!(!f.app.world().resource::<EditorSettings>().preview.show_grid);
    assert!(f.path().exists());
    assert_eq!(
        f.app.world().resource::<MaximizedPanel>().0,
        Some(ToolPanel::Viewport)
    );
    assert!(
        f.app
            .world()
            .resource::<PreviewCameraController>()
            .frame_requested
    );
    assert_eq!(
        f.app.world().resource::<TransformGizmoSettings>().mode,
        TransformGizmoMode::Rotate
    );
    assert_eq!(f.app.world().resource::<Delivered>().0.len(), 4);
    f.app.update();
    assert_eq!(f.app.world().resource::<Delivered>().0.len(), 4);
    f.unchanged_document();
}

#[test]
fn focused_text_keeps_real_keyboard_actions_from_reaching_persistence() {
    let mut f = Fixture::new("en-US");
    let window = keyboard(&mut f);
    f.app.update();
    let editor = f
        .app
        .world_mut()
        .spawn(bevy::text::EditableText::new("draft"))
        .id();
    f.app
        .world_mut()
        .insert_resource(bevy::input_focus::InputFocus::from_entity(editor));
    tap(
        &mut f,
        window,
        KeyCode::KeyG,
        bevy::input::keyboard::Key::Character("g".into()),
    );
    f.app.update();
    assert!(f.app.world().resource::<Delivered>().0.is_empty());
    assert!(!f.path().exists());
    f.unchanged_document();
}

#[derive(Resource)]
struct Enabled(bool);
#[test]
fn action_set_gate_defers_activation_and_delivers_it_once_after_reenable() {
    let mut f = Fixture::new("en-US");
    f.app.insert_resource(Enabled(false)).configure_sets(
        Update,
        ViewportSet::Actions.run_if(|gate: Res<Enabled>| gate.0),
    );
    let button = f.button(ViewportAction::ToggleMaximize);
    f.activate(button);
    f.app.update();
    assert!(
        f.app
            .world()
            .get::<PendingFeathersActivation>(button)
            .is_some()
    );
    assert!(f.app.world().resource::<Delivered>().0.is_empty());
    f.app.world_mut().resource_mut::<Enabled>().0 = true;
    f.app.update();
    f.app.update();
    assert_eq!(
        f.app.world().resource::<Delivered>().0,
        [ViewportAction::ToggleMaximize]
    );
    f.unchanged_document();
}

#[test]
fn classified_and_explicit_native_controls_pass_the_actual_audit() {
    let mut f = Fixture::new("en-US");
    f.button(ViewportAction::FramePreview);
    f.app.world_mut().spawn((
        control_input::AuditedButton,
        EditorNativeControl,
        ViewportAction::FramePreview,
    ));
    f.app.update();
    assert!(f.app.world().resource::<Delivered>().0.is_empty());
    f.unchanged_document();
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "must use FeathersActionButton")]
fn unclassified_native_viewport_button_is_rejected_by_the_actual_audit() {
    let mut f = Fixture::new("en-US");
    f.app
        .world_mut()
        .spawn((control_input::AuditedButton, ViewportAction::FramePreview));
    f.app.update();
}
