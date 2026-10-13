//! Production toolbar/plugin/Feathers composition, not substitute controls.
use super::*;
use crate::{
    feathers::{
        action_activation::PendingFeathersActivation,
        icon::{SvgFile, SvgPlugin},
    },
    input::EditorKeyboardPlugin,
    menus::{MenuKind, TabContextMenu},
    transport_input::{AuditedButton, clear_hover, is_pressed, set_phase},
};
use bevy::{
    ecs::system::RunSystemOnce,
    input::{InputPlugin, keyboard::Key},
    input_focus::{InputDispatchPlugin, InputFocus, InputFocusPlugin},
    time::TimeUpdateStrategy,
    ui::{InteractionDisabled, Selected},
};
use std::time::{Duration, Instant};

#[derive(Resource, Default)]
struct Events(Vec<TransportAction>);

fn record(event: On<TransportAction>, mut events: ResMut<Events>) {
    events.0.push(*event);
}

struct Fixture {
    app: App,
}
impl Fixture {
    fn new() -> Self {
        let mut app = App::new();
        app.insert_resource(crate::test_support::session_with_playback_mode(
            EffectPlaybackMode::LoopRestart,
        ))
        .insert_resource(Localizer::new("en-US").unwrap())
        .init_resource::<MenuState>()
        .init_resource::<crate::ModulePaletteState>()
        .init_resource::<Time>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Events>()
        .add_message::<bevy::input::keyboard::KeyboardInput>()
        .configure_sets(
            Update,
            (
                TransportSet::Input,
                TransportSet::Actions,
                TransportSet::Playback,
                TransportSet::Sync,
            )
                .chain(),
        )
        .add_plugins(EditorTransportPlugin)
        .add_observer(record);
        Self { app }
    }
    fn control(&mut self, feathers: bool, disabled: bool) -> Entity {
        let mut entity = self.app.world_mut().spawn((
            AuditedButton,
            TransportAction::Stop,
            BackgroundColor(theme::BUTTON),
        ));
        if feathers {
            entity.insert(FeathersActionButton);
        } else {
            entity.insert(EditorNativeControl);
        }
        if disabled {
            entity.insert(InteractionDisabled);
        }
        let entity = entity.id();
        set_phase(self.app.world_mut(), entity, Phase::None);
        self.app.update();
        entity
    }
    fn events(&self) -> &[TransportAction] {
        &self.app.world().resource::<Events>().0
    }
}

#[test]
fn feathers_activate_is_consumed_once_and_only_closes_transient_menus() {
    for kind in [
        MenuKind::File,
        MenuKind::Edit,
        MenuKind::View,
        MenuKind::Help,
    ] {
        let mut fixture = Fixture::new();
        let control = fixture.control(true, false);
        fixture.app.insert_resource(MenuState {
            open: Some(kind),
            panels_open: true,
            tab_context: Some(TabContextMenu {
                panel: crate::docking::ToolPanel::Timeline,
                position: [12.0, 24.0],
            }),
            show_grid: false,
            show_about: true,
        });
        let revision = fixture.app.world().resource::<EditorSession>().ui_revision;
        fixture
            .app
            .world_mut()
            .trigger(Activate { entity: control });
        fixture.app.update();
        assert_eq!(fixture.events(), &[TransportAction::Stop]);
        assert!(
            !fixture
                .app
                .world()
                .entity(control)
                .contains::<PendingFeathersActivation>()
        );
        assert!(!is_pressed(fixture.app.world(), control));
        let menu = fixture.app.world().resource::<MenuState>();
        assert!(menu.open.is_none() && !menu.panels_open && menu.tab_context.is_none());
        assert!(!menu.show_grid && menu.show_about);
        assert_eq!(
            fixture.app.world().resource::<EditorSession>().ui_revision,
            revision + 1
        );
        assert_eq!(
            fixture
                .app
                .world()
                .get::<BackgroundColor>(control)
                .unwrap()
                .0,
            theme::BUTTON
        );
        fixture.app.update();
        assert_eq!(fixture.events().len(), 1);
    }
}

