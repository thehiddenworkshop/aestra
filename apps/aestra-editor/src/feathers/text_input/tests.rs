//! Shared shipping/candidate tests of the actual editor composition and value bridge.
use super::*;
use bevy::{
    feathers::{FeathersPlugins, controls::*},
    input::{InputPlugin, keyboard::Key},
    input_focus::{FocusCause, InputDispatchPlugin, InputFocus, InputFocusPlugin},
    render::{RenderPlugin, settings::WgpuSettings},
    text::{TextEdit, TextPlugin},
    ui_widgets::{SliderRange, SliderValue, UiWidgetsPlugins},
    window::{ExitCondition, PrimaryWindow, WindowPlugin},
};

#[derive(Component)]
struct TestField;

#[derive(Resource, Default)]
pub(crate) struct Changes(pub(crate) Vec<(Entity, String, bool)>);

fn capture(change: On<ValueChange<String>>, mut changes: ResMut<Changes>) {
    changes
        .0
        .push((change.source, change.value.clone(), change.is_final));
}

pub(crate) fn app() -> App {
    app_with(|_| {})
}

pub(crate) fn app_with(setup: impl FnOnce(&mut App)) -> App {
    let mut app = App::new();
    // Explicit headless dependencies: DefaultPlugins would initialize Winit in
    // the shipping feature graph, even with no primary window requested.
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
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
        TextPlugin,
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
    ))
    .add_plugins((FeathersPlugins, EditorTextInputPlugin))
    .insert_resource(crate::theme::feathers_theme())
    .init_resource::<Changes>()
    .add_observer(capture);
    setup(&mut app);
    app.finish();
    app.cleanup();
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    app
}

fn settle(app: &mut App) {
    for _ in 0..3 {
        app.update();
    }
}

pub(crate) fn field(app: &mut App, initial: &str) -> (Entity, Entity) {
    let mut container = Entity::PLACEHOLDER;
    let mut commands = app.world_mut().commands();
    commands.spawn(Node::default()).with_children(|parent| {
        container = spawn_text_input(parent, initial, "Effect name", TestField);
    });
    app.world_mut().flush();
    settle(app);
    let input = app.world().get::<Children>(container).unwrap()[0];
    assert!(app.world().get::<FeathersTextInput>(input).is_some());
    app.world_mut().resource_mut::<Changes>().0.clear();
    (container, input)
}

pub(crate) fn focus(app: &mut App, input: Entity) {
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(input, FocusCause::Navigated);
    settle(app);
    app.world_mut().resource_mut::<Changes>().0.clear();
}

pub(crate) fn tap(app: &mut App, code: KeyCode, logical_key: Key, text: Option<&str>) {
    let window = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(world)
            .unwrap()
    };
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: logical_key.clone(),
            state,
            text: if state.is_pressed() {
                text.map(Into::into)
            } else {
                None
            },
            repeat: false,
            window,
        });
    }
    settle(app);
}

pub(crate) fn value(app: &App, input: Entity) -> String {
    app.world()
        .get::<EditableText>(input)
        .unwrap()
        .value()
        .to_string()
}

#[test]
fn composition_keeps_initial_text_accessible_labels_and_container_identity() {
    let mut app = app();
    let (container, input) = field(&mut app, "Étincelles 火");
    assert!(app.world().get::<TestField>(container).is_some());
    assert!(
        app.world()
            .get::<FeathersTextInputContainer>(container)
            .is_some()
    );
    assert_eq!(
        app.world().get::<AccessibleLabel>(container).unwrap().0,
        "Effect name"
    );
    assert_eq!(
        app.world().get::<AccessibleLabel>(input).unwrap().0,
        "Effect name"
    );
    assert_eq!(value(&app, input), "Étincelles 火");
    assert!(
        !app.world()
            .get::<EditableText>(input)
            .unwrap()
            .allow_newlines
    );
}

#[test]
fn real_keyboard_unicode_typing_emits_live_changes_at_the_container() {
    let mut app = app();
    let (container, input) = field(&mut app, "");
    focus(&mut app, input);
    tap(
        &mut app,
        KeyCode::KeyA,
        Key::Character("é火🙂".into()),
        Some("é火🙂"),
    );
    assert_eq!(value(&app, input), "é火🙂");
    let changes = &app.world().resource::<Changes>().0;
    assert!(
        changes
            .iter()
            .any(|c| c == &(container, "é火🙂".into(), false)),
        "{changes:?}"
    );
    assert!(changes.iter().all(|c| c.0 == container && !c.2));
}

#[test]
fn enter_submits_one_final_container_value_without_inserting_a_newline() {
    let mut app = app();
    let (container, input) = field(&mut app, "Spark");
    focus(&mut app, input);
    tap(&mut app, KeyCode::Enter, Key::Enter, None);
    let changes = &app.world().resource::<Changes>().0;
    assert_eq!(
        changes.iter().filter(|c| c.2).collect::<Vec<_>>(),
        [&(container, "Spark".into(), true)]
    );
    assert_eq!(value(&app, input), "Spark");
}

