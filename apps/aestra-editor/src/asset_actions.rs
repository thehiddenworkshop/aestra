//! Shared source actions and dialogs, independent of the Library browsing panel.
mod background;
#[cfg(test)]
mod tests;
use crate::effect_authoring::{create_reusable_effect_from_emitters, explode_effect_clip};
use crate::project_content::ProjectEffectWatchState;
use crate::timeline::TimelineState;
use crate::*;
use aestra_core::{EffectAssetRef, EffectClipId, EmitterId};
use aestra_project::{ProjectEffectRelation, ProjectEffectUsageGraph};
use bevy::ui_widgets::Activate;
use std::{fs, path::Path};

pub(crate) struct EditorAssetActionsPlugin;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum AssetActionsSet {
    Actions,
    Sync,
}

impl Plugin for EditorAssetActionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AssetOperationState>()
            .init_resource::<RenderedAssetRelationOverlay>()
            .add_observer(queue_asset_action_activation)
            .add_observer(execute_asset_action)
            .add_observer(update_source_rename_draft)
            .add_observer(update_reusable_effect_extraction_draft)
            .add_observer(update_reusable_effect_extraction_replace)
            .add_observer(resolve_asset_operation)
            .add_systems(
                Update,
                (
                    dismiss_asset_operation_with_escape,
                    handle_asset_action_buttons,
                    sync_asset_operation_overlay,
                )
                    .chain()
                    .in_set(AssetActionsSet::Actions),
            )
            .add_systems(
                Update,
                queue_asset_relation_overlay_rebuild.in_set(AssetActionsSet::Sync),
            );
    }
}

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssetAction {
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
pub(crate) struct AssetOperationState {
    rename: Option<SourceRenameState>,
    extraction: Option<ReusableEffectExtractionState>,
    dependency_inspector: Option<DependencyInspectorState>,
    deletion: Option<EffectDeletionState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceRenameState {
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
struct DependencyInspectorState {
    source: ProjectEffectEntryId,
    graph: ProjectEffectUsageGraph,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EffectDeletionState {
    source: ProjectEffectEntryId,
    graph: ProjectEffectUsageGraph,
    error: Option<String>,
}

#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
struct RenderedAssetRelationOverlay(Option<AssetRelationOverlayView>);

#[derive(Debug, Clone, PartialEq, Eq)]
enum AssetRelationOverlayView {
    Inspector(DependencyInspectorState),
    Deletion(EffectDeletionState),
}

impl AssetOperationState {
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
struct AssetOperationOverlay;

#[derive(Component)]
struct SourceRenameDialog;

#[derive(Component)]
struct SourceRenameInput;

#[derive(Component)]
struct SourceRenameError;

#[derive(Component)]
struct ReusableEffectExtractionDialog;

#[derive(Component)]
struct ReusableEffectExtractionInput;

#[derive(Component)]
struct ReusableEffectExtractionReplace;

#[derive(Component)]
struct ReusableEffectExtractionError;

#[derive(Component)]
struct DependencyInspectorDialog;

#[derive(Component)]
struct EffectDeletionDialog;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum AssetOperationAction {
    ConfirmRename,
    ConfirmReusableEffectExtraction,
    NavigateToEffect {
        effect: EffectAssetRef,
        clip: Option<EffectClipId>,
    },
    ConfirmEffectDeletion,
    Cancel,
}

pub(crate) fn spawn_asset_operation_overlay(
    parent: &mut ChildSpawnerCommands,
    state: &AssetOperationState,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) {
    let rename = state.rename.as_ref();
    let extraction = state.extraction.as_ref();
    parent
        .spawn((
            AssetOperationOverlay,
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
                    SourceRenameDialog,
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
                        SourceRenameInput,
                    );
                    dialog.commands().entity(input).insert(Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(32.0),
                        ..default()
                    });
                    dialog.spawn((
                        SourceRenameError,
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
                                ("common-cancel", AssetOperationAction::Cancel),
                                (
                                    "library-rename-confirm",
                                    AssetOperationAction::ConfirmRename,
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
                                ("common-cancel", AssetOperationAction::Cancel),
                                (
                                    "library-extract-confirm",
                                    AssetOperationAction::ConfirmReusableEffectExtraction,
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
    inspector: Option<&DependencyInspectorState>,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) {
    let source_name = inspector
        .and_then(|inspector| catalog.entry(inspector.source))
        .map_or_else(String::new, |entry| entry.display_name.clone());
    overlay
        .spawn((
            DependencyInspectorDialog,
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
            spawn_asset_dialog_buttons(
                dialog,
                &[("common-close", AssetOperationAction::Cancel)],
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
                let action = AssetOperationAction::NavigateToEffect {
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
    deletion: Option<&EffectDeletionState>,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) {
    let source_name = deletion
        .and_then(|deletion| catalog.entry(deletion.source))
        .map_or_else(String::new, |entry| entry.display_name.clone());
    overlay
        .spawn((
            EffectDeletionDialog,
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
            spawn_asset_dialog_buttons(
                dialog,
                &[
                    ("common-cancel", AssetOperationAction::Cancel),
                    (
                        "library-delete-confirm",
                        AssetOperationAction::ConfirmEffectDeletion,
                    ),
                ],
                localizer,
            );
        });
}

fn spawn_asset_dialog_buttons(
    parent: &mut ChildSpawnerCommands,
    buttons: &[(&str, AssetOperationAction)],
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

fn update_source_rename_draft(
    change: On<ValueChange<String>>,
    inputs: Query<(), With<SourceRenameInput>>,
    mut state: ResMut<AssetOperationState>,
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
    mut state: ResMut<AssetOperationState>,
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
    mut state: ResMut<AssetOperationState>,
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

fn dismiss_asset_operation_with_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<AssetOperationState>,
) {
    if state.is_open() && keys.just_pressed(KeyCode::Escape) {
        state.close_all();
    }
}

fn sync_asset_operation_overlay(
    state: Res<AssetOperationState>,
    mut nodes: ParamSet<(
        Query<&mut Node, With<AssetOperationOverlay>>,
        Query<&mut Node, With<SourceRenameDialog>>,
        Query<&mut Node, With<ReusableEffectExtractionDialog>>,
        Query<&mut Node, With<DependencyInspectorDialog>>,
        Query<&mut Node, With<EffectDeletionDialog>>,
        Query<(&mut Text, &mut Node), With<SourceRenameError>>,
        Query<(&mut Text, &mut Node), With<ReusableEffectExtractionError>>,
    )>,
    rename_inputs: Query<Entity, With<SourceRenameInput>>,
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

fn queue_asset_relation_overlay_rebuild(
    state: Res<AssetOperationState>,
    mut rendered: ResMut<RenderedAssetRelationOverlay>,
    mut session: ResMut<EditorSession>,
) {
    let view = state
        .dependency_inspector
        .clone()
        .map(AssetRelationOverlayView::Inspector)
        .or_else(|| {
            state
                .deletion
                .clone()
                .map(AssetRelationOverlayView::Deletion)
        });
    if rendered.0 != view {
        rendered.0 = view;
        session.ui_revision += 1;
    }
}

fn queue_asset_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<AssetAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

#[allow(clippy::type_complexity)]
fn handle_asset_action_buttons(
    mut commands: Commands,
    mut interactions: Query<
        (
            Entity,
            &Interaction,
            &AssetAction,
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
                commands.trigger(*action);
            }
            _ => {}
        }
    }
}

fn execute_asset_action(
    action: On<AssetAction>,
    mut session: ResMut<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    mut commands: Commands,
    io_tasks: Option<Res<crate::project_content::io::ProjectIoTasks>>,
    mut watch: ResMut<ProjectEffectWatchState>,
    mut operation: ResMut<AssetOperationState>,
    timeline: Option<Res<TimelineState>>,
    localizer: Res<Localizer>,
) {
    if !crate::project_content::io::idle(io_tasks) {
        return;
    }
    match *action {
        AssetAction::RefreshProject => watch.request_refresh(),
        AssetAction::AddSpriteMaterial => session.add_sprite_material(),
        AssetAction::AddGridFlipbook => session.add_grid_flipbook(),
        AssetAction::RenameProjectEffect(source) => {
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
            operation.rename = Some(SourceRenameState {
                source,
                draft: entry.display_name.clone(),
                error: None,
            });
        }
        AssetAction::MoveProjectEffect(source) => {
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
        AssetAction::InspectProjectEffect(source) => {
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
                        Some(DependencyInspectorState { source, graph });
                    session.ui_revision += 1;
                }
                Err(error) => {
                    let mut args = FluentArgs::new();
                    args.set("message", error);
                    session.status = localizer.text_with("library-status-operation-failed", &args);
                }
            }
        }
        AssetAction::DeleteProjectEffect(source) => {
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
        AssetAction::CreateReusableEffectFromSelection => {
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
        AssetAction::ExplodeEffectClip(clip) => {
            if let Err(error) = explode_effect_clip(clip, &catalog, &mut session, &localizer) {
                session.status = error;
            }
        }
    }
}

fn resolve_asset_operation(
    activate: On<Activate>,
    actions: Query<&AssetOperationAction>,
    mut commands: Commands,
    mut state: ResMut<AssetOperationState>,
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
        AssetOperationAction::Cancel => {
            state.close_all();
            return;
        }
        AssetOperationAction::NavigateToEffect { effect, clip } => {
            state.close_all();
            commands.trigger(clip.map_or(DocumentAction::OpenCatalog(effect), |clip| {
                DocumentAction::OpenCatalogClip(effect, clip)
            }));
            return;
        }
        AssetOperationAction::ConfirmEffectDeletion => {
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
        AssetOperationAction::ConfirmReusableEffectExtraction => {
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
        AssetOperationAction::ConfirmRename => {}
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
