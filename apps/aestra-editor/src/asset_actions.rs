//! Shared source actions and dialogs, independent of the Library browsing panel.
mod background;
#[cfg(test)]
mod tests;
use crate::effect_authoring::{create_reusable_effect_from_emitters, explode_effect_clip};
use crate::project_content::ProjectEffectWatchState;
use crate::timeline::TimelineState;
use crate::*;
use aestra_core::{EffectClipId, EmitterId};
use bevy::ui_widgets::Activate;

pub(crate) struct EditorAssetActionsPlugin;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum AssetActionsSet {
    Actions,
}

impl Plugin for EditorAssetActionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AssetOperationState>()
            .add_observer(queue_asset_action_activation)
            .add_observer(execute_asset_action)
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
            );
    }
}

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssetAction {
    RefreshProject,
    CreateReusableEffectFromSelection,
    ExplodeEffectClip(EffectClipId),
}

#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AssetOperationState {
    extraction: Option<ReusableEffectExtractionState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReusableEffectExtractionState {
    emitters: Vec<EmitterId>,
    draft: String,
    replace_selection: bool,
    error: Option<String>,
}

impl AssetOperationState {
    pub(crate) fn is_open(&self) -> bool {
        self.extraction.is_some()
    }

    fn close_all(&mut self) {
        self.extraction = None;
    }
}

#[derive(Component)]
struct AssetOperationOverlay;

#[derive(Component)]
struct ReusableEffectExtractionDialog;

#[derive(Component)]
struct ReusableEffectExtractionInput;

#[derive(Component)]
struct ReusableEffectExtractionReplace;

#[derive(Component)]
struct ReusableEffectExtractionError;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum AssetOperationAction {
    ConfirmReusableEffectExtraction,
    Cancel,
}

pub(crate) fn spawn_asset_operation_overlay(
    parent: &mut ChildSpawnerCommands,
    state: &AssetOperationState,
    localizer: &Localizer,
) {
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
        });
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
        Query<&mut Node, With<ReusableEffectExtractionDialog>>,
        Query<(&mut Text, &mut Node), With<ReusableEffectExtractionError>>,
    )>,
    extraction_inputs: Query<Entity, With<ReusableEffectExtractionInput>>,
    mut editable_texts: Query<(&ChildOf, &mut EditableText)>,
) {
    if !state.is_changed() {
        return;
    }
    let display = if state.is_open() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut nodes.p0() {
        node.display = display;
    }
    for mut node in &mut nodes.p1() {
        node.display = display;
    }
    for (parent, mut text) in &mut editable_texts {
        if extraction_inputs.contains(parent.parent())
            && let Some(extraction) = state.extraction.as_ref()
            && text.value() != extraction.draft.as_str()
        {
            text.editor_mut().set_text(&extraction.draft);
            text.queue_edit(TextEdit::TextEnd(false));
        }
    }
    for (mut text, mut node) in &mut nodes.p2() {
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
        }
    }
}