#[test]
fn activation_without_tab_context_does_not_rebuild_panels() {
    let mut fixture = Fixture::new();
    let control = fixture.control(true, false);
    let revision = fixture.app.world().resource::<EditorSession>().ui_revision;
    fixture
        .app
        .world_mut()
        .trigger(Activate { entity: control });
    fixture.app.update();
    assert_eq!(
        fixture.app.world().resource::<EditorSession>().ui_revision,
        revision
    );
}

#[test]
fn pressed_without_activate_cannot_execute_a_feathers_transport_action() {
    let mut fixture = Fixture::new();
    let control = fixture.control(true, false);
    set_phase(fixture.app.world_mut(), control, Phase::Pressed);
    fixture.app.update();
    assert!(fixture.events().is_empty());
    assert!(fixture.app.world().resource::<EditorSession>().playing);
}

#[test]
fn disabled_activation_does_not_dispatch_close_menus_or_drop_its_pending_marker() {
    let mut fixture = Fixture::new();
    let control = fixture.control(true, true);
    fixture.app.world_mut().resource_mut::<MenuState>().open = Some(MenuKind::File);
    fixture
        .app
        .world_mut()
        .trigger(Activate { entity: control });
    fixture.app.update();
    assert!(fixture.events().is_empty());
    assert!(fixture.app.world().resource::<EditorSession>().playing);
    assert_eq!(
        fixture.app.world().resource::<MenuState>().open,
        Some(MenuKind::File)
    );
    assert!(
        fixture
            .app
            .world()
            .entity(control)
            .contains::<PendingFeathersActivation>()
    );
}

#[test]
fn unrelated_feathers_activation_is_not_queued_as_transport() {
    let mut fixture = Fixture::new();
    let unrelated = fixture.app.world_mut().spawn(FeathersActionButton).id();
    fixture
        .app
        .world_mut()
        .trigger(Activate { entity: unrelated });
    fixture.app.update();
    assert!(fixture.events().is_empty());
    assert!(
        !fixture
            .app
            .world()
            .entity(unrelated)
            .contains::<PendingFeathersActivation>()
    );
}

#[test]
fn an_accepted_activate_survives_visual_release_before_the_update_handler() {
    let mut fixture = Fixture::new();
    let control = fixture.control(true, false);
    fixture
        .app
        .world_mut()
        .trigger(Activate { entity: control });
    // Press/release is transient visual state, not authority to discard an accepted Activate.
    fixture.app.world_mut().flush();
    set_phase(fixture.app.world_mut(), control, Phase::None);
    fixture.app.update();
    assert_eq!(fixture.events(), &[TransportAction::Stop]);
    assert!(
        !fixture
            .app
            .world()
            .entity(control)
            .contains::<PendingFeathersActivation>()
    );
    fixture.app.update();
    assert_eq!(fixture.events().len(), 1);
}

#[test]
fn native_hover_press_release_and_disabled_colors_preserve_single_action_behavior() {
    let mut fixture = Fixture::new();
    let control = fixture.control(false, false);
    set_phase(fixture.app.world_mut(), control, Phase::Hovered);
    fixture.app.update();
    assert_eq!(
        fixture
            .app
            .world()
            .get::<BackgroundColor>(control)
            .unwrap()
            .0,
        theme::BUTTON_HOVER
    );
    set_phase(fixture.app.world_mut(), control, Phase::Pressed);
    fixture.app.update();
    assert_eq!(fixture.events(), &[TransportAction::Stop]);
    assert_eq!(
        fixture
            .app
            .world()
            .get::<BackgroundColor>(control)
            .unwrap()
            .0,
        theme::ACCENT_DIM
    );
    fixture.app.update();
    assert_eq!(fixture.events().len(), 1);
    set_phase(fixture.app.world_mut(), control, Phase::Hovered);
    fixture.app.update();
    clear_hover(fixture.app.world_mut(), control);
    fixture.app.update();
    assert_eq!(
        fixture
            .app
            .world()
            .get::<BackgroundColor>(control)
            .unwrap()
            .0,
        theme::BUTTON
    );
    fixture
        .app
        .world_mut()
        .entity_mut(control)
        .insert(InteractionDisabled);
    set_phase(fixture.app.world_mut(), control, Phase::Pressed);
    fixture.app.update();
    assert_eq!(fixture.events().len(), 1);
    assert_eq!(
        fixture
            .app
            .world()
            .get::<BackgroundColor>(control)
            .unwrap()
            .0,
        theme::PANEL_DARK
    );
}

