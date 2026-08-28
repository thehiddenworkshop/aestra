//! Library workspace, project-effect catalog, and panel-local authoring actions.

use crate::*;
use bevy::ui_widgets::Activate;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

const DEFAULT_PROJECT_EFFECT_ROOT: &str = "assets/effects";

pub(crate) struct EditorLibraryPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LibrarySet {
    Input,
    Actions,
    Sync,
}

impl Plugin for EditorLibraryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProjectEffectCatalog>()
            .init_resource::<LibraryState>()
            .add_observer(queue_library_action_activation)
            .add_observer(execute_library_action)
            .add_observer(update_library_query)
            .add_systems(
                Update,
                (
                    library_keyboard_input.in_set(LibrarySet::Input),
                    handle_library_action_buttons.in_set(LibrarySet::Actions),
                ),
            )
            .add_systems(
                Update,
                (update_layer_selection, sync_library_filtering).in_set(LibrarySet::Sync),
            );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ProjectEffectEntryId(u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProjectEffectStatus {
    Valid,
    Invalid { message: String },
    Unsupported { found: u32, current: u32 },
}

impl ProjectEffectStatus {
    fn is_openable(&self) -> bool {
        matches!(self, Self::Valid)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectEffectEntry {
    pub(crate) id: ProjectEffectEntryId,
    display_name: String,
    path: PathBuf,
    status: ProjectEffectStatus,
}

#[derive(Resource)]
pub(crate) struct ProjectEffectCatalog {
    entries: Vec<ProjectEffectEntry>,
}

impl Default for ProjectEffectCatalog {
    fn default() -> Self {
        Self::scan(DEFAULT_PROJECT_EFFECT_ROOT)
    }
}

impl ProjectEffectCatalog {
    pub(crate) fn scan(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        let mut paths = fs::read_dir(root)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "ron"))
            .collect::<Vec<_>>();
        paths.sort();

        let mut used_ids = HashSet::new();
        let mut entries = paths
            .into_iter()
            .map(|path| {
                let mut id = stable_entry_id(root, &path);
                while !used_ids.insert(id) {
                    id.0 = id.0.wrapping_add(1);
                }
                let fallback_name = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("Unnamed effect")
                    .trim_end_matches(".aestra")
                    .replace(['_', '-'], " ");
                let (display_name, status) = match EffectAsset::load_ron(&path) {
                    Ok(effect) => (effect.name, ProjectEffectStatus::Valid),
                    Err(aestra_bevy::AssetError::UnsupportedFormat { found, current }) => (
                        fallback_name,
                        ProjectEffectStatus::Unsupported { found, current },
                    ),
                    Err(error) => (
                        fallback_name,
                        ProjectEffectStatus::Invalid {
                            message: error.to_string(),
                        },
                    ),
                };
                ProjectEffectEntry {
                    id,
                    display_name,
                    path,
                    status,
                }
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.display_name
                .to_lowercase()
                .cmp(&right.display_name.to_lowercase())
                .then_with(|| left.path.cmp(&right.path))
        });
        Self { entries }
    }

    pub(crate) fn entries(&self) -> &[ProjectEffectEntry] {
        &self.entries
    }

    pub(crate) fn entry(&self, id: ProjectEffectEntryId) -> Option<&ProjectEffectEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub(crate) fn openable_path(&self, id: ProjectEffectEntryId) -> Option<&Path> {
        self.entry(id)
            .filter(|entry| entry.status.is_openable())
            .map(|entry| entry.path.as_path())
    }
}

fn stable_entry_id(root: &Path, path: &Path) -> ProjectEffectEntryId {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let normalized = relative.to_string_lossy().replace('\\', "/").to_lowercase();
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in normalized.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    ProjectEffectEntryId(hash)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "filter controls are introduced in Library Slice 2"
    )
)]
pub(crate) enum LibraryOriginFilter {
    #[default]
    All,
    Project,
    CurrentDocument,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "filter controls are introduced in Library Slice 2"
    )
)]
pub(crate) enum LibraryKindFilter {
    #[default]
    All,
    Effect,
    Texture,
    Mesh,
    Material,
    Flipbook,
}

#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LibraryState {
    pub(crate) query: String,
    pub(crate) origin: LibraryOriginFilter,
    pub(crate) kind: LibraryKindFilter,
}

