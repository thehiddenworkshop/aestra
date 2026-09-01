//! Library workspace, project-effect catalog, and panel-local authoring actions.

use crate::feathers::context_menu::{
    keyboard_context_menu_requested, pointer_position_in_node, should_dismiss_pointer_context_menu,
    spawn_pointer_context_menu, spawn_pointer_context_menu_item,
};
use crate::timeline::TimelineState;
use crate::*;
use aestra_bevy::{
    AssetDefinition, AssetId, ChoreographyTrackId, CurveId, Diagnostic, EffectAsset,
    EffectAssetRef, EffectClip, EffectClipId, EffectId, EffectParameter, Emitter, EmitterId,
    EmitterTransform, EventId, EventLink, FlipbookDefinition, GradientId, MaterialDefinition,
    MaterialId, MaterialInput, ModuleParameters, ParameterId, RendererProperties,
    SpriteColorSource, ValidationReport, Value,
};
use aestra_compiler::{EffectCompiler, ProjectCompileError};
use aestra_project::{
    ProjectAssetIndex, ProjectAssetIndexAvailability, ProjectAssetOperationError,
    ProjectDependencyDiagnosticCode, ProjectEffectDeletePolicy, ProjectEffectEntry,
    ProjectEffectRelation, ProjectEffectStatus, ProjectEffectUsageGraph,
};
use aestra_runtime::CompiledEffectProject;
#[cfg(test)]
use bevy::ui_widgets::ScrollArea;
use bevy::{
    feathers::cursor::EntityCursor,
    input_focus::InputFocus,
    picking::{
        events::{Click, Drag, DragEnd, DragStart, Pointer},
        pointer::PointerButton,
    },
    ui_widgets::{Activate, ActiveDescendant},
    window::SystemCursorIcon,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const DEFAULT_PROJECT_EFFECT_ROOT: &str = "assets/effects";
const PROJECT_EFFECT_POLL_INTERVAL_SECONDS: f32 = 0.25;
const PROJECT_EFFECT_STABLE_OBSERVATIONS: u8 = 2;

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
            .init_resource::<ProjectEffectWatchState>()
            .init_resource::<LibraryState>()
            .init_resource::<LibraryAssetOperationState>()
            .init_resource::<RenderedLibraryRelationOverlay>()
            .add_observer(queue_library_action_activation)
            .add_observer(activate_library_list_entry)
            .add_observer(execute_library_action)
            .add_observer(update_library_rename_draft)
            .add_observer(update_reusable_effect_extraction_draft)
            .add_observer(update_reusable_effect_extraction_replace)
            .add_observer(resolve_library_asset_operation)
            .add_observer(update_library_query)
            .add_observer(begin_project_effect_drag)
            .add_observer(update_project_effect_drag)
            .add_observer(end_project_effect_drag)
            .add_observer(open_project_effect_context_menu)
            .add_systems(
                Update,
                poll_project_effect_catalog.in_set(LibrarySet::Input),
            )
            .add_systems(
                Update,
                (
                    dismiss_library_asset_operation_with_escape,
                    open_focused_library_context_menu,
                    dismiss_library_context_menu,
                    handle_library_action_buttons,
                    sync_library_asset_operation_overlay,
                )
                    .chain()
                    .in_set(LibrarySet::Actions),
            )
            .add_systems(
                Update,
                (
                    sync_library_filtering,
                    restore_library_context_menu_focus,
                    queue_library_relation_overlay_rebuild,
                )
                    .chain()
                    .in_set(LibrarySet::Sync),
            );
    }
}

#[derive(Resource)]
pub(crate) struct ProjectEffectCatalog {
    index: ProjectAssetIndex,
}

impl Default for ProjectEffectCatalog {
    fn default() -> Self {
        Self::scan(DEFAULT_PROJECT_EFFECT_ROOT)
    }
}

impl ProjectEffectCatalog {
    pub(crate) fn scan(root: impl AsRef<Path>) -> Self {
        Self {
            index: ProjectAssetIndex::scan(root),
        }
    }

    pub(crate) fn entries(&self) -> &[ProjectEffectEntry] {
        self.index.effects()
    }

    pub(crate) fn root(&self) -> &Path {
        self.index.root()
    }

    pub(crate) fn refresh(&mut self) {
        self.index.refresh();
    }

    pub(crate) fn create_effect_source(
        &mut self,
        effect: &EffectAsset,
    ) -> Result<ProjectEffectEntry, ProjectAssetOperationError> {
        self.index.create_effect_source(effect)
    }

    pub(crate) fn entry(&self, id: ProjectEffectEntryId) -> Option<&ProjectEffectEntry> {
        self.index.entry(id)
    }

    pub(crate) fn openable_path(&self, reference: EffectAssetRef) -> Option<&Path> {
        self.index
            .resolve(reference)
            .ok()
            .map(|entry| entry.path.as_path())
    }

    pub(crate) fn effect_for_placement(
        &self,
        owner: &EffectAsset,
        reference: EffectAssetRef,
    ) -> Result<EffectAsset, String> {
        if reference.id == owner.id {
            return Err("an effect cannot reference itself".into());
        }
        let source = self
            .index
            .load_effect(reference)
            .map_err(|error| error.to_string())?;
        let project = self
            .index
            .resolve_effect_project(&source)
            .map_err(|error| error.to_string())?;
        if project.effect(owner.id).is_some() {
            return Err("placing this effect would create a reference cycle".into());
        }
        Ok(source)
    }

    pub(crate) fn load_effect(&self, reference: EffectAssetRef) -> Result<EffectAsset, String> {
        self.index
            .load_effect(reference)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn compile_project(
        &self,
        root: &EffectAsset,
    ) -> Result<CompiledEffectProject, String> {
        EffectCompiler::default()
            .compile_project(root, &self.index)
            .map_err(|error| match error {
                ProjectCompileError::Dependencies(report) => report
                    .diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.message)
                    .collect::<Vec<_>>()
                    .join("; "),
                error => error.to_string(),
            })
    }

    pub(crate) fn dependency_validation_report(&self, effect: &EffectAsset) -> ValidationReport {
        let mut validation = ValidationReport::default();
        let Err(report) = self.index.resolve_effect_project(effect) else {
            return validation;
        };
        for diagnostic in report
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.owner == effect.id)
        {
            let Some(index) = effect
                .effect_clips
                .iter()
                .position(|clip| clip.id == diagnostic.clip)
            else {
                continue;
            };
            let code = match diagnostic.code {
                ProjectDependencyDiagnosticCode::InvalidTiming => DiagnosticCode::InvalidTiming,
                ProjectDependencyDiagnosticCode::Cycle => DiagnosticCode::ReferenceCycle,
                ProjectDependencyDiagnosticCode::Missing
                | ProjectDependencyDiagnosticCode::Duplicate
                | ProjectDependencyDiagnosticCode::Unresolvable
                | ProjectDependencyDiagnosticCode::IndexUnavailable
                | ProjectDependencyDiagnosticCode::SourceChanged => {
                    DiagnosticCode::InvalidReference
                }
            };
            validation.push(Diagnostic::error(
                code,
                format!("effect.effect_clips[{index}].source"),
                diagnostic.message,
            ));
        }
        validation
    }

    pub(crate) fn effect_clip_dependency_error(
        &self,
        effect: &EffectAsset,
        clip: EffectClipId,
    ) -> Option<String> {
        self.dependency_validation_report(effect)
            .diagnostics
            .into_iter()
            .find(|diagnostic| {
                effect
                    .effect_clips
                    .iter()
                    .position(|candidate| candidate.id == clip)
                    .is_some_and(|index| {
                        diagnostic.path == format!("effect.effect_clips[{index}].source")
                    })
            })
            .map(|diagnostic| diagnostic.message)
    }

    fn availability(&self) -> &ProjectAssetIndexAvailability {
        self.index.availability()
    }

    fn rename_effect_source(
        &mut self,
        source: ProjectEffectEntryId,
        name: &str,
    ) -> Result<ProjectEffectEntry, ProjectAssetOperationError> {
        self.index.rename_effect_source(source, name)
    }

    fn move_effect_source(
        &mut self,
        source: ProjectEffectEntryId,
        destination: &Path,
    ) -> Result<ProjectEffectEntry, ProjectAssetOperationError> {
        self.index.move_effect_source(source, destination)
    }

    fn effect_usage_graph(
        &self,
        reference: EffectAssetRef,
    ) -> Result<ProjectEffectUsageGraph, String> {
        self.index
            .effect_usage_graph(reference)
            .map_err(|error| error.to_string())
    }

    fn delete_effect_source(
        &mut self,
        source: ProjectEffectEntryId,
    ) -> Result<ProjectEffectEntry, ProjectAssetOperationError> {
        self.index
            .delete_effect_source(source, ProjectEffectDeletePolicy::AllowReferenced)
    }

    fn effect_name(&self, reference: EffectAssetRef) -> String {
        self.index.resolve(reference).map_or_else(
            |_| reference.to_string(),
            |entry| entry.display_name.clone(),
        )
    }

    #[cfg(test)]
    pub(crate) fn from_entries(entries: Vec<ProjectEffectEntry>) -> Self {
        Self {
            index: ProjectAssetIndex::from_entries("virtual", entries),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectEffectFileStamp {
    path: PathBuf,
    length: u64,
    modified_nanos: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectEffectTreeSnapshot {
    available: bool,
    diagnostic: Option<String>,
    files: Vec<ProjectEffectFileStamp>,
}

impl ProjectEffectTreeSnapshot {
    fn scan(root: &Path) -> Self {
        let mut files = Vec::new();
        let result = collect_project_effect_file_stamps(root, &mut files);
        files.sort_by(|left, right| left.path.cmp(&right.path));
        match result {
            Ok(()) => Self {
                available: true,
                diagnostic: None,
                files,
            },
            Err(error) => Self {
                available: false,
                diagnostic: Some(error.to_string()),
                files,
            },
        }
    }

    fn file(&self, path: &Path) -> Option<&ProjectEffectFileStamp> {
        self.files
            .iter()
            .find(|candidate| paths_refer_to_same_source(&candidate.path, path))
    }
}

#[derive(Resource)]
struct ProjectEffectWatchState {
    poll: Timer,
    committed: ProjectEffectTreeSnapshot,
    pending: Option<(ProjectEffectTreeSnapshot, u8)>,
}

impl FromWorld for ProjectEffectWatchState {
    fn from_world(world: &mut World) -> Self {
        let root = world.resource::<ProjectEffectCatalog>().root().to_owned();
        Self {
            poll: Timer::from_seconds(PROJECT_EFFECT_POLL_INTERVAL_SECONDS, TimerMode::Repeating),
            committed: ProjectEffectTreeSnapshot::scan(&root),
            pending: None,
        }
    }
}

impl ProjectEffectWatchState {
    fn observe(&mut self, snapshot: ProjectEffectTreeSnapshot) -> bool {
        if snapshot == self.committed {
            self.pending = None;
            return false;
        }
        match self.pending.as_mut() {
            Some((pending, observations)) if pending == &snapshot => {
                *observations = observations.saturating_add(1);
                if *observations < PROJECT_EFFECT_STABLE_OBSERVATIONS {
                    return false;
                }
            }
            _ => {
                self.pending = Some((snapshot, 1));
                return PROJECT_EFFECT_STABLE_OBSERVATIONS <= 1;
            }
        }
        self.committed = snapshot;
        self.pending = None;
        true
    }

    fn accept_current(&mut self, root: &Path) {
        self.committed = ProjectEffectTreeSnapshot::scan(root);
        self.pending = None;
        self.poll.reset();
    }
}

fn collect_project_effect_file_stamps(
    directory: &Path,
    files: &mut Vec<ProjectEffectFileStamp>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_project_effect_file_stamps(&path, files)?;
            continue;
        }
        if !file_type.is_file()
            || !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".aestra.ron"))
        {
            continue;
        }
        let metadata = entry.metadata()?;
        let modified_nanos = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |duration| duration.as_nanos());
        files.push(ProjectEffectFileStamp {
            path,
            length: metadata.len(),
            modified_nanos,
        });
    }
    Ok(())
}

fn poll_project_effect_catalog(
    time: Option<Res<Time>>,
    mut watch: ResMut<ProjectEffectWatchState>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    let Some(time) = time else {
        return;
    };
    if !watch.poll.tick(time.delta()).just_finished() {
        return;
    }
    let snapshot = ProjectEffectTreeSnapshot::scan(catalog.root());
    let previous = watch.committed.clone();
    if !watch.observe(snapshot.clone()) {
        return;
    }
    apply_project_effect_catalog_refresh(
        &mut catalog,
        &mut session,
        &previous,
        &snapshot,
        &localizer,
    );
}

fn apply_project_effect_catalog_refresh(
    catalog: &mut ProjectEffectCatalog,
    session: &mut EditorSession,
    previous: &ProjectEffectTreeSnapshot,
    current: &ProjectEffectTreeSnapshot,
    localizer: &Localizer,
) {
    let source_path = session.source_path.clone();
    let source_changed = source_path.as_deref().is_some_and(|path| {
        previous.file(path) != current.file(path)
            && (previous.file(path).is_some() || current.file(path).is_some())
    });
    catalog.refresh();

    let revision = session.ui_revision;
    let mut status_set = false;
    if source_changed && let Some(source_path) = source_path {
        let resolved_path = catalog
            .openable_path(EffectAssetRef::new(session.effect.id))
            .map(Path::to_owned);
        let path = resolved_path.as_deref().unwrap_or(&source_path);
        if session.dirty {
            if resolved_path.is_some() && path != source_path {
                session.source_path = Some(path.to_owned());
                let mut args = FluentArgs::new();
                args.set("path", path.display().to_string());
                session.status = localizer.text_with("library-status-source-moved-dirty", &args);
            } else if current.file(path).is_some() {
                session.status = localizer.text("library-status-source-conflict");
            } else {
                session.status = localizer.text("library-status-open-source-missing");
            }
            status_set = true;
        } else if current.file(path).is_some() {
            let matches_clean_session = path == source_path
                && EffectAsset::load_ron(path)
                    .ok()
                    .is_some_and(|effect| effect == session.effect);
            // An editor save changes the filesystem stamp too. When the session already owns
            // these exact bytes, retain its more useful save status and playback state.
            if !matches_clean_session {
                match session.open(path) {
                    Ok(()) => {
                        let mut args = FluentArgs::new();
                        args.set("path", path.display().to_string());
                        session.status =
                            localizer.text_with("library-status-source-reloaded", &args);
                    }
                    Err(error) => {
                        let mut args = FluentArgs::new();
                        args.set("message", error.to_string());
                        session.status =
                            localizer.text_with("library-status-source-reload-failed", &args);
                    }
                }
            }
            status_set = true;
        } else {
            session.status = localizer.text("library-status-open-source-missing");
            status_set = true;
        }
    }
    if session.ui_revision == revision {
        session.ui_revision += 1;
    }
    if !status_set {
        let mut args = FluentArgs::new();
        args.set("count", catalog.entries().len());
        session.status = localizer.text_with("library-status-catalog-refreshed", &args);
    }
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

#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub(crate) struct LibraryState {
    pub(crate) query: String,
    pub(crate) origin: LibraryOriginFilter,
    pub(crate) kind: LibraryKindFilter,
    pub(crate) context_effect: Option<ProjectEffectEntryId>,
    pub(crate) context_menu_position: Vec2,
    pub(crate) restore_context_effect_focus: Option<ProjectEffectEntryId>,
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
    RenameProjectEffect(ProjectEffectEntryId),
    MoveProjectEffect(ProjectEffectEntryId),
    InspectProjectEffect(ProjectEffectEntryId),
    DeleteProjectEffect(ProjectEffectEntryId),
    CreateReusableEffectFromSelection,
    ExplodeEffectClip(EffectClipId),
}

