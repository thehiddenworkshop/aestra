//! Explicit project choice on a fresh launch and from File → Recent Projects.
use super::*;
use bevy::feathers::theme::ThemedText;
use bevy::input_focus::tab_navigation::TabGroup;

#[derive(Resource)]
pub(crate) struct ProjectLauncherState {
    pending_startup: bool,
    overlay: Option<Entity>,
}

impl ProjectLauncherState {
    pub(crate) fn new(pending_startup: bool) -> Self {
        Self {
            pending_startup,
            overlay: None,
        }
    }
}

impl Default for ProjectLauncherState {
    fn default() -> Self {
        Self::new(false)
    }
}

pub(crate) fn project_opened(world: &mut World) {
    if let Some(mut state) = world.get_resource_mut::<ProjectLauncherState>() {
        state.pending_startup = false;
    }
}

#[derive(Event)]
pub(crate) struct OpenProjectLauncher;

#[derive(Component, Clone, Copy)]
enum Choice {
    New,
    Open,
    Examples,
    Recent(usize),
    Close,
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<ProjectLauncherState>()
        .add_observer(open)
        .add_observer(choose)
        .add_systems(
            Update,
            (show_on_first_launch, escape).in_set(PersistenceSet::Actions),
        );
}

fn show_on_first_launch(
    mut commands: Commands,
    mut state: ResMut<ProjectLauncherState>,
    protection: Res<DocumentProtectionState>,
    session: Res<EditorSession>,
    io_tasks: Option<Res<crate::project_content::io::ProjectIoTasks>>,
) {
    if !state.pending_startup
        || protection.is_open()
        || state.overlay.is_some()
        || !crate::project_content::io::idle(io_tasks)
    {
        return;
    }
    // Recovery may have restored a document while startup was waiting for its choice.
    if session.source_path.is_some() || (session.dirty && !session.is_untouched_starter()) {
        state.pending_startup = false;
        return;
    }
    commands.trigger(OpenProjectLauncher);
}

