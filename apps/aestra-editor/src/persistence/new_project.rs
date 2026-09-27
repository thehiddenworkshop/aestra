//! Project creation UI. Disk work and publication reuse the protected project-open pipeline.
use super::*;
use crate::project::{CreateProjectError, CreateProjectRequest};
use crate::project_content::io::IoGuard;
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::input::{ButtonState, keyboard::KeyboardInput};
use bevy::input_focus::FocusedInput;
use bevy::input_focus::{FocusCause, InputFocus, tab_navigation::TabGroup};
use bevy::text::{EditableText, TextEdit};
use bevy::ui_widgets::ValueChange;

#[derive(Event)]
pub(super) struct OpenNewProject;

#[derive(Resource, Default)]
struct Prompt {
    overlay: Option<Entity>,
    name: String,
    location: String,
    guard: Option<IoGuard>,
    busy: bool,
    error: Option<String>,
    return_focus: Option<Entity>,
    edited: bool,
    location_edited: bool,
}

#[derive(Component, PartialEq, Eq)]
enum Field {
    Name,
    Location,
}
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Browse,
    Cancel,
    Create,
}
#[derive(Component)]
struct Destination;
#[derive(Component)]
struct DestinationSummary;
#[derive(Component)]
struct Feedback;
#[derive(Component)]
struct PendingFocus;

pub(super) fn register(app: &mut App) {
    app.init_resource::<Prompt>()
        .add_observer(open)
        .add_observer(change)
        .add_observer(choose)
        .add_observer(keyboard)
        .add_systems(
            Update,
            (escape, sync).chain().in_set(PersistenceSet::Actions),
        );
}

fn request(prompt: &Prompt) -> CreateProjectRequest {
    CreateProjectRequest {
        name: prompt.name.clone(),
        parent: PathBuf::from(&prompt.location),
    }
}

// Canonical Windows paths are useful for I/O, but their device prefix is not UI copy.
fn display_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    if let Some(network) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{network}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
    }
}

