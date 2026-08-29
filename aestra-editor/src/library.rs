//! Library workspace, project-effect catalog, and panel-local authoring actions.

use crate::*;
use bevy::ui_widgets::Activate;
#[cfg(test)]
use bevy::ui_widgets::ScrollArea;
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
            .add_observer(activate_library_list_entry)
            .add_observer(execute_library_action)
            .add_observer(update_library_query)
            .add_systems(
                Update,
                handle_library_action_buttons.in_set(LibrarySet::Actions),
            )
            .add_systems(Update, sync_library_filtering.in_set(LibrarySet::Sync));
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
    availability: ProjectEffectCatalogAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProjectEffectCatalogAvailability {
    Ready,
    Unavailable { root: PathBuf, message: String },
}

impl Default for ProjectEffectCatalog {
    fn default() -> Self {
        Self::scan(DEFAULT_PROJECT_EFFECT_ROOT)
    }
}

impl ProjectEffectCatalog {
    pub(crate) fn scan(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        let directory = match fs::read_dir(root) {
            Ok(directory) => directory,
            Err(error) => {
                return Self {
                    entries: Vec::new(),
                    availability: ProjectEffectCatalogAvailability::Unavailable {
                        root: root.to_owned(),
                        message: error.to_string(),
                    },
                };
            }
        };
        let mut paths = directory
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
        Self {
            entries,
            availability: ProjectEffectCatalogAvailability::Ready,
        }
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

    #[cfg(test)]
    fn from_entries(entries: Vec<ProjectEffectEntry>) -> Self {
        Self {
            entries,
            availability: ProjectEffectCatalogAvailability::Ready,
        }
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
}

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

#[derive(Component)]
struct LibraryCatalogUnavailable;

#[derive(Component)]
struct LibraryProjectEffectsSection;

#[derive(Component)]
struct LibraryCurrentResourcesSection;

#[derive(Component)]
struct LibraryTextureMeshSection;

#[derive(Component)]
struct LibraryMaterialsSection;

#[derive(Component)]
struct LibraryFlipbooksSection;

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

fn activate_library_list_entry(
    change: On<ValueChange<Entity>>,
    lists: Query<(), With<KeyboardNavigableList>>,
    actions: Query<&DocumentAction, With<ProjectEffectRow>>,
    mut commands: Commands,
) {
    if lists.contains(change.source)
        && let Ok(action) = actions.get(change.value)
    {
        commands.trigger(*action);
    }
}

fn sync_library_filtering(
    mut commands: Commands,
    state: Res<LibraryState>,
    catalog: Res<ProjectEffectCatalog>,
    mut nodes: Query<(
        Entity,
        &mut Node,
        Option<&ProjectEffectRow>,
        Has<LibraryCatalogEmpty>,
        Has<LibraryNoResults>,
        Has<LibraryCatalogUnavailable>,
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
    let catalog_ready = matches!(
        catalog.availability,
        ProjectEffectCatalogAvailability::Ready
    );
    for (entity, mut node, row, catalog_empty, no_results, unavailable) in &mut nodes {
        if let Some(row) = row {
            let visible = catalog
                .entry(row.0)
                .is_some_and(|entry| state.matches_project_effect(entry));
            node.display = if visible {
                Display::Flex
            } else {
                Display::None
            };
            if visible {
                commands.entity(entity).insert(ListItem);
            } else {
                commands.entity(entity).remove::<ListItem>();
            }
        } else if catalog_empty {
            node.display = if catalog_ready && catalog.entries().is_empty() {
                Display::Flex
            } else {
                Display::None
            };
        } else if no_results {
            node.display = if catalog_ready && !catalog.entries().is_empty() && visible_count == 0 {
                Display::Flex
            } else {
                Display::None
            };
        } else if unavailable {
            node.display = if catalog_ready {
                Display::None
            } else {
                Display::Flex
            };
        }
    }
    let text = if catalog_ready {
        let mut args = FluentArgs::new();
        args.set("count", visible_count);
        localizer.text_with("assets-found", &args)
    } else {
        localizer.text("library-unavailable-meta")
    };
    for mut count in &mut counts {
        count.0.clone_from(&text);
    }
}

fn spawn_project_effects(
    panel: &mut ChildSpawnerCommands,
    catalog: &ProjectEffectCatalog,
    state: &LibraryState,
    localizer: &Localizer,
) {
    let visible_count = catalog
        .entries()
        .iter()
        .filter(|entry| state.matches_project_effect(entry))
        .count();
    let mut args = FluentArgs::new();
    args.set("count", visible_count);
    let project_meta = if matches!(
        catalog.availability,
        ProjectEffectCatalogAvailability::Ready
    ) {
        localizer.text_with("assets-found", &args)
    } else {
        localizer.text("library-unavailable-meta")
    };
    let section = spawn_list_section_header(
        panel,
        &localizer.text("assets-project-effects"),
        &project_meta,
    );
    panel
        .commands()
        .entity(section.root)
        .insert(LibraryProjectEffectsSection);
    panel
        .commands()
        .entity(section.meta)
        .insert(LibraryProjectCount);
    panel
        .spawn(Node {
            width: Val::Percent(100.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
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

    for entry in catalog.entries() {
        let source = entry.path.display().to_string();
        let accessible_label = project_effect_accessible_label(entry, localizer);
        let row = match &entry.status {
            ProjectEffectStatus::Valid => spawn_action_list_row(
                panel,
                &entry.display_name,
                Some(&source),
                None,
                &accessible_label,
                DocumentAction::OpenCatalog(entry.id),
            ),
            ProjectEffectStatus::Invalid { .. } => spawn_status_list_row(
                panel,
                &entry.display_name,
                Some(&source),
                ListRowStatus {
                    label: &localizer.text("library-status-invalid"),
                    color: theme::ACCENT,
                },
                &accessible_label,
            ),
            ProjectEffectStatus::Unsupported { .. } => spawn_status_list_row(
                panel,
                &entry.display_name,
                Some(&source),
                ListRowStatus {
                    label: &localizer.text("library-status-unsupported"),
                    color: theme::ACCENT,
                },
                &accessible_label,
            ),
        };
        panel.commands().entity(row).insert((
            ProjectEffectRow(entry.id),
            ListItem,
            KeyboardNavigableListRow,
            project_effect_tooltip(entry, localizer),
        ));
    }
    let catalog_empty = spawn_list_empty_state(
        panel,
        &localizer.text("library-empty-title"),
        &localizer.text("library-empty-message"),
        theme::TEXT_MUTED,
        if matches!(
            catalog.availability,
            ProjectEffectCatalogAvailability::Ready
        ) && catalog.entries().is_empty()
        {
            Display::Flex
        } else {
            Display::None
        },
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
        if matches!(
            catalog.availability,
            ProjectEffectCatalogAvailability::Ready
        ) && !catalog.entries().is_empty()
            && visible_count == 0
        {
            Display::Flex
        } else {
            Display::None
        },
    );
    panel.commands().entity(no_results).insert(LibraryNoResults);
    let (unavailable_message, unavailable_tooltip) = match &catalog.availability {
        ProjectEffectCatalogAvailability::Ready => {
            let mut args = FluentArgs::new();
            args.set("path", DEFAULT_PROJECT_EFFECT_ROOT);
            let message = localizer.text_with("library-unavailable-message", &args);
            (message.clone(), EditorTooltip::description(message))
        }
        ProjectEffectCatalogAvailability::Unavailable { root, message } => {
            let path = root.display().to_string();
            let mut args = FluentArgs::new();
            args.set("path", path.as_str());
            (
                localizer.text_with("library-unavailable-message", &args),
                EditorTooltip::titled(localizer.text("library-unavailable-title"), message)
                    .with_footer(path),
            )
        }
    };
    let unavailable = spawn_list_empty_state(
        panel,
        &localizer.text("library-unavailable-title"),
        &unavailable_message,
        theme::ACCENT,
        if matches!(
            catalog.availability,
            ProjectEffectCatalogAvailability::Unavailable { .. }
        ) {
            Display::Flex
        } else {
            Display::None
        },
    );
    panel
        .commands()
        .entity(unavailable)
        .insert((LibraryCatalogUnavailable, unavailable_tooltip));
}

fn project_effect_accessible_label(entry: &ProjectEffectEntry, localizer: &Localizer) -> String {
    match entry.status {
        ProjectEffectStatus::Valid => {
            let mut args = FluentArgs::new();
            args.set("name", entry.display_name.as_str());
            localizer.text_with("library-open-effect", &args)
        }
        ProjectEffectStatus::Invalid { ref message } => {
            let mut args = FluentArgs::new();
            args.set("name", entry.display_name.as_str());
            args.set("message", message.as_str());
            localizer.text_with("library-invalid-accessible", &args)
        }
        ProjectEffectStatus::Unsupported { found, current } => {
            let mut args = FluentArgs::new();
            args.set("name", entry.display_name.as_str());
            args.set("found", i64::from(found));
            args.set("current", i64::from(current));
            localizer.text_with("library-unsupported-accessible", &args)
        }
    }
}

fn project_effect_tooltip(entry: &ProjectEffectEntry, localizer: &Localizer) -> EditorTooltip {
    let source = entry.path.display().to_string();
    match &entry.status {
        ProjectEffectStatus::Valid => {
            let mut args = FluentArgs::new();
            args.set("path", source.as_str());
            EditorTooltip::titled(
                &entry.display_name,
                localizer.text_with("library-effect-source", &args),
            )
        }
        ProjectEffectStatus::Invalid { message } => {
            EditorTooltip::titled(localizer.text("library-status-invalid"), message)
                .with_footer(source)
        }
        ProjectEffectStatus::Unsupported { found, current } => {
            let mut args = FluentArgs::new();
            args.set("found", i64::from(*found));
            args.set("current", i64::from(*current));
            EditorTooltip::titled(
                localizer.text("library-status-unsupported"),
                localizer.text_with("library-unsupported-description", &args),
            )
            .with_footer(source)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CurrentResourceCounts {
    texture_mesh: usize,
    materials: usize,
    flipbooks: usize,
}

impl CurrentResourceCounts {
    fn total(self) -> usize {
        self.texture_mesh + self.materials + self.flipbooks
    }
}

fn current_resource_counts(effect: &EffectAsset) -> CurrentResourceCounts {
    CurrentResourceCounts {
        texture_mesh: effect
            .assets
            .iter()
            .filter(|asset| matches!(asset.kind, AssetKind::Texture | AssetKind::Mesh))
            .count(),
        materials: effect.materials.len(),
        flipbooks: effect.flipbooks.len()
            + effect
                .assets
                .iter()
                .filter(|asset| asset.kind == AssetKind::Flipbook)
                .count(),
    }
}

fn spawn_current_document_resources(
    panel: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    let counts = current_resource_counts(&session.effect);
    let mut args = FluentArgs::new();
    args.set("count", counts.total());
    let section = spawn_list_section_header(
        panel,
        &localizer.text("library-current-document-resources"),
        &localizer.text_with("assets-registered", &args),
    );
    panel
        .commands()
        .entity(section.root)
        .insert(LibraryCurrentResourcesSection);

    let texture_mesh_assets = session
        .effect
        .assets
        .iter()
        .filter(|asset| matches!(asset.kind, AssetKind::Texture | AssetKind::Mesh));
    if counts.texture_mesh > 0 {
        args.set("count", counts.texture_mesh);
        let section = spawn_list_section_header(
            panel,
            &localizer.text("library-textures-meshes"),
            &localizer.text_with("assets-registered", &args),
        );
        panel
            .commands()
            .entity(section.root)
            .insert(LibraryTextureMeshSection);
        for asset in texture_mesh_assets {
            let kind = match asset.kind {
                AssetKind::Texture => localizer.text("library-kind-texture"),
                AssetKind::Mesh => localizer.text("library-kind-mesh"),
                AssetKind::Flipbook => unreachable!("filtered above"),
            };
            spawn_info_list_row(
                panel,
                &asset.name,
                Some(&format!("{kind}  ·  {}", asset.path)),
            );
        }
    }

    args.set("count", counts.materials);
    let section = spawn_list_section_header(
        panel,
        &localizer.text("assets-materials"),
        &localizer.text_with("assets-registered", &args),
    );
    panel
        .commands()
        .entity(section.root)
        .insert(LibraryMaterialsSection);
    library_toolbar_button(
        panel,
        &localizer.text("assets-add-sprite-material"),
        LibraryAction::AddSpriteMaterial,
    );
    for material in &session.effect.materials {
        spawn_info_list_row(
            panel,
            &material.name,
            Some(&format!(
                "{}  ·  {}",
                localizer.text("assets-sprite"),
                localize_blend_mode(material.blend, localizer)
            )),
        );
    }

    let imported_flipbooks = session
        .effect
        .assets
        .iter()
        .filter(|asset| asset.kind == AssetKind::Flipbook);
    args.set("count", counts.flipbooks);
    let section = spawn_list_section_header(
        panel,
        &localizer.text("assets-flipbooks"),
        &localizer.text_with("assets-registered", &args),
    );
    panel
        .commands()
        .entity(section.root)
        .insert(LibraryFlipbooksSection);
    library_toolbar_button(
        panel,
        &localizer.text("assets-add-grid-flipbook"),
        LibraryAction::AddGridFlipbook,
    );
    for asset in imported_flipbooks {
        spawn_info_list_row(
            panel,
            &asset.name,
            Some(&format!(
                "{}  ·  {}",
                localizer.text("library-kind-flipbook"),
                asset.path
            )),
        );
    }
    for flipbook in &session.effect.flipbooks {
        let mut args = FluentArgs::new();
        args.set("frames", flipbook.frames.len());
        args.set("fps", flipbook.frame_rate as f64);
        spawn_info_list_row(
            panel,
            &flipbook.name,
            Some(&localizer.text_with("assets-flipbook-summary", &args)),
        );
    }
}

fn localize_blend_mode(blend: BlendMode, localizer: &Localizer) -> String {
    localizer.text(match blend {
        BlendMode::Alpha => "library-blend-alpha",
        BlendMode::Additive => "library-blend-additive",
        BlendMode::Multiply => "library-blend-multiply",
    })
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
            ..default()
        })
        .with_children(|body| {
            let list = spawn_vertical_scroll_area(
                body,
                ScrollMemoryKey::Library,
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                |panel| {
                    spawn_project_effects(panel, catalog, state, localizer);
                    spawn_current_document_resources(panel, session, localizer);
                },
            );
            body.commands().entity(list).insert((
                ListBox,
                KeyboardNavigableList,
                TabIndex(0),
                AccessibleLabel(localizer.text("assets-project-effects")),
            ));
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

fn execute_library_action(action: On<LibraryAction>, mut session: ResMut<EditorSession>) {
    match *action {
        LibraryAction::AddSpriteMaterial => session.add_sprite_material(),
        LibraryAction::AddGridFlipbook => session.add_grid_flipbook(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::blank_effect;
    use bevy::{asset::AssetPlugin, scene::ScenePlugin, text::TextPlugin};

    #[derive(Resource, Default)]
    struct CapturedDocumentAction(Option<DocumentAction>);

    fn capture_document_action(
        action: On<DocumentAction>,
        mut captured: ResMut<CapturedDocumentAction>,
    ) {
        captured.0 = Some(*action);
    }

    fn spawn_test_library(
        mut commands: Commands,
        session: Res<EditorSession>,
        catalog: Res<ProjectEffectCatalog>,
        state: Res<LibraryState>,
        localizer: Res<Localizer>,
    ) {
        commands.spawn(Node::default()).with_children(|parent| {
            spawn_library(parent, &session, &catalog, &state, &localizer);
        });
    }

    fn marker_count<T: Component>(app: &mut App) -> usize {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<T>>();
        query.iter(world).count()
    }

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
        assert_eq!(
            catalog.availability,
            ProjectEffectCatalogAvailability::Ready
        );
    }

    #[test]
    fn missing_project_catalog_is_reported_as_unavailable() {
        let temporary = tempfile::tempdir().unwrap();
        let missing = temporary.path().join("missing-effects");

        let catalog = ProjectEffectCatalog::scan(&missing);

        assert!(catalog.entries().is_empty());
        assert!(matches!(
            catalog.availability,
            ProjectEffectCatalogAvailability::Unavailable { ref root, ref message }
                if root == &missing && !message.is_empty()
        ));
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
    fn library_composition_separates_project_resources_and_choreography() {
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        let effect_id = session.effect.id.to_string();
        let valid_id = ProjectEffectEntryId(1);
        let invalid_id = ProjectEffectEntryId(2);
        let unsupported_id = ProjectEffectEntryId(3);
        let catalog = ProjectEffectCatalog::from_entries(vec![
            ProjectEffectEntry {
                id: valid_id,
                display_name: "Prism Bloom".into(),
                path: PathBuf::from("assets/effects/prism_bloom.aestra.ron"),
                status: ProjectEffectStatus::Valid,
            },
            ProjectEffectEntry {
                id: invalid_id,
                display_name: "Broken Effect".into(),
                path: PathBuf::from("assets/effects/broken.aestra.ron"),
                status: ProjectEffectStatus::Invalid {
                    message: "Invalid RON fixture".into(),
                },
            },
            ProjectEffectEntry {
                id: unsupported_id,
                display_name: "Future Effect".into(),
                path: PathBuf::from("assets/effects/future.aestra.ron"),
                status: ProjectEffectStatus::Unsupported {
                    found: 99,
                    current: aestra_bevy::CURRENT_FORMAT_VERSION,
                },
            },
        ]);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .insert_resource(session)
        .insert_resource(catalog)
        .init_resource::<LibraryState>()
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_systems(Startup, spawn_test_library);

        app.update();

        assert_eq!(marker_count::<LibraryProjectEffectsSection>(&mut app), 1);
        assert_eq!(marker_count::<LibraryCurrentResourcesSection>(&mut app), 1);
        assert_eq!(marker_count::<LibraryTextureMeshSection>(&mut app), 0);
        assert_eq!(marker_count::<LibraryMaterialsSection>(&mut app), 1);
        assert_eq!(marker_count::<LibraryFlipbooksSection>(&mut app), 1);
        assert_eq!(marker_count::<ChoreographyAction>(&mut app), 0);
        assert_eq!(marker_count::<ListBox>(&mut app), 1);
        assert_eq!(marker_count::<ScrollArea>(&mut app), 1);
        assert_eq!(marker_count::<KeyboardNavigableList>(&mut app), 1);
        assert_eq!(marker_count::<LibraryCatalogUnavailable>(&mut app), 1);

        let rows = {
            let world = app.world_mut();
            let mut query = world.query::<(
                &ProjectEffectRow,
                Has<Button>,
                Option<&DocumentAction>,
                Has<EditorTooltip>,
                &AccessibleLabel,
                Has<ListItem>,
                Has<KeyboardNavigableListRow>,
            )>();
            query
                .iter(world)
                .map(
                    |(row, button, action, tooltip, label, list_item, keyboard_row)| {
                        (
                            row.0,
                            button,
                            action.copied(),
                            tooltip,
                            label.0.clone(),
                            list_item,
                            keyboard_row,
                        )
                    },
                )
                .collect::<Vec<_>>()
        };
        assert_eq!(rows.len(), 3);
        let valid = rows.iter().find(|row| row.0 == valid_id).unwrap();
        assert!(valid.1);
        assert_eq!(valid.2, Some(DocumentAction::OpenCatalog(valid_id)));
        assert!(valid.3);
        assert!(valid.4.starts_with("Open "));
        assert!(valid.4.contains("Prism Bloom"));
        assert!(valid.5);
        assert!(valid.6);
        for id in [invalid_id, unsupported_id] {
            let status = rows.iter().find(|row| row.0 == id).unwrap();
            assert!(!status.1);
            assert_eq!(status.2, None);
            assert!(status.3);
            assert!(!status.4.is_empty());
            assert!(status.5);
            assert!(status.6);
        }
        assert!(
            rows.iter()
                .find(|row| row.0 == invalid_id)
                .unwrap()
                .4
                .contains("Invalid RON fixture")
        );
        let unsupported_label = &rows.iter().find(|row| row.0 == unsupported_id).unwrap().4;
        assert!(unsupported_label.contains("99"));
        assert!(unsupported_label.contains(&aestra_bevy::CURRENT_FORMAT_VERSION.to_string()));

        let exposes_raw_id = {
            let world = app.world_mut();
            let mut query = world.query::<&Text>();
            query.iter(world).any(|text| text.0.contains(&effect_id))
        };
        assert!(!exposes_raw_id);
    }

    #[test]
    fn list_value_change_activates_the_entry_semantic_action() {
        let mut app = App::new();
        app.init_resource::<CapturedDocumentAction>()
            .add_observer(activate_library_list_entry)
            .add_observer(capture_document_action);
        let list = app.world_mut().spawn(KeyboardNavigableList).id();
        let id = ProjectEffectEntryId(42);
        let row = app
            .world_mut()
            .spawn((ProjectEffectRow(id), DocumentAction::OpenCatalog(id)))
            .id();

        app.world_mut().trigger(ValueChange::<Entity> {
            source: list,
            value: row,
            is_final: true,
        });
        app.update();

        assert_eq!(
            app.world().resource::<CapturedDocumentAction>().0,
            Some(DocumentAction::OpenCatalog(id))
        );
    }

    #[test]
    fn current_resource_projection_tracks_new_open_undo_and_redo() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("library-projection.aestra.ron");
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.effect.save_ron(&path).unwrap();
        let original = current_resource_counts(&session.effect);

        session.add_sprite_material();
        let edited = current_resource_counts(&session.effect);
        assert_eq!(edited.materials, original.materials + 1);
        assert_eq!(edited.texture_mesh, original.texture_mesh);
        assert_eq!(edited.flipbooks, original.flipbooks);

        session.undo();
        assert_eq!(current_resource_counts(&session.effect), original);
        session.redo();
        assert_eq!(current_resource_counts(&session.effect), edited);

        let blank = current_resource_counts(&blank_effect());
        session.new_effect();
        assert_eq!(current_resource_counts(&session.effect), blank);
        session.open(&path).unwrap();
        assert_eq!(current_resource_counts(&session.effect), original);
    }

    #[test]
    fn library_resource_labels_are_localized_in_english_and_french() {
        let english = Localizer::new("en-US").unwrap();
        let french = Localizer::new("fr-FR").unwrap();

        assert_eq!(
            english.text("library-current-document-resources"),
            "CURRENT DOCUMENT RESOURCES"
        );
        assert_eq!(
            french.text("library-current-document-resources"),
            "RESSOURCES DU DOCUMENT COURANT"
        );
        assert_eq!(
            localize_blend_mode(BlendMode::Additive, &english),
            "Additive"
        );
        assert_eq!(localize_blend_mode(BlendMode::Additive, &french), "Additif");
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
        let catalog = ProjectEffectCatalog::from_entries(vec![
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
        ]);
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
        let unavailable = app
            .world_mut()
            .spawn((LibraryCatalogUnavailable, Node::default()))
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
        assert!(app.world().get::<ListItem>(prism).is_some());
        assert!(app.world().get::<ListItem>(plasma).is_none());
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
        assert!(app.world().get::<ListItem>(prism).is_none());
        assert!(app.world().get::<ListItem>(plasma).is_none());
        let count_text = &app.world().get::<Text>(count).unwrap().0;
        assert!(count_text.contains('0'));
        assert!(count_text.ends_with(" FOUND"));
        assert_eq!(
            app.world().get::<Node>(no_results).unwrap().display,
            Display::Flex
        );
        assert!(app.world().entities().contains(prism));
        assert!(app.world().entities().contains(plasma));

        {
            let mut catalog = app.world_mut().resource_mut::<ProjectEffectCatalog>();
            catalog.entries.clear();
            catalog.availability = ProjectEffectCatalogAvailability::Unavailable {
                root: PathBuf::from("assets/effects"),
                message: "permission denied".into(),
            };
        }
        app.update();

        assert_eq!(
            app.world().get::<Node>(catalog_empty).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<Node>(no_results).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<Node>(unavailable).unwrap().display,
            Display::Flex
        );
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
            .insert_resource(ProjectEffectCatalog::from_entries(vec![
                ProjectEffectEntry {
                    id: expected_id,
                    display_name: "Injected".into(),
                    path: PathBuf::from("virtual/injected.aestra.ron"),
                    status: ProjectEffectStatus::Valid,
                },
            ]))
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
}