#[test]
#[should_panic(expected = "must use FeathersActionButton")]
fn audit_rejects_unclassified_actual_native_buttons() {
    let mut fixture = Fixture::new();
    fixture
        .app
        .world_mut()
        .spawn((AuditedButton, TransportAction::Stop));
    fixture.app.update();
}

fn repository_assets() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|root| root.join("assets"))
        .find(|assets| assets.join("icons/stop.svg").is_file())
        .expect("repository icon assets")
}

struct Toolbar {
    app: App,
    root: Entity,
    window: Entity,
    camera: Entity,
}
impl Toolbar {
    fn new(mode: EffectPlaybackMode, locale: &str, scale: f32) -> Self {
        use bevy::{
            feathers::FeathersPlugins,
            render::{RenderPlugin, settings::WgpuSettings},
            ui_widgets::UiWidgetsPlugins,
            window::{ExitCondition, WindowPlugin, WindowResolution},
        };
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin {
                file_path: repository_assets().to_string_lossy().into_owned(),
                ..default()
            },
            bevy::scene::ScenePlugin,
            RenderPlugin {
                render_creation: WgpuSettings {
                    backends: None,
                    ..default()
                }
                .into(),
                ..default()
            },
            bevy::image::ImagePlugin::default(),
            bevy::mesh::MeshPlugin,
            bevy::camera::CameraPlugin,
            bevy::transform::TransformPlugin,
            bevy::text::TextPlugin,
            InputPlugin,
            InputFocusPlugin,
            InputDispatchPlugin,
            bevy::picking::DefaultPickingPlugins,
            UiWidgetsPlugins,
        ))
        .add_plugins((
            bevy::sprite::SpritePlugin,
            bevy::ui::UiPlugin,
            WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            },
            FeathersPlugins,
            SvgPlugin,
            EditorKeyboardPlugin,
            EditorTransportPlugin,
        ))
        .insert_resource(theme::feathers_theme())
        .insert_resource(crate::test_support::session_with_playback_mode(mode))
        .insert_resource(Localizer::new(locale).unwrap())
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
        .init_resource::<MenuState>()
        .init_resource::<crate::ModulePaletteState>()
        .init_resource::<Events>()
        .add_observer(record)
        .configure_sets(
            Update,
            (
                TransportSet::Input,
                TransportSet::Actions,
                TransportSet::Playback,
                TransportSet::Sync,
            )
                .chain(),
        );
        app.finish();
        app.cleanup();
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: WindowResolution::new(800, 500).with_scale_factor_override(scale),
                    ..default()
                },
                bevy::window::PrimaryWindow,
            ))
            .id();
        let camera = app
            .world_mut()
            .spawn((
                Camera2d,
                bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window)),
            ))
            .id();
        let root = Self::spawn(&mut app, camera);
        let mut toolbar = Self {
            app,
            root,
            window,
            camera,
        };
        toolbar.settle();
        toolbar
    }
    fn spawn(app: &mut App, camera: Entity) -> Entity {
        app.world_mut()
            .run_system_once(
                move |mut commands: Commands,
                      server: Res<AssetServer>,
                      session: Res<EditorSession>,
                      localizer: Res<Localizer>| {
                    let mut root = commands.spawn((
                        Node {
                            width: Val::Px(400.0),
                            height: Val::Px(100.0),
                            align_items: AlignItems::Start,
                            ..default()
                        },
                        UiTargetCamera(camera),
                    ));
                    root.with_children(|parent| {
                        spawn_transport_controls(parent, &session, &localizer, &server)
                    });
                    root.id()
                },
            )
            .unwrap()
    }
    fn settle(&mut self) {
        for _ in 0..4 {
            self.app.update();
        }
    }
    fn button(&mut self, action: TransportAction) -> Entity {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &TransportAction)>()
            .iter(world)
            .find_map(|(entity, candidate)| (*candidate == action).then_some(entity))
            .unwrap()
    }
    fn activate(&mut self, action: TransportAction) {
        let entity = self.button(action);
        self.app.world_mut().trigger(Activate { entity });
        self.settle();
    }
    fn visible(&mut self, play: bool) -> bool {
        let world = self.app.world_mut();
        if play {
            world
                .query_filtered::<&Node, With<PlaybackPlayIcon>>()
                .single(world)
                .unwrap()
                .display
                != Display::None
        } else {
            world
                .query_filtered::<&Node, With<PlaybackPauseIcon>>()
                .single(world)
                .unwrap()
                .display
                != Display::None
        }
    }
    fn wait_icons(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let world = self.app.world_mut();
            let icons: Vec<_> = world
                .query::<&UiSvg>()
                .iter(world)
                .map(|svg| svg.0.clone())
                .collect();
            assert_eq!(icons.len(), 5);
            if icons.iter().all(|handle| {
                world
                    .resource::<AssetServer>()
                    .is_loaded_with_dependencies(handle.id())
            }) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "actual toolbar SVG loading did not finish"
            );
            self.app.update();
            std::thread::yield_now();
        }
        self.settle();
    }
}

