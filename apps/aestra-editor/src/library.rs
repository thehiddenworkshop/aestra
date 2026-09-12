//! Transitional Library workspace and panel-local authoring adapters.
mod background;
use crate::effect_authoring::{create_reusable_effect_from_emitters, explode_effect_clip};

use crate::feathers::context_menu::{
    keyboard_context_menu_requested, pointer_position_in_node, should_dismiss_pointer_context_menu,
    spawn_pointer_context_menu, spawn_pointer_context_menu_item,
};
use crate::timeline::TimelineState;
use crate::*;
use aestra_compiler::{MaterialCompiler, MaterialPresetCategory};
#[cfg(test)]
use aestra_core::material::{MaterialProgram, MaterialProgramRef};
#[cfg(test)]
use aestra_core::{
    AssetDefinition, ChoreographyTrackId, EffectClip, EffectId, Emitter, MaterialId, Value,
};
use aestra_core::{EffectAsset, EffectAssetRef, EffectClipId, EmitterId, MaterialPresetId};
use aestra_project::{
    ProjectAssetIndexAvailability, ProjectEffectEntry, ProjectEffectRelation, ProjectEffectStatus,
    ProjectEffectUsageGraph,
};
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
#[cfg(test)]
use std::path::PathBuf;
use std::{collections::BTreeSet, fs, path::Path};

pub(crate) struct EditorLibraryPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LibrarySet {
    Actions,
    Sync,
}

impl Plugin for EditorLibraryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LibraryState>()
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
                    reset_library_for_project_root,
                    sync_library_filtering,
                    sync_material_preset_filtering,
                    restore_library_context_menu_focus,
                    queue_library_relation_overlay_rebuild,
                )
                    .chain()
                    .in_set(LibrarySet::Sync),
            );
    }
}

use crate::project_content::ProjectEffectWatchState;
#[cfg(test)]
use crate::project_content::apply_project_effect_catalog_refresh;
#[cfg(test)]
use crate::project_content::poll_project_effect_catalog;

fn reset_library_for_project_root(
    catalog: Res<ProjectEffectCatalog>,
    mut previous: Local<Option<std::path::PathBuf>>,
    mut library: ResMut<LibraryState>,
) {
    if previous.as_deref() != Some(catalog.root()) {
        *previous = Some(catalog.root().to_owned());
        *library = LibraryState::default();
    }
}

#[cfg(test)]
use aestra_project::ProjectTreeStamp as ProjectEffectTreeSnapshot;

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

    fn matches_material_preset(&self, preset: &aestra_compiler::MaterialPresetDescriptor) -> bool {
        if self.origin == LibraryOriginFilter::CurrentDocument
            || !matches!(
                self.kind,
                LibraryKindFilter::All | LibraryKindFilter::Material
            )
        {
            return false;
        }
        let query = self.query.trim().to_lowercase();
        query.is_empty()
            || preset.display_name.to_lowercase().contains(&query)
            || preset.description.to_lowercase().contains(&query)
            || preset.tags.iter().any(|tag| tag.contains(&query))
    }
}

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LibraryAction {
    RefreshProject,
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
        .cached_effect(relation.owner)
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

#[derive(Component)]
struct LibraryMaterialPresetsSection;

#[derive(Component)]
struct LibraryMaterialPresetCount;

#[derive(Component)]
struct LibraryMaterialPresetRow {
    preset: MaterialPresetId,
}

#[derive(Component)]
struct LibraryMaterialPresetCategoryHeader(MaterialPresetCategory);

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
    mut drag: On<Pointer<DragStart>>,
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
    drag.propagate(false);
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
    mut drag: On<Pointer<Drag>>,
    mut ghosts: Query<&mut Node, With<ProjectEffectDragGhost>>,
) {
    if ghosts.is_empty() {
        return;
    }
    drag.propagate(false);
    let position = drag.pointer_location.position + Vec2::new(14.0, 14.0);
    for mut node in &mut ghosts {
        node.left = Val::Px(position.x);
        node.top = Val::Px(position.y);
    }
}

