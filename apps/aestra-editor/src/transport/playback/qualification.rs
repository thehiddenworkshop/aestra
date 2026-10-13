//! Real input/plugin/action/session integration on both engines.
use super::*;
use crate::{
    asset_actions::AssetOperationState,
    asset_browser::BrowserSurface,
    feathers::code_editor::CodeEditor,
    input::EditorKeyboardPlugin,
    material_function_editor::graph::FunctionGraphMenuState,
    material_graph::MaterialGraphPaletteState,
    menus::{MenuKind, MenuState, TabContextMenu},
    persistence::DocumentProtectionState,
};
use bevy::{
    input::{InputPlugin, keyboard::Key},
    input_focus::{InputDispatchPlugin, InputFocusPlugin},
    time::TimeUpdateStrategy,
    window::PrimaryWindow,
};
use std::time::Duration;

#[derive(Resource, Default)]
struct Actions(Vec<TransportAction>);

struct Fixture {
    app: App,
    window: Entity,
}
impl Fixture {
    fn new(mode: EffectPlaybackMode) -> Self {
        let mut session = crate::test_support::session_with_playback_mode(mode);
        session.seek_time(0.5);
        session.playing = true;
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputPlugin,
            InputFocusPlugin,
            InputDispatchPlugin,
            EditorKeyboardPlugin,
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
        .insert_resource(session)
        .init_resource::<ModulePaletteState>()
        .init_resource::<Actions>()
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
        .add_observer(
            |action: On<TransportAction>, mut actions: ResMut<Actions>| {
                actions.0.push(*action);
            },
        );
        register(&mut app);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        app.update();
        Self { app, window }
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource::<EditorSession>()
    }
    fn key(&mut self, code: KeyCode, logical: Key, state: ButtonState, repeat: bool) {
        self.app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: logical,
            state,
            repeat,
            text: None,
            window: self.window,
        });
    }
    fn tap(&mut self, code: KeyCode, logical: Key) {
        self.key(code, logical.clone(), ButtonState::Pressed, false);
        self.key(code, logical, ButtonState::Released, false);
    }
    fn repeat(&mut self, code: KeyCode, logical: Key) {
        self.key(code, logical, ButtonState::Pressed, true);
    }
    fn update(&mut self) {
        self.app.update();
    }
    fn events(&self) -> &[TransportAction] {
        &self.app.world().resource::<Actions>().0
    }
    fn assert_events(&self, expected: &[TransportAction]) {
        assert_eq!(self.events(), expected);
    }
}

