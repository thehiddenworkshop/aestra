//! Folder creation goes through the project operation planner and serialized project I/O.
use super::AssetBrowserState;
use crate::project_content::io::{self, IoGuard};
use crate::*;
use aestra_project::{
    ProjectContentVersion, ProjectSourceId, content::operations::OperationRequest,
};
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::ui_widgets::{Activate, ValueChange};

#[derive(Event)]
pub(super) struct OpenFolderPrompt;
#[derive(Component)]
struct NameField;
#[derive(Component, Clone, Copy)]
enum Choice {
    Create,
    Cancel,
}
#[derive(Resource, Default)]
struct Prompt {
    target: Option<(ProjectSourceId, ProjectContentVersion)>,
    overlay: Option<Entity>,
    name: String,
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<Prompt>()
        .add_observer(open)
        .add_observer(change)
        .add_observer(choose)
        .add_systems(Update, escape);
}
fn open(
    _: On<OpenFolderPrompt>,
    mut commands: Commands,
    mut prompt: ResMut<Prompt>,
    state: Res<AssetBrowserState>,
    catalog: Res<ProjectEffectCatalog>,
    localizer: Res<Localizer>,
) {
    if prompt.overlay.is_some() {
        return;
    }
    prompt.target = Some((
        state.folder_id(catalog.content()),
        catalog.content_revision(),
    ));
    prompt.name.clear();
    let overlay = commands
        .spawn((
            TabGroup::modal(),
            super::panel::BrowserSurface,
            crate::feathers::node_graph::FeathersGraphNavigationBlocker,
            RelativeCursorPosition::default(),
            GlobalZIndex(320),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: Val::Px(400.0),
                        max_width: Val::Percent(95.0),
                        padding: UiRect::all(Val::Px(16.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(12.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new(localizer.text("browser-new-folder")),
                        TextColor(theme::TEXT),
                        TextFont {
                            font_size: 16.0.into(),
                            ..default()
                        },
                    ));
                    crate::feathers::text_input::spawn_text_input(
                        panel,
                        "",
                        &localizer.text("browser-folder-name"),
                        NameField,
                    );
                    crate::feathers::button::spawn_action_button(
                        panel,
                        &localizer.text("browser-folder-create"),
                        Choice::Create,
                        false,
                    );
                    crate::feathers::button::spawn_action_button(
                        panel,
                        &localizer.text("browser-folder-cancel"),
                        Choice::Cancel,
                        false,
                    );
                });
        })
        .id();
    prompt.overlay = Some(overlay);
}
fn change(
    event: On<ValueChange<String>>,
    fields: Query<(), With<NameField>>,
    mut prompt: ResMut<Prompt>,
) {
    if fields.contains(event.source) {
        prompt.name.clone_from(&event.value);
    }
}
fn close(commands: &mut Commands, prompt: &mut Prompt) {
    if let Some(entity) = prompt.overlay.take() {
        commands.entity(entity).try_despawn();
    }
    prompt.target = None;
}
fn escape(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut prompt: ResMut<Prompt>,
    mut commands: Commands,
) {
    if keys.is_some_and(|keys| keys.just_pressed(KeyCode::Escape)) {
        close(&mut commands, &mut prompt);
    }
}
fn choose(
    event: On<Activate>,
    choices: Query<&Choice>,
    mut prompt: ResMut<Prompt>,
    mut commands: Commands,
    catalog: Res<ProjectEffectCatalog>,
    mut session: ResMut<EditorSession>,
    tasks: Option<Res<io::ProjectIoTasks>>,
) {
    let Ok(choice) = choices.get(event.entity) else {
        return;
    };
    if matches!(choice, Choice::Cancel) {
        close(&mut commands, &mut prompt);
        return;
    }
    if !io::idle(tasks) {
        return;
    }
    let Some((parent, version)) = prompt.target else {
        return;
    };
    if version != catalog.content_revision() {
        session.status = "Project changed; reopen New Folder".into();
        close(&mut commands, &mut prompt);
        return;
    }
    let request = OperationRequest::CreateFolder {
        parent,
        name: prompt.name.clone(),
    };
    let guard = IoGuard::capture(&catalog, &session);
    let mut prepared = catalog.clone();
    io::enqueue(&mut commands, guard.clone(), move || {
        prepared.refresh();
        let result = prepared
            .content()
            .plan_operation(request)
            .and_then(|plan| plan.apply());
        prepared.refresh();
        io::completion(move |world| {
            if !guard.same_project(world.resource::<ProjectEffectCatalog>()) {
                return;
            }
            let status = match &result {
                Ok(result) => format!("Created folder: {}", result.created_directory.display()),
                Err(error) => format!("Folder creation failed: {error}"),
            };
            io::publish_catalog(world, prepared);
            world.resource_mut::<EditorSession>().status = status;
        })
    });
    close(&mut commands, &mut prompt);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn create_and_cancel_use_scoped_project_io() {
        for cancel in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let catalog = ProjectEffectCatalog::scan(directory.path());
            let parent = catalog.content().source_tree().root();
            let version = catalog.content_revision();
            let session = crate::test_support::session_with_timing_slack();
            let effect = session.effect.clone();
            let mut app = App::new();
            app.insert_resource(catalog)
                .insert_resource(session)
                .insert_resource(Localizer::new("en-US").unwrap())
                .insert_resource(Prompt {
                    target: Some((parent, version)),
                    overlay: None,
                    name: "Textures".into(),
                })
                .add_observer(choose);
            let action = app
                .world_mut()
                .spawn(if cancel {
                    Choice::Cancel
                } else {
                    Choice::Create
                })
                .id();
            app.world_mut().trigger(Activate { entity: action });
            io::drain(app.world_mut());
            assert_eq!(directory.path().join("Textures").is_dir(), !cancel);
            assert_eq!(app.world().resource::<EditorSession>().effect, effect);
            assert!(app.world().resource::<Prompt>().target.is_none());
        }
    }
}