fn open(
    _: On<OpenProjectLauncher>,
    mut commands: Commands,
    mut state: ResMut<ProjectLauncherState>,
    mut protection: ResMut<DocumentProtectionState>,
    settings: Res<EditorSettings>,
    localizer: Res<Localizer>,
) {
    if state.overlay.is_some() || protection.is_open() {
        return;
    }
    let overlay = commands
        .spawn((
            TabGroup::modal(),
            crate::feathers::node_graph::FeathersGraphNavigationBlocker,
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
            BackgroundColor(theme::APP_BG),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: Val::Px(1080.0),
                        max_width: Val::Percent(94.0),
                        height: Val::Px(650.0),
                        max_height: Val::Percent(90.0),
                        padding: UiRect::all(Val::Px(24.0)),
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
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            padding: UiRect::bottom(Val::Px(14.0)),
                            border: UiRect::bottom(Val::Px(1.0)),
                            ..default()
                        })
                        .insert(BorderColor::all(theme::BORDER_BRIGHT))
                        .with_children(|header| {
                            header
                                .spawn(Node {
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(5.0),
                                    ..default()
                                })
                                .with_children(|titles| {
                                    label(titles, "AESTRA".into(), 12.0, theme::ACCENT);
                                    label(
                                        titles,
                                        localizer.text("project-launcher-title"),
                                        28.0,
                                        theme::TEXT,
                                    );
                                });
                            if !state.pending_startup {
                                button(
                                    header,
                                    localizer.text("transaction-cancel"),
                                    Choice::Close,
                                    false,
                                );
                            }
                        });
                    panel
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            min_height: Val::Px(0.0),
                            flex_grow: 1.0,
                            column_gap: Val::Px(20.0),
                            ..default()
                        })
                        .with_children(|body| {
                            body.spawn(Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                height: Val::Percent(100.0),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(10.0),
                                ..default()
                            })
                            .with_children(|projects| {
                                label(
                                    projects,
                                    localizer.text("project-launcher-recent"),
                                    15.0,
                                    theme::TEXT,
                                );
                                projects
                                    .spawn(Node {
                                        width: Val::Percent(100.0),
                                        min_height: Val::Px(0.0),
                                        flex_grow: 1.0,
                                        flex_direction: FlexDirection::Column,
                                        row_gap: Val::Px(8.0),
                                        overflow: Overflow::scroll_y(),
                                        ..default()
                                    })
                                    .with_children(|recent| {
                                        if settings.general.recent_projects.is_empty() {
                                            recent
                                                .spawn((
                                                    Node {
                                                        width: Val::Percent(100.0),
                                                        min_height: Val::Px(125.0),
                                                        align_items: AlignItems::Center,
                                                        justify_content: JustifyContent::Center,
                                                        border: UiRect::all(Val::Px(1.0)),
                                                        ..default()
                                                    },
                                                    BackgroundColor(theme::PANEL_DARK),
                                                    BorderColor::all(theme::BORDER_BRIGHT),
                                                ))
                                                .with_children(|empty| {
                                                    label(
                                                        empty,
                                                        localizer
                                                            .text("project-launcher-no-recent"),
                                                        14.0,
                                                        theme::TEXT_FAINT,
                                                    );
                                                });
                                        }
                                        for (index, path) in
                                            settings.general.recent_projects.iter().enumerate()
                                        {
                                            recent_project(
                                                recent,
                                                index,
                                                path,
                                                settings.general.active_project.as_deref()
                                                    == Some(path.as_path()),
                                                &localizer,
                                            );
                                        }
                                    });
                            });
                            body.spawn((
                                Node {
                                    width: Val::Px(285.0),
                                    flex_shrink: 0.0,
                                    height: Val::Percent(100.0),
                                    padding: UiRect::all(Val::Px(18.0)),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(6.0)),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(12.0),
                                    ..default()
                                },
                                BackgroundColor(theme::PANEL_DARK),
                                BorderColor::all(theme::BORDER_BRIGHT),
                            ))
                            .with_children(|actions| {
                                label(
                                    actions,
                                    localizer.text("project-launcher-start"),
                                    15.0,
                                    theme::TEXT,
                                );
                                button(
                                    actions,
                                    localizer.text("file-new-project"),
                                    Choice::New,
                                    true,
                                );
                                button(
                                    actions,
                                    localizer.text("file-open-project"),
                                    Choice::Open,
                                    false,
                                );
                                actions.spawn(Node {
                                    flex_grow: 1.0,
                                    ..default()
                                });
                                label(
                                    actions,
                                    localizer.text("project-launcher-examples-heading"),
                                    15.0,
                                    theme::TEXT,
                                );
                                button(
                                    actions,
                                    localizer.text("project-launcher-examples"),
                                    Choice::Examples,
                                    false,
                                );
                            });
                        });
                    panel
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(1.0),
                            ..default()
                        })
                        .insert(BackgroundColor(theme::BORDER_BRIGHT));
                });
        })
        .id();
    state.overlay = Some(overlay);
    protection.project_launcher_open = true;
}

fn label(parent: &mut ChildSpawnerCommands, value: String, size: f32, color: Color) {
    parent.spawn((
        Text::new(value),
        TextFont {
            font_size: size.into(),
            ..default()
        },
        TextColor(color),
        Pickable::IGNORE,
    ));
}