fn error_text(error: CreateProjectError, localizer: &Localizer) -> String {
    match error {
        CreateProjectError::InvalidName => localizer.text("project-create-invalid-name"),
        CreateProjectError::InvalidLocation => localizer.text("project-create-invalid-location"),
        CreateProjectError::DestinationExists => localizer.text("project-create-exists"),
        CreateProjectError::Io(message) => {
            format!("{}\n{message}", localizer.text("project-create-failed"))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn open(
    _: On<OpenNewProject>,
    mut commands: Commands,
    mut prompt: ResMut<Prompt>,
    mut protection: ResMut<DocumentProtectionState>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    localizer: Res<Localizer>,
    focus: Option<Res<InputFocus>>,
) {
    if prompt.overlay.is_some() {
        return;
    }
    let root = catalog.root();
    let project_root = if root.file_name().is_some_and(|name| name == "assets") {
        root.parent().unwrap_or(root)
    } else {
        root
    };
    *prompt = Prompt {
        location: display_path(project_root.parent().unwrap_or(project_root)),
        guard: Some(IoGuard::capture(&catalog, &session)),
        return_focus: focus.as_deref().and_then(InputFocus::get),
        ..default()
    };
    protection.project_create_open = true;
    prompt.overlay = Some(
        commands
            .spawn((
                TabGroup::modal(),
                crate::feathers::node_graph::FeathersGraphNavigationBlocker,
                RelativeCursorPosition::default(),
                GlobalZIndex(325),
                Pickable {
                    should_block_lower: true,
                    is_hoverable: true,
                },
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            ))
            .with_children(|overlay| {
                overlay
                    .spawn((
                        Node {
                            width: Val::Px(680.0),
                            max_width: Val::Percent(92.0),
                            padding: UiRect::all(Val::Px(28.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(10.0)),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(18.0),
                            ..default()
                        },
                        BackgroundColor(theme::PANEL),
                        BorderColor::all(theme::BORDER_BRIGHT),
                    ))
                    .with_children(|panel| {
                        panel
                            .spawn(Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(8.0),
                                padding: UiRect::bottom(Val::Px(8.0)),
                                ..default()
                            })
                            .with_children(|header| {
                                text_label(
                                    header,
                                    localizer.text("file-new-project"),
                                    26.0,
                                    theme::TEXT,
                                );
                            });
                        panel
                            .spawn(Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(8.0),
                                ..default()
                            })
                            .with_children(|group| {
                                label(group, localizer.text("project-create-name"));
                                let name = input(
                                    group,
                                    "",
                                    &localizer.text("project-create-name"),
                                    Field::Name,
                                );
                                group.commands().entity(name).insert(PendingFocus);
                            });
                        panel
                            .spawn(Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(8.0),
                                ..default()
                            })
                            .with_children(|group| {
                                label(group, localizer.text("project-create-location"));
                                group
                                    .spawn(Node {
                                        column_gap: Val::Px(10.0),
                                        align_items: AlignItems::Center,
                                        ..default()
                                    })
                                    .with_children(|row| {
                                        input(
                                            row,
                                            &prompt.location,
                                            &localizer.text("project-create-location"),
                                            Field::Location,
                                        );
                                        dialog_button(
                                            row,
                                            &localizer.text("project-create-browse"),
                                            Choice::Browse,
                                            false,
                                        );
                                    });
                            });
                        panel
                            .spawn((
                                DestinationSummary,
                                Node {
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(8.0),
                                    padding: UiRect::all(Val::Px(14.0)),
                                    border_radius: BorderRadius::all(Val::Px(6.0)),
                                    min_width: Val::Px(0.0),
                                    ..default()
                                },
                                BackgroundColor(theme::PANEL_DARK),
                            ))
                            .with_children(|summary| {
                                text_label(
                                    summary,
                                    localizer.text("project-create-destination"),
                                    12.0,
                                    theme::TEXT_MUTED,
                                );
                                summary.spawn((
                                    Destination,
                                    Text::default(),
                                    TextFont {
                                        font_size: 12.0.into(),
                                        ..default()
                                    },
                                    TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
                                    Node {
                                        width: Val::Percent(100.0),
                                        min_width: Val::Px(0.0),
                                        ..default()
                                    },
                                    TextColor(theme::TEXT),
                                    Pickable::IGNORE,
                                ));
                            });
                        panel.spawn((
                            Feedback,
                            Text::default(),
                            TextFont {
                                font_size: 12.0.into(),
                                ..default()
                            },
                            TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
                            Node {
                                width: Val::Percent(100.0),
                                min_width: Val::Px(0.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_MUTED),
                            Pickable::IGNORE,
                        ));
                        panel
                            .spawn(Node {
                                justify_content: JustifyContent::End,
                                column_gap: Val::Px(10.0),
                                padding: UiRect::top(Val::Px(8.0)),
                                ..default()
                            })
                            .with_children(|buttons| {
                                dialog_button(
                                    buttons,
                                    &localizer.text("transaction-cancel"),
                                    Choice::Cancel,
                                    false,
                                );
                                dialog_button(
                                    buttons,
                                    &localizer.text("project-create-confirm"),
                                    Choice::Create,
                                    true,
                                );
                            });
                    });
            })
            .id(),
    );
}

fn label(parent: &mut ChildSpawnerCommands, text: String) {
    text_label(parent, text, 13.0, theme::TEXT);
}

fn text_label(parent: &mut ChildSpawnerCommands, text: String, size: f32, color: Color) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: size.into(),
            ..default()
        },
        TextColor(color),
        TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
        Pickable::IGNORE,
    ));
}