impl LibraryState {
    fn matches_project_effect(&self, entry: &ProjectEffectEntry) -> bool {
        if self.origin == LibraryOriginFilter::CurrentDocument
            || !matches!(
                self.kind,
                LibraryKindFilter::All | LibraryKindFilter::Effect
            )
        {
            return false;
        }
        let query = self.query.trim().to_lowercase();
        query.is_empty()
            || entry.display_name.to_lowercase().contains(&query)
            || entry.path.to_string_lossy().to_lowercase().contains(&query)
    }
}

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryAction {
    AddSpriteMaterial,
    AddGridFlipbook,
    AddEmitter,
    DuplicateEmitter,
    DeleteEmitter,
    SelectLayer(usize),
}

#[derive(Component)]
struct LayerRow(usize);

#[derive(Component)]
struct AssetButtonLabel;

#[derive(Component)]
struct LibrarySearchInput;

#[derive(Component)]
struct ProjectEffectRow(ProjectEffectEntryId);

#[derive(Component)]
struct LibraryProjectCount;

#[derive(Component)]
struct LibraryCatalogEmpty;

#[derive(Component)]
struct LibraryNoResults;

fn queue_library_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<LibraryAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

fn update_library_query(
    change: On<ValueChange<String>>,
    inputs: Query<(), With<LibrarySearchInput>>,
    mut state: ResMut<LibraryState>,
) {
    if !inputs.contains(change.source) || state.query == change.value {
        return;
    }
    state.query.clone_from(&change.value);
}