#[test]
fn focus_loss_submits_only_the_field_being_left_once() {
    let mut app = app();
    let (container, input) = field(&mut app, "First");
    let (_, next) = field(&mut app, "Second");
    focus(&mut app, input);
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(next, FocusCause::Pressed);
    settle(&mut app);
    let finals: Vec<_> = app
        .world()
        .resource::<Changes>()
        .0
        .iter()
        .filter(|c| c.2)
        .cloned()
        .collect();
    assert_eq!(finals, [(container, "First".into(), true)]);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(next));
}

#[test]
fn separate_fields_keep_changes_and_parent_identity_isolated() {
    let mut app = app();
    let (first, a) = field(&mut app, "");
    let (second, b) = field(&mut app, "");
    app.world_mut()
        .get_mut::<EditableText>(a)
        .unwrap()
        .queue_edit(TextEdit::Insert("Alpha".into()));
    app.world_mut()
        .get_mut::<EditableText>(b)
        .unwrap()
        .queue_edit(TextEdit::Insert("Beta".into()));
    settle(&mut app);
    let changes = &app.world().resource::<Changes>().0;
    assert!(changes.contains(&(first, "Alpha".into(), false)));
    assert!(changes.contains(&(second, "Beta".into(), false)));
    assert!(changes.iter().all(|c| !c.2));
    assert_eq!(value(&app, a), "Alpha");
    assert_eq!(value(&app, b), "Beta");
}

#[test]
fn unrelated_editable_text_is_not_forwarded_as_an_editor_field_change() {
    let mut app = app();
    let (_, input) = field(&mut app, "");
    app.world_mut()
        .entity_mut(input)
        .remove::<FeathersTextInput>();
    app.world_mut()
        .get_mut::<EditableText>(input)
        .unwrap()
        .queue_edit(TextEdit::Insert("Other".into()));
    settle(&mut app);
    assert_eq!(value(&app, input), "Other");
    assert!(app.world().resource::<Changes>().0.is_empty());
}

#[test]
fn replacement_text_keeps_the_field_identity_and_is_used_by_submit() {
    let mut app = app();
    let (container, input) = field(&mut app, "Old");
    focus(&mut app, input);
    app.world_mut()
        .entity_mut(input)
        .insert(EditableText::new("New 火"));
    settle(&mut app);
    tap(&mut app, KeyCode::Enter, Key::Enter, None);
    assert!(
        app.world()
            .resource::<Changes>()
            .0
            .contains(&(container, "New 火".into(), true))
    );
    assert_eq!(
        app.world().get::<AccessibleLabel>(input).unwrap().0,
        "Effect name"
    );
}

#[test]
fn shared_scene_wrappers_spawn_real_controls_and_preserve_slider_value_and_range() {
    let mut app = app();
    let mut ids = Vec::new();
    let mut commands = app.world_mut().commands();
    macro_rules! spawn {
        ($scene:expr) => {{
            let mut entity = commands.spawn_empty();
            entity.apply_scene($scene);
            ids.push(entity.id());
        }};
    }
    spawn!(scenes::feathers_button());
    spawn!(scenes::feathers_primary_button());
    spawn!(scenes::feathers_plain_button());
    spawn!(scenes::feathers_tool_button());
    spawn!(scenes::feathers_checkbox());
    spawn!(scenes::feathers_text_input_container());
    spawn!(scenes::feathers_text_input());
    spawn!(scenes::feathers_slider(3.25, -4.0, 8.0));
    spawn!(scenes::feathers_color_slider(0.4, ColorChannel::Alpha));
    let target = commands.spawn(Node::default()).id();
    spawn!(scenes::feathers_vertical_scrollbar(target));
    spawn!(scenes::feathers_horizontal_scrollbar(target));
    spawn!(scenes::feathers_menu());
    spawn!(scenes::feathers_menu_button());
    spawn!(scenes::feathers_menu_popup());
    spawn!(scenes::feathers_menu_item());
    spawn!(scenes::feathers_menu_divider());
    spawn!(scenes::feathers_hue_saturation_plane());
    app.world_mut().flush();
    settle(&mut app);
    macro_rules! has {
        ($index:expr, $ty:ty) => {
            assert!(
                app.world().get::<$ty>(ids[$index]).is_some(),
                stringify!($ty)
            );
        };
    }
    has!(0, FeathersButton);
    assert_eq!(
        *app.world().get::<ButtonVariant>(ids[1]).unwrap(),
        ButtonVariant::Primary
    );
    assert_eq!(
        *app.world().get::<ButtonVariant>(ids[2]).unwrap(),
        ButtonVariant::Plain
    );
    has!(3, FeathersToolButton);
    has!(4, FeathersCheckbox);
    has!(5, FeathersTextInputContainer);
    has!(6, FeathersTextInput);
    has!(7, FeathersSlider);
    assert_eq!(app.world().get::<SliderValue>(ids[7]).unwrap().0, 3.25);
    let range = app.world().get::<SliderRange>(ids[7]).unwrap();
    assert_eq!((range.start(), range.end()), (-4.0, 8.0));
    has!(8, FeathersColorSlider);
    has!(9, FeathersScrollbar);
    has!(10, FeathersScrollbar);
    has!(11, FeathersMenu);
    has!(12, FeathersMenuButton);
    has!(13, FeathersMenuPopup);
    has!(14, FeathersMenuItem);
    has!(15, FeathersMenuDivider);
    assert_eq!(
        *app.world().get::<FeathersColorPlane>(ids[16]).unwrap(),
        FeathersColorPlane::HueSaturation
    );
}