fn input(parent: &mut ChildSpawnerCommands, value: &str, label: &str, field: Field) -> Entity {
    let location = field == Field::Location;
    let entity = crate::feathers::text_input::spawn_text_input(parent, value, label, field);
    parent.commands().entity(entity).insert((
        Node {
            height: Val::Px(40.0),
            width: if location {
                Val::Px(0.0)
            } else {
                Val::Percent(100.0)
            },
            min_width: Val::Px(0.0),
            flex_grow: if location { 1.0 } else { 0.0 },
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(Val::Px(10.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
            ..default()
        },
        ThemeBackgroundColor(bevy::feathers::tokens::TEXT_INPUT_LABEL_BG),
        BorderColor::all(theme::BORDER_BRIGHT),
    ));
    entity
}

fn dialog_button(parent: &mut ChildSpawnerCommands, label: &str, choice: Choice, primary: bool) {
    let mut button = parent.spawn_empty();
    if primary {
        button.apply_scene(crate::feathers::scenes::feathers_primary_button());
    } else {
        button.apply_scene(crate::feathers::scenes::feathers_button());
    }
    button
        .insert((
            choice,
            crate::feathers::button::FeathersActionButton,
            AccessibleLabel(label.to_owned()),
            Node {
                height: Val::Px(38.0),
                min_width: Val::Px(if primary { 148.0 } else { 100.0 }),
                flex_shrink: 0.0,
                padding: UiRect::horizontal(Val::Px(14.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((Text::new(label), ThemedText, Pickable::IGNORE));
        });
}

fn change(event: On<ValueChange<String>>, fields: Query<&Field>, mut prompt: ResMut<Prompt>) {
    if prompt.overlay.is_none() || prompt.busy {
        return;
    }
    let Ok(field) = fields.get(event.source) else {
        return;
    };
    match field {
        Field::Name => {
            if prompt.name == event.value {
                return;
            }
            prompt.name.clone_from(&event.value);
            prompt.edited = true;
        }
        Field::Location => {
            if prompt.location == event.value {
                return;
            }
            prompt.location.clone_from(&event.value);
            prompt.location_edited = true;
        }
    }
    prompt.error = None;
}

fn update_location_text(text: &mut EditableText, location: &str) {
    // Keep the live editor's font styles and layout generation. Replacing the
    // component resets them, but unchanged TextFont won't apply them a second time.
    text.clear();
    text.editor_mut().set_text(location);
    text.queue_edit(TextEdit::TextStart(false));
}

fn keyboard(
    mut event: On<FocusedInput<KeyboardInput>>,
    parents: Query<&ChildOf>,
    fields: Query<(), With<Field>>,
    choices: Query<(Entity, &Choice)>,
    mut commands: Commands,
) {
    if event.input.state == ButtonState::Pressed
        && event.input.key_code == KeyCode::Enter
        && !event.input.repeat
        && parents
            .get(event.event_target())
            .is_ok_and(|parent| fields.contains(parent.parent()))
    {
        event.propagate(false);
        if let Some((entity, _)) = choices
            .iter()
            .find(|(_, choice)| **choice == Choice::Create)
        {
            commands.trigger(Activate { entity });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn choose(
    event: On<Activate>,
    choices: Query<&Choice>,
    fields: Query<&Field>,
    mut texts: Query<(&mut EditableText, &ChildOf)>,
    mut prompt: ResMut<Prompt>,
    mut commands: Commands,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    settings: Res<EditorSettings>,
    localizer: Res<Localizer>,
    mut protection: ResMut<DocumentProtectionState>,
    io_tasks: Option<Res<crate::project_content::io::ProjectIoTasks>>,
) {
    let Ok(choice) = choices.get(event.entity) else {
        return;
    };
    if prompt.overlay.is_none() || prompt.busy {
        return;
    }
    if *choice == Choice::Cancel {
        close(&mut commands, &mut prompt, &mut protection);
        return;
    }
    // Read the live edit buffer; a button press may precede the value-change bridge.
    for (text, parent) in &texts {
        match fields.get(parent.parent()) {
            Ok(Field::Name) => prompt.name = text.value().to_string(),
            Ok(Field::Location) => prompt.location = text.value().to_string(),
            _ => {}
        }
    }
    if *choice == Choice::Browse {
        if let Some(folder) = FileDialog::new()
            .set_title(localizer.text("project-create-location"))
            .set_directory(&prompt.location)
            .pick_folder()
        {
            prompt.location = display_path(&folder);
            prompt.error = None;
            for (mut text, parent) in &mut texts {
                if fields.get(parent.parent()) == Ok(&Field::Location) {
                    update_location_text(&mut text, &prompt.location);
                }
            }
        }
        return;
    }
    if !crate::project_content::io::idle(io_tasks) {
        return;
    }
    if !prompt
        .guard
        .as_ref()
        .is_some_and(|guard| guard.matches(&catalog, &session))
    {
        prompt.error = Some(localizer.text("project-operation-open-stale"));
        return;
    }
    let request = request(&prompt);
    if let Err(error) = request.validate() {
        prompt.error = Some(error_text(error, &localizer));
        return;
    }
    prompt.error = None;
    prompt.busy = true;
    background::queue_project_creation(
        &mut commands,
        &session,
        &settings,
        &catalog,
        &localizer,
        request,
    );
}

fn close(commands: &mut Commands, prompt: &mut Prompt, protection: &mut DocumentProtectionState) {
    if let Some(overlay) = prompt.overlay.take() {
        commands.entity(overlay).try_despawn();
    }
    let previous = prompt.return_focus.take();
    commands.queue(move |world: &mut World| {
        let previous = previous.filter(|entity| world.get_entity(*entity).is_ok());
        if let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
            if let Some(entity) = previous {
                focus.set(entity, FocusCause::Navigated);
            } else {
                focus.clear();
            }
        }
    });
    prompt.guard = None;
    protection.project_create_open = false;
}

fn escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut prompt: ResMut<Prompt>,
    mut protection: ResMut<DocumentProtectionState>,
) {
    if keys.just_pressed(KeyCode::Escape) && prompt.overlay.is_some() && !prompt.busy {
        close(&mut commands, &mut prompt, &mut protection);
    }
}

#[allow(clippy::too_many_arguments)]
fn sync(
    prompt: Res<Prompt>,
    localizer: Res<Localizer>,
    mut commands: Commands,
    mut destinations: Query<&mut Text, (With<Destination>, Without<Feedback>)>,
    mut summaries: Query<&mut Node, (With<DestinationSummary>, Without<Feedback>)>,
    mut feedback: Query<
        (&mut Text, &mut TextColor, &mut Node),
        (With<Feedback>, Without<Destination>),
    >,
    controls: Query<(Entity, Option<&Choice>, Option<&Field>)>,
    parents: Query<&ChildOf>,
    inputs: Query<Entity, With<EditableText>>,
    pending_focus: Query<(Entity, &Children), With<PendingFocus>>,
    mut focus: Option<ResMut<InputFocus>>,
) {
    for (container, children) in &pending_focus {
        if let Some(input) = children.iter().find(|child| inputs.contains(*child)) {
            if let Some(focus) = focus.as_deref_mut() {
                focus.set(input, FocusCause::Navigated);
            }
            commands.entity(container).remove::<PendingFocus>();
        }
    }
    if prompt.overlay.is_none() || !prompt.is_changed() {
        return;
    }
    let request = request(&prompt);
    let validation = request.validate();
    for mut text in &mut destinations {
        text.0 = display_path(&request.destination());
    }
    for mut node in &mut summaries {
        node.display = if prompt.name.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
    }
    for (mut text, mut color, mut node) in &mut feedback {
        text.0 = if prompt.busy {
            localizer.text("project-create-running")
        } else if let Some(error) = &prompt.error {
            error.clone()
        } else {
            validation
                .as_ref()
                .err()
                .filter(|error| match error {
                    CreateProjectError::InvalidName => prompt.edited,
                    _ => prompt.location_edited,
                })
                .map(|error| match error {
                    CreateProjectError::InvalidName => {
                        localizer.text("project-create-invalid-name")
                    }
                    _ => localizer.text("project-create-invalid-location"),
                })
                .unwrap_or_default()
        };
        node.display = if text.0.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        color.0 = if !prompt.busy && !text.0.is_empty() {
            Color::srgb(1.0, 0.62, 0.57)
        } else {
            theme::TEXT_MUTED
        };
    }
    for (entity, choice, field) in &controls {
        if choice.is_none() && field.is_none() {
            continue;
        }
        let disabled = prompt.busy || (choice == Some(&Choice::Create) && validation.is_err());
        if disabled {
            commands.entity(entity).insert(InteractionDisabled);
        } else {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
        if field.is_some() {
            for input in &inputs {
                if parents
                    .get(input)
                    .is_ok_and(|parent| parent.parent() == entity)
                {
                    if disabled {
                        commands.entity(input).insert(InteractionDisabled);
                    } else {
                        commands.entity(input).remove::<InteractionDisabled>();
                    }
                }
            }
        }
    }
}

/// Keep failures visible and retryable; successful publication alone closes the dialog.
pub(super) fn finished(world: &mut World, result: Result<(), String>) {
    if !world.contains_resource::<Prompt>() {
        return;
    }
    match result {
        Err(error) => {
            let mut prompt = world.resource_mut::<Prompt>();
            prompt.busy = false;
            prompt.error = Some(error);
        }
        Ok(()) => {
            let overlay = {
                let mut prompt = world.resource_mut::<Prompt>();
                prompt.busy = false;
                prompt.guard = None;
                prompt.return_focus = None;
                prompt.overlay.take()
            };
            if let Some(overlay) = overlay {
                let _ = world.despawn(overlay);
            }
            world
                .resource_mut::<DocumentProtectionState>()
                .project_create_open = false;
            crate::asset_browser::reveal_created_project(world);
            if let Some(mut active) =
                world.get_resource_mut::<crate::editor_view::ActiveEditorContext>()
            {
                *active = default();
            }
            if let Some(mut maximized) = world.get_resource_mut::<crate::docking::MaximizedPanel>()
            {
                maximized.0 = None;
            }
            if let Some(mut layout) = world.get_resource_mut::<WorkspaceLayout>() {
                layout.show(ToolPanel::Timeline);
                layout.show(ToolPanel::ModuleStack);
                layout.show(ToolPanel::Assets);
            }
            if let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
                // The previous focus belongs to the old project, unlike Cancel.
                focus.clear();
            }
        }
    }
}

pub(super) fn creation_error(error: CreateProjectError, localizer: &Localizer) -> String {
    error_text(error, localizer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_switches_the_visible_browser_and_preview_through_normal_updates() {
        let parent = tempfile::tempdir().unwrap();
        let old_root = parent.path().join("old");
        fs::create_dir(&old_root).unwrap();
        let mut app = crate::asset_browser::tests::browser_app(&old_root);
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<WorkspaceLayout>();
        {
            let mut layout = app.world_mut().resource_mut::<WorkspaceLayout>();
            layout.show(ToolPanel::MaterialGraph);
            layout.show(ToolPanel::Diagnostics);
        }
        super::super::install_document_open_test_runtime(&mut app, parent.path().join("recovery"));
        register(&mut app);
        app.update();
        open_prompt(&mut app);
        set_field(&mut app, Field::Name, "New Project");
        choose(&mut app, Choice::Create);
        let deadline = Instant::now() + Duration::from_secs(15);
        while app.world().resource::<Prompt>().overlay.is_some() {
            app.update();
            assert!(
                Instant::now() < deadline,
                "project creation did not finish: {:?}",
                app.world().resource::<Prompt>().error
            );
            std::thread::yield_now();
        }
        for _ in 0..3 {
            app.update();
        }
        let layout = app.world().resource::<WorkspaceLayout>();
        assert!(layout.is_active(ToolPanel::Assets));
        assert!(layout.is_active(ToolPanel::Timeline));
        assert_eq!(
            app.world().resource::<ProjectEffectCatalog>().root(),
            parent
                .path()
                .join("New Project/assets")
                .canonicalize()
                .unwrap()
        );
        assert_eq!(
            app.world().resource::<EditorSession>().effect.name,
            "Untitled Effect"
        );
        let world = app.world_mut();
        let labels: Vec<_> = world
            .query::<&Text>()
            .iter(world)
            .map(|text| text.0.clone())
            .collect();
        // The browser displays the asset root's name, not its parent project folder.
        assert!(labels.iter().any(|label| label == "assets"));
        assert!(!labels.iter().any(|label| label == "old"));
        for folder in ["effects", "materials", "meshes", "shaders", "textures"] {
            assert!(
                labels.iter().any(|label| label == folder),
                "missing new project folder {folder}"
            );
        }
    }

    #[test]
    fn picked_folder_updates_the_live_editor_without_resetting_its_rendering_state() {
        let mut text = EditableText::new(r"C:\Old");
        text.editor_mut().set_scale(2.0);
        text.cursor_width = 0.3;
        text.visible_width = Some(32.0);
        update_location_text(&mut text, r"C:\Projects\Chosen folder");
        assert_eq!(text.value().to_string(), r"C:\Projects\Chosen folder");
        assert_eq!(text.editor().get_scale(), 2.0);
        assert_eq!(text.cursor_width, 0.3);
        assert_eq!(text.visible_width, Some(32.0));
        assert_eq!(text.pending_edits.len(), 1);
        assert!(matches!(text.pending_edits[0], TextEdit::TextStart(false)));
        update_location_text(&mut text, r"D:\Another folder");
        assert_eq!(text.value().to_string(), r"D:\Another folder");
        assert_eq!(text.editor().get_scale(), 2.0);
    }

    #[test]
    fn paths_hide_device_prefixes_without_changing_normal_or_network_paths() {
        assert_eq!(
            display_path(Path::new(r"\\?\C:\Projects\Demo")),
            r"C:\Projects\Demo"
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share\Demo")),
            r"\\server\share\Demo"
        );
        assert_eq!(
            display_path(Path::new(r"C:\Projects\Demo")),
            r"C:\Projects\Demo"
        );
    }

    #[test]
    fn dialog_has_visible_fields_inline_browse_and_quiet_initial_feedback() {
        let parent = tempfile::tempdir().unwrap();
        let mut app = app(parent.path());
        open_prompt(&mut app);
        let world = app.world_mut();
        assert!(
            world
                .query_filtered::<&Text, With<Feedback>>()
                .single(world)
                .unwrap()
                .0
                .is_empty()
        );
        let fields: Vec<_> = world
            .query::<(&Field, &Node, &ChildOf)>()
            .iter(world)
            .map(|(field, node, parent)| (field == &Field::Location, node.clone(), parent.parent()))
            .collect();
        assert_eq!(fields.len(), 2);
        for (_, node, _) in &fields {
            assert_eq!(node.height, Val::Px(40.0));
            assert_eq!(node.border, UiRect::all(Val::Px(1.0)));
        }
        let location_parent = fields.iter().find(|(location, _, _)| *location).unwrap().2;
        let browse_parent = world
            .query::<(&Choice, &ChildOf)>()
            .iter(world)
            .find_map(|(choice, parent)| (*choice == Choice::Browse).then_some(parent.parent()))
            .unwrap();
        assert_eq!(location_parent, browse_parent);
        assert!(!world.resource::<Prompt>().location.starts_with(r"\\?\"));

        // Browse/focus loss on another field must not flag an untouched blank name.
        set_field(&mut app, Field::Name, "");
        set_field(&mut app, Field::Location, &display_path(parent.path()));
        app.update();
        let world = app.world_mut();
        assert!(
            world
                .query_filtered::<&Text, With<Feedback>>()
                .single(world)
                .unwrap()
                .0
                .is_empty()
        );
        assert_eq!(
            world
                .query_filtered::<&Node, With<DestinationSummary>>()
                .single(world)
                .unwrap()
                .display,
            Display::None
        );
        assert!(
            !world
                .query::<&Text>()
                .iter(world)
                .any(|text| text.0.contains("fresh workspace")
                    || text.0.contains("also used")
                    || text.0.contains("Includes folders"))
        );

        set_field(&mut app, Field::Name, "Bad/name");
        app.update();
        let world = app.world_mut();
        assert!(
            !world
                .query_filtered::<&Text, With<Feedback>>()
                .single(world)
                .unwrap()
                .0
                .is_empty()
        );
        set_field(&mut app, Field::Name, "Demo");
        app.update();
        let world = app.world_mut();
        assert!(
            world
                .query_filtered::<&Text, With<Feedback>>()
                .single(world)
                .unwrap()
                .0
                .is_empty()
        );
        assert!(
            world
                .query_filtered::<&Text, With<Destination>>()
                .single(world)
                .unwrap()
                .0
                .ends_with("Demo")
        );
    }

    fn app(parent: &Path) -> App {
        let old = parent.join("old");
        fs::create_dir(&old).unwrap();
        let catalog = crate::project::catalog_for_folder(&old).unwrap();
        let mut session = crate::test_support::session_with_timing_slack();
        session.save_as(old.join("original.aestra.ron")).unwrap();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::scene::ScenePlugin,
            bevy::text::TextPlugin,
        ))
        .init_asset::<bevy_resvg::prelude::SvgFile>()
        .insert_resource(session)
        .insert_resource(catalog)
        .insert_resource(Localizer::new("en-US").unwrap())
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_observer(super::super::resolve_document_protection);
        crate::persistence::install_document_open_test_runtime(&mut app, parent.join("recovery"));
        register(&mut app);
        // Install the initial preview before capturing the modal's document/project guard.
        app.update();
        app
    }

    fn open_prompt(app: &mut App) {
        app.world_mut().trigger(DocumentAction::NewProject);
        app.world_mut().flush();
        app.update();
        assert!(app.world().resource::<Prompt>().overlay.is_some());
        assert!(app.world().resource::<DocumentProtectionState>().is_open());
    }

    fn set_field(app: &mut App, field: Field, value: &str) {
        let world = app.world_mut();
        let container = world
            .query::<(Entity, &Field)>()
            .iter(world)
            .find_map(|(entity, marker)| (*marker == field).then_some(entity))
            .unwrap();
        let input = world
            .query::<(Entity, &ChildOf)>()
            .iter(world)
            .find_map(|(entity, parent)| (parent.parent() == container).then_some(entity))
            .unwrap();
        *world.get_mut::<EditableText>(input).unwrap() = EditableText::new(value);
        world.trigger(ValueChange {
            source: container,
            value: value.to_owned(),
            is_final: false,
        });
        world.flush();
    }

    fn choose(app: &mut App, choice: Choice) {
        let world = app.world_mut();
        let entity = world
            .query::<(Entity, &Choice)>()
            .iter(world)
            .find_map(|(entity, actual)| (*actual == choice).then_some(entity))
            .unwrap();
        world.trigger(Activate { entity });
        world.flush();
    }

    fn assert_original(app: &App, effect: &aestra_core::EffectAsset, root: &Path) {
        assert_eq!(&app.world().resource::<EditorSession>().effect, effect);
        assert_eq!(app.world().resource::<ProjectEffectCatalog>().root(), root);
    }

    #[test]
    fn cancel_and_escape_do_not_write_or_switch_and_restore_focus() {
        for escape in [false, true] {
            let parent = tempfile::tempdir().unwrap();
            let mut app = app(parent.path());
            let original = app.world().resource::<EditorSession>().effect.clone();
            let root = app
                .world()
                .resource::<ProjectEffectCatalog>()
                .root()
                .to_owned();
            let previous = app.world_mut().spawn_empty().id();
            app.world_mut()
                .resource_mut::<InputFocus>()
                .set(previous, FocusCause::Navigated);
            open_prompt(&mut app);
            let focus = app.world().resource::<InputFocus>().get().unwrap();
            assert!(app.world().get::<EditableText>(focus).is_some());
            set_field(&mut app, Field::Name, "Cancelled");
            if escape {
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::Escape);
                app.update();
            } else {
                choose(&mut app, Choice::Cancel);
            }
            assert_original(&app, &original, &root);
            assert!(!parent.path().join("Cancelled").exists());
            assert!(app.world().resource::<Prompt>().overlay.is_none());
            assert!(!app.world().resource::<DocumentProtectionState>().is_open());
            assert_eq!(app.world().resource::<InputFocus>().get(), Some(previous));
        }
    }

    #[test]
    fn creation_opens_only_after_completion_and_rejects_double_submission() {
        let parent = tempfile::tempdir().unwrap();
        let mut app = app(parent.path());
        let original = app.world().resource::<EditorSession>().effect.clone();
        let root = app
            .world()
            .resource::<ProjectEffectCatalog>()
            .root()
            .to_owned();
        open_prompt(&mut app);
        set_field(&mut app, Field::Name, "My Project");
        app.update();
        assert!(
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .any(|text| text.0.contains("My Project"))
        );
        choose(&mut app, Choice::Create);
        choose(&mut app, Choice::Create);
        choose(&mut app, Choice::Cancel);
        assert!(
            app.world().resource::<Prompt>().busy,
            "{:?}",
            app.world().resource::<Prompt>().error
        );
        assert_original(&app, &original, &root);
        crate::project_content::io::drain(app.world_mut());
        let new_root = parent
            .path()
            .join("My Project/assets")
            .canonicalize()
            .unwrap();
        assert_eq!(
            app.world().resource::<ProjectEffectCatalog>().root(),
            new_root
        );
        assert!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .entries()
                .is_empty()
        );
        assert_ne!(
            app.world().resource::<EditorSession>().effect.id,
            original.id
        );
        assert!(
            app.world()
                .resource::<EditorSession>()
                .source_path
                .is_none()
        );
        assert!(app.world().resource::<Prompt>().overlay.is_none());
        assert!(!app.world().resource::<DocumentProtectionState>().is_open());
    }

    #[test]
    fn invalid_input_and_collision_preserve_the_workspace_and_allow_retry() {
        let parent = tempfile::tempdir().unwrap();
        let mut app = app(parent.path());
        let original = app.world().resource::<EditorSession>().effect.clone();
        let root = app
            .world()
            .resource::<ProjectEffectCatalog>()
            .root()
            .to_owned();
        open_prompt(&mut app);
        set_field(&mut app, Field::Name, "../escape");
        choose(&mut app, Choice::Create);
        assert!(!app.world().resource::<Prompt>().busy);
        assert!(app.world().resource::<Prompt>().error.is_some());
        set_field(&mut app, Field::Name, "old");
        choose(&mut app, Choice::Create);
        crate::project_content::io::drain(app.world_mut());
        assert_original(&app, &original, &root);
        assert!(
            app.world()
                .resource::<Prompt>()
                .error
                .as_ref()
                .unwrap()
                .contains("already exists")
        );
        set_field(&mut app, Field::Name, "Good");
        choose(&mut app, Choice::Create);
        crate::project_content::io::drain(app.world_mut());
        assert_eq!(
            app.world().resource::<ProjectEffectCatalog>().root(),
            parent.path().join("Good/assets").canonicalize().unwrap()
        );
    }

    #[test]
    fn dirty_work_requires_explicit_approval_before_showing_creation() {
        let parent = tempfile::tempdir().unwrap();
        let mut app = app(parent.path());
        app.world_mut()
            .resource_mut::<EditorSession>()
            .adjust_effect_duration(0.1);
        let original = app.world().resource::<EditorSession>().effect.clone();
        app.world_mut().trigger(DocumentAction::NewProject);
        app.world_mut().flush();
        assert!(app.world().resource::<Prompt>().overlay.is_none());
        assert_eq!(
            app.world().resource::<DocumentProtectionState>().pending,
            Some(DocumentAction::NewProject)
        );
        let cancel = app.world_mut().spawn(DocumentProtectionAction::Cancel).id();
        app.world_mut().trigger(Activate { entity: cancel });
        app.world_mut().flush();
        assert_eq!(app.world().resource::<EditorSession>().effect, original);
        assert!(app.world().resource::<EditorSession>().dirty);
        assert!(!app.world().resource::<DocumentProtectionState>().is_open());
        app.world_mut().trigger(DocumentAction::NewProject);
        app.world_mut().flush();
        let discard = app
            .world_mut()
            .spawn(DocumentProtectionAction::Discard)
            .id();
        app.world_mut().trigger(Activate { entity: discard });
        app.world_mut().flush();
        assert!(app.world().resource::<Prompt>().overlay.is_some());
        // Approval alone doesn't destroy the old draft; cancelling creation keeps it.
        choose(&mut app, Choice::Cancel);
        assert_eq!(app.world().resource::<EditorSession>().effect, original);
        assert!(app.world().resource::<EditorSession>().dirty);
    }

    #[test]
    fn changed_document_during_io_is_not_replaced_and_created_project_is_reported() {
        let parent = tempfile::tempdir().unwrap();
        let mut app = app(parent.path());
        let root = app
            .world()
            .resource::<ProjectEffectCatalog>()
            .root()
            .to_owned();
        open_prompt(&mut app);
        set_field(&mut app, Field::Name, "Created");
        choose(&mut app, Choice::Create);
        let mut completion = crate::project_content::io::prepared_completion(app.world_mut());
        app.world_mut()
            .resource_mut::<EditorSession>()
            .adjust_effect_duration(0.1);
        let changed = app.world().resource::<EditorSession>().effect.clone();
        completion.apply(app.world_mut());
        app.world_mut().flush();
        assert_original(&app, &changed, &root);
        assert!(parent.path().join("Created/assets/effects").is_dir());
        let prompt = app.world().resource::<Prompt>();
        assert!(!prompt.busy);
        assert!(
            prompt
                .error
                .as_ref()
                .unwrap()
                .contains("project was created")
        );
        choose(&mut app, Choice::Cancel);
        assert_original(&app, &changed, &root);
    }

    #[test]
    fn save_then_create_saves_the_old_effect_before_opening_the_prompt() {
        let parent = tempfile::tempdir().unwrap();
        let mut app = app(parent.path());
        app.world_mut()
            .resource_mut::<EditorSession>()
            .adjust_effect_duration(0.1);
        let changed = app.world().resource::<EditorSession>().effect.clone();
        let source = app
            .world()
            .resource::<EditorSession>()
            .source_path
            .clone()
            .unwrap();
        app.world_mut().trigger(DocumentAction::NewProject);
        app.world_mut().flush();
        let save = app.world_mut().spawn(DocumentProtectionAction::Save).id();
        app.world_mut().trigger(Activate { entity: save });
        app.world_mut().flush();
        assert!(app.world().resource::<Prompt>().overlay.is_none());
        crate::project_content::io::drain(app.world_mut());
        assert!(app.world().resource::<Prompt>().overlay.is_some());
        assert_eq!(
            aestra_core::EffectAsset::load_ron(&source).unwrap(),
            changed
        );
        choose(&mut app, Choice::Cancel);
        assert_eq!(app.world().resource::<EditorSession>().effect, changed);
        assert!(!app.world().resource::<EditorSession>().dirty);
    }

    #[test]
    fn rejected_queued_creation_releases_the_dialog_without_writing() {
        use bevy::ecs::system::RunSystemOnce;
        let parent = tempfile::tempdir().unwrap();
        let mut app = app(parent.path());
        open_prompt(&mut app);
        app.world_mut().resource_mut::<Prompt>().busy = true;
        let original = app.world().resource::<EditorSession>().effect.clone();
        let destination_parent = parent.path().to_owned();
        app.world_mut()
            .run_system_once(
                move |mut commands: Commands,
                      session: Res<EditorSession>,
                      settings: Res<EditorSettings>,
                      catalog: Res<ProjectEffectCatalog>,
                      localizer: Res<Localizer>| {
                    // Capture the plan against the current document, then reject it when
                    // the earlier queued edit applies before the I/O command starts.
                    commands.queue(|world: &mut World| {
                        world
                            .resource_mut::<EditorSession>()
                            .adjust_effect_duration(0.1);
                    });
                    background::queue_project_creation(
                        &mut commands,
                        &session,
                        &settings,
                        &catalog,
                        &localizer,
                        CreateProjectRequest {
                            name: "Rejected".into(),
                            parent: destination_parent.clone(),
                        },
                    );
                },
            )
            .unwrap();
        let world = app.world_mut();
        assert!(!world.resource::<Prompt>().busy);
        assert!(
            world
                .resource::<Prompt>()
                .error
                .as_ref()
                .unwrap()
                .contains("Creation was cancelled")
        );
        assert!(!parent.path().join("Rejected").exists());
        assert!(crate::project_content::io::idle_world(world));
        assert_ne!(world.resource::<EditorSession>().effect, original);
    }
}