fn sync_library_filtering(
    state: Res<LibraryState>,
    catalog: Res<ProjectEffectCatalog>,
    mut nodes: Query<(
        &mut Node,
        Option<&ProjectEffectRow>,
        Has<LibraryCatalogEmpty>,
        Has<LibraryNoResults>,
    )>,
    mut counts: Query<&mut Text, With<LibraryProjectCount>>,
    localizer: Res<Localizer>,
) {
    if !state.is_changed() && !catalog.is_changed() {
        return;
    }
    let visible_count = catalog
        .entries()
        .iter()
        .filter(|entry| state.matches_project_effect(entry))
        .count();
    for (mut node, row, catalog_empty, no_results) in &mut nodes {
        if let Some(row) = row {
            node.display = if catalog
                .entry(row.0)
                .is_some_and(|entry| state.matches_project_effect(entry))
            {
                Display::Flex
            } else {
                Display::None
            };
        } else if catalog_empty {
            node.display = if catalog.entries().is_empty() {
                Display::Flex
            } else {
                Display::None
            };
        } else if no_results {
            node.display = if !catalog.entries().is_empty() && visible_count == 0 {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
    let mut args = FluentArgs::new();
    args.set("count", visible_count);
    let text = localizer.text_with("assets-found", &args);
    for mut count in &mut counts {
        count.0.clone_from(&text);
    }
}

pub(crate) fn spawn_library(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    state: &LibraryState,
    localizer: &Localizer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel_heading(
                panel,
                &localizer.text("assets-current-effect"),
                &localizer.text(if session.dirty {
                    "assets-modified"
                } else {
                    "assets-saved"
                }),
            );
            panel
                .spawn((
                    Node {
                        margin: UiRect::all(Val::Px(10.0)),
                        padding: UiRect::all(Val::Px(10.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(5.0)),
                        ..default()
                    },
                    BackgroundColor(theme::SELECTION),
                    BorderColor::all(theme::ACCENT_DIM),
                ))
                .with_children(|asset| {
                    asset.spawn((
                        Text::new(&session.effect.name),
                        TextFont {
                            font_size: FontSize::Px(13.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                    ));
                    asset.spawn((
                        Text::new(session.effect.id.to_string()),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::ACCENT),
                    ));
                });

            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::all(Val::Px(8.0)),
                    ..default()
                })
                .with_children(|search| {
                    spawn_search_field(
                        search,
                        &state.query,
                        &localizer.text("library-search-placeholder"),
                        &localizer.text("library-search-clear"),
                        LibrarySearchInput,
                    );
                });

            let mut args = FluentArgs::new();
            let visible_count = catalog
                .entries()
                .iter()
                .filter(|entry| state.matches_project_effect(entry))
                .count();
            args.set("count", visible_count);
            let section = spawn_list_section_header(
                panel,
                &localizer.text("assets-project-effects"),
                &localizer.text_with("assets-found", &args),
            );
            panel
                .commands()
                .entity(section.meta)
                .insert(LibraryProjectCount);
            for entry in catalog.entries() {
                let secondary = entry.path.display().to_string();
                let row = match &entry.status {
                    ProjectEffectStatus::Valid => spawn_action_list_row(
                        panel,
                        &entry.display_name,
                        Some(&secondary),
                        None,
                        &entry.display_name,
                        DocumentAction::OpenCatalog(entry.id),
                    ),
                    ProjectEffectStatus::Invalid { .. } => spawn_status_list_row(
                        panel,
                        &entry.display_name,
                        Some(&secondary),
                        ListRowStatus {
                            label: &localizer.text("library-status-invalid"),
                            color: theme::ACCENT,
                        },
                    ),
                    ProjectEffectStatus::Unsupported { .. } => spawn_status_list_row(
                        panel,
                        &entry.display_name,
                        Some(&secondary),
                        ListRowStatus {
                            label: &localizer.text("library-status-unsupported"),
                            color: theme::ACCENT,
                        },
                    ),
                };
                panel
                    .commands()
                    .entity(row)
                    .insert(ProjectEffectRow(entry.id));
            }
            let catalog_empty = spawn_list_empty_state(
                panel,
                &localizer.text("library-empty-title"),
                &localizer.text("library-empty-message"),
                theme::TEXT_MUTED,
            );
            panel
                .commands()
                .entity(catalog_empty)
                .insert(LibraryCatalogEmpty);
            let no_results = spawn_list_empty_state(
                panel,
                &localizer.text("library-no-results-title"),
                &localizer.text("library-no-results-message"),
                theme::TEXT_MUTED,
            );
            panel.commands().entity(no_results).insert(LibraryNoResults);

            args.set("count", session.effect.assets.len());
            panel_heading(
                panel,
                &localizer.text("assets-render-assets"),
                &localizer.text_with("assets-registered", &args),
            );
            if session.effect.assets.is_empty() {
                panel.spawn((
                    Text::new(localizer.text("assets-no-render-assets")),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_FAINT),
                    Node {
                        margin: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                        ..default()
                    },
                ));
            }
            for asset in &session.effect.assets {
                panel
                    .spawn(Node {
                        min_height: Val::Px(38.0),
                        margin: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                        padding: UiRect::axes(Val::Px(9.0), Val::Px(5.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new(format!("{:?}  {}", asset.kind, asset.name)),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(theme::TEXT),
                        ));
                        row.spawn((
                            Text::new(&asset.path),
                            TextFont {
                                font_size: FontSize::Px(8.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_FAINT),
                        ));
                    });
            }

            args.set("count", session.effect.materials.len());
            panel_heading(
                panel,
                &localizer.text("assets-materials"),
                &localizer.text_with("assets-registered", &args),
            );
            library_toolbar_button(
                panel,
                &localizer.text("assets-add-sprite-material"),
                LibraryAction::AddSpriteMaterial,
            );
            for material in &session.effect.materials {
                panel
                    .spawn(Node {
                        min_height: Val::Px(38.0),
                        margin: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                        padding: UiRect::axes(Val::Px(9.0), Val::Px(5.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new(&material.name),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(theme::TEXT),
                        ));
                        row.spawn((
                            Text::new(format!(
                                "{}  ·  {:?}",
                                localizer.text("assets-sprite"),
                                material.blend
                            )),
                            TextFont {
                                font_size: FontSize::Px(8.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_FAINT),
                        ));
                    });
            }

            args.set("count", session.effect.flipbooks.len());
            panel_heading(
                panel,
                &localizer.text("assets-flipbooks"),
                &localizer.text_with("assets-registered", &args),
            );
            library_toolbar_button(
                panel,
                &localizer.text("assets-add-grid-flipbook"),
                LibraryAction::AddGridFlipbook,
            );
            for flipbook in &session.effect.flipbooks {
                panel
                    .spawn(Node {
                        min_height: Val::Px(38.0),
                        margin: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                        padding: UiRect::axes(Val::Px(9.0), Val::Px(5.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new(&flipbook.name),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(theme::TEXT),
                        ));
                        let mut args = FluentArgs::new();
                        args.set("frames", flipbook.frames.len());
                        args.set("fps", flipbook.frame_rate as f64);
                        row.spawn((
                            Text::new(localizer.text_with("assets-flipbook-summary", &args)),
                            TextFont {
                                font_size: FontSize::Px(8.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_FAINT),
                        ));
                    });
            }

            args.set("count", session.effect.emitters.len());
            panel_heading(
                panel,
                &localizer.text("assets-layers"),
                &localizer.text_with("assets-active", &args),
            );
            library_toolbar_button(
                panel,
                &localizer.text("assets-add-emitter"),
                LibraryAction::AddEmitter,
            );
            for (index, layer) in session.effect.emitters.iter().enumerate() {
                let selected = index == session.selected_layer_index();
                panel
                    .spawn((
                        Button,
                        EditorNativeControl,
                        LibraryAction::SelectLayer(index),
                        LayerRow(index),
                        Node {
                            height: Val::Px(42.0),
                            margin: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                            padding: UiRect::horizontal(Val::Px(9.0)),
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(9.0),
                            border_radius: BorderRadius::all(Val::Px(4.0)),
                            ..default()
                        },
                        BackgroundColor(if selected {
                            theme::SELECTION
                        } else {
                            theme::PANEL_DARK
                        }),
                    ))
                    .with_children(|row| {
                        row.spawn((
                            Node {
                                width: Val::Px(7.0),
                                height: Val::Px(24.0),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(layer_color(index)),
                        ));
                        row.spawn((
                            Text::new(&layer.name),
                            TextFont {
                                font_size: FontSize::Px(12.0),
                                ..default()
                            },
                            TextColor(if layer.enabled {
                                theme::TEXT
                            } else {
                                theme::TEXT_FAINT
                            }),
                        ));
                    });
            }
        });
}

fn library_toolbar_button<A: Component>(parent: &mut ChildSpawnerCommands, label: &str, action: A) {
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.to_owned()),
            Node {
                height: Val::Px(32.0),
                min_width: Val::Px(78.0),
                padding: UiRect::horizontal(Val::Px(12.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                ThemedText,
                AssetButtonLabel,
                Pickable::IGNORE,
            ));
        });
}

#[allow(clippy::type_complexity)]
fn handle_library_action_buttons(
    mut commands: Commands,
    mut interactions: Query<
        (
            Entity,
            &Interaction,
            &LibraryAction,
            Option<&FeathersActionButton>,
            Option<&PendingFeathersActivation>,
            &mut BackgroundColor,
        ),
        (
            Changed<Interaction>,
            Or<(With<Button>, With<FeathersActionButton>)>,
        ),
    >,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    for (entity, interaction, action, feathers, pending, mut background) in &mut interactions {
        match *interaction {
            Interaction::Hovered if feathers.is_none() => background.0 = theme::BUTTON_HOVER,
            Interaction::None if feathers.is_none() => background.0 = theme::PANEL_DARK,
            Interaction::Pressed => {
                if feathers.is_some() {
                    if pending.is_none() {
                        continue;
                    }
                    commands
                        .entity(entity)
                        .remove::<PendingFeathersActivation>()
                        .insert(Interaction::None);
                } else {
                    background.0 = theme::ACCENT_DIM;
                }
                menu.open = None;
                menu.panels_open = false;
                if menu.tab_context.take().is_some() {
                    session.ui_revision += 1;
                }
                commands.trigger(*action);
            }
            _ => {}
        }
    }
}

fn execute_library_action(
    action: On<LibraryAction>,
    mut session: ResMut<EditorSession>,
    mut curves: ResMut<CurvesState>,
    mut layout: ResMut<WorkspaceLayout>,
    localizer: Res<Localizer>,
) {
    match *action {
        LibraryAction::AddSpriteMaterial => session.add_sprite_material(),
        LibraryAction::AddGridFlipbook => session.add_grid_flipbook(),
        LibraryAction::AddEmitter => {
            session.add_layer();
            curves.clear();
        }
        LibraryAction::DuplicateEmitter => {
            session.duplicate_selected_layer();
            curves.clear();
        }
        LibraryAction::DeleteEmitter => {
            if preview_selected_emitter_deletion(&mut session, &localizer) {
                reveal_dock_panel(&mut layout, &mut session, DockPanel::Changes);
                curves.clear();
            }
        }
        LibraryAction::SelectLayer(index) => {
            session.select_layer(index);
            curves.clear();
        }
    }
}

fn library_keyboard_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    palette: Res<ModulePaletteState>,
) {
    if palette.open {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if control && keys.just_pressed(KeyCode::Enter) {
        commands.trigger(LibraryAction::AddEmitter);
    }
    if control && keys.just_pressed(KeyCode::KeyD) {
        commands.trigger(LibraryAction::DuplicateEmitter);
    }
    if keys.just_pressed(KeyCode::Delete) {
        commands.trigger(LibraryAction::DeleteEmitter);
    }
}

fn preview_selected_emitter_deletion(session: &mut EditorSession, localizer: &Localizer) -> bool {
    if session.effect.emitters.len() <= 1 {
        session.status = localizer.text("assets-status-minimum-emitter");
        return false;
    }
    let id = session.selected_layer().id;
    session.preview_transaction(EffectTransaction::single(
        localizer.text("assets-change-delete-emitter"),
        EffectCommand::RemoveEmitter { id },
    ))
}

fn update_layer_selection(
    session: Res<EditorSession>,
    mut rows: Query<(&LayerRow, &mut BackgroundColor)>,
) {
    if !session.is_changed() {
        return;
    }
    for (row, mut color) in &mut rows {
        color.0 = if row.0 == session.selected_layer_index() {
            theme::SELECTION
        } else {
            theme::PANEL_DARK
        };
    }
}

pub(crate) fn layer_color(index: usize) -> Color {
    match index % 4 {
        0 => Color::srgb(0.48, 0.31, 0.98),
        1 => Color::srgb(0.17, 0.75, 0.95),
        2 => Color::srgb(0.98, 0.47, 0.21),
        _ => Color::srgb(0.84, 0.29, 0.72),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_effect(path: &Path, name: &str) {
        let mut effect = EffectAsset::from_ron(EFFECT_SOURCE).expect("sample effect is valid");
        effect.name = name.into();
        effect.save_ron(path).expect("effect fixture should save");
    }

    #[test]
    fn project_catalog_is_sorted_and_ids_are_stable_across_scans() {
        let temporary = tempfile::tempdir().unwrap();
        write_effect(&temporary.path().join("zeta.aestra.ron"), "Zeta");
        write_effect(&temporary.path().join("alpha.aestra.ron"), "Alpha");

        let first = ProjectEffectCatalog::scan(temporary.path());
        let second = ProjectEffectCatalog::scan(temporary.path());

        assert_eq!(
            first
                .entries()
                .iter()
                .map(|entry| entry.display_name.as_str())
                .collect::<Vec<_>>(),
            ["Alpha", "Zeta"]
        );
        assert_eq!(
            first
                .entries()
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            second
                .entries()
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn empty_project_catalog_is_a_valid_state() {
        let temporary = tempfile::tempdir().unwrap();

        let catalog = ProjectEffectCatalog::scan(temporary.path());

        assert!(catalog.entries().is_empty());
    }

    #[test]
    fn project_catalog_preserves_invalid_and_unsupported_files() {
        let temporary = tempfile::tempdir().unwrap();
        let valid_path = temporary.path().join("valid.aestra.ron");
        let invalid_path = temporary.path().join("broken.aestra.ron");
        let unsupported_path = temporary.path().join("future.aestra.ron");
        write_effect(&valid_path, "Valid");
        fs::write(&invalid_path, "this is not RON").unwrap();
        fs::write(
            &unsupported_path,
            EFFECT_SOURCE.replacen("format_version: 3", "format_version: 99", 1),
        )
        .unwrap();

        let catalog = ProjectEffectCatalog::scan(temporary.path());

        assert_eq!(catalog.entries().len(), 3);
        let valid = catalog
            .entries()
            .iter()
            .find(|entry| entry.path == valid_path)
            .unwrap();
        let invalid = catalog
            .entries()
            .iter()
            .find(|entry| entry.path == invalid_path)
            .unwrap();
        let unsupported = catalog
            .entries()
            .iter()
            .find(|entry| entry.path == unsupported_path)
            .unwrap();
        assert_eq!(valid.status, ProjectEffectStatus::Valid);
        assert!(matches!(
            invalid.status,
            ProjectEffectStatus::Invalid { ref message } if !message.is_empty()
        ));
        assert_eq!(
            unsupported.status,
            ProjectEffectStatus::Unsupported {
                found: 99,
                current: aestra_bevy::CURRENT_FORMAT_VERSION,
            }
        );
        assert_eq!(catalog.openable_path(valid.id), Some(valid_path.as_path()));
        assert_eq!(catalog.openable_path(invalid.id), None);
        assert_eq!(catalog.openable_path(unsupported.id), None);
    }

    #[test]
    fn library_state_filters_project_effects_by_query_origin_and_kind() {
        let entry = ProjectEffectEntry {
            id: ProjectEffectEntryId(1),
            display_name: "Prism Bloom".into(),
            path: PathBuf::from("assets/effects/prism_bloom.aestra.ron"),
            status: ProjectEffectStatus::Valid,
        };
        let mut state = LibraryState {
            query: "bloom".into(),
            origin: LibraryOriginFilter::Project,
            kind: LibraryKindFilter::Effect,
        };
        assert!(state.matches_project_effect(&entry));

        state.query = "PRISM_BLOOM.AESTRA".into();
        assert!(state.matches_project_effect(&entry));

        state.query = "plasma".into();
        assert!(!state.matches_project_effect(&entry));
        state.query.clear();
        state.origin = LibraryOriginFilter::CurrentDocument;
        assert!(!state.matches_project_effect(&entry));
        state.origin = LibraryOriginFilter::All;
        for kind in [
            LibraryKindFilter::Texture,
            LibraryKindFilter::Mesh,
            LibraryKindFilter::Material,
            LibraryKindFilter::Flipbook,
        ] {
            state.kind = kind;
            assert!(!state.matches_project_effect(&entry));
        }
    }

    #[test]
    fn live_search_updates_library_state_without_rebuilding_editor_ui() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let revision = session.ui_revision;
        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<LibraryState>()
            .add_observer(update_library_query);
        let input = app.world_mut().spawn(LibrarySearchInput).id();

        app.world_mut().trigger(ValueChange::<String> {
            source: input,
            value: String::from("PrIsM"),
            is_final: false,
        });

        assert_eq!(app.world().resource::<LibraryState>().query, "PrIsM");
        assert_eq!(
            app.world().resource::<EditorSession>().ui_revision,
            revision
        );
        assert!(app.world().entities().contains(input));
        app.world_mut().despawn(input);
        assert_eq!(app.world().resource::<LibraryState>().query, "PrIsM");
    }

    #[test]
    fn filtering_updates_existing_rows_count_and_empty_state_in_place() {
        let first_id = ProjectEffectEntryId(1);
        let second_id = ProjectEffectEntryId(2);
        let catalog = ProjectEffectCatalog {
            entries: vec![
                ProjectEffectEntry {
                    id: first_id,
                    display_name: "Prism Bloom".into(),
                    path: PathBuf::from("assets/effects/prism_bloom.aestra.ron"),
                    status: ProjectEffectStatus::Valid,
                },
                ProjectEffectEntry {
                    id: second_id,
                    display_name: "Plasma Burst".into(),
                    path: PathBuf::from("assets/effects/plasma_burst.aestra.ron"),
                    status: ProjectEffectStatus::Valid,
                },
            ],
        };
        let mut app = App::new();
        app.insert_resource(catalog)
            .init_resource::<LibraryState>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_systems(Update, sync_library_filtering);
        let prism = app
            .world_mut()
            .spawn((ProjectEffectRow(first_id), Node::default()))
            .id();
        let plasma = app
            .world_mut()
            .spawn((ProjectEffectRow(second_id), Node::default()))
            .id();
        let count = app
            .world_mut()
            .spawn((LibraryProjectCount, Text::new("")))
            .id();
        let catalog_empty = app
            .world_mut()
            .spawn((LibraryCatalogEmpty, Node::default()))
            .id();
        let no_results = app
            .world_mut()
            .spawn((LibraryNoResults, Node::default()))
            .id();
        app.update();

        app.world_mut().resource_mut::<LibraryState>().query = "prism".into();
        app.update();

        assert_eq!(
            app.world().get::<Node>(prism).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Node>(plasma).unwrap().display,
            Display::None
        );
        let count_text = &app.world().get::<Text>(count).unwrap().0;
        assert!(count_text.contains('1'));
        assert!(count_text.ends_with(" FOUND"));
        assert_eq!(
            app.world().get::<Node>(catalog_empty).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<Node>(no_results).unwrap().display,
            Display::None
        );

        app.world_mut().resource_mut::<LibraryState>().query = "missing".into();
        app.update();

        assert_eq!(
            app.world().get::<Node>(prism).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<Node>(plasma).unwrap().display,
            Display::None
        );
        let count_text = &app.world().get::<Text>(count).unwrap().0;
        assert!(count_text.contains('0'));
        assert!(count_text.ends_with(" FOUND"));
        assert_eq!(
            app.world().get::<Node>(no_results).unwrap().display,
            Display::Flex
        );
        assert!(app.world().entities().contains(prism));
        assert!(app.world().entities().contains(plasma));
    }

    fn app_with_session(session: EditorSession) -> App {
        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<CurvesState>()
            .init_resource::<WorkspaceLayout>()
            .init_resource::<MenuState>()
            .init_resource::<ModulePaletteState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Localizer::new("en-US").expect("test locale should load"))
            .add_plugins(EditorLibraryPlugin);
        app
    }

    #[test]
    fn library_plugin_owns_catalog_and_panel_actions() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let initial_materials = session.effect.materials.len();
        let mut app = app_with_session(session);
        let control = app
            .world_mut()
            .spawn((
                Button,
                FeathersActionButton,
                Interaction::None,
                LibraryAction::AddSpriteMaterial,
                BackgroundColor::default(),
            ))
            .id();

        app.world_mut().trigger(Activate { entity: control });
        app.update();

        assert!(app.world().contains_resource::<ProjectEffectCatalog>());
        assert!(
            !app.world()
                .entity(control)
                .contains::<PendingFeathersActivation>()
        );
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .materials
                .len(),
            initial_materials + 1
        );
    }

    #[test]
    fn library_plugin_preserves_an_injected_project_catalog() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let expected_id = ProjectEffectEntryId(42);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(ProjectEffectCatalog {
                entries: vec![ProjectEffectEntry {
                    id: expected_id,
                    display_name: "Injected".into(),
                    path: PathBuf::from("virtual/injected.aestra.ron"),
                    status: ProjectEffectStatus::Valid,
                }],
            })
            .init_resource::<CurvesState>()
            .init_resource::<WorkspaceLayout>()
            .init_resource::<MenuState>()
            .init_resource::<ModulePaletteState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_plugins(EditorLibraryPlugin);

        assert_eq!(
            app.world().resource::<ProjectEffectCatalog>().entries()[0].id,
            expected_id
        );
    }

    #[test]
    fn selecting_a_layer_clears_the_curve_workspace_selection() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.select_layer(0);
        let mut app = app_with_session(session);
        app.insert_resource({
            let mut state = CurvesState::default();
            state.select_for_test(ModuleId::new(), 0, 0);
            state
        });
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            LibraryAction::SelectLayer(1),
            BackgroundColor::default(),
        ));

        app.update();

        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .selected_layer_index(),
            1
        );
        assert!(!app.world().resource::<CurvesState>().has_selection());
    }

    #[test]
    fn emitter_actions_add_and_duplicate_through_one_contract() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let initial_emitters = session.effect.emitters.len();
        let mut app = app_with_session(session);

        app.world_mut().trigger(LibraryAction::AddEmitter);
        app.update();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .emitters
                .len(),
            initial_emitters + 1
        );

        app.world_mut().trigger(LibraryAction::DuplicateEmitter);
        app.update();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.emitters.len(), initial_emitters + 2);
        assert!(session.can_undo());
    }

    #[test]
    fn delete_emitter_action_opens_a_review_and_clears_curves() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let mut app = app_with_session(session);
        assert!(
            app.world_mut()
                .resource_mut::<WorkspaceLayout>()
                .show(DockPanel::Changes)
        );
        app.insert_resource({
            let mut state = CurvesState::default();
            state.select_for_test(ModuleId::new(), 0, 0);
            state
        });

        app.world_mut().trigger(LibraryAction::DeleteEmitter);
        app.update();

        assert!(
            app.world()
                .resource::<EditorSession>()
                .pending_change
                .is_some()
        );
        assert!(!app.world().resource::<CurvesState>().has_selection());
    }

    #[test]
    fn delete_emitter_action_protects_the_last_emitter() {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.effect.emitters.truncate(1);
        let mut app = app_with_session(session);

        app.world_mut().trigger(LibraryAction::DeleteEmitter);
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.emitters.len(), 1);
        assert!(session.pending_change.is_none());
        assert_eq!(session.status, "An effect must keep at least one emitter");
    }

    #[test]
    fn emitter_shortcuts_route_through_the_library_action_contract() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let initial_emitters = session.effect.emitters.len();
        let mut app = app_with_session(session);
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ControlLeft);
            keys.press(KeyCode::Enter);
        }

        app.update();

        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .emitters
                .len(),
            initial_emitters + 1
        );
    }
}