#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LibraryAssetOperationState {
    rename: Option<LibraryRenameState>,
    extraction: Option<ReusableEffectExtractionState>,
    dependency_inspector: Option<LibraryDependencyInspectorState>,
    deletion: Option<LibraryEffectDeletionState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LibraryRenameState {
    source: ProjectEffectEntryId,
    draft: String,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReusableEffectExtractionState {
    emitters: Vec<EmitterId>,
    draft: String,
    replace_selection: bool,
    error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LibraryDependencyInspectorState {
    source: ProjectEffectEntryId,
    graph: ProjectEffectUsageGraph,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LibraryEffectDeletionState {
    source: ProjectEffectEntryId,
    graph: ProjectEffectUsageGraph,
    error: Option<String>,
}

#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
struct RenderedLibraryRelationOverlay(Option<LibraryRelationOverlayView>);

#[derive(Debug, Clone, PartialEq, Eq)]
enum LibraryRelationOverlayView {
    Inspector(LibraryDependencyInspectorState),
    Deletion(LibraryEffectDeletionState),
}

impl LibraryAssetOperationState {
    pub(crate) fn is_open(&self) -> bool {
        self.rename.is_some()
            || self.extraction.is_some()
            || self.dependency_inspector.is_some()
            || self.deletion.is_some()
    }

    fn close_all(&mut self) {
        self.rename = None;
        self.extraction = None;
        self.dependency_inspector = None;
        self.deletion = None;
    }
}

#[derive(Component)]
struct LibraryAssetOperationOverlay;

#[derive(Component)]
struct LibraryRenameDialog;

#[derive(Component)]
struct LibraryRenameInput;

#[derive(Component)]
struct LibraryRenameError;

#[derive(Component)]
struct ReusableEffectExtractionDialog;

#[derive(Component)]
struct ReusableEffectExtractionInput;

#[derive(Component)]
struct ReusableEffectExtractionReplace;

#[derive(Component)]
struct ReusableEffectExtractionError;

#[derive(Component)]
struct LibraryDependencyInspectorDialog;

#[derive(Component)]
struct LibraryEffectDeletionDialog;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum LibraryAssetOperationAction {
    ConfirmRename,
    ConfirmReusableEffectExtraction,
    NavigateToEffect {
        effect: EffectAssetRef,
        clip: Option<EffectClipId>,
    },
    ConfirmEffectDeletion,
    Cancel,
}

pub(crate) fn spawn_library_asset_operation_overlay(
    parent: &mut ChildSpawnerCommands,
    state: &LibraryAssetOperationState,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) {
    let rename = state.rename.as_ref();
    let extraction = state.extraction.as_ref();
    parent
        .spawn((
            LibraryAssetOperationOverlay,
            GlobalZIndex(310),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Node {
                display: if state.is_open() {
                    Display::Flex
                } else {
                    Display::None
                },
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.005, 0.007, 0.014, 0.82)),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    LibraryRenameDialog,
                    Node {
                        display: if rename.is_some() {
                            Display::Flex
                        } else {
                            Display::None
                        },
                        width: Val::Px(440.0),
                        max_width: Val::Percent(92.0),
                        padding: UiRect::all(Val::Px(22.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(12.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(7.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                    BorderColor::all(theme::BORDER_BRIGHT),
                ))
                .with_children(|dialog| {
                    dialog.spawn((
                        Text::new(localizer.text("library-rename-dialog-title")),
                        TextFont {
                            font_size: FontSize::Px(17.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        Pickable::IGNORE,
                    ));
                    dialog.spawn((
                        Text::new(localizer.text("library-rename-dialog-description")),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        Pickable::IGNORE,
                    ));
                    let input = spawn_text_input(
                        dialog,
                        rename.map_or("", |rename| rename.draft.as_str()),
                        &localizer.text("library-rename-input"),
                        LibraryRenameInput,
                    );
                    dialog.commands().entity(input).insert(Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(32.0),
                        ..default()
                    });
                    dialog.spawn((
                        LibraryRenameError,
                        Text::new(
                            rename
                                .and_then(|rename| rename.error.as_deref())
                                .unwrap_or_default(),
                        ),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::PLAYHEAD),
                        Node {
                            display: if rename.and_then(|rename| rename.error.as_ref()).is_some() {
                                Display::Flex
                            } else {
                                Display::None
                            },
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                    dialog
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::End,
                            column_gap: Val::Px(8.0),
                            margin: UiRect::top(Val::Px(6.0)),
                            ..default()
                        })
                        .with_children(|buttons| {
                            for (message, action) in [
                                ("common-cancel", LibraryAssetOperationAction::Cancel),
                                (
                                    "library-rename-confirm",
                                    LibraryAssetOperationAction::ConfirmRename,
                                ),
                            ] {
                                let label = localizer.text(message);
                                buttons
                                    .spawn_empty()
                                    .apply_scene(ui_shell::feathers_button())
                                    .insert((
                                        action,
                                        FeathersActionButton,
                                        AccessibleLabel(label.clone()),
                                        Node {
                                            min_width: Val::Px(82.0),
                                            height: Val::Px(30.0),
                                            padding: UiRect::horizontal(Val::Px(12.0)),
                                            align_items: AlignItems::Center,
                                            justify_content: JustifyContent::Center,
                                            ..default()
                                        },
                                    ))
                                    .with_child((Text::new(label), ThemedText, Pickable::IGNORE));
                            }
                        });
                });
            overlay
                .spawn((
                    ReusableEffectExtractionDialog,
                    Node {
                        display: if extraction.is_some() {
                            Display::Flex
                        } else {
                            Display::None
                        },
                        width: Val::Px(460.0),
                        max_width: Val::Percent(92.0),
                        padding: UiRect::all(Val::Px(22.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(12.0),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(7.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                    BorderColor::all(theme::BORDER_BRIGHT),
                ))
                .with_children(|dialog| {
                    dialog.spawn((
                        Text::new(localizer.text("library-extract-dialog-title")),
                        TextFont {
                            font_size: FontSize::Px(17.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        Pickable::IGNORE,
                    ));
                    dialog.spawn((
                        Text::new(localizer.text("library-extract-dialog-description")),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        Pickable::IGNORE,
                    ));
                    let input = spawn_text_input(
                        dialog,
                        extraction.map_or("", |extraction| extraction.draft.as_str()),
                        &localizer.text("library-extract-input"),
                        ReusableEffectExtractionInput,
                    );
                    dialog.commands().entity(input).insert(Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(32.0),
                        ..default()
                    });
                    dialog
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            min_height: Val::Px(28.0),
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(9.0),
                            ..default()
                        })
                        .with_children(|row| {
                            let mut checkbox = row.spawn_empty();
                            checkbox.apply_scene(ui_shell::feathers_checkbox()).insert((
                                ReusableEffectExtractionReplace,
                                AccessibleLabel(
                                    localizer.text("library-extract-replace-selection"),
                                ),
                            ));
                            if extraction.is_none_or(|extraction| extraction.replace_selection) {
                                checkbox.insert(Checked);
                            }
                            row.spawn((
                                Text::new(localizer.text("library-extract-replace-selection")),
                                TextFont {
                                    font_size: FontSize::Px(11.0),
                                    ..default()
                                },
                                TextColor(theme::TEXT),
                                Pickable::IGNORE,
                            ));
                        });
                    dialog.spawn((
                        ReusableEffectExtractionError,
                        Text::new(
                            extraction
                                .and_then(|extraction| extraction.error.as_deref())
                                .unwrap_or_default(),
                        ),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::PLAYHEAD),
                        Node {
                            display: if extraction
                                .and_then(|extraction| extraction.error.as_ref())
                                .is_some()
                            {
                                Display::Flex
                            } else {
                                Display::None
                            },
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                    dialog
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::End,
                            column_gap: Val::Px(8.0),
                            margin: UiRect::top(Val::Px(6.0)),
                            ..default()
                        })
                        .with_children(|buttons| {
                            for (message, action) in [
                                ("common-cancel", LibraryAssetOperationAction::Cancel),
                                (
                                    "library-extract-confirm",
                                    LibraryAssetOperationAction::ConfirmReusableEffectExtraction,
                                ),
                            ] {
                                let label = localizer.text(message);
                                buttons
                                    .spawn_empty()
                                    .apply_scene(ui_shell::feathers_button())
                                    .insert((
                                        action,
                                        FeathersActionButton,
                                        AccessibleLabel(label.clone()),
                                        Node {
                                            min_width: Val::Px(82.0),
                                            height: Val::Px(30.0),
                                            padding: UiRect::horizontal(Val::Px(12.0)),
                                            align_items: AlignItems::Center,
                                            justify_content: JustifyContent::Center,
                                            ..default()
                                        },
                                    ))
                                    .with_child((Text::new(label), ThemedText, Pickable::IGNORE));
                            }
                        });
                });
            spawn_dependency_inspector_dialog(
                overlay,
                state.dependency_inspector.as_ref(),
                catalog,
                localizer,
            );
            spawn_effect_deletion_dialog(overlay, state.deletion.as_ref(), catalog, localizer);
        });
}

fn spawn_dependency_inspector_dialog(
    overlay: &mut ChildSpawnerCommands,
    inspector: Option<&LibraryDependencyInspectorState>,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) {
    let source_name = inspector
        .and_then(|inspector| catalog.entry(inspector.source))
        .map_or_else(String::new, |entry| entry.display_name.clone());
    overlay
        .spawn((
            LibraryDependencyInspectorDialog,
            Node {
                display: if inspector.is_some() {
                    Display::Flex
                } else {
                    Display::None
                },
                width: Val::Px(560.0),
                max_width: Val::Percent(92.0),
                max_height: Val::Percent(84.0),
                padding: UiRect::all(Val::Px(22.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(12.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(7.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER_BRIGHT),
        ))
        .with_children(|dialog| {
            let mut args = FluentArgs::new();
            args.set("name", source_name.as_str());
            dialog.spawn((
                Text::new(localizer.text_with("library-dependencies-title", &args)),
                TextFont {
                    font_size: FontSize::Px(17.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Pickable::IGNORE,
            ));
            dialog.spawn((
                Text::new(localizer.text("library-dependencies-description")),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::TEXT_MUTED),
                Pickable::IGNORE,
            ));
            spawn_vertical_scroll_area(
                dialog,
                ScrollMemoryKey::LibraryRelations,
                Node {
                    width: Val::Percent(100.0),
                    max_height: Val::Px(430.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(12.0),
                    ..default()
                },
                |content| {
                    if let Some(inspector) = inspector {
                        spawn_relation_section(
                            content,
                            &localizer.text("library-dependencies-uses"),
                            &inspector.graph.dependencies,
                            false,
                            catalog,
                            localizer,
                        );
                        spawn_relation_section(
                            content,
                            &localizer.text("library-dependencies-used-by"),
                            &inspector.graph.usages,
                            true,
                            catalog,
                            localizer,
                        );
                    }
                },
            );
            spawn_library_dialog_buttons(
                dialog,
                &[("common-close", LibraryAssetOperationAction::Cancel)],
                localizer,
            );
        });
}

fn spawn_relation_section(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    relations: &[ProjectEffectRelation],
    reverse: bool,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(5.0),
            ..default()
        })
        .with_children(|section| {
            section.spawn((
                Text::new(format!("{title}  ·  {}", relations.len())),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(theme::ACCENT),
                Pickable::IGNORE,
            ));
            if relations.is_empty() {
                section.spawn((
                    Text::new(localizer.text("library-dependencies-none")),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                    Pickable::IGNORE,
                ));
                return;
            }
            for relation in relations {
                let effect = if reverse {
                    relation.owner
                } else {
                    relation.dependency
                };
                let label = catalog.effect_name(effect);
                let meta = relation_meta(relation, reverse, catalog, localizer);
                let action = LibraryAssetOperationAction::NavigateToEffect {
                    effect,
                    clip: reverse.then_some(relation.clip),
                };
                section
                    .spawn_empty()
                    .apply_scene(ui_shell::feathers_button())
                    .insert((
                        action,
                        FeathersActionButton,
                        AccessibleLabel(label.clone()),
                        Node {
                            width: Val::Percent(100.0),
                            min_height: Val::Px(38.0),
                            padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::SpaceBetween,
                            column_gap: Val::Px(10.0),
                            ..default()
                        },
                    ))
                    .with_children(|row| {
                        row.spawn((Text::new(label), ThemedText, Pickable::IGNORE));
                        row.spawn((
                            Text::new(meta),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(theme::TEXT_MUTED),
                            Pickable::IGNORE,
                        ));
                    });
            }
        });
}

fn relation_meta(
    relation: &ProjectEffectRelation,
    reverse: bool,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) -> String {
    if relation.depth > 1 {
        let mut args = FluentArgs::new();
        args.set("depth", relation.depth as i64);
        return localizer.text_with("library-dependencies-indirect", &args);
    }
    if !reverse {
        return localizer.text("library-dependencies-direct");
    }
    let clip_index = catalog
        .load_effect(relation.owner)
        .ok()
        .and_then(|effect| {
            effect
                .effect_clips
                .iter()
                .position(|clip| clip.id == relation.clip)
        })
        .map_or(1, |index| index + 1);
    let mut args = FluentArgs::new();
    args.set("index", clip_index as i64);
    localizer.text_with("library-dependencies-clip", &args)
}

fn spawn_effect_deletion_dialog(
    overlay: &mut ChildSpawnerCommands,
    deletion: Option<&LibraryEffectDeletionState>,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) {
    let source_name = deletion
        .and_then(|deletion| catalog.entry(deletion.source))
        .map_or_else(String::new, |entry| entry.display_name.clone());
    overlay
        .spawn((
            LibraryEffectDeletionDialog,
            Node {
                display: if deletion.is_some() {
                    Display::Flex
                } else {
                    Display::None
                },
                width: Val::Px(520.0),
                max_width: Val::Percent(92.0),
                max_height: Val::Percent(84.0),
                padding: UiRect::all(Val::Px(22.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(12.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(7.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::PLAYHEAD),
        ))
        .with_children(|dialog| {
            let mut args = FluentArgs::new();
            args.set("name", source_name.as_str());
            dialog.spawn((
                Text::new(localizer.text_with("library-delete-title", &args)),
                TextFont {
                    font_size: FontSize::Px(17.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                Pickable::IGNORE,
            ));
            if let Some(deletion) = deletion {
                let direct = deletion.graph.direct_usages().count();
                let indirect = deletion.graph.transitive_usages().count();
                let mut args = FluentArgs::new();
                args.set("direct", direct as i64);
                args.set("indirect", indirect as i64);
                let message = if direct > 0 {
                    localizer.text_with("library-delete-referenced-warning", &args)
                } else {
                    localizer.text("library-delete-unreferenced-warning")
                };
                dialog.spawn((
                    Text::new(message),
                    TextFont {
                        font_size: FontSize::Px(11.0),
                        ..default()
                    },
                    TextColor(if direct > 0 {
                        theme::PLAYHEAD
                    } else {
                        theme::TEXT_MUTED
                    }),
                    Pickable::IGNORE,
                ));
                if direct > 0 {
                    spawn_vertical_scroll_area(
                        dialog,
                        ScrollMemoryKey::LibraryDeletion,
                        Node {
                            width: Val::Percent(100.0),
                            max_height: Val::Px(260.0),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        },
                        |content| {
                            spawn_relation_section(
                                content,
                                &localizer.text("library-dependencies-used-by"),
                                &deletion.graph.usages,
                                true,
                                catalog,
                                localizer,
                            );
                        },
                    );
                }
                if let Some(error) = deletion.error.as_deref() {
                    dialog.spawn((
                        Text::new(error),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::PLAYHEAD),
                        Pickable::IGNORE,
                    ));
                }
            }
            spawn_library_dialog_buttons(
                dialog,
                &[
                    ("common-cancel", LibraryAssetOperationAction::Cancel),
                    (
                        "library-delete-confirm",
                        LibraryAssetOperationAction::ConfirmEffectDeletion,
                    ),
                ],
                localizer,
            );
        });
}

fn spawn_library_dialog_buttons(
    parent: &mut ChildSpawnerCommands,
    buttons: &[(&str, LibraryAssetOperationAction)],
    localizer: &Localizer,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::End,
            column_gap: Val::Px(8.0),
            margin: UiRect::top(Val::Px(6.0)),
            ..default()
        })
        .with_children(|row| {
            for (message, action) in buttons {
                let label = localizer.text(message);
                row.spawn_empty()
                    .apply_scene(ui_shell::feathers_button())
                    .insert((
                        *action,
                        FeathersActionButton,
                        AccessibleLabel(label.clone()),
                        Node {
                            min_width: Val::Px(82.0),
                            height: Val::Px(30.0),
                            padding: UiRect::horizontal(Val::Px(12.0)),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                    ))
                    .with_child((Text::new(label), ThemedText, Pickable::IGNORE));
            }
        });
}

fn update_library_rename_draft(
    change: On<ValueChange<String>>,
    inputs: Query<(), With<LibraryRenameInput>>,
    mut state: ResMut<LibraryAssetOperationState>,
) {
    if !inputs.contains(change.source) {
        return;
    }
    let Some(rename) = state.rename.as_mut() else {
        return;
    };
    rename.draft.clone_from(&change.value);
    rename.error = None;
}

fn update_reusable_effect_extraction_draft(
    change: On<ValueChange<String>>,
    inputs: Query<(), With<ReusableEffectExtractionInput>>,
    mut state: ResMut<LibraryAssetOperationState>,
) {
    if !inputs.contains(change.source) {
        return;
    }
    let Some(extraction) = state.extraction.as_mut() else {
        return;
    };
    extraction.draft.clone_from(&change.value);
    extraction.error = None;
}

fn update_reusable_effect_extraction_replace(
    change: On<ValueChange<bool>>,
    controls: Query<(), With<ReusableEffectExtractionReplace>>,
    mut commands: Commands,
    mut state: ResMut<LibraryAssetOperationState>,
) {
    if !controls.contains(change.source) {
        return;
    }
    let Some(extraction) = state.extraction.as_mut() else {
        return;
    };
    extraction.replace_selection = change.value;
    extraction.error = None;
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
}

fn dismiss_library_asset_operation_with_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<LibraryAssetOperationState>,
) {
    if state.is_open() && keys.just_pressed(KeyCode::Escape) {
        state.close_all();
    }
}

fn sync_library_asset_operation_overlay(
    state: Res<LibraryAssetOperationState>,
    mut nodes: ParamSet<(
        Query<&mut Node, With<LibraryAssetOperationOverlay>>,
        Query<&mut Node, With<LibraryRenameDialog>>,
        Query<&mut Node, With<ReusableEffectExtractionDialog>>,
        Query<&mut Node, With<LibraryDependencyInspectorDialog>>,
        Query<&mut Node, With<LibraryEffectDeletionDialog>>,
        Query<(&mut Text, &mut Node), With<LibraryRenameError>>,
        Query<(&mut Text, &mut Node), With<ReusableEffectExtractionError>>,
    )>,
    rename_inputs: Query<Entity, With<LibraryRenameInput>>,
    extraction_inputs: Query<Entity, With<ReusableEffectExtractionInput>>,
    mut editable_texts: Query<(&ChildOf, &mut EditableText)>,
) {
    if !state.is_changed() {
        return;
    }
    for mut node in &mut nodes.p0() {
        node.display = if state.is_open() {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut nodes.p1() {
        node.display = if state.rename.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut nodes.p2() {
        node.display = if state.extraction.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut nodes.p3() {
        node.display = if state.dependency_inspector.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    for mut node in &mut nodes.p4() {
        node.display = if state.deletion.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (parent, mut text) in &mut editable_texts {
        if rename_inputs.contains(parent.parent())
            && let Some(rename) = state.rename.as_ref()
            && text.value() != rename.draft.as_str()
        {
            text.editor_mut().set_text(&rename.draft);
            text.queue_edit(TextEdit::TextEnd(false));
        } else if extraction_inputs.contains(parent.parent())
            && let Some(extraction) = state.extraction.as_ref()
            && text.value() != extraction.draft.as_str()
        {
            text.editor_mut().set_text(&extraction.draft);
            text.queue_edit(TextEdit::TextEnd(false));
        }
    }
    for (mut text, mut node) in &mut nodes.p5() {
        let error = state
            .rename
            .as_ref()
            .and_then(|rename| rename.error.as_ref());
        text.0 = error.cloned().unwrap_or_default();
        node.display = if error.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (mut text, mut node) in &mut nodes.p6() {
        let error = state
            .extraction
            .as_ref()
            .and_then(|extraction| extraction.error.as_ref());
        text.0 = error.cloned().unwrap_or_default();
        node.display = if error.is_some() {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn queue_library_relation_overlay_rebuild(
    state: Res<LibraryAssetOperationState>,
    mut rendered: ResMut<RenderedLibraryRelationOverlay>,
    mut session: ResMut<EditorSession>,
) {
    let view = state
        .dependency_inspector
        .clone()
        .map(LibraryRelationOverlayView::Inspector)
        .or_else(|| {
            state
                .deletion
                .clone()
                .map(LibraryRelationOverlayView::Deletion)
        });
    if rendered.0 != view {
        rendered.0 = view;
        session.ui_revision += 1;
    }
}

#[derive(Component)]
struct AssetButtonLabel;

#[derive(Component)]
struct LibrarySearchInput;

#[derive(Component)]
struct ProjectEffectDragGhost;

#[derive(Component)]
struct ProjectEffectContextMenu;

#[derive(Component)]
struct ProjectEffectContextMenuAnchor;

#[derive(Component, Clone, Copy)]
pub(crate) struct ProjectEffectRow(ProjectEffectEntryId);

impl ProjectEffectRow {
    pub(crate) fn id(self) -> ProjectEffectEntryId {
        self.0
    }
}

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

/// Hosts the Library's pointer context menu outside the hovered list row hierarchy.
#[derive(Component)]
struct LibraryContextMenuHost;

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

fn begin_project_effect_drag(
    drag: On<Pointer<DragStart>>,
    rows: Query<&ProjectEffectRow>,
    parents: Query<&ChildOf>,
    catalog: Res<ProjectEffectCatalog>,
    ghosts: Query<Entity, With<ProjectEffectDragGhost>>,
    mut commands: Commands,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Some(row) = project_effect_row_from_entity(drag.event_target(), &rows, &parents) else {
        return;
    };
    let Some(entry) = catalog.entry(row.id()) else {
        return;
    };
    for ghost in &ghosts {
        commands.entity(ghost).despawn();
    }
    let position = drag.pointer_location.position + Vec2::new(14.0, 14.0);
    commands
        .spawn((
            ProjectEffectDragGhost,
            GlobalZIndex(400),
            Pickable::IGNORE,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(position.x),
                top: Val::Px(position.y),
                max_width: Val::Px(260.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(7.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_LIGHT.with_alpha(0.97)),
            BorderColor::all(theme::ACCENT),
        ))
        .with_children(|ghost| {
            ghost
                .spawn((
                    Node {
                        width: Val::Px(18.0),
                        height: Val::Px(18.0),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(theme::ACCENT),
                    Pickable::IGNORE,
                ))
                .with_child((
                    Text::new("FX"),
                    TextFont {
                        font_size: FontSize::Px(8.0),
                        ..default()
                    },
                    TextColor(theme::PANEL_DARK),
                    Pickable::IGNORE,
                ));
            ghost.spawn((
                Text::new(&entry.display_name),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                TextLayout::no_wrap(),
                Node {
                    min_width: Val::Px(0.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                Pickable::IGNORE,
            ));
        });
}

fn project_effect_row_from_entity<'a>(
    mut entity: Entity,
    rows: &'a Query<&ProjectEffectRow>,
    parents: &Query<&ChildOf>,
) -> Option<&'a ProjectEffectRow> {
    loop {
        if let Ok(row) = rows.get(entity) {
            return Some(row);
        }
        entity = parents.get(entity).ok()?.parent();
    }
}

fn open_project_effect_context_menu(
    mut click: On<Pointer<Click>>,
    rows: Query<&ProjectEffectRow>,
    hosts: Query<(&ComputedNode, &UiGlobalTransform), With<LibraryContextMenuHost>>,
    parents: Query<&ChildOf>,
    catalog: Res<ProjectEffectCatalog>,
    mut state: ResMut<LibraryState>,
    mut session: ResMut<EditorSession>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let mut entity = click.event_target();
    let mut source = None;
    let position = loop {
        if source.is_none()
            && let Ok(row) = rows.get(entity)
        {
            let Some(entry) = catalog.entry(row.id()) else {
                return;
            };
            if !matches!(entry.status, ProjectEffectStatus::Valid) {
                return;
            }
            source = Some(row.id());
        }
        if let Ok((node, transform)) = hosts.get(entity) {
            break pointer_position_in_node(click.pointer_location.position, node, transform);
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let Some(source) = source else {
        return;
    };
    state.context_effect = Some(source);
    state.context_menu_position = position;
    session.ui_revision += 1;
    click.propagate(false);
}

fn dismiss_library_context_menu(
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    keys: Res<ButtonInput<KeyCode>>,
    surfaces: Query<&RelativeCursorPosition, With<ProjectEffectContextMenu>>,
    mut state: ResMut<LibraryState>,
    mut session: ResMut<EditorSession>,
) {
    let primary_pressed = buttons
        .as_deref()
        .is_some_and(|buttons| buttons.just_pressed(MouseButton::Left));
    if should_dismiss_pointer_context_menu(
        state.context_effect.is_some(),
        primary_pressed,
        keys.just_pressed(KeyCode::Escape),
        surfaces.iter().any(RelativeCursorPosition::cursor_over),
    ) {
        state.restore_context_effect_focus = state.context_effect;
        state.context_effect = None;
        session.ui_revision += 1;
    }
}

fn open_focused_library_context_menu(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Option<Res<InputFocus>>,
    active_descendants: Query<&ActiveDescendant>,
    rows: Query<(&ProjectEffectRow, &ComputedNode, &UiGlobalTransform)>,
    hosts: Query<(&ComputedNode, &UiGlobalTransform), With<LibraryContextMenuHost>>,
    parents: Query<&ChildOf>,
    catalog: Res<ProjectEffectCatalog>,
    mut state: ResMut<LibraryState>,
    mut session: ResMut<EditorSession>,
) {
    if !keyboard_context_menu_requested(&keys) || state.context_effect.is_some() {
        return;
    }
    let Some(mut entity) = focus.as_deref().and_then(InputFocus::get) else {
        return;
    };
    if let Ok(active) = active_descendants.get(entity)
        && let Some(descendant) = active.0
    {
        entity = descendant;
    }
    let (source, pointer_position) = loop {
        if let Ok((row, node, transform)) = rows.get(entity) {
            let Some(entry) = catalog.entry(row.id()) else {
                return;
            };
            if !matches!(entry.status, ProjectEffectStatus::Valid) {
                return;
            }
            let top_left = transform.translation.trunc() - node.size() * 0.5;
            break (
                row.id(),
                top_left + Vec2::new((node.size().x - 8.0).max(0.0), node.size().y * 0.5),
            );
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    let position = loop {
        if let Ok((node, transform)) = hosts.get(entity) {
            break pointer_position_in_node(pointer_position, node, transform);
        }
        let Ok(parent) = parents.get(entity) else {
            return;
        };
        entity = parent.parent();
    };
    state.context_effect = Some(source);
    state.context_menu_position = position;
    session.ui_revision += 1;
}

fn update_project_effect_drag(
    drag: On<Pointer<Drag>>,
    mut ghosts: Query<&mut Node, With<ProjectEffectDragGhost>>,
) {
    let position = drag.pointer_location.position + Vec2::new(14.0, 14.0);
    for mut node in &mut ghosts {
        node.left = Val::Px(position.x);
        node.top = Val::Px(position.y);
    }
}

fn end_project_effect_drag(
    _drag: On<Pointer<DragEnd>>,
    ghosts: Query<Entity, With<ProjectEffectDragGhost>>,
    mut commands: Commands,
) {
    for ghost in &ghosts {
        commands.entity(ghost).despawn();
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
    let catalog_ready = matches!(catalog.availability(), ProjectAssetIndexAvailability::Ready);
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
    let project_meta = if matches!(catalog.availability(), ProjectAssetIndexAvailability::Ready) {
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
                DocumentAction::OpenCatalog(
                    entry
                        .reference
                        .expect("a valid indexed effect has a semantic reference"),
                ),
            ),
            ProjectEffectStatus::DuplicateId { .. } => spawn_status_list_row(
                panel,
                &entry.display_name,
                Some(&source),
                ListRowStatus {
                    label: &localizer.text("library-status-duplicate-id"),
                    color: theme::ACCENT,
                },
                &accessible_label,
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
        if matches!(entry.status, ProjectEffectStatus::Valid) {
            panel
                .commands()
                .entity(row)
                .insert(EntityCursor::System(SystemCursorIcon::Grab))
                .observe(open_project_effect_context_menu);
        }
    }
    let catalog_empty = spawn_list_empty_state(
        panel,
        &localizer.text("library-empty-title"),
        &localizer.text("library-empty-message"),
        theme::TEXT_MUTED,
        if matches!(catalog.availability(), ProjectAssetIndexAvailability::Ready)
            && catalog.entries().is_empty()
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
        if matches!(catalog.availability(), ProjectAssetIndexAvailability::Ready)
            && !catalog.entries().is_empty()
            && visible_count == 0
        {
            Display::Flex
        } else {
            Display::None
        },
    );
    panel.commands().entity(no_results).insert(LibraryNoResults);
    let (unavailable_message, unavailable_tooltip) = match catalog.availability() {
        ProjectAssetIndexAvailability::Ready => {
            let mut args = FluentArgs::new();
            args.set("path", DEFAULT_PROJECT_EFFECT_ROOT);
            let message = localizer.text_with("library-unavailable-message", &args);
            (message.clone(), EditorTooltip::description(message))
        }
        ProjectAssetIndexAvailability::Unavailable { root, message } => {
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
            catalog.availability(),
            ProjectAssetIndexAvailability::Unavailable { .. }
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

fn spawn_project_effect_context_menu(
    parent: &mut ChildSpawnerCommands,
    localizer: &Localizer,
    source: ProjectEffectEntryId,
    position: Vec2,
) {
    spawn_pointer_context_menu(
        parent,
        position,
        ProjectEffectContextMenuAnchor,
        ProjectEffectContextMenu,
        |menu| {
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text("library-inspect-dependencies"),
                LibraryAction::InspectProjectEffect(source),
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text("library-rename-effect"),
                LibraryAction::RenameProjectEffect(source),
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text("library-move-effect"),
                LibraryAction::MoveProjectEffect(source),
            );
            spawn_pointer_context_menu_item(
                menu,
                &localizer.text("library-delete-effect"),
                LibraryAction::DeleteProjectEffect(source),
            );
        },
    );
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
        ProjectEffectStatus::DuplicateId {
            reference,
            ref sources,
        } => {
            let mut args = FluentArgs::new();
            args.set("name", entry.display_name.as_str());
            args.set("id", reference.id.to_string());
            args.set("count", sources.len() as i64);
            localizer.text_with("library-duplicate-id-accessible", &args)
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
        ProjectEffectStatus::DuplicateId { reference, sources } => {
            let mut args = FluentArgs::new();
            args.set("id", reference.id.to_string());
            args.set("count", sources.len() as i64);
            EditorTooltip::titled(
                localizer.text("library-status-duplicate-id"),
                localizer.text_with("library-duplicate-id-description", &args),
            )
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
        .spawn((
            LibraryContextMenuHost,
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                position_type: PositionType::Relative,
                ..default()
            },
        ))
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
            if let Some(source) = state.context_effect
                && catalog
                    .entry(source)
                    .is_some_and(|entry| matches!(entry.status, ProjectEffectStatus::Valid))
            {
                spawn_project_effect_context_menu(
                    body,
                    localizer,
                    source,
                    state.context_menu_position,
                );
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
            Option<&mut BackgroundColor>,
        ),
        (
            Changed<Interaction>,
            Or<(With<Button>, With<FeathersActionButton>)>,
        ),
    >,
    mut menu: ResMut<MenuState>,
    mut library: ResMut<LibraryState>,
    mut session: ResMut<EditorSession>,
) {
    for (entity, interaction, action, feathers, pending, mut background) in &mut interactions {
        match *interaction {
            Interaction::Hovered if feathers.is_none() => {
                if let Some(background) = background.as_deref_mut() {
                    background.0 = theme::BUTTON_HOVER;
                }
            }
            Interaction::None if feathers.is_none() => {
                if let Some(background) = background.as_deref_mut() {
                    background.0 = theme::PANEL_DARK;
                }
            }
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
                    if let Some(background) = background.as_deref_mut() {
                        background.0 = theme::ACCENT_DIM;
                    }
                }
                menu.open = None;
                menu.panels_open = false;
                if menu.tab_context.take().is_some() {
                    session.ui_revision += 1;
                }
                library.restore_context_effect_focus = library.context_effect;
                if library.context_effect.take().is_some() {
                    session.ui_revision += 1;
                }
                commands.trigger(*action);
            }
            _ => {}
        }
    }
}

fn restore_library_context_menu_focus(
    mut focus: Option<ResMut<InputFocus>>,
    rows: Query<(Entity, &ProjectEffectRow)>,
    mut state: ResMut<LibraryState>,
) {
    let Some(focus) = focus.as_deref_mut() else {
        return;
    };
    let Some(source) = state.restore_context_effect_focus else {
        return;
    };
    let Some((entity, _)) = rows.iter().find(|(_, row)| row.id() == source) else {
        return;
    };
    focus.set(entity, bevy::input_focus::FocusCause::Navigated);
    state.restore_context_effect_focus = None;
}

fn execute_library_action(
    action: On<LibraryAction>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut watch: ResMut<ProjectEffectWatchState>,
    mut operation: ResMut<LibraryAssetOperationState>,
    timeline: Option<Res<TimelineState>>,
    localizer: Res<Localizer>,
) {
    match *action {
        LibraryAction::AddSpriteMaterial => session.add_sprite_material(),
        LibraryAction::AddGridFlipbook => session.add_grid_flipbook(),
        LibraryAction::RenameProjectEffect(source) => {
            let Some(entry) = catalog.entry(source) else {
                session.status = localizer.text("library-status-source-missing");
                return;
            };
            let is_current = session
                .source_path
                .as_deref()
                .is_some_and(|path| paths_refer_to_same_source(path, &entry.path));
            if is_current && session.dirty {
                session.status = localizer.text("library-status-save-before-rename");
                return;
            }
            operation.close_all();
            operation.rename = Some(LibraryRenameState {
                source,
                draft: entry.display_name.clone(),
                error: None,
            });
        }
        LibraryAction::MoveProjectEffect(source) => {
            let Some(entry) = catalog.entry(source).cloned() else {
                session.status = localizer.text("library-status-source-missing");
                return;
            };
            let is_current = session
                .source_path
                .as_deref()
                .is_some_and(|path| paths_refer_to_same_source(path, &entry.path));
            let Some(destination) = rfd::FileDialog::new()
                .set_title(localizer.text("library-move-dialog-title"))
                .set_directory(catalog.root())
                .pick_folder()
            else {
                return;
            };
            match catalog.move_effect_source(source, &destination) {
                Ok(moved) => {
                    watch.accept_current(catalog.root());
                    if is_current {
                        session.source_path = Some(moved.path.clone());
                    }
                    session.ui_revision += 1;
                    let mut args = FluentArgs::new();
                    args.set("path", moved.path.display().to_string());
                    session.status = localizer.text_with("library-status-effect-moved", &args);
                }
                Err(error) => {
                    let mut args = FluentArgs::new();
                    args.set("message", error.to_string());
                    session.status = localizer.text_with("library-status-operation-failed", &args);
                }
            }
        }
        LibraryAction::InspectProjectEffect(source) => {
            let Some(entry) = catalog.entry(source) else {
                session.status = localizer.text("library-status-source-missing");
                return;
            };
            let Some(reference) = entry.reference else {
                session.status = localizer.text("library-status-source-unresolvable");
                return;
            };
            match catalog.effect_usage_graph(reference) {
                Ok(graph) => {
                    operation.close_all();
                    operation.dependency_inspector =
                        Some(LibraryDependencyInspectorState { source, graph });
                    session.ui_revision += 1;
                }
                Err(error) => {
                    let mut args = FluentArgs::new();
                    args.set("message", error);
                    session.status = localizer.text_with("library-status-operation-failed", &args);
                }
            }
        }
        LibraryAction::DeleteProjectEffect(source) => {
            let Some(entry) = catalog.entry(source) else {
                session.status = localizer.text("library-status-source-missing");
                return;
            };
            if session
                .source_path
                .as_deref()
                .is_some_and(|path| paths_refer_to_same_source(path, &entry.path))
            {
                session.status = localizer.text("library-status-switch-before-delete");
                return;
            }
            let Some(reference) = entry.reference else {
                session.status = localizer.text("library-status-source-unresolvable");
                return;
            };
            match catalog.effect_usage_graph(reference) {
                Ok(graph) => {
                    operation.close_all();
                    operation.deletion = Some(LibraryEffectDeletionState {
                        source,
                        graph,
                        error: None,
                    });
                    session.ui_revision += 1;
                }
                Err(error) => {
                    let mut args = FluentArgs::new();
                    args.set("message", error);
                    session.status = localizer.text_with("library-status-operation-failed", &args);
                }
            }
        }
        LibraryAction::CreateReusableEffectFromSelection => {
            let mut emitters = timeline.as_deref().map_or_else(Vec::new, |timeline| {
                timeline.selected_local_emitters(&session.effect)
            });
            if let Some(emitter) = session.selection.emitter(&session.effect)
                && (emitters.is_empty() || !emitters.contains(&emitter))
            {
                emitters.clear();
                emitters.push(emitter);
            }
            if emitters.is_empty() {
                session.status = localizer.text("library-extract-no-selection");
                return;
            }
            operation.close_all();
            operation.extraction = Some(ReusableEffectExtractionState {
                emitters,
                draft: localizer.text("library-extract-default-name"),
                replace_selection: true,
                error: None,
            });
        }
        LibraryAction::ExplodeEffectClip(clip) => {
            if let Err(error) = explode_effect_clip(clip, &catalog, &mut session, &localizer) {
                session.status = error;
            }
        }
    }
}

fn resolve_library_asset_operation(
    activate: On<Activate>,
    actions: Query<&LibraryAssetOperationAction>,
    mut commands: Commands,
    mut state: ResMut<LibraryAssetOperationState>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut watch: ResMut<ProjectEffectWatchState>,
    mut session: ResMut<EditorSession>,
    mut timeline: Option<ResMut<TimelineState>>,
    localizer: Res<Localizer>,
) {
    let Ok(action) = actions.get(activate.entity) else {
        return;
    };
    match *action {
        LibraryAssetOperationAction::Cancel => {
            state.close_all();
            return;
        }
        LibraryAssetOperationAction::NavigateToEffect { effect, clip } => {
            state.close_all();
            commands.trigger(clip.map_or(DocumentAction::OpenCatalog(effect), |clip| {
                DocumentAction::OpenCatalogClip(effect, clip)
            }));
            return;
        }
        LibraryAssetOperationAction::ConfirmEffectDeletion => {
            let Some(deletion) = state.deletion.clone() else {
                return;
            };
            let Some(reference) = catalog
                .entry(deletion.source)
                .and_then(|entry| entry.reference)
            else {
                if let Some(deletion) = state.deletion.as_mut() {
                    deletion.error = Some(localizer.text("library-status-source-missing"));
                }
                session.ui_revision += 1;
                return;
            };
            match catalog.effect_usage_graph(reference) {
                Ok(graph) if graph != deletion.graph => {
                    if let Some(deletion) = state.deletion.as_mut() {
                        deletion.graph = graph;
                        deletion.error = Some(localizer.text("library-delete-usages-changed"));
                    }
                    session.ui_revision += 1;
                    return;
                }
                Err(error) => {
                    if let Some(deletion) = state.deletion.as_mut() {
                        deletion.error = Some(error);
                    }
                    session.ui_revision += 1;
                    return;
                }
                Ok(_) => {}
            }
            match catalog.delete_effect_source(deletion.source) {
                Ok(entry) => {
                    watch.accept_current(catalog.root());
                    state.close_all();
                    session.ui_revision += 1;
                    let mut args = FluentArgs::new();
                    args.set("name", entry.display_name);
                    session.status = localizer.text_with("library-status-effect-deleted", &args);
                }
                Err(error) => {
                    if let Some(deletion) = state.deletion.as_mut() {
                        deletion.error = Some(error.to_string());
                    }
                    session.ui_revision += 1;
                }
            }
            return;
        }
        LibraryAssetOperationAction::ConfirmReusableEffectExtraction => {
            let Some(extraction) = state.extraction.clone() else {
                return;
            };
            match create_reusable_effect_from_emitters(
                &extraction,
                &mut catalog,
                &mut session,
                &localizer,
            ) {
                Ok(()) => {
                    watch.accept_current(catalog.root());
                    state.extraction = None;
                    if let Some(timeline) = timeline.as_deref_mut() {
                        timeline.clear_emitter_selection();
                    }
                }
                Err(error) => {
                    if let Some(extraction) = state.extraction.as_mut() {
                        extraction.error = Some(error);
                    }
                }
            }
            return;
        }
        LibraryAssetOperationAction::ConfirmRename => {}
    }
    let Some(rename) = state.rename.clone() else {
        return;
    };
    let Some(entry) = catalog.entry(rename.source).cloned() else {
        if let Some(rename) = state.rename.as_mut() {
            rename.error = Some(localizer.text("library-status-source-missing"));
        }
        return;
    };
    let is_current = session
        .source_path
        .as_deref()
        .is_some_and(|path| paths_refer_to_same_source(path, &entry.path));
    if is_current && session.dirty {
        if let Some(rename) = state.rename.as_mut() {
            rename.error = Some(localizer.text("library-status-save-before-rename"));
        }
        return;
    }
    match catalog.rename_effect_source(rename.source, &rename.draft) {
        Ok(renamed) => {
            watch.accept_current(catalog.root());
            if is_current {
                session.accept_external_source_rename(
                    renamed.path.clone(),
                    renamed.display_name.clone(),
                );
            } else {
                session.ui_revision += 1;
            }
            let mut args = FluentArgs::new();
            args.set("name", renamed.display_name.as_str());
            session.status = localizer.text_with("library-status-effect-renamed", &args);
            state.rename = None;
        }
        Err(error) => {
            if let Some(rename) = state.rename.as_mut() {
                rename.error = Some(error.to_string());
            }
        }
    }
}

#[derive(Debug)]
struct ReusableEffectPlan {
    effect: EffectAsset,
    selected: Vec<EmitterId>,
    clip_start: f32,
    clip_duration: f32,
}

fn reusable_effect_plan(
    owner: &EffectAsset,
    selected: &[EmitterId],
    name: &str,
) -> Result<ReusableEffectPlan, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("the reusable effect needs a name".into());
    }
    let requested = selected.iter().copied().collect::<BTreeSet<_>>();
    let ordered = normalized_choreography_order_for_effect(owner)
        .into_iter()
        .filter_map(|track| match track {
            ChoreographyTrackId::Emitter(emitter) if requested.contains(&emitter) => Some(emitter),
            _ => None,
        })
        .collect::<Vec<_>>();
    if ordered.is_empty() {
        return Err("the selected emitters no longer exist".into());
    }
    let selected = ordered.iter().copied().collect::<BTreeSet<_>>();
    if let Some(event) = owner
        .events
        .iter()
        .find(|event| selected.contains(&event.source) != selected.contains(&event.target))
    {
        return Err(format!(
            "event link {} crosses the selection boundary; select both connected emitters",
            event.id
        ));
    }
    let emitters = ordered
        .iter()
        .filter_map(|id| owner.emitters.iter().find(|emitter| emitter.id == *id))
        .collect::<Vec<_>>();
    let clip_start = emitters
        .iter()
        .map(|emitter| emitter.start_time)
        .fold(f32::INFINITY, f32::min);
    let clip_end = emitters
        .iter()
        .map(|emitter| emitter.start_time + emitter.duration)
        .fold(f32::NEG_INFINITY, f32::max);
    let clip_duration = (clip_end - clip_start).max(0.05);

    let mut effect = EffectAsset::new(name, clip_duration);
    effect.playback_mode = EffectPlaybackMode::Once;
    effect.assets.clone_from(&owner.assets);
    effect.flipbooks.clone_from(&owner.flipbooks);
    effect.materials.clone_from(&owner.materials);
    effect.parameters.clone_from(&owner.parameters);
    effect.dependencies.clone_from(&owner.dependencies);
    effect.emitters = emitters
        .into_iter()
        .cloned()
        .map(|mut emitter| {
            emitter.start_time -= clip_start;
            emitter.start_reference = None;
            emitter
        })
        .collect();
    effect.events = owner
        .events
        .iter()
        .filter(|event| selected.contains(&event.source) && selected.contains(&event.target))
        .cloned()
        .collect();
    effect.choreography_order = ordered
        .iter()
        .copied()
        .map(ChoreographyTrackId::Emitter)
        .collect();
    effect.validate().map_err(|report| report.to_string())?;
    Ok(ReusableEffectPlan {
        effect,
        selected: ordered,
        clip_start,
        clip_duration,
    })
}

fn create_reusable_effect_from_emitters(
    extraction: &ReusableEffectExtractionState,
    catalog: &mut ProjectEffectCatalog,
    session: &mut EditorSession,
    localizer: &Localizer,
) -> Result<(), String> {
    let plan = reusable_effect_plan(&session.effect, &extraction.emitters, &extraction.draft)?;
    let created = catalog
        .create_effect_source(&plan.effect)
        .map_err(|error| error.to_string())?;

    if extraction.replace_selection {
        let clip = EffectClip::new(
            EffectAssetRef::new(plan.effect.id),
            plan.clip_start,
            plan.clip_duration,
        );
        let clip_id = clip.id;
        let selected = plan.selected.iter().copied().collect::<BTreeSet<_>>();
        let mut order = normalized_choreography_order_for_effect(&session.effect);
        let insertion = order
            .iter()
            .position(|track| {
                matches!(track, ChoreographyTrackId::Emitter(emitter) if selected.contains(emitter))
            })
            .unwrap_or(order.len());
        order.retain(|track| {
            !matches!(track, ChoreographyTrackId::Emitter(emitter) if selected.contains(emitter))
        });
        order.insert(
            insertion.min(order.len()),
            ChoreographyTrackId::EffectClip(clip_id),
        );

        let mut commands = plan
            .selected
            .iter()
            .copied()
            .map(|id| EffectCommand::RemoveEmitter { id })
            .collect::<Vec<_>>();
        commands.push(EffectCommand::AddEffectClip {
            clip,
            index: session.effect.effect_clips.len(),
        });
        commands.push(EffectCommand::SetChoreographyOrder { order });
        if !session.execute_transaction(
            EffectTransaction::new(localizer.text("library-extract-command"), commands),
            true,
        ) {
            let transaction_error = session.status.clone();
            let rollback_error = fs::remove_file(&created.path).err();
            catalog.refresh();
            return Err(match rollback_error {
                Some(error) => {
                    format!("{transaction_error}; removing the new source also failed: {error}")
                }
                None => transaction_error,
            });
        }
        session.select_effect_clip(clip_id);
    } else {
        session.ui_revision += 1;
    }

    let mut args = FluentArgs::new();
    args.set("name", plan.effect.name.as_str());
    args.set("count", plan.selected.len() as i64);
    session.status = localizer.text_with("library-extract-created", &args);
    Ok(())
}

fn explode_effect_clip(
    clip_id: EffectClipId,
    catalog: &ProjectEffectCatalog,
    session: &mut EditorSession,
    localizer: &Localizer,
) -> Result<(), String> {
    let clip = session
        .effect
        .effect_clips
        .iter()
        .find(|candidate| candidate.id == clip_id)
        .cloned()
        .ok_or_else(|| localizer.text("library-explode-clip-missing"))?;
    let source = catalog.load_effect(clip.source)?;
    let source_name = source.name.clone();
    let mut exploded = ExplodedEffectContent::default();
    flatten_effect_window(
        catalog,
        &source,
        &clip.parameter_overrides,
        clip.source_offset,
        clip.duration,
        clip.start_time,
        clip.transform,
        &mut BTreeSet::new(),
        &mut exploded,
    )?;
    if exploded.emitters.is_empty() {
        return Err("the clip contains no emitters in its visible time range".into());
    }

    let first_emitter = exploded.emitters[0].id;
    let emitter_count = exploded.emitters.len();
    let mut order = normalized_choreography_order_for_effect(&session.effect);
    let insertion = order
        .iter()
        .position(|track| *track == ChoreographyTrackId::EffectClip(clip_id))
        .unwrap_or(order.len());
    order.retain(|track| *track != ChoreographyTrackId::EffectClip(clip_id));
    order.splice(
        insertion..insertion,
        exploded
            .emitters
            .iter()
            .map(|emitter| ChoreographyTrackId::Emitter(emitter.id)),
    );

    let mut commands = Vec::new();
    let asset_index = session.effect.assets.len();
    commands.extend(
        exploded
            .assets
            .into_iter()
            .enumerate()
            .map(|(offset, asset)| EffectCommand::AddAsset {
                asset,
                index: asset_index + offset,
            }),
    );
    let flipbook_index = session.effect.flipbooks.len();
    commands.extend(
        exploded
            .flipbooks
            .into_iter()
            .enumerate()
            .map(|(offset, flipbook)| EffectCommand::AddFlipbook {
                flipbook,
                index: flipbook_index + offset,
            }),
    );
    let material_index = session.effect.materials.len();
    commands.extend(
        exploded
            .materials
            .into_iter()
            .enumerate()
            .map(|(offset, material)| EffectCommand::AddMaterial {
                material,
                index: material_index + offset,
            }),
    );
    let parameter_index = session.effect.parameters.len();
    commands.extend(
        exploded
            .parameters
            .into_iter()
            .enumerate()
            .map(|(offset, parameter)| EffectCommand::AddParameter {
                parameter,
                index: parameter_index + offset,
            }),
    );
    let emitter_index = session.effect.emitters.len();
    commands.extend(
        exploded
            .emitters
            .into_iter()
            .enumerate()
            .map(|(offset, emitter)| EffectCommand::AddEmitter {
                emitter,
                index: emitter_index + offset,
            }),
    );
    let event_index = session.effect.events.len();
    commands.extend(
        exploded
            .events
            .into_iter()
            .enumerate()
            .map(|(offset, event)| EffectCommand::AddEvent {
                event,
                index: event_index + offset,
            }),
    );
    commands.push(EffectCommand::RemoveEffectClip { id: clip_id });
    commands.push(EffectCommand::SetChoreographyOrder { order });
    if !session.execute_transaction(
        EffectTransaction::new(localizer.text("library-explode-command"), commands),
        true,
    ) {
        return Err(session.status.clone());
    }
    session.select_emitter(first_emitter);

    let mut args = FluentArgs::new();
    args.set("name", source_name);
    args.set("count", emitter_count as i64);
    session.status = localizer.text_with("library-explode-created", &args);
    Ok(())
}

#[derive(Default)]
struct ExplodedEffectContent {
    assets: Vec<AssetDefinition>,
    flipbooks: Vec<FlipbookDefinition>,
    materials: Vec<MaterialDefinition>,
    parameters: Vec<EffectParameter>,
    emitters: Vec<Emitter>,
    events: Vec<EventLink>,
}

#[derive(Default)]
struct ExplodedResourceMap {
    assets: BTreeMap<AssetId, AssetId>,
    materials: BTreeMap<MaterialId, MaterialId>,
    parameters: BTreeMap<ParameterId, ParameterId>,
}

#[allow(clippy::too_many_arguments)]
fn flatten_effect_window(
    catalog: &ProjectEffectCatalog,
    source: &EffectAsset,
    overrides: &BTreeMap<ParameterId, Value>,
    window_start: f32,
    window_duration: f32,
    destination_start: f32,
    transform: EmitterTransform,
    ancestors: &mut BTreeSet<EffectId>,
    output: &mut ExplodedEffectContent,
) -> Result<(), String> {
    if !ancestors.insert(source.id) {
        return Err(format!(
            "effect reference cycle encountered at '{}'",
            source.name
        ));
    }
    let result = (|| {
        let mut resolved = source.clone();
        bake_parameter_overrides(&mut resolved, overrides)?;
        let resources = import_effect_resources(&resolved, output)?;
        let window_end = window_start + window_duration;
        let occurrences = effect_occurrences(&resolved, window_start, window_end)?;
        let mut emitter_ids = BTreeMap::<(EmitterId, i64), EmitterId>::new();

        for emitter in &resolved.emitters {
            for occurrence in &occurrences {
                let occurrence_start = emitter.start_time + *occurrence as f32 * resolved.duration;
                let occurrence_end = occurrence_start + emitter.duration;
                let visible_start = occurrence_start.max(window_start);
                let visible_end = occurrence_end.min(window_end);
                if visible_end - visible_start <= f32::EPSILON {
                    continue;
                }
                let mut local = emitter.clone();
                local.regenerate_ids();
                local.start_time = destination_start + visible_start - window_start;
                local.duration = visible_end - visible_start;
                local.transform = compose_emitter_transforms(transform, emitter.transform);
                remap_emitter_resources(&mut local, &resources)?;
                emitter_ids.insert((emitter.id, *occurrence), local.id);
                output.emitters.push(local);
            }
        }

        for event in &resolved.events {
            for occurrence in &occurrences {
                let (Some(source), Some(target)) = (
                    emitter_ids.get(&(event.source, *occurrence)),
                    emitter_ids.get(&(event.target, *occurrence)),
                ) else {
                    continue;
                };
                let mut local = event.clone();
                local.id = EventId::new();
                local.source = *source;
                local.target = *target;
                output.events.push(local);
            }
        }

        for clip in &resolved.effect_clips {
            for occurrence in &occurrences {
                let occurrence_start = clip.start_time + *occurrence as f32 * resolved.duration;
                let occurrence_end = occurrence_start + clip.duration;
                let visible_start = occurrence_start.max(window_start);
                let visible_end = occurrence_end.min(window_end);
                if visible_end - visible_start <= f32::EPSILON {
                    continue;
                }
                let child = catalog.load_effect(clip.source)?;
                flatten_effect_window(
                    catalog,
                    &child,
                    &clip.parameter_overrides,
                    clip.source_offset + visible_start - occurrence_start,
                    visible_end - visible_start,
                    destination_start + visible_start - window_start,
                    compose_emitter_transforms(transform, clip.transform),
                    ancestors,
                    output,
                )?;
            }
        }
        Ok(())
    })();
    ancestors.remove(&source.id);
    result
}

fn effect_occurrences(
    source: &EffectAsset,
    window_start: f32,
    window_end: f32,
) -> Result<Vec<i64>, String> {
    if !source.playback_mode.is_looping() {
        return Ok(vec![0]);
    }
    if !source.duration.is_finite() || source.duration <= 0.0 {
        return Err(format!("effect '{}' has an invalid duration", source.name));
    }
    let first = (window_start / source.duration).floor() as i64 - 1;
    let last = (window_end / source.duration).ceil() as i64 + 1;
    if last.saturating_sub(first) > 4096 {
        return Err("the clip spans too many loop iterations to explode safely".into());
    }
    Ok((first..=last).collect())
}

fn bake_parameter_overrides(
    effect: &mut EffectAsset,
    overrides: &BTreeMap<ParameterId, Value>,
) -> Result<(), String> {
    for (id, value) in overrides {
        let parameter = effect
            .parameters
            .iter_mut()
            .find(|parameter| parameter.id == *id)
            .ok_or_else(|| format!("override references missing source parameter {id}"))?;
        if !parameter.exposed {
            return Err(format!(
                "source parameter '{}' is not public and cannot be baked",
                parameter.name
            ));
        }
        let expected = parameter.default.value_type();
        let actual = value.value_type();
        if expected != actual {
            return Err(format!(
                "source parameter '{}' expects {expected:?}, found {actual:?}",
                parameter.name
            ));
        }
        parameter.default = value.clone();
    }
    Ok(())
}

fn import_effect_resources(
    effect: &EffectAsset,
    output: &mut ExplodedEffectContent,
) -> Result<ExplodedResourceMap, String> {
    let mut resources = ExplodedResourceMap::default();
    for asset in &effect.assets {
        resources.assets.insert(asset.id, AssetId::new());
    }
    for flipbook in &effect.flipbooks {
        resources.assets.insert(flipbook.id, AssetId::new());
    }
    for material in &effect.materials {
        resources.materials.insert(material.id, MaterialId::new());
    }
    for parameter in &effect.parameters {
        resources
            .parameters
            .insert(parameter.id, ParameterId::new());
    }

    for asset in &effect.assets {
        let mut local = asset.clone();
        local.id = mapped_asset(asset.id, &resources)?;
        output.assets.push(local);
    }
    for flipbook in &effect.flipbooks {
        let mut local = flipbook.clone();
        local.id = mapped_asset(flipbook.id, &resources)?;
        local.texture = mapped_asset(flipbook.texture, &resources)?;
        output.flipbooks.push(local);
    }
    for parameter in &effect.parameters {
        let mut local = parameter.clone();
        local.id = mapped_parameter(parameter.id, &resources)?;
        local.exposed = false;
        remap_value(&mut local.default, &resources)?;
        output.parameters.push(local);
    }
    for material in &effect.materials {
        let mut local = material.clone();
        local.id = mapped_material(material.id, &resources)?;
        let MaterialProperties::Sprite {
            softness,
            color,
            texture,
            ..
        } = &mut local.properties;
        remap_material_input(softness, &resources)?;
        if let SpriteColorSource::Value(color) = color {
            remap_material_input(color, &resources)?;
        }
        if let Some(texture) = texture {
            *texture = mapped_asset(*texture, &resources)?;
        }
        output.materials.push(local);
    }
    Ok(resources)
}

fn remap_emitter_resources(
    emitter: &mut Emitter,
    resources: &ExplodedResourceMap,
) -> Result<(), String> {
    for module in &mut emitter.modules {
        for parameter in module.bindings.values_mut() {
            *parameter = mapped_parameter(*parameter, resources)?;
        }
        if let ModuleParameters::Custom(values) = &mut module.parameters {
            for value in values.values_mut() {
                remap_value(value, resources)?;
            }
        }
    }
    for renderer in &mut emitter.renderers {
        renderer.material = mapped_material(renderer.material, resources)?;
        match &mut renderer.properties {
            RendererProperties::Flipbook { flipbook, .. } => {
                *flipbook = mapped_asset(*flipbook, resources)?;
            }
            RendererProperties::Mesh { asset } => {
                *asset = mapped_asset(*asset, resources)?;
            }
            RendererProperties::Custom(values) => {
                for value in values.values_mut() {
                    remap_value(value, resources)?;
                }
            }
            RendererProperties::Sprite | RendererProperties::Ribbon { .. } => {}
        }
    }
    Ok(())
}

fn remap_value(value: &mut Value, resources: &ExplodedResourceMap) -> Result<(), String> {
    match value {
        Value::Curve(curve) => curve.id = CurveId::new(),
        Value::Vec3Curve(curve) => {
            for axis in &mut curve.curves {
                axis.id = CurveId::new();
            }
        }
        Value::Gradient(gradient) => gradient.id = GradientId::new(),
        Value::Parameter(parameter) => *parameter = mapped_parameter(*parameter, resources)?,
        Value::Asset(asset) => *asset = mapped_asset(*asset, resources)?,
        Value::Material(material) => *material = mapped_material(*material, resources)?,
        Value::Bool(_)
        | Value::U32(_)
        | Value::Scalar(_)
        | Value::Vec2(_)
        | Value::Vec3(_)
        | Value::Vec4(_)
        | Value::Text(_)
        | Value::Range(_)
        | Value::Vec3Range(_)
        | Value::Shape(_) => {}
    }
    Ok(())
}

fn remap_material_input<T>(
    input: &mut MaterialInput<T>,
    resources: &ExplodedResourceMap,
) -> Result<(), String> {
    if let MaterialInput::Parameter(parameter) = input {
        *parameter = mapped_parameter(*parameter, resources)?;
    }
    Ok(())
}

fn mapped_asset(id: AssetId, resources: &ExplodedResourceMap) -> Result<AssetId, String> {
    resources
        .assets
        .get(&id)
        .copied()
        .ok_or_else(|| format!("source references missing asset {id}"))
}

fn mapped_material(id: MaterialId, resources: &ExplodedResourceMap) -> Result<MaterialId, String> {
    resources
        .materials
        .get(&id)
        .copied()
        .ok_or_else(|| format!("source references missing material {id}"))
}

fn mapped_parameter(
    id: ParameterId,
    resources: &ExplodedResourceMap,
) -> Result<ParameterId, String> {
    resources
        .parameters
        .get(&id)
        .copied()
        .ok_or_else(|| format!("source references missing parameter {id}"))
}

fn compose_emitter_transforms(
    parent: EmitterTransform,
    child: EmitterTransform,
) -> EmitterTransform {
    let parent_translation = Vec3::from_array(parent.translation);
    let parent_rotation = Quat::from_array(parent.rotation);
    let parent_scale = Vec3::from_array(parent.scale);
    let child_translation = Vec3::from_array(child.translation);
    let child_rotation = Quat::from_array(child.rotation);
    let child_scale = Vec3::from_array(child.scale);
    EmitterTransform {
        translation: (parent_translation + parent_rotation * (child_translation * parent_scale))
            .to_array(),
        rotation: (parent_rotation * child_rotation).normalize().to_array(),
        scale: (parent_scale * child_scale).to_array(),
    }
}

fn normalized_choreography_order_for_effect(effect: &EffectAsset) -> Vec<ChoreographyTrackId> {
    let mut seen = BTreeSet::new();
    let mut order = Vec::with_capacity(effect.effect_clips.len() + effect.emitters.len());
    for track in &effect.choreography_order {
        let exists = match *track {
            ChoreographyTrackId::EffectClip(id) => {
                effect.effect_clips.iter().any(|clip| clip.id == id)
            }
            ChoreographyTrackId::Emitter(id) => {
                effect.emitters.iter().any(|emitter| emitter.id == id)
            }
        };
        if exists && seen.insert(*track) {
            order.push(*track);
        }
    }
    for clip in &effect.effect_clips {
        let track = ChoreographyTrackId::EffectClip(clip.id);
        if seen.insert(track) {
            order.push(track);
        }
    }
    for emitter in &effect.emitters {
        let track = ChoreographyTrackId::Emitter(emitter.id);
        if seen.insert(track) {
            order.push(track);
        }
    }
    order
}

fn paths_refer_to_same_source(left: &Path, right: &Path) -> bool {
    left == right
        || fs::canonicalize(left)
            .ok()
            .zip(fs::canonicalize(right).ok())
            .is_some_and(|(left, right)| left == right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feathers::list_row::CompactListRow;
    use crate::session::blank_effect;
    use crate::test_support;
    use crate::timeline::{TimelineState, spawn_timeline};
    use aestra_bevy::EventTrigger;
    use bevy::{asset::AssetPlugin, scene::ScenePlugin, text::TextPlugin};

    #[derive(Resource, Default)]
    struct CapturedDocumentAction(Option<DocumentAction>);

    #[derive(Resource, Default)]
    struct CapturedLibraryAction(Option<LibraryAction>);

    fn capture_document_action(
        action: On<DocumentAction>,
        mut captured: ResMut<CapturedDocumentAction>,
    ) {
        captured.0 = Some(*action);
    }

    fn capture_library_action(
        action: On<LibraryAction>,
        mut captured: ResMut<CapturedLibraryAction>,
    ) {
        captured.0 = Some(*action);
    }

    fn library_action_test_app(session: EditorSession, catalog: ProjectEffectCatalog) -> App {
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<CurvesState>()
            .init_resource::<WorkspaceLayout>()
            .init_resource::<MenuState>()
            .init_resource::<ModulePaletteState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_plugins(EditorLibraryPlugin);
        app
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

    fn spawn_test_library_asset_operation_overlay(
        mut commands: Commands,
        state: Res<LibraryAssetOperationState>,
        catalog: Res<ProjectEffectCatalog>,
        localizer: Res<Localizer>,
    ) {
        commands.spawn(Node::default()).with_children(|parent| {
            spawn_library_asset_operation_overlay(parent, &state, &catalog, &localizer);
        });
    }

    fn spawn_pre_m6_acceptance_surface(
        mut commands: Commands,
        asset_server: Res<AssetServer>,
        session: Res<EditorSession>,
        catalog: Res<ProjectEffectCatalog>,
        library: Res<LibraryState>,
        timeline: Res<TimelineState>,
        registry: Res<EditorModuleRegistry>,
        curves: Res<CurvesState>,
        localizer: Res<Localizer>,
    ) {
        commands
            .spawn(Node {
                width: Val::Px(536.0),
                height: Val::Px(320.0),
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Row,
                overflow: Overflow::clip(),
                ..default()
            })
            .with_children(|root| {
                root.spawn(Node {
                    width: Val::Px(176.0),
                    min_width: Val::Px(0.0),
                    height: Val::Percent(100.0),
                    overflow: Overflow::clip(),
                    ..default()
                })
                .with_children(|panel| {
                    spawn_library(panel, &session, &catalog, &library, &localizer);
                });
                root.spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    height: Val::Percent(100.0),
                    overflow: Overflow::clip(),
                    ..default()
                })
                .with_children(|panel| {
                    spawn_timeline(
                        panel,
                        &session,
                        &timeline,
                        &catalog,
                        &registry,
                        &curves,
                        &localizer,
                        &asset_server,
                    );
                });
            });
    }

    fn marker_count<T: Component>(app: &mut App) -> usize {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<T>>();
        query.iter(world).count()
    }

    fn write_effect(path: &Path, name: &str) {
        let mut effect = test_support::effect_with_timing_slack();
        effect.name = name.into();
        effect.save_ron(path).expect("effect fixture should save");
    }

    fn test_source_id(value: u64) -> ProjectEffectEntryId {
        ProjectEffectEntryId::from_u64(value)
    }

    fn test_effect_ref(value: u128) -> EffectAssetRef {
        EffectAssetRef::new(aestra_bevy::EffectId::from_u128(value))
    }

    #[test]
    fn project_catalog_is_sorted_and_source_rows_are_stable_across_scans() {
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
            catalog.availability(),
            &ProjectAssetIndexAvailability::Ready
        );
    }

    #[test]
    fn missing_project_catalog_is_reported_as_unavailable() {
        let temporary = tempfile::tempdir().unwrap();
        let missing = temporary.path().join("missing-effects");

        let catalog = ProjectEffectCatalog::scan(&missing);

        assert!(catalog.entries().is_empty());
        assert!(matches!(
            catalog.availability(),
            ProjectAssetIndexAvailability::Unavailable { root, message }
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
        let future_source = test_support::effect_with_timing_slack()
            .to_pretty_ron()
            .unwrap()
            .replacen("format_version: 3", "format_version: 99", 1);
        fs::write(&unsupported_path, future_source).unwrap();

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
        assert_eq!(
            catalog.openable_path(valid.reference.unwrap()),
            Some(valid_path.as_path())
        );
        assert!(invalid.reference.is_none());
        assert!(unsupported.reference.is_none());
    }

    #[test]
    fn duplicate_effect_ids_are_visible_but_not_openable() {
        let temporary = tempfile::tempdir().unwrap();
        write_effect(&temporary.path().join("one.aestra.ron"), "One");
        write_effect(&temporary.path().join("two.aestra.ron"), "Two");

        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let reference = catalog.entries()[0].reference.unwrap();

        assert!(
            catalog
                .entries()
                .iter()
                .all(|entry| matches!(entry.status, ProjectEffectStatus::DuplicateId { .. }))
        );
        assert_eq!(catalog.openable_path(reference), None);
    }

    #[test]
    fn library_state_filters_project_effects_by_query_origin_and_kind() {
        let entry = ProjectEffectEntry {
            id: test_source_id(1),
            reference: Some(test_effect_ref(101)),
            display_name: "Prism Bloom".into(),
            path: PathBuf::from("assets/effects/prism_bloom.aestra.ron"),
            status: ProjectEffectStatus::Valid,
        };
        let mut state = LibraryState {
            query: "bloom".into(),
            origin: LibraryOriginFilter::Project,
            kind: LibraryKindFilter::Effect,
            ..default()
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
        let session = test_support::session_with_timing_slack();
        let effect_id = session.effect.id.to_string();
        let valid_id = test_source_id(1);
        let invalid_id = test_source_id(2);
        let unsupported_id = test_source_id(3);
        let valid_reference = test_effect_ref(102);
        let catalog = ProjectEffectCatalog::from_entries(vec![
            ProjectEffectEntry {
                id: valid_id,
                reference: Some(valid_reference),
                display_name: "Prism Bloom".into(),
                path: PathBuf::from("assets/effects/prism_bloom.aestra.ron"),
                status: ProjectEffectStatus::Valid,
            },
            ProjectEffectEntry {
                id: invalid_id,
                reference: None,
                display_name: "Broken Effect".into(),
                path: PathBuf::from("assets/effects/broken.aestra.ron"),
                status: ProjectEffectStatus::Invalid {
                    message: "Invalid RON fixture".into(),
                },
            },
            ProjectEffectEntry {
                id: unsupported_id,
                reference: None,
                display_name: "Future Effect".into(),
                path: PathBuf::from("assets/effects/future.aestra.ron"),
                status: ProjectEffectStatus::Unsupported {
                    found: 99,
                    current: aestra_bevy::CURRENT_FORMAT_VERSION,
                },
            },
        ]);
        let library = LibraryState {
            context_effect: Some(valid_id),
            context_menu_position: Vec2::new(19.0, 27.0),
            ..default()
        };
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .insert_resource(session)
        .insert_resource(catalog)
        .insert_resource(library)
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
        assert_eq!(valid.2, Some(DocumentAction::OpenCatalog(valid_reference)));
        assert!(valid.3);
        assert!(valid.4.starts_with("Open "));
        assert!(valid.4.contains("Prism Bloom"));
        assert!(valid.5);
        assert!(valid.6);
        let context_anchor = {
            let world = app.world_mut();
            let mut query =
                world.query_filtered::<(&ChildOf, &Node), With<ProjectEffectContextMenuAnchor>>();
            let (parent, node) = query.single(world).unwrap();
            (
                world
                    .get::<LibraryContextMenuHost>(parent.parent())
                    .is_some(),
                node.left,
                node.top,
            )
        };
        assert_eq!(context_anchor, (true, Val::Px(19.0), Val::Px(27.0)));
        assert_eq!(marker_count::<ProjectEffectContextMenu>(&mut app), 1);
        let menu_semantics = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<(
                Has<bevy::ui_widgets::MenuPopup>,
                &bevy::ui_widgets::MenuFocusState,
            ), With<ProjectEffectContextMenu>>();
            let (popup, focus) = query.single(world).unwrap();
            (popup, focus.clone())
        };
        assert_eq!(
            menu_semantics,
            (
                true,
                bevy::ui_widgets::MenuFocusState::Opening(
                    bevy::input_focus::tab_navigation::NavAction::First,
                ),
            )
        );
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
    fn blank_and_current_document_surfaces_compose_at_compact_width() {
        for blank in [true, false] {
            let mut session = test_support::session_with_timing_slack();
            if blank {
                session.new_effect();
            }
            let emitter_count = session.effect.emitters.len();
            let duration = session.playback_duration();
            let catalog = ProjectEffectCatalog::from_entries(vec![ProjectEffectEntry {
                id: test_source_id(1),
                reference: Some(test_effect_ref(103)),
                display_name: "Prism Bloom".into(),
                path: PathBuf::from("assets/effects/prism_bloom.aestra.ron"),
                status: ProjectEffectStatus::Valid,
            }]);
            let mut app = App::new();
            app.add_plugins((
                MinimalPlugins,
                AssetPlugin::default(),
                ScenePlugin,
                TextPlugin,
            ))
            .init_asset::<Image>()
            .init_asset::<bevy_resvg::prelude::SvgFile>()
            .insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<LibraryState>()
            .insert_resource(TimelineState::framed(duration))
            .init_resource::<EditorModuleRegistry>()
            .init_resource::<CurvesState>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_systems(Startup, spawn_pre_m6_acceptance_surface);

            app.update();

            assert_eq!(marker_count::<LibraryProjectEffectsSection>(&mut app), 1);
            assert_eq!(marker_count::<LibraryCurrentResourcesSection>(&mut app), 1);
            assert_eq!(marker_count::<ProjectEffectRow>(&mut app), 1);
            let track_headers = {
                let world = app.world_mut();
                let mut query = world.query_filtered::<
                    &ChoreographyAction,
                    (With<ListItem>, With<KeyboardNavigableListRow>),
                >();
                query
                    .iter(world)
                    .filter(|action| matches!(action, ChoreographyAction::SelectEmitter(_)))
                    .count()
            };
            assert_eq!(track_headers, emitter_count);
            assert!(marker_count::<CompactListRow>(&mut app) >= 1);
        }
    }

    #[test]
    fn list_value_change_activates_the_entry_semantic_action() {
        let mut app = App::new();
        app.init_resource::<CapturedDocumentAction>()
            .add_observer(activate_library_list_entry)
            .add_observer(capture_document_action);
        let list = app.world_mut().spawn(KeyboardNavigableList).id();
        let id = test_source_id(42);
        let reference = test_effect_ref(104);
        let row = app
            .world_mut()
            .spawn((ProjectEffectRow(id), DocumentAction::OpenCatalog(reference)))
            .id();

        app.world_mut().trigger(ValueChange::<Entity> {
            source: list,
            value: row,
            is_final: true,
        });
        app.update();

        assert_eq!(
            app.world().resource::<CapturedDocumentAction>().0,
            Some(DocumentAction::OpenCatalog(reference))
        );
    }

    #[test]
    fn current_resource_projection_tracks_new_open_undo_and_redo() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("library-projection.aestra.ron");
        let mut session = test_support::session_with_timing_slack();
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
        let session = test_support::session_with_timing_slack();
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
        let first_id = test_source_id(1);
        let second_id = test_source_id(2);
        let catalog = ProjectEffectCatalog::from_entries(vec![
            ProjectEffectEntry {
                id: first_id,
                reference: Some(test_effect_ref(105)),
                display_name: "Prism Bloom".into(),
                path: PathBuf::from("assets/effects/prism_bloom.aestra.ron"),
                status: ProjectEffectStatus::Valid,
            },
            ProjectEffectEntry {
                id: second_id,
                reference: Some(test_effect_ref(106)),
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
            *catalog = ProjectEffectCatalog::scan("missing-test-effects");
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
    fn project_catalog_rejects_self_and_transitive_effect_cycles() {
        let temporary = tempfile::tempdir().unwrap();
        let mut owner = test_support::effect_with_timing_slack();
        owner.id = aestra_bevy::EffectId::from_u128(0xa11ce);
        owner.name = "Owner".into();
        owner.effect_clips.clear();
        let mut child = test_support::effect_with_timing_slack();
        child.id = aestra_bevy::EffectId::from_u128(0xc41d);
        child.name = "Child".into();
        child.effect_clips = vec![aestra_bevy::EffectClip::new(
            EffectAssetRef::new(owner.id),
            0.0,
            0.5,
        )];
        owner
            .save_ron(temporary.path().join("owner.aestra.ron"))
            .unwrap();
        child
            .save_ron(temporary.path().join("child.aestra.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(temporary.path());

        let self_error = catalog
            .effect_for_placement(&owner, EffectAssetRef::new(owner.id))
            .unwrap_err();
        assert!(self_error.contains("cannot reference itself"));
        let cycle_error = catalog
            .effect_for_placement(&owner, EffectAssetRef::new(child.id))
            .unwrap_err();
        assert!(cycle_error.contains("reference cycle"));
    }

    #[test]
    fn missing_project_references_are_projected_into_editor_diagnostics() {
        let temporary = tempfile::tempdir().unwrap();
        let mut owner = test_support::effect_with_timing_slack();
        owner.id = aestra_bevy::EffectId::from_u128(0xa11ce);
        owner.effect_clips = vec![aestra_bevy::EffectClip::new(
            EffectAssetRef::new(aestra_bevy::EffectId::from_u128(0xdead)),
            0.25,
            0.75,
        )];
        let catalog = ProjectEffectCatalog::scan(temporary.path());

        let report = catalog.dependency_validation_report(&owner);

        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].code, DiagnosticCode::InvalidReference);
        assert_eq!(report.diagnostics[0].path, "effect.effect_clips[0].source");
        assert!(!report.diagnostics[0].message.is_empty());
    }

    #[test]
    fn library_plugin_owns_catalog_and_panel_actions() {
        let session = test_support::session_with_timing_slack();
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
    fn context_menu_library_actions_dispatch_without_a_background_component() {
        let clip = EffectClipId::new();
        let mut app = App::new();
        app.insert_resource(test_support::session_with_timing_slack())
            .init_resource::<MenuState>()
            .init_resource::<LibraryState>()
            .init_resource::<CapturedLibraryAction>()
            .add_observer(queue_library_action_activation)
            .add_observer(capture_library_action)
            .add_systems(Update, handle_library_action_buttons);
        let item = app
            .world_mut()
            .spawn((
                Interaction::None,
                LibraryAction::ExplodeEffectClip(clip),
                FeathersActionButton,
            ))
            .id();

        app.world_mut().trigger(Activate { entity: item });
        app.update();

        assert_eq!(
            app.world().resource::<CapturedLibraryAction>().0,
            Some(LibraryAction::ExplodeEffectClip(clip))
        );
    }

    #[test]
    fn library_rename_keeps_the_open_document_clean_and_updates_its_source_path() {
        let temporary = tempfile::tempdir().unwrap();
        let original = temporary.path().join("original.aestra.ron");
        let session = test_support::session_with_source_path(&original);
        session.effect.save_ron(&original).unwrap();
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let source = catalog.entries()[0].id;
        let reference = catalog.entries()[0].reference.unwrap();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<CurvesState>()
            .init_resource::<WorkspaceLayout>()
            .init_resource::<MenuState>()
            .init_resource::<ModulePaletteState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_plugins(EditorLibraryPlugin);

        app.world_mut()
            .trigger(LibraryAction::RenameProjectEffect(source));
        app.world_mut()
            .resource_mut::<LibraryAssetOperationState>()
            .rename
            .as_mut()
            .unwrap()
            .draft = "Renamed Effect".into();
        let confirm = app
            .world_mut()
            .spawn(LibraryAssetOperationAction::ConfirmRename)
            .id();
        app.world_mut().trigger(Activate { entity: confirm });

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.name, "Renamed Effect");
        assert_eq!(
            session.source_path.as_deref().unwrap().file_name().unwrap(),
            "renamed_effect.aestra.ron"
        );
        assert!(!session.dirty);
        assert!(!original.exists());
        let catalog = app.world().resource::<ProjectEffectCatalog>();
        assert_eq!(
            catalog.openable_path(reference),
            session.source_path.as_deref()
        );
        assert!(
            !app.world()
                .resource::<LibraryAssetOperationState>()
                .is_open()
        );
    }

    #[test]
    fn library_rename_requires_saving_when_the_source_is_open_and_dirty() {
        let temporary = tempfile::tempdir().unwrap();
        let original = temporary.path().join("original.aestra.ron");
        let mut session = test_support::session_with_source_path(&original);
        session.effect.save_ron(&original).unwrap();
        session.dirty = true;
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let source = catalog.entries()[0].id;
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<CurvesState>()
            .init_resource::<WorkspaceLayout>()
            .init_resource::<MenuState>()
            .init_resource::<ModulePaletteState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_plugins(EditorLibraryPlugin);

        app.world_mut()
            .trigger(LibraryAction::RenameProjectEffect(source));

        assert!(
            !app.world()
                .resource::<LibraryAssetOperationState>()
                .is_open()
        );
        assert!(
            app.world()
                .resource::<EditorSession>()
                .status
                .contains("Save")
        );
        assert!(original.exists());
    }

    #[test]
    fn reusable_effect_extraction_replaces_selected_emitters_and_is_undoable() {
        let temporary = tempfile::tempdir().unwrap();
        let mut owner = EffectAsset::new("Owner", 4.0);
        owner.playback_mode = EffectPlaybackMode::Once;
        let mut first = Emitter::basic_sprite("First", 1.0);
        first.start_time = 0.5;
        let first_id = first.id;
        let mut second = Emitter::basic_sprite("Second", 0.75);
        second.start_time = 1.25;
        let second_id = second.id;
        let mut untouched = Emitter::basic_sprite("Untouched", 1.0);
        untouched.start_time = 2.5;
        let untouched_id = untouched.id;
        owner.emitters = vec![first, second, untouched];
        owner.events.push(EventLink {
            id: EventId::new(),
            source: first_id,
            trigger: EventTrigger::OnDeath,
            target: second_id,
        });
        owner.choreography_order = vec![
            ChoreographyTrackId::Emitter(first_id),
            ChoreographyTrackId::Emitter(second_id),
            ChoreographyTrackId::Emitter(untouched_id),
        ];
        let mut session = test_support::session_from_effect_with_source_path(
            owner,
            temporary.path().join("owner.aestra.ron"),
        );
        let mut catalog = ProjectEffectCatalog::scan(temporary.path());
        let localizer = Localizer::new("en-US").unwrap();
        let extraction = ReusableEffectExtractionState {
            emitters: vec![first_id, second_id],
            draft: "Prismatic Burst".into(),
            replace_selection: true,
            error: None,
        };

        create_reusable_effect_from_emitters(&extraction, &mut catalog, &mut session, &localizer)
            .unwrap();

        let created_path = temporary.path().join("prismatic_burst.aestra.ron");
        let created = EffectAsset::load_ron(&created_path).unwrap();
        assert_eq!(created.name, "Prismatic Burst");
        assert_eq!(created.duration, 1.5);
        assert_eq!(created.playback_mode, EffectPlaybackMode::Once);
        assert_eq!(created.emitters.len(), 2);
        assert_eq!(created.emitters[0].start_time, 0.0);
        assert_eq!(created.emitters[1].start_time, 0.75);
        assert_eq!(created.events.len(), 1);
        assert_eq!(session.effect.emitters.len(), 1);
        assert_eq!(session.effect.emitters[0].id, untouched_id);
        assert_eq!(session.effect.effect_clips.len(), 1);
        let clip = &session.effect.effect_clips[0];
        assert_eq!(clip.source, EffectAssetRef::new(created.id));
        assert_eq!(clip.start_time, 0.5);
        assert_eq!(clip.duration, 1.5);
        assert_eq!(
            session.effect.choreography_order,
            vec![
                ChoreographyTrackId::EffectClip(clip.id),
                ChoreographyTrackId::Emitter(untouched_id),
            ]
        );
        assert!(session.can_undo());

        session.undo();
        assert_eq!(session.effect.emitters.len(), 3);
        assert!(session.effect.effect_clips.is_empty());
        assert_eq!(session.effect.events.len(), 1);
        assert!(
            created_path.exists(),
            "undo keeps the reusable asset available"
        );
    }

    #[test]
    fn reusable_effect_extraction_rejects_cross_boundary_event_links() {
        let mut owner = EffectAsset::new("Owner", 2.0);
        let first = Emitter::basic_sprite("First", 1.0);
        let first_id = first.id;
        let second = Emitter::basic_sprite("Second", 1.0);
        let second_id = second.id;
        owner.emitters = vec![first, second];
        owner.events.push(EventLink {
            id: EventId::new(),
            source: first_id,
            trigger: EventTrigger::OnSpawn,
            target: second_id,
        });

        let error = reusable_effect_plan(&owner, &[first_id], "Partial").unwrap_err();

        assert!(error.contains("crosses the selection boundary"));
    }

    #[test]
    fn reusable_effect_extraction_overlay_tracks_modal_state_without_query_conflicts() {
        let state = LibraryAssetOperationState {
            extraction: Some(ReusableEffectExtractionState {
                emitters: vec![EmitterId::new()],
                draft: "Reusable Burst".into(),
                replace_selection: true,
                error: Some("Choose another name".into()),
            }),
            ..default()
        };
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .insert_resource(state)
        .insert_resource(ProjectEffectCatalog::from_entries(Vec::new()))
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_systems(Startup, spawn_test_library_asset_operation_overlay)
        .add_systems(Update, sync_library_asset_operation_overlay);

        app.update();

        let world = app.world_mut();
        let overlay = world
            .query_filtered::<&Node, With<LibraryAssetOperationOverlay>>()
            .single(world)
            .unwrap();
        assert_eq!(overlay.display, Display::Flex);
        let extraction = world
            .query_filtered::<&Node, With<ReusableEffectExtractionDialog>>()
            .single(world)
            .unwrap();
        assert_eq!(extraction.display, Display::Flex);
        let rename = world
            .query_filtered::<&Node, With<LibraryRenameDialog>>()
            .single(world)
            .unwrap();
        assert_eq!(rename.display, Display::None);
        let (error, error_node) = world
            .query_filtered::<(&Text, &Node), With<ReusableEffectExtractionError>>()
            .single(world)
            .unwrap();
        assert_eq!(error.0, "Choose another name");
        assert_eq!(error_node.display, Display::Flex);

        world
            .resource_mut::<LibraryAssetOperationState>()
            .extraction = None;
        app.update();

        let world = app.world_mut();
        let overlay = world
            .query_filtered::<&Node, With<LibraryAssetOperationOverlay>>()
            .single(world)
            .unwrap();
        assert_eq!(overlay.display, Display::None);
        let extraction = world
            .query_filtered::<&Node, With<ReusableEffectExtractionDialog>>()
            .single(world)
            .unwrap();
        assert_eq!(extraction.display, Display::None);
    }

    #[test]
    fn relation_overlay_change_queues_one_follow_up_shell_rebuild() {
        let source = test_source_id(701);
        let state = LibraryAssetOperationState {
            dependency_inspector: Some(LibraryDependencyInspectorState {
                source,
                graph: ProjectEffectUsageGraph::default(),
            }),
            ..default()
        };
        let session = test_support::session_with_timing_slack();
        let initial_revision = session.ui_revision;
        let mut app = App::new();
        app.insert_resource(state)
            .init_resource::<RenderedLibraryRelationOverlay>()
            .insert_resource(session)
            .add_systems(Update, queue_library_relation_overlay_rebuild);

        app.update();
        assert_eq!(
            app.world().resource::<EditorSession>().ui_revision,
            initial_revision + 1
        );
        app.update();
        assert_eq!(
            app.world().resource::<EditorSession>().ui_revision,
            initial_revision + 1,
            "an unchanged relation view must not rebuild every frame"
        );
    }

    #[test]
    fn explode_replaces_the_clip_with_editable_emitters_and_is_undoable() {
        let temporary = tempfile::tempdir().unwrap();
        let child_path = temporary.path().join("child.aestra.ron");
        let mut child = EffectAsset::new("Child", 2.0);
        let texture = AssetDefinition::texture("Child Texture", "textures/child.png");
        let texture_id = texture.id;
        child.assets.push(texture);
        let MaterialProperties::Sprite { texture, .. } = &mut child.materials[0].properties;
        *texture = Some(texture_id);
        child
            .emitters
            .push(Emitter::basic_sprite("First", child.duration));
        let parameter = aestra_bevy::ParameterId::new();
        child.parameters.push(aestra_bevy::EffectParameter {
            id: parameter,
            name: "Intensity".into(),
            default: Value::Scalar(1.0),
            exposed: true,
        });
        child.emitters[0]
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == aestra_bevy::MODULE_EMISSION)
            .unwrap()
            .bindings
            .insert("spawn_rate".into(), parameter);
        let mut second = child.emitters[0].clone();
        second.regenerate_ids();
        second.name = "Second".into();
        second.transform.translation = [1.0, 0.0, 0.0];
        child.emitters.push(second);
        child.save_ron(&child_path).unwrap();
        let child_reference = EffectAssetRef::new(child.id);

        let owner = blank_effect();
        let mut session = test_support::session_from_effect_with_source_path(
            owner,
            temporary.path().join("owner.aestra.ron"),
        );
        let mut clip = aestra_bevy::EffectClip::new(child_reference, 0.25, 1.0);
        let clip_id = clip.id;
        clip.transform.translation = [2.0, 0.0, 0.0];
        clip.parameter_overrides
            .insert(parameter, Value::Scalar(3.5));
        session.effect.effect_clips.push(clip.clone());
        session.effect.choreography_order = vec![ChoreographyTrackId::EffectClip(clip_id)];
        let original_emitter_count = session.effect.emitters.len();
        let original_asset_count = session.effect.assets.len();
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let catalog_entries = catalog.entries().len();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<CurvesState>()
            .init_resource::<WorkspaceLayout>()
            .init_resource::<MenuState>()
            .init_resource::<ModulePaletteState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_plugins(EditorLibraryPlugin);

        app.world_mut()
            .trigger(LibraryAction::ExplodeEffectClip(clip_id));

        let session = app.world().resource::<EditorSession>();
        assert!(
            !session
                .effect
                .effect_clips
                .iter()
                .any(|candidate| candidate.id == clip_id),
            "{}",
            session.status
        );
        let local_emitters = &session.effect.emitters[original_emitter_count..];
        assert_eq!(local_emitters.len(), 2);
        assert_eq!(local_emitters[0].name, "First");
        assert_eq!(local_emitters[1].name, "Second");
        assert_eq!(local_emitters[0].start_time, clip.start_time);
        assert_eq!(local_emitters[0].duration, clip.duration);
        assert_eq!(local_emitters[0].transform.translation, [2.0, 0.0, 0.0]);
        assert_eq!(local_emitters[1].transform.translation, [3.0, 0.0, 0.0]);
        assert_eq!(session.effect.assets.len(), original_asset_count + 1);
        assert_ne!(session.effect.assets.last().unwrap().id, texture_id);
        let local_parameter = session.effect.parameters.last().unwrap();
        assert_ne!(local_parameter.id, parameter);
        assert_eq!(local_parameter.default, Value::Scalar(3.5));
        assert!(!local_parameter.exposed);
        assert_eq!(
            local_emitters[0]
                .modules
                .iter()
                .find(|module| module.module_type.0 == aestra_bevy::MODULE_EMISSION)
                .unwrap()
                .bindings["spawn_rate"],
            local_parameter.id
        );
        assert_eq!(
            session.effect.choreography_order[0],
            ChoreographyTrackId::Emitter(local_emitters[0].id)
        );
        assert!(
            session.status.contains("editable emitters"),
            "{}",
            session.status
        );
        assert!(
            !app.world()
                .resource::<LibraryAssetOperationState>()
                .is_open()
        );
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .entries()
                .len(),
            catalog_entries
        );
        assert!(child_path.exists());

        assert!(app.world().resource::<EditorSession>().can_undo());
        app.world_mut().resource_mut::<EditorSession>().undo();
        assert!(
            app.world()
                .resource::<EditorSession>()
                .status
                .starts_with("Undid"),
            "{}",
            app.world().resource::<EditorSession>().status
        );
        let session = app.world().resource::<EditorSession>();
        let restored = session
            .effect
            .effect_clips
            .iter()
            .find(|candidate| candidate.id == clip_id)
            .unwrap();
        assert_eq!(restored.source, child_reference);
        assert_eq!(restored.parameter_overrides[&parameter], Value::Scalar(3.5));
        assert_eq!(session.effect.emitters.len(), original_emitter_count);
        assert_eq!(session.effect.assets.len(), original_asset_count);
        assert_eq!(
            session.effect.choreography_order,
            vec![ChoreographyTrackId::EffectClip(clip_id)]
        );
    }

    #[test]
    fn baking_rejects_invalid_overrides_instead_of_silently_discarding_them() {
        let mut child = EffectAsset::new("Child", 1.0);
        let parameter = aestra_bevy::ParameterId::new();
        child.parameters.push(aestra_bevy::EffectParameter {
            id: parameter,
            name: "Count".into(),
            default: Value::U32(2),
            exposed: true,
        });
        let mut clip = aestra_bevy::EffectClip::new(child.id, 0.0, 1.0);
        clip.parameter_overrides
            .insert(parameter, Value::Scalar(2.0));

        let error = bake_parameter_overrides(&mut child, &clip.parameter_overrides).unwrap_err();

        assert!(error.contains("expects U32, found Scalar"));
        assert_eq!(child.parameters[0].default, Value::U32(2));
    }

    #[test]
    fn exploding_recursively_materializes_nested_clip_emitters() {
        let temporary = tempfile::tempdir().unwrap();
        let mut grandchild = EffectAsset::new("Grandchild", 1.0);
        let mut nested_emitter = Emitter::basic_sprite("Nested", 1.0);
        nested_emitter.transform.translation = [1.0, 0.0, 0.0];
        grandchild.emitters.push(nested_emitter);
        grandchild
            .save_ron(temporary.path().join("grandchild.aestra.ron"))
            .unwrap();

        let mut child = EffectAsset::new("Child", 2.0);
        let mut nested_clip = aestra_bevy::EffectClip::new(grandchild.id, 0.5, 1.0);
        nested_clip.transform.translation = [2.0, 0.0, 0.0];
        child.effect_clips.push(nested_clip);
        child
            .save_ron(temporary.path().join("child.aestra.ron"))
            .unwrap();

        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let mut output = ExplodedEffectContent::default();
        let parent_transform = EmitterTransform {
            translation: [3.0, 0.0, 0.0],
            ..default()
        };
        flatten_effect_window(
            &catalog,
            &child,
            &BTreeMap::new(),
            0.0,
            2.0,
            0.25,
            parent_transform,
            &mut BTreeSet::new(),
            &mut output,
        )
        .unwrap();

        assert_eq!(output.emitters.len(), 1);
        assert_eq!(output.emitters[0].name, "Nested");
        assert_eq!(output.emitters[0].start_time, 0.75);
        assert_eq!(output.emitters[0].duration, 1.0);
        assert_eq!(output.emitters[0].transform.translation, [6.0, 0.0, 0.0]);
    }

    #[test]
    fn project_effect_watch_requires_two_stable_observations() {
        let temporary = tempfile::tempdir().unwrap();
        let initial = ProjectEffectTreeSnapshot::scan(temporary.path());
        let mut watch = ProjectEffectWatchState {
            poll: Timer::from_seconds(PROJECT_EFFECT_POLL_INTERVAL_SECONDS, TimerMode::Repeating),
            committed: initial,
            pending: None,
        };
        write_effect(&temporary.path().join("new.aestra.ron"), "New");
        let changed = ProjectEffectTreeSnapshot::scan(temporary.path());

        assert!(!watch.observe(changed.clone()));
        assert!(watch.observe(changed.clone()));
        assert!(!watch.observe(changed));
    }

    #[test]
    fn catalog_refresh_reloads_a_clean_open_source() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("open.aestra.ron");
        let session = test_support::session_with_source_path(&path);
        session.effect.save_ron(&path).unwrap();
        let mut catalog = ProjectEffectCatalog::scan(temporary.path());
        let previous = ProjectEffectTreeSnapshot::scan(temporary.path());
        let mut changed = EffectAsset::load_ron(&path).unwrap();
        changed.name = "Externally Renamed".into();
        changed.save_ron(&path).unwrap();
        let current = ProjectEffectTreeSnapshot::scan(temporary.path());
        let mut session = session;

        apply_project_effect_catalog_refresh(
            &mut catalog,
            &mut session,
            &previous,
            &current,
            &Localizer::new("en-US").unwrap(),
        );

        assert_eq!(session.effect.name, "Externally Renamed");
        assert!(!session.dirty);
        assert!(session.status.contains("Reloaded externally changed"));
        assert_eq!(
            catalog
                .load_effect(EffectAssetRef::new(changed.id))
                .unwrap()
                .name,
            "Externally Renamed"
        );
    }

    #[test]
    fn catalog_refresh_preserves_dirty_edits_when_the_source_changes() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("open.aestra.ron");
        let mut session = test_support::session_with_source_path(&path);
        session.effect.save_ron(&path).unwrap();
        session.effect.name = "Unsaved Editor Name".into();
        session.dirty = true;
        let mut catalog = ProjectEffectCatalog::scan(temporary.path());
        let previous = ProjectEffectTreeSnapshot::scan(temporary.path());
        let mut changed = EffectAsset::load_ron(&path).unwrap();
        changed.name = "External Name".into();
        changed.save_ron(&path).unwrap();
        let current = ProjectEffectTreeSnapshot::scan(temporary.path());

        apply_project_effect_catalog_refresh(
            &mut catalog,
            &mut session,
            &previous,
            &current,
            &Localizer::new("en-US").unwrap(),
        );

        assert_eq!(session.effect.name, "Unsaved Editor Name");
        assert!(session.dirty);
        assert!(
            session
                .status
                .contains("unsaved editor changes were preserved")
        );
        assert_eq!(
            catalog
                .load_effect(EffectAssetRef::new(changed.id))
                .unwrap()
                .name,
            "External Name"
        );
    }

    #[test]
    fn catalog_refresh_does_not_treat_an_editor_save_as_an_external_reload() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("open.aestra.ron");
        let mut session = test_support::session_with_source_path(&path);
        session.effect.save_ron(&path).unwrap();
        let mut catalog = ProjectEffectCatalog::scan(temporary.path());
        let previous = ProjectEffectTreeSnapshot::scan(temporary.path());
        session.effect.name = "Saved In Editor".into();
        session.dirty = true;
        session.save().unwrap();
        session.status = "Saved by editor".into();
        let current = ProjectEffectTreeSnapshot::scan(temporary.path());

        apply_project_effect_catalog_refresh(
            &mut catalog,
            &mut session,
            &previous,
            &current,
            &Localizer::new("en-US").unwrap(),
        );

        assert_eq!(session.effect.name, "Saved In Editor");
        assert!(!session.dirty);
        assert_eq!(session.status, "Saved by editor");
    }

    #[test]
    fn catalog_refresh_tracks_an_externally_moved_dirty_source_by_effect_id() {
        let temporary = tempfile::tempdir().unwrap();
        let nested = temporary.path().join("nested");
        fs::create_dir(&nested).unwrap();
        let original = temporary.path().join("open.aestra.ron");
        let moved = nested.join("open.aestra.ron");
        let mut session = test_support::session_with_source_path(&original);
        session.effect.save_ron(&original).unwrap();
        session.dirty = true;
        let mut catalog = ProjectEffectCatalog::scan(temporary.path());
        let previous = ProjectEffectTreeSnapshot::scan(temporary.path());
        fs::rename(&original, &moved).unwrap();
        let current = ProjectEffectTreeSnapshot::scan(temporary.path());

        apply_project_effect_catalog_refresh(
            &mut catalog,
            &mut session,
            &previous,
            &current,
            &Localizer::new("en-US").unwrap(),
        );

        assert_eq!(session.source_path.as_deref(), Some(moved.as_path()));
        assert!(session.dirty);
        assert!(session.status.contains("moved to"));
    }

    #[test]
    fn dependency_inspector_reports_actionable_reverse_clip_usages() {
        let temporary = tempfile::tempdir().unwrap();
        let child_path = temporary.path().join("child.aestra.ron");
        let mut child = EffectAsset::new("Child", 1.0);
        child.id = EffectId::from_u128(0xD01);
        child.save_ron(&child_path).unwrap();
        let owner_path = temporary.path().join("owner.aestra.ron");
        let mut owner = EffectAsset::new("Owner", 1.0);
        owner.id = EffectId::from_u128(0xD02);
        let clip = EffectClip::new(child.id, 0.0, 1.0);
        let clip_id = clip.id;
        owner.effect_clips.push(clip);
        owner.save_ron(&owner_path).unwrap();

        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let source = catalog
            .entries()
            .iter()
            .find(|entry| entry.reference == Some(child.id.into()))
            .unwrap()
            .id;
        let session = test_support::session_with_timing_slack();
        let mut app = library_action_test_app(session, catalog);

        app.world_mut()
            .trigger(LibraryAction::InspectProjectEffect(source));

        let inspector = app
            .world()
            .resource::<LibraryAssetOperationState>()
            .dependency_inspector
            .as_ref()
            .unwrap();
        let usage = inspector.graph.direct_usages().next().unwrap();
        assert_eq!(usage.owner.id, owner.id);
        assert_eq!(usage.clip, clip_id);
    }

    #[test]
    fn dependency_navigation_opens_and_selects_the_exact_owner_clip() {
        let session = test_support::session_with_timing_slack();
        let catalog = ProjectEffectCatalog::from_entries(Vec::new());
        let owner = EffectAssetRef::new(EffectId::from_u128(0xD11));
        let clip = EffectClipId::new();
        let mut app = library_action_test_app(session, catalog);
        app.init_resource::<CapturedDocumentAction>()
            .add_observer(capture_document_action);
        let action = app
            .world_mut()
            .spawn(LibraryAssetOperationAction::NavigateToEffect {
                effect: owner,
                clip: Some(clip),
            })
            .id();

        app.world_mut().trigger(Activate { entity: action });
        app.world_mut().flush();

        assert_eq!(
            app.world().resource::<CapturedDocumentAction>().0,
            Some(DocumentAction::OpenCatalogClip(owner, clip))
        );
    }

    #[test]
    fn confirmed_effect_deletion_removes_the_source_after_showing_usages() {
        let temporary = tempfile::tempdir().unwrap();
        let child_path = temporary.path().join("child.aestra.ron");
        let mut child = EffectAsset::new("Child", 1.0);
        child.id = EffectId::from_u128(0xD21);
        child.save_ron(&child_path).unwrap();
        let mut owner = EffectAsset::new("Owner", 1.0);
        owner.id = EffectId::from_u128(0xD22);
        owner.effect_clips.push(EffectClip::new(child.id, 0.0, 1.0));
        owner
            .save_ron(temporary.path().join("owner.aestra.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let source = catalog
            .entries()
            .iter()
            .find(|entry| entry.reference == Some(child.id.into()))
            .unwrap()
            .id;
        let session = test_support::session_with_timing_slack();
        let mut app = library_action_test_app(session, catalog);

        app.world_mut()
            .trigger(LibraryAction::DeleteProjectEffect(source));
        assert_eq!(
            app.world()
                .resource::<LibraryAssetOperationState>()
                .deletion
                .as_ref()
                .unwrap()
                .graph
                .direct_usages()
                .count(),
            1
        );
        let confirm = app
            .world_mut()
            .spawn(LibraryAssetOperationAction::ConfirmEffectDeletion)
            .id();
        app.world_mut().trigger(Activate { entity: confirm });

        assert!(!child_path.exists());
        assert!(
            !app.world()
                .resource::<LibraryAssetOperationState>()
                .is_open()
        );
        assert!(
            app.world()
                .resource::<EditorSession>()
                .status
                .contains("Deleted")
        );
    }

    #[test]
    fn library_plugin_preserves_an_injected_project_catalog() {
        let session = test_support::session_with_timing_slack();
        let expected_id = test_source_id(42);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(ProjectEffectCatalog::from_entries(vec![
                ProjectEffectEntry {
                    id: expected_id,
                    reference: Some(test_effect_ref(107)),
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