#[test]
fn actual_toolbar_has_four_localized_accessible_actions_and_exact_layout_at_two_dpis() {
    for (locale, scale) in [("en-US", 1.0), ("fr-FR", 2.0)] {
        let mut toolbar = Toolbar::new(EffectPlaybackMode::LoopRestart, locale, scale);
        toolbar.wait_icons();
        for (action, message) in [
            (TransportAction::TogglePlayback, "toolbar-play"),
            (TransportAction::Stop, "toolbar-stop"),
            (
                TransportAction::TogglePlaybackMode(EffectPlaybackMode::LoopRestart),
                "toolbar-loop-restart",
            ),
            (
                TransportAction::TogglePlaybackMode(EffectPlaybackMode::LoopContinuous),
                "toolbar-loop-continuous",
            ),
        ] {
            let entity = toolbar.button(action);
            let world = toolbar.app.world();
            let label = world.resource::<Localizer>().text(message);
            assert_eq!(world.get::<AccessibleLabel>(entity).unwrap().0, label);
            assert_eq!(
                world
                    .get::<EditorTooltip>(entity)
                    .unwrap()
                    .accessible_label(),
                label
            );
            assert!(world.entity(entity).contains::<FeathersActionButton>());
            let computed = world.get::<ComputedNode>(entity).unwrap();
            assert!((computed.size().x * computed.inverse_scale_factor() - 28.0).abs() < 0.1);
            assert!((computed.size().y * computed.inverse_scale_factor() - 28.0).abs() < 0.1);
            let parent = world.get::<ChildOf>(entity).unwrap().parent();
            let computed = world.get::<ComputedNode>(parent).unwrap();
            assert!((computed.size().y * computed.inverse_scale_factor() - 34.0).abs() < 0.1);
            assert_eq!(
                world.get::<BackgroundColor>(parent).unwrap().0,
                theme::PANEL_DARK
            );
        }
    }
}