fn recent_project(
    parent: &mut ChildSpawnerCommands,
    index: usize,
    path: &Path,
    current: bool,
    localizer: &Localizer,
) {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let available = path.is_dir();
    let mut row = parent.spawn_empty();
    if available {
        row.apply_scene(crate::feathers::scenes::feathers_button());
    }
    row.insert((
        Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(72.0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: Val::Px(14.0),
            padding: UiRect::all(Val::Px(12.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        BackgroundColor(theme::PANEL_DARK),
        BorderColor::all(if current {
            theme::ACCENT_DIM
        } else {
            theme::BORDER_BRIGHT
        }),
    ));
    if available {
        row.insert((
            Choice::Recent(index),
            FeathersActionButton,
            AccessibleLabel(format!("{name}, {}", new_project::display_path(path))),
        ));
    }
    row.with_children(|row| {
        row.spawn((
            Node {
                width: Val::Px(40.0),
                height: Val::Px(40.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            BackgroundColor(if available {
                theme::ACCENT_DIM
            } else {
                theme::PANEL
            }),
        ))
        .with_children(|tile| {
            label(
                tile,
                name.chars()
                    .next()
                    .unwrap_or('P')
                    .to_uppercase()
                    .to_string(),
                20.0,
                if available {
                    theme::TEXT
                } else {
                    theme::TEXT_FAINT
                },
            );
        });
        row.spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            min_width: Val::Px(0.0),
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|text| {
            label(
                text,
                name,
                15.0,
                if available {
                    theme::TEXT
                } else {
                    theme::TEXT_FAINT
                },
            );
            label(
                text,
                new_project::display_path(path),
                11.0,
                theme::TEXT_FAINT,
            );
        });
        if !available {
            label(
                row,
                localizer.text("project-launcher-missing"),
                11.0,
                theme::TEXT_FAINT,
            );
        } else if current {
            label(
                row,
                localizer.text("project-launcher-current"),
                11.0,
                theme::ACCENT,
            );
        }
    });
}

fn button(parent: &mut ChildSpawnerCommands, value: String, choice: Choice, primary: bool) {
    let mut item = parent.spawn_empty();
    if primary {
        item.apply_scene(crate::feathers::scenes::feathers_primary_button());
    } else {
        item.apply_scene(crate::feathers::scenes::feathers_button());
    }
    item.insert((
        choice,
        FeathersActionButton,
        AccessibleLabel(value.clone()),
        Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(40.0),
            min_width: Val::Px(145.0),
            padding: UiRect::horizontal(Val::Px(14.0)),
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(Val::Px(4.0)),
            ..default()
        },
    ))
    .with_children(|item| {
        item.spawn((Text::new(value), ThemedText, Pickable::IGNORE));
    });
}

fn close(
    commands: &mut Commands,
    state: &mut ProjectLauncherState,
    protection: &mut DocumentProtectionState,
) {
    if let Some(overlay) = state.overlay.take() {
        commands.entity(overlay).try_despawn();
    }
    protection.project_launcher_open = false;
}

fn choose(
    event: On<Activate>,
    choices: Query<&Choice>,
    mut commands: Commands,
    mut state: ResMut<ProjectLauncherState>,
    mut protection: ResMut<DocumentProtectionState>,
) {
    let Ok(choice) = choices.get(event.entity) else {
        return;
    };
    if state.overlay.is_none() {
        return;
    }
    if matches!(choice, Choice::Close) && state.pending_startup {
        return;
    }
    let action = match choice {
        Choice::New => Some(DocumentAction::NewProject),
        Choice::Open => Some(DocumentAction::OpenProject),
        Choice::Examples => Some(DocumentAction::ExploreExamples),
        Choice::Recent(index) => Some(DocumentAction::OpenRecentProject(*index)),
        Choice::Close => None,
    };
    close(&mut commands, &mut state, &mut protection);
    if let Some(action) = action {
        commands.trigger(action);
    }
}

fn escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut state: ResMut<ProjectLauncherState>,
    mut protection: ResMut<DocumentProtectionState>,
) {
    if keys.just_pressed(KeyCode::Escape) && state.overlay.is_some() && !state.pending_startup {
        close(&mut commands, &mut state, &mut protection);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_launch_is_a_large_browser_with_explicit_actions_and_nonselectable_missing_rows() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::scene::ScenePlugin,
        ));
        app.init_asset::<Font>();
        let existing = tempfile::tempdir().unwrap();
        let missing = existing.path().join("missing");
        let mut settings = EditorSettings::default();
        settings.general.recent_projects = vec![existing.path().to_owned(), missing];
        app.insert_resource(settings)
            .insert_resource(Localizer::new("en-US").unwrap())
            .insert_resource(ProjectLauncherState::new(true))
            .init_resource::<DocumentProtectionState>()
            .add_observer(open);

        app.world_mut().trigger(OpenProjectLauncher);
        app.world_mut().flush();

        let state = app.world().resource::<ProjectLauncherState>();
        assert!(state.overlay.is_some());
        assert!(
            app.world()
                .resource::<DocumentProtectionState>()
                .project_launcher_open
        );
        let choices: Vec<_> = app
            .world_mut()
            .query::<&Choice>()
            .iter(app.world())
            .copied()
            .collect();
        assert_eq!(choices.len(), 4);
        assert!(choices.iter().any(|choice| matches!(choice, Choice::New)));
        assert!(choices.iter().any(|choice| matches!(choice, Choice::Open)));
        assert!(
            choices
                .iter()
                .any(|choice| matches!(choice, Choice::Examples))
        );
        assert!(
            choices
                .iter()
                .any(|choice| matches!(choice, Choice::Recent(0)))
        );
    }
}