fn end_project_effect_drag(
    mut drag: On<Pointer<DragEnd>>,
    ghosts: Query<Entity, With<ProjectEffectDragGhost>>,
    mut commands: Commands,
) {
    if ghosts.is_empty() {
        return;
    }
    drag.propagate(false);
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

fn sync_material_preset_filtering(
    mut commands: Commands,
    state: Res<LibraryState>,
    catalog: Res<ProjectEffectCatalog>,
    mut rows: Query<(Entity, &LibraryMaterialPresetRow, &mut Node)>,
    mut categories: Query<
        (&LibraryMaterialPresetCategoryHeader, &mut Node),
        (
            Without<LibraryMaterialPresetRow>,
            Without<LibraryMaterialPresetsSection>,
        ),
    >,
    mut sections: Query<
        &mut Node,
        (
            With<LibraryMaterialPresetsSection>,
            Without<LibraryMaterialPresetRow>,
            Without<LibraryMaterialPresetCategoryHeader>,
        ),
    >,
    mut counts: Query<&mut Text, With<LibraryMaterialPresetCount>>,
    localizer: Res<Localizer>,
) {
    if !state.is_changed() && !catalog.is_changed() {
        return;
    }
    let presets = catalog
        .material_preset_catalog()
        .unwrap_or_else(|_| MaterialCompiler.material_preset_catalog());
    let visible = presets
        .iter()
        .filter(|preset| state.matches_material_preset(preset))
        .map(|preset| preset.id)
        .collect::<BTreeSet<_>>();
    for (entity, row, mut node) in &mut rows {
        let shown = visible.contains(&row.preset);
        node.display = if shown { Display::Flex } else { Display::None };
        if shown {
            commands.entity(entity).insert(ListItem);
        } else {
            commands.entity(entity).remove::<ListItem>();
        }
    }
    for (header, mut node) in &mut categories {
        let shown = presets
            .iter()
            .any(|preset| preset.category == header.0 && visible.contains(&preset.id));
        node.display = if shown { Display::Flex } else { Display::None };
    }
    for mut section in &mut sections {
        section.display = if visible.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
    }
    let mut args = FluentArgs::new();
    args.set("count", visible.len());
    let text = localizer.text_with("assets-found", &args);
    for mut count in &mut counts {
        count.0.clone_from(&text);
    }
}

fn spawn_material_presets(
    panel: &mut ChildSpawnerCommands,
    catalog: &ProjectEffectCatalog,
    state: &LibraryState,
    localizer: &Localizer,
) {
    let presets = catalog
        .material_preset_catalog()
        .unwrap_or_else(|_| MaterialCompiler.material_preset_catalog());
    let visible_count = presets
        .iter()
        .filter(|preset| state.matches_material_preset(preset))
        .count();
    let mut args = FluentArgs::new();
    args.set("count", visible_count);
    let section = spawn_list_section_header(
        panel,
        &localizer.text("assets-material-presets"),
        &localizer.text_with("assets-found", &args),
    );
    panel
        .commands()
        .entity(section.root)
        .insert(LibraryMaterialPresetsSection);
    panel
        .commands()
        .entity(section.meta)
        .insert(LibraryMaterialPresetCount);

    let mut grouped = presets.iter().collect::<Vec<_>>();
    grouped.sort_by(|left, right| {
        (left.category, left.display_name.as_str())
            .cmp(&(right.category, right.display_name.as_str()))
    });
    let mut category = None;
    for preset in grouped {
        if category != Some(preset.category) {
            category = Some(preset.category);
            panel.spawn((
                LibraryMaterialPresetCategoryHeader(preset.category),
                Text::new(preset.category.display_name()),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(theme::ACCENT),
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(9.0), Val::Px(5.0)),
                    display: if presets.iter().any(|candidate| {
                        candidate.category == preset.category
                            && state.matches_material_preset(candidate)
                    }) {
                        Display::Flex
                    } else {
                        Display::None
                    },
                    ..default()
                },
            ));
        }
        let visible = state.matches_material_preset(preset);
        let row_entity = {
            let mut row = panel.spawn((
                LibraryMaterialPresetRow { preset: preset.id },
                Node {
                    width: Val::Percent(100.0),
                    min_height: Val::Px(62.0),
                    min_width: Val::Px(0.0),
                    padding: UiRect::axes(Val::Px(9.0), Val::Px(6.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(9.0),
                    display: if visible {
                        Display::Flex
                    } else {
                        Display::None
                    },
                    ..default()
                },
                BackgroundColor(theme::PANEL_DARK),
                AccessibleLabel(format!("{} material preset", preset.display_name)),
                EditorTooltip::titled(preset.display_name.clone(), preset.description.clone())
                    .with_footer("Material preset preview"),
            ));
            let row_entity = row.id();
            row.with_children(|row| {
                spawn_material_preset_preview(row, preset.id, 48.0);
                row.spawn((
                    Node {
                        min_width: Val::Px(0.0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(3.0),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|labels| {
                    labels.spawn((
                        Text::new(preset.display_name.clone()),
                        TextFont {
                            font_size: FontSize::Px(10.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_MUTED),
                        TextLayout::no_wrap(),
                        Pickable::IGNORE,
                    ));
                    labels.spawn((
                        Text::new(preset.description.clone()),
                        TextFont {
                            font_size: FontSize::Px(8.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                        TextLayout::no_wrap(),
                        Node {
                            width: Val::Percent(100.0),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                });
            });
            row_entity
        };
        if visible {
            panel.commands().entity(row_entity).insert(ListItem);
        }
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
            args.set("path", catalog.root().display().to_string());
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
                    panel.spawn((
                        Text::new(format!(
                            "{}\n{}",
                            localizer.text("project-active"),
                            crate::project::display_name(catalog.root())
                        )),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        Node {
                            margin: UiRect::all(Val::Px(8.0)),
                            ..default()
                        },
                        EditorTooltip::description(catalog.root().display().to_string()),
                    ));
                    library_toolbar_button(
                        panel,
                        &localizer.text("file-open-project"),
                        DocumentAction::OpenProject,
                    );
                    spawn_project_effects(panel, catalog, state, localizer);
                    library_toolbar_button(
                        panel,
                        &localizer.text("library-refresh"),
                        LibraryAction::RefreshProject,
                    );
                    spawn_material_presets(panel, catalog, state, localizer);
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
    catalog: Res<ProjectEffectCatalog>,
    mut commands: Commands,
    io_tasks: Option<Res<crate::project_content::io::ProjectIoTasks>>,
    mut watch: ResMut<ProjectEffectWatchState>,
    mut operation: ResMut<LibraryAssetOperationState>,
    timeline: Option<Res<TimelineState>>,
    localizer: Res<Localizer>,
) {
    if !crate::project_content::io::idle(io_tasks) {
        return;
    }
    match *action {
        LibraryAction::RefreshProject => watch.request_refresh(),
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
                .set_directory(catalog.effect_root())
                .pick_folder()
            else {
                return;
            };
            background::queue(
                &mut commands,
                background::SourceAction::Move {
                    source,
                    destination,
                    current: is_current,
                },
                &catalog,
                &session,
                &localizer,
            );
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
            match catalog.cached_effect_usage_graph(reference) {
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
            background::queue(
                &mut commands,
                background::SourceAction::InspectDeletion(source),
                &catalog,
                &session,
                &localizer,
            );
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
    catalog: Res<ProjectEffectCatalog>,
    io_tasks: Option<Res<crate::project_content::io::ProjectIoTasks>>,
    session: Res<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !crate::project_content::io::idle(io_tasks) {
        return;
    }
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
            background::queue(
                &mut commands,
                background::SourceAction::Delete(deletion),
                &catalog,
                &session,
                &localizer,
            );
            return;
        }
        LibraryAssetOperationAction::ConfirmReusableEffectExtraction => {
            let Some(extraction) = state.extraction.clone() else {
                return;
            };
            background::queue(
                &mut commands,
                background::SourceAction::Extract(extraction),
                &catalog,
                &session,
                &localizer,
            );
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
    background::queue(
        &mut commands,
        background::SourceAction::Rename {
            rename,
            current: is_current,
        },
        &catalog,
        &session,
        &localizer,
    );
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
    use bevy::{
        asset::AssetPlugin,
        camera::NormalizedRenderTarget,
        ecs::error::{FallbackErrorHandler, panic},
        picking::pointer::{Location, PointerId},
        scene::ScenePlugin,
        text::TextPlugin,
        window::WindowRef,
    };

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
            .add_plugins((EditorProjectContentPlugin, EditorLibraryPlugin));
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
        EffectAssetRef::new(aestra_core::EffectId::from_u128(value))
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
    fn project_catalog_resolves_bundled_effect_materials_outside_the_effect_folder() {
        let asset_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
        let effect_root = asset_root.join("effects");
        let catalog = ProjectEffectCatalog::scan_project(&asset_root, &effect_root);
        let reference =
            EffectAssetRef::new(EffectId::from_u128(0xa3574a00_0000_4000_8000_000000009001));

        let effect = catalog
            .load_effect(reference)
            .expect("the Material Graph Lab row should be openable");

        assert_eq!(effect.name, "Material Graph Lab");
        assert!(catalog.openable_path(reference).is_some());
        catalog
            .compile_project(&effect)
            .expect("its sibling material program should resolve from the project asset root");
    }

    #[test]
    fn project_catalog_merges_bundled_material_presets_with_builtins() {
        let asset_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
        let catalog = ProjectEffectCatalog::scan_project(&asset_root, asset_root.join("effects"));

        let presets = catalog.material_preset_catalog().unwrap();
        let hologram = presets
            .iter()
            .find(|preset| preset.display_name == "Hologram")
            .expect("the bundled project preset should be registered");

        assert_eq!(hologram.category.display_name(), "Shaping");
        assert!(hologram.tags.iter().any(|tag| tag == "hologram"));
        assert!(
            presets
                .get(aestra_compiler::MATERIAL_PRESET_DISSOLVE)
                .is_some()
        );
        for name in [
            "Additive Flame",
            "Soft Smoke",
            "Energy Beam",
            "Magic Shield",
            "Ghost",
            "Portal",
            "Impact Flash",
        ] {
            assert!(
                presets.iter().any(|preset| preset.display_name == name),
                "bundled preset {name} should be registered"
            );
        }
        let functions = catalog.material_function_library().unwrap();
        let pulse = functions
            .iter()
            .find(|function| function.name == "Pulse Wave (Custom WESL)")
            .expect("the bundled custom WESL function should be registered");
        assert!(pulse.custom_wesl.is_some());
    }

    #[test]
    fn library_material_preset_filter_matches_metadata_and_respects_scope() {
        let catalog = MaterialCompiler.material_preset_catalog();
        let preset = catalog
            .get(aestra_compiler::MATERIAL_PRESET_DISSOLVE)
            .expect("Dissolve preset should be registered");
        let mut state = LibraryState {
            query: "threshold".into(),
            ..default()
        };

        assert!(state.matches_material_preset(preset));
        state.query = "definitely absent".into();
        assert!(!state.matches_material_preset(preset));
        state.query.clear();
        state.kind = LibraryKindFilter::Texture;
        assert!(!state.matches_material_preset(preset));
        state.kind = LibraryKindFilter::Material;
        assert!(state.matches_material_preset(preset));
        state.origin = LibraryOriginFilter::CurrentDocument;
        assert!(!state.matches_material_preset(preset));
    }

    #[test]
    fn project_catalog_creates_effects_in_the_effect_authoring_folder() {
        let temporary = tempfile::tempdir().unwrap();
        let effect_root = temporary.path().join("effects");
        fs::create_dir(&effect_root).unwrap();
        fs::create_dir(temporary.path().join("materials")).unwrap();
        let mut catalog = ProjectEffectCatalog::scan_project(temporary.path(), &effect_root);
        let effect = EffectAsset::new("Created Effect", 1.0);

        let created = catalog.create_effect_source(&effect).unwrap();

        assert_eq!(created.path.parent(), Some(effect_root.as_path()));
        assert_eq!(catalog.effect_root(), effect_root);
    }

    #[test]
    fn project_catalog_watch_tracks_material_program_sources() {
        let temporary = tempfile::tempdir().unwrap();
        let material = temporary.path().join("test.aestra.material.ron");
        fs::write(&material, "material").unwrap();

        let snapshot = ProjectEffectTreeSnapshot::scan(temporary.path());

        assert!(snapshot.file(&material).is_some());
        for name in [
            "function.aestra.material-function.ron",
            "preset.aestra.material-preset.ron",
            "UPPER.AESTRA.MATERIAL.RON",
        ] {
            let path = temporary.path().join(name);
            fs::write(&path, "source").unwrap();
            assert!(
                ProjectEffectTreeSnapshot::scan(temporary.path())
                    .file(&path)
                    .is_some()
            );
        }
    }

    #[test]
    fn missing_material_dependencies_have_actionable_diagnostics() {
        let temporary = tempfile::tempdir().unwrap();
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let mut effect = test_support::effect_with_timing_slack();
        effect
            .material_instances
            .push(aestra_core::material::MaterialInstance {
                id: MaterialId::new(),
                program: MaterialProgramRef::Project(aestra_core::MaterialProgramId::new()),
                values: default(),
                render_state: aestra_core::material::MaterialRenderState::additive_sprite(),
            });
        let error = catalog.compile_project(&effect).unwrap_err();
        assert!(!error.trim().is_empty());
        assert!(!catalog.dependency_validation_report(&effect).is_valid());
    }

    #[test]
    fn clean_external_reload_resolves_project_materials() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("root.aestra.ron");
        let program = MaterialProgram::additive_sprite("Project material");
        program
            .save_ron(temporary.path().join("material.aestra.material.ron"))
            .unwrap();
        let mut effect = test_support::effect_with_timing_slack();
        effect
            .material_instances
            .push(aestra_core::material::MaterialInstance {
                id: MaterialId::new(),
                program: MaterialProgramRef::Project(program.id),
                values: default(),
                render_state: aestra_core::material::MaterialRenderState::additive_sprite(),
            });
        effect.save_ron(&path).unwrap();
        let mut catalog = ProjectEffectCatalog::scan(temporary.path());
        let mut session = test_support::session_with_timing_slack();
        session.open_compiled_effect(
            &path,
            effect.clone(),
            catalog.compile_project(&effect).unwrap().root,
        );
        let previous = ProjectEffectTreeSnapshot::scan(temporary.path());
        effect.name = "Externally changed material effect".into();
        effect.save_ron(&path).unwrap();
        let current = ProjectEffectTreeSnapshot::scan(temporary.path());
        apply_project_effect_catalog_refresh(
            &mut catalog,
            &mut session,
            &previous,
            &current,
            &Localizer::new("en-US").unwrap(),
        );
        assert_eq!(session.effect.name, effect.name);
        assert!(
            session
                .preview
                .as_ref()
                .unwrap()
                .effect()
                .material_program(program.id)
                .is_some()
        );
        assert!(!session.dirty);
    }

    #[test]
    fn project_effect_drag_end_cleanup_does_not_bubble_to_ancestors() {
        let mut app = App::new();
        app.insert_resource(FallbackErrorHandler(panic))
            .add_observer(end_project_effect_drag);
        let parent = app.world_mut().spawn_empty().id();
        let target = app.world_mut().spawn(ChildOf(parent)).id();
        let ghost = app.world_mut().spawn(ProjectEffectDragGhost).id();
        let window = app.world_mut().spawn(Window::default()).id();
        let event = Pointer::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::Window(
                    WindowRef::Entity(window).normalize(None).unwrap(),
                ),
                position: Vec2::ZERO,
            },
            DragEnd {
                button: PointerButton::Primary,
                distance: Vec2::ZERO,
            },
            target,
        );

        app.world_mut().trigger(event);
        app.update();

        assert!(app.world().get_entity(ghost).is_err());
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
                current: aestra_core::CURRENT_FORMAT_VERSION,
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
                    current: aestra_core::CURRENT_FORMAT_VERSION,
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
        assert!(unsupported_label.contains(&aestra_core::CURRENT_FORMAT_VERSION.to_string()));

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
            .add_plugins((EditorProjectContentPlugin, EditorLibraryPlugin));
        app
    }

    #[test]
    fn project_catalog_rejects_self_and_transitive_effect_cycles() {
        let temporary = tempfile::tempdir().unwrap();
        let mut owner = test_support::effect_with_timing_slack();
        owner.id = aestra_core::EffectId::from_u128(0xa11ce);
        owner.name = "Owner".into();
        owner.effect_clips.clear();
        let mut child = test_support::effect_with_timing_slack();
        child.id = aestra_core::EffectId::from_u128(0xc41d);
        child.name = "Child".into();
        child.effect_clips = vec![aestra_core::EffectClip::new(
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
        owner.id = aestra_core::EffectId::from_u128(0xa11ce);
        owner.effect_clips = vec![aestra_core::EffectClip::new(
            EffectAssetRef::new(aestra_core::EffectId::from_u128(0xdead)),
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
    fn library_panel_actions_use_the_independent_project_service() {
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
            .add_plugins((EditorProjectContentPlugin, EditorLibraryPlugin));

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
        assert_eq!(session.effect.name, "Editor Test Effect");
        crate::project_content::io::drain(app.world_mut());
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
            .add_plugins((EditorProjectContentPlugin, EditorLibraryPlugin));

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
        let parameter = aestra_core::ParameterId::new();
        child.parameters.push(aestra_core::EffectParameter {
            id: parameter,
            name: "Intensity".into(),
            default: Value::Scalar(1.0),
            exposed: true,
        });
        child.emitters[0]
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == aestra_core::MODULE_EMISSION)
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
        let mut clip = aestra_core::EffectClip::new(child_reference, 0.25, 1.0);
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
            .add_plugins((EditorProjectContentPlugin, EditorLibraryPlugin));

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
                .find(|module| module.module_type.0 == aestra_core::MODULE_EMISSION)
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
    fn project_effect_watch_requires_two_stable_observations() {
        let temporary = tempfile::tempdir().unwrap();
        let initial = ProjectEffectTreeSnapshot::scan(temporary.path());
        let version = aestra_project::ProjectContentVersion {
            generation: 1,
            revision: 0,
        };
        let mut watch = aestra_project::ProjectContentRefresh::new(version, initial);
        write_effect(&temporary.path().join("new.aestra.ron"), "New");
        let changed = ProjectEffectTreeSnapshot::scan(temporary.path());

        assert!(watch.observe(version, &changed).is_none());
        assert!(watch.observe(version, &changed).is_some());
        assert!(watch.observe(version, &changed).is_none());
    }

    #[test]
    fn project_switch_replaces_the_watch_baseline_without_reloading_the_document() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        write_effect(&second.path().join("new.aestra.ron"), "New project");
        let session = test_support::session_with_timing_slack();
        let original = session.effect.clone();
        let mut app = App::new();
        app.insert_resource(ProjectEffectCatalog::scan(first.path()))
            .init_resource::<ProjectEffectWatchState>()
            .insert_resource(session)
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_systems(Update, poll_project_effect_catalog);
        app.insert_resource(ProjectEffectCatalog::scan(second.path()));
        app.update();
        let watch = app.world().resource::<ProjectEffectWatchState>();
        assert_eq!(
            watch.version(),
            app.world()
                .resource::<ProjectEffectCatalog>()
                .content_revision()
        );
        assert_eq!(app.world().resource::<EditorSession>().effect, original);
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
        crate::project_content::io::drain(app.world_mut());
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

        crate::project_content::io::drain(app.world_mut());
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
            .add_plugins((EditorProjectContentPlugin, EditorLibraryPlugin));

        assert_eq!(
            app.world().resource::<ProjectEffectCatalog>().entries()[0].id,
            expected_id
        );
    }
}