#[test]
fn real_play_stop_activation_updates_icons_without_respawning_controls() {
    let mut toolbar = Toolbar::new(EffectPlaybackMode::LoopRestart, "en-US", 1.0);
    let play = toolbar.button(TransportAction::TogglePlayback);
    assert!(!toolbar.visible(true) && toolbar.visible(false));
    toolbar.activate(TransportAction::TogglePlayback);
    assert!(toolbar.visible(true) && !toolbar.visible(false));
    toolbar.activate(TransportAction::TogglePlayback);
    assert!(!toolbar.visible(true) && toolbar.visible(false));
    toolbar.activate(TransportAction::Stop);
    assert!(toolbar.visible(true) && !toolbar.visible(false));
    assert_eq!(toolbar.button(TransportAction::TogglePlayback), play);
    assert_eq!(
        toolbar.app.world().resource::<Events>().0,
        [
            TransportAction::TogglePlayback,
            TransportAction::TogglePlayback,
            TransportAction::Stop,
        ]
    );
}

#[test]
fn real_loop_buttons_are_exclusive_and_toggle_back_to_once_then_follow_undo() {
    let mut toolbar = Toolbar::new(EffectPlaybackMode::LoopRestart, "fr-FR", 1.0);
    let restart = toolbar.button(TransportAction::TogglePlaybackMode(
        EffectPlaybackMode::LoopRestart,
    ));
    let continuous = toolbar.button(TransportAction::TogglePlaybackMode(
        EffectPlaybackMode::LoopContinuous,
    ));
    toolbar.activate(TransportAction::TogglePlaybackMode(
        EffectPlaybackMode::LoopContinuous,
    ));
    assert!(!toolbar.app.world().entity(restart).contains::<Selected>());
    assert!(
        toolbar
            .app
            .world()
            .entity(continuous)
            .contains::<Selected>()
    );
    assert_eq!(
        *toolbar
            .app
            .world()
            .get::<ButtonVariant>(continuous)
            .unwrap(),
        ButtonVariant::Primary
    );
    toolbar.activate(TransportAction::TogglePlaybackMode(
        EffectPlaybackMode::LoopContinuous,
    ));
    for entity in [restart, continuous] {
        assert!(!toolbar.app.world().entity(entity).contains::<Selected>());
        assert_eq!(
            *toolbar.app.world().get::<ButtonVariant>(entity).unwrap(),
            ButtonVariant::Normal
        );
    }
    assert_eq!(
        toolbar
            .app
            .world()
            .resource::<EditorSession>()
            .effect
            .playback_mode,
        EffectPlaybackMode::Once
    );
    assert!(toolbar.app.world().resource::<EditorSession>().can_undo());
    toolbar
        .app
        .world_mut()
        .resource_mut::<EditorSession>()
        .undo();
    toolbar.settle();
    assert!(
        toolbar
            .app
            .world()
            .entity(continuous)
            .contains::<Selected>()
    );
    assert!(!toolbar.app.world().entity(restart).contains::<Selected>());
}

#[test]
fn disabled_actual_toolbar_control_cannot_change_playback_or_selected_mode() {
    let mut toolbar = Toolbar::new(EffectPlaybackMode::LoopRestart, "en-US", 1.0);
    let control = toolbar.button(TransportAction::TogglePlaybackMode(
        EffectPlaybackMode::LoopContinuous,
    ));
    toolbar
        .app
        .world_mut()
        .entity_mut(control)
        .insert(InteractionDisabled);
    toolbar
        .app
        .world_mut()
        .trigger(Activate { entity: control });
    toolbar.settle();
    assert!(toolbar.app.world().resource::<Events>().0.is_empty());
    assert_eq!(
        toolbar
            .app
            .world()
            .resource::<EditorSession>()
            .effect
            .playback_mode,
        EffectPlaybackMode::LoopRestart
    );
}