#[test]
fn same_frame_play_restart_and_step_chords_execute_once_in_input_order() {
    let mut fixture = Fixture::new(EffectPlaybackMode::LoopRestart);
    fixture.tap(KeyCode::Space, Key::Space);
    fixture.tap(KeyCode::KeyR, Key::Character("r".into()));
    fixture.tap(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.update();
    fixture.assert_events(&[
        TransportAction::TogglePlayback,
        TransportAction::Restart,
        TransportAction::StepFrame(1),
    ]);
    assert_eq!(fixture.session().frame(), 1);
    assert!(!fixture.session().playing);
    fixture.update();
    assert_eq!(fixture.events().len(), 3);
    assert_eq!(fixture.session().frame(), 1);
}

#[test]
fn restart_follows_logical_letters_not_qwerty_positions() {
    let mut fixture = Fixture::new(EffectPlaybackMode::LoopRestart);
    fixture.tap(KeyCode::KeyR, Key::Character("p".into()));
    fixture.update();
    fixture.assert_events(&[]);
    fixture.tap(KeyCode::KeyP, Key::Character("R".into()));
    fixture.update();
    fixture.assert_events(&[TransportAction::Restart]);
    assert_eq!(fixture.session().frame(), 0);
}

#[test]
fn repeats_step_both_directions_but_never_repeat_play_or_restart() {
    let mut fixture = Fixture::new(EffectPlaybackMode::Once);
    let frame = fixture.session().frame();
    fixture.repeat(KeyCode::Space, Key::Space);
    fixture.repeat(KeyCode::KeyR, Key::Character("r".into()));
    fixture.repeat(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.repeat(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.repeat(KeyCode::ArrowLeft, Key::ArrowLeft);
    fixture.update();
    fixture.assert_events(&[
        TransportAction::StepFrame(1),
        TransportAction::StepFrame(1),
        TransportAction::StepFrame(-1),
    ]);
    assert_eq!(fixture.session().frame(), frame + 1);
    assert!(!fixture.session().playing);
    fixture.update();
    assert_eq!(fixture.events().len(), 3);
}

#[test]
fn event_time_alt_gating_distinguishes_repeats_in_the_same_batch() {
    let mut fixture = Fixture::new(EffectPlaybackMode::Once);
    let frame = fixture.session().frame();
    fixture.key(KeyCode::AltLeft, Key::Alt, ButtonState::Pressed, false);
    fixture.tap(KeyCode::ArrowLeft, Key::ArrowLeft);
    fixture.repeat(KeyCode::ArrowLeft, Key::ArrowLeft);
    fixture.key(KeyCode::AltLeft, Key::Alt, ButtonState::Released, false);
    fixture.tap(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.repeat(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.update();
    fixture.assert_events(&[TransportAction::StepFrame(1), TransportAction::StepFrame(1)]);
    assert_eq!(fixture.session().frame(), frame + 2);
}

#[test]
fn held_alt_sides_survive_frames_and_releasing_one_side_still_reserves_arrows() {
    let mut fixture = Fixture::new(EffectPlaybackMode::Once);
    fixture.key(KeyCode::AltLeft, Key::Alt, ButtonState::Pressed, false);
    fixture.key(KeyCode::AltRight, Key::Alt, ButtonState::Pressed, false);
    fixture.update();
    fixture.key(KeyCode::AltLeft, Key::Alt, ButtonState::Released, false);
    fixture.repeat(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.update();
    fixture.assert_events(&[]);
    fixture.key(KeyCode::AltRight, Key::Alt, ButtonState::Released, false);
    fixture.repeat(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.update();
    fixture.assert_events(&[TransportAction::StepFrame(1)]);
}

#[test]
fn focus_loss_drops_ambiguous_discrete_and_repeat_inputs_without_replaying_them() {
    let mut fixture = Fixture::new(EffectPlaybackMode::Once);
    let frame = fixture.session().frame();
    fixture.tap(KeyCode::Space, Key::Space);
    fixture.repeat(KeyCode::ArrowRight, Key::ArrowRight);
    fixture
        .app
        .world_mut()
        .write_message(bevy::input::keyboard::KeyboardFocusLost);
    fixture.update();
    fixture.assert_events(&[]);
    assert_eq!(fixture.session().frame(), frame);
    fixture.update();
    fixture.assert_events(&[]);
    fixture.tap(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.update();
    fixture.assert_events(&[TransportAction::StepFrame(1)]);
}

fn blocked_then_released(set: impl FnOnce(&mut World), clear: impl FnOnce(&mut World)) {
    let mut fixture = Fixture::new(EffectPlaybackMode::LoopRestart);
    let frame = fixture.session().frame();
    let effect = fixture.session().effect.clone();
    let revision = fixture.session().ui_revision;
    let status = fixture.session().status.clone();
    set(fixture.app.world_mut());
    fixture.tap(KeyCode::Space, Key::Space);
    fixture.tap(KeyCode::KeyR, Key::Character("r".into()));
    fixture.tap(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.repeat(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.key(
        KeyCode::ArrowRight,
        Key::ArrowRight,
        ButtonState::Released,
        false,
    );
    fixture.update();
    fixture.assert_events(&[]);
    assert_eq!(fixture.session().frame(), frame);
    assert!(fixture.session().playing);
    assert_eq!(fixture.session().effect, effect);
    assert_eq!(fixture.session().ui_revision, revision);
    assert_eq!(fixture.session().status, status);
    clear(fixture.app.world_mut());
    fixture.update();
    fixture.assert_events(&[]);
    fixture.tap(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.update();
    fixture.assert_events(&[TransportAction::StepFrame(1)]);
    assert_eq!(fixture.session().frame(), frame + 1);
}

#[test]
fn every_document_modal_drops_transport_and_repeat_work_until_a_fresh_chord() {
    for state in DocumentProtectionState::blocking_states_for_test() {
        blocked_then_released(
            move |world| {
                world.insert_resource(state);
            },
            |world| {
                world.insert_resource(DocumentProtectionState::default());
            },
        );
    }
}

#[test]
fn extraction_and_every_material_function_menu_block_transport_without_stale_repeats() {
    blocked_then_released(
        |world| {
            world.insert_resource(AssetOperationState::open_for_test());
        },
        |world| {
            world.insert_resource(AssetOperationState::default());
        },
    );
    for state in MaterialGraphPaletteState::blocking_states_for_test() {
        blocked_then_released(
            move |world| {
                world.insert_resource(state);
            },
            |world| {
                world.insert_resource(MaterialGraphPaletteState::default());
            },
        );
    }
    for state in FunctionGraphMenuState::blocking_states_for_test() {
        blocked_then_released(
            move |world| {
                world.insert_resource(state);
            },
            |world| {
                world.insert_resource(FunctionGraphMenuState::default());
            },
        );
    }
}

#[test]
fn every_main_panel_tab_about_menu_and_module_palette_reserves_transport() {
    let mut states: Vec<_> = [
        MenuKind::File,
        MenuKind::Edit,
        MenuKind::View,
        MenuKind::Help,
    ]
    .into_iter()
    .map(|kind| MenuState {
        open: Some(kind),
        ..default()
    })
    .collect();
    states.extend([
        MenuState {
            panels_open: true,
            ..default()
        },
        MenuState {
            tab_context: Some(TabContextMenu {
                panel: crate::docking::ToolPanel::Timeline,
                position: [0.0, 0.0],
            }),
            ..default()
        },
        MenuState {
            show_about: true,
            ..default()
        },
    ]);
    for state in states {
        blocked_then_released(
            move |world| {
                world.insert_resource(state);
            },
            |world| {
                world.insert_resource(MenuState::default());
            },
        );
    }
    blocked_then_released(
        |world| {
            world.resource_mut::<ModulePaletteState>().open = true;
        },
        |world| {
            world.resource_mut::<ModulePaletteState>().open = false;
        },
    );
}

#[test]
fn code_text_and_menu_descendants_reserve_transport_and_despawn_releases_ownership() {
    for kind in 0..3 {
        blocked_then_released(
            move |world| {
                let owner = match kind {
                    0 => world.spawn(CodeEditor::new("source".into())).id(),
                    1 => world.spawn(bevy::text::EditableText::new("filter")).id(),
                    _ => world.spawn(bevy::ui_widgets::MenuItem).id(),
                };
                let child = world.spawn(ChildOf(owner)).id();
                world.insert_resource(InputFocus::from_entity(child));
            },
            |world| {
                let child = world.resource::<InputFocus>().get().unwrap();
                let owner = world.get::<ChildOf>(child).unwrap().parent();
                world.despawn(owner);
            },
        );
    }
}

#[test]
fn all_unfocused_windows_block_but_focused_floating_window_allows_global_transport() {
    let mut fixture = Fixture::new(EffectPlaybackMode::LoopRestart);
    fixture
        .app
        .world_mut()
        .get_mut::<Window>(fixture.window)
        .unwrap()
        .focused = false;
    fixture.tap(KeyCode::Space, Key::Space);
    fixture.repeat(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.update();
    fixture.assert_events(&[]);
    fixture.app.world_mut().spawn(Window {
        focused: true,
        ..default()
    });
    fixture.update();
    fixture.assert_events(&[]);
    fixture.tap(KeyCode::Space, Key::Space);
    fixture.update();
    fixture.assert_events(&[TransportAction::TogglePlayback]);
    assert!(!fixture.session().playing);
}

#[test]
fn assets_keeps_global_transport_available_except_while_editing_its_search() {
    let mut fixture = Fixture::new(EffectPlaybackMode::LoopRestart);
    let surface = fixture.app.world_mut().spawn(BrowserSurface).id();
    let search = fixture
        .app
        .world_mut()
        .spawn((bevy::text::EditableText::new("Find"), ChildOf(surface)))
        .id();
    fixture.app.insert_resource(InputFocus::from_entity(search));
    fixture.tap(KeyCode::Space, Key::Space);
    fixture.update();
    fixture.assert_events(&[]);
    fixture
        .app
        .insert_resource(InputFocus::from_entity(surface));
    fixture.tap(KeyCode::Space, Key::Space);
    fixture.update();
    fixture.assert_events(&[TransportAction::TogglePlayback]);
    assert!(!fixture.session().playing);
}

#[test]
fn raw_repeat_fallback_without_collector_steps_once_and_obeys_alt_and_modal_gates() {
    let mut app = App::new();
    let mut session = crate::test_support::session_with_playback_mode(EffectPlaybackMode::Once);
    session.seek_time(0.5);
    let frame = session.frame();
    app.insert_resource(session)
        .init_resource::<ModulePaletteState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Actions>()
        .add_message::<KeyboardInput>()
        .add_observer(execute_transport_action)
        .add_observer(
            |action: On<TransportAction>, mut actions: ResMut<Actions>| {
                actions.0.push(*action);
            },
        )
        .add_systems(Update, transport_keyboard_input);
    let repeat = KeyboardInput {
        key_code: KeyCode::ArrowRight,
        logical_key: Key::ArrowRight,
        state: ButtonState::Pressed,
        text: None,
        repeat: true,
        window: Entity::PLACEHOLDER,
    };
    app.world_mut().write_message(repeat.clone());
    app.update();
    assert_eq!(app.world().resource::<EditorSession>().frame(), frame + 1);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::AltRight);
    app.world_mut().write_message(repeat.clone());
    app.update();
    assert_eq!(app.world().resource::<EditorSession>().frame(), frame + 1);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::AltRight);
    app.world_mut().resource_mut::<ModulePaletteState>().open = true;
    app.world_mut().write_message(repeat);
    app.update();
    app.world_mut().resource_mut::<ModulePaletteState>().open = false;
    app.update();
    assert_eq!(
        app.world().resource::<Actions>().0,
        [TransportAction::StepFrame(1)]
    );
}

#[test]
fn snapshot_collector_and_raw_repeat_reader_cannot_dispatch_the_same_event_twice() {
    let mut fixture = Fixture::new(EffectPlaybackMode::Once);
    fixture.tap(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.repeat(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.update();
    fixture.assert_events(&[TransportAction::StepFrame(1), TransportAction::StepFrame(1)]);
    fixture.update();
    fixture.update();
    assert_eq!(fixture.events().len(), 2);
}

#[test]
fn stop_restart_and_step_keep_seed_and_do_not_dirty_the_authored_effect() {
    let mut fixture = Fixture::new(EffectPlaybackMode::Once);
    let effect = fixture.session().effect.clone();
    let seed = fixture.session().preview_seed;
    for action in [
        TransportAction::Stop,
        TransportAction::Restart,
        TransportAction::StepFrame(-1),
    ] {
        fixture.app.world_mut().trigger(action);
        fixture.update();
        assert_eq!(fixture.session().frame(), 0);
        assert_eq!(fixture.session().preview_seed, seed);
        assert_eq!(fixture.session().effect, effect);
        assert!(!fixture.session().can_undo());
    }
    assert!(!fixture.session().playing);
}

#[test]
fn once_play_resets_while_loop_play_resumes_at_the_same_frame() {
    for mode in [
        EffectPlaybackMode::Once,
        EffectPlaybackMode::LoopRestart,
        EffectPlaybackMode::LoopContinuous,
    ] {
        let mut fixture = Fixture::new(mode);
        let frame = fixture.session().frame();
        fixture.tap(KeyCode::Space, Key::Space);
        fixture.update();
        assert!(!fixture.session().playing);
        assert_eq!(fixture.session().frame(), frame);
        fixture.tap(KeyCode::Space, Key::Space);
        fixture.update();
        assert!(fixture.session().playing);
        assert_eq!(
            fixture.session().frame(),
            if mode.is_looping() { frame } else { 0 }
        );
    }
}

#[test]
fn actual_registration_advances_bevy_time_at_session_speed_and_honors_pause_and_stop() {
    let mut fixture = Fixture::new(EffectPlaybackMode::LoopContinuous);
    fixture
        .app
        .world_mut()
        .resource_mut::<EditorSession>()
        .speed = 2.0;
    let frame = fixture.session().frame();
    fixture
        .app
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            0.05,
        )));
    fixture.update();
    assert_eq!(fixture.session().frame(), frame + 6);
    fixture.tap(KeyCode::Space, Key::Space);
    fixture.update();
    assert_eq!(
        fixture.session().frame(),
        frame + 6,
        "input must pause before the registered playback system advances"
    );
    fixture.update();
    assert_eq!(fixture.session().frame(), frame + 6);
    fixture.app.world_mut().trigger(TransportAction::Stop);
    fixture.update();
    assert_eq!(fixture.session().frame(), 0);
    assert!(!fixture.session().playing);
}

#[test]
fn registered_once_playback_stops_at_end_and_continuous_loop_crosses_boundaries() {
    for mode in [EffectPlaybackMode::Once, EffectPlaybackMode::LoopContinuous] {
        let mut fixture = Fixture::new(mode);
        let duration = fixture.session().playback_duration();
        fixture
            .app
            .world_mut()
            .resource_mut::<EditorSession>()
            .seek_time(duration - 0.05);
        fixture
            .app
            .world_mut()
            .resource_mut::<EditorSession>()
            .playing = true;
        fixture
            .app
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                0.1,
            )));
        fixture.update();
        if mode == EffectPlaybackMode::Once {
            assert!(!fixture.session().playing);
            assert_eq!(fixture.session().time(), duration);
        } else {
            assert!(fixture.session().playing);
            assert!(fixture.session().simulation_time() > duration);
            assert!(fixture.session().time() < duration);
        }
    }
}

#[test]
fn modal_ownership_blocks_keyboard_without_implicitly_pausing_live_playback() {
    let mut fixture = Fixture::new(EffectPlaybackMode::LoopContinuous);
    let frame = fixture.session().frame();
    fixture.app.insert_resource(MenuState {
        open: Some(MenuKind::Edit),
        ..default()
    });
    fixture
        .app
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            0.05,
        )));
    fixture.tap(KeyCode::Space, Key::Space);
    fixture.repeat(KeyCode::ArrowRight, Key::ArrowRight);
    fixture.update();
    fixture.assert_events(&[]);
    assert!(fixture.session().playing);
    assert_eq!(fixture.session().frame(), frame + 3);
    assert_eq!(
        fixture.app.world().resource::<MenuState>().open,
        Some(MenuKind::Edit)
    );
}