#[test]
fn focused_real_play_button_space_and_enter_activate_exactly_once() {
    for (code, logical) in [(KeyCode::Space, Key::Space), (KeyCode::Enter, Key::Enter)] {
        let mut toolbar = Toolbar::new(EffectPlaybackMode::LoopRestart, "en-US", 1.0);
        let control = toolbar.button(TransportAction::TogglePlayback);
        toolbar
            .app
            .insert_resource(InputFocus::from_entity(control));
        crate::input::tests::tap(&mut toolbar.app, toolbar.window, code, logical);
        toolbar.settle();
        assert_eq!(
            toolbar.app.world().resource::<Events>().0,
            [TransportAction::TogglePlayback]
        );
        assert!(!toolbar.app.world().resource::<EditorSession>().playing);
    }
}

#[test]
fn real_space_activation_of_stop_and_loop_buttons_never_toggles_global_playback() {
    for action in [
        TransportAction::Stop,
        TransportAction::TogglePlaybackMode(EffectPlaybackMode::LoopRestart),
        TransportAction::TogglePlaybackMode(EffectPlaybackMode::LoopContinuous),
    ] {
        let mut toolbar = Toolbar::new(EffectPlaybackMode::LoopRestart, "en-US", 1.0);
        let control = toolbar.button(action);
        toolbar
            .app
            .insert_resource(InputFocus::from_entity(control));
        crate::input::tests::tap(&mut toolbar.app, toolbar.window, KeyCode::Space, Key::Space);
        toolbar.settle();
        assert_eq!(toolbar.app.world().resource::<Events>().0, [action]);
        assert_eq!(
            toolbar.app.world().resource::<EditorSession>().playing,
            action != TransportAction::Stop
        );
    }
}

#[test]
fn disabled_focused_button_ignores_real_space_enter_and_repeats_then_recovers() {
    let mut toolbar = Toolbar::new(EffectPlaybackMode::LoopRestart, "en-US", 1.0);
    let control = toolbar.button(TransportAction::TogglePlayback);
    toolbar
        .app
        .world_mut()
        .entity_mut(control)
        .insert(InteractionDisabled);
    toolbar
        .app
        .insert_resource(InputFocus::from_entity(control));
    for (code, key) in [(KeyCode::Space, Key::Space), (KeyCode::Enter, Key::Enter)] {
        crate::input::tests::tap(&mut toolbar.app, toolbar.window, code, key.clone());
        toolbar
            .app
            .world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
                key_code: code,
                logical_key: key.clone(),
                state: bevy::input::ButtonState::Pressed,
                text: None,
                repeat: true,
                window: toolbar.window,
            });
        // Complete the held-key sequence before testing a fresh activation.
        crate::input::tests::key(
            &mut toolbar.app,
            toolbar.window,
            code,
            key,
            bevy::input::ButtonState::Released,
        );
        toolbar.settle();
    }
    assert!(toolbar.app.world().resource::<Events>().0.is_empty());
    assert!(toolbar.app.world().resource::<EditorSession>().playing);
    toolbar
        .app
        .world_mut()
        .entity_mut(control)
        .remove::<InteractionDisabled>();
    crate::input::tests::tap(&mut toolbar.app, toolbar.window, KeyCode::Enter, Key::Enter);
    toolbar.settle();
    assert_eq!(
        toolbar.app.world().resource::<Events>().0,
        [TransportAction::TogglePlayback]
    );
}

#[test]
fn actual_headless_click_activates_once_and_disabled_click_cannot_dispatch() {
    let mut toolbar = Toolbar::new(EffectPlaybackMode::LoopRestart, "en-US", 1.0);
    let control = toolbar.button(TransportAction::TogglePlayback);
    toolbar
        .app
        .world_mut()
        .entity_mut(control)
        .insert(bevy::ui::Pressed);
    crate::transport_input::click(toolbar.app.world_mut(), control, toolbar.window);
    toolbar.settle();
    assert_eq!(
        toolbar.app.world().resource::<Events>().0,
        [TransportAction::TogglePlayback]
    );
    assert!(!toolbar.app.world().resource::<EditorSession>().playing);
    toolbar
        .app
        .world_mut()
        .entity_mut(control)
        .insert((bevy::ui::Pressed, InteractionDisabled));
    crate::transport_input::click(toolbar.app.world_mut(), control, toolbar.window);
    toolbar.settle();
    assert_eq!(toolbar.app.world().resource::<Events>().0.len(), 1);
}

#[test]
fn focused_transport_button_still_allows_restart_and_frame_step_shortcuts() {
    let mut toolbar = Toolbar::new(EffectPlaybackMode::LoopRestart, "en-US", 1.0);
    let control = toolbar.button(TransportAction::Stop);
    toolbar
        .app
        .insert_resource(InputFocus::from_entity(control));
    crate::input::tests::tap(
        &mut toolbar.app,
        toolbar.window,
        KeyCode::KeyR,
        Key::Character("r".into()),
    );
    crate::input::tests::tap(
        &mut toolbar.app,
        toolbar.window,
        KeyCode::ArrowRight,
        Key::ArrowRight,
    );
    toolbar.settle();
    assert_eq!(
        toolbar.app.world().resource::<Events>().0,
        [TransportAction::Restart, TransportAction::StepFrame(1),]
    );
}

#[test]
fn idle_toolbar_preserves_control_focus_and_loaded_image_identity() {
    let mut toolbar = Toolbar::new(EffectPlaybackMode::LoopRestart, "en-US", 1.0);
    toolbar.wait_icons();
    let control = toolbar.button(TransportAction::TogglePlayback);
    toolbar
        .app
        .insert_resource(InputFocus::from_entity(control));
    let world = toolbar.app.world_mut();
    let mut before: Vec<_> = world
        .query::<(Entity, &ImageNode)>()
        .iter(world)
        .map(|(entity, image)| (entity, image.image.id()))
        .collect();
    before.sort_by_key(|item| item.0);
    for _ in 0..10 {
        toolbar.app.update();
    }
    assert_eq!(toolbar.button(TransportAction::TogglePlayback), control);
    assert_eq!(
        toolbar.app.world().resource::<InputFocus>().get(),
        Some(control)
    );
    let world = toolbar.app.world_mut();
    let mut after: Vec<_> = world
        .query::<(Entity, &ImageNode)>()
        .iter(world)
        .map(|(entity, image)| (entity, image.image.id()))
        .collect();
    after.sort_by_key(|item| item.0);
    assert_eq!(before, after);
    assert!(world.resource::<Events>().0.is_empty());
}

#[test]
fn multiple_toolbars_share_loaded_svg_rasters_and_teardown_releases_them() {
    let mut toolbar = Toolbar::new(EffectPlaybackMode::LoopRestart, "en-US", 1.0);
    toolbar.wait_icons();
    let second = Toolbar::spawn(&mut toolbar.app, toolbar.camera);
    toolbar.settle();
    let world = toolbar.app.world_mut();
    let images: std::collections::HashSet<_> = world
        .query_filtered::<&ImageNode, With<UiSvg>>()
        .iter(world)
        .map(|image| image.image.id())
        .collect();
    assert_eq!(images.len(), 5);
    assert_eq!(world.resource::<Assets<SvgFile>>().len(), 5);
    world.despawn(toolbar.root);
    world.despawn(second);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !toolbar.app.world().resource::<Assets<SvgFile>>().is_empty()
        || images.iter().any(|id| {
            toolbar
                .app
                .world()
                .resource::<Assets<Image>>()
                .contains(*id)
        })
    {
        assert!(
            Instant::now() < deadline,
            "retired toolbar retained its SVG assets"
        );
        toolbar.app.update();
        std::thread::yield_now();
    }
}
